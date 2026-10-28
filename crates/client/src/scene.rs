//! Backend-independent block scene planning, shared by every renderer entry point.
pub mod ground;
mod surfaces;
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
    Surface(usize),
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
    pub shadow_detail_scale: f64,
    pub shadow_budget_exceeded: bool,
    pub surface_pieces: usize,
}

pub struct SceneOptions<'a> {
    pub policy: RenderPolicy,
    pub selected: Option<u32>,
    pub highlighted: &'a HashSet<u32>,
    pub explode: f64,
    pub shadow_resolution: Option<f64>,
    /// The caller must invalidate with retain/clear when scene data changes.
    pub reuse_shadow: bool,
    /// Diagnostic/entry-point control; full-detail policies always bypass it.
    pub merge_surfaces: bool,
}

struct ShadowCache {
    visibility: Visibility,
    resolution: Option<f64>,
    full_detail: bool,
    budget: Option<usize>,
    selected: Option<u32>,
    highlighted: HashSet<u32>,
    explode: f64,
    instances: Instances,
    lod_counts: [usize; 3],
    triangles: usize,
    detail_scale: f64,
    budget_exceeded: bool,
    merge_surfaces: bool,
}

struct Candidate {
    index: usize,
    bounds: Aabb,
    offset: DVec3,
    pixels: f64,
    stud_pixels: f64,
    detail: PartDetail,
    previous: Option<u8>,
}

