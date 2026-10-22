use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::{fmt, str::FromStr};

/// Address signed i64 blocks and compose local cells with full-hash record keys.
pub const MAX_DEPTH: usize = 192;

/// Spatial octants use q = x_bit + 2*y_bit + 4*z_bit (public X east, Y north, Z up).
#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "Vec<u8>", into = "Vec<u8>")]
pub struct OctreePath(Vec<u8>);

impl OctreePath {
    pub fn root() -> Self {
        Self::default()
    }

    pub fn new(octants: impl Into<Vec<u8>>) -> Result<Self> {
        let octants = octants.into();
        if octants.len() > MAX_DEPTH || octants.iter().any(|q| *q > 7) {
            return Err(Error::InvalidPath);
        }
        Ok(Self(octants))
    }

    pub fn octants(&self) -> &[u8] {
        &self.0
    }

    pub fn child(&self, octant: u8) -> Result<Self> {
        let mut path = self.0.clone();
        path.push(octant);
        Self::new(path)
    }

    pub fn is_prefix_of(&self, other: &Self) -> bool {
        other.0.starts_with(&self.0)
    }

    /// Structural write independence; application read dependencies can still conflict.
    pub fn is_disjoint_from(&self, other: &Self) -> bool {
        !self.is_prefix_of(other) && !other.is_prefix_of(self)
    }

    /// A stable world-wide block address; append octants to subdivide the block.
    pub fn for_block(coord: [i64; 3]) -> Self {
        let biased = coord.map(|v| (v as u64) ^ (1 << 63));
        Self(
            (0..64)
                .rev()
                .map(|bit| {
                    ((biased[0] >> bit) & 1) as u8
                        | (((biased[1] >> bit) & 1) as u8) << 1
                        | (((biased[2] >> bit) & 1) as u8) << 2
                })
                .collect(),
        )
    }
}

impl TryFrom<Vec<u8>> for OctreePath {
    type Error = Error;
    fn try_from(value: Vec<u8>) -> Result<Self> {
        Self::new(value)
    }
}

impl From<OctreePath> for Vec<u8> {
    fn from(value: OctreePath) -> Self {
        value.0
    }
}

impl FromStr for OctreePath {
    type Err = Error;
    fn from_str(value: &str) -> Result<Self> {
        if value.is_empty() || value == "/" {
            return Ok(Self::root());
        }
        Self::new(
            value
                .split('/')
                .map(|s| match s.as_bytes() {
                    [q @ b'0'..=b'7'] => Ok(q - b'0'),
                    _ => Err(Error::InvalidPath),
                })
                .collect::<Result<Vec<_>>>()?,
        )
    }
}

impl fmt::Display for OctreePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.0.is_empty() {
            return f.write_str("/");
        }
        for (i, q) in self.0.iter().enumerate() {
            if i > 0 {
                f.write_str("/")?;
            }
            write!(f, "{q}")?;
        }
        Ok(())
    }
}
