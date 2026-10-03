use std::sync::Arc;

use parking_lot::RwLock;

use crate::terrain::terrain_gen::{Biome, BiomeMap, HeightMap};
use crate::terrain::{BlockType, Chunk, ChunkPos};

pub struct ChunkGenerator {
    seed:       u64,
    height_map: Arc<RwLock<Option<HeightMap>>>,
    biome_map:  Arc<RwLock<Option<BiomeMap>>>,
}

impl ChunkGenerator {
    pub fn new<U>(seed: U) -> Self
    where
        U: Into<u64>,
    {
        Self {
            seed:       seed.into(),
            height_map: Arc::new(RwLock::new(None)),
            biome_map:  Arc::new(RwLock::new(None)),
        }
    }

    pub fn generate(&self, pos: ChunkPos) -> Chunk {
        // Lazy initialization of height map
        {
            let mut hm = self.height_map.write();
            if hm.is_none() {
                *hm = Some(HeightMap::new(512, 512, self.seed));
            }
        }

        // Lazy initialization of biome map
        {
            // if let Some(ref mut bm) = self.biome_map.read().as_ref() {
            // }

            let mut bm = self.biome_map.write();
            if bm.as_ref().is_none() {
                let hm_lock = self.height_map.read();
                if let Some(hm) = hm_lock.as_ref() {
                    *bm = Some(BiomeMap::from(hm));
                }
            }
        }

        let mut chunk = Chunk::new(pos);

        let hm_lock = self.height_map.read();
        let bm_lock = self.biome_map.read();

        if let (Some(height_map), Some(biome_map)) = (hm_lock.as_ref(), bm_lock.as_ref()) {
            // PERF: @nested : Loop moved to thread engine
            for x in 0..16 {
                for z in 0..16 {
                    let world_x = (pos.x * 16 + x as i32) as usize;
                    let world_z = (pos.z * 16 + z as i32) as usize;

                    let elevation = height_map.get(world_x, world_z);
                    let biome = biome_map.get(world_x, world_z);

                    let height = self.elevation_to_block_height(elevation);

                    self.fill_column(&mut chunk, x, z, height, biome, elevation);
                }
            }
        }

        chunk
    }

    fn elevation_to_block_height(&self, elevation: f64) -> usize {
        // Map [-1, 1] to [10, 200]
        let normalized = (elevation + 1.0) / 2.0; // [0, 1]
        ((normalized * 190.0) + 10.0) as usize
    }

    fn fill_column(
        &self,
        chunk: &mut Chunk,
        x: usize,
        z: usize,
        height: usize,
        biome: Biome,
        elevation: f64,
    ) {
        for y in 0..height.min(256) {
            let block = self.get_block_for_biome(y, height, biome, elevation);
            chunk.set_block(x, y, z, block);
        }

        // Water at sea level (elevation -0.05)
        let sea_level = self.elevation_to_block_height(-0.05);
        if height < sea_level {
            for y in height..sea_level.min(256) {
                chunk.set_block(x, y, z, BlockType::Water);
            }
        }
    }

    #[rustfmt::skip]
    fn get_block_for_biome(&self, y: usize, height: usize, biome: Biome, _elevation: f64) -> BlockType {
        if y >= height {
            return BlockType::Air;
        }

        let depth = height - y;

        // TODO: @from_into : This is a data transformation, plain and simple.
        // We should be using From<Biome> for BlockType or similar.
        match biome {
            Biome::Ocean => BlockType::Stone,
            Biome::Beach => match depth {
                depth if depth <= 2 => BlockType::Sand,
                _ => BlockType::Stone
            }
            Biome::Plains => match depth {
                0 => BlockType::Grass,
                depth if depth <= 3 => BlockType::Dirt,
                _ => BlockType::Stone,
            }
            Biome::Forest => match depth {
                0 => BlockType::Grass,
                depth if depth <= 4 => BlockType::Dirt,
                _ => BlockType::Stone,
            }
            Biome::Mountain => match depth {
                depth if depth <= 2 => BlockType::Stone,
                depth if depth <= 6 => BlockType::Cobblestone,
                _ => BlockType::Stone,
            }
            Biome::Snow => match depth {
                0 => BlockType::Grass, // White snow-like top
                depth if depth <= 2 => BlockType::Dirt,
                _ => BlockType::Stone,
            }
            Biome::SnowMountain => match depth {
                depth if depth <= 1 => BlockType::Stone,
                _ => BlockType::Stone,
            }
            Biome::Desert => match depth {
                depth if depth <= 4 => BlockType::Sand,
                _ => BlockType::Stone,
            }
        }
    }
}
