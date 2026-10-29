//! Preferred standard parts/colors, plus open-ended LDraw/Studio asset support.
use crate::{Brick, color_rgb};
use anyhow::{Result, ensure};
use serde::Serialize;
use std::collections::BTreeSet;
#[path = "ldraw_colors.rs"]
mod ldraw_colors;

pub const PARTS: &[(&str, &str)] = &[
    ("3005.dat", "Brick 1 × 1"),
    ("3004.dat", "Brick 1 × 2"),
    ("3010.dat", "Brick 1 × 4"),
    ("3009.dat", "Brick 1 × 6"),
    ("3008.dat", "Brick 1 × 8"),
    ("3003.dat", "Brick 2 × 2"),
    ("3002.dat", "Brick 2 × 3"),
    ("3001.dat", "Brick 2 × 4"),
    ("2456.dat", "Brick 2 × 6"),
    ("3007.dat", "Brick 2 × 8"),
    ("4201.dat", "Brick 8 × 8"),
    ("4204.dat", "Brick 8 × 16"),
    ("3024.dat", "Plate 1 × 1"),
    ("3023.dat", "Plate 1 × 2"),
    ("3710.dat", "Plate 1 × 4"),
    ("3666.dat", "Plate 1 × 6"),
    ("3022.dat", "Plate 2 × 2"),
    ("3021.dat", "Plate 2 × 3"),
    ("3020.dat", "Plate 2 × 4"),
    ("3795.dat", "Plate 2 × 6"),
    ("3034.dat", "Plate 2 × 8"),
    ("3832.dat", "Plate 2 × 10"),
    ("4282.dat", "Plate 2 × 16"),
    ("3031.dat", "Plate 4 × 4"),
    ("3032.dat", "Plate 4 × 6"),
    ("3035.dat", "Plate 4 × 8"),
    ("41539.dat", "Plate 8 × 8"),
    ("3026.dat", "Plate 6 × 24"),
    ("3070b.dat", "Tile 1 × 1"),
    ("3069b.dat", "Tile 1 × 2"),
    ("3068b.dat", "Tile 2 × 2"),
    ("2431.dat", "Tile 1 × 4"),
    ("60594.dat", "Window frame 1 × 4 × 3"),
    ("60592.dat", "Window frame 1 × 2 × 2"),
    ("3037.dat", "Slope 45° 2 × 4"),
    ("3039.dat", "Slope 45° 2 × 2"),
    ("3633.dat", "Lattice fence 1 × 4 × 1"),
    ("3062b.dat", "Round brick 1 × 1, hollow stud"),
    ("3941.dat", "Round brick 2 × 2"),
    ("87081.dat", "Round brick 4 × 4"),
    ("2039.dat", "Lamppost 2 × 2 × 7"),
    ("4073.dat", "Round plate 1 × 1"),
    ("4740.dat", "Inverted dish 2 × 2"),
    ("87580.dat", "Centre-stud plate 2 × 2"),
    ("970c00.dat", "Minifigure hips and legs"),
    ("76382p00.dat", "Minifigure torso, arms and yellow hands"),
    ("3626a.dat", "Minifigure head, solid stud"),
    ("3626ap01.dat", "Minifigure head, classic grin"),
    ("4624c01.dat", "Wheel rim with tyre assembly"),
    ("3901.dat", "Minifigure hair, male smooth"),
    ("4211.dat", "Classic vehicle base 4 × 5"),
    ("3788.dat", "Studded vehicle mudguard 2 × 4"),
    ("3823.dat", "Windscreen 2 × 4 × 2"),
    ("4083.dat", "Open-stud roll bar 1 × 4 × 2"),
    ("3829c01.dat", "Steering stand with black wheel"),
];

pub const COLORS: &[(u32, &str)] = &[
    (0, "Black"),
    (1, "Blue"),
    (2, "Green"),
    (3, "Dark turquoise"),
    (4, "Red"),
    (7, "Light gray"),
    (12, "Salmon"),
    (14, "Yellow"),
    (15, "White"),
    (19, "Tan"),
    (25, "Orange"),
    (28, "Dark tan"),
    (70, "Reddish brown"),
    (71, "Light bluish gray"),
    (72, "Dark bluish gray"),
    (272, "Dark blue"),
    (288, "Dark green"),
    (320, "Dark red"),
    (321, "Dark azure"),
    (378, "Sand green"),
];

pub fn canonical_part(part: &str) -> String {
    match part {
        "3070.dat" => "3070b.dat".into(),
        "3069.dat" => "3069b.dat".into(),
        "3068.dat" => "3068b.dat".into(),
        _ => part.to_owned(),
    }
}

pub fn part_name(part: &str) -> &str {
    PARTS
        .iter()
        .find(|(p, _)| *p == part)
        .map(|(_, name)| *name)
        .unwrap_or(part)
}
pub fn color_name(color: u32) -> &'static str {
    COLORS
        .iter()
        .find(|(c, _)| *c == color)
        .map(|(_, name)| *name)
        .or_else(|| {
            ldraw_colors::COLORS
                .iter()
                .find(|(c, _, _, _)| *c == color)
                .map(|(_, name, _, _)| *name)
        })
        .unwrap_or("Custom color")
}