struct SurfaceView {
    visible: bool,
    pixels: f64,
    stud_pixels: f64,
}
fn surface_allowed(chunk: &surfaces::Chunk, bricks: &[Brick], options: &SceneOptions<'_>) -> bool {
    options.explode == 0.
        && options.merge_surfaces
        && !options.policy.full_detail
        && options.policy.triangle_budget.is_some()
        && (options.selected.is_none() && options.highlighted.is_empty()
            || !chunk.members.iter().any(|&index| {
                options.selected == Some(bricks[index].id)
                    || options.highlighted.contains(&bricks[index].id)
            }))
}
fn surface_scale(options: &SceneOptions<'_>) -> f64 {
    if options
        .policy
        .triangle_budget
        .is_some_and(|budget| budget <= 500_000)
    {
        8.
    } else {
        4.
    }
}
fn color_surfaces(
    surfaces: Option<&surfaces::Surfaces>,
    camera: &Camera,
    bricks: &[Brick],
    options: &SceneOptions<'_>,
) -> Vec<bool> {
    let views: Vec<_> = surfaces
        .into_iter()
        .flat_map(|surfaces| &surfaces.chunks)
        .map(|chunk| {
            let distance = ((camera.eye - camera.eye.clamp(chunk.bounds.min, chunk.bounds.max))
                * DISPLAY_SCALE)
                .length()
                .max(0.1);
            SurfaceView {
                visible: surface_allowed(chunk, bricks, options)
                    && camera.visibility.contains(chunk.bounds),
                pixels: chunk.max_diameter * camera.projection_pixels / distance,
                stud_pixels: camera.projection_pixels * 0.6 / distance,
            }
        })
        .collect();
    surface_flags(&views, surface_scale(options), false)
}
fn shadow_surfaces(
    surfaces: Option<&surfaces::Surfaces>,
    shadow: &Visibility,
    bricks: &[Brick],
    options: &SceneOptions<'_>,
) -> Vec<bool> {
    let views: Vec<_> = surfaces
        .into_iter()
        .flat_map(|surfaces| &surfaces.chunks)
        .map(|chunk| SurfaceView {
            visible: surface_allowed(chunk, bricks, options) && shadow.contains(chunk.bounds),
            pixels: 0.,
            stud_pixels: options
                .shadow_resolution
                .map_or(f64::INFINITY, |resolution| {
                    shadow.projected_diameter(chunk.bounds, resolution) * 0.6
                        / (chunk.bounds.size() * DISPLAY_SCALE).length().max(0.1)
                }),
        })
        .collect();
    surface_flags(&views, surface_scale(options), true)
}
fn surface_flags(views: &[SurfaceView], scale: f64, shadow: bool) -> Vec<bool> {
    views
        .iter()
        .map(|view| {
            view.visible
                && if shadow {
                    view.stud_pixels / scale < crate::lod::STUD_FLAT_PX * 0.88
                } else {
                    view.pixels / scale < 8. * 0.88
                        && view.stud_pixels < crate::lod::STUD_FLAT_PX * 0.88
                }
        })
        .collect()
}
fn is_merged(index: usize, surfaces: Option<&surfaces::Surfaces>, flags: &[bool]) -> bool {
    surfaces
        .and_then(|surfaces| surfaces.membership[index])
        .is_some_and(|chunk| flags[chunk])
}
fn surface_cost(surfaces: Option<&surfaces::Surfaces>, flags: &[bool]) -> usize {
    surfaces.map_or(0, |surfaces| {
        surfaces
            .chunks
            .iter()
            .zip(flags)
            .filter(|(_, active)| **active)
            .map(|(chunk, _)| chunk.mesh.vertices.len() / 27)
            .sum()
    })
}
fn add_surfaces(
    instances: &mut Instances,
    surfaces: Option<&surfaces::Surfaces>,
    flags: &[bool],
    origin: DVec3,
) -> usize {
    let mut pieces = 0;
    if let Some(surfaces) = surfaces {
        for (index, chunk) in surfaces.chunks.iter().enumerate() {
            if flags[index] {
                instance(
                    instances.entry(GeometryKey::Surface(index)).or_default(),
                    DMat4::from_translation(chunk.origin - origin),
                    0xffffff,
                    false,
                );
                pieces += chunk.members.len();
            }
        }
    }
    pieces
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
    shadow_detail_scale: f64,
    shadow_cache: Option<ShadowCache>,
    surfaces: Option<surfaces::Surfaces>,
}
impl ScenePlanner {
    pub fn reset_detail(&mut self) {
        self.previous_lod.clear();
        self.previous_shadow_lod.clear();
        self.detail_scale = 1.;
        self.shadow_cache = None;
        self.shadow_detail_scale = 1.;
    }
    pub fn clear(&mut self) {
        self.reset_detail();
        self.part_details.clear();
        self.surfaces = None;
    }
    pub fn retain(&mut self, bricks: &[Brick]) {
        self.shadow_cache = None;
        self.surfaces = None;
        let ids: HashSet<_> = bricks.iter().map(|b| b.id).collect();
        self.previous_lod.retain(|id, _| ids.contains(id));
        self.previous_shadow_lod.retain(|id, _| ids.contains(id));
    }
    pub fn invalidate_meshes(&mut self) {
        self.shadow_cache = None;
        self.part_details.clear();
        self.surfaces = None;
    }
    pub fn surface_mesh(&self, index: usize) -> &Mesh {
        &self.surfaces.as_ref().unwrap().chunks[index].mesh
    }
    fn detail(&mut self, part: &str, mesh: &Mesh) -> PartDetail {
        if let Some(detail) = self.part_details.get(part) {
            return *detail;
        }
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
        // Large static regions only: dynamic simulation is bounded to 5,000
        // bodies and must not rebuild derived chunk surfaces each solver tick.
        if self.surfaces.is_none()
            && bricks.len() >= 10_000
            && options.policy.triangle_budget.is_some()
            && !options.policy.full_detail
            && options.merge_surfaces
            && options.explode == 0.
        {
            self.surfaces = Some(surfaces::Surfaces::build(bricks, meshes));
        }
        let flags = color_surfaces(self.surfaces.as_ref(), camera, bricks, &options);
        let mut aggregates = BTreeMap::<(i64, i64, i64, u32), Aabb>::new();
        let candidates = if options.explode > 0. || !options.policy.cull_camera {
            (0..bricks.len()).collect()
        } else {
            tree.frustum(&camera.visibility.planes, camera.origin)
        };
        let mut visible = Vec::new();
        for index in candidates {
            if is_merged(index, self.surfaces.as_ref(), &flags) {
                continue;
            }
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
                previous: self.previous_lod.get(&brick.id).copied(),
            });
        }
        let surfaces = self.surfaces.as_ref();
        let scale = if options.policy.full_detail {
            1.
        } else if let Some(budget) = options.policy.triangle_budget {
            crate::lod::budget_scale(self.detail_scale.max(1.), budget, |scale| {
                visible
                    .iter()
                    .fold(surface_cost(surfaces, &flags), |total, candidate| {
                        if is_merged(candidate.index, surfaces, &flags) {
                            return total;
                        }
                        let brick = &bricks[candidate.index];
                        let lod =
                            color_level(brick, candidate, scale, candidate.previous, &options);
                        total.saturating_add(candidate.detail.cost(lod))
                    })
            })
        } else {
            1.
        };
        self.detail_scale = scale;
        plan.detail_scale = scale;
        plan.surface_pieces = add_surfaces(&mut plan.color, surfaces, &flags, camera.origin);
        plan.lod_counts[2] += plan.surface_pieces;
        plan.estimated_color_triangles = surface_cost(surfaces, &flags);
        for candidate in visible {
            if is_merged(candidate.index, surfaces, &flags) {
                continue;
            }
            let brick = &bricks[candidate.index];
            let bounds = candidate.bounds;
            let offset = candidate.offset;
            let selected = options.selected == Some(brick.id);
            let highlighted = options.highlighted.contains(&brick.id);
            let lod = color_level(brick, &candidate, scale, candidate.previous, &options);
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
            && cached.budget == options.policy.triangle_budget
            && cached.merge_surfaces == options.merge_surfaces
            && cached.selected == options.selected
            && cached.highlighted == *options.highlighted
            && cached.explode == options.explode
        {
            plan.shadow = cached.instances.clone();
            plan.shadow_lod_counts = cached.lod_counts;
            plan.estimated_shadow_triangles = cached.triangles;
            plan.shadow_detail_scale = cached.detail_scale;
            plan.shadow_budget_exceeded = cached.budget_exceeded;
            return plan;
        }
        let flags = shadow_surfaces(self.surfaces.as_ref(), shadow, bricks, &options);
        let casters = if options.explode > 0. {
            (0..bricks.len()).collect()
        } else {
            tree.frustum(&shadow.planes, shadow.origin)
        };
        let mut shadow_candidates = Vec::new();
        for index in casters {
            if is_merged(index, self.surfaces.as_ref(), &flags) {
                continue;
            }
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
            let pixels = options
                .shadow_resolution
                .map_or(f64::INFINITY, |resolution| {
                    shadow.projected_diameter(bounds, resolution) * 0.6
                        / (bounds.size() * DISPLAY_SCALE).length().max(0.1)
                });
            shadow_candidates.push(Candidate {
                index,
                bounds,
                offset,
                pixels,
                stud_pixels: pixels,
                detail,
                previous: self.previous_shadow_lod.get(&brick.id).copied(),
            });
        }
        let surfaces = self.surfaces.as_ref();
        let scale = if let Some(budget) = options
            .policy
            .triangle_budget
            .filter(|_| !options.policy.full_detail)
        {
            crate::lod::budget_scale(self.shadow_detail_scale.max(1.), budget, |scale| {
                shadow_candidates
                    .iter()
                    .fold(surface_cost(surfaces, &flags), |sum, candidate| {
                        if is_merged(candidate.index, surfaces, &flags) {
                            sum
                        } else {
                            sum.saturating_add(candidate.detail.cost(shadow_level(
                                &bricks[candidate.index],
                                candidate,
                                scale,
                                &options,
                            )))
                        }
                    })
            })
        } else {
            1.
        };
        self.shadow_detail_scale = scale;
        plan.shadow_detail_scale = scale;
        plan.shadow_lod_counts[2] +=
            add_surfaces(&mut plan.shadow, surfaces, &flags, camera.origin);
        plan.estimated_shadow_triangles = surface_cost(surfaces, &flags);
        for candidate in shadow_candidates {
            if is_merged(candidate.index, surfaces, &flags) {
                continue;
            }
            let brick = &bricks[candidate.index];
            let lod = shadow_level(brick, &candidate, scale, &options);
            self.previous_shadow_lod.insert(brick.id, lod);
            plan.shadow_lod_counts[lod as usize] += 1;
            plan.estimated_shadow_triangles = plan
                .estimated_shadow_triangles
                .saturating_add(candidate.detail.cost(lod));
            instance(
                plan.shadow
                    .entry(GeometryKey::Part(brick.part.clone(), lod))
                    .or_default(),
                brick_matrix(brick, camera.origin, candidate.offset),
                brick.rgb,
                false,
            );
        }
        plan.shadow_budget_exceeded = options
            .policy
            .triangle_budget
            .is_some_and(|budget| plan.estimated_shadow_triangles > budget);
        if options.reuse_shadow {
            self.shadow_cache = Some(ShadowCache {
                visibility: shadow.clone(),
                resolution: options.shadow_resolution,
                full_detail: options.policy.full_detail,
                budget: options.policy.triangle_budget,
                selected: options.selected,
                highlighted: options.highlighted.clone(),
                explode: options.explode,
                instances: plan.shadow.clone(),
                lod_counts: plan.shadow_lod_counts,
                triangles: plan.estimated_shadow_triangles,
                detail_scale: plan.shadow_detail_scale,
                budget_exceeded: plan.shadow_budget_exceeded,
                merge_surfaces: options.merge_surfaces,
            });
        }
        plan
    }
}

