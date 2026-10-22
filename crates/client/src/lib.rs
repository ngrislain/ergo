pub mod navigation;
#[cfg(target_arch = "wasm32")]
pub mod renderer;
#[cfg(all(target_arch = "wasm32", feature = "mcp-entry"))]
mod runtime;
pub mod walking;
