use crate::{
    lighting::ShadowFocus,
    navigation::{MAX_FRAME_DT, Navigation, NavigationInput, NavigationMode, TURN_RATE},
    scene::{Camera, GeometryKey, RenderPolicy, SceneOptions, ScenePlanner, ground},
    walking::{Scene as CollisionScene, Walker},
};
use ergo_world::{
    Aabb, Brick,
    geometry::{Mesh, mesh_ray},
    spatial::{Octree, Relation, relation},
};
use glam::{DMat4, DVec3, Mat4};
use std::collections::{BTreeMap, HashMap, HashSet};
use wasm_bindgen::{JsCast, JsValue};
use web_sys::{HtmlCanvasElement, WebGl2RenderingContext as Gl, WebGlProgram, WebGlShader};

mod batches;
mod illumination;
mod shaders;
pub use crate::scene::{DISPLAY_SCALE, Visibility};
use batches::{Batches, Pass};
use illumination::{LightPass, Observer};
use shaders::{FRAGMENT, VERTEX};

pub struct Renderer {
    pub canvas: HtmlCanvasElement,
    gl: Gl,
    program: WebGlProgram,
    lighting: LightPass,
    shadow_focus: ShadowFocus,
    pub geo: ergo_world::GeoReference,
    pub ground_visible: bool,
    /// Studio fill/environment with a fixed key unless point_light overrides it.
    pub studio_lighting: bool,
    /// Optional world-space point key; the homepage positions it with the cursor.
    pub point_light: Option<DVec3>,
    /// Indirect/environment illumination and broad studio fill, independently
    /// configurable without a different material or renderer implementation.
    pub ambient_light: f32,
    pub studio_fill: f32,
    gpu: Batches,
    planner: ScenePlanner,
    pub policy: RenderPolicy,
    meshes: BTreeMap<String, Mesh>,
    lod_meshes: BTreeMap<GeometryKey, Mesh>,
    pub ground: HashMap<(i64, i64), ergo_world::terrain::GroundBlock>,
    pub bricks: Vec<Brick>,
    pub tree: Octree,
    pub target: DVec3,
    pub distance: f64,
    pub yaw: f64,
    pub pitch: f64,
    pub selected: Option<u32>,
    pub highlighted: HashSet<u32>,
    pub grid: bool,
    pub explode: f64,
    pub fov_degrees: f64,
    pub fog_distance: f64,
    pub lod_counts: [usize; 4],
    pub navigation: Navigation,
    walker: Walker,
    capture_mode: bool,
    transparent_background: bool,
    origin: DVec3,
    dirty: bool,
    pub draw_calls: usize,
    pub triangles: usize,
    pub gpu_bytes: usize,
    pub ground_stud_instances: usize,
    pub detail_scale: f64,
    pub shadow_lod_counts: [usize; 3],
    pub estimated_shadow_triangles: usize,
    pub color_budget_exceeded: bool,
    pub shadow_detail_scale: f64,
    pub shadow_budget_exceeded: bool,
    pub surface_pieces: usize,
    pub scene_plan_ms: f64,
    pub rendered_frames: u64,
}

