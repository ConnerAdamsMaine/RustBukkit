use std::net::{IpAddr, Ipv4Addr, SocketAddr};

const SERVER_PORT: u16 = 25565;

pub const SERVER_ADDR: SocketAddr =
    SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), SERVER_PORT);

pub const CHUNK_SEED: u64 = 12345;

// not needed anymore
// pub const WORLD_NAME: &str = "world";

/// TODO: @ref : will need to move this to be a `const fn` so we can recurse upwards to the root
/// dir.
pub const WORLD_PATH: &str = "../../world";

pub const NETWORK_VALID_PROTOCOL_VERSION: i32 = 772; // Minecraft 1.21.7

pub const GAMELOOP_SLEEP_TICK: u64 = 50; // 20 ticks per second

// pub const GAMEPLOOP_TICK_RATE: u64 = 1000 / GAMELOOP_SLEEP_TICK; // technically no?
// What we're using atm
pub const GAMELOOP_TICK_RATE: u64 = 20; // 20 ticks per second (50ms per tick)

pub const GAMELOOP_DELTA_TIME: f32 = GAMELOOP_SLEEP_TICK as f32 / 1000.0; // in seconds
pub const GAMELOOP_TICK_RATE_DURATION: std::time::Duration =
    std::time::Duration::from_millis(GAMELOOP_SLEEP_TICK);

pub const TERRAIN_CHUNK_SIZE: usize = 16;
pub const TERRAIN_CHUNK_HEIGHT: usize = 256;

pub const ERROR_THRESHOLD: usize = 5;
const ERROR_WINDOW: u64 = 10;

pub const ERROR_WINDOW_SECS: std::time::Duration = std::time::Duration::from_secs(ERROR_WINDOW);

pub const CHUNK_SIZE_BYTES: usize = 232 * 1024;
pub const INITIAL_BUFFER_MB: usize = 256;
pub const MAX_BUFFER_MB: usize = 2048; // 2 GB max
pub const INITIAL_CAPACITY: usize = INITIAL_BUFFER_MB * 1024 * 1024 / CHUNK_SIZE_BYTES; // ~1130 chunks
pub const MAX_CAPACITY: usize = MAX_BUFFER_MB * 1024 * 1024 / CHUNK_SIZE_BYTES; // ~9033 chunks

pub const WORLD_MAX_CHUNKS: i32 = 10240;
pub const WORLD_REGION_SIZE: i32 = 32;
