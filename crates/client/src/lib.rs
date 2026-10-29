pub mod lighting;
pub mod lod;
pub mod navigation;
pub mod normals;
pub mod part_lod;
#[cfg(all(target_arch = "wasm32", feature = "site-entry"))]
mod preview;
#[cfg(target_arch = "wasm32")]
pub mod renderer;
#[cfg(all(target_arch = "wasm32", feature = "mcp-entry"))]
mod runtime;
pub mod scene;
pub mod walking;