impl Renderer {
    pub fn new(canvas: HtmlCanvasElement) -> Result<Self, JsValue> {
        Self::new_mode(canvas, false, false, RenderPolicy::INTERACTIVE)
    }
    pub fn new_capture(canvas: HtmlCanvasElement) -> Result<Self, JsValue> {
        Self::new_mode(canvas, true, false, RenderPolicy::CAPTURE)
    }
    pub fn new_preview(canvas: HtmlCanvasElement) -> Result<Self, JsValue> {
        Self::new_mode(canvas, false, true, RenderPolicy::ILLUSTRATION)
    }
    fn new_mode(
        canvas: HtmlCanvasElement,
        capture_mode: bool,
        transparent_background: bool,
        policy: RenderPolicy,
    ) -> Result<Self, JsValue> {
        let options = js_sys::Object::new();
        js_sys::Reflect::set(&options, &"antialias".into(), &true.into())?;
        js_sys::Reflect::set(&options, &"alpha".into(), &transparent_background.into())?;
        js_sys::Reflect::set(
            &options,
            &"preserveDrawingBuffer".into(),
            &capture_mode.into(),
        )?;
        let gl: Gl = canvas.get_context_with_context_options("webgl2", &options)?.ok_or_else(|| JsValue::from_str("WebGL2 is unavailable. Enable hardware acceleration or use a WebGL2-capable browser."))?.dyn_into()?;
        let program = link(&gl, VERTEX, FRAGMENT)?;
        let lighting = LightPass::new(
            &gl,
            if canvas.client_width() <= 600 {
                1024
            } else {
                2048
            },
        )?;
        gl.enable(Gl::DEPTH_TEST);
        gl.depth_func(Gl::LEQUAL);
        gl.use_program(Some(&program));
        Ok(Self {
            canvas,
            gl,
            program,
            lighting,
            shadow_focus: ShadowFocus::default(),
            geo: Default::default(),
            ground_visible: true,
            studio_lighting: transparent_background,
            point_light: None,
            ambient_light: 1.,
            studio_fill: 1.,
            gpu: Batches::default(),
            planner: ScenePlanner::default(),
            policy,
            meshes: BTreeMap::new(),
            lod_meshes: BTreeMap::new(),
            ground: HashMap::new(),
            bricks: Vec::new(),
            tree: Octree::new(Vec::new()),
            target: DVec3::new(24.0, 6.0, 24.0),
            distance: 88.0,
            yaw: 0.73,
            pitch: 0.65,
            selected: None,
            highlighted: HashSet::new(),
            grid: true,
            explode: 0.0,
            fov_degrees: 45.0,
            capture_mode,
            transparent_background,
            fog_distance: 2200.0,
            lod_counts: [0; 4],
            navigation: Navigation::default(),
            walker: Walker::default(),
            origin: DVec3::ZERO,
            dirty: true,
            draw_calls: 0,
            triangles: 0,
            gpu_bytes: 0,
            ground_stud_instances: 0,
            detail_scale: 1.,
            shadow_lod_counts: [0; 3],
            estimated_shadow_triangles: 0,
            color_budget_exceeded: false,
            shadow_detail_scale: 1.,
            shadow_budget_exceeded: false,
            surface_pieces: 0,
            scene_plan_ms: 0.,
            rendered_frames: 0,
        })
    }

    pub fn set_bricks(&mut self, bricks: Vec<Brick>) {
        self.gpu.invalidate_surfaces();
        self.planner.retain(&bricks);
        self.tree = Octree::new(bricks.iter().map(|b| b.render_bounds).collect());
        self.bricks = bricks;
        if self
            .selected
            .is_some_and(|id| !self.bricks.iter().any(|b| b.id == id))
        {
            self.selected = None;
        }
        self.dirty = true;
    }

