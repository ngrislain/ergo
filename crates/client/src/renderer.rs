use crate::{
    navigation::{MAX_FRAME_DT, Navigation, NavigationInput, NavigationMode, TURN_RATE},
    walking::{Scene as WalkScene, Walker},
};
use ergo_world::{
    Aabb, Brick,
    geometry::{Mesh, fallback_mesh, mesh_ray},
    spatial::{Octree, Relation, relation},
};
use glam::{DMat4, DVec3, DVec4, Mat4};
use std::collections::{BTreeMap, HashMap, HashSet};
use wasm_bindgen::{JsCast, JsValue};
use web_sys::{
    HtmlCanvasElement, WebGl2RenderingContext as Gl, WebGlBuffer, WebGlProgram, WebGlShader,
    WebGlVertexArrayObject,
};

mod illumination;
mod shaders;
use illumination::{LightPass, Observer};
use shaders::{FRAGMENT, VERTEX};

struct GpuMesh {
    vao: WebGlVertexArrayObject,
    vertex_buffer: WebGlBuffer,
    instance_buffer: WebGlBuffer,
    vertices: i32,
    instances: i32,
}

const DISPLAY_SCALE: DVec3 = DVec3::new(
    1.0,
    ergo_world::physical::BRICK_HEIGHT_M / ergo_world::physical::STUD_PITCH_M,
    1.0,
);

pub struct Renderer {
    pub canvas: HtmlCanvasElement,
    gl: Gl,
    program: WebGlProgram,
    lighting: LightPass,
    pub geo: ergo_world::GeoReference,
    pub ground_visible: bool,
    /// Fixed world-space night-light eye/target for illustrations; None is a headlamp.
    pub flashlight_anchor: Option<(DVec3, DVec3)>,
    gpu: HashMap<String, GpuMesh>,
    pub meshes: BTreeMap<String, Mesh>,
    lod_meshes: BTreeMap<String, Mesh>,
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
}

