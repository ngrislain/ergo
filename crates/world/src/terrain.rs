//! Lazy horizontal ground cells and palette projection. Native kernel positions
//! remain Y-up; serialized block coordinates and all public APIs are Z-up.
use crate::{BlockCoord, Brick, SCHEMA_VERSION, catalog::COLORS, color_rgb};
use serde::{Deserialize, Serialize};

pub const SAMPLE_GRID_SIZE: usize = 32;
pub const GROUND_TOP: f64 = -0.01;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(try_from = "GroundWire")]
pub struct GroundBlock {
    pub schema: u32,
    pub coord: BlockCoord,
    pub bricks: Vec<Brick>,
    /// One standard material color for the entire special baseplate.
    pub color: u32,
    pub sample_rgb: Option<u32>,
    pub source: String,
    pub attribution: String,
    pub attribution_url: Option<String>,
    pub generated_at: u64,
    pub source_tiles: Vec<String>,
    pub fallback_reason: Option<String>,
}
impl GroundBlock {
    pub fn fallback(x: i64, y: i64, reason: String) -> Self {
        Self {
            schema: SCHEMA_VERSION,
            coord: BlockCoord::from([x, y, 0]),
            bricks: Vec::new(),
            color: 71,
            sample_rgb: None,
            source: "palette fallback".into(),
            attribution: String::new(),
            attribution_url: None,
            generated_at: 0,
            source_tiles: Vec::new(),
            fallback_reason: Some(reason),
        }
    }
    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.schema == SCHEMA_VERSION
                && self.coord.1 == 0
                && self.bricks.is_empty()
                && COLORS.iter().any(|(code, _)| *code == self.color)
                && self.sample_rgb.is_none_or(|c| c <= 0xffffff)
                && self
                    .attribution_url
                    .as_ref()
                    .is_none_or(|s| s.starts_with("https://") || s.starts_with("http://")),
            "Invalid generated ground block"
        );
        Ok(())
    }
}
#[derive(Deserialize)]
struct GroundWire {
    schema: u32,
    coord: BlockCoord,
    bricks: Vec<Brick>,
    #[serde(default)]
    color: Option<u32>,
    #[serde(default)]
    sample_rgb: Option<u32>,
    #[serde(default)]
    colors: Vec<u32>,
    source: String,
    attribution: String,
    attribution_url: Option<String>,
    generated_at: u64,
    source_tiles: Vec<String>,
    fallback_reason: Option<String>,
}
impl TryFrom<GroundWire> for GroundBlock {
    type Error = String;
    fn try_from(w: GroundWire) -> Result<Self, String> {
        let (color, sample_rgb, source) = if let Some(color) = w.color {
            (color, w.sample_rgb, w.source)
        } else {
            if w.colors.is_empty()
                || w.colors.len() > 1024
                || w.colors
                    .iter()
                    .any(|c| !COLORS.iter().any(|(code, _)| code == c))
            {
                return Err("Missing/invalid baseplate color".into());
            }
            let rgb = average_rgb(w.colors.into_iter().map(color_rgb));
            (
                nearest_color(rgb),
                Some(rgb),
                format!("{} (legacy grid averaged)", w.source),
            )
        };
        let block = Self {
            schema: w.schema,
            coord: w.coord,
            bricks: w.bricks,
            color,
            sample_rgb,
            source,
            attribution: w.attribution,
            attribution_url: w.attribution_url,
            generated_at: w.generated_at,
            source_tiles: w.source_tiles,
            fallback_reason: w.fallback_reason,
        };
        block.validate().map_err(|e| e.to_string())?;
        Ok(block)
    }
}
fn linear(c: f64) -> f64 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}
pub fn average_rgb(samples: impl IntoIterator<Item = u32>) -> u32 {
    let mut sum = [0.0; 3];
    let mut count = 0;
    for rgb in samples {
        for (channel, shift) in [16, 8, 0].into_iter().enumerate() {
            sum[channel] += linear(((rgb >> shift) & 255) as f64 / 255.0);
        }
        count += 1;
    }
    if count == 0 {
        return color_rgb(71);
    }
    let encode = |v: f64| {
        let v = v / count as f64;
        let s = if v <= 0.0031308 {
            12.92 * v
        } else {
            1.055 * v.powf(1.0 / 2.4) - 0.055
        };
        (s * 255.0).round().clamp(0.0, 255.0) as u32
    };
    (encode(sum[0]) << 16) | (encode(sum[1]) << 8) | encode(sum[2])
}
pub fn lab(rgb: u32) -> [f64; 3] {
    let r = linear(((rgb >> 16) & 255) as f64 / 255.0);
    let g = linear(((rgb >> 8) & 255) as f64 / 255.0);
    let b = linear((rgb & 255) as f64 / 255.0);
    let f = |t: f64| {
        if t > 216.0 / 24389.0 {
            t.cbrt()
        } else {
            (24389.0 / 27.0 * t + 16.0) / 116.0
        }
    };
    let x = f((0.4124564 * r + 0.3575761 * g + 0.1804375 * b) / 0.95047);
    let y = f(0.2126729 * r + 0.7151522 * g + 0.0721750 * b);
    let z = f((0.0193339 * r + 0.1191920 * g + 0.9503041 * b) / 1.08883);
    [116.0 * y - 16.0, 500.0 * (x - y), 200.0 * (y - z)]
}
pub fn nearest_color(rgb: u32) -> u32 {
    let sample = lab(rgb);
    COLORS
        .iter()
        .min_by(|(a, _), (b, _)| {
            let distance = |code: u32| {
                let p = lab(color_rgb(code));
                (0..3).map(|i| (p[i] - sample[i]).powi(2)).sum::<f64>()
            };
            distance(*a).total_cmp(&distance(*b))
        })
        .unwrap()
        .0
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn palette_projection_preserves_every_enabled_color() {
        for &(c, _) in COLORS {
            assert_eq!(nearest_color(color_rgb(c)), c);
        }
    }
}