    pub fn changed(&mut self) {
        self.dirty = true;
    }
    /// Attribute/topology edits must invalidate derived surfaces as well as the
    /// frame; camera-only invalidation deliberately retains those caches.
    pub fn scene_changed(&mut self) {
        self.planner.retain(&self.bricks);
        self.gpu.invalidate_surfaces();
        self.dirty = true;
    }
    pub fn set_policy(&mut self, policy: RenderPolicy) {
        self.policy = policy;
        self.planner.reset_detail();
        self.dirty = true;
    }
    /// Replacing prototypes invalidates derived LODs and GPU geometry, not just
    /// instance transforms. All entry points use this same invalidation path.
    pub fn set_meshes(&mut self, meshes: BTreeMap<String, Mesh>) {
        let changed: HashSet<_> = self
            .meshes
            .iter()
            .filter(|(part, previous)| {
                meshes.get(*part).is_none_or(|next| {
                    previous.vertices != next.vertices
                        || previous.quality != next.quality
                        || previous.body_bounds.min != next.body_bounds.min
                        || previous.body_bounds.max != next.body_bounds.max
                })
            })
            .map(|(part, _)| part.clone())
            .collect();
        self.gpu.invalidate_parts(&changed);
        if !changed.is_empty() || meshes.keys().any(|part| !self.meshes.contains_key(part)) {
            self.planner.invalidate_meshes();
            self.gpu.invalidate_surfaces();
        }
        self.lod_meshes
            .retain(|key, _| !matches!(key,GeometryKey::Part(part,_) if changed.contains(part)));
        self.meshes = meshes;
        self.dirty = true;
    }
    pub fn meshes(&self) -> &BTreeMap<String, Mesh> {
        &self.meshes
    }
    pub fn set_ground(&mut self, block: ergo_world::terrain::GroundBlock) {
        if block.validate().is_ok() {
            self.ground.insert((block.coord.0, block.coord.2), block);
            self.dirty = true;
        }
    }
    pub fn visibility(&self) -> Visibility {
        self.camera().visibility
    }
    fn camera(&self) -> Camera {
        Camera::new(
            self.eye(),
            self.target,
            self.canvas.client_width() as f64,
            self.canvas.client_height() as f64,
            self.fov_degrees,
            self.fog_distance,
        )
    }
    pub fn visible_ground_cells(&self) -> Vec<(i64, i64)> {
        ground::visible_cells(&self.camera(), self.fog_distance)
    }

    pub fn set_camera(&mut self, position: DVec3, target: DVec3, fov: f64) -> Result<(), JsValue> {
        if !position.is_finite()
            || !target.is_finite()
            || position.abs().max_element() > ergo_world::MAX_WORLD_COORD
            || target.abs().max_element() > ergo_world::MAX_WORLD_COORD
            || !(10.0..=110.0).contains(&fov)
            || (position - target).length() < 0.001
        {
            return Err("Invalid camera".into());
        }
        let delta = position - target;
        self.navigation.set_mode(NavigationMode::Orbit);
        self.target = target;
        self.distance = delta.length();
        self.yaw = delta.x.atan2(delta.z);
        self.pitch = delta.y.atan2(delta.x.hypot(delta.z));
        self.fov_degrees = fov;
        self.dirty = true;
        Ok(())
    }
    pub fn preset(&mut self, view: &str) -> Result<(), JsValue> {
        if let Some(bounds) = ergo_world::bounds(&self.bricks) {
            self.fit(bounds);
        }
        self.fov_degrees = 45.0;
        match view {
            "isometric" => {
                self.yaw = 0.73;
                self.pitch = 0.65;
            }
            "north" => {
                self.yaw = std::f64::consts::PI;
                self.pitch = 0.0;
            }
            "south" => {
                self.yaw = 0.0;
                self.pitch = 0.0;
            }
            "east" => {
                self.yaw = std::f64::consts::FRAC_PI_2;
                self.pitch = 0.0;
            }
            "west" => {
                self.yaw = -std::f64::consts::FRAC_PI_2;
                self.pitch = 0.0;
            }
            "top" => self.pitch = std::f64::consts::FRAC_PI_2 - 1e-4,
            _ => return Err("Unknown camera preset".into()),
        };
        self.dirty = true;
        Ok(())
    }
    pub fn png(&mut self, width: u32, height: u32) -> Result<String, JsValue> {
        if !(160..=1024).contains(&width) || !(120..=768).contains(&height) {
            return Err("Invalid capture dimensions".into());
        }
        self.canvas.set_attribute(
            "style",
            &format!("width:{width}px;height:{height}px;display:block;max-width:none"),
        )?;
        self.canvas.set_width(width);
        self.canvas.set_height(height);
        self.render();
        self.canvas.to_data_url_with_type("image/png")
    }

    pub fn clear(&mut self) {
        self.gpu.clear();
        self.planner.clear();
        self.meshes.clear();
        self.lod_meshes.clear();
        self.ground.clear();
        self.bricks.clear();
        self.tree = Octree::new(Vec::new());
        self.selected = None;
        self.highlighted.clear();
        self.explode = 0.0;
        self.dirty = true;
    }

