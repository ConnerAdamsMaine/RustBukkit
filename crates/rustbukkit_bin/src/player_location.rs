use std::io::ErrorKind;
use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use tracing::warn;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PlayerLocation {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl PlayerLocation {
    pub const SPAWN: Self = Self {
        x: 0.5,
        y: 64.0,
        z: 0.5,
    };

    pub fn chunk(self) -> (i32, i32) {
        ((self.x / 16.0).floor() as i32, (self.z / 16.0).floor() as i32)
    }

    fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite() && self.z.is_finite()
    }
}

pub struct PlayerLocationStore {
    root: PathBuf,
}

impl PlayerLocationStore {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    fn path(&self, uuid: Uuid) -> PathBuf {
        self.root.join(format!("{uuid}.json"))
    }

    pub async fn load(&self, uuid: Uuid) -> Result<PlayerLocation> {
        let path = self.path(uuid);
        let bytes = match tokio::fs::read(&path).await {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(PlayerLocation::SPAWN),
            Err(error) => return Err(error).with_context(|| format!("reading {}", path.display())),
        };
        match serde_json::from_slice::<PlayerLocation>(&bytes) {
            Ok(location) if location.is_finite() => Ok(location),
            Ok(_) | Err(_) => {
                warn!("Invalid saved player location in {}; using spawn", path.display());
                Ok(PlayerLocation::SPAWN)
            }
        }
    }

    pub async fn save(&self, uuid: Uuid, location: PlayerLocation) -> Result<()> {
        tokio::fs::create_dir_all(&self.root)
            .await
            .with_context(|| format!("creating {}", self.root.display()))?;
        let path = self.path(uuid);
        let bytes = serde_json::to_vec(&location)?;
        tokio::fs::write(&path, bytes)
            .await
            .with_context(|| format!("writing {}", path.display()))
    }
}
