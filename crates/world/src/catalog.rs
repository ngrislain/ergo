//! Catalogue views of the installed part library and the colour palette.
use crate::{Brick, color_rgb};
use anyhow::{Context, Result, ensure};
use serde::Serialize;
use std::collections::BTreeSet;
#[path = "ldraw_colors.rs"]
mod ldraw_colors;

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

/// Canonical library ID of a part or alias; unknown IDs are returned as given.
pub fn canonical_id(part: &str) -> &str {
    crate::part_library::get(part).map_or(part, |entry| entry.catalogue.id.as_str())
}

pub fn canonical_part(part: &str) -> String {
    crate::part_library::get(part)
        .map_or_else(|| part.to_owned(), |entry| entry.catalogue.id.clone())
}

pub fn part_name(part: &str) -> &str {
    crate::part_library::get(part).map_or(part, |entry| entry.catalogue.name.as_str())
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
    pub library: LibrarySummary,
    pub part_resolution: &'static str,
}

/// Identity and size of the installed library; its parts are searched, not listed.
#[derive(Serialize)]
pub struct LibrarySummary {
    pub revision: &'static str,
    pub format: &'static str,
    pub parts: usize,
    pub reviewed: usize,
    pub derived: usize,
    /// BrickLink items without official geometry (listed, not buildable).
    pub listed: usize,
    pub mass_calibration: &'static crate::part_library::MassCalibration,
}
impl LibrarySummary {
    pub fn current() -> Self {
        let library = crate::part_library::library();
        let reviewed = crate::part_library::reviewed_parts().count();
        Self {
            revision: &library.revision,
            format: crate::part_library::FORMAT,
            parts: library.parts.len(),
            reviewed,
            derived: library.parts.len() - reviewed,
            listed: library.listed.len(),
            mass_calibration: &library.mass_calibration,
        }
    }
}
#[derive(Serialize)]
pub struct Part {
    pub id: &'static str,
    pub name: &'static str,
    /// Human-reviewed physical record; derived parts are buildable but not reviewed.
    pub recommended: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<&'static str>,
    #[serde(skip_serializing_if = "<[String]>::is_empty")]
    pub bricklink: &'static [String],
    #[serde(skip_serializing_if = "Option::is_none")]
    pub derivation: Option<&'static str>,
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
        parts: crate::part_library::reviewed_parts()
            .map(describe)
            .collect(),
        library: LibrarySummary::current(),
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
        part_resolution: "Every part of the installed library is buildable: the official LDraw library deduplicated against BrickLink numbers (via Rebrickable) as identity aliases. `parts` lists the human-reviewed recommended set; search the rest with get_catalog(search=...) or GET /api/catalog/parts. Derived parts (`derivation`) carry automatically derived nominal bodies, stud and socket grids and a calibrated ABS volume mass estimate, not reviewed records; derived connectors and clutch strengths are not inferred. Connector endpoints are part-local public east/north/up brick units; radius_m is SI. Fits infer geometric connectivity, not calibrated strength.",
    }
}