    pub fn fit(&mut self, bounds: Aabb) {
        self.navigation.set_mode(NavigationMode::Orbit);
        self.target = bounds.center();
        self.distance = bounds.size().length().max(8.0) * 1.35;
        self.pitch = 0.65;
        self.distance = self.distance.min(self.fog_distance * 0.85);
        self.dirty = true;
    }

    pub fn eye(&self) -> DVec3 {
        self.target + self.backward() * self.distance
    }

    fn set_eye(&mut self, eye: DVec3) {
        self.target = eye - self.backward() * self.distance;
        self.dirty = true;
    }

    pub fn orbit(&mut self, dx: f64, dy: f64) {
        let eye = self.eye();
        self.yaw -= dx * 0.007;
        self.pitch = (self.pitch + dy * 0.006).clamp(
            if self.navigation.mode() == NavigationMode::Orbit {
                0.08
            } else {
                -1.5
            },
            1.5,
        );
        if self.navigation.mode() != NavigationMode::Orbit {
            self.set_eye(eye);
        }
        self.dirty = true;
    }

    pub fn pan(&mut self, dx: f64, dy: f64) {
        let side = DVec3::new(self.yaw.cos(), 0.0, -self.yaw.sin());
        let forward = DVec3::new(self.yaw.sin(), 0.0, self.yaw.cos());
        if self.navigation.mode() == NavigationMode::Walk {
            return;
        }
        let scale = if self.navigation.mode() == NavigationMode::Orbit {
            self.distance
        } else {
            20.0
        };
        self.target += (-side * dx + forward * dy) * scale * 0.0015;
        self.target = self.target.clamp(
            DVec3::splat(-ergo_world::MAX_WORLD_COORD),
            DVec3::splat(ergo_world::MAX_WORLD_COORD),
        );
        self.dirty = true;
    }

    pub fn zoom(&mut self, amount: f64) {
        if self.navigation.mode() != NavigationMode::Orbit {
            self.navigation.scroll(amount);
            return;
        }
        self.distance =
            (self.distance * (amount * 0.001).exp()).clamp(3.0, self.fog_distance * 0.9);
        self.dirty = true;
    }
    fn backward(&self) -> DVec3 {
        DVec3::new(
            self.yaw.sin() * self.pitch.cos(),
            self.pitch.sin(),
            self.yaw.cos() * self.pitch.cos(),
        )
    }
    pub fn set_navigation(&mut self, mode: NavigationMode) {
        if self.navigation.mode() == NavigationMode::Orbit && mode != NavigationMode::Orbit {
            self.shadow_focus
                .update(true, self.eye(), self.target, self.distance);
        }
        if !self.navigation.set_mode(mode) {
            return;
        }
        let mut eye = self.eye();
        if mode == NavigationMode::Orbit {
            let pivot = self
                .selected_brick()
                .map(|b| b.bounds.center())
                .unwrap_or(eye - self.backward() * 88.0);
            let delta = eye - pivot;
            self.target = pivot;
            self.distance = delta.length().max(3.0);
            self.yaw = delta.x.atan2(delta.z);
            self.pitch = delta.y.atan2(delta.x.hypot(delta.z));
        } else {
            if mode == NavigationMode::Walk {
                let height = self.navigation.eye_height();
                let scene = CollisionScene::new(&self.bricks, &self.tree, height, &|_| false);
                let mut floor = scene.support_height(eye.x, eye.z, (eye.y - height).max(0.0));
                if !scene.clear(DVec3::new(eye.x, floor, eye.z)) {
                    floor = scene.support_height(eye.x, eye.z, f64::INFINITY);
                }
                eye.y = floor + height;
                self.pitch = 0.0;
                self.walker = Walker::default();
            }
            self.distance = 1.0;
            self.set_eye(eye);
        }
        self.shadow_focus.reanchor(self.eye());
        self.dirty = true;
    }
    pub fn teleport(&mut self, position: DVec3) {
        if self.navigation.mode() == NavigationMode::Orbit {
            self.target = position + DVec3::Y * 5.0;
            self.distance = 100.0;
        } else {
            let eye = position
                + DVec3::Y
                    * if self.navigation.mode() == NavigationMode::Walk {
                        self.navigation.eye_height()
                    } else {
                        30.0
                    };
            self.distance = 1.0;
            self.set_eye(eye);
            self.walker = Walker::default();
        }
        self.dirty = true;
    }
    pub fn set_eye_height(&mut self, height: f64) {
        self.target.y += self.navigation.set_eye_height(height);
        self.dirty = true;
    }
    pub fn navigate(
        &mut self,
        input: NavigationInput,
        dt: f64,
        blocked: &dyn Fn(ergo_world::BlockCoord) -> bool,
    ) {
        if self.navigation.mode() == NavigationMode::Orbit {
            return;
        }
        let dt = dt.clamp(0.0, MAX_FRAME_DT);
        let original = self.eye();
        let mut eye = original;
        let orientation = (self.yaw, self.pitch);
        self.yaw += input.turn * TURN_RATE * dt;
        self.navigation.accelerate(input.throttle, dt);
        let status = self.navigation.status();
        if self.navigation.mode() == NavigationMode::Fly {
            self.pitch = (self.pitch - input.vertical * TURN_RATE * dt).clamp(-1.5, 1.5);
            let mut direction = -self.backward();
            let length = (direction * DISPLAY_SCALE).length();
            direction /= length;
            let scene = CollisionScene::new(&self.bricks, &self.tree, status.eye_height, blocked);
            eye = scene.fly_slide(eye, direction * status.speed * dt);
        } else {
            let direction = DVec3::new(-self.yaw.sin(), 0.0, -self.yaw.cos()) * input.vertical;
            let scene = CollisionScene::new(&self.bricks, &self.tree, status.eye_height, blocked);
            let feet = self.walker.advance(
                &scene,
                eye - DVec3::Y * status.eye_height,
                direction * status.speed,
                dt,
                input.jump,
            );
            eye = feet + DVec3::Y * status.eye_height;
        }
        eye = eye.clamp(
            DVec3::splat(-ergo_world::MAX_WORLD_COORD),
            DVec3::splat(ergo_world::MAX_WORLD_COORD),
        );
        if (eye - original).length_squared() > 1e-20 || orientation != (self.yaw, self.pitch) {
            self.set_eye(eye);
        }
    }

