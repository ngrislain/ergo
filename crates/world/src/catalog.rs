//! A deliberately curated starter catalog, not a claim of BrickLink sales rank.
use crate::{Brick, color_rgb, geometry::part_spec};
use anyhow::{Result, ensure};
use serde::Serialize;
use std::collections::BTreeSet;

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
    ("3024.dat", "Plate 1 × 1"),
    ("3023.dat", "Plate 1 × 2"),
    ("3710.dat", "Plate 1 × 4"),
    ("3666.dat", "Plate 1 × 6"),
    ("3022.dat", "Plate 2 × 2"),
    ("3021.dat", "Plate 2 × 3"),
    ("3020.dat", "Plate 2 × 4"),
    ("3795.dat", "Plate 2 × 6"),
    ("3034.dat", "Plate 2 × 8"),
    ("3031.dat", "Plate 4 × 4"),
    ("3032.dat", "Plate 4 × 6"),
    ("3035.dat", "Plate 4 × 8"),
    ("41539.dat", "Plate 8 × 8"),
    ("3070b.dat", "Tile 1 × 1"),
    ("3069b.dat", "Tile 1 × 2"),
    ("3068b.dat", "Tile 2 × 2"),
    ("2431.dat", "Tile 1 × 4"),
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
        .unwrap_or("Unsupported color")
}

pub fn validate(bricks: &[Brick]) -> Result<()> {
    let parts: BTreeSet<_> = bricks
        .iter()
        .filter(|b| !PARTS.iter().any(|(p, _)| *p == b.part) || part_spec(&b.part).is_none())
        .map(|b| b.part.clone())
        .collect();
    let colors: BTreeSet<_> = bricks
        .iter()
        .filter(|b| !COLORS.iter().any(|(c, _)| *c == b.color) || b.rgb != color_rgb(b.color))
        .map(|b| b.color)
        .collect();
    ensure!(
        parts.is_empty(),
        "Unsupported parts in the starter catalog: {}",
        parts.into_iter().take(30).collect::<Vec<_>>().join(", ")
    );
    ensure!(
        colors.is_empty(),
        "Unsupported or custom colors: {colors:?}. Use one of the 20 starter colors"
    );
    Ok(())
}

#[derive(Serialize)]
pub struct Catalog {
    pub parts: Vec<Part>,
    pub colors: Vec<Color>,
}
#[derive(Serialize)]
pub struct Part {
    pub id: &'static str,
    pub name: &'static str,
}
#[derive(Serialize)]
pub struct Color {
    pub code: u32,
    pub name: &'static str,
    pub rgb: u32,
}

pub fn catalog() -> Catalog {
    Catalog {
        parts: PARTS.iter().map(|(id, name)| Part { id, name }).collect(),
        colors: COLORS
            .iter()
            .map(|(code, name)| Color {
                code: *code,
                name,
                rgb: color_rgb(*code),
            })
            .collect(),
    }
}