/// API record of one installed library part (reviewed or derived).
pub fn describe(entry: &'static crate::part_library::LibraryPart) -> Part {
    let id = entry.catalogue.id.as_str();
    let size = crate::coordinates::dimensions(
        crate::geometry::catalogue_body_bounds(&entry.catalogue)
            .expect("Library parts have nominal bodies")
            .size(),
    );
    let public = |p: [f64; 3]| {
        crate::coordinates::public(crate::ldraw::conversion() * glam::DVec3::from_array(p))
            .to_array()
    };
    Part {
        id,
        name: &entry.catalogue.name,
        recommended: entry.catalogue.derivation.is_none(),
        category: entry.catalogue.category.as_deref(),
        bricklink: &entry.catalogue.bricklink,
        derivation: entry.catalogue.derivation.as_deref(),
        set_count: entry.catalogue.set_count,
        connectors: crate::attachments::connectors(id)
            .iter()
            .map(|c| ConnectorInfo {
                kind: c.kind,
                start: public(c.start),
                end: public(c.end),
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
                    grid.first[1] + f64::from(grid.depth - 1),
                ))
                .to_array(),
            })
            .collect(),
        library_revision: entry.revision.clone(),
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
    fn natural_id_order() {
        let mut ids = vec!["3001b", "10", "3001", "2", "3001a", "973p4f", "03080board"];
        ids.sort_by(|a, b| natural(a, b));
        assert_eq!(
            ids,
            ["2", "10", "973p4f", "3001", "3001a", "3001b", "03080board"]
        );
        let page = search(&SearchQuery {
            limit: Some(128),
            ..Default::default()
        })
        .unwrap();
        let keys: Vec<String> = page
            .results
            .iter()
            .map(|hit| match hit {
                SearchHit::Part(p) => p.id.trim_end_matches(".dat").to_owned(),
                SearchHit::Listed(l) => l.bricklink.to_owned(),
            })
            .collect();
        assert!(
            keys.windows(2).all(|w| natural(&w[0], &w[1]).is_le()),
            "{keys:?}"
        );
    }
    #[test]
    fn search_folds_dimension_spellings() {
        assert_eq!(fold("Brick 2 × 4"), "brick 2x4");
        assert_eq!(fold("brick 2 x 4"), "brick 2x4");
        assert_eq!(fold("Support 2 x 2 x 7 Lamppost"), "support 2x2x7 lamppost");
        assert_eq!(fold("Minifig x ray"), "minifig x ray");
        for q in ["2x4", "2 x 4", "2 × 4"] {
            let page = search(&SearchQuery {
                q: Some(format!("brick {q}")),
                ..Default::default()
            })
            .unwrap();
            assert!(
                matches!(&page.results[0], SearchHit::Part(p) if p.id == "3001.dat"),
                "{q}"
            );
        }
    }
    #[test]
    fn search_pages_reviewed_parts_first_with_revision_bound_cursors() {
        let page = search(&SearchQuery {
            q: Some("brick 2".into()),
            limit: Some(5),
            ..Default::default()
        })
        .unwrap();
        assert_eq!(page.results.len(), 5);
        assert!(page.total > 5);
        let cursor = page.next_cursor.clone().unwrap();
        let next = search(&SearchQuery {
            q: Some("brick 2".into()),
            cursor: Some(cursor),
            limit: Some(5),
            ..Default::default()
        })
        .unwrap();
        assert!(!next.results.is_empty());
        assert!(
            search(&SearchQuery {
                cursor: Some(format!("{}:0", "0".repeat(64))),
                ..Default::default()
            })
            .is_err()
        );
        // Aliases and categories are searchable.
        let alias = search(&SearchQuery {
            q: Some("3023.dat".into()),
            ..Default::default()
        })
        .unwrap();
        assert!(matches!(&alias.results[0], SearchHit::Part(p) if p.id == "3023b.dat"));
        assert!(!page.categories.is_empty());
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

/// Which library tier a search covers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Tier {
    /// Reviewed and derived buildable parts plus listed BrickLink-only items.
    #[default]
    All,
    /// Buildable parts only (reviewed and derived).
    Buildable,
    Reviewed,
    Derived,
    /// BrickLink items without official geometry.
    Listed,
}

#[derive(Clone, Debug, Default, serde::Deserialize)]
pub struct SearchQuery {
    #[serde(default)]
    pub q: Option<String>,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub tier: Tier,
    #[serde(default)]
    pub cursor: Option<String>,
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Serialize)]
#[serde(untagged)]
pub enum SearchHit {
    Part(Box<Part>),
    Listed(ListedInfo),
}
#[derive(Serialize)]
pub struct ListedInfo {
    pub bricklink: &'static str,
    pub name: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rebrickable: Option<&'static str>,
    /// Always false: there is no official geometry to build with.
    pub buildable: bool,
}
#[derive(Serialize)]
pub struct SearchPage {
    pub revision: &'static str,
    pub total: usize,
    pub results: Vec<SearchHit>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
    /// Category counts for the query and tier, before the category filter.
    pub categories: Vec<(&'static str, usize)>,
}

pub const MAX_SEARCH_LIMIT: usize = 128;

/// Natural ordering: digit runs compare numerically, other characters by
/// case-insensitive code point; ties fall back to the plain string.
fn natural(a: &str, b: &str) -> std::cmp::Ordering {
    let (mut x, mut y) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (x.peek().copied(), y.peek().copied()) {
            (None, None) => return a.cmp(b),
            (None, _) => return std::cmp::Ordering::Less,
            (_, None) => return std::cmp::Ordering::Greater,
            (Some(p), Some(q)) if p.is_ascii_digit() && q.is_ascii_digit() => {
                let take = |it: &mut std::iter::Peekable<std::str::Chars>| {
                    let mut digits = String::new();
                    while let Some(c) = it.peek().copied().filter(char::is_ascii_digit) {
                        digits.push(c);
                        it.next();
                    }
                    digits
                };
                let (m, n) = (take(&mut x), take(&mut y));
                let (m, n) = (m.trim_start_matches('0'), n.trim_start_matches('0'));
                let order = m.len().cmp(&n.len()).then_with(|| m.cmp(n));
                if order.is_ne() {
                    return order;
                }
            }
            (Some(p), Some(q)) => {
                let order = p.to_ascii_lowercase().cmp(&q.to_ascii_lowercase());
                if order.is_ne() {
                    return order;
                }
                x.next();
                y.next();
            }
        }
    }
}

/// Case- and dimension-insensitive search form: `Brick 2 × 4`, `brick 2 x 4`
/// and `brick 2x4` all fold to `brick 2x4`.
fn fold(text: &str) -> String {
    let lower = text.to_lowercase().replace('×', " x ");
    let mut out: Vec<String> = Vec::new();
    let mut tokens = lower.split_whitespace().peekable();
    while let Some(token) = tokens.next() {
        let digit_before = out
            .last()
            .is_some_and(|t| t.ends_with(|c: char| c.is_ascii_digit()));
        let digit_after = tokens
            .peek()
            .is_some_and(|t| t.starts_with(|c: char| c.is_ascii_digit()));
        if token == "x" && digit_before && digit_after {
            let next = tokens.next().unwrap();
            let last = out.last_mut().unwrap();
            last.push('x');
            last.push_str(next);
        } else {
            out.push(token.to_owned());
        }
    }
    out.join(" ")
}

