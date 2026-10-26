//! On-demand raster sampling for ground blocks. No bulk prefetch, rendering, or
//! simulation. Satellite/aerial defaults include attribution, stable identification, >=7-day
//! source caching, conditional refresh, and serialized/coalesced source fetches.
use anyhow::{Context, Result, ensure};
use ergo_world::{
    BLOCK_SIZE, GeoReference,
    terrain::{GroundBlock, SAMPLE_GRID_SIZE, average_rgb, nearest_color},
};
use futures::StreamExt;
use object_store::{ObjectStore, path::Path};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashMap},
    io::Cursor,
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
const TTL: u64 = 7 * 86400;
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Tile {
    format: String,
    fetched_at: u64,
    etag: Option<String>,
    modified: Option<String>,
    media_type: String,
    #[serde(with = "serde_bytes")]
    image: Vec<u8>,
}
#[derive(Clone)]
pub struct GroundCache {
    store: Arc<dyn ObjectStore>,
    prefix: String,
    http: reqwest::Client,
    template: String,
    provider: String,
    attribution: String,
    attribution_url: String,
    enabled: bool,
    gate: Arc<tokio::sync::Mutex<()>>,
    failures: Arc<std::sync::Mutex<HashMap<String, (Instant, String)>>>,
}
impl GroundCache {
    pub fn provider(&self) -> &str {
        &self.provider
    }
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }
    pub fn new(store: Arc<dyn ObjectStore>, prefix: String) -> Result<Self> {
        let source = std::env::var("ERGO_GROUND_SOURCE").unwrap_or_else(|_| "satellite".into());
        ensure!(
            ["satellite", "osm", "disabled"].contains(&source.as_str()),
            "ERGO_GROUND_SOURCE must be satellite, osm, or disabled"
        );
        let satellite = source != "osm";
        let template = std::env::var("ERGO_MAP_TILE_URL")
            .unwrap_or_else(|_| if satellite {"https://server.arcgisonline.com/ArcGIS/rest/services/World_Imagery/MapServer/tile/{z}/{y}/{x}".into()}else{"https://tile.openstreetmap.org/{z}/{x}/{y}.png".into()});
        ensure!(
            ["{z}", "{x}", "{y}"].iter().all(|p| template.contains(p)),
            "Map tile URL must have z/x/y placeholders"
        );
        let agent = std::env::var("ERGO_MAP_USER_AGENT")
            .unwrap_or_else(|_| "Ergo/0.1 interactive brick-world ground viewer".into());
        let http = reqwest::Client::builder()
            .user_agent(agent)
            .timeout(Duration::from_secs(5))
            .build()?;
        Ok(Self {
            store,
            prefix,
            http,
            template,
            provider: std::env::var("ERGO_MAP_PROVIDER")
                .unwrap_or_else(|_| if satellite {"Esri World Imagery (satellite/aerial)".into()}else{"OpenStreetMap Carto".into()}),
            attribution: std::env::var("ERGO_MAP_ATTRIBUTION")
                .unwrap_or_else(|_| if satellite {"Tiles © Esri — Source: Esri, Maxar, Earthstar Geographics, and the GIS User Community".into()}else{"© OpenStreetMap contributors".into()}),
            attribution_url: std::env::var("ERGO_MAP_ATTRIBUTION_URL")
                .unwrap_or_else(|_| if satellite {"https://www.arcgis.com/home/item.html?id=10df2279f9684e4a9f6a7f08febac2a9".into()}else{"https://www.openstreetmap.org/copyright".into()}),
            enabled: source!="disabled",
            gate: Arc::new(tokio::sync::Mutex::new(())),
            failures: Arc::new(std::sync::Mutex::new(HashMap::new())),
        })
    }
    fn path(&self, key: &str) -> Path {
        Path::from(if self.prefix.is_empty() {
            key.into()
        } else {
            format!("{}/{key}", self.prefix)
        })
    }
    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>> {
        match self.store.get(&self.path(key)).await {
            Ok(r) => Ok(Some(r.bytes().await?.to_vec())),
            Err(object_store::Error::NotFound { .. }) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }
    async fn put<T: Serialize>(&self, key: &str, value: &T) -> Result<()> {
        let mut data = Vec::new();
        ciborium::into_writer(value, &mut data)?;
        self.store.put(&self.path(key), data.into()).await?;
        Ok(())
    }
    pub async fn block(&self, geo: GeoReference, x: i64, y: i64) -> GroundBlock {
        let key = match self.block_key(geo, x, y) {
            Ok(key) => key,
            Err(e) => return GroundBlock::fallback(x, y, e.to_string()),
        };
        if let Ok(Some(bytes)) = self.get(&key).await
            && let Ok(block) = ciborium::from_reader::<GroundBlock, _>(bytes.as_slice())
        {
            let ttl = if block.fallback_reason.is_some() {
                60
            } else {
                TTL
            };
            if now().saturating_sub(block.generated_at) < ttl {
                return block;
            }
        }
        let result = if self.enabled {
            self.generate(geo, x, y).await
        } else {
            Err(anyhow::anyhow!("Map sampling is disabled"))
        };
        match result {
            Ok(block) => block,
            Err(e) => {
                let mut block =
                    GroundBlock::fallback(x, y, format!("Map source unavailable: {e:#}"));
                block.generated_at = now();
                let _ = self.put(&key, &block).await;
                block
            }
        }
    }
    fn block_key(&self, geo: GeoReference, x: i64, y: i64) -> Result<String> {
        geo.validate()?;
        ensure!(
            x.unsigned_abs() < 1_000_000_000 && y.unsigned_abs() < 1_000_000_000,
            "Ground cell out of range"
        );
        let palette: Vec<_> = ergo_world::catalog::COLORS
            .iter()
            .map(|(code, _)| (*code, ergo_world::color_rgb(*code)))
            .collect();
        let signature = hex::encode(Sha256::digest(format!(
            "{:.12}:{:.12}:{:.12}:{}:{}:{}:{}:{}:ground-average-v3:{palette:?}",
            geo.latitude,
            geo.longitude,
            geo.arcseconds_per_block,
            self.template,
            self.provider,
            self.attribution,
            self.attribution_url,
            self.enabled
        )));
        Ok(format!("ground/blocks/{signature}/{x}_{y}.cbor"))
    }
    async fn generate(&self, geo: GeoReference, x: i64, y: i64) -> Result<GroundBlock> {
        let _guard = self.gate.lock().await;
        let key = self.block_key(geo, x, y)?;
        if let Some(bytes) = self.get(&key).await? {
            let block: GroundBlock = ciborium::from_reader(bytes.as_slice())?;
            let ttl = if block.fallback_reason.is_some() {
                60
            } else {
                TTL
            };
            if now().saturating_sub(block.generated_at) < ttl {
                return Ok(block);
            }
        }
        let [latitude, _] = geo.to_gps(glam_point(
            (x as f64 + 0.5) * BLOCK_SIZE,
            (y as f64 + 0.5) * BLOCK_SIZE,
        ));
        let zoom = (648000.0 * latitude.to_radians().cos().max(0.01) / geo.arcseconds_per_block)
            .log2()
            .floor()
            .clamp(0.0, 18.0) as u32;
        let mut locations = Vec::new();
        let mut needed = BTreeMap::new();
        for row in 0..SAMPLE_GRID_SIZE {
            for column in 0..SAMPLE_GRID_SIZE {
                let p = glam_point(
                    (x as f64 + (column as f64 + 0.5) / SAMPLE_GRID_SIZE as f64) * BLOCK_SIZE,
                    (y as f64 + (row as f64 + 0.5) / SAMPLE_GRID_SIZE as f64) * BLOCK_SIZE,
                );
                let [lat, lon] = geo.to_gps(p);
                let (tx, ty, px, py) = mercator(lat, lon, zoom)?;
                locations.push((tx, ty, px, py));
                needed.entry((tx, ty)).or_insert(());
            }
        }
        ensure!(
            needed.len() <= 16,
            "Ground cell requires too many source tiles; use a coarser licensed source"
        );
        let mut decoded = BTreeMap::new();
        let mut sources = Vec::new();
        for &(tx, ty) in needed.keys() {
            let bytes = self.tile(zoom, tx, ty).await?;
            decoded.insert((tx, ty), decode(&bytes)?);
            sources.push(format!("{}: z{zoom}/{tx}/{ty}", self.provider));
        }
        let rgb = average_rgb(locations.into_iter().map(|(tx, ty, px, py)| {
            let (width, height, bytes) = &decoded[&(tx, ty)];
            let index = ((py.min(*height - 1) * width + px.min(*width - 1)) * 3) as usize;
            ((bytes[index] as u32) << 16)
                | ((bytes[index + 1] as u32) << 8)
                | bytes[index + 2] as u32
        }));
        let block = GroundBlock {
            schema: ergo_world::SCHEMA_VERSION,
            coord: ergo_world::BlockCoord::from([x, y, 0]),
            bricks: Vec::new(),
            color: nearest_color(rgb),
            sample_rgb: Some(rgb),
            source: self.provider.clone(),
            attribution: self.attribution.clone(),
            attribution_url: Some(self.attribution_url.clone()),
            generated_at: now(),
            source_tiles: sources,
            fallback_reason: None,
        };
        self.put(&key, &block).await?;
        Ok(block)
    }
    async fn tile(&self, zoom: u32, x: u32, y: u32) -> Result<Vec<u8>> {
        let provider = hex::encode(Sha256::digest(&self.template));
        let key = format!("ground/rasters/{provider}/{zoom}/{x}/{y}.cbor");
        if let Some((_, reason)) = self
            .failures
            .lock()
            .unwrap()
            .get(&key)
            .cloned()
            .filter(|(t, _)| t.elapsed() < Duration::from_secs(60))
        {
            anyhow::bail!(reason);
        }
        let old: Option<Tile> = self
            .get(&key)
            .await?
            .map(|data| ciborium::from_reader(data.as_slice()))
            .transpose()?;
        if let Some(tile) = &old {
            ensure!(
                tile.format == "ergo-raster-1"
                    && ["image/png", "image/jpeg"].contains(&tile.media_type.as_str()),
                "Unsupported Ergo raster description"
            );
        }
        if let Some(tile) = &old
            && now().saturating_sub(tile.fetched_at) < TTL
        {
            return Ok(tile.image.clone());
        }
        let url = self
            .template
            .replace("{z}", &zoom.to_string())
            .replace("{x}", &x.to_string())
            .replace("{y}", &y.to_string());
        let mut request = self.http.get(url);
        let origin =
            std::env::var("ERGO_PUBLIC_URL").unwrap_or_else(|_| "http://127.0.0.1:3000".into());
        request = request.header(
            reqwest::header::REFERER,
            format!("{}/", origin.trim_end_matches('/')),
        );
        if let Some(tile) = &old {
            if let Some(etag) = &tile.etag {
                request = request.header(reqwest::header::IF_NONE_MATCH, etag);
            }
            if let Some(date) = &tile.modified {
                request = request.header(reqwest::header::IF_MODIFIED_SINCE, date);
            }
        }
        let result: Result<Tile> = async {
            let response = request.send().await.map_err(reqwest::Error::without_url)?;
            if response.status() == reqwest::StatusCode::NOT_MODIFIED {
                let mut tile = old.context("Unexpected 304")?;
                tile.fetched_at = now();
                return Ok(tile);
            }
            let response = response
                .error_for_status()
                .map_err(reqwest::Error::without_url)?;
            let etag = response
                .headers()
                .get(reqwest::header::ETAG)
                .and_then(|v| v.to_str().ok())
                .map(str::to_owned);
            let modified = response
                .headers()
                .get(reqwest::header::LAST_MODIFIED)
                .and_then(|v| v.to_str().ok())
                .map(str::to_owned);
            let mut stream = response.bytes_stream();
            let mut png = Vec::new();
            while let Some(chunk) = stream.next().await {
                let chunk = chunk.map_err(reqwest::Error::without_url)?;
                ensure!(
                    png.len() + chunk.len() <= 1024 * 1024,
                    "Raster tile exceeds 1 MiB"
                );
                png.extend_from_slice(&chunk);
            }
            decode(&png)?;
            Ok(Tile {
                format: "ergo-raster-1".into(),
                fetched_at: now(),
                etag,
                modified,
                media_type: if png.starts_with(b"\x89PNG") {
                    "image/png"
                } else {
                    "image/jpeg"
                }
                .into(),
                image: png,
            })
        }
        .await;
        match result {
            Ok(tile) => {
                self.put(&key, &tile).await?;
                Ok(tile.image)
            }
            Err(e) => {
                let mut failures = self.failures.lock().unwrap();
                failures.retain(|_, (when, _)| when.elapsed() < Duration::from_secs(60));
                if failures.len() > 512 {
                    failures.clear();
                }
                failures.insert(key, (Instant::now(), e.to_string()));
                Err(e)
            }
        }
    }
}
fn glam_point(x: f64, north: f64) -> glam::DVec3 {
    glam::DVec3::new(x, 0.0, -north)
}
pub fn mercator(lat: f64, lon: f64, zoom: u32) -> Result<(u32, u32, u32, u32)> {
    ensure!(
        zoom <= 18
            && lat.is_finite()
            && lat.abs() <= 85.05112878
            && lon.is_finite()
            && lon.abs() <= 180.0,
        "Location lies outside Web Mercator coverage"
    );
    let n = (1_u32 << zoom) as f64;
    let x = ((lon + 180.0) / 360.0 * n).rem_euclid(n);
    let y = (1.0 - lat.to_radians().tan().asinh() / std::f64::consts::PI) * 0.5 * n;
    Ok((
        x.floor() as u32,
        y.floor().clamp(0.0, n - 1.0) as u32,
        ((x.fract() * 256.0).floor() as u32).min(255),
        ((y.fract() * 256.0).floor() as u32).min(255),
    ))
}
fn decode(bytes: &[u8]) -> Result<(u32, u32, Vec<u8>)> {
    if bytes.starts_with(&[0xff, 0xd8]) {
        let mut decoder = jpeg_decoder::Decoder::new(Cursor::new(bytes));
        decoder.read_info()?;
        let info = decoder.info().context("JPEG has no image header")?;
        ensure!(
            info.width == 256 && info.height == 256,
            "Expected a 256×256 raster tile"
        );
        let data = decoder.decode()?;
        let rgb = match info.pixel_format {
            jpeg_decoder::PixelFormat::RGB24 => data,
            jpeg_decoder::PixelFormat::L8 => data.into_iter().flat_map(|v| [v; 3]).collect(),
            _ => anyhow::bail!("Unsupported JPEG pixel format"),
        };
        return Ok((256, 256, rgb));
    }
    let mut decoder = png::Decoder::new(Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info()?;
    let info = reader.info();
    ensure!(
        info.width == 256 && info.height == 256,
        "Expected a 256×256 raster tile"
    );
    let mut buffer = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buffer)?;
    let bytes = &buffer[..info.buffer_size()];
    let mut rgb = Vec::with_capacity(256 * 256 * 3);
    match info.color_type {
        png::ColorType::Rgb => rgb.extend_from_slice(bytes),
        png::ColorType::Rgba => {
            for p in bytes.chunks_exact(4) {
                rgb.extend_from_slice(&p[..3]);
            }
        }
        png::ColorType::Grayscale => {
            for &p in bytes {
                rgb.extend_from_slice(&[p, p, p]);
            }
        }
        png::ColorType::GrayscaleAlpha => {
            for p in bytes.chunks_exact(2) {
                rgb.extend_from_slice(&[p[0]; 3]);
            }
        }
        _ => anyhow::bail!("Unsupported raster format"),
    };
    Ok((256, 256, rgb))
}

