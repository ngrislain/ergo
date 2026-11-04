//! Persistent content-addressed browser data. A fresh authorized HTTP 304 is
//! still required before cached bytes are displayed. Cache writes are optional.
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;

#[wasm_bindgen(module = "/src/browser_cache.js")]
extern "C" {
    #[wasm_bindgen(js_name = cacheGet)]
    fn get_entry(url: &str) -> js_sys::Promise;
    #[wasm_bindgen(js_name = cachePut)]
    fn put_entry(url: &str, etag: &str, bytes: &js_sys::Uint8Array) -> js_sys::Promise;
    #[wasm_bindgen(js_name = cacheForget)]
    fn forget_entry(url: &str) -> js_sys::Promise;
    #[wasm_bindgen(js_name = cachePin)]
    pub(super) fn pin(url: &str);
    #[wasm_bindgen(js_name = cacheSuspend)]
    pub(crate) fn suspend();
    #[wasm_bindgen(js_name = cacheResume)]
    pub(crate) fn resume();
    #[wasm_bindgen(js_name = requestPersistence)]
    fn persist_storage() -> js_sys::Promise;
    #[wasm_bindgen(js_name = warmBasicLibrary)]
    fn warm_basic_library(revision: &str) -> js_sys::Promise;
}
pub(super) fn warm_library(revision: &str) {
    let promise = warm_basic_library(revision);
    wasm_bindgen_futures::spawn_local(async move {
        let _ = JsFuture::from(promise).await;
    });
}
pub(super) struct Cached {
    pub etag: String,
    pub bytes: Vec<u8>,
}
pub(super) async fn get(url: &str) -> Option<Cached> {
    let value = JsFuture::from(get_entry(url)).await.ok()?;
    if value.is_null() || value.is_undefined() {
        return None;
    }
    Some(Cached {
        etag: js_sys::Reflect::get(&value, &"etag".into())
            .ok()?
            .as_string()?,
        bytes: js_sys::Uint8Array::new(&js_sys::Reflect::get(&value, &"bytes".into()).ok()?)
            .to_vec(),
    })
}
/// Public floor colour fields have an independent seven-day freshness policy;
/// this never supplies cached construction geometry or authorizes a world read.
pub(super) async fn ground(url: &str) -> Option<ergo_world::terrain::GroundBlock> {
    let cached = get(url).await?;
    let block: ergo_world::terrain::GroundBlock = serde_json::from_slice(&cached.bytes).ok()?;
    let now = (js_sys::Date::now() / 1000.) as u64;
    (block.validate().is_ok()
        && block.surface.is_some()
        && block.fallback_reason.is_none()
        && block.generated_at <= now + 300
        && now.saturating_sub(block.generated_at) < 7 * 86400)
        .then_some(block)
}
pub(super) fn forget(url: &str) {
    let promise = forget_entry(url);
    wasm_bindgen_futures::spawn_local(async move {
        let _ = JsFuture::from(promise).await;
    });
}
pub(super) fn put_background(url: String, etag: String, bytes: Vec<u8>) {
    let data = js_sys::Uint8Array::from(bytes.as_slice());
    let promise = put_entry(&url, &etag, &data);
    wasm_bindgen_futures::spawn_local(async move {
        let _ = JsFuture::from(promise).await;
    });
}
pub(super) fn request_persistence() {
    wasm_bindgen_futures::spawn_local(async {
        let _ = JsFuture::from(persist_storage()).await;
    });
}