impl Renderer {
    pub fn new(canvas: HtmlCanvasElement) -> Result<Self, JsValue> {
        Self::new_mode(canvas, false, false)
    }
    pub fn new_capture(canvas: HtmlCanvasElement) -> Result<Self, JsValue> {
        Self::new_mode(canvas, true, false)
    }
    pub fn new_preview(canvas: HtmlCanvasElement) -> Result<Self, JsValue> {
        Self::new_mode(canvas, false, true)
    }
    fn new_mode(
        canvas: HtmlCanvasElement,
        capture_mode: bool,
        transparent_background: bool,
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
            geo: Default::default(),
            ground_visible: true,
            flashlight_anchor: None,
            gpu: HashMap::new(),
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
        })
    }

    pub fn set_bricks(&mut self, bricks: Vec<Brick>) {
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
    pub fn set_ground(&mut self, block: ergo_world::terrain::GroundBlock) {
        if block.validate().is_ok() {
            self.ground.insert((block.coord.0, block.coord.2), block);
            self.dirty = true;
        }
    }
    pub fn visibility(&self) -> Visibility {
        let display = DISPLAY_SCALE;
        let origin = (self.target / 1024.0).floor() * 1024.0;
        let aspect = (self.canvas.client_width().max(1) as f64)
            / (self.canvas.client_height().max(1) as f64);
        let m = DMat4::perspective_rh_gl(
            self.fov_degrees.to_radians(),
            aspect,
            0.1,
            self.fog_distance,
        ) * DMat4::look_at_rh(
            (self.eye() - origin) * display,
            (self.target - origin) * display,
            DVec3::Y,
        ) * DMat4::from_scale(display);
        let rows = [
            DVec4::new(m.x_axis.x, m.y_axis.x, m.z_axis.x, m.w_axis.x),
            DVec4::new(m.x_axis.y, m.y_axis.y, m.z_axis.y, m.w_axis.y),
            DVec4::new(m.x_axis.z, m.y_axis.z, m.z_axis.z, m.w_axis.z),
            DVec4::new(m.x_axis.w, m.y_axis.w, m.z_axis.w, m.w_axis.w),
        ];
        Visibility {
            origin,
            eye: self.eye(),
            far: self.fog_distance,
            planes: [
                rows[3] + rows[0],
                rows[3] - rows[0],
                rows[3] + rows[1],
                rows[3] - rows[1],
                rows[3] + rows[2],
                rows[3] - rows[2],
            ]
            .map(|p| p.to_array()),
        }
    }
    pub fn visible_ground_cells(&self) -> Vec<(i64, i64)> {
        let visibility = self.visibility();
        let camera = ergo_world::BlockCoord::at(self.eye());
        let radius = (self.fog_distance / 1024.0).ceil() as i64 + 1;
        let mut cells = Vec::new();
        for x in camera.0 - radius..=camera.0 + radius {
            for y in camera.2 - radius..=camera.2 + radius {
                let min = DVec3::new(x as f64 * 1024.0, -1.0 / 3.0, y as f64 * 1024.0);
                if visibility.contains(Aabb::new(min, min + DVec3::new(1024.0, 1.0 / 3.0, 1024.0)))
                {
                    cells.push((x, y));
                }
            }
        }
        cells
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
        for (_, mesh) in self.gpu.drain() {
            self.gl.delete_buffer(Some(&mesh.vertex_buffer));
            self.gl.delete_buffer(Some(&mesh.instance_buffer));
            self.gl.delete_vertex_array(Some(&mesh.vao));
        }
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
                let scene = WalkScene::new(&self.bricks, &self.tree, height, &|_| false);
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
            eye += direction * status.speed * dt;
        } else {
            let direction = DVec3::new(-self.yaw.sin(), 0.0, -self.yaw.cos()) * input.vertical;
            let scene = WalkScene::new(&self.bricks, &self.tree, status.eye_height, blocked);
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
        let w = self.canvas.client_width() as f64;
        let h = self.canvas.client_height() as f64;
        let projection =
            DMat4::perspective_rh_gl(self.fov_degrees.to_radians(), w / h, 0.1, self.fog_distance);
        let display = DISPLAY_SCALE;
        let view = DMat4::look_at_rh(self.eye() * display, self.target * display, DVec3::Y);
        let inverse = (projection * view * DMat4::from_scale(display)).inverse();
        let near = inverse.project_point3(DVec3::new(x / w * 2.0 - 1.0, 1.0 - y / h * 2.0, -1.0));
        let far = inverse.project_point3(DVec3::new(x / w * 2.0 - 1.0, 1.0 - y / h * 2.0, 1.0));
        let direction = (far - near).normalize();
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
        let origin = (self.target / 1024.0).floor() * 1024.0;
        if origin != self.origin {
            self.origin = origin;
            self.dirty = true;
        }
        let changed = self.dirty;
        if self.dirty {
            self.upload();
            self.dirty = false;
        }
        let light = self.lighting.prepare(
            Observer {
                geo: self.geo,
                eye: self.eye(),
                target: self.target,
                origin: self.origin,
                display_scale: DISPLAY_SCALE,
                distance: self.distance,
                flashlight_anchor: self.flashlight_anchor,
            },
            &self.gpu,
            changed,
        );
        if light.updated {
            let _ = self.canvas.set_attribute(
                "data-lighting",
                if light.sun.daylight() {
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
        let view = Mat4::look_at_rh(
            eye,
            (self.target - self.origin).as_vec3() * display,
            glam::Vec3::Y,
        );
        let projection = Mat4::perspective_rh_gl(
            (self.fov_degrees as f32).to_radians(),
            width as f32 / height as f32,
            0.1,
            self.fog_distance as f32,
        );
        if !self.transparent_background {
            self.lighting.sky(&light, projection * view, eye);
        }
        gl.use_program(Some(&self.program));
        self.lighting.uniforms(&self.program, &light);
        gl.uniform_matrix4fv_with_f32_array(
            gl.get_uniform_location(&self.program, "view_projection")
                .as_ref(),
            false,
            &(projection * view).to_cols_array(),
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
            self.ground_stud_range() as f32,
        );
        gl.uniform2f(
            gl.get_uniform_location(&self.program, "fog_limits")
                .as_ref(),
            (self.fog_distance * 0.65) as f32,
            self.fog_distance as f32,
        );
        self.draw_calls = 0;
        self.triangles = 0;
        for (key, mesh) in &self.gpu {
            if mesh.instances == 0 {
                continue;
            }
            gl.uniform1i(
                gl.get_uniform_location(&self.program, "floor_mode")
                    .as_ref(),
                (key == "__ground__") as i32,
            );
            gl.bind_vertex_array(Some(&mesh.vao));
            gl.draw_arrays_instanced(Gl::TRIANGLES, 0, mesh.vertices, mesh.instances);
            self.draw_calls += 1;
            self.triangles += mesh.vertices as usize * mesh.instances as usize / 3;
        }
        gl.bind_vertex_array(None);
    }

    fn upload(&mut self) {
        let mut groups: BTreeMap<String, Vec<f32>> = BTreeMap::new();
        let visibility = self.visibility();
        self.lod_counts = [0; 4];
        let candidates = if self.explode > 0.0 {
            (0..self.bricks.len()).collect()
        } else {
            self.tree.frustum(&visibility.planes, visibility.origin)
        };
        let pixels = self.canvas.client_height().max(1) as f64
            / (2.0 * (self.fov_degrees.to_radians() * 0.5).tan());
        let mut aggregates = BTreeMap::<(i64, i64, i64, u32), Aabb>::new();
        for index in candidates {
            let brick = &self.bricks[index];
            let Some(mesh) = self.meshes.get(&brick.part) else {
                continue;
            };
            let offset = DVec3::Y * brick.bounds.center().y * self.explode;
            let bounds = Aabb::new(
                brick.render_bounds.min + offset,
                brick.render_bounds.max + offset,
            );
            if !visibility.contains(bounds) {
                continue;
            }
            let distance = ((bounds.center() - self.eye()) * DISPLAY_SCALE)
                .length()
                .max(0.1);
            let size = (bounds.size() * DISPLAY_SCALE).length() * pixels / distance;
            let lod = if self.selected == Some(brick.id) || size >= 24.0 {
                0
            } else if size >= 8.0 {
                1
            } else if size >= 2.0 || self.highlighted.contains(&brick.id) {
                2
            } else {
                3
            };
            self.lod_counts[lod] += 1;
            if lod == 3 {
                let p = (bounds.center() / 8.0).floor();
                let key = (p.x as i64, p.y as i64, p.z as i64, brick.rgb);
                aggregates
                    .entry(key)
                    .and_modify(|b| *b = b.union(bounds))
                    .or_insert(bounds);
                continue;
            }
            let key = format!("{}|{lod}", brick.part);
            if lod == 1 {
                self.lod_meshes.entry(key.clone()).or_insert_with(|| {
                    if ergo_world::geometry::part_spec(&brick.part).is_some() {
                        fallback_mesh(&brick.part)
                    } else {
                        ergo_world::geometry::body_mesh(&brick.part, mesh.body_bounds)
                    }
                });
            }
            if lod == 2 {
                self.lod_meshes.entry(key.clone()).or_insert_with(|| {
                    ergo_world::geometry::body_mesh(&brick.part, mesh.body_bounds)
                });
            }
            let position = brick.position - self.origin + offset;
            let matrix = DMat4::from_cols(
                brick.transform.x_axis.extend(0.0),
                brick.transform.y_axis.extend(0.0),
                brick.transform.z_axis.extend(0.0),
                position.extend(1.0),
            );
            instance(
                groups.entry(key).or_default(),
                matrix,
                brick.rgb,
                self.selected == Some(brick.id) || self.highlighted.contains(&brick.id),
            );
        }
        for ((_, _, _, rgb), bounds) in aggregates {
            let position =
                DVec3::new(bounds.center().x, bounds.max.y, bounds.center().z) - self.origin;
            instance(
                groups.entry("__aggregate__".into()).or_default(),
                DMat4::from_scale_rotation_translation(
                    bounds.size().max(DVec3::splat(0.001)),
                    glam::DQuat::IDENTITY,
                    position,
                ),
                rgb,
                false,
            );
        }
        let ground = ergo_world::geometry::ground_mesh();
        let visible_ground = if self.ground_visible {
            self.visible_ground_cells()
        } else {
            Vec::new()
        };
        for &(x, y) in &visible_ground {
            let color = self
                .ground
                .get(&(x, y))
                .map(|block| block.color)
                .unwrap_or(71);
            let top = ergo_world::terrain::GROUND_TOP;
            let center =
                DVec3::new((x as f64 + 0.5) * 1024.0, top, (y as f64 + 0.5) * 1024.0) - self.origin;
            instance(
                groups.entry("__ground__".into()).or_default(),
                DMat4::from_scale_rotation_translation(
                    DVec3::new(1024.0, 1.0 / 3.0, 1024.0),
                    glam::DQuat::IDENTITY,
                    center,
                ),
                ergo_world::color_rgb(color),
                false,
            );
        }
        self.ground_stud_instances = 0;
        let eye = self.eye();
        let maximum_distance = self.ground_stud_range();
        if !visible_ground.is_empty()
            && (eye.y - ergo_world::terrain::GROUND_TOP).abs() * DISPLAY_SCALE.y < maximum_distance
        {
            let cells: HashSet<_> = visible_ground.into_iter().collect();
            let center_x = eye.x.floor() as i64;
            let center_z = eye.z.floor() as i64;
            for x in center_x - 48..=center_x + 48 {
                for z in center_z - 48..=center_z + 48 {
                    let cell = (x.div_euclid(1024), z.div_euclid(1024));
                    if !cells.contains(&cell) {
                        continue;
                    }
                    let position = DVec3::new(
                        x as f64 + 0.5,
                        ergo_world::terrain::GROUND_TOP,
                        z as f64 + 0.5,
                    );
                    if ((position - eye) * DISPLAY_SCALE).length() > maximum_distance
                        || !visibility.contains(Aabb::new(
                            position - DVec3::new(0.3, 0., 0.3),
                            position + DVec3::new(0.3, 1. / 6., 0.3),
                        ))
                    {
                        continue;
                    }
                    let color = self.ground.get(&cell).map(|b| b.color).unwrap_or(71);
                    instance(
                        groups.entry("__ground_studs__".into()).or_default(),
                        DMat4::from_translation(position - self.origin),
                        ergo_world::color_rgb(color),
                        false,
                    );
                    self.ground_stud_instances += 1;
                }
            }
        }
        let studs = ergo_world::geometry::ground_stud_mesh();
        self.gpu.retain(|key, mesh| {
            if groups.contains_key(key) {
                true
            } else {
                self.gl.delete_buffer(Some(&mesh.vertex_buffer));
                self.gl.delete_buffer(Some(&mesh.instance_buffer));
                self.gl.delete_vertex_array(Some(&mesh.vao));
                false
            }
        });
        self.gpu_bytes = 0;
        for (key, data) in groups {
            let cpu = if key == "__ground__" || key == "__aggregate__" {
                &ground
            } else if key == "__ground_studs__" {
                &studs
            } else if key.ends_with("|0") {
                &self.meshes[key.strip_suffix("|0").unwrap()]
            } else {
                &self.lod_meshes[&key]
            };
            let mesh = self
                .gpu
                .entry(key)
                .or_insert_with(|| create_mesh(&self.gl, cpu));
            self.gl.bind_vertex_array(Some(&mesh.vao));
            self.gl
                .bind_buffer(Gl::ARRAY_BUFFER, Some(&mesh.instance_buffer));
            let array = js_sys::Float32Array::from(data.as_slice());
            self.gl
                .buffer_data_with_array_buffer_view(Gl::ARRAY_BUFFER, &array, Gl::DYNAMIC_DRAW);
            mesh.instances = (data.len() / 20) as i32;
            self.gpu_bytes += cpu.vertices.len() * 4 + data.len() * 4;
        }
        self.gl.bind_vertex_array(None);
    }

    fn ground_stud_range(&self) -> f64 {
        if !self.ground_visible {
            return 0.0;
        }
        let pixels = self.canvas.client_height().max(1) as f64
            / (2.0 * (self.fov_degrees.to_radians() * 0.5).tan());
        (pixels * 0.6 / 4.0).min(100.0)
    }
}

impl Drop for Renderer {
    fn drop(&mut self) {
        self.clear();
        self.gl.delete_program(Some(&self.program));
    }
}

pub struct Visibility {
    pub planes: [[f64; 4]; 6],
    pub origin: DVec3,
    eye: DVec3,
    far: f64,
}
impl Visibility {
    pub fn contains(&self, bounds: Aabb) -> bool {
        let center = bounds.center() - self.origin;
        let half = bounds.size() * 0.5;
        if self.planes.iter().any(|p| {
            let n = DVec3::new(p[0], p[1], p[2]);
            n.dot(center) + p[3] + n.abs().dot(half) < 0.0
        }) {
            return false;
        }
        let nearest = self.eye.clamp(bounds.min, bounds.max);
        ((self.eye - nearest) * DISPLAY_SCALE).length() <= self.far
    }
}
fn instance(out: &mut Vec<f32>, matrix: DMat4, rgb: u32, selected: bool) {
    out.extend(matrix.to_cols_array().iter().map(|v| *v as f32));
    out.extend_from_slice(&[
        ((rgb >> 16) & 255) as f32 / 255.0,
        ((rgb >> 8) & 255) as f32 / 255.0,
        (rgb & 255) as f32 / 255.0,
        if selected { 1.0 } else { 0.0 },
    ]);
}

fn create_mesh(gl: &Gl, mesh: &Mesh) -> GpuMesh {
    let vao = gl.create_vertex_array().expect("WebGL VAO allocation");
    gl.bind_vertex_array(Some(&vao));
    let vertex_buffer = gl.create_buffer().expect("WebGL geometry allocation");
    gl.bind_buffer(Gl::ARRAY_BUFFER, Some(&vertex_buffer));
    let array = js_sys::Float32Array::from(mesh.vertices.as_slice());
    gl.buffer_data_with_array_buffer_view(Gl::ARRAY_BUFFER, &array, Gl::STATIC_DRAW);
    for i in 0..3 {
        gl.enable_vertex_attrib_array(i);
        gl.vertex_attrib_pointer_with_i32(i, 3, Gl::FLOAT, false, 36, (i * 12) as i32);
    }
    let instance_buffer = gl.create_buffer().expect("WebGL instance allocation");
    gl.bind_buffer(Gl::ARRAY_BUFFER, Some(&instance_buffer));
    for i in 0..4 {
        gl.enable_vertex_attrib_array(3 + i);
        gl.vertex_attrib_pointer_with_i32(3 + i, 4, Gl::FLOAT, false, 80, (i * 16) as i32);
        gl.vertex_attrib_divisor(3 + i, 1);
    }
    gl.enable_vertex_attrib_array(7);
    gl.vertex_attrib_pointer_with_i32(7, 4, Gl::FLOAT, false, 80, 64);
    gl.vertex_attrib_divisor(7, 1);
    GpuMesh {
        vao,
        vertex_buffer,
        instance_buffer,
        vertices: (mesh.vertices.len() / 9) as i32,
        instances: 0,
    }
}

fn shader(gl: &Gl, kind: u32, source: &str) -> Result<WebGlShader, JsValue> {
    let shader = gl.create_shader(kind).ok_or("Cannot allocate shader")?;
    gl.shader_source(&shader, source);
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
