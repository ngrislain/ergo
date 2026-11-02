//! One deterministic catalogue, pinned to official LDraw and BrickLink sources.
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap},
    sync::LazyLock,
};

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Part {
    pub id: String,
    pub name: String,
    pub mass_kg: f64,
    pub mass_source: String,
    /// Rectangular local X/Y footprint in studs, height in plates; source-only
    /// parts retain their reviewed nominal interface profiles instead.
    pub size: Option<[u32; 3]>,
    pub studs: Option<bool>,
    pub aliases: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub set_count: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nominal: Option<crate::part_profiles::Nominal>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub connectors: Vec<crate::attachments::Connector>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LockedPart {
    pub revision: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_revision: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vertex_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub asset: Option<PackedAsset>,
    pub files: Vec<String>,
    pub catalogue: Part,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SourceFile {
    pub path: String,
    pub sha256: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PackedAsset {
    pub offset: usize,
    pub bytes: usize,
    /// SHA-256 of decoded native Mesh CBOR, not compressed transport bytes.
    pub mesh_hash: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Library {
    pub format: String,
    pub authorities: BTreeMap<String, String>,
    pub source_archive_sha256: Option<String>,
    pub normalization: String,
    pub parts: BTreeMap<String, LockedPart>,
    pub files: BTreeMap<String, SourceFile>,
}

pub fn library() -> &'static Library {
    static LIBRARY: LazyLock<Library> = LazyLock::new(|| {
        let value: Library = serde_json::from_str(include_str!("../library/sources.lock.json"))
            .expect("Checked-in part library must be valid");
        assert_eq!(value.format, "ergo-part-library-lock-1");
        value
    });
    &LIBRARY
}

pub fn all() -> impl ExactSizeIterator<Item = &'static LockedPart> {
    library().parts.values()
}

pub fn get(id: &str) -> Option<&'static LockedPart> {
    static INDEX: LazyLock<HashMap<&'static str, &'static LockedPart>> = LazyLock::new(|| {
        let mut index = HashMap::new();
        for entry in all() {
            for id in std::iter::once(&entry.catalogue.id).chain(&entry.catalogue.aliases) {
                assert!(
                    index.insert(id.as_str(), entry).is_none(),
                    "Duplicate canonical ID or alias"
                );
            }
        }
        index
    });
    INDEX.get(id).copied()
}

/// Existing nominal interface datums keep their reviewed profile when an
/// official identity alias changes the catalogue spelling, not its pose.
pub fn profile_id(part: &str) -> &str {
    get(part).map_or(part, |entry| {
        entry
            .catalogue
            .profile_id
            .as_deref()
            .unwrap_or(&entry.catalogue.id)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn canonical_shapes_sources_and_aliases_are_unique_and_pinned() {
        assert_eq!(library().authorities.len(), 2);
        let mut shapes = std::collections::BTreeSet::new();
        let mut rectangular = 0;
        for entry in all() {
            let part = &entry.catalogue;
            assert!(crate::catalog::valid_part(&part.id));
            assert!(part.mass_kg.is_finite() && part.mass_kg > 0.);
            assert_eq!(entry.revision.len(), 64);
            assert_eq!(entry.vertex_hash.as_ref().map(String::len), Some(64));
            assert!(entry.files.contains(&part.id));
            for file in &entry.files {
                let source = &library().files[file];
                assert!(source.path.starts_with("parts/") || source.path.starts_with("p/"));
                assert_eq!(source.sha256.len(), 64);
            }
            if let Some(size) = part.size {
                rectangular += 1;
                assert!(size.iter().all(|dimension| *dimension > 0));
                assert!(
                    shapes.insert((size, part.studs)),
                    "Near-duplicate rectangular part: {}",
                    part.id
                );
                assert_eq!(crate::geometry::part_spec(&part.id).unwrap().width, size[0]);
            }
            for alias in &part.aliases {
                assert!(std::ptr::eq(get(alias).unwrap(), entry));
                assert_eq!(crate::catalog::canonical_part(alias), part.id);
            }
            assert_eq!(crate::physical::mass_kg(&part.id).unwrap(), part.mass_kg);
            let body = crate::geometry::nominal_body_bounds(&part.id).unwrap();
            for port in &part.connectors {
                let start = glam::DVec3::from_array(port.start);
                let end = glam::DVec3::from_array(port.end);
                assert!(start.is_finite() && end.is_finite() && (end - start).length() > 1e-6);
                assert!(port.radius.is_finite() && port.radius > 0. && port.inset >= 0.);
                assert!(port.inset * 2. < (end - start).length());
                if let Some(key) = port.key {
                    let key = glam::DVec3::from_array(key);
                    assert!(
                        key.is_finite() && key.length() > 0. && key.dot(end - start).abs() < 1e-8
                    );
                }
                let c = crate::ldraw::conversion();
                let axis = (end - start).normalize();
                let radial = (glam::DVec3::ONE - axis * axis)
                    .max(glam::DVec3::ZERO)
                    .map(f64::sqrt)
                    * port.radius;
                for point in [start, end] {
                    assert!(
                        body.expanded(crate::attachments::PORT_OVERHANG).contains(
                            crate::Aabb::new(point - radial, point + radial)
                                .transformed(c, glam::DVec3::ZERO)
                        ),
                        "{} connector exceeds the server search halo",
                        part.id
                    );
                }
            }
        }
        assert_eq!(all().len(), 124);
        assert_eq!(rectangular, 44);
        for id in [
            "2780.dat",
            "3700.dat",
            "32278.dat",
            "3705.dat",
            "3713.dat",
            "4600.dat",
            "3820.dat",
            "3837.dat",
        ] {
            let part = &get(id).unwrap().catalogue;
            assert!(part.set_count.is_some_and(|sets| sets > 7));
            assert!(!part.connectors.is_empty());
            assert!(crate::geometry::nominal_body_bounds(id).is_some());
        }
    }
}
