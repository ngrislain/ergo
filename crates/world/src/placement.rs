//! Shared CAD grid placement. World/view positions remain continuous; placement
//! snaps one coherent translation, never each constituent brick independently.
use crate::{
    Aabb, WorldExport,
    assembly::{Model, Transform},
    geometry::Mesh,
};
use anyhow::{Result, ensure};
use glam::{DMat3, DVec3};

pub fn snap_point(p: DVec3) -> DVec3 {
    DVec3::new(p.x.round(), (p.y * 3.).round() / 3., p.z.round())
}

/// Nominal geometry, not the cosmetic gaps or source-mesh extrema.
pub fn body_bounds(mesh: &Mesh) -> Aabb {
    mesh.physical
        .as_ref()
        .map(|p| p.body_bounds)
        .or_else(|| crate::geometry::nominal_body_bounds(&mesh.part))
        .unwrap_or(mesh.body_bounds)
}

pub fn snapping_delta(bounds: Aabb) -> DVec3 {
    // Internal +Z is south: max.z is the public minimum-north corner.
    let corner = DVec3::new(bounds.min.x, bounds.min.y, bounds.max.z);
    let delta = snap_point(corner) - corner;
    delta.map(|v| if v.abs() < 1e-9 { 0. } else { v })
}

pub fn snap_translation(bounds: Aabb, translation: DVec3) -> DVec3 {
    (translation
        + snapping_delta(Aabb::new(
            bounds.min + translation,
            bounds.max + translation,
        )))
    .map(|v| if v.abs() < 1e-9 { 0. } else { v })
}

/// Translate all poses and dependent frames coherently, retaining a current
/// model hierarchy. Callers enforce edit permissions/anchor restrictions.
pub fn translate_scene(scene: &mut WorldExport, delta: DVec3) -> Result<()> {
    ensure!(delta.is_finite(), "Invalid placement translation");
    if delta == DVec3::ZERO {
        return Ok(());
    }
    let mut model = Model::for_scene(scene);
    model.apply_transform(Transform::from_pose(
        DMat3::IDENTITY,
        crate::ldraw::conversion().inverse() * delta,
    ))?;
    for brick in &mut scene.bricks {
        brick.translated(delta);
    }
    for joint in &mut scene.joints {
        joint.anchor += delta;
    }
    for group in &mut scene.groups {
        group.pivot_world += delta;
    }
    scene.model = Some(model);
    Ok(())
}

pub fn snap_scene(scene: &mut WorldExport) -> Result<DVec3> {
    let bounds = scene
        .bricks
        .iter()
        .map(|b| {
            scene.meshes.get(&b.part).map_or(b.bounds, |mesh| {
                body_bounds(mesh).transformed(b.transform, b.position)
            })
        })
        .reduce(Aabb::union);
    let delta = bounds.map_or(DVec3::ZERO, snapping_delta);
    translate_scene(scene, delta)?;
    Ok(delta)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn snaps_a_whole_scene_preserving_internal_frames_and_hierarchy() {
        let mut d = crate::demo::Demo::new();
        d.add("3003", 19, 11303.201454471395, 0., -76.47131723361423);
        d.add("3003", 19, 11303.201454471395, 1., -76.47131723361423);
        let mut scene = WorldExport {
            schema: crate::SCHEMA_VERSION,
            name: "offset".into(),
            geo: Default::default(),
            joints: crate::constraints::inferred_joints(&d.bricks),
            bricks: d.bricks,
            meshes: d.meshes,
            fixed_bricks: vec![],
            groups: vec![],
            model: None,
            ground: vec![],
        };
        scene.model = Some(Model::from_bricks(&scene.bricks));
        let old = scene.clone();
        let delta = snap_scene(&mut scene).unwrap();
        assert!((delta - DVec3::new(-0.201454471395, 0., 0.47131723361423)).length() < 1e-9);
        assert_eq!(
            scene.bricks[1].position - scene.bricks[0].position,
            old.bricks[1].position - old.bricks[0].position
        );
        assert!((scene.joints[0].anchor - old.joints[0].anchor - delta).length() < 1e-10);
        assert!(scene.model.as_ref().unwrap().matches(&scene.bricks));
        assert_eq!(snap_scene(&mut scene).unwrap(), DVec3::ZERO);
    }
    #[test]
    fn negative_coordinates_and_plate_heights_snap_deterministically() {
        assert_eq!(
            snap_point(DVec3::new(-2.6, 0.4, 3.49)),
            DVec3::new(-3., 1. / 3., 3.)
        );
        assert_eq!(
            snap_point(DVec3::new(-2.5, 0., 2.5)),
            DVec3::new(-3., 0., 3.)
        );
    }
}
