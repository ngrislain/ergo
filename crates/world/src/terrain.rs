//! Lazy horizontal ground cells and palette projection. Native kernel positions
//! remain Y-up; serialized block coordinates and all public APIs are Z-up.
use crate::{BlockCoord, Brick, SCHEMA_VERSION, catalog::COLORS, color_rgb};
use serde::{Deserialize, Serialize};

/// Boundary-inclusive field: one sample every 16 studs, evaluated per stud.
pub const SAMPLE_GRID_SIZE: usize = 65;
pub const GROUND_TOP: f64 = -0.01;
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GroundSurface {
    pub format: String,
    /// Row-major sRGB; columns run east, rows run north, including cell edges.
    pub rgb: Vec<u32>,
}
impl GroundSurface {
    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.format == "ergo-ground-surface-1"
                && self.rgb.len() == SAMPLE_GRID_SIZE * SAMPLE_GRID_SIZE
                && self.rgb.iter().all(|rgb| *rgb <= 0xffffff),
            "Invalid ground colour surface"
        );
        Ok(())
    }
    /// Smooth categorical weights round region corners while the winning
    /// material remains one exact standard colour. Studs evaluate at centres.
    pub fn rgb_at(&self, east: f64, north: f64) -> u32 {
        let scale = (SAMPLE_GRID_SIZE - 1) as f64 / crate::BLOCK_SIZE;
        let x = east.clamp(0., crate::BLOCK_SIZE) * scale;
        let y = north.clamp(0., crate::BLOCK_SIZE) * scale;
        let x0 = (x.floor() as usize).min(SAMPLE_GRID_SIZE - 2);
        let y0 = (y.floor() as usize).min(SAMPLE_GRID_SIZE - 2);
        let u = x - x0 as f64;
        let v = y - y0 as f64;
        let colors = [
            self.rgb[y0 * SAMPLE_GRID_SIZE + x0],
            self.rgb[y0 * SAMPLE_GRID_SIZE + x0 + 1],
            self.rgb[(y0 + 1) * SAMPLE_GRID_SIZE + x0],
            self.rgb[(y0 + 1) * SAMPLE_GRID_SIZE + x0 + 1],
        ]
        .map(standard_ground_rgb);
        let weights = [(1. - u) * (1. - v), u * (1. - v), (1. - u) * v, u * v];
        let mut best = (0., u32::MAX);
        for color in colors {
            let weight = colors
                .iter()
                .zip(weights)
                .filter(|(c, _)| **c == color)
                .map(|(_, w)| w)
                .sum::<f64>();
            if weight > best.0 || (weight == best.0 && color < best.1) {
                best = (weight, color);
            }
        }
        best.1
    }
}

