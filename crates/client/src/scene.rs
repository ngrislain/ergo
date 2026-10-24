//! Backend-independent block scene planning, shared by every renderer entry point.
pub mod ground;
mod view;
use ergo_world::{Aabb, Brick, geometry::Mesh, spatial::Octree};
use glam::{DMat4, DVec3};
use std::collections::{BTreeMap, HashMap, HashSet};
pub use view::{Camera, DISPLAY_SCALE, Visibility};

/// Quality and visibility are explicit policies, independent of canvas alpha.
#[derive(Clone, Copy)]
pub struct RenderPolicy {
    pub full_detail: bool,
    pub cull_camera: bool,
}
impl RenderPolicy {
    pub const INTERACTIVE: Self = Self {
        full_detail: false,
        cull_camera: true,
    };
    pub const CAPTURE: Self = Self {
        full_detail: true,
        cull_camera: true,
    };
    pub const ILLUSTRATION: Self = Self {
        full_detail: true,
        cull_camera: false,
    };
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum GeometryKey {
    Part(String, u8),
    Aggregate,
    Ground,
    GroundStuds(u8),
}
pub type Instances = BTreeMap<GeometryKey, Vec<f32>>;

#[derive(Default)]
pub struct FramePlan {
    pub color: Instances,
    pub shadow: Instances,
    pub lod_counts: [usize; 4],
}

pub struct SceneOptions<'a> {
    pub policy: RenderPolicy,
    pub selected: Option<u32>,
    pub highlighted: &'a HashSet<u32>,
    pub explode: f64,
}

#[derive(Default)]
pub struct ScenePlanner {
    previous_lod: HashMap<u32, u8>,
}
impl ScenePlanner {
    pub fn clear(&mut self) {
        self.previous_lod.clear();
    }
    pub fn retain(&mut self, bricks: &[Brick]) {
        let ids: HashSet<_> = bricks.iter().map(|b| b.id).collect();
        self.previous_lod.retain(|id, _| ids.contains(id));
    }
    pub fn plan(
        &mut self,
        bricks: &[Brick],
        tree: &Octree,
        meshes: &BTreeMap<String, Mesh>,
        camera: &Camera,
        shadow: &Visibility,
        options: SceneOptions<'_>,
    ) -> FramePlan {
        let mut plan = FramePlan::default();
        let mut aggregates = BTreeMap::<(i64, i64, i64, u32), Aabb>::new();
        let candidates = if options.explode > 0. || !options.policy.cull_camera {
            (0..bricks.len()).collect()
        } else {
            tree.frustum(&camera.visibility.planes, camera.origin)
        };
        for index in candidates {
            let brick = &bricks[index];
            if !meshes.contains_key(&brick.part) {
                continue;
            }
            let offset = DVec3::Y * brick.bounds.center().y * options.explode;
            let bounds = Aabb::new(
                brick.render_bounds.min + offset,
                brick.render_bounds.max + offset,
            );
            if options.policy.cull_camera && !camera.visibility.contains(bounds) {
                continue;
            }
            let distance = ((bounds.center() - camera.eye) * DISPLAY_SCALE)
                .length()
                .max(0.1);
            let size =
                (bounds.size() * DISPLAY_SCALE).length() * camera.projection_pixels / distance;
            let rectangular = ergo_world::geometry::part_spec(&brick.part);
            let selected = options.selected == Some(brick.id);
            let highlighted = options.highlighted.contains(&brick.id);
            let mut lod = if options.policy.full_detail || selected {
                0
            } else {
                crate::lod::body_level(size, self.previous_lod.get(&brick.id).copied())
            };
            if highlighted {
                lod = lod.min(2);
            }
            // A box is not a silhouette-preserving simplification of an arbitrary
            // LDraw part. Only known rectangular catalog parts use body proxies.
            if rectangular.is_none() && lod < 3 {
                lod = 0;
            }
            let stud_pixels =
                camera.projection_pixels * 0.6 * (brick.transform.x_axis * DISPLAY_SCALE).length()
                    / distance;
            if lod == 2
                && stud_pixels > crate::lod::STUD_FLAT_PX
                && rectangular.is_some_and(|p| p.studs)
            {
                lod = 1;
            }
            self.previous_lod.insert(brick.id, lod);
            plan.lod_counts[lod as usize] += 1;
            if lod == 3 {
                let p = (bounds.center() / 8.).floor();
                aggregates
                    .entry((p.x as i64, p.y as i64, p.z as i64, brick.rgb))
                    .and_modify(|b| *b = b.union(bounds))
                    .or_insert(bounds);
            } else {
                instance(
                    plan.color
                        .entry(GeometryKey::Part(brick.part.clone(), lod))
                        .or_default(),
                    brick_matrix(brick, camera.origin, offset),
                    brick.rgb,
                    selected || highlighted,
                );
            }
        }
        for ((_, _, _, rgb), bounds) in aggregates {
            let position =
                DVec3::new(bounds.center().x, bounds.max.y, bounds.center().z) - camera.origin;
            instance(
                plan.color.entry(GeometryKey::Aggregate).or_default(),
                DMat4::from_scale_rotation_translation(
                    bounds.size().max(DVec3::splat(0.001)),
                    glam::DQuat::IDENTITY,
                    position,
                ),
                rgb,
                false,
            );
        }
        // Light-space traversal is independent of camera visibility. Resident
        // offscreen pieces still cast shadows; unloaded pieces are not invented.
        let casters = if options.explode > 0. {
            (0..bricks.len()).collect()
        } else {
            tree.frustum(&shadow.planes, shadow.origin)
        };
        for index in casters {
            let brick = &bricks[index];
            if !meshes.contains_key(&brick.part) {
                continue;
            }
            let offset = DVec3::Y * brick.bounds.center().y * options.explode;
            if !shadow.contains(Aabb::new(
                brick.render_bounds.min + offset,
                brick.render_bounds.max + offset,
            )) {
                continue;
            }
            instance(
                plan.shadow
                    .entry(GeometryKey::Part(brick.part.clone(), 0))
                    .or_default(),
                brick_matrix(brick, camera.origin, offset),
                brick.rgb,
                false,
            );
        }
        plan
    }
}

