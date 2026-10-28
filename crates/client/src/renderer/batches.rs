//! Shared immutable geometry, with independently culled color/depth instances.
use crate::scene::{FramePlan, GeometryKey};
use ergo_world::geometry::Mesh;
use std::collections::{BTreeMap, HashSet};
use web_sys::{WebGl2RenderingContext as Gl, WebGlBuffer, WebGlVertexArrayObject};

#[derive(Clone, Copy)]
pub(super) enum Pass {
    Color = 0,
    Shadow = 1,
}

struct Instances {
    vao: WebGlVertexArrayObject,
    buffer: WebGlBuffer,
    data: Vec<f32>,
}
pub(super) struct Geometry {
    gl: Gl,
    vertex_buffer: WebGlBuffer,
    vertices: i32,
    stud_mode: i32,
    stud_offset: [f32; 2],
    instances: [Instances; 2],
}
impl Geometry {
    fn new(gl: &Gl, mesh: &Mesh) -> Self {
        let vertex_buffer = gl.create_buffer().expect("WebGL geometry allocation");
        gl.bind_buffer(Gl::ARRAY_BUFFER, Some(&vertex_buffer));
        gl.buffer_data_with_array_buffer_view(
            Gl::ARRAY_BUFFER,
            &js_sys::Float32Array::from(mesh.vertices.as_slice()),
            Gl::STATIC_DRAW,
        );
        let instances = std::array::from_fn(|_| {
            let vao = gl.create_vertex_array().expect("WebGL VAO allocation");
            gl.bind_vertex_array(Some(&vao));
            gl.bind_buffer(Gl::ARRAY_BUFFER, Some(&vertex_buffer));
            for i in 0..3 {
                gl.enable_vertex_attrib_array(i);
                gl.vertex_attrib_pointer_with_i32(i, 3, Gl::FLOAT, false, 36, (i * 12) as i32);
            }
            let buffer = gl.create_buffer().expect("WebGL instance allocation");
            gl.bind_buffer(Gl::ARRAY_BUFFER, Some(&buffer));
            for i in 0..4 {
                gl.enable_vertex_attrib_array(3 + i);
                gl.vertex_attrib_pointer_with_i32(3 + i, 4, Gl::FLOAT, false, 80, (i * 16) as i32);
                gl.vertex_attrib_divisor(3 + i, 1);
            }
            gl.enable_vertex_attrib_array(7);
            gl.vertex_attrib_pointer_with_i32(7, 4, Gl::FLOAT, false, 80, 64);
            gl.vertex_attrib_divisor(7, 1);
            Instances {
                vao,
                buffer,
                data: Vec::new(),
            }
        });
        let spec = crate::part_lod::RenderPart::resolve(mesh).and_then(|part| part.rectangular());
        Self {
            gl: gl.clone(),
            vertex_buffer,
            vertices: (mesh.vertices.len() / 9) as i32,
            stud_mode: spec
                .filter(|p| p.studs)
                .map_or(0, |_| if mesh.quality == "body_lod" { 2 } else { 1 }),
            stud_offset: spec.map_or([0., 0.], |p| [p.width as f32 * 0.5, p.depth as f32 * 0.5]),
            instances,
        }
    }
    fn update(&mut self, pass: Pass, data: &[f32]) -> bool {
        let draw = &mut self.instances[pass as usize];
        if draw.data == data {
            return false;
        }
        self.gl.bind_vertex_array(Some(&draw.vao));
        self.gl.bind_buffer(Gl::ARRAY_BUFFER, Some(&draw.buffer));
        self.gl.buffer_data_with_array_buffer_view(
            Gl::ARRAY_BUFFER,
            &js_sys::Float32Array::from(data),
            Gl::DYNAMIC_DRAW,
        );
        draw.data.clear();
        draw.data.extend_from_slice(data);
        true
    }
}
impl Drop for Geometry {
    fn drop(&mut self) {
        self.gl.delete_buffer(Some(&self.vertex_buffer));
        for draw in &self.instances {
            self.gl.delete_buffer(Some(&draw.buffer));
            self.gl.delete_vertex_array(Some(&draw.vao));
        }
    }
}

