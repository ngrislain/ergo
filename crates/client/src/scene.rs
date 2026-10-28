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
    pub triangle_budget: Option<usize>,
}
impl RenderPolicy {
    pub const INTERACTIVE: Self = Self {
        full_detail: false,
        cull_camera: true,
        triangle_budget: Some(crate::lod::INTERACTIVE_TRIANGLE_BUDGET),
    };
    pub const CAPTURE: Self = Self {
        full_detail: true,
        cull_camera: true,
        triangle_budget: None,
    };
    pub const DETAILED_INTERACTIVE: Self = Self {
        full_detail: false,
        cull_camera: true,
        triangle_budget: None,
    };
    pub const PERFORMANCE: Self = Self {
        full_detail: false,
        cull_camera: true,
        triangle_budget: Some(500_000),
    };
    pub const ILLUSTRATION: Self = Self {
        full_detail: true,
        cull_camera: false,
        triangle_budget: None,
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
    pub shadow_lod_counts: [usize; 3],
    pub detail_scale: f64,
    pub estimated_color_triangles: usize,
    pub estimated_shadow_triangles: usize,
    pub budget_exceeded: bool,
}

pub struct SceneOptions<'a> {
    pub policy: RenderPolicy,
    pub selected: Option<u32>,
    pub highlighted: &'a HashSet<u32>,
    pub explode: f64,
    pub shadow_resolution: Option<f64>,
    /// The caller must invalidate with retain/clear when scene data changes.
    pub reuse_shadow: bool,
}

struct ShadowCache {
    visibility: Visibility,
    resolution: Option<f64>,
    full_detail: bool,
    selected: Option<u32>,
    highlighted: HashSet<u32>,
    explode: f64,
    instances: Instances,
    lod_counts: [usize; 3],
    triangles: usize,
}

struct Candidate {
    index: usize,
    bounds: Aabb,
    offset: DVec3,
    pixels: f64,
    stud_pixels: f64,
    detail: PartDetail,
}

#[derive(Clone, Copy)]
struct PartDetail {
    part: Option<crate::part_lod::RenderPart>,
    triangles: [usize; 3],
}
impl PartDetail {
    fn cost(self, level: u8) -> usize {
        if level == 3 {
            12
        } else {
            self.triangles[level as usize]
        }
    }
}

