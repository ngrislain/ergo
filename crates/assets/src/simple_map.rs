//! Small, bounded OSM basemap rasterizer. Commercial POIs are excluded by source
//! classification before labels/collision placement, not erased from raster text.
use anyhow::{Context, Result, ensure};
use font8x8::UnicodeFonts;
use prost::Message;
use std::io::Read;
use tiny_skia::{FillRule, Paint, PathBuilder, Pixmap, Stroke, Transform};

pub(crate) const STYLE: &str = "ergo-basic-osm-v1";
pub(crate) const URL: &str = "https://tiles.openfreemap.org/planet/latest/{z}/{x}/{y}.pbf";
pub(crate) const MAX_ZOOM: u32 = 14;
const MAX_DECODED: usize = 8 * 1024 * 1024;
const MAX_POINTS: usize = 300_000;

// Mapbox Vector Tile v2 wire fields, as used by the OpenMapTiles schema.
#[derive(Clone, PartialEq, Message)]
struct Tile {
    #[prost(message, repeated, tag = "3")]
    layers: Vec<Layer>,
}
#[derive(Clone, PartialEq, Message)]
struct Layer {
    #[prost(string, tag = "1")]
    name: String,
    #[prost(message, repeated, tag = "2")]
    features: Vec<Feature>,
    #[prost(string, repeated, tag = "3")]
    keys: Vec<String>,
    #[prost(message, repeated, tag = "4")]
    values: Vec<Value>,
    #[prost(uint32, optional, tag = "5")]
    extent: Option<u32>,
    #[prost(uint32, tag = "15")]
    version: u32,
}
#[derive(Clone, PartialEq, Message)]
struct Feature {
    #[prost(uint64, optional, tag = "1")]
    id: Option<u64>,
    #[prost(uint32, repeated, packed = "true", tag = "2")]
    tags: Vec<u32>,
    #[prost(uint32, tag = "3")]
    kind: u32,
    #[prost(uint32, repeated, packed = "true", tag = "4")]
    geometry: Vec<u32>,
}
#[derive(Clone, PartialEq, Message)]
struct Value {
    #[prost(string, optional, tag = "1")]
    string: Option<String>,
    #[prost(float, optional, tag = "2")]
    float: Option<f32>,
    #[prost(double, optional, tag = "3")]
    double: Option<f64>,
    #[prost(int64, optional, tag = "4")]
    int: Option<i64>,
    #[prost(uint64, optional, tag = "5")]
    uint: Option<u64>,
    #[prost(sint64, optional, tag = "6")]
    sint: Option<i64>,
    #[prost(bool, optional, tag = "7")]
    boolean: Option<bool>,
}
fn property<'a>(layer: &'a Layer, feature: &Feature, name: &str) -> Option<&'a Value> {
    feature.tags.chunks_exact(2).find_map(|pair| {
        (layer.keys.get(pair[0] as usize)?.as_str() == name)
            .then(|| layer.values.get(pair[1] as usize))?
    })
}
fn string<'a>(layer: &'a Layer, feature: &Feature, name: &str) -> &'a str {
    property(layer, feature, name)
        .and_then(|v| v.string.as_deref())
        .unwrap_or("")
}
fn rank(layer: &Layer, feature: &Feature) -> f64 {
    property(layer, feature, "rank")
        .and_then(|v| {
            v.double
                .or(v.float.map(f64::from))
                .or(v.int.map(|n| n as f64))
                .or(v.uint.map(|n| n as f64))
        })
        .unwrap_or(1.)
}
fn glyph(c: char) -> Option<[u8; 8]> {
    font8x8::BASIC_FONTS
        .get(c)
        .or_else(|| font8x8::LATIN_FONTS.get(c))
        .or_else(|| font8x8::GREEK_FONTS.get(c))
}
fn name(layer: &Layer, feature: &Feature) -> Option<String> {
    ["name:latin", "name", "name:en", "name_en"]
        .into_iter()
        .find_map(|key| {
            let text = string(layer, feature, key)
                .replace(['’', '‘'], "'")
                .replace(['–', '—', '‑'], "-")
                .replace('\u{a0}', " ");
            (!text.is_empty()
                && text.chars().count() <= 64
                && text.chars().all(|c| glyph(c).is_some()))
            .then(|| text.to_owned())
        })
}

