use super::{GpuMesh, link, shaders};
use crate::lighting::{SolarPosition, solar_position};
use ergo_world::GeoReference;
use glam::{DVec3, Mat4, Vec3};
use std::collections::HashMap;
use wasm_bindgen::JsValue;
use web_sys::{WebGl2RenderingContext as Gl, WebGlFramebuffer, WebGlProgram, WebGlTexture};

pub(super) struct LightPass {
    gl: Gl,
    framebuffer: WebGlFramebuffer,
    depth: WebGlTexture,
    shadow_program: WebGlProgram,
    sky_program: WebGlProgram,
    pub size: i32,
    previous_matrix: Option<Mat4>,
    solar: Option<([i64; 3], SolarPosition)>,
}

pub(super) struct Observer {
    pub projection_pixels: f64,
    pub geo: GeoReference,
    pub eye: DVec3,
    pub target: DVec3,
    pub origin: DVec3,
    pub display_scale: DVec3,
    pub distance: f64,
    pub studio: bool,
    pub point_light: Option<DVec3>,
}

pub(super) struct LightFrame {
    studio: bool,
    point: bool,
    pub sun: SolarPosition,
    pub updated: bool,
    matrix: Mat4,
    pub position: Vec3,
    forward: Vec3,
}

impl LightPass {
    pub fn new(gl: &Gl, size: i32) -> Result<Self, JsValue> {
        let depth = gl
            .create_texture()
            .ok_or("Cannot allocate shadow texture")?;
        gl.bind_texture(Gl::TEXTURE_2D, Some(&depth));
        gl.tex_storage_2d(Gl::TEXTURE_2D, 1, Gl::DEPTH_COMPONENT24, size, size);
        for name in [Gl::TEXTURE_MIN_FILTER, Gl::TEXTURE_MAG_FILTER] {
            gl.tex_parameteri(Gl::TEXTURE_2D, name, Gl::LINEAR as i32);
        }
        for name in [Gl::TEXTURE_WRAP_S, Gl::TEXTURE_WRAP_T] {
            gl.tex_parameteri(Gl::TEXTURE_2D, name, Gl::CLAMP_TO_EDGE as i32);
        }
        gl.tex_parameteri(
            Gl::TEXTURE_2D,
            Gl::TEXTURE_COMPARE_MODE,
            Gl::COMPARE_REF_TO_TEXTURE as i32,
        );
        gl.tex_parameteri(Gl::TEXTURE_2D, Gl::TEXTURE_COMPARE_FUNC, Gl::LEQUAL as i32);
        let framebuffer = gl
            .create_framebuffer()
            .ok_or("Cannot allocate shadow framebuffer")?;
        gl.bind_framebuffer(Gl::FRAMEBUFFER, Some(&framebuffer));
        gl.framebuffer_texture_2d(
            Gl::FRAMEBUFFER,
            Gl::DEPTH_ATTACHMENT,
            Gl::TEXTURE_2D,
            Some(&depth),
            0,
        );
        let buffers = js_sys::Array::new();
        buffers.push(&JsValue::from_f64(Gl::NONE as f64));
        gl.draw_buffers(&buffers);
        gl.read_buffer(Gl::NONE);
        let complete = gl.check_framebuffer_status(Gl::FRAMEBUFFER) == Gl::FRAMEBUFFER_COMPLETE;
        gl.bind_framebuffer(Gl::FRAMEBUFFER, None);
        gl.bind_texture(Gl::TEXTURE_2D, None);
        if !complete {
            gl.delete_texture(Some(&depth));
            gl.delete_framebuffer(Some(&framebuffer));
            return Err("WebGL shadow framebuffer is incomplete".into());
        }
        Ok(Self {
            gl: gl.clone(),
            framebuffer,
            depth,
            shadow_program: link(gl, shaders::SHADOW_VERTEX, shaders::SHADOW_FRAGMENT)?,
            sky_program: link(gl, shaders::SKY_VERTEX, shaders::SKY_FRAGMENT)?,
            size,
            previous_matrix: None,
            solar: None,
        })
    }

