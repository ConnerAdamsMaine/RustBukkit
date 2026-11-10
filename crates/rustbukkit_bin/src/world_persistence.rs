use std::io::{Error, ErrorKind};
use std::path::PathBuf;

use rustbukkit_chunks::{BlockKind, BlockPos, World};
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex;
use uuid::Uuid;

const MAGIC: &[u8; 8] = b"RBWORLD1";
const RECORD_LEN: usize = 13;

pub struct WorldStore {
    root:       PathBuf,
    write_lock: Mutex<()>,
}

impl WorldStore {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            write_lock: Mutex::new(()),
        }
    }

    pub async fn load(&self) -> Result<World, Error> {
        let path = self.root.join("blocks.bin");
        let bytes = match tokio::fs::read(&path).await {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(World::new()),
            Err(error) => return Err(error),
        };
        if bytes.len() < 12 || &bytes[..8] != MAGIC {
            return Err(Error::new(ErrorKind::InvalidData, "invalid world overlay header"));
        }
        let count = u32::from_be_bytes(bytes[8..12].try_into().map_err(|_| ErrorKind::InvalidData)?) as usize;
        if count > 1_000_000 || bytes.len() != 12 + count * RECORD_LEN {
            return Err(Error::new(ErrorKind::InvalidData, "invalid world overlay length"));
        }
        let world = World::new();
        for record in bytes[12..].as_chunks::<RECORD_LEN>().0 {
            let x = i32::from_be_bytes(record[0..4].try_into().map_err(|_| ErrorKind::InvalidData)?);
            let y = i32::from_be_bytes(record[4..8].try_into().map_err(|_| ErrorKind::InvalidData)?);
            let z = i32::from_be_bytes(record[8..12].try_into().map_err(|_| ErrorKind::InvalidData)?);
            let position = BlockPos::new(x, y, z)
                .ok_or_else(|| Error::new(ErrorKind::InvalidData, "invalid saved block position"))?;
            let block = match record[12] {
                0 => BlockKind::Air,
                1 => BlockKind::Bedrock,
                2 => BlockKind::Dirt,
                3 => BlockKind::GrassBlock,
                4 => BlockKind::Stone,
                5 => BlockKind::Cobblestone,
                6 => BlockKind::Sand,
                7 => BlockKind::Water,
                _ => return Err(Error::new(ErrorKind::InvalidData, "invalid saved block kind")),
            };
            world.set_block(position, block);
        }
        Ok(world)
    }

    pub async fn save(&self, world: &World) -> Result<(), Error> {
        let _guard = self.write_lock.lock().await;
        let blocks = world.overrides_snapshot();
        let count = u32::try_from(blocks.len())
            .map_err(|_| Error::new(ErrorKind::InvalidData, "world overlay is too large"))?;
        let mut bytes = Vec::with_capacity(12 + blocks.len() * RECORD_LEN);
        bytes.extend_from_slice(MAGIC);
        bytes.extend_from_slice(&count.to_be_bytes());
        for (position, block) in blocks {
            bytes.extend_from_slice(&position.x.to_be_bytes());
            bytes.extend_from_slice(&position.y.to_be_bytes());
            bytes.extend_from_slice(&position.z.to_be_bytes());
            bytes.push(match block {
                BlockKind::Air => 0,
                BlockKind::Bedrock => 1,
                BlockKind::Dirt => 2,
                BlockKind::GrassBlock => 3,
                BlockKind::Stone => 4,
                BlockKind::Cobblestone => 5,
                BlockKind::Sand => 6,
                BlockKind::Water => 7,
            });
        }
        tokio::fs::create_dir_all(&self.root).await?;
        let temporary = self.root.join(format!("blocks.{}.tmp", Uuid::new_v4()));
        let mut file = tokio::fs::File::create(&temporary).await?;
        file.write_all(&bytes).await?;
        file.sync_all().await?;
        drop(file);
        tokio::fs::rename(&temporary, self.root.join("blocks.bin")).await
    }
}

#[cfg(test)]
mod tests {
    use rustbukkit_chunks::{BlockKind, BlockPos, World};
    use uuid::Uuid;

    use super::WorldStore;

    #[tokio::test]
    async fn saves_sparse_semantic_blocks_across_restarts() {
        let root = std::env::temp_dir().join(format!("rustbukkit-world-test-{}", Uuid::new_v4()));
        let store = WorldStore::new(root.clone());
        let world = World::new();
        let cleared = BlockPos::new(-1, world.spawn_y(-1, 0) - 1, 0).unwrap();
        let placed = BlockPos::new(17, world.spawn_y(17, 2) + 1, 2).unwrap();
        world.set_block(cleared, BlockKind::Air);
        world.set_block(placed, BlockKind::Cobblestone);
        store.save(&world).await.unwrap();
        let first = tokio::fs::read(root.join("blocks.bin")).await.unwrap();
        store.save(&world).await.unwrap();
        assert_eq!(tokio::fs::read(root.join("blocks.bin")).await.unwrap(), first);
        let reloaded = store.load().await.unwrap();
        assert_eq!(reloaded.get_block(cleared), BlockKind::Air);
        assert_eq!(reloaded.get_block(placed), BlockKind::Cobblestone);
        assert_eq!(reloaded.overrides_snapshot().len(), 2);
        tokio::fs::remove_dir_all(root).await.unwrap();
    }
}