    pub fn selected_brick(&self) -> Option<Brick> {
        self.selected
            .and_then(|id| self.bricks.iter().find(|b| b.id == id).cloned())
    }

    pub fn relations(&self, id: u32) -> Vec<Relation> {
        let Some(brick) = self.bricks.iter().find(|b| b.id == id) else {
            return Vec::new();
        };
        self.tree
            .query(brick.bounds.expanded(0.003))
            .into_iter()
            .filter_map(|i| {
                let other = &self.bricks[i];
                if other.id == id {
                    None
                } else {
                    relation(brick, other)
                }
            })
            .collect()
    }

    pub fn pick(&mut self, x: f64, y: f64) -> Option<u32> {
        let (near, direction) = self.camera().ray(
            x / self.canvas.client_width().max(1) as f64 * 2. - 1.,
            1. - y / self.canvas.client_height().max(1) as f64 * 2.,
        );
        let mut best = f64::INFINITY;
        let mut selected = None;
        // Exploded view moves individual bricks; its broad-phase is rebuilt on demand.
        let temporary;
        let tree = if self.explode > 0.0 {
            temporary = Octree::new(
                self.bricks
                    .iter()
                    .map(|b| {
                        let offset = DVec3::Y * b.bounds.center().y * self.explode;
                        Aabb::new(b.render_bounds.min + offset, b.render_bounds.max + offset)
                    })
                    .collect(),
            );
            &temporary
        } else {
            &self.tree
        };
        for (i, t) in tree.ray(near, direction) {
            if t > best {
                break;
            }
            let brick = &self.bricks[i];
            let Some(mesh) = self.meshes.get(&brick.part) else {
                continue;
            };
            let inverse = brick.transform.inverse();
            let position = brick.position + DVec3::Y * brick.bounds.center().y * self.explode;
            if let Some(t) = mesh_ray(mesh, inverse * (near - position), inverse * direction)
                && t < best
            {
                best = t;
                selected = Some(brick.id);
            }
        }
        self.selected = selected;
        self.dirty = true;
        selected
    }

