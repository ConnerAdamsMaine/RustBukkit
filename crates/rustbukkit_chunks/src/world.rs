use std::collections::HashMap;

use parking_lot::RwLock;

use crate::{BlockKind, FlatWorld, TerrainGenerator};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BlockPos {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

impl BlockPos {
    pub fn new(x: i32, y: i32, z: i32) -> Option<Self> {
        (i64::from(x).abs() <= 29_999_984
            && i64::from(z).abs() <= 29_999_984
            && (FlatWorld::MIN_Y..FlatWorld::MIN_Y + FlatWorld::HEIGHT).contains(&y))
        .then_some(Self { x, y, z })
    }

    pub fn chunk(self) -> ChunkPos {
        ChunkPos {
            x: self.x.div_euclid(16),
            z: self.z.div_euclid(16),
        }
    }

    fn local(self) -> (u8, i32, u8) {
        (self.x.rem_euclid(16) as u8, self.y, self.z.rem_euclid(16) as u8)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ChunkPos {
    pub x: i32,
    pub z: i32,
}

#[derive(Debug, Clone)]
pub struct ChunkSnapshot {
    overrides:       HashMap<(u8, i32, u8), BlockKind>,
    terrain_columns: Vec<crate::terrain::TerrainColumn>,
}

impl ChunkSnapshot {
    pub fn block_at(&self, x: u8, y: i32, z: u8) -> BlockKind {
        if x >= 16 || z >= 16 {
            return BlockKind::Air;
        }
        self.overrides
            .get(&(x, y, z))
            .copied()
            .unwrap_or_else(|| self.terrain_columns[usize::from(z) * 16 + usize::from(x)].block_at(y))
    }
}

type LocalBlockPos = (u8, i32, u8);
type ChunkOverrides = HashMap<LocalBlockPos, BlockKind>;
type WorldOverrides = HashMap<ChunkPos, ChunkOverrides>;

pub struct World {
    overrides: RwLock<WorldOverrides>,
    terrain:   TerrainGenerator,
}

impl Default for World {
    fn default() -> Self {
        Self {
            overrides: RwLock::new(HashMap::new()),
            terrain:   TerrainGenerator::default(),
        }
    }
}

impl World {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_seed(seed: u64) -> Self {
        Self {
            overrides: RwLock::new(HashMap::new()),
            terrain:   TerrainGenerator::new(seed),
        }
    }

    /// Y coordinate of the first air block above terrain at this column.
    pub fn spawn_y(&self, x: i32, z: i32) -> i32 {
        self.terrain.spawn_y(x, z)
    }

    /// Seed used by the active terrain generator, reported to clients on join.
    pub fn seed(&self) -> u64 {
        self.terrain.seed()
    }

    pub fn sea_level(&self) -> i32 {
        crate::terrain::SEA_LEVEL
    }

    pub fn get_block(&self, position: BlockPos) -> BlockKind {
        self.overrides
            .read()
            .get(&position.chunk())
            .and_then(|blocks| blocks.get(&position.local()).copied())
            .unwrap_or_else(|| self.terrain.block_at(position.x, position.y, position.z))
    }

    pub fn set_block(&self, position: BlockPos, block: BlockKind) -> BlockKind {
        let chunk = position.chunk();
        let mut overrides = self.overrides.write();
        let old = overrides
            .get(&chunk)
            .and_then(|blocks| blocks.get(&position.local()).copied())
            .unwrap_or_else(|| self.terrain.block_at(position.x, position.y, position.z));
        if block == self.terrain.block_at(position.x, position.y, position.z) {
            if let Some(blocks) = overrides.get_mut(&chunk) {
                blocks.remove(&position.local());
                if blocks.is_empty() {
                    overrides.remove(&chunk);
                }
            }
        } else {
            overrides
                .entry(chunk)
                .or_default()
                .insert(position.local(), block);
        }
        old
    }

    /// Changes a block only if it still has the expected kind.
    ///
    /// The comparison and mutation share one write lock, so competing block
    /// actions cannot both succeed against the same observed block.
    pub fn set_block_if(&self, position: BlockPos, expected: BlockKind, block: BlockKind) -> bool {
        let chunk = position.chunk();
        let mut overrides = self.overrides.write();
        let current = overrides
            .get(&chunk)
            .and_then(|blocks| blocks.get(&position.local()).copied())
            .unwrap_or_else(|| self.terrain.block_at(position.x, position.y, position.z));
        if current != expected {
            return false;
        }
        if block == self.terrain.block_at(position.x, position.y, position.z) {
            if let Some(blocks) = overrides.get_mut(&chunk) {
                blocks.remove(&position.local());
                if blocks.is_empty() {
                    overrides.remove(&chunk);
                }
            }
        } else {
            overrides
                .entry(chunk)
                .or_default()
                .insert(position.local(), block);
        }
        true
    }

    pub fn snapshot_chunk(&self, chunk: ChunkPos) -> ChunkSnapshot {
        let terrain_columns = (0..16)
            .flat_map(|z| (0..16).map(move |x| self.terrain.column(chunk.x * 16 + x, chunk.z * 16 + z)))
            .collect();
        ChunkSnapshot {
            overrides: self.overrides.read().get(&chunk).cloned().unwrap_or_default(),
            terrain_columns,
        }
    }

    pub fn overrides_snapshot(&self) -> Vec<(BlockPos, BlockKind)> {
        let overrides = self.overrides.read();
        let mut blocks = Vec::new();
        for (chunk, entries) in overrides.iter() {
            for (&(local_x, y, local_z), &block) in entries {
                blocks.push((
                    BlockPos {
                        x: chunk.x * 16 + i32::from(local_x),
                        y,
                        z: chunk.z * 16 + i32::from(local_z),
                    },
                    block,
                ));
            }
        }
        blocks.sort_unstable_by_key(|(position, _)| (position.x, position.y, position.z));
        blocks
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Barrier};

    use super::{BlockKind, BlockPos, ChunkPos, World};

    #[test]
    fn sparse_changes_override_and_restore_generated_blocks() {
        let world = World::new();
        let x = -1;
        let z = 17;
        let y = world.spawn_y(x, z) - 1;
        let pos = BlockPos::new(x, y, z).unwrap();
        assert_eq!(pos.chunk(), ChunkPos { x: -1, z: 1 });
        let generated = world.get_block(pos);
        assert_ne!(generated, BlockKind::Air);
        assert_eq!(world.set_block(pos, BlockKind::Air), generated);
        assert_eq!(world.get_block(pos), BlockKind::Air);
        assert_eq!(world.snapshot_chunk(pos.chunk()).block_at(15, y, 1), BlockKind::Air);
        world.set_block(pos, generated);
        assert_eq!(world.get_block(pos), generated);
    }

    #[test]
    fn conditional_update_respects_negative_chunk_coordinates() {
        let world = World::new();
        let x = -1;
        let z = -17;
        let y = world.spawn_y(x, z) - 1;
        let pos = BlockPos::new(x, y, z).unwrap();
        let generated = world.get_block(pos);
        assert_eq!(pos.chunk(), ChunkPos { x: -1, z: -2 });
        assert!(!world.set_block_if(pos, BlockKind::Air, BlockKind::Air));
        assert!(world.set_block_if(pos, generated, BlockKind::Air));
        assert!(!world.set_block_if(pos, generated, BlockKind::Air));
        assert_eq!(world.get_block(pos), BlockKind::Air);
        assert!(world.set_block_if(pos, BlockKind::Air, generated));
        assert!(world.overrides_snapshot().is_empty());
    }

    #[test]
    fn competing_conditional_breaks_only_one_succeeds() {
        let world = Arc::new(World::new());
        let barrier = Arc::new(Barrier::new(3));
        let pos = BlockPos::new(-1, world.spawn_y(-1, -17) - 1, -17).unwrap();
        let generated = world.get_block(pos);
        let mut workers = Vec::new();
        for _ in 0..2 {
            let world = Arc::clone(&world);
            let barrier = Arc::clone(&barrier);
            workers.push(std::thread::spawn(move || {
                barrier.wait();
                world.set_block_if(pos, generated, BlockKind::Air)
            }));
        }
        barrier.wait();
        let successes = workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .filter(|&succeeded| succeeded)
            .count();
        assert_eq!(successes, 1);
        assert_eq!(world.get_block(pos), BlockKind::Air);
    }
}
