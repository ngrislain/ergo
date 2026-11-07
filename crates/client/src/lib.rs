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

/// Verify and install a published part library (or its reviewed tier) before
/// creating any runtime or preview.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(js_name = installPartLibrary)]
pub fn install_part_library(json: &[u8]) -> Result<(), wasm_bindgen::JsValue> {
    ergo_world::part_library::install_json(json)
        .map_err(|e| wasm_bindgen::JsValue::from_str(&format!("{e:#}")))
}