#[derive(Default)]
pub struct ScenePlanner {
    previous_lod: HashMap<u32, u8>,
    previous_shadow_lod: HashMap<u32, u8>,
    part_details: HashMap<String, PartDetail>,
    detail_scale: f64,
    shadow_cache: Option<ShadowCache>,
}
impl ScenePlanner {
    pub fn clear(&mut self) {
        self.previous_lod.clear();
        self.previous_shadow_lod.clear();
        self.part_details.clear();
        self.detail_scale = 1.;
        self.shadow_cache = None;
    }
    pub fn retain(&mut self, bricks: &[Brick]) {
        self.shadow_cache = None;
        let ids: HashSet<_> = bricks.iter().map(|b| b.id).collect();
        self.previous_lod.retain(|id, _| ids.contains(id));
        self.previous_shadow_lod.retain(|id, _| ids.contains(id));
    }
    pub fn invalidate_meshes(&mut self) {
        self.shadow_cache = None;
        self.part_details.clear();
    }
    fn detail(&mut self, part: &str, mesh: &Mesh) -> PartDetail {
        *self.part_details.entry(part.into()).or_insert_with(|| {
            let profile = crate::part_lod::RenderPart::resolve(mesh);
            let source = mesh.vertices.len() / 27;
            PartDetail {
                part: profile,
                triangles: profile.map_or([source; 3], |part| {
                    [
                        source,
                        part.mesh(1).vertices.len() / 27,
                        part.mesh(2).vertices.len() / 27,
                    ]
                }),
            }
        })
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
        let mut visible = Vec::new();
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
            let pixels =
                (bounds.size() * DISPLAY_SCALE).length() * camera.projection_pixels / distance;
            let stud_pixels =
                camera.projection_pixels * 0.6 * (brick.transform.x_axis * DISPLAY_SCALE).length()
                    / distance;
            visible.push(Candidate {
                index,
                bounds,
                offset,
                pixels,
                stud_pixels,
                detail: self.detail(&brick.part, &meshes[&brick.part]),
            });
        }
        let scale = if options.policy.full_detail {
            1.
        } else if let Some(budget) = options.policy.triangle_budget {
            crate::lod::budget_scale(self.detail_scale.max(1.), budget, |scale| {
                visible.iter().fold(0usize, |total, candidate| {
                    let brick = &bricks[candidate.index];
                    let lod = color_level(
                        brick,
                        candidate,
                        scale,
                        self.previous_lod.get(&brick.id).copied(),
                        &options,
                    );
                    total.saturating_add(candidate.detail.cost(lod))
                })
            })
        } else {
            1.
        };
        self.detail_scale = scale;
        plan.detail_scale = scale;
        for candidate in visible {
            let brick = &bricks[candidate.index];
            let bounds = candidate.bounds;
            let offset = candidate.offset;
            let selected = options.selected == Some(brick.id);
            let highlighted = options.highlighted.contains(&brick.id);
            let lod = color_level(
                brick,
                &candidate,
                scale,
                self.previous_lod.get(&brick.id).copied(),
                &options,
            );
            plan.estimated_color_triangles = plan
                .estimated_color_triangles
                .saturating_add(candidate.detail.cost(lod));
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
        plan.budget_exceeded = options
            .policy
            .triangle_budget
            .is_some_and(|budget| plan.estimated_color_triangles > budget);
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
        if options.reuse_shadow
            && let Some(cached) = &self.shadow_cache
            && cached.visibility == *shadow
            && cached.resolution == options.shadow_resolution
            && cached.full_detail == options.policy.full_detail
            && cached.selected == options.selected
            && cached.highlighted == *options.highlighted
            && cached.explode == options.explode
        {
            plan.shadow = cached.instances.clone();
            plan.shadow_lod_counts = cached.lod_counts;
            plan.estimated_shadow_triangles = cached.triangles;
            return plan;
        }
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
            let bounds = Aabb::new(
                brick.render_bounds.min + offset,
                brick.render_bounds.max + offset,
            );
            if !shadow.contains(bounds) {
                continue;
            }
            let detail = self.detail(&brick.part, &meshes[&brick.part]);
            let lod = if options.policy.full_detail
                || options.selected == Some(brick.id)
                || options.highlighted.contains(&brick.id)
                || detail.part.is_none()
            {
                0
            } else if let Some(resolution) = options.shadow_resolution {
                let pixels = shadow.projected_diameter(bounds, resolution) * 0.6
                    / (bounds.size() * DISPLAY_SCALE).length().max(0.1);
                crate::lod::shadow_level(pixels, self.previous_shadow_lod.get(&brick.id).copied())
            } else {
                0
            };
            self.previous_shadow_lod.insert(brick.id, lod);
            plan.shadow_lod_counts[lod as usize] += 1;
            plan.estimated_shadow_triangles = plan
                .estimated_shadow_triangles
                .saturating_add(detail.cost(lod));
            instance(
                plan.shadow
                    .entry(GeometryKey::Part(brick.part.clone(), lod))
                    .or_default(),
                brick_matrix(brick, camera.origin, offset),
                brick.rgb,
                false,
            );
        }
        if options.reuse_shadow {
            self.shadow_cache = Some(ShadowCache {
                visibility: shadow.clone(),
                resolution: options.shadow_resolution,
                full_detail: options.policy.full_detail,
                selected: options.selected,
                highlighted: options.highlighted.clone(),
                explode: options.explode,
                instances: plan.shadow.clone(),
                lod_counts: plan.shadow_lod_counts,
                triangles: plan.estimated_shadow_triangles,
            });
        }
        plan
    }
}

