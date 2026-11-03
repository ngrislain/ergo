//! Preferred standard parts/colors, plus open-ended LDraw/Studio asset support.
use crate::{Brick, color_rgb};
use anyhow::{Result, ensure};
use serde::Serialize;
use std::{collections::BTreeSet, sync::LazyLock};
#[path = "ldraw_colors.rs"]
mod ldraw_colors;

pub static PARTS: LazyLock<Vec<(&'static str, &'static str)>> = LazyLock::new(|| {
    crate::part_library::all()
        .map(|entry| (entry.catalogue.id.as_str(), entry.catalogue.name.as_str()))
        .collect()
});

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
    crate::part_library::get(part)
        .map_or_else(|| part.to_owned(), |entry| entry.catalogue.id.clone())
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
    pub constructs: Vec<crate::constructs::ConstructInfo>,
    pub part_resolution: &'static str,
}
#[derive(Serialize)]
pub struct Part {
    pub id: &'static str,
    pub name: &'static str,
    pub recommended: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub set_count: Option<u32>,
    pub connectors: Vec<ConnectorInfo>,
    pub aliases: Vec<String>,
    pub mass_kg: f64,
    pub mass_source: String,
    pub nominal_dimensions_mm: [f64; 3],
    pub top_studs: usize,
    pub receiving_grids: Vec<ReceivingGridInfo>,
    pub library_revision: String,
}
#[derive(Serialize)]
pub struct ReceivingGridInfo {
    pub width: u32,
    pub depth: u32,
    /// First receiving centre in part-local public east/north/up units.
    pub first: [f64; 3],
}
#[derive(Serialize)]
pub struct ConnectorInfo {
    pub kind: crate::attachments::Kind,
    pub start: [f64; 3],
    pub end: [f64; 3],
    pub radius_m: f64,
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
        constructs: crate::constructs::catalog(),
        parts: PARTS
            .iter()
            .map(|(id, name)| {
                let entry = crate::part_library::get(id).expect("Catalogued source part");
                let size = crate::coordinates::dimensions(
                    crate::geometry::nominal_body_bounds(id)
                        .expect("Reviewed nominal profile")
                        .size(),
                );
                Part {
                    id,
                    name,
                    recommended: true,
                    set_count: crate::part_library::get(id)
                        .and_then(|entry| entry.catalogue.set_count),
                    connectors: crate::attachments::connectors(id)
                        .iter()
                        .map(|c| ConnectorInfo {
                            kind: c.kind,
                            start: crate::coordinates::public(
                                crate::ldraw::conversion() * glam::DVec3::from_array(c.start),
                            )
                            .to_array(),
                            end: crate::coordinates::public(
                                crate::ldraw::conversion() * glam::DVec3::from_array(c.end),
                            )
                            .to_array(),
                            radius_m: c.radius * 0.0004,
                        })
                        .collect(),
                    aliases: entry.catalogue.aliases.clone(),
                    mass_kg: entry.catalogue.mass_kg,
                    mass_source: entry.catalogue.mass_source.clone(),
                    nominal_dimensions_mm: [size.x * 8., size.y * 8., size.z * 9.6],
                    top_studs: crate::part_profiles::stud_centres(id).len(),
                    receiving_grids: crate::part_profiles::receiving_grids(id)
                        .into_iter()
                        .map(|grid| ReceivingGridInfo {
                            width: grid.width,
                            depth: grid.depth,
                            first: crate::coordinates::public(glam::DVec3::new(
                                grid.first[0],
                                grid.bottom,
                                grid.first[1],
                            ))
                            .to_array(),
                        })
                        .collect(),
                    library_revision: entry.revision.clone(),
                }
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
        part_resolution: "Any resolvable LDraw/Studio .dat ID. The bundled selection includes reviewed common parts with recorded BrickLink set_count > 7; it is not an exhaustive catalogue-wide popularity guarantee. Connector endpoints are part-local public east/north/up brick units; radius_m is SI. Fits infer geometric connectivity, not calibrated strength. Other non-standard parts require frozen sourced physical records for publication.",
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
