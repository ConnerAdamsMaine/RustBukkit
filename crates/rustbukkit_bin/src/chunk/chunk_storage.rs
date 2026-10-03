use std::collections::HashMap;
use std::ops::AddAssign;
use std::path::PathBuf;
use std::sync::atomic::AtomicUsize;
use std::sync::{Arc, mpsc};

use anyhow::Result;
use parking_lot::RwLock;
use rayon::prelude::*;
use tracing::{debug, error, info, trace, warn};

use crate::chunk::cache::LruCache;
use crate::consts::{
    CHUNK_SIZE_BYTES,
    INITIAL_BUFFER_MB,
    INITIAL_CAPACITY,
    MAX_BUFFER_MB,
    MAX_CAPACITY,
    WORLD_PATH,
};
use crate::core::ChunkGenThreadPool;
use crate::terrain::{Chunk, ChunkGenerator, ChunkPos};
use crate::world::{Region, RegionPos};

const SLEEP_TIME_SECS: u64 = 300; // 5 minutes
const SLEEP_TIME_DURATION: tokio::time::Duration = tokio::time::Duration::from_secs(SLEEP_TIME_SECS);

// Memory budget constants
// const CHUNK_SIZE_BYTES: usize = 232 * 1024; // ~232 KB per chunk
// const INITIAL_BUFFER_MB: usize = 256; // 256 MB initial
// const MAX_BUFFER_MB: usize = 2048; // 2 GB max
//
// const INITIAL_CAPACITY: usize = INITIAL_BUFFER_MB * 1024 * 1024 / CHUNK_SIZE_BYTES; // ~1130 chunks
// const MAX_CAPACITY: usize = MAX_BUFFER_MB * 1024 * 1024 / CHUNK_SIZE_BYTES; // ~9033 chunks

#[allow(dead_code)]
pub struct CacheLenCapacity((usize, usize));

impl From<(usize, usize)> for CacheLenCapacity {
    fn from(value: (usize, usize)) -> Self {
        CacheLenCapacity(value)
    }
}

pub struct ChunkStorage {
    // PERF: @locking : Is there a way to work around the use of a RwLock here?
    cache:           Arc<RwLock<LruCache<ChunkPos, Chunk>>>,
    world_dir:       PathBuf,
    chunk_generator: Arc<ChunkGenerator>,
    evictions:       AtomicUsize,
    chunk_gen_pool:  Arc<ChunkGenThreadPool>,
}

impl ChunkStorage {
    pub fn new(
        chunk_generator: Arc<ChunkGenerator>,
        chunk_gen_pool: Arc<ChunkGenThreadPool>,
    ) -> Result<Self> {
        // let world_dir = PathBuf::from(WORLD_NAME);
        let world_dir = PathBuf::from(WORLD_PATH);

        // NOTE: Do not call world_dir.canonicalize() before checking existence,
        // This WILL crash if the directory does not exist yet.

        // Create world directory if it doesn't exist
        if !world_dir.exists() {
            warn!("[STARTUP] World directory not found at {:?}, creating...", world_dir);
            // `.unwrap()` is on purpose here, so we can log -> crash immediately (HERE, not up the
            // call stack) if creation fails.
            std::fs::create_dir_all(&world_dir)
                .map_err(|e| {
                    error!("Failed to create world directory {:?}: {e}", world_dir);
                    e
                })
                .unwrap();
        }
        info!("[STARTUP] World directory found at {:?}", world_dir.canonicalize()?);

        info!(
            "[STARTUP] Initializing chunk cache: {}-{}MB ({}-{} chunks)",
            INITIAL_BUFFER_MB, MAX_BUFFER_MB, INITIAL_CAPACITY, MAX_CAPACITY
        );

        let storage = Self {
            cache: Arc::new(RwLock::new(LruCache::with_growth(
                INITIAL_CAPACITY,
                MAX_CAPACITY,
                CHUNK_SIZE_BYTES,
            ))),
            world_dir,
            chunk_generator,
            evictions: AtomicUsize::new(0),
            chunk_gen_pool,
        };

        // Pregenerate 64x64 chunk area on startup
        debug!("[STARTUP] Starting pregeneration of spawn area...");
        storage.pregenerate_spawn_area()?;

        storage.start_hit_reset_task();

        storage.chunk_gen_pool.signal_init_complete();

        Ok(storage)
    }