#[derive(Default)]
pub(super) struct Batches {
    geometry: BTreeMap<GeometryKey, Geometry>,
    pub uploaded_bytes: usize,
    pub shadow_changed: bool,
    pending_shadow_invalidation: bool,
}
impl Batches {
    pub fn clear(&mut self) {
        self.pending_shadow_invalidation |= self
            .geometry
            .values()
            .any(|mesh| !mesh.instances[Pass::Shadow as usize].data.is_empty());
        self.geometry.clear();
    }
    pub fn invalidate_parts(&mut self, changed: &HashSet<String>) {
        self.geometry.retain(|key, mesh| {
            let keep = !matches!(key,GeometryKey::Part(part,_) if changed.contains(part));
            if !keep && !mesh.instances[Pass::Shadow as usize].data.is_empty() {
                self.pending_shadow_invalidation = true;
            }
            keep
        });
    }
    pub fn begin(&mut self, plan: &FramePlan) {
        self.uploaded_bytes = 0;
        self.shadow_changed = std::mem::take(&mut self.pending_shadow_invalidation);
        self.geometry.retain(|key, mesh| {
            let keep = plan.color.contains_key(key) || plan.shadow.contains_key(key);
            if !keep && !mesh.instances[Pass::Shadow as usize].data.is_empty() {
                self.shadow_changed = true;
            }
            keep
        });
    }
    pub fn update(&mut self, gl: &Gl, key: GeometryKey, mesh: &Mesh, plan: &FramePlan) {
        let gpu = self
            .geometry
            .entry(key.clone())
            .or_insert_with(|| Geometry::new(gl, mesh));
        for (pass, list) in [(Pass::Color, &plan.color), (Pass::Shadow, &plan.shadow)] {
            let data = list.get(&key).map_or(&[][..], Vec::as_slice);
            if gpu.update(pass, data) {
                self.uploaded_bytes += data.len() * 4;
                if matches!(pass, Pass::Shadow) {
                    self.shadow_changed = true;
                }
            }
        }
    }
    pub fn bytes(&self) -> usize {
        self.geometry
            .values()
            .map(|mesh| {
                mesh.vertices as usize * 36
                    + mesh
                        .instances
                        .iter()
                        .map(|i| i.data.len() * 4)
                        .sum::<usize>()
            })
            .sum()
    }
    /// Both passes consume the same prototype cache and attribute layout.
    pub fn draw(
        &self,
        gl: &Gl,
        program: &web_sys::WebGlProgram,
        pass: Pass,
        full_detail: bool,
    ) -> (usize, usize, usize) {
        let mut calls = 0;
        let mut triangles = 0;
        let mut instances = 0;
        for (key, mesh) in &self.geometry {
            let draw = &mesh.instances[pass as usize];
            let count = (draw.data.len() / 20) as i32;
            if count == 0 {
                continue;
            }
            let stud_mode = if full_detail && mesh.stud_mode == 1 {
                3
            } else {
                mesh.stud_mode
            };
            gl.uniform1i(
                gl.get_uniform_location(program, "stud_mode").as_ref(),
                stud_mode,
            );
            if matches!(pass, Pass::Color) {
                gl.uniform2f(
                    gl.get_uniform_location(program, "stud_offset").as_ref(),
                    mesh.stud_offset[0],
                    mesh.stud_offset[1],
                );
                gl.uniform1i(
                    gl.get_uniform_location(program, "floor_mode").as_ref(),
                    matches!(key, GeometryKey::Ground) as i32,
                );
            }
            gl.bind_vertex_array(Some(&draw.vao));
            gl.draw_arrays_instanced(Gl::TRIANGLES, 0, mesh.vertices, count);
            calls += 1;
            triangles += mesh.vertices as usize * count as usize / 3;
            instances += count as usize;
        }
        gl.bind_vertex_array(None);
        (calls, triangles, instances)
    }
}