/// sRGB-channel interpolation, matching the renderer's RGB8 linear sampler.
pub fn blend_rgb(rgb: [u32; 4], x: f64, y: f64) -> u32 {
    [16, 8, 0].into_iter().fold(0, |out, shift| {
        let c = rgb.map(|v| ((v >> shift) & 255) as f64);
        let a = c[0] * (1. - x) + c[1] * x;
        let b = c[2] * (1. - x) + c[3] * x;
        out | (((a * (1. - y) + b * y).round().clamp(0., 255.) as u32) << shift)
    })
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(try_from = "GroundWire")]
pub struct GroundBlock {
    pub schema: u32,
    pub cell_size: u32,
    pub coord: BlockCoord,
    pub bricks: Vec<Brick>,
    /// Representative palette colour, also used for legacy/fallback records.
    pub color: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub surface: Option<GroundSurface>,
    pub sample_rgb: Option<u32>,
    pub source: String,
    pub attribution: String,
    pub attribution_url: Option<String>,
    pub generated_at: u64,
    pub source_tiles: Vec<String>,
    pub fallback_reason: Option<String>,
}
impl GroundBlock {
    pub fn fallback(x: i64, y: i64, reason: String) -> Self {
        Self {
            schema: SCHEMA_VERSION,
            cell_size: crate::BLOCK_SIZE as u32,
            coord: BlockCoord::from([x, y, 0]),
            bricks: Vec::new(),
            color: 71,
            surface: None,
            sample_rgb: None,
            source: "palette fallback".into(),
            attribution: String::new(),
            attribution_url: None,
            generated_at: 0,
            source_tiles: Vec::new(),
            fallback_reason: Some(reason),
        }
    }
    pub fn validate(&self) -> anyhow::Result<()> {
        if let Some(surface) = &self.surface {
            surface.validate()?;
        }
        anyhow::ensure!(
            self.schema == SCHEMA_VERSION
                && self.cell_size == crate::BLOCK_SIZE as u32
                && self.coord.1 == 0
                && self.bricks.is_empty()
                && COLORS.iter().any(|(code, _)| *code == self.color)
                && self.sample_rgb.is_none_or(|c| c <= 0xffffff)
                && self
                    .attribution_url
                    .as_ref()
                    .is_none_or(|s| s.starts_with("https://") || s.starts_with("http://")),
            "Invalid generated ground block"
        );
        Ok(())
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GroundWire {
    schema: u32,
    cell_size: u32,
    coord: BlockCoord,
    bricks: Vec<Brick>,
    color: u32,
    #[serde(default)]
    surface: Option<GroundSurface>,
    #[serde(default)]
    sample_rgb: Option<u32>,
    source: String,
    attribution: String,
    attribution_url: Option<String>,
    generated_at: u64,
    source_tiles: Vec<String>,
    fallback_reason: Option<String>,
}
impl TryFrom<GroundWire> for GroundBlock {
    type Error = String;
    fn try_from(w: GroundWire) -> Result<Self, String> {
        let block = Self {
            schema: w.schema,
            cell_size: w.cell_size,
            coord: w.coord,
            bricks: w.bricks,
            color: w.color,
            surface: w.surface,
            sample_rgb: w.sample_rgb,
            source: w.source,
            attribution: w.attribution,
            attribution_url: w.attribution_url,
            generated_at: w.generated_at,
            source_tiles: w.source_tiles,
            fallback_reason: w.fallback_reason,
        };
        block.validate().map_err(|e| e.to_string())?;
        Ok(block)
    }
}
fn linear(c: f64) -> f64 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}
pub fn average_rgb(samples: impl IntoIterator<Item = u32>) -> u32 {
    let mut sum = [0.0; 3];
    let mut count = 0;
    for rgb in samples {
        for (channel, shift) in [16, 8, 0].into_iter().enumerate() {
            sum[channel] += linear(((rgb >> shift) & 255) as f64 / 255.0);
        }
        count += 1;
    }
    if count == 0 {
        return color_rgb(71);
    }
    let encode = |v: f64| {
        let v = v / count as f64;
        let s = if v <= 0.0031308 {
            12.92 * v
        } else {
            1.055 * v.powf(1.0 / 2.4) - 0.055
        };
        (s * 255.0).round().clamp(0.0, 255.0) as u32
    };
    (encode(sum[0]) << 16) | (encode(sum[1]) << 8) | encode(sum[2])
}
pub fn lab(rgb: u32) -> [f64; 3] {
    let r = linear(((rgb >> 16) & 255) as f64 / 255.0);
    let g = linear(((rgb >> 8) & 255) as f64 / 255.0);
    let b = linear((rgb & 255) as f64 / 255.0);
    let f = |t: f64| {
        if t > 216.0 / 24389.0 {
            t.cbrt()
        } else {
            (24389.0 / 27.0 * t + 16.0) / 116.0
        }
    };
    let x = f((0.4124564 * r + 0.3575761 * g + 0.1804375 * b) / 0.95047);
    let y = f(0.2126729 * r + 0.7151522 * g + 0.0721750 * b);
    let z = f((0.0193339 * r + 0.1191920 * g + 0.9503041 * b) / 1.08883);
    [116.0 * y - 16.0, 500.0 * (x - y), 200.0 * (y - z)]
}
pub fn nearest_color(rgb: u32) -> u32 {
    let sample = lab(rgb);
    COLORS
        .iter()
        .min_by(|(a, _), (b, _)| {
            let distance = |code: u32| {
                let p = lab(color_rgb(code));
                (0..3).map(|i| (p[i] - sample[i]).powi(2)).sum::<f64>()
            };
            distance(*a).total_cmp(&distance(*b))
        })
        .unwrap()
        .0
}

pub const TERRAIN_COLORS: &[u32] = &[1, 2, 288, 71, 72, 14, 15, 70, 19, 28];
/// Re-project legacy blended caches at display time as well as new samples.
pub fn standard_ground_rgb(rgb: u32) -> u32 {
    static PALETTE: std::sync::LazyLock<Vec<u32>> =
        std::sync::LazyLock::new(|| TERRAIN_COLORS.iter().map(|code| color_rgb(*code)).collect());
    if PALETTE.contains(&rgb) {
        rgb
    } else {
        color_rgb(nearest_terrain_color(rgb))
    }
}
pub fn nearest_terrain_color(rgb: u32) -> u32 {
    let sample = lab(rgb);
    *TERRAIN_COLORS
        .iter()
        .min_by(|a, b| {
            let distance = |code: u32| {
                let p = lab(color_rgb(code));
                (0..3).map(|i| (p[i] - sample[i]).powi(2)).sum::<f64>()
            };
            distance(**a).total_cmp(&distance(**b))
        })
        .unwrap()
}
/// OSM Carto raster colours, classified before averaging so water is not turned
/// grey by nearby labels/coastlines. This is map-colour sampling, not a claimed
/// global vector land-cover dataset.
pub fn map_terrain_color(rgb: u32) -> u32 {
    let r = ((rgb >> 16) & 255) as i32;
    let g = ((rgb >> 8) & 255) as i32;
    let b = (rgb & 255) as i32;
    if b - r >= 25 && g - r >= 12 {
        return 1;
    }
    if g - r >= 12 && g - b >= 10 {
        return if g < 125 { 288 } else { 2 };
    }
    if (r >= 210 && g >= 225 && b >= 225 && b - r >= 8 && b - r < 25)
        || (r >= 248 && g >= 248 && b >= 248)
    {
        return 15;
    }
    if r - g < 18 && (r - b).abs() < 18 && r < 218 {
        return if r < 115 { 72 } else { 71 };
    }
    if r > 215 && r - b > 35 && g >= r - 25 {
        return 14;
    }
    if r > g + 8 && g > b + 12 && r < 215 {
        return 70;
    }
    19
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ground_field_uses_only_standard_colours_including_legacy_blended_records() {
        let mut surface = GroundSurface {
            format: "ergo-ground-surface-1".into(),
            rgb: (0..SAMPLE_GRID_SIZE)
                .flat_map(|y| {
                    (0..SAMPLE_GRID_SIZE)
                        .map(move |x| ((x * 4).min(255) as u32) << 16 | (y * 4).min(255) as u32)
                })
                .collect(),
        };
        surface.validate().unwrap();
        let pitch = crate::BLOCK_SIZE / (SAMPLE_GRID_SIZE - 1) as f64;
        assert_eq!(
            surface.rgb_at(pitch / 2., pitch / 2.),
            standard_ground_rgb(0x040004)
        );
        assert_eq!(surface.rgb_at(pitch, 0.), standard_ground_rgb(0x040000));
        assert_eq!(
            surface.rgb_at(0., crate::BLOCK_SIZE),
            standard_ground_rgb(0x0000ff)
        );
        for x in 0..crate::BLOCK_SIZE as u32 {
            assert!(
                TERRAIN_COLORS
                    .iter()
                    .any(|c| color_rgb(*c) == surface.rgb_at(x as f64 + 0.5, 512.5))
            );
        }
        assert_ne!(
            surface.rgb_at(0.5, 0.5),
            surface.rgb_at(crate::BLOCK_SIZE - 0.5, crate::BLOCK_SIZE - 0.5)
        );
        let mut block = GroundBlock::fallback(0, 0, "fixture".into());
        block.surface = Some(surface.clone());
        let encoded = serde_json::to_value(&block).unwrap();
        serde_json::from_value::<GroundBlock>(encoded.clone()).unwrap();
        let mut bad = encoded;
        bad["surface"]["rgb"] = serde_json::json!([0]);
        assert!(serde_json::from_value::<GroundBlock>(bad).is_err());
        surface.rgb[0] = 0x1000000;
        assert!(surface.validate().is_err());
        let old = serde_json::to_value(GroundBlock::fallback(0, 0, "old".into())).unwrap();
        assert!(old.get("surface").is_none());
        assert!(
            serde_json::from_value::<GroundBlock>(old)
                .unwrap()
                .surface
                .is_none()
        );
    }
    #[test]
    fn categorical_regions_round_corners_without_creating_gradient_colours() {
        let blue = color_rgb(1);
        let green = color_rgb(2);
        let mut surface = GroundSurface {
            format: "ergo-ground-surface-1".into(),
            rgb: vec![green; SAMPLE_GRID_SIZE * SAMPLE_GRID_SIZE],
        };
        surface.rgb[0] = blue;
        let pitch = crate::BLOCK_SIZE / (SAMPLE_GRID_SIZE - 1) as f64;
        assert_eq!(surface.rgb_at(pitch * 0.2, pitch * 0.2), blue);
        // A nearest-sample rectangle would still be blue here. Aggregating the
        // three green corners yields a rounded categorical boundary instead.
        assert_eq!(surface.rgb_at(pitch * 0.3, pitch * 0.3), green);
        for x in 0..128 {
            for y in 0..128 {
                assert!(
                    [blue, green].contains(&surface.rgb_at(f64::from(x) + 0.5, f64::from(y) + 0.5))
                );
            }
        }
    }
    #[test]
    fn map_water_and_land_use_the_requested_palette() {
        assert_eq!(map_terrain_color(0xaad3df), 1);
        assert_eq!(map_terrain_color(0xadd19e), 2);
        assert_eq!(map_terrain_color(0xddecec), 15);
        assert_eq!(map_terrain_color(0x999999), 71);
        assert_eq!(map_terrain_color(0xf5e9c6), 14);
        assert_eq!(map_terrain_color(0xc6b89e), 70);
        assert_eq!(map_terrain_color(0xf2efe9), 19);
        for rgb in (0..=0xffffff).step_by(997) {
            assert!(TERRAIN_COLORS.contains(&map_terrain_color(rgb)));
            assert!(TERRAIN_COLORS.contains(&nearest_terrain_color(rgb)));
        }
    }
    #[test]
    fn palette_projection_preserves_every_enabled_color() {
        for &(c, _) in COLORS {
            assert_eq!(nearest_color(color_rgb(c)), c);
        }
    }
}
