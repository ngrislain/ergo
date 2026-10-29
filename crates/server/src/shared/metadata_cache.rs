//! Small, revision-keyed discovery responses; authorization always precedes hits.
use super::*;
use bytes::Bytes;
use std::collections::VecDeque;

#[derive(Default)]
pub(super) struct MetadataCache {
    entries: VecDeque<(String, Bytes)>,
    bytes: usize,
}
impl MetadataCache {
    pub fn get(&mut self, key: &str) -> Option<Bytes> {
        let index = self
            .entries
            .iter()
            .position(|(candidate, _)| candidate == key)?;
        let entry = self.entries.remove(index)?;
        let bytes = entry.1.clone();
        self.entries.push_back(entry);
        Some(bytes)
    }
    pub fn insert(&mut self, key: String, bytes: Bytes) {
        if bytes.len() > 512 * 1024 {
            return;
        }
        if let Some(index) = self
            .entries
            .iter()
            .position(|(candidate, _)| *candidate == key)
        {
            self.bytes -= self.entries.remove(index).unwrap().1.len();
        }
        while self.bytes + bytes.len() > 4 * 1024 * 1024 {
            if let Some((_, old)) = self.entries.pop_front() {
                self.bytes -= old.len();
            } else {
                break;
            }
        }
        self.bytes += bytes.len();
        self.entries.push_back((key, bytes));
    }
}
pub(super) fn unchanged(headers: &HeaderMap, etag: &str) -> bool {
    headers
        .get("if-none-match")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| {
            value.split(',').any(|value| {
                value.trim() == "*"
                    || value.trim().trim_start_matches("W/") == etag.trim_start_matches("W/")
            })
        })
}
pub(super) fn response(bytes: Option<Bytes>, etag: &str, source: &str) -> Response {
    let mut response = bytes.map_or_else(
        || StatusCode::NOT_MODIFIED.into_response(),
        IntoResponse::into_response,
    );
    response.headers_mut().insert("etag", etag.parse().unwrap());
    response
        .headers_mut()
        .insert("cache-control", "private, no-cache".parse().unwrap());
    response
        .headers_mut()
        .insert("content-type", "application/json".parse().unwrap());
    response
        .headers_mut()
        .insert("x-ergo-metadata-cache", source.parse().unwrap());
    response
}