/// Search the installed library. Results are in one natural ID order across
/// every tier (`2` < `10` < `3001` < `3001b`); listed BrickLink-only items use
/// their BrickLink number. Cursors are offsets bound to the immutable revision.
pub fn search(query: &SearchQuery) -> Result<SearchPage> {
    let library = crate::part_library::library();
    let limit = query.limit.unwrap_or(48).clamp(1, MAX_SEARCH_LIMIT);
    let offset = match &query.cursor {
        None => 0,
        Some(cursor) => {
            let (revision, offset) = cursor.split_once(':').context("Invalid cursor")?;
            ensure!(
                revision == library.revision,
                "The part library changed; restart the search"
            );
            offset.parse().context("Invalid cursor")?
        }
    };
    let needle = query
        .q
        .as_deref()
        .map(str::trim)
        .filter(|q| !q.is_empty())
        .map(fold);
    let terms: Vec<&str> = needle
        .as_deref()
        .map_or(vec![], |n| n.split_whitespace().collect());
    let matches = |fields: &[&str]| {
        terms.is_empty() || {
            let folded: Vec<String> = fields.iter().map(|field| fold(field)).collect();
            terms
                .iter()
                .all(|term| folded.iter().any(|field| field.contains(term)))
        }
    };
    let category = query.category.as_deref().filter(|c| !c.is_empty());
    let wants = |reviewed: bool| match query.tier {
        Tier::All | Tier::Buildable => true,
        Tier::Reviewed => reviewed,
        Tier::Derived => !reviewed,
        Tier::Listed => false,
    };
    let mut parts: Vec<&'static crate::part_library::LibraryPart> = library
        .parts
        .values()
        .filter(|entry| wants(entry.catalogue.derivation.is_none()))
        .filter(|entry| {
            let c = &entry.catalogue;
            let mut fields: Vec<&str> = vec![&c.id, &c.name];
            fields.extend(c.aliases.iter().map(String::as_str));
            fields.extend(c.bricklink.iter().map(String::as_str));
            fields.extend(c.keywords.iter().map(String::as_str));
            fields.extend(c.category.as_deref());
            matches(&fields)
        })
        .collect();

    let listed: Vec<&'static crate::part_library::Listed> =
        if matches!(query.tier, Tier::All | Tier::Listed) {
            library
                .listed
                .iter()
                .filter(|item| {
                    let mut fields: Vec<&str> = vec![&item.bricklink, &item.name];
                    fields.extend(item.category.as_deref());
                    fields.extend(item.rebrickable.as_deref());
                    matches(&fields)
                })
                .collect()
        } else {
            vec![]
        };
    let mut counts: std::collections::BTreeMap<&'static str, usize> = Default::default();
    for c in parts
        .iter()
        .filter_map(|e| e.catalogue.category.as_deref())
        .chain(listed.iter().filter_map(|i| i.category.as_deref()))
    {
        *counts.entry(c).or_default() += 1;
    }
    let mut categories: Vec<(&'static str, usize)> = counts.into_iter().collect();
    categories.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
    if let Some(category) = category {
        parts.retain(|e| e.catalogue.category.as_deref() == Some(category));
    }
    let listed: Vec<_> = listed
        .into_iter()
        .filter(|i| category.is_none_or(|c| i.category.as_deref() == Some(c)))
        .collect();
    let total = parts.len() + listed.len();
    // One ID order across every tier; listed items are keyed by BrickLink number.
    let mut ordered: Vec<(
        &str,
        Option<&'static crate::part_library::LibraryPart>,
        Option<&'static crate::part_library::Listed>,
    )> = parts
        .iter()
        .map(|entry| {
            (
                entry.catalogue.id.trim_end_matches(".dat"),
                Some(*entry),
                None,
            )
        })
        .chain(
            listed
                .iter()
                .map(|item| (item.bricklink.as_str(), None, Some(*item))),
        )
        .collect();
    ordered.sort_by(|a, b| natural(a.0, b.0).then(a.1.is_none().cmp(&b.1.is_none())));
    let results: Vec<SearchHit> = ordered
        .into_iter()
        .skip(offset)
        .take(limit)
        .map(|(_, part, item)| match (part, item) {
            (Some(entry), _) => SearchHit::Part(Box::new(describe(entry))),
            (None, Some(item)) => SearchHit::Listed(ListedInfo {
                bricklink: &item.bricklink,
                name: &item.name,
                category: item.category.as_deref(),
                rebrickable: item.rebrickable.as_deref(),
                buildable: false,
            }),
            (None, None) => unreachable!("every hit is a part or a listed item"),
        })
        .collect();
    let end = offset + results.len();
    Ok(SearchPage {
        revision: &library.revision,
        total,
        next_cursor: (end < total).then(|| format!("{}:{end}", library.revision)),
        results,
        categories,
    })
}
