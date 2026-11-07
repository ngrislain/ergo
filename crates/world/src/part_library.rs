//! The single part library: one synthesized, content-addressed index of the
//! official LDraw library, deduplicated against BrickLink through Rebrickable.
//! Each part carries either a human-reviewed physical record or an
//! automatically derived (`derivation`) profile. The library is produced by the
//! operator synthesis (`ergo-server parts-sync`) and installed once per process.
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap},
    sync::RwLock,
};

pub const FORMAT: &str = "ergo-part-library-2";

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Part {
    pub id: String,
    pub name: String,
    /// Official LDraw `!CATEGORY`, else the description's first word.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub keywords: Vec<String>,
    /// BrickLink item numbers deduplicated onto this geometry.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bricklink: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rebrickable: Option<String>,
    pub mass_kg: f64,
    pub mass_source: String,
    /// Rectangular local X/Y footprint in studs, height in plates. Reviewed
    /// rectangular parts only; every other part has a `nominal` record.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<[u32; 3]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub studs: Option<bool>,
    /// Identity aliases: official moved/alias files and deduplicated IDs.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub aliases: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub set_count: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nominal: Option<crate::part_profiles::Nominal>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub connectors: Vec<crate::attachments::Connector>,
    /// Set only on automatically derived, unreviewed profiles (`auto-v1`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub derivation: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LibraryPart {
    /// Covers the pinned source closure, baker format and physical record.
    pub revision: String,
    /// SHA-256 of the complete internal float32 vertex stream.
    pub vertex_hash: String,
    /// SHA-256 of the native Mesh CBOR stored at `objects/<mesh_hash>.cbor`.
    pub mesh_hash: String,
    pub catalogue: Part,
}

/// A BrickLink catalogue item with no official LDraw geometry. Listed for
/// reference only: it cannot be built and no substitute shape is invented.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Listed {
    pub bricklink: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rebrickable: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MassCalibration {
    /// Multiplier applied to the ABS voxel estimate of derived parts.
    pub factor: f64,
    /// Reviewed parts used for the fit, and |relative error| percentiles.
    pub samples: usize,
    pub p50: f64,
    pub p90: f64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Library {
    pub format: String,
    /// SHA-256 of this document serialized with an empty revision.
    pub revision: String,
    /// Content hashes of every synthesis input (archive, dumps, reviews).
    pub inputs: BTreeMap<String, String>,
    pub derivation: String,
    pub mass_calibration: MassCalibration,
    pub parts: BTreeMap<String, LibraryPart>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub listed: Vec<Listed>,
}

/// Committed human-reviewed physical records, a synthesis input.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Reviews {
    pub format: String,
    pub note: String,
    pub parts: Vec<Review>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Review {
    pub id: String,
    pub mass_kg: f64,
    pub mass_source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub set_count: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<[u32; 3]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub studs: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nominal: Option<crate::part_profiles::Nominal>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub connectors: Vec<crate::attachments::Connector>,
}

struct Installed {
    library: &'static Library,
    index: HashMap<&'static str, &'static LibraryPart>,
    meshes: HashMap<&'static str, &'static LibraryPart>,
}
static INSTALLED: RwLock<Option<&'static Installed>> = RwLock::new(None);

/// Validate and install the process-wide library. Installing the same
/// revision again is a no-op. A different revision replaces it for every
/// later lookup (hot swap); earlier revisions stay valid in memory, so
/// references obtained before the swap remain sound.
pub fn install(library: Library) -> Result<()> {
    if INSTALLED
        .read()
        .unwrap()
        .is_some_and(|current| current.library.revision == library.revision)
    {
        return Ok(());
    }
    validate(&library)?;
    let library: &'static Library = Box::leak(Box::new(library));
    let mut index = HashMap::new();
    let mut meshes = HashMap::new();
    for entry in library.parts.values() {
        for id in std::iter::once(&entry.catalogue.id).chain(&entry.catalogue.aliases) {
            index.insert(id.as_str(), entry);
        }
        meshes.insert(entry.mesh_hash.as_str(), entry);
    }
    let installed: &'static Installed = Box::leak(Box::new(Installed {
        library,
        index,
        meshes,
    }));
    *INSTALLED.write().unwrap() = Some(installed);
    Ok(())
}

/// SHA-256 content revision: the document serialized with an empty revision.
pub fn content_revision(library: &Library) -> Result<String> {
    use sha2::{Digest, Sha256};
    let mut unrevisioned = library.clone();
    unrevisioned.revision.clear();
    Ok(format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&unrevisioned)?)
    ))
}

/// Parse published library JSON and verify its content revision.
pub fn verify(json: &[u8]) -> Result<Library> {
    let library: Library = serde_json::from_slice(json)?;
    ensure!(
        content_revision(&library)? == library.revision,
        "Part library revision does not match its content"
    );
    Ok(library)
}

/// The reviewed tier of a library as its own self-consistent library: the
/// small document behind static illustrations and the test fixture.
pub fn reviewed_tier(library: &Library) -> Result<Library> {
    let mut tier = library.clone();
    tier.parts
        .retain(|_, entry| entry.catalogue.derivation.is_none());
    tier.listed.clear();
    tier.revision = content_revision(&tier)?;
    validate(&tier)?;
    Ok(tier)
}