    pub fn render(&mut self) {
        let dpr = if self.capture_mode {
            1.0
        } else {
            web_sys::window()
                .map(|w| w.device_pixel_ratio())
                .unwrap_or(1.0)
                .min(2.0)
        };
        let width = (self.canvas.client_width() as f64 * dpr).max(1.0) as u32;
        let height = (self.canvas.client_height() as f64 * dpr).max(1.0) as u32;
        if self.canvas.width() != width || self.canvas.height() != height {
            self.canvas.set_width(width);
            self.canvas.set_height(height);
            self.dirty = true;
        }
        let camera = self.camera();
        let origin = camera.origin;
        if origin != self.origin {
            self.origin = origin;
            self.dirty = true;
        }
        let shadow_view = self.shadow_focus.update(
            self.navigation.mode() == NavigationMode::Orbit,
            self.eye(),
            self.target,
            self.distance,
        );
        let observer = Observer {
            geo: self.geo,
            eye: self.eye(),
            target: self.target,
            shadow: shadow_view,
            origin: self.origin,
            display_scale: DISPLAY_SCALE,
            projection_pixels: camera.projection_pixels,
            studio: self.studio_lighting,
            point_light: self.point_light,
        };
        let light = self.lighting.prepare(&observer);
        let replan = self.dirty || light.shadow_changed;
        if !replan && !light.updated && !self.capture_mode {
            let _ = self.canvas.set_attribute("data-frame-redrawn", "false");
            let _ = self.canvas.set_attribute("data-shadow-draw-calls", "0");
            let _ = self.canvas.set_attribute("data-instance-upload-bytes", "0");
            return;
        }
        if replan {
            let shadow = Visibility::new(
                light.matrix.as_dmat4() * DMat4::from_scale(DISPLAY_SCALE),
                self.origin,
            );
            self.upload(&camera, &shadow);
            self.dirty = false;
        }
        let shadow_calls = self.lighting.draw(
            &observer,
            &light,
            &self.gpu,
            replan && self.gpu.shadow_changed,
        );
        if light.updated {
            let _ = self.canvas.set_attribute(
                "data-lighting",
                if self.point_light.is_some() {
                    "point"
                } else if self.studio_lighting {
                    "studio"
                } else if light.sun.daylight() {
                    "sunlight"
                } else {
                    "flashlight"
                },
            );
            let _ = self.canvas.set_attribute(
                "data-sun-elevation",
                &format!("{:.3}", light.sun.elevation.to_degrees()),
            );
            let _ = self.canvas.set_attribute(
                "data-sun-azimuth",
                &format!("{:.3}", light.sun.azimuth.to_degrees()),
            );
            let _ = self
                .canvas
                .set_attribute("data-shadow-map", &self.lighting.size.to_string());
            let _ = self.canvas.set_attribute("data-renderer", "ergo-webgl2");
        }
        let light_position = format!(
            "{:.5},{:.5},{:.5}",
            light.position.x, light.position.y, light.position.z
        );
        if self.canvas.get_attribute("data-light-position").as_deref()
            != Some(light_position.as_str())
        {
            let _ = self
                .canvas
                .set_attribute("data-light-position", &light_position);
        }
        let gl = &self.gl;
        gl.viewport(0, 0, width as i32, height as i32);
        gl.clear_color(
            0.0,
            0.0,
            0.0,
            if self.transparent_background {
                0.0
            } else {
                1.0
            },
        );
        gl.clear(Gl::COLOR_BUFFER_BIT | Gl::DEPTH_BUFFER_BIT);
        gl.use_program(Some(&self.program));
        let display = DISPLAY_SCALE.as_vec3();
        let eye = (self.eye() - self.origin).as_vec3() * display;
        let view_projection = camera.view_projection.as_mat4();
        if !self.transparent_background {
            self.lighting.sky(&light, view_projection, eye);
        }
        gl.use_program(Some(&self.program));
        self.lighting.uniforms(&self.program, &light);
        gl.uniform1f(
            gl.get_uniform_location(&self.program, "ambient_light")
                .as_ref(),
            self.ambient_light,
        );
        gl.uniform1f(
            gl.get_uniform_location(&self.program, "studio_fill")
                .as_ref(),
            self.studio_fill,
        );
        gl.uniform1f(
            gl.get_uniform_location(&self.program, "projection_pixels")
                .as_ref(),
            camera.projection_pixels as f32,
        );
        gl.uniform_matrix4fv_with_f32_array(
            gl.get_uniform_location(&self.program, "view_projection")
                .as_ref(),
            false,
            &view_projection.to_cols_array(),
        );
        gl.uniform_matrix4fv_with_f32_array(
            gl.get_uniform_location(&self.program, "display_scale")
                .as_ref(),
            false,
            &Mat4::from_scale(display).to_cols_array(),
        );
        gl.uniform3f(
            gl.get_uniform_location(&self.program, "eye").as_ref(),
            eye.x,
            eye.y,
            eye.z,
        );
        gl.uniform1i(
            gl.get_uniform_location(&self.program, "grid").as_ref(),
            self.grid as i32,
        );
        gl.uniform1f(
            gl.get_uniform_location(&self.program, "ground_stud_range")
                .as_ref(),
            if self.ground_visible {
                ground::stud_range(camera.projection_pixels) as f32
            } else {
                0.
            },
        );
        gl.uniform2f(
            gl.get_uniform_location(&self.program, "fog_limits")
                .as_ref(),
            (self.fog_distance * 0.65) as f32,
            self.fog_distance as f32,
        );
        let (calls, triangles, instances) =
            self.gpu
                .draw(gl, &self.program, Pass::Color, self.policy.full_detail);
        self.draw_calls = calls;
        self.triangles = triangles;
        self.rendered_frames += 1;
        let _ = self.canvas.set_attribute("data-frame-redrawn", "true");
        let _ = self
            .canvas
            .set_attribute("data-rendered-frames", &self.rendered_frames.to_string());
        for (name, value) in [
            ("data-color-instances", instances),
            ("data-surface-pieces", self.surface_pieces),
            ("data-color-triangles", self.triangles),
            (
                "data-brick-shadow-triangles-estimate",
                self.estimated_shadow_triangles,
            ),
            ("data-shadow-draw-calls", shadow_calls),
            (
                "data-instance-upload-bytes",
                if replan { self.gpu.uploaded_bytes } else { 0 },
            ),
        ] {
            let value = value.to_string();
            if self.canvas.get_attribute(name).as_deref() != Some(value.as_str()) {
                let _ = self.canvas.set_attribute(name, &value);
            }
        }
        for (name, value) in [
            ("data-detail-scale", self.detail_scale.to_string()),
            (
                "data-shadow-detail-scale",
                self.shadow_detail_scale.to_string(),
            ),
            ("data-scene-plan-ms", format!("{:.3}", self.scene_plan_ms)),
            (
                "data-shadow-budget-estimate-exceeded",
                self.shadow_budget_exceeded.to_string(),
            ),
            (
                "data-lod-counts",
                serde_json::to_string(&self.lod_counts).unwrap(),
            ),
            (
                "data-shadow-lod-counts",
                serde_json::to_string(&self.shadow_lod_counts).unwrap(),
            ),
            (
                "data-color-budget-estimate-exceeded",
                self.color_budget_exceeded.to_string(),
            ),
        ] {
            if self.canvas.get_attribute(name).as_deref() != Some(value.as_str()) {
                let _ = self.canvas.set_attribute(name, &value);
            }
        }
    }