    pub fn prepare(
        &mut self,
        observer: Observer,
        gpu: &HashMap<String, GpuMesh>,
        changed: bool,
    ) -> LightFrame {
        let gps = observer.geo.to_gps(observer.target);
        let seconds = (js_sys::Date::now() / 60000.0).floor() * 60.0;
        let key = [
            (gps[0] * 10000.0).round() as i64,
            (gps[1] * 10000.0).round() as i64,
            (seconds / 60.0) as i64,
        ];
        let updated = self.solar.is_none_or(|(previous, _)| previous != key);
        let sun = if observer.studio {
            let direction = DVec3::new(-0.6, 1., 0.8).normalize();
            SolarPosition {
                direction,
                elevation: direction.y.asin(),
                azimuth: direction.x.atan2(-direction.z),
            }
        } else if updated {
            let position = solar_position(gps[0], gps[1], seconds);
            self.solar = Some((key, position));
            position
        } else {
            self.solar.unwrap().1
        };
        let eye = ((observer.eye - observer.origin) * observer.display_scale).as_vec3();
        let target = ((observer.target - observer.origin) * observer.display_scale).as_vec3();
        let (matrix, position, forward) = if let Some(position) = observer.point_light {
            let position = ((position - observer.origin) * observer.display_scale).as_vec3();
            let forward = (target - position).normalize();
            let up = if forward.y.abs() > 0.98 {
                Vec3::Z
            } else {
                Vec3::Y
            };
            (
                Mat4::perspective_rh_gl(100_f32.to_radians(), 1., 2., 256.)
                    * Mat4::look_at_rh(position, target, up),
                position,
                forward,
            )
        } else if sun.daylight() {
            let span = if observer.studio {
                32.
            } else {
                (observer.distance * 0.7).clamp(18.0, 180.0) as f32
            };
            let direction = sun.direction.as_vec3();
            let position = target + direction * span * 2.0;
            let up = if direction.y.abs() > 0.98 {
                Vec3::Z
            } else {
                Vec3::Y
            };
            let view = Mat4::look_at_rh(position, target, up);
            (
                Mat4::orthographic_rh_gl(-span, span, -span, span, 0.1, span * 4.0) * view,
                position,
                -direction,
            )
        } else {
            let direction = (target - eye).normalize();
            // A navigation headlamp is coaxial with the *rendered* camera ray.
            // A lateral/up offset causes parallax on nearby walking surfaces.
            let position = eye;
            let forward = direction;
            let up = if forward.y.abs() > 0.98 {
                Vec3::Z
            } else {
                Vec3::Y
            };
            let view = Mat4::look_at_rh(position, position + forward, up);
            (
                Mat4::perspective_rh_gl(
                    64_f32.to_radians(),
                    1.0,
                    0.1,
                    ((target - eye).length() * 3.0).clamp(100.0, 1600.0),
                ) * view,
                position,
                forward,
            )
        };
        if changed || self.previous_matrix != Some(matrix) {
            let gl = &self.gl;
            gl.bind_framebuffer(Gl::FRAMEBUFFER, Some(&self.framebuffer));
            gl.viewport(0, 0, self.size, self.size);
            gl.clear(Gl::DEPTH_BUFFER_BIT);
            gl.enable(Gl::POLYGON_OFFSET_FILL);
            gl.polygon_offset(1.0, 2.0);
            gl.use_program(Some(&self.shadow_program));
            uniform_matrix(
                gl,
                &self.shadow_program,
                "display_scale",
                Mat4::from_scale(observer.display_scale.as_vec3()),
            );
            uniform_matrix(gl, &self.shadow_program, "shadow_matrix", matrix);
            uniform_vector(gl, &self.shadow_program, "eye", eye);
            gl.uniform1f(
                gl.get_uniform_location(&self.shadow_program, "projection_pixels")
                    .as_ref(),
                observer.projection_pixels as f32,
            );
            for mesh in gpu.values().filter(|mesh| mesh.instances > 0) {
                gl.uniform1i(
                    gl.get_uniform_location(&self.shadow_program, "stud_mode")
                        .as_ref(),
                    mesh.stud_mode,
                );
                gl.bind_vertex_array(Some(&mesh.vao));
                gl.draw_arrays_instanced(Gl::TRIANGLES, 0, mesh.vertices, mesh.instances);
            }
            gl.bind_vertex_array(None);
            gl.disable(Gl::POLYGON_OFFSET_FILL);
            gl.bind_framebuffer(Gl::FRAMEBUFFER, None);
            self.previous_matrix = Some(matrix);
        }
        LightFrame {
            studio: observer.studio,
            point: observer.point_light.is_some(),
            sun,
            updated,
            matrix,
            position,
            forward,
        }
    }

