//! Public/persisted schema-2 coordinates are east/north/up: X east, Y north,
//! Z height. The render/physics kernel keeps its Y-up convention internally.
//! All interchange vectors, matrices, and meshes cross these explicit adapters.
use glam::{DMat3, DVec3};
use serde::{Deserialize, Deserializer, Serialize, Serializer, ser::SerializeSeq};

pub fn public(p: DVec3) -> DVec3 {
    DVec3::new(p.x, -p.z, p.y)
}
pub fn internal(p: DVec3) -> DVec3 {
    DVec3::new(p.x, p.z, -p.y)
}
pub fn dimensions(p: DVec3) -> DVec3 {
    DVec3::new(p.x, p.z, p.y)
}
fn permutation() -> DMat3 {
    DMat3::from_cols(DVec3::X, DVec3::Z, DVec3::NEG_Y)
}
pub fn public_matrix(m: DMat3) -> DMat3 {
    permutation() * m * permutation().transpose()
}
pub fn internal_matrix(m: DMat3) -> DMat3 {
    permutation().transpose() * m * permutation()
}

#[derive(Clone, Copy, Serialize, Deserialize)]
pub struct BoundsWire {
    min: [f64; 3],
    max: [f64; 3],
}
impl From<crate::Aabb> for BoundsWire {
    fn from(b: crate::Aabb) -> Self {
        Self {
            min: [b.min.x, -b.max.z, b.min.y],
            max: [b.max.x, -b.min.z, b.max.y],
        }
    }
}
impl From<BoundsWire> for crate::Aabb {
    fn from(b: BoundsWire) -> Self {
        Self::new(
            DVec3::new(b.min[0], b.min[2], -b.max[1]),
            DVec3::new(b.max[0], b.max[2], -b.min[1]),
        )
    }
}

pub mod vec3 {
    use super::*;
    pub fn serialize<S: Serializer>(p: &DVec3, s: S) -> Result<S::Ok, S::Error> {
        public(*p).to_array().serialize(s)
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<DVec3, D::Error> {
        Ok(internal(DVec3::from(<[f64; 3]>::deserialize(d)?)))
    }
}
pub mod mat3 {
    use super::*;
    pub fn serialize<S: Serializer>(m: &DMat3, s: S) -> Result<S::Ok, S::Error> {
        public_matrix(*m).to_cols_array().serialize(s)
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<DMat3, D::Error> {
        Ok(internal_matrix(DMat3::from_cols_array(
            &<[f64; 9]>::deserialize(d)?,
        )))
    }
}
pub mod axis {
    use super::*;
    pub fn serialize<S: Serializer>(p: &DVec3, s: S) -> Result<S::Ok, S::Error> {
        vec3::serialize(p, s)
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<DVec3, D::Error> {
        vec3::deserialize(d)
    }
}
pub mod mesh_vertices {
    use super::*;
    // A proper rotation (east/up/south -> east/north/up) preserves handedness,
    // triangle winding, chiral geometry, normals, and positive rotation angles.
    pub fn serialize<S: Serializer>(vertices: &[f32], s: S) -> Result<S::Ok, S::Error> {
        if !vertices.len().is_multiple_of(27) {
            return Err(serde::ser::Error::custom("Incomplete mesh triangles"));
        }
        let mut seq = s.serialize_seq(Some(vertices.len()))?;
        for vertex in vertices.chunks_exact(9) {
            for value in [
                vertex[0], -vertex[2], vertex[1], vertex[3], -vertex[5], vertex[4], vertex[6],
                vertex[7], vertex[8],
            ] {
                seq.serialize_element(&value)?;
            }
        }
        seq.end()
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<f32>, D::Error> {
        let vertices = Vec::<f32>::deserialize(d)?;
        if !vertices.len().is_multiple_of(27) {
            return Err(serde::de::Error::custom("Incomplete mesh triangles"));
        }
        let mut out = Vec::with_capacity(vertices.len());
        for vertex in vertices.chunks_exact(9) {
            out.extend_from_slice(&[
                vertex[0], vertex[2], -vertex[1], vertex[3], vertex[5], -vertex[4], vertex[6],
                vertex[7], vertex[8],
            ]);
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn interchange_is_z_up_and_mesh_roundtrips_without_loss() {
        let mut d = crate::demo::Demo::new();
        d.add("3003", 4, 2.0, 0.0, 8.0);
        let value = serde_json::to_value(&d.bricks[0]).unwrap();
        assert_eq!(value["bounds"]["min"], serde_json::json!([2.0, -10.0, 0.0]));
        let decoded: crate::Brick = serde_json::from_value(value).unwrap();
        assert_eq!(decoded.position, d.bricks[0].position);
        let mesh = &d.meshes["3003.dat"];
        let data = serde_json::to_vec(mesh).unwrap();
        let decoded: crate::geometry::Mesh = serde_json::from_slice(&data).unwrap();
        assert_eq!(mesh.vertices, decoded.vertices);
        let rotation = DMat3::from_rotation_x(0.3);
        assert!(
            (internal_matrix(public_matrix(rotation)) - rotation)
                .to_cols_array()
                .iter()
                .all(|x| x.abs() < 1e-12)
        );
        assert!((permutation().determinant() - 1.0).abs() < 1e-12);
    }
}