pub fn valid_part(part: &str) -> bool {
    part.ends_with(".dat")
        && part
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_. /".contains(&b))
        && crate::ldraw::normalize_name(part).is_ok_and(|name| name == part)
}
pub fn extended_rgb(color: u32) -> Option<u32> {
    ldraw_colors::COLORS
        .iter()
        .find(|(code, _, _, _)| *code == color)
        .map(|(_, _, rgb, _)| *rgb)
}
/// Catalogue ALPHA, including LDraw's direct transparent-colour encoding.
/// This is a rendering material property, not a collision or mass override.
pub fn color_alpha(color: u32) -> u8 {
    match color >> 24 {
        2 => 255,
        3 => 128,
        _ => ldraw_colors::COLORS
            .binary_search_by_key(&color, |(code, _, _, _)| *code)
            .ok()
            .map_or(255, |index| ldraw_colors::COLORS[index].3),
    }
}
pub fn known_color(color: u32) -> bool {
    matches!(color >> 24, 2 | 3)
        || extended_rgb(color).is_some()
        || COLORS.iter().any(|(code, _)| *code == color)
}

pub fn validate(bricks: &[Brick]) -> Result<()> {
    let parts: BTreeSet<_> = bricks
        .iter()
        .filter(|b| !valid_part(&b.part))
        .map(|b| b.part.clone())
        .collect();
    let colors: BTreeSet<_> = bricks
        .iter()
        .filter(|b| b.rgb > 0xffffff || [16, 24].contains(&b.color))
        .map(|b| b.color)
        .collect();
    ensure!(
        parts.is_empty(),
        "Invalid LDraw part IDs: {}",
        parts.into_iter().take(30).collect::<Vec<_>>().join(", ")
    );
    ensure!(
        colors.is_empty(),
        "Unresolved inherited colors or invalid RGB values: {colors:?}"
    );
    Ok(())
}

#[derive(Serialize)]
pub struct Catalog {
    pub parts: Vec<Part>,
    pub colors: Vec<Color>,
    pub part_resolution: &'static str,
}
#[derive(Serialize)]
pub struct Part {
    pub id: &'static str,
    pub name: &'static str,
    pub recommended: bool,
}
#[derive(Serialize)]
pub struct Color {
    pub code: u32,
    pub name: &'static str,
    pub rgb: u32,
    pub alpha: u8,
    pub recommended: bool,
}

pub fn catalog() -> Catalog {
    Catalog {
        parts: PARTS
            .iter()
            .map(|(id, name)| Part {
                id,
                name,
                recommended: true,
            })
            .collect(),
        colors: COLORS
            .iter()
            .map(|(code, name)| Color {
                code: *code,
                name,
                rgb: color_rgb(*code),
                alpha: 255,
                recommended: true,
            })
            .chain(
                ldraw_colors::COLORS
                    .iter()
                    .filter(|(code, _, _, _)| {
                        !COLORS.iter().any(|(c, _)| c == code) && ![16, 24].contains(code)
                    })
                    .map(|(code, name, _, alpha)| Color {
                        code: *code,
                        name,
                        rgb: color_rgb(*code),
                        alpha: *alpha,
                        recommended: false,
                    }),
            )
            .collect(),
        part_resolution: "Any resolvable LDraw/Studio .dat ID; listed parts are recommendations, not an allowlist. Profiled specialty parts require genuine source geometry and use sourced catalogue masses with explicit nominal collision/interface proxies, not calibrated clutch strength. Other non-standard parts require frozen sourced physical records for publication.",
    }
}

pub fn preferred_catalog() -> Catalog {
    let mut catalog = catalog();
    catalog.colors.retain(|c| c.recommended);
    catalog
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn material_alpha_uses_catalogue_and_direct_colour_semantics() {
        assert!(
            ldraw_colors::COLORS
                .windows(2)
                .all(|rows| rows[0].0 < rows[1].0)
        );
        assert_eq!(color_alpha(43), 128);
        assert_eq!(color_alpha(47), 128);
        assert_eq!(color_alpha(1), 255);
        assert_eq!(color_alpha(0x3123456), 128);
        assert_eq!(color_alpha(0x2123456), 255);
        assert_eq!(color_alpha(999_999), 255);
    }
    #[test]
    fn complete_palette_and_open_parts_keep_standard_recommendations_first() {
        let catalog = catalog();
        assert!(catalog.colors.len() > 300);
        assert!(catalog.colors[..COLORS.len()].iter().all(|c| c.recommended));
        assert!(
            catalog.colors[COLORS.len()..]
                .iter()
                .all(|c| !c.recommended)
        );
        assert_eq!(preferred_catalog().colors.len(), COLORS.len());
        assert_eq!(
            catalog.colors.iter().find(|c| c.code == 47).unwrap().alpha,
            128
        );
        assert_eq!(color_rgb(47), 0xfcfcfc);
        assert!(valid_part("3039.dat"));
        assert!(valid_part("973pb1234.dat"));
        assert!(!valid_part("../part.dat"));
        assert!(!valid_part("part?x=.dat"));
    }
}