fn color_level(
    brick: &Brick,
    candidate: &Candidate,
    scale: f64,
    previous: Option<u8>,
    options: &SceneOptions<'_>,
) -> u8 {
    let mut lod = if options.policy.full_detail || options.selected == Some(brick.id) {
        0
    } else {
        crate::lod::body_level(candidate.pixels / scale, previous)
    };
    if options.highlighted.contains(&brick.id) {
        lod = lod.min(2);
    }
    // A scene-wide bias may simplify a visible brick, but must not merge it
    // into a spatial bucket while its actual silhouette is still resolvable.
    // Merging larger wall/window pieces creates overlapping colored envelopes.
    if lod == 3 && crate::lod::body_level(candidate.pixels, previous) < 3 {
        lod = 2;
    }
    if candidate.detail.part.is_none() && lod < 3 {
        lod = 0;
    }
    // Preserve the existing geometric-to-analytic stud transition, even when
    // that makes the global triangle target unattainable at close range.
    if lod == 2
        && candidate.stud_pixels > crate::lod::STUD_FLAT_PX
        && candidate.detail.part.is_some_and(|part| part.studded())
    {
        lod = 1;
    }
    lod
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
                shadow_resolution: Some(1024.),
                reuse_shadow: false,
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
    #[test]
    fn global_budget_reduces_district_like_repetition_and_keeps_selection_detailed() {
        let mut demo = Demo::new();
        for index in 0..1000 {
            demo.add(
                "3007",
                4,
                (index % 40) as f64 * 10.,
                0.,
                (index / 40) as f64 * 4.,
            );
        }
        let camera = Camera::new(
            DVec3::new(200., 300., 850.),
            DVec3::new(200., 0., 50.),
            1280.,
            720.,
            45.,
            4000.,
        );
        let baseline = plan(
            &demo,
            &camera,
            &camera.visibility,
            RenderPolicy {
                triangle_budget: None,
                ..RenderPolicy::INTERACTIVE
            },
        );
        let result = plan(
            &demo,
            &camera,
            &camera.visibility,
            RenderPolicy {
                triangle_budget: Some(100_000),
                ..RenderPolicy::INTERACTIVE
            },
        );
        assert!(result.detail_scale > 1.);
        assert!(result.estimated_color_triangles < baseline.estimated_color_triangles);
        assert_eq!(
            result.lod_counts[3], 0,
            "A global budget must not aggregate still-resolvable bricks"
        );
        let result = ScenePlanner::default().plan(
            &demo.bricks,
            &Octree::new(demo.bricks.iter().map(|b| b.render_bounds).collect()),
            &demo.meshes,
            &camera,
            &camera.visibility,
            SceneOptions {
                policy: RenderPolicy {
                    triangle_budget: Some(1),
                    ..RenderPolicy::INTERACTIVE
                },
                selected: Some(1),
                highlighted: &HashSet::new(),
                explode: 0.,
                shadow_resolution: Some(16.),
                reuse_shadow: false,
            },
        );
        assert_eq!(
            result.color[&GeometryKey::Part("3007.dat".into(), 0)].len() / 20,
            1
        );
        assert!(
            result.budget_exceeded,
            "Pinned geometry must not be removed to fake a hard budget"
        );
    }
    #[test]
    fn interactive_shadow_lod_keeps_offscreen_casters_and_capture_quality() {
        let mut demo = Demo::new();
        demo.add("3007", 4, 0., 0., 0.);
        demo.add("3007", 4, 100., 0., 0.);
        let camera = Camera::new(
            DVec3::new(4., 6., 20.),
            DVec3::new(4., 1., 1.),
            800.,
            600.,
            30.,
            1000.,
        );
        let light = Camera::new(
            DVec3::new(50., 500., 500.),
            DVec3::new(50., 0., 0.),
            256.,
            256.,
            90.,
            2000.,
        );
        let tree = Octree::new(demo.bricks.iter().map(|b| b.render_bounds).collect());
        let result = ScenePlanner::default().plan(
            &demo.bricks,
            &tree,
            &demo.meshes,
            &camera,
            &light.visibility,
            SceneOptions {
                policy: RenderPolicy::INTERACTIVE,
                selected: None,
                highlighted: &HashSet::new(),
                explode: 0.,
                shadow_resolution: Some(256.),
                reuse_shadow: false,
            },
        );
        assert_eq!(
            result.shadow.values().map(|v| v.len() / 20).sum::<usize>(),
            2
        );
        assert!(
            result
                .shadow
                .contains_key(&GeometryKey::Part("3007.dat".into(), 2))
        );
        let capture = plan(&demo, &camera, &light.visibility, RenderPolicy::CAPTURE);
        assert!(
            capture
                .shadow
                .keys()
                .all(|key| matches!(key, GeometryKey::Part(_, 0)))
        );
        assert!(result.estimated_shadow_triangles < capture.estimated_shadow_triangles);
    }
    #[test]
    fn shadow_cache_reuses_static_lighting_but_rebuilds_for_pose_and_selection() {
        let mut demo = Demo::new();
        demo.add("3007", 4, 0., 0., 0.);
        let camera = Camera::new(
            DVec3::new(4., 6., 30.),
            DVec3::new(4., 1., 1.),
            800.,
            600.,
            45.,
            1000.,
        );
        let light = Camera::new(
            DVec3::new(4., 500., 500.),
            DVec3::new(4., 0., 0.),
            256.,
            256.,
            90.,
            2000.,
        );
        let mut planner = ScenePlanner::default();
        let highlighted = HashSet::new();
        let options = |selected| SceneOptions {
            policy: RenderPolicy::INTERACTIVE,
            selected,
            highlighted: &highlighted,
            explode: 0.,
            shadow_resolution: Some(256.),
            reuse_shadow: true,
        };
        let tree = Octree::new(demo.bricks.iter().map(|b| b.render_bounds).collect());
        let first = planner.plan(
            &demo.bricks,
            &tree,
            &demo.meshes,
            &camera,
            &light.visibility,
            options(None),
        );
        let second = planner.plan(
            &demo.bricks,
            &tree,
            &demo.meshes,
            &camera,
            &light.visibility,
            options(None),
        );
        assert_eq!(first.shadow, second.shadow);
        let selected = planner.plan(
            &demo.bricks,
            &tree,
            &demo.meshes,
            &camera,
            &light.visibility,
            options(Some(1)),
        );
        assert!(
            selected
                .shadow
                .contains_key(&GeometryKey::Part("3007.dat".into(), 0))
        );
        demo.bricks[0].position.x += 2.;
        demo.bricks[0].bounds.min.x += 2.;
        demo.bricks[0].bounds.max.x += 2.;
        demo.bricks[0].render_bounds.min.x += 2.;
        demo.bricks[0].render_bounds.max.x += 2.;
        planner.retain(&demo.bricks);
        let tree = Octree::new(demo.bricks.iter().map(|b| b.render_bounds).collect());
        let moved = planner.plan(
            &demo.bricks,
            &tree,
            &demo.meshes,
            &camera,
            &light.visibility,
            options(None),
        );
        assert_ne!(first.shadow, moved.shadow);
    }
}
