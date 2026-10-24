pub mod navigation;
pub mod lighting;
#[cfg(target_arch = "wasm32")]
pub mod renderer;
#[cfg(all(target_arch = "wasm32", feature = "mcp-entry"))]
mod runtime;
#[cfg(all(target_arch = "wasm32", feature = "site-entry"))]
mod preview;
pub mod walking;