/// Explicit non-commercial landmark classes. Unknown POI classes fail closed;
/// a restaurant called "Eiffel Tower" is still a restaurant, not a landmark.
fn landmark(class: &str, subclass: &str) -> bool {
    matches!(
        class,
        "museum" | "castle" | "monument" | "memorial" | "archaeological_site" | "place_of_worship"
    ) || class == "attraction"
        && matches!(
            subclass,
            "attraction"
                | "monument"
                | "memorial"
                | "castle"
                | "archaeological_site"
                | "tower"
                | "cathedral"
                | "church"
        )
}

#[derive(Clone, Copy)]
struct Window {
    scale: f32,
    offset: [f32; 2],
}
impl Window {
    fn new(zoom: u32, x: u32, y: u32, extent: u32) -> Result<Self> {
        ensure!(
            zoom <= 18 && (256..=65536).contains(&extent),
            "Unsupported map coordinate extent"
        );
        let factor = 1 << zoom.saturating_sub(MAX_ZOOM);
        Ok(Self {
            scale: 256. * factor as f32 / extent as f32,
            offset: [(x % factor) as f32 * 256., (y % factor) as f32 * 256.],
        })
    }
    fn point(&self, p: [i64; 2]) -> [f32; 2] {
        [
            p[0] as f32 * self.scale - self.offset[0],
            p[1] as f32 * self.scale - self.offset[1],
        ]
    }
}
fn geometry(feature: &Feature, window: Window, budget: &mut usize) -> Result<Vec<Vec<[f32; 2]>>> {
    let mut input = feature.geometry.iter();
    let mut position = [0i64; 2];
    let mut paths: Vec<Vec<[f32; 2]>> = Vec::new();
    while let Some(&command) = input.next() {
        let count = (command >> 3) as usize;
        ensure!(
            count > 0 && count <= MAX_POINTS,
            "Invalid map geometry command"
        );
        match command & 7 {
            1 | 2 => {
                ensure!(count <= *budget, "Map geometry point budget exceeded");
                *budget -= count;
                for _ in 0..count {
                    for value in &mut position {
                        let &encoded = input.next().context("Truncated map coordinate")?;
                        let delta = i64::from(encoded >> 1) ^ -i64::from(encoded & 1);
                        *value = value
                            .checked_add(delta)
                            .context("Map coordinate overflow")?;
                    }
                    if command & 7 == 1 {
                        paths.push(Vec::new());
                    }
                    paths
                        .last_mut()
                        .context("Map line has no origin")?
                        .push(window.point(position));
                }
            }
            7 => {
                ensure!(count == 1, "Invalid polygon closure");
                ensure!(*budget > 0, "Map geometry point budget exceeded");
                *budget -= 1;
                let path = paths.last_mut().context("Map polygon has no origin")?;
                path.push(*path.first().context("Empty map ring")?);
            }
            _ => anyhow::bail!("Unsupported map geometry command"),
        }
    }
    Ok(paths)
}
fn path(paths: &[Vec<[f32; 2]>], closed: bool) -> Option<tiny_skia::Path> {
    let mut builder = PathBuilder::new();
    for points in paths {
        if let Some(first) = points.first() {
            builder.move_to(first[0], first[1]);
            for p in &points[1..] {
                builder.line_to(p[0], p[1]);
            }
            if closed {
                builder.close();
            }
        }
    }
    builder.finish()
}
fn paint(code: u32) -> Paint<'static> {
    let rgb = ergo_world::color_rgb(code);
    let mut paint = Paint::default();
    paint.set_color_rgba8((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8, 255);
    paint.anti_alias = false;
    paint
}
fn terrain(layer: &str, class: &str, subclass: &str) -> Option<u32> {
    match layer {
        "water" => Some(1),
        "park" => Some(2),
        "landcover" | "landuse" => [class, subclass].into_iter().find_map(|value| match value {
            "wood" | "forest" => Some(288),
            "grass" | "grassland" | "scrub" | "park" | "garden" | "recreation_ground"
            | "wetland" => Some(2),
            "sand" | "beach" | "farmland" | "crop" => Some(19),
            "rock" | "bare_rock" | "scree" => Some(71),
            "ice" | "glacier" | "snow" => Some(15),
            _ => None,
        }),
        _ => None,
    }
}
fn road_width(class: &str, zoom: u32) -> Option<f32> {
    let base = match class {
        "motorway" | "trunk" => 2.8,
        "primary" | "secondary" => 1.8,
        "tertiary" => 1.2,
        "minor" | "residential" | "unclassified" | "service" | "street" => 0.8,
        "path" | "track" | "pedestrian" | "cycleway" => 0.25,
        _ => return None,
    };
    Some((base * 2f32.powi(zoom as i32 - 14)).max(0.7))
}
#[derive(Clone)]
struct Label {
    priority: u8,
    text: String,
    centre: [f32; 2],
    angle: f32,
}
impl Label {
    fn bounds(&self) -> [f32; 4] {
        let w = self.text.chars().count() as f32 * 4.;
        let x = self.angle.cos().abs() * w + self.angle.sin().abs() * 4. + 3.;
        let y = self.angle.sin().abs() * w + self.angle.cos().abs() * 4. + 3.;
        [
            self.centre[0] - x,
            self.centre[1] - y,
            self.centre[0] + x,
            self.centre[1] + y,
        ]
    }
    fn pixels(&self) -> Vec<(i32, i32)> {
        let width = self.text.chars().count() as f32 * 8.;
        let (sin, cos) = self.angle.sin_cos();
        let mut pixels = Vec::new();
        for (index, c) in self.text.chars().enumerate() {
            if let Some(glyph) = glyph(c) {
                for (row, bits) in glyph.into_iter().enumerate() {
                    for column in 0..8 {
                        if bits & (1 << column) != 0 {
                            let x = (index * 8 + column) as f32 - width / 2.;
                            let y = row as f32 - 4.;
                            pixels.push((
                                (self.centre[0] + x * cos - y * sin).round() as i32,
                                (self.centre[1] + x * sin + y * cos).round() as i32,
                            ));
                        }
                    }
                }
            }
        }
        pixels
    }
}
fn label(layer: &Layer, feature: &Feature, paths: &[Vec<[f32; 2]>], zoom: u32) -> Option<Label> {
    let class = string(layer, feature, "class");
    let priority = match layer.name.as_str() {
        "poi"
            if zoom >= 15
                && rank(layer, feature) <= 3.
                && landmark(class, string(layer, feature, "subclass")) =>
        {
            0
        }
        "mountain_peak" if rank(layer, feature) <= 4. => 1,
        "water_name" => 2,
        "park" if zoom >= 12 => 3,
        "place"
            if matches!(
                class,
                "city" | "town" | "village" | "hamlet" | "suburb" | "quarter" | "neighbourhood"
            ) =>
        {
            4
        }
        "transportation_name" if zoom >= 13 && road_width(class, zoom).is_some() => 5,
        _ => return None,
    };
    let text = name(layer, feature)?;
    let mut angle = 0.;
    let centre = if feature.kind == 2 {
        let segment = paths.iter().flat_map(|p| p.windows(2)).max_by(|a, b| {
            let length = |s: &[[f32; 2]]| (s[1][0] - s[0][0]).hypot(s[1][1] - s[0][1]);
            length(a).total_cmp(&length(b))
        })?;
        let dx = segment[1][0] - segment[0][0];
        let dy = segment[1][1] - segment[0][1];
        if dx.hypot(dy) < text.chars().count() as f32 * 8. + 12. {
            return None;
        }
        angle = dy.atan2(dx);
        if angle.abs() > std::f32::consts::FRAC_PI_2 {
            angle += if angle > 0. {
                -std::f32::consts::PI
            } else {
                std::f32::consts::PI
            };
        }
        [
            (segment[0][0] + segment[1][0]) / 2.,
            (segment[0][1] + segment[1][1]) / 2.,
        ]
    } else {
        let points = paths.first()?;
        let count = points.len() as f32;
        [
            points.iter().map(|p| p[0]).sum::<f32>() / count,
            points.iter().map(|p| p[1]).sum::<f32>() / count,
        ]
    };
    Some(Label {
        priority,
        text,
        centre,
        angle,
    })
}
fn set_pixel(pixmap: &mut Pixmap, x: i32, y: i32, code: u32) {
    if (0..256).contains(&x) && (0..256).contains(&y) {
        let rgb = ergo_world::color_rgb(code);
        let offset = ((y * 256 + x) * 4) as usize;
        pixmap.data_mut()[offset..offset + 4].copy_from_slice(&[
            (rgb >> 16) as u8,
            (rgb >> 8) as u8,
            rgb as u8,
            255,
        ]);
    }
}

