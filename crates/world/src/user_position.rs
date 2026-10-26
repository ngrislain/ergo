//! Private per-user navigation context, separate from accepted construction data.
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PositionUpdate {
    pub world_id: String,
    pub position_world: [f64; 3],
    pub view_target_world: [f64; 3],
    pub navigation: String,
    pub client_session: String,
    pub sequence: u64,
}
impl PositionUpdate {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            !self.world_id.is_empty() && self.world_id.len() <= 128,
            "Invalid position world"
        );
        ensure!(
            !self.client_session.is_empty() && self.client_session.len() <= 64 && self.sequence > 0,
            "Invalid position sequence"
        );
        ensure!(
            ["orbit", "fly", "walk"].contains(&self.navigation.as_str()),
            "Invalid navigation mode"
        );
        ensure!(
            self.position_world
                .iter()
                .chain(&self.view_target_world)
                .all(|n| n.is_finite() && n.abs() <= crate::MAX_WORLD_COORD),
            "Position is outside the world coordinate range"
        );
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UserPosition {
    pub format: String,
    pub schema: u32,
    pub context: PositionUpdate,
    pub updated_at_ms: u64,
}
impl UserPosition {
    pub fn new(context: PositionUpdate, updated_at_ms: u64) -> Self {
        Self {
            format: "ergo-user-position-1".into(),
            schema: crate::SCHEMA_VERSION,
            context,
            updated_at_ms,
        }
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.format == "ergo-user-position-1" && self.schema == crate::SCHEMA_VERSION,
            "Unsupported user position format"
        );
        self.context.validate()
    }
    pub fn ground_position(&self) -> [f64; 3] {
        [
            self.context.position_world[0].round(),
            self.context.position_world[1].round(),
            0.,
        ]
    }
}