    /// Start hit count reset task (runs every 5 minutes)
    pub fn start_hit_reset_task(&self) {
        let cache = Arc::clone(&self.cache);
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(SLEEP_TIME_DURATION).await;
                cache.write().reset_hit_counts();
                // let mut cache_lock = cache.write();
                // cache_lock.reset_hit_counts();
                // drop(cache_lock);
                debug!("[CHUNK] Hit counts reset");
            }
        });
    }

    fn pregenerate_spawn_area(&self) -> Result<()> {
        info!("[STARTUP] Pregenerating spawn area (16x16 chunks)...");

        let start = std::time::Instant::now();
        let mut generated = 0;
        let (tx, rx) = mpsc::channel();

        // Generate a 16x16 area centered around origin using thread pool

        // PERF: @nested : Loop moved to thread engine
        for cx in -8..8 {
            for cz in -8..8 {
                let chunk_pos = ChunkPos::new(cx, cz);

                // Check if chunk exists on disk
                let region_pos = RegionPos::from(chunk_pos);

                // if !self.chunk_exists_on_disk(region_pos)? {
                if !self.world_dir.join(region_pos.filename()).exists() {
                    // Clone needed data for thread pool task
                    let generator = Arc::clone(&self.chunk_generator);
                    let tx = tx.clone();

                    // Submit to thread pool
                    self.chunk_gen_pool.execute(move || {
                        let chunk = generator.generate(chunk_pos);
                        let _ = tx.send((chunk_pos, chunk));
                    })?;

                    generated += 1;

                    // Periodically receive and cache generated chunks
                    if generated % 256 == 0 {
                        trace!("[CHUNK] Submitted {} chunks to generation pool", generated);
                        self.receive_and_cache_chunks(&rx)?;
                    }
                }
            }
        }

        // Drop the original sender so receiver knows when all tasks are done
        drop(tx);

        // Receive all remaining chunks
        self.receive_and_cache_all_chunks(&rx)?;

        self.flush_cache()?;

        let elapsed = start.elapsed();
        let cache = self.cache.read();
        info!(
            "[STARTUP] Pregeneration complete: {} new chunks in {:.2}s ({:.0} chunks/sec), cache: {}/{}",
            generated,
            elapsed.as_secs_f64(),
            generated as f64 / elapsed.as_secs_f64(),
            cache.len(),
            cache.current_capacity()
        );

        Ok(())
    }

    fn receive_and_cache_chunks(&self, rx: &mpsc::Receiver<(ChunkPos, Chunk)>) -> Result<()> {
        // Receive chunks with a short timeout to avoid blocking
        while let Ok((pos, chunk)) = rx.try_recv() {
            debug!("[CHUNK] Caching pregenerated chunk at {}", pos);
            let (_, expanded, evicted) = {
                let mut cache = self.cache.write();
                cache.insert(pos, chunk)
            };

            if expanded {
                let cache = self.cache.read();
                debug!("[CHUNK] Cache expanded to {} chunks during pregeneration", cache.current_capacity());
            }

            if let Some(evicted_pos) = evicted {
                debug!("[CHUNK] Evicted {} during pregeneration", evicted_pos);
            }
        }
        Ok(())
    }

    fn receive_and_cache_all_chunks(&self, rx: &mpsc::Receiver<(ChunkPos, Chunk)>) -> Result<()> {
        // Receive all remaining chunks from the channel
        while let Ok((pos, chunk)) = rx.recv() {
            let (_, expanded, evicted) = {
                let mut cache = self.cache.write();
                cache.insert(pos, chunk)
            };

            if expanded {
                let cache = self.cache.read();
                debug!("[CHUNK] Cache expanded to {} chunks during pregeneration", cache.current_capacity());
            }

            if let Some(evicted_pos) = evicted {
                debug!("[CHUNK] Evicted {} during pregeneration", evicted_pos);
            }
        }
        Ok(())
    }

    pub fn get_chunk(&self, chunk_pos: ChunkPos) -> Result<Chunk> {
        // Check cache first
        {
            let cache = self.cache.write();
            if let Some(chunk) = cache.get(&chunk_pos) {
                debug!("[CHUNK] Cache hit for {}", chunk_pos);
                return Ok(chunk.clone());
            }
        }

        let region_pos = RegionPos::from_chunk(chunk_pos.x, chunk_pos.z);
        let region_path = self.world_dir.join(region_pos.filename());

        // Try to load from disk
        // if let Ok(chunk) = self.load_chunk_from_disk(chunk_pos) {
        if let Ok(chunk) = self.load_chunk_from_disk(chunk_pos.x, chunk_pos.z, region_path) {
            debug!("[CHUNK] Loaded chunk {} from disk", chunk_pos);
            self.cache.write().insert(chunk_pos, chunk.clone());
            return Ok(chunk);
        }

        // Generate new chunk
        debug!("[CHUNK] Generating new chunk at {}", chunk_pos);
        let chunk = self.chunk_generator.generate(chunk_pos);
        self.cache.write().insert(chunk_pos, chunk.clone());

        Ok(chunk)
    }

    #[allow(dead_code)]
    pub fn save_chunk(&self, chunk: Chunk) -> Result<()> {
        // Update cache
        let (_, expanded, evicted_key) = {
            let mut cache = self.cache.write();
            cache.insert(chunk.pos, chunk.clone())
        };

        if expanded {
            let cache = self.cache.read();
            let usage = cache.usage_ratio();
            let capacity = cache.current_capacity();
            drop(cache);
            info!("[CHUNK] Cache expanded to {} chunks ({:.1}% usage)", capacity, usage * 100.0);
        }

        if let Some(evicted_pos) = evicted_key {
            // let mut evictions = self.evictions.write();
            // *evictions += 1;
            self.evictions.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            warn!(
                "[CHUNK] Evicted low-hit chunk {} (total evictions: {})",
                evicted_pos,
                self.evictions.load(std::sync::atomic::Ordering::SeqCst)
            );
        }

        // If cache is getting full, flush to disk
        let cache = self.cache.read();
        if cache.len() > cache.current_capacity() / 2 {
            warn!("[CHUNK] Cache over 50% full, flushing to disk...");
            drop(cache);
            self.flush_cache()?;
        }

        Ok(())
    }

    pub fn flush_cache(&self) -> Result<()> {
        warn!("[CHUNK] Flushing all cached chunks to disk...");

        let start = std::time::Instant::now();

        let guard = self.cache.write();

        let mut region_map: HashMap<RegionPos, Vec<Chunk>> = HashMap::new();
        let mut saved_count = 0;
        let mut skipped_count = 0;

        self.fill_region_map(&guard, &mut skipped_count, &mut region_map, &mut saved_count);
        // explicit drop after setting up flush_tracking
        drop(guard);

        self.par_gen_cache(region_map, self.world_dir.clone());

        let duration = start.elapsed();

        info!(
            "[CHUNK] Flushed {} chunks to disk in {:.2}s ({:.0} chunks/sec){}",
            saved_count,
            duration.as_secs_f64(),
            if duration.as_secs_f64() > 0.0001 {
                saved_count as f64 / duration.as_secs_f64()
            } else {
                0.0
            },
            if skipped_count > 0 {
                format!(" (skipped {} invalid chunks)", skipped_count)
            } else {
                "".to_string()
            }
        );

        Ok(())
    }

    fn fill_region_map(
        &self,
        guard: &parking_lot::RwLockWriteGuard<'_, LruCache<ChunkPos, Chunk>>,
        skipped_count: &mut usize,
        region_map: &mut HashMap<RegionPos, Vec<Chunk>>,
        saved_count: &mut usize,
    ) {
        for (_, chunk) in guard.iter() {
            let region_pos = RegionPos::from_chunk(chunk.pos.x, chunk.pos.z);

            if !region_pos.is_valid() {
                warn!("Skipping save for chunk outside bounds: ({}, {})", chunk.pos.x, chunk.pos.z);
                skipped_count.add_assign(1);
                continue;
            }

            region_map.entry(region_pos).or_default().push(chunk.clone());
            saved_count.add_assign(1);
        }
    }

    fn par_gen_cache<P: AsRef<std::path::Path> + Send + Sync>(
        &self,
        region_map: HashMap<RegionPos, Vec<Chunk>>,
        world_dir: P,
    ) {
        let groups: Vec<(RegionPos, Vec<Chunk>)> = region_map.into_par_iter().collect();
        groups.par_iter().for_each(|(region_pos, chunks)| {
            let region_path = world_dir.as_ref().join(region_pos.filename());

            let result = (|| -> Result<()> {
                let mut region = if region_path.exists() {
                    let data = std::fs::read(&region_path)?;
                    Region::deserialize(&data)?
                } else {
                    Region::new(*region_pos)
                };

                for chunk in chunks {
                    region.insert(chunk.clone());
                }

                let serialized = region.serialize();
                std::fs::write(&region_path, serialized)?;
                Ok(())
            })();

            match result {
                Ok(()) => {
                    debug!(
                        "Saved {} chunks to region file {:?}",
                        chunks.len(),
                        region_path.canonicalize().unwrap()
                    );
                }
                Err(e) => {
                    error!("Failed to save region: {:?} ({} chunks): {}", region_pos, chunks.len(), e);
                }
            }
        });
    }

    // old impl.
    // pub fn flush_cache(&self) -> Result<()> {
    //     warn!("[CHUNK - V1] Flushing all cached chunks to disk...");
    //
    //     // PERF: @caching : DRASTICALLY improved caching times
    //     return self.flush_cache_2();
    //
    //     let chunks_to_save: Vec<Chunk> = self
    //         .cache
    //         .write()
    //         .iter()
    //         .map(|(_, chunk)| chunk.clone())
    //         .collect();
    //
    //     let region_positions: Vec<RegionPos> = chunks_to_save
    //         .iter()
    //         .map(|chunk| RegionPos::from_chunk(chunk.pos.x, chunk.pos.z))
    //         .collect();
    //
    //     let start = std::time::Instant::now();
    //
    //     for chunk in chunks_to_save {
    //         let region_pos = RegionPos::from_chunk(chunk.pos.x, chunk.pos.z); // refactor - temp
    //         self.save_chunk_to_disk_vectored(region_pos, chunk)?;
    //     }
    //
    //     let end = std::time::Instant::now();
    //     let duration = end.duration_since(start);
    //     info!(
    //         "[CHUNK] Flushed {} chunks to disk in {:.2}s ({:.00} chunks/sec)",
    //         region_positions.len(),
    //         duration.as_secs_f64(),
    //         region_positions.len() as f64 / duration.as_secs_f64()
    //     );
    //
    //     Ok(())
    // }

    fn load_chunk_from_disk(&self, chunk_x: i32, chunk_z: i32, region_path: PathBuf) -> Result<Chunk> {
        if !region_path.exists() {
            return Err(anyhow::anyhow!("Region file not found"));
        }

        let data = std::fs::read(&region_path)?;
        let region = Region::deserialize(&data)?;

        region
            .get(chunk_x, chunk_z)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Chunk not found in region"))
    }

    #[allow(dead_code)]
    pub fn cache_stats(&self) -> CacheLenCapacity {
        CacheLenCapacity::from((self.cache.read().len(), self.cache.read().current_capacity()))
    }
}

impl Clone for ChunkStorage {
    fn clone(&self) -> Self {
        Self {
            cache:           self.cache.clone(),
            world_dir:       self.world_dir.clone(),
            chunk_generator: self.chunk_generator.clone(),
            evictions:       AtomicUsize::from(self.evictions.load(std::sync::atomic::Ordering::SeqCst)),
            chunk_gen_pool:  self.chunk_gen_pool.clone(),
        }
    }
}
