pub mod catalog;
pub mod constraints;
pub mod coordinates;
pub mod demo;
pub mod geometry;
pub mod groups;
pub mod ldraw;
pub mod mcp;
pub mod physical;
#[cfg(feature = "simulation")]
pub mod physics;
#[cfg(feature = "simulation")]
pub mod solidity;
pub mod spatial;
pub mod terrain;
pub mod validation;

use glam::{DMat3, DVec3};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const BLOCK_SIZE: f64 = 1024.0;
pub const SCHEMA_VERSION: u32 = 2;
pub const MAX_SIMULATION_BRICKS: usize = 5000;
// Keep world positions comfortably inside JS's exact integer range, including offsets.
pub const MAX_WORLD_COORD: f64 = 1_000_000_000_000.0;

#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(from = "[i64;3]", into = "[i64;3]")]
pub struct BlockCoord(pub i64, pub i64, pub i64);

impl From<[i64; 3]> for BlockCoord {
    fn from(p: [i64; 3]) -> Self {
        Self(p[0], p[2], !p[1])
    }
}
impl From<BlockCoord> for [i64; 3] {
    fn from(p: BlockCoord) -> Self {
        [p.0, !p.2, p.1]
    }
}

impl BlockCoord {
    pub fn at(p: DVec3) -> Self {
        let p = coordinates::public(p) / BLOCK_SIZE;
        Self::from([p.x.floor() as i64, p.y.floor() as i64, p.z.floor() as i64])
    }

    pub fn key(self) -> String {
        format!("{}_{}_{}", self.0, !self.2, self.1)
    }

    pub fn origin(self) -> DVec3 {
        DVec3::new(self.0 as f64, self.1 as f64, self.2 as f64) * BLOCK_SIZE
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct GeoReference {
    pub latitude: f64,
    pub longitude: f64,
    pub arcseconds_per_block: f64,
}

impl Default for GeoReference {
    fn default() -> Self {
        Self {
            latitude: 48.8566,
            longitude: 2.3522,
            arcseconds_per_block: 1.0,
        }
    }
}

impl GeoReference {
    pub fn validate(self) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.latitude.is_finite() && self.latitude.abs() <= 90.0,
            "Invalid origin latitude"
        );
        anyhow::ensure!(
            self.longitude.is_finite() && self.longitude.abs() <= 180.0,
            "Invalid origin longitude"
        );
        anyhow::ensure!(
            self.arcseconds_per_block.is_finite() && self.arcseconds_per_block > 0.0,
            "Arc-seconds per block must be positive"
        );
        Ok(())
    }

    pub fn to_world(self, latitude: f64, longitude: f64) -> anyhow::Result<DVec3> {
        anyhow::ensure!(
            latitude.is_finite()
                && latitude.abs() <= 90.0
                && longitude.is_finite()
                && longitude.abs() <= 180.0,
            "Invalid GPS coordinates"
        );
        let longitude_delta = (longitude - self.longitude + 180.0).rem_euclid(360.0) - 180.0;
        let scale = 3600.0 * BLOCK_SIZE / self.arcseconds_per_block;
        Ok(DVec3::new(
            longitude_delta * scale,
            0.0,
            -(latitude - self.latitude) * scale,
        ))
    }

    pub fn to_gps(self, p: DVec3) -> [f64; 2] {
        let scale = self.arcseconds_per_block / (3600.0 * BLOCK_SIZE);
        [
            self.latitude - p.z * scale,
            (self.longitude + p.x * scale + 180.0).rem_euclid(360.0) - 180.0,
        ]
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(from = "coordinates::BoundsWire", into = "coordinates::BoundsWire")]
pub struct Aabb {
    pub min: DVec3,
    pub max: DVec3,
}

impl Aabb {
    pub fn new(min: DVec3, max: DVec3) -> Self {
        Self { min, max }
    }
    pub fn center(self) -> DVec3 {
        (self.min + self.max) * 0.5
    }
    pub fn size(self) -> DVec3 {
        self.max - self.min
    }
    pub fn union(self, other: Self) -> Self {
        Self::new(self.min.min(other.min), self.max.max(other.max))
    }
    pub fn expanded(self, n: f64) -> Self {
        Self::new(self.min - DVec3::splat(n), self.max + DVec3::splat(n))
    }
    pub fn intersects(self, other: Self) -> bool {
        self.min.cmple(other.max).all() && self.max.cmpge(other.min).all()
    }
    pub fn contains(self, other: Self) -> bool {
        self.min.cmple(other.min).all() && self.max.cmpge(other.max).all()
    }