fn brick_matrix(brick: &Brick, origin: DVec3, offset: DVec3) -> DMat4 {
    DMat4::from_cols(
        brick.transform.x_axis.extend(0.),
        brick.transform.y_axis.extend(0.),
        brick.transform.z_axis.extend(0.),
        (brick.position - origin + offset).extend(1.),
    )
}
pub fn instance(out: &mut Vec<f32>, matrix: DMat4, rgb: u32, selected: bool) {
    out.extend(matrix.to_cols_array().iter().map(|v| *v as f32));
    out.extend_from_slice(&[
        ((rgb >> 16) & 255) as f32 / 255.,
        ((rgb >> 8) & 255) as f32 / 255.,
        (rgb & 255) as f32 / 255.,
        if selected { 1. } else { 0. },
    ]);
}

#[cfg(test)]
mod tests {
    use super::*;
    use ergo_world::demo::Demo;
    fn plan(demo: &Demo, camera: &Camera, shadow: &Visibility, policy: RenderPolicy) -> FramePlan {
        ScenePlanner::default().plan(
            &demo.bricks,
            &Octree::new(demo.bricks.iter().map(|b| b.render_bounds).collect()),
            &demo.meshes,
            camera,
            shadow,
            SceneOptions {
                policy,
                selected: None,
                highlighted: &HashSet::new(),
                explode: 0.,
            },
        )
    }
    #[test]
    fn offscreen_resident_brick_casts_a_shadow_without_a_color_instance() {
        let mut demo = Demo::new();
        demo.add("3003", 4, 0., 0., 0.);
        demo.add("3003", 2, 12., 0., 0.);
        let camera = Camera::new(
            DVec3::new(1., 3., 10.),
            DVec3::new(1., 1., 1.),
            800.,
            600.,
            30.,
            100.,
        );
        let light = Camera::new(
            DVec3::new(7., 25., 15.),
            DVec3::new(7., 1., 1.),
            1024.,
            1024.,
            90.,
            100.,
        );
        let result = plan(&demo, &camera, &light.visibility, RenderPolicy::CAPTURE);
        assert_eq!(
            result.color.values().map(|v| v.len() / 20).sum::<usize>(),
            1
        );
        assert_eq!(
            result.shadow.values().map(|v| v.len() / 20).sum::<usize>(),
            2
        );
        assert_eq!(
            result.shadow.len(),
            1,
            "Repeating parts must share a geometry batch"
        );
    }
    #[test]
    fn rebasing_preserves_visibility_and_center_pick_ray() {
        let eye = DVec3::new(4., 6., 20.);
        let target = DVec3::new(2., 1., 0.);
        let a = Camera::new(eye, target, 800., 600., 45., 100.);
        let delta = DVec3::splat(1024. * 1_000_000.);
        let b = Camera::new(eye + delta, target + delta, 800., 600., 45., 100.);
        let bounds = Aabb::new(target - DVec3::ONE, target + DVec3::ONE);
        assert!(a.visibility.contains(bounds));
        assert!(
            b.visibility
                .contains(Aabb::new(bounds.min + delta, bounds.max + delta))
        );
        let (p, ray) = b.ray(0., 0.);
        assert!((ray - (target - eye).normalize()).length() < 1e-10);
        assert!((p - (eye + delta)).cross(ray).length() < 1e-6);
    }
    #[test]
    fn custom_part_retains_canonical_geometry_at_medium_distance() {
        let mut demo = Demo::new();
        demo.add("3003", 4, 0., 0., 0.);
        let mut mesh = demo.meshes.remove("3003.dat").unwrap();
        mesh.part = "custom.dat".into();
        demo.bricks[0].part = mesh.part.clone();
        demo.meshes.insert(mesh.part.clone(), mesh);
        let camera = Camera::new(
            DVec3::new(1., 10., 160.),
            DVec3::new(1., 1., 1.),
            800.,
            600.,
            45.,
            1000.,
        );
        let result = plan(
            &demo,
            &camera,
            &camera.visibility,
            RenderPolicy::INTERACTIVE,
        );
        assert!(
            result
                .color
                .contains_key(&GeometryKey::Part("custom.dat".into(), 0))
        );
    }
    #[test]
    fn captures_use_full_meshes_and_illustrations_keep_all_color_instances() {
        let mut demo = Demo::new();
        demo.add("3003", 4, 0., 0., 0.);
        demo.add("3003", 2, 100., 0., 0.);
        let camera = Camera::new(
            DVec3::new(1., 5., 100.),
            DVec3::new(1., 1., 1.),
            800.,
            600.,
            30.,
            500.,
        );
        let capture = plan(&demo, &camera, &camera.visibility, RenderPolicy::CAPTURE);
        assert!(
            capture
                .color
                .keys()
                .all(|key| matches!(key, GeometryKey::Part(_, 0)))
        );
        let illustration = plan(
            &demo,
            &camera,
            &camera.visibility,
            RenderPolicy::ILLUSTRATION,
        );
        assert_eq!(
            illustration
                .color
                .values()
                .map(|v| v.len() / 20)
                .sum::<usize>(),
            2
        );
    }
}