#[cfg(test)]
mod jpeg_tests {
    #[test]
    fn satellite_jpeg_tiles_decode_without_a_live_imagery_request() {
        // Synthetic solid RGB(20,120,40), 256x256 JPEG, Pillow q90/optimize.
        let mut bytes=hex::decode("ffd8ffe000104a46494600010100000100010000ffdb0043000302020302020303030304030304050805050404050a070706080c0a0c0c0b0a0b0b0d0e12100d0e110e0b0b1016101113141515150c0f171816141812141514ffdb00430103040405040509050509140d0b0d1414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414ffc00011080100010003012200021101031101ffc4001500010100000000000000000000000000000007ffc40014100100000000000000000000000000000000ffc400160101010100000000000000000000000000000607ffc40014110100000000000000000000000000000000ffda000c03010002110311003f008100826180").unwrap();
        bytes.extend(std::iter::repeat_n(0, 382));
        bytes.extend([0xff, 0xd9]);
        let (width, height, rgb) = super::decode(&bytes).unwrap();
        assert_eq!((width, height, rgb.len()), (256, 256, 256 * 256 * 3));
        for (actual, expected) in rgb[..3].iter().zip([20, 120, 40]) {
            assert!(actual.abs_diff(expected) <= 2);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn cache(store: Arc<dyn ObjectStore>) -> GroundCache {
        GroundCache {
            store,
            prefix: String::new(),
            http: reqwest::Client::new(),
            template: "http://127.0.0.1:9/{z}/{x}/{y}.png".into(),
            provider: "Synthetic test tiles".into(),
            attribution: "Test fixture".into(),
            attribution_url: "https://example.invalid/credits".into(),
            enabled: true,
            gate: Arc::new(tokio::sync::Mutex::new(())),
            failures: Arc::new(std::sync::Mutex::new(HashMap::new())),
        }
    }
    fn png() -> Vec<u8> {
        let mut bytes = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut bytes, 256, 256);
            encoder.set_color(png::ColorType::Rgb);
            encoder.set_depth(png::BitDepth::Eight);
            let rgb = ergo_world::color_rgb(4);
            let pixel = [(rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8];
            let data = pixel.repeat(256 * 256);
            let mut writer = encoder.write_header().unwrap();
            writer.write_image_data(&data).unwrap();
            writer.finish().unwrap();
        }
        bytes
    }
    #[tokio::test]
    async fn visible_ground_is_generated_quantized_persisted_and_reused() {
        let store: Arc<dyn ObjectStore> = Arc::new(object_store::memory::InMemory::new());
        let service = cache(store.clone());
        let geo = GeoReference::default();
        let zoom = 18;
        let provider = hex::encode(Sha256::digest(&service.template));
        let mut keys = Vec::new();
        let mut cells = BTreeMap::new();
        for row in 0..SAMPLE_GRID_SIZE {
            for col in 0..SAMPLE_GRID_SIZE {
                let [lat, lon] = geo.to_gps(glam_point(
                    (col as f64 + 0.5) * BLOCK_SIZE / SAMPLE_GRID_SIZE as f64,
                    (row as f64 + 0.5) * BLOCK_SIZE / SAMPLE_GRID_SIZE as f64,
                ));
                let (x, y, _, _) = mercator(lat, lon, zoom).unwrap();
                cells.insert((x, y), ());
            }
        }
        for (x, y) in cells.keys() {
            let key = format!("ground/rasters/{provider}/{zoom}/{x}/{y}.cbor");
            service
                .put(
                    &key,
                    &Tile {
                        format: "ergo-raster-1".into(),
                        fetched_at: now(),
                        etag: None,
                        modified: None,
                        media_type: "image/png".into(),
                        image: png(),
                    },
                )
                .await
                .unwrap();
            keys.push(key);
        }
        let a = service.block(geo, 0, 0).await;
        assert!(a.fallback_reason.is_none(), "{:?}", a.fallback_reason);
        assert!(a.bricks.is_empty());
        assert_eq!(a.color, 4);
        assert_eq!(<[i64; 3]>::from(a.coord), [0, 0, 0]);
        for key in keys {
            store.delete(&service.path(&key)).await.unwrap();
        }
        let second = cache(store);
        let b = second.block(geo, 0, 0).await;
        assert_eq!(b.color, a.color);
        assert!(b.fallback_reason.is_none());
        assert_eq!(b.generated_at, a.generated_at);
    }
}