/// Verify and install published library JSON (browser, MCP and server).
pub fn install_json(json: &[u8]) -> Result<()> {
    install(verify(json)?)
}

pub fn validate(library: &Library) -> Result<()> {
    ensure!(library.format == FORMAT, "Unsupported part library format");
    ensure!(
        library.revision.len() == 64,
        "Part library revision must be a SHA-256"
    );
    let mut seen = std::collections::HashSet::new();
    for (key, entry) in &library.parts {
        let part = &entry.catalogue;
        ensure!(key == &part.id, "Part library key mismatch for {key}");
        ensure!(
            crate::catalog::valid_part(&part.id),
            "Invalid part {}",
            part.id
        );
        ensure!(
            part.mass_kg.is_finite() && part.mass_kg > 0.,
            "Part {} needs a positive mass",
            part.id
        );
        ensure!(
            entry.vertex_hash.len() == 64 && entry.mesh_hash.len() == 64,
            "Part {} must pin its geometry",
            part.id
        );
        ensure!(
            part.size.is_some() != part.nominal.is_some(),
            "Part {} needs exactly one of a rectangular size or a nominal profile",
            part.id
        );
        ensure!(
            part.derivation.is_none() || part.size.is_none(),
            "Derived part {} cannot claim a reviewed rectangular datum",
            part.id
        );
        for id in std::iter::once(&part.id).chain(&part.aliases) {
            ensure!(seen.insert(id.as_str()), "Duplicate part ID or alias {id}");
        }
    }
    Ok(())
}

fn installed() -> Option<&'static Installed> {
    #[cfg(any(test, feature = "test-library"))]
    if INSTALLED.read().unwrap().is_none() {
        install_test_library();
    }
    *INSTALLED.read().unwrap()
}

/// Test builds install the committed synthesized fixture: the reviewed tier
/// plus a few derived parts (`ergo_assets::library::FIXTURE_DERIVED`).
#[cfg(any(test, feature = "test-library"))]
pub fn install_test_library() {
    let library: Library =
        serde_json::from_str(include_str!("../tests/fixtures/part-library.json"))
            .expect("Valid part-library test fixture");
    // Only the first caller installs: tests never replace a library swapped in.
    if INSTALLED.read().unwrap().is_none() {
        install(library).expect("Test part library");
    }
}

/// The installed library. Panics only if called before installation.
pub fn library() -> &'static Library {
    installed()
        .expect("The part library has not been installed")
        .library
}

pub fn is_installed() -> bool {
    installed().is_some()
}

pub fn all() -> impl Iterator<Item = &'static LibraryPart> {
    installed()
        .into_iter()
        .flat_map(|installed| installed.library.parts.values())
}

/// Human-reviewed parts, the recommended building set.
pub fn reviewed_parts() -> impl Iterator<Item = &'static LibraryPart> {
    all().filter(|entry| entry.catalogue.derivation.is_none())
}

pub fn get(id: &str) -> Option<&'static LibraryPart> {
    installed()?.index.get(id).copied()
}

/// The library part whose geometry object has this mesh hash, if any.
pub fn by_mesh_hash(hash: &str) -> Option<&'static LibraryPart> {
    installed()?.meshes.get(hash).copied()
}

/// True only for a human-reviewed library entry or one of its aliases.
pub fn reviewed(id: &str) -> bool {
    get(id).is_some_and(|entry| entry.catalogue.derivation.is_none())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reviewed_shapes_and_aliases_are_unique_and_pinned() {
        let mut shapes = std::collections::BTreeSet::new();
        let mut rectangular = 0;
        for entry in reviewed_parts() {
            let part = &entry.catalogue;
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
        assert_eq!(reviewed_parts().count(), 138);
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

    #[test]
    fn validation_rejects_duplicate_ids_and_unprofiled_parts() {
        let mut library = library().clone();
        library.revision = "f".repeat(64);
        assert!(validate(&library).is_ok());
        let mut broken = library.clone();
        let first = broken.parts.values_mut().next().unwrap();
        first.catalogue.size = None;
        first.catalogue.nominal = None;
        assert!(validate(&broken).is_err());
        let mut broken = library.clone();
        let alias = broken.parts.values().nth(1).unwrap().catalogue.id.clone();
        broken
            .parts
            .values_mut()
            .next()
            .unwrap()
            .catalogue
            .aliases
            .push(alias);
        assert!(validate(&broken).is_err());
        let mut broken = library;
        let first = broken
            .parts
            .values_mut()
            .find(|p| p.catalogue.size.is_some())
            .unwrap();
        first.catalogue.derivation = Some("auto-v1".into());
        assert!(validate(&broken).is_err());
        // Reinstalling the current revision is a no-op.
        let current = super::library();
        assert!(install(current.clone()).is_ok());
        assert!(std::ptr::eq(super::library(), current));
    }

    #[test]
    fn published_json_is_verified_against_its_content_revision() {
        let json = serde_json::to_vec(super::library()).unwrap();
        assert!(verify(&json).is_ok());
        let mut tampered = super::library().clone();
        tampered
            .parts
            .values_mut()
            .next()
            .unwrap()
            .catalogue
            .mass_kg *= 2.;
        assert!(verify(&serde_json::to_vec(&tampered).unwrap()).is_err());
    }
}