    pub fn sky(&self, frame: &LightFrame, view_projection: Mat4, eye: Vec3) {
        let gl = &self.gl;
        gl.disable(Gl::DEPTH_TEST);
        gl.depth_mask(false);
        gl.use_program(Some(&self.sky_program));
        uniform_matrix(
            gl,
            &self.sky_program,
            "inverse_view_projection",
            view_projection.inverse(),
        );
        uniform_vector(gl, &self.sky_program, "eye", eye);
        uniform_vector(
            gl,
            &self.sky_program,
            "sun_direction",
            frame.sun.direction.as_vec3(),
        );
        gl.uniform1f(
            gl.get_uniform_location(&self.sky_program, "sky_amount")
                .as_ref(),
            frame.sun.sky_amount(),
        );
        gl.draw_arrays(Gl::TRIANGLES, 0, 3);
        gl.depth_mask(true);
        gl.enable(Gl::DEPTH_TEST);
    }

    pub fn uniforms(&self, program: &WebGlProgram, frame: &LightFrame) {
        let gl = &self.gl;
        gl.uniform1i(
            gl.get_uniform_location(program, "studio").as_ref(),
            frame.studio as i32,
        );
        gl.uniform1i(
            gl.get_uniform_location(program, "point_light").as_ref(),
            frame.point as i32,
        );
        gl.active_texture(Gl::TEXTURE0);
        gl.bind_texture(Gl::TEXTURE_2D, Some(&self.depth));
        gl.uniform1i(gl.get_uniform_location(program, "shadow_map").as_ref(), 0);
        gl.uniform1f(
            gl.get_uniform_location(program, "shadow_texel").as_ref(),
            1.0 / self.size as f32,
        );
        uniform_matrix(gl, program, "shadow_matrix", frame.matrix);
        uniform_vector(gl, program, "sun_direction", frame.sun.direction.as_vec3());
        uniform_vector(gl, program, "light_position", frame.position);
        uniform_vector(gl, program, "light_forward", frame.forward);
        gl.uniform1i(
            gl.get_uniform_location(program, "flashlight").as_ref(),
            (!frame.sun.daylight()) as i32,
        );
        gl.uniform1f(
            gl.get_uniform_location(program, "sky_amount").as_ref(),
            frame.sun.sky_amount(),
        );
    }
}

fn uniform_matrix(gl: &Gl, program: &WebGlProgram, name: &str, matrix: Mat4) {
    gl.uniform_matrix4fv_with_f32_array(
        gl.get_uniform_location(program, name).as_ref(),
        false,
        &matrix.to_cols_array(),
    );
}
fn uniform_vector(gl: &Gl, program: &WebGlProgram, name: &str, vector: Vec3) {
    gl.uniform3f(
        gl.get_uniform_location(program, name).as_ref(),
        vector.x,
        vector.y,
        vector.z,
    );
}
impl Drop for LightPass {
    fn drop(&mut self) {
        self.gl.delete_framebuffer(Some(&self.framebuffer));
        self.gl.delete_texture(Some(&self.depth));
        self.gl.delete_program(Some(&self.shadow_program));
        self.gl.delete_program(Some(&self.sky_program));
    }
}