fn shadow_level(
    brick: &Brick,
    candidate: &Candidate,
    scale: f64,
    options: &SceneOptions<'_>,
) -> u8 {
    if options.policy.full_detail
        || options.selected == Some(brick.id)
        || options.highlighted.contains(&brick.id)
        || candidate.detail.part.is_none()
    {
        0
    } else {
        crate::lod::shadow_level(candidate.pixels / scale, candidate.previous)
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
                merge_surfaces: true,
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
                merge_surfaces: true,
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
                merge_surfaces: true,
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
            merge_surfaces: true,
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

    #[test]
    fn merged_regions_restore_selected_geometry_rebuild_after_edits_and_bypass_captures() {
        let mut demo = Demo::new();
        for index in 0..10_000 {
            demo.add(
                "3068b",
                4,
                (index % 100) as f64 * 2.,
                0.,
                (index / 100) as f64 * 2.,
            );
        }
        let camera = Camera::new(
            DVec3::new(100., 300., 1000.),
            DVec3::new(100., 0., 100.),
            1280.,
            720.,
            45.,
            4000.,
        );
        let highlighted = HashSet::new();
        let options = |selected, policy| SceneOptions {
            policy,
            selected,
            highlighted: &highlighted,
            explode: 0.,
            shadow_resolution: Some(128.),
            reuse_shadow: true,
            merge_surfaces: true,
        };
        let tree = Octree::new(
            demo.bricks
                .iter()
                .map(|brick| brick.render_bounds)
                .collect(),
        );
        let mut planner = ScenePlanner::default();
        let merged = planner.plan(
            &demo.bricks,
            &tree,
            &demo.meshes,
            &camera,
            &camera.visibility,
            options(None, RenderPolicy::INTERACTIVE),
        );
        assert!(merged.surface_pieces > 9000);
        assert!(merged.estimated_color_triangles < 10_000);
        let cached = planner.surface_mesh(0).vertices.as_ptr();
        planner.reset_detail();
        assert_eq!(
            planner.surface_mesh(0).vertices.as_ptr(),
            cached,
            "Quality changes must reuse cached surface vertices"
        );
        let selected = planner.plan(
            &demo.bricks,
            &tree,
            &demo.meshes,
            &camera,
            &camera.visibility,
            options(Some(1), RenderPolicy::INTERACTIVE),
        );
        assert_eq!(
            selected.color[&GeometryKey::Part("3068b.dat".into(), 0)].len() / 20,
            1
        );
        assert!(selected.surface_pieces < merged.surface_pieces);
        demo.bricks[0].translated(DVec3::Y);
        planner.retain(&demo.bricks);
        let tree = Octree::new(
            demo.bricks
                .iter()
                .map(|brick| brick.render_bounds)
                .collect(),
        );
        let moved = planner.plan(
            &demo.bricks,
            &tree,
            &demo.meshes,
            &camera,
            &camera.visibility,
            options(None, RenderPolicy::INTERACTIVE),
        );
        let index = moved
            .color
            .keys()
            .find_map(|key| {
                if let GeometryKey::Surface(index) = key {
                    Some(*index)
                } else {
                    None
                }
            })
            .unwrap();
        let chunk = &planner.surfaces.as_ref().unwrap().chunks[index];
        assert!(
            ergo_world::geometry::mesh_ray(
                &chunk.mesh,
                DVec3::new(1., 0.1, 1.) - chunk.origin,
                -DVec3::Y
            )
            .is_none(),
            "A moved piece must not leave a stale surface at its old location"
        );
        let capture = planner.plan(
            &demo.bricks,
            &tree,
            &demo.meshes,
            &camera,
            &camera.visibility,
            options(None, RenderPolicy::CAPTURE),
        );
        assert_eq!(capture.surface_pieces, 0);
        assert!(
            capture
                .color
                .keys()
                .all(|key| matches!(key, GeometryKey::Part(_, 0)))
        );
        assert_eq!(demo.bricks.len(), 10_000);
    }

    #[test]
    fn global_shadow_budget_coarsens_studs_but_keeps_the_selected_source_mesh() {
        let mut demo = Demo::new();
        for index in 0..200 {
            demo.add(
                "3003",
                4,
                (index % 20) as f64 * 2.,
                0.,
                (index / 20) as f64 * 2.,
            );
        }
        let camera = Camera::new(
            DVec3::new(20., 40., 80.),
            DVec3::new(20., 0., 10.),
            1280.,
            720.,
            60.,
            1000.,
        );
        let tree = Octree::new(
            demo.bricks
                .iter()
                .map(|brick| brick.render_bounds)
                .collect(),
        );
        let result = ScenePlanner::default().plan(
            &demo.bricks,
            &tree,
            &demo.meshes,
            &camera,
            &camera.visibility,
            SceneOptions {
                policy: RenderPolicy {
                    triangle_budget: Some(3000),
                    ..RenderPolicy::INTERACTIVE
                },
                selected: Some(1),
                highlighted: &HashSet::new(),
                explode: 0.,
                shadow_resolution: Some(512.),
                reuse_shadow: false,
                merge_surfaces: true,
            },
        );
        assert!(result.shadow_detail_scale > 1.);
        assert!(result.estimated_shadow_triangles <= 3000);
        assert!(!result.shadow_budget_exceeded);
        assert_eq!(
            result.shadow[&GeometryKey::Part("3003.dat".into(), 0)].len() / 20,
            1
        );
    }
}