    pub fn transformed(self, m: DMat3, p: DVec3) -> Self {
        let center = m * self.center() + p;
        let half = m.abs() * (self.size() * 0.5);
        Self::new(center - half, center + half)
    }

    pub fn ray(self, origin: DVec3, direction: DVec3) -> Option<f64> {
        let mut near: f64 = 0.0;
        let mut far = f64::INFINITY;
        for axis in 0..3 {
            if direction[axis].abs() < 1e-12 {
                if origin[axis] < self.min[axis] || origin[axis] > self.max[axis] {
                    return None;
                }
            } else {
                let a = (self.min[axis] - origin[axis]) / direction[axis];
                let b = (self.max[axis] - origin[axis]) / direction[axis];
                near = near.max(a.min(b));
                far = far.min(a.max(b));
                if near > far {
                    return None;
                }
            }
        }
        Some(near)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Brick {
    pub id: u32,
    pub part: String,
    pub color: u32,
    pub rgb: u32,
    #[serde(with = "coordinates::vec3")]
    pub position: DVec3,
    /// Column-major transform in world brick units, including nonuniform LDU conversion.
    #[serde(with = "coordinates::mat3")]
    pub transform: DMat3,
    /// Body bounds, excluding decorative studs for catalog parts.
    pub bounds: Aabb,
    /// Includes every rendered vertex, used for partitioning and picking.
    pub render_bounds: Aabb,
    pub step: u32,
}

impl Brick {
    pub fn translated(&mut self, delta: DVec3) {
        self.position += delta;
        self.bounds.min += delta;
        self.bounds.max += delta;
        self.render_bounds.min += delta;
        self.render_bounds.max += delta;
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Block {
    pub schema: u32,
    pub coord: BlockCoord,
    pub bricks: Vec<Brick>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BlockInfo {
    pub coord: BlockCoord,
    pub key: String,
    pub brick_count: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WorldManifest {
    pub schema: u32,
    pub id: String,
    pub name: String,
    pub source: String,
    pub brick_count: usize,
    pub bounds: Aabb,
    pub geo: GeoReference,
    pub blocks: Vec<BlockInfo>,
    pub geometry: BTreeMap<String, String>,
    pub proxy_parts: Vec<String>,
    pub notes: Vec<String>,
    #[serde(default)]
    pub joints: Vec<JointSpec>,
    #[serde(default)]
    pub fixed_bricks: Vec<u32>,
    pub validation: validation::ValidationReport,
    pub owner: String,
    #[serde(default)]
    pub ground: Vec<terrain::GroundBlock>,
    #[serde(default)]
    pub groups: Vec<groups::Group>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct JointSpec {
    pub a: u32,
    pub b: u32,
    /// World-space joint position in the reference (unsimulated) scene.
    #[serde(with = "coordinates::vec3")]
    pub anchor: DVec3,
    #[serde(flatten)]
    pub kind: JointKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub break_force: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub break_torque: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strength_source: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum JointKind {
    Fixed,
    Revolute {
        #[serde(with = "coordinates::axis")]
        axis: DVec3,
        #[serde(default)]
        limits: Option<[f64; 2]>,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WorldExport {
    pub schema: u32,
    pub name: String,
    pub geo: GeoReference,
    pub bricks: Vec<Brick>,
    pub meshes: BTreeMap<String, geometry::Mesh>,
    #[serde(default)]
    pub joints: Vec<JointSpec>,
    #[serde(default)]
    pub fixed_bricks: Vec<u32>,
    #[serde(default)]
    pub ground: Vec<terrain::GroundBlock>,
    #[serde(default)]
    pub groups: Vec<groups::Group>,
}

/// Replicate boundary-crossing bricks into all intersected cells. IDs deduplicate them
/// in the viewer. Limit spans so malformed models cannot allocate billions of cells.
pub fn partition(bricks: &[Brick]) -> anyhow::Result<BTreeMap<BlockCoord, Block>> {
    let mut blocks = BTreeMap::new();
    let mut references = 0_usize;
    for brick in bricks {
        let b = brick.render_bounds;
        anyhow::ensure!(
            b.min.is_finite()
                && b.max.is_finite()
                && b.min.abs().max_element() <= MAX_WORLD_COORD
                && b.max.abs().max_element() <= MAX_WORLD_COORD,
            "Model coordinates exceed world limits"
        );
        let min = DVec3::new(b.min.x, -b.max.z, b.min.y) / BLOCK_SIZE;
        let max = (DVec3::new(b.max.x, -b.min.z, b.max.y) - DVec3::splat(1e-8)) / BLOCK_SIZE;
        let lo = [
            min.x.floor() as i64,
            min.y.floor() as i64,
            min.z.floor() as i64,
        ];
        let hi = [
            max.x.floor() as i64,
            max.y.floor() as i64,
            max.z.floor() as i64,
        ];
        let count = (hi[0] - lo[0] + 1).max(1) as u128
            * (hi[1] - lo[1] + 1).max(1) as u128
            * (hi[2] - lo[2] + 1).max(1) as u128;
        anyhow::ensure!(count <= 64, "A single part spans more than 64 blocks");
        references += count as usize;
        anyhow::ensure!(
            references <= 4_000_000,
            "Model has too many cross-block references"
        );
        for x in lo[0]..=hi[0].max(lo[0]) {
            for y in lo[1]..=hi[1].max(lo[1]) {
                for z in lo[2]..=hi[2].max(lo[2]) {
                    let coord = BlockCoord::from([x, y, z]);
                    blocks
                        .entry(coord)
                        .or_insert_with(|| Block {
                            schema: SCHEMA_VERSION,
                            coord,
                            bricks: Vec::new(),
                        })
                        .bricks
                        .push(brick.clone());
                }
            }
        }
    }
    Ok(blocks)
}

pub fn bounds(bricks: &[Brick]) -> Option<Aabb> {
    bricks.iter().map(|b| b.render_bounds).reduce(Aabb::union)
}

pub fn color_rgb(code: u32) -> u32 {
    if code & 0xFF000000 == 0x02000000 {
        return code & 0xFFFFFF;
    }
    match code {
        0 => 0x252a32,
        1 => 0x245ba8,
        2 => 0x237841,
        3 => 0x008f9b,
        4 => 0xc52d2c,
        5 => 0xc870a0,
        6 => 0x583927,
        7 => 0x9ba3a8,
        8 => 0x59636d,
        9 => 0x83b7d7,
        10 => 0x4b9f4a,
        11 => 0x55a5af,
        12 => 0xf2705e,
        13 => 0xfc97ac,
        14 => 0xf2cc3e,
        15 => 0xf3f3ed,
        19 => 0xd9c49a,
        22 => 0x81007b,
        25 => 0xf28c28,
        28 => 0x958a73,
        70 => 0x582a12,
        71 => 0xa0a5a9,
        72 => 0x6c6e68,
        73 => 0x6e99c9,
        78 => 0xf6d7b3,
        85 => 0x441a91,
        272 => 0x19325a,
        288 => 0x184632,
        320 => 0x872b22,
        321 => 0x0c8caa,
        322 => 0x36aebf,
        323 => 0xadd9d0,
        378 => 0x9ba99e,
        484 => 0xa65331,
        _ => 0xb6a9c9,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn negative_cells_use_floor_not_truncation() {
        assert_eq!(
            BlockCoord::at(DVec3::new(-0.1, -1024.0, -1024.1)),
            BlockCoord(-1, -1, -2)
        );
    }

    #[test]
    fn geo_roundtrip_and_arcsecond_scale() {
        let geo = GeoReference::default();
        let p = geo
            .to_world(geo.latitude + 1.0 / 3600.0, geo.longitude - 1.0 / 3600.0)
            .unwrap();
        assert!((p - DVec3::new(-1024.0, 0.0, -1024.0)).length() < 1e-6);
        let gps = geo.to_gps(p);
        assert!((gps[0] - geo.latitude - 1.0 / 3600.0).abs() < 1e-10);
        assert!((gps[1] - geo.longitude + 1.0 / 3600.0).abs() < 1e-10);
    }

    #[test]
    fn ray_parallel_to_boundary_is_finite() {
        let b = Aabb::new(DVec3::ZERO, DVec3::ONE);
        assert_eq!(b.ray(DVec3::new(0.0, 0.5, -1.0), DVec3::Z), Some(1.0));
        assert_eq!(b.ray(DVec3::new(2.0, 0.5, -1.0), DVec3::Z), None);
    }
}