pub(crate) fn render(bytes: &[u8], zoom: u32, x: u32, y: u32) -> Result<Vec<u8>> {
    ensure!(
        bytes.len() <= MAX_DECODED,
        "Vector tile exceeds input budget"
    );
    let decoded;
    let bytes = if bytes.starts_with(&[0x1f, 0x8b]) {
        let mut output = Vec::new();
        flate2::read::GzDecoder::new(bytes)
            .take((MAX_DECODED + 1) as u64)
            .read_to_end(&mut output)?;
        ensure!(
            output.len() <= MAX_DECODED,
            "Vector tile expands beyond budget"
        );
        decoded = output;
        decoded.as_slice()
    } else {
        bytes
    };
    let tile = Tile::decode(bytes).context("Invalid vector map tile")?;
    ensure!(
        tile.layers.len() <= 64
            && tile.layers.iter().map(|l| l.features.len()).sum::<usize>() <= 50_000,
        "Vector tile feature budget exceeded"
    );
    let mut pixmap = Pixmap::new(256, 256).context("Map raster allocation failed")?;
    let rgb = ergo_world::color_rgb(19);
    pixmap.fill(tiny_skia::Color::from_rgba8(
        (rgb >> 16) as u8,
        (rgb >> 8) as u8,
        rgb as u8,
        255,
    ));
    let mut labels = Vec::new();
    let mut budget = MAX_POINTS;
    for name in [
        "landcover",
        "landuse",
        "park",
        "water",
        "waterway",
        "transportation",
        "mountain_peak",
        "water_name",
        "place",
        "transportation_name",
        "poi",
    ] {
        for layer in tile.layers.iter().filter(|layer| layer.name == name) {
            ensure!(
                (1..=2).contains(&layer.version),
                "Unsupported vector tile layer version"
            );
            let window = Window::new(zoom, x, y, layer.extent.unwrap_or(4096))?;
            for feature in &layer.features {
                ensure!(
                    feature.tags.len() % 2 == 0,
                    "Invalid vector tile properties"
                );
                let class = string(layer, feature, "class");
                let subclass = string(layer, feature, "subclass");
                if name == "poi" && (!landmark(class, subclass) || rank(layer, feature) > 3.) {
                    continue;
                }
                let paths = geometry(feature, window, &mut budget)?;
                if feature.kind == 3 {
                    if let Some(color) = terrain(name, class, subclass)
                        && let Some(path) = path(&paths, true)
                    {
                        pixmap.fill_path(
                            &path,
                            &paint(color),
                            FillRule::Winding,
                            Transform::identity(),
                            None,
                        );
                    }
                } else if feature.kind == 2
                    && let Some(path) = path(&paths, false)
                {
                    if name == "waterway" {
                        pixmap.stroke_path(
                            &path,
                            &paint(1),
                            &Stroke {
                                width: road_width("minor", zoom).unwrap_or(1.),
                                ..Default::default()
                            },
                            Transform::identity(),
                            None,
                        );
                    } else if name == "transportation"
                        && let Some(width) = road_width(class, zoom)
                    {
                        for (color, width) in [(71, width + 1.5), (15, width)] {
                            pixmap.stroke_path(
                                &path,
                                &paint(color),
                                &Stroke {
                                    width,
                                    ..Default::default()
                                },
                                Transform::identity(),
                                None,
                            );
                        }
                    }
                }
                if let Some(label) = label(layer, feature, &paths, zoom) {
                    labels.push(label);
                }
            }
        }
    }
    labels.sort_by(|a, b| {
        a.priority
            .cmp(&b.priority)
            .then(a.text.cmp(&b.text))
            .then(a.centre[0].total_cmp(&b.centre[0]))
            .then(a.centre[1].total_cmp(&b.centre[1]))
    });
    let mut occupied: Vec<[f32; 4]> = Vec::new();
    for label in labels {
        let b = label.bounds();
        if b[2] < -512. || b[0] > 768. || b[3] < -512. || b[1] > 768. {
            continue;
        }
        if occupied
            .iter()
            .any(|a| a[0] < b[2] && a[2] > b[0] && a[1] < b[3] && a[3] > b[1])
        {
            continue;
        }
        if occupied.len() >= 512 {
            break;
        }
        occupied.push(b);
        let pixels = label.pixels();
        for &(x, y) in &pixels {
            for dx in -1..=1 {
                for dy in -1..=1 {
                    set_pixel(&mut pixmap, x + dx, y + dy, 15);
                }
            }
        }
        for (x, y) in pixels {
            set_pixel(&mut pixmap, x, y, 72);
        }
    }
    Ok(pixmap.encode_png()?)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    pub(crate) fn natural_fixture() -> Vec<u8> {
        Tile {
            layers: vec![Layer {
                name: "water".into(),
                features: vec![Feature {
                    kind: 3,
                    // A water strip spanning every child tile in the western
                    // half of its source parent, with boundaries on the grid.
                    geometry: vec![9, 0, 0, 26, 4096, 0, 0, 8192, 4095, 0, 15],
                    ..Default::default()
                }],
                extent: Some(4096),
                version: 2,
                ..Default::default()
            }],
        }
        .encode_to_vec()
    }
    fn point(class: &str, subclass: &str, title: &str) -> Layer {
        Layer {
            name: "poi".into(),
            keys: vec!["class".into(), "subclass".into(), "name".into()],
            values: [class, subclass, title]
                .into_iter()
                .map(|s| Value {
                    string: Some(s.into()),
                    ..Default::default()
                })
                .collect(),
            features: vec![Feature {
                tags: vec![0, 0, 1, 1, 2, 2],
                kind: 1,
                geometry: vec![9, 3072, 3072],
                ..Default::default()
            }],
            extent: Some(4096),
            version: 2,
        }
    }
    #[test]
    fn commercial_pois_never_change_the_map_even_with_a_landmark_name() {
        let empty = render(&Tile { layers: vec![] }.encode_to_vec(), 16, 1, 1).unwrap();
        for class in [
            "shop",
            "restaurant",
            "cafe",
            "bar",
            "pub",
            "hotel",
            "office",
            "supermarket",
            "bank",
        ] {
            let tile = Tile {
                layers: vec![point(class, class, "Eiffel Tower")],
            };
            assert_eq!(
                render(&tile.encode_to_vec(), 16, 1, 1).unwrap(),
                empty,
                "{class}"
            );
        }
        let tile = Tile {
            layers: vec![point("museum", "museum", "Musee")],
        };
        assert_ne!(render(&tile.encode_to_vec(), 16, 1, 1).unwrap(), empty);
    }
    #[test]
    fn landmarks_and_natural_features_are_explicitly_selected() {
        assert!(landmark("attraction", "monument"));
        assert!(landmark("place_of_worship", "christian"));
        assert!(!landmark("shop", "museum"));
        assert!(!landmark("attraction", "theme_park"));
        assert_eq!(terrain("landcover", "wood", ""), Some(288));
        assert_eq!(terrain("landcover", "snow", ""), Some(15));
        assert_eq!(terrain("water", "lake", ""), Some(1));
        assert_eq!(terrain("landuse", "commercial", ""), None);
        assert!(road_width("minor", 18).unwrap() > road_width("minor", 14).unwrap());
    }
    #[test]
    fn overzoom_and_malformed_geometry_are_bounded() {
        let a = Window::new(18, 100, 100, 4096).unwrap();
        let b = Window::new(18, 101, 100, 4096).unwrap();
        assert_eq!(a.point([1024, 1024])[0] - b.point([1024, 1024])[0], 256.);
        assert!(Window::new(19, 0, 0, 4096).is_err());
        let mut budget = MAX_POINTS;
        assert!(
            geometry(
                &Feature {
                    geometry: vec![9, 0],
                    ..Default::default()
                },
                a,
                &mut budget
            )
            .is_err()
        );
        assert!(render(&vec![0; MAX_DECODED + 1], 18, 0, 0).is_err());
    }
    #[test]
    fn repeated_polygon_closures_cannot_bypass_the_point_budget() {
        let window = Window::new(14, 0, 0, 4096).unwrap();
        let feature = Feature {
            geometry: vec![9, 0, 0, 15, 15],
            ..Default::default()
        };
        let mut budget = 2;
        assert!(geometry(&feature, window, &mut budget).is_err());
        assert_eq!(budget, 0);
    }
}