    fn upload(&mut self, camera: &Camera, shadow: &Visibility) {
        let start = js_sys::Date::now();
        let mut plan = self.planner.plan(
            &self.bricks,
            &self.tree,
            &self.meshes,
            camera,
            shadow,
            SceneOptions {
                policy: self.policy,
                selected: self.selected,
                highlighted: &self.highlighted,
                explode: self.explode,
                shadow_resolution: self
                    .policy
                    .triangle_budget
                    .map(|_| self.lighting.size as f64),
                reuse_shadow: true,
                merge_surfaces: true,
            },
        );
        self.scene_plan_ms = js_sys::Date::now() - start;
        self.lod_counts = plan.lod_counts;
        self.detail_scale = plan.detail_scale;
        self.shadow_lod_counts = plan.shadow_lod_counts;
        self.estimated_shadow_triangles = plan.estimated_shadow_triangles;
        self.color_budget_exceeded = plan.budget_exceeded;
        self.shadow_detail_scale = plan.shadow_detail_scale;
        self.shadow_budget_exceeded = plan.shadow_budget_exceeded;
        self.surface_pieces = plan.surface_pieces;
        self.ground_stud_instances = 0;
        if self.ground_visible {
            let (instances, count) = ground::plan(camera, self.fog_distance, &self.ground);
            self.ground_stud_instances = count;
            plan.shadow.extend(instances.clone());
            plan.color.extend(instances);
        }
        self.gpu.begin(&plan);
        let keys: std::collections::BTreeSet<_> = plan
            .color
            .keys()
            .chain(plan.shadow.keys())
            .chain(plan.transparent.keys())
            .chain(plan.transparent_shadow.keys())
            .cloned()
            .collect();
        for key in keys {
            let cpu = match &key {
                GeometryKey::Part(part, 0) => &self.meshes[part],
                GeometryKey::Surface(index) => self.planner.surface_mesh(*index),
                _ => self
                    .lod_meshes
                    .entry(key.clone())
                    .or_insert_with(|| match &key {
                        GeometryKey::Ground | GeometryKey::Aggregate => {
                            ergo_world::geometry::ground_mesh()
                        }
                        GeometryKey::GroundStuds(segments) => {
                            ergo_world::geometry::ground_stud_mesh_with_segments(u32::from(
                                *segments,
                            ))
                        }
                        GeometryKey::Part(part, level) => {
                            crate::part_lod::RenderPart::resolve(&self.meshes[part])
                                .expect("Planned part LOD must have a reviewed display profile")
                                .mesh(*level)
                        }
                        GeometryKey::Surface(_) => unreachable!(),
                    }),
            };
            self.gpu.update(&self.gl, key, cpu, &plan);
        }
        self.gpu_bytes = self.gpu.bytes();
        self.gl.bind_vertex_array(None);
    }
}

