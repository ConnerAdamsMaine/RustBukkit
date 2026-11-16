use std::io::ErrorKind;
use std::path::PathBuf;

use anyhow::{Context, Result};
use rustbukkit_inventory::Inventory;
use serde::{Deserialize, Serialize};
use tracing::warn;
use uuid::Uuid;

const OVERWORLD: &str = "minecraft:overworld";

#[derive(Serialize, Deserialize)]
struct StoredPlayer {
    uuid:      Uuid,
    username:  String,
    dimension: String,
    location:  PlayerLocation,
    #[serde(default = "empty_inventory")]
    inventory: Inventory,
}

fn empty_inventory() -> Inventory {
    Inventory::empty(0).expect("hotbar selection 0 is always valid")
}

/// Everything a session needs to resume a returning player.
pub struct PlayerSnapshot {
    pub location:  PlayerLocation,
    pub inventory: Inventory,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlayerLocation {
    pub x:                    f64,
    pub y:                    f64,
    pub z:                    f64,
    #[serde(default)]
    pub yaw:                  f32,
    #[serde(default)]
    pub pitch:                f32,
    #[serde(default)]
    pub on_ground:            bool,
    #[serde(default)]
    pub selected_hotbar_slot: u8,
}

impl PlayerLocation {
    pub const fn spawn(y: f64) -> Self {
        Self {
            x: 0.5,
            y,
            z: 0.5,
            yaw: 0.0,
            pitch: 0.0,
            on_ground: true,
            selected_hotbar_slot: 0,
        }
    }

    pub fn chunk(self) -> (i32, i32) {
        ((self.x / 16.0).floor() as i32, (self.z / 16.0).floor() as i32)
    }

    pub fn is_valid(self) -> bool {
        self.x.is_finite()
            && self.y.is_finite()
            && self.z.is_finite()
            && self.yaw.is_finite()
            && self.pitch.is_finite()
            && self.x.abs() <= 29_999_984.0
            && self.z.abs() <= 29_999_984.0
            && (-64.0..=320.0).contains(&self.y)
            && self.selected_hotbar_slot < 9
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

    pub async fn load(&self, uuid: Uuid, username: &str, spawn: PlayerLocation) -> Result<PlayerSnapshot> {
        let path = self.path(uuid);
        let bytes = match tokio::fs::read(&path).await {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == ErrorKind::NotFound => {
                return Ok(PlayerSnapshot {
                    location:  spawn,
                    inventory: empty_inventory(),
                });
            }
            Err(error) => return Err(error).with_context(|| format!("reading {}", path.display())),
        };
        let saved_player = match serde_json::from_slice::<StoredPlayer>(&bytes) {
            Ok(saved) if saved.uuid == uuid && saved.username == username && saved.dimension == OVERWORLD => {
                Some(saved)
            }
            Ok(_) => None,
            Err(_) => {
                serde_json::from_slice::<PlayerLocation>(&bytes)
                    .ok()
                    .map(|location| {
                        StoredPlayer {
                            uuid,
                            username: username.to_owned(),
                            dimension: OVERWORLD.to_owned(),
                            location,
                            inventory: empty_inventory(),
                        }
                    })
            } // legacy location-only file
        };
        match saved_player {
            Some(saved) if saved.location.is_valid() && saved.location.y >= spawn.y - 512.0 => {
                Ok(PlayerSnapshot {
                    location:  saved.location,
                    inventory: saved.inventory,
                })
            }
            _ => {
                warn!("Invalid saved player location in {}; using spawn", path.display());
                Ok(PlayerSnapshot {
                    location:  spawn,
                    inventory: empty_inventory(),
                })
            }
        }
    }

    pub async fn save(
        &self,
        uuid: Uuid,
        username: &str,
        location: PlayerLocation,
        inventory: &Inventory,
    ) -> Result<()> {
        tokio::fs::create_dir_all(&self.root)
            .await
            .with_context(|| format!("creating {}", self.root.display()))?;
        let path = self.path(uuid);
        let saved = StoredPlayer {
            uuid,
            username: username.to_owned(),
            dimension: OVERWORLD.to_owned(),
            location,
            inventory: inventory.clone(),
        };
        let bytes = serde_json::to_vec(&saved)?;
        let temporary = self.root.join(format!("{uuid}.{}.tmp", Uuid::new_v4()));
        let mut file = tokio::fs::File::create(&temporary)
            .await
            .with_context(|| format!("creating {}", temporary.display()))?;
        use tokio::io::AsyncWriteExt;
        file.write_all(&bytes).await?;
        file.sync_all().await?;
        drop(file);
        tokio::fs::rename(&temporary, &path)
            .await
            .with_context(|| format!("replacing {}", path.display()))
    }
}

#[cfg(test)]
mod tests {
    use rustbukkit_inventory::{ItemKind, ItemStack};
    use uuid::Uuid;

    use super::{PlayerLocation, PlayerLocationStore};

    #[tokio::test]
    async fn migrates_legacy_coordinates_and_writes_identity_metadata() {
        let root = std::env::temp_dir().join(format!("rustbukkit-player-store-test-{}", Uuid::new_v4()));
        tokio::fs::create_dir_all(&root).await.unwrap();
        let uuid = Uuid::new_v4();
        let path = root.join(format!("{uuid}.json"));
        tokio::fs::write(&path, b"{\"x\":12.5,\"y\":64.0,\"z\":-3.0}")
            .await
            .unwrap();
        let store = PlayerLocationStore::new(root.clone());
        let mut snapshot = store
            .load(uuid, "Traveler", PlayerLocation::spawn(64.0))
            .await
            .unwrap();
        assert_eq!(snapshot.location.x, 12.5);
        assert_eq!(snapshot.inventory.carried(), None);
        snapshot.location.yaw = 45.0;
        snapshot.location.selected_hotbar_slot = 2;
        snapshot.inventory.set(
            rustbukkit_inventory::InventorySlot::new(36).unwrap(),
            Some(ItemStack::new(ItemKind::Dirt, 3).unwrap()),
        );
        store
            .save(uuid, "Traveler", snapshot.location, &snapshot.inventory)
            .await
            .unwrap();
        let saved: serde_json::Value =
            serde_json::from_slice(&tokio::fs::read(&path).await.unwrap()).unwrap();
        assert_eq!(saved["uuid"], uuid.to_string());
        assert_eq!(saved["username"], "Traveler");
        assert_eq!(saved["dimension"], "minecraft:overworld");
        assert_eq!(saved["location"]["yaw"], 45.0);
        assert_eq!(saved["location"]["selected_hotbar_slot"], 2);
        assert_eq!(saved["inventory"]["slots"][36]["count"], 3);
        let reloaded = store
            .load(uuid, "Traveler", PlayerLocation::spawn(64.0))
            .await
            .unwrap();
        assert_eq!(reloaded.location, snapshot.location);
        assert_eq!(reloaded.inventory.window_slot(36), Some(ItemStack::new(ItemKind::Dirt, 3).unwrap()));
        tokio::fs::remove_dir_all(root).await.unwrap();
    }
}
