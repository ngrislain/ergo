//! Persistent local construction/index packets. Bytes are usable only after a
//! fresh authorized HTTP 304; offline/error responses never authorize a hit.
use sha2::{Digest, Sha256};
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
const NAME: &str = "ergo-read-packets-v1";
const MAX_ENTRY: usize = 64 * 1024 * 1024;
const BUDGET: usize = 128 * 1024 * 1024;

pub(super) struct Cached {
    pub etag: String,
    pub bytes: Vec<u8>,
}
async fn cache() -> Option<web_sys::Cache> {
    let storage = web_sys::window()?.caches().ok()?;
    JsFuture::from(storage.open(NAME))
        .await
        .ok()?
        .dyn_into()
        .ok()
}
fn key(url: &str) -> Option<String> {
    let window = web_sys::window()?;
    let parsed = web_sys::Url::new_with_base(url, &window.location().href().ok()?).ok()?;
    for parameter in ["from_x", "from_y", "from_z"] {
        parsed.search_params().delete(parameter);
    }
    Some(parsed.href())
}
pub(super) async fn get(url: &str) -> Option<Cached> {
    let cache = cache().await?;
    let key = key(url)?;
    let response = JsFuture::from(cache.match_with_str(&key))
        .await
        .ok()?
        .dyn_into::<web_sys::Response>()
        .ok()?;
    let bytes = response
        .headers()
        .get("x-ergo-local-bytes")
        .ok()??
        .parse::<usize>()
        .ok()?;
    if bytes > MAX_ENTRY {
        return None;
    }
    let etag = response.headers().get("etag").ok()??;
    let body = JsFuture::from(response.array_buffer().ok()?).await.ok()?;
    let body = js_sys::Uint8Array::new(&body).to_vec();
    let checksum = response.headers().get("x-ergo-local-sha256").ok()??;
    if body.len() != bytes || format!("{:x}", Sha256::digest(&body)) != checksum {
        return None;
    }
    Some(Cached { etag, bytes: body })
}
pub(super) async fn forget(url: &str) {
    if let (Some(cache), Some(key)) = (cache().await, key(url)) {
        let _ = JsFuture::from(cache.delete_with_str(&key)).await;
    }
}
pub(super) async fn put(url: &str, etag: &str, bytes: &[u8]) {
    if bytes.len() > MAX_ENTRY {
        return;
    }
    let Some(cache) = cache().await else {
        return;
    };
    let Some(key) = key(url) else {
        return;
    };
    let Ok(keys) = JsFuture::from(cache.keys()).await else {
        return;
    };
    let mut entries = Vec::new();
    let mut size = 0usize;
    for request in js_sys::Array::from(&keys).iter() {
        let Ok(request) = request.dyn_into::<web_sys::Request>() else {
            continue;
        };
        if request.url() == key {
            continue;
        }
        let Ok(value) = JsFuture::from(cache.match_with_request(&request)).await else {
            continue;
        };
        let Ok(response) = value.dyn_into::<web_sys::Response>() else {
            continue;
        };
        let amount = response
            .headers()
            .get("x-ergo-local-bytes")
            .ok()
            .flatten()
            .and_then(|n| n.parse::<usize>().ok())
            .unwrap_or(MAX_ENTRY);
        size = size.saturating_add(amount);
        entries.push((request, amount));
    }
    for (request, amount) in entries {
        if size.saturating_add(bytes.len()) <= BUDGET {
            break;
        }
        if JsFuture::from(cache.delete_with_request(&request))
            .await
            .is_ok()
        {
            size = size.saturating_sub(amount);
        }
    }
    let Ok(headers) = web_sys::Headers::new() else {
        return;
    };
    if headers.set("etag", etag).is_err()
        || headers.set("content-type", "application/cbor").is_err()
        || headers
            .set("x-ergo-local-bytes", &bytes.len().to_string())
            .is_err()
        || headers
            .set(
                "x-ergo-local-sha256",
                &format!("{:x}", Sha256::digest(bytes)),
            )
            .is_err()
    {
        return;
    }
    let options = web_sys::ResponseInit::new();
    options.set_headers(&headers);
    let mut data = bytes.to_vec();
    let Ok(response) = web_sys::Response::new_with_opt_u8_array_and_init(Some(&mut data), &options)
    else {
        return;
    };
    let _ = JsFuture::from(cache.put_with_str(&key, &response)).await;
}