impl Drop for Renderer {
    fn drop(&mut self) {
        self.clear();
        self.gl.delete_program(Some(&self.program));
    }
}

fn shader(gl: &Gl, kind: u32, source: &str) -> Result<WebGlShader, JsValue> {
    let shader = gl.create_shader(kind).ok_or("Cannot allocate shader")?;
    gl.shader_source(&shader, &shaders::source(source));
    gl.compile_shader(&shader);
    if gl
        .get_shader_parameter(&shader, Gl::COMPILE_STATUS)
        .as_bool()
        .unwrap_or(false)
    {
        Ok(shader)
    } else {
        Err(gl.get_shader_info_log(&shader).unwrap_or_default().into())
    }
}
fn link(gl: &Gl, vertex: &str, fragment: &str) -> Result<WebGlProgram, JsValue> {
    let v = shader(gl, Gl::VERTEX_SHADER, vertex)?;
    let f = shader(gl, Gl::FRAGMENT_SHADER, fragment)?;
    let program = gl.create_program().ok_or("Cannot allocate WebGL program")?;
    gl.attach_shader(&program, &v);
    gl.attach_shader(&program, &f);
    gl.link_program(&program);
    gl.delete_shader(Some(&v));
    gl.delete_shader(Some(&f));
    if gl
        .get_program_parameter(&program, Gl::LINK_STATUS)
        .as_bool()
        .unwrap_or(false)
    {
        Ok(program)
    } else {
        Err(gl.get_program_info_log(&program).unwrap_or_default().into())
    }
}
