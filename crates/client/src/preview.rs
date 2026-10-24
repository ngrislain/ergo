//! Read-only landing illustration using the navigator's renderer and catalog.
use crate::renderer::Renderer;
use ergo_world::{Brick, demo::Demo};
use wasm_bindgen::prelude::*;
use web_sys::HtmlCanvasElement;

#[wasm_bindgen]
pub struct LandingPreview {
    renderer: Renderer,
    scene: Demo,
    added: usize,
    distance: f64,
    previous_assembly: f64,
}

#[wasm_bindgen]
impl LandingPreview {
    #[wasm_bindgen(constructor)]
    pub fn new(canvas: HtmlCanvasElement) -> Result<LandingPreview, JsValue> {
        console_error_panic_hook::set_once();
        let mut renderer = Renderer::new_preview(canvas)?;
        let scene = doorway();
        renderer.meshes = scene.meshes.clone();
        renderer.set_bricks(scene.bricks.clone());
        renderer.fit(ergo_world::bounds(&scene.bricks).ok_or("Empty landing illustration")?);
        renderer.grid = false;
        renderer.ground_visible = false;
        let distance = renderer.distance;
        Ok(Self {
            renderer,
            scene,
            added: 0,
            distance,
            previous_assembly: -1.0,
        })
    }

    pub fn piece_count(&self) -> usize {
        self.scene.bricks.len()
    }

    pub fn add_brick(&mut self) -> usize {
        if self.added < 5 {
            let colors = [14, 4, 28, 1, 14];
            self.scene.add(
                "3003",
                colors[self.added],
                1.0,
                1.0 / 3.0 + self.added as f64,
                9.0,
            );
            self.added += 1;
            self.renderer.meshes = self.scene.meshes.clone();
            self.renderer.set_bricks(self.scene.bricks.clone());
            self.previous_assembly = -1.0;
        }
        self.piece_count()
    }

    pub fn render(
        &mut self,
        yaw: f64,
        pitch: f64,
        explosion: f64,
        assembly: f64,
        zoom: f64,
    ) -> Result<(), JsValue> {
        if ![yaw, pitch, explosion, assembly, zoom]
            .iter()
            .all(|x| x.is_finite())
        {
            return Err("Invalid preview view".into());
        }
        let assembly = assembly.clamp(0.0, 1.0);
        if assembly != self.previous_assembly {
            let count = self.scene.bricks.len();
            let bricks: Vec<Brick> = self
                .scene
                .bricks
                .iter()
                .enumerate()
                .map(|(i, original)| {
                    let mut brick = original.clone();
                    if i >= 12 {
                        let progress =
                            (assembly * 1.4 - i as f64 / count as f64 * 0.4).clamp(0.0, 1.0);
                        let offset =
                            glam::DVec3::Y * (1.0 - progress).powi(4) * (14.0 + (i % 3) as f64);
                        brick.position += offset;
                        brick.bounds = ergo_world::Aabb::new(
                            brick.bounds.min + offset,
                            brick.bounds.max + offset,
                        );
                        brick.render_bounds = ergo_world::Aabb::new(
                            brick.render_bounds.min + offset,
                            brick.render_bounds.max + offset,
                        );
                    }
                    brick
                })
                .collect();
            self.renderer.set_bricks(bricks);
            self.previous_assembly = assembly;
        }
        let aspect = self.renderer.canvas.client_width().max(1) as f64
            / self.renderer.canvas.client_height().max(1) as f64;
        let explosion = explosion.clamp(0.0, 1.0);
        let distance = self.distance
            * (1.15 / aspect).max(1.0)
            * (1.0 + explosion * 0.25)
            * zoom.clamp(0.55, 2.5);
        let explosion = explosion * 0.55;
        let pitch = pitch.clamp(-1.4, 1.4);
        if self.renderer.yaw != yaw
            || self.renderer.pitch != pitch
            || self.renderer.distance != distance
            || self.renderer.explode != explosion
        {
            self.renderer.yaw = yaw;
            self.renderer.pitch = pitch;
            self.renderer.distance = distance;
            self.renderer.explode = explosion;
            self.renderer.changed();
        }
        self.renderer.render();
        Ok(())
    }
}

fn doorway() -> Demo {
    let mut scene = Demo::new();
    for x in [0.0, 4.0, 8.0, 12.0] {
        for z in [0.0, 4.0, 8.0] {
            scene.add("3031", 71, x, 0.0, z);
        }
    }
    let base = 1.0 / 3.0;
    scene.add("3020", 15, 6.0, base, 7.0);
    scene.add("3001", 1, 6.0, base, 5.0);
    for layer in 0..6 {
        scene.add(
            "3003",
            if layer == 0 { 15 } else { 4 },
            4.0,
            base + layer as f64,
            3.0,
        );
        scene.add(
            "3003",
            if layer == 0 { 15 } else { 1 },
            10.0,
            base + layer as f64,
            3.0,
        );
    }
    scene.add("3007", 14, 4.0, base + 6.0, 3.0);
    scene.add("3001", 14, 6.0, base + 7.0, 3.0);
    scene.add("3003", 4, 7.0, base + 8.0, 3.0);
    for layer in 0..3 {
        scene.add("3005", 70, 1.0, base + layer as f64, 1.0);
    }
    scene.add("3001", 28, 0.0, base + 3.0, 0.0);
    scene.add("3003", 28, 1.0, base + 4.0, 0.0);
    scene.add("3004", 1, 12.0, base, 8.0);
    scene.add("3004", 1, 12.0, base, 9.0);
    scene.add("3003", 14, 12.0, base + 1.0, 8.0);
    scene.add("3003", 14, 12.0, base + 2.0, 8.0);
    scene.add("3022", 4, 12.0, base + 3.0, 8.0);
    scene.add("3005", 14, 11.0, base + 1.0, 8.0);
    scene.add("3005", 14, 14.0, base + 1.0, 8.0);
    scene.add("3020", 1, 0.0, base, 7.0);
    scene.add("3003", 4, 13.0, base, 1.0);
    scene
}
