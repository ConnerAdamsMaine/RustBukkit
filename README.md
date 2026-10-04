# RustBukkit

A Rust Minecraft server implementation. The active server path is an offline-mode Minecraft 1.19.2 flat-world milestone.

**Status**: Vanilla 1.19.2 flat-world connection verified; active development | **Configured version**: 1.19.2

## Quick Start

### Prerequisites

- The Rust nightly toolchain specified in `rust-toolchain.toml`
- A Minecraft Java Edition 1.19.2 client for the manual acceptance test

### Build & Run
```bash
cargo check --workspace
cargo test --workspace
cargo run -p rustbukkit_bin
```

Run these commands from the repository root. Edit `server.conf` to select the bind address and exact Minecraft version. The current version registry supports only `mcVersion = "1.19.2"` and requires `onlineMode = false`. The default bind address is `0.0.0.0:25565`; on the same computer, connect to `localhost:25565`.

For the manual milestone check, add the server to the 1.19.2 multiplayer list, connect, verify the grass surface at Y=63 and player feet at Y=64, look around and move, stay connected for at least 60 seconds, then disconnect and reconnect. A different client protocol should receive a version-specific login disconnect.

Player locations are saved by offline UUID in `world/playerdata/` when a connection ends and restored on reconnect, including after a server restart. A player connecting for the first time starts at spawn.

For connections from the internet, allow inbound TCP port `25565` in the server computer's firewall and forward TCP port `25565` on your router to that computer's local IP address. Give players your public IP address or a domain pointing to it. `0.0.0.0` is only the bind address; players cannot use it as the server address.

On Windows, run this in an elevated PowerShell window to allow the port through the host firewall:

```powershell
New-NetFirewallRule -DisplayName "RustBukkit TCP 25565" -Direction Inbound -Action Allow -Protocol TCP -LocalPort 25565
```

The current login flow does not verify Minecraft accounts. Anyone who can reach the server can choose a username.

---

## Legacy implementation notes

The sections below describe older subsystems retained in the repository. They are not claims about the active 1.19.2 server path.

### Networking & Protocol
- **Full Minecraft 1.21.7 Protocol Support**: Handshake, Login, Configuration, and Play states
- **Async I/O**: Built on Tokio for high-performance concurrent connections
- **Proper Packet Framing**: VarInt length prefixes, packet IDs, and state-aware handling
- **Error Handling**: Comprehensive error tracking and logging with structured tracing

### Player Management
- **Connection Lifecycle**: Smooth transitions through handshake → login → configuration → play
- **Position Synchronization**: Real-time player position and rotation updates
- **UUID Tracking**: Proper player identification and persistence
- **Movement Handling**: Client-side prediction support with server validation

### Chunk System
- **Dynamic Chunk Loading**: Chunks load/unload based on player position (render distance)
- **LRU Cache**: Memory-efficient caching with automatic eviction (256MB initial, 2GB max)
- **Async Generation**: Chunks generated in thread pool without blocking main async runtime
- **Persistent Storage**: Chunks saved to disk in region files
- **Pre-generation**: Spawn area chunks pre-generated on startup (64x64 chunk grid)

### Data Management
- **Registry System**: Proper dimension and feature registries per Minecraft protocol
- **NBT Encoding**: Full NBT (Named Binary Tag) support for complex data structures
- **Entity Metadata**: Support for entity spawning and status updates

### Terrain & World
- **Terrain Generation**: Deterministic, seeded terrain with noise-based heightmaps
- **Realistic Features**: Mountains, rivers, lakes, biome transitions
- **Block Data**: Foundation for block-level world representation
- **World Persistence**: Automatic world directory creation and chunk persistence

---

## Project Structure

```
RustBukkit/
├── src/
│   ├── core/
│   │   ├── server.rs          # Main server struct, TCP listener, connection handler
│   │   ├── game_loop.rs       # 50ms tick game loop for server-wide updates
│   │   ├── thread_pool.rs     # Chunk generation thread pool management
│   │   └── mod.rs
│   │
│   ├── network/
│   │   ├── login.rs           # Handshake and Login state packet handling
│   │   ├── protocol.rs        # VarInt/NBT encoding, packet serialization
│   │   └── mod.rs
│   │
│   ├── player/
│   │   ├── player.rs          # Player struct, main handler, state machine
│   │   ├── connection_state.rs # Connection state enumeration
│   │   ├── login.rs           # Deprecated (use network/login.rs)
│   │   ├── configuration.rs   # Configuration state: features, registries, dimensions
│   │   ├── join_game.rs       # Play state initialization (spawn position, difficulty)
│   │   ├── play_state.rs      # Play state packet handlers (movement, positioning)
│   │   ├── movement_handler.rs # Client position/rotation processing
│   │   └── mod.rs
│   │
│   ├── chunk/
│   │   ├── chunk_storage.rs   # Main chunk cache, memory management, loading queue
│   │   ├── chunk_sender.rs    # Chunk packet serialization and sending
│   │   ├── chunk_data_packet.rs # Modern Chunk Data packet format (1.21.7)
│   │   ├── chunk_protocol.rs  # Legacy chunk encoding (deprecated)
│   │   ├── cache.rs           # LRU cache implementation
│   │   └── mod.rs
│   │
│   ├── world/
│   │   ├── region.rs          # Region file handling (32x32 chunk regions)
│   │   ├── mod.rs
│   │   └── [...] 
│   │
│   ├── terrain/
│   │   ├── chunk_generator.rs # Noise-based terrain generation
│   │   ├── mod.rs
│   │   └── [...]
│   │
│   ├── data/
│   │   ├── registries.rs      # Game registries (biomes, dimensions, features)
│   │   └── mod.rs
│   │
│   ├── sdk/
│   │   ├── packet_logger.rs   # Development tool for packet inspection
│   │   └── mod.rs
│   │
│   ├── main.rs                # Entry point, logging initialization
│   ├── lib.rs                 # Library exports
│   ├── serialization.rs       # Helper functions for binary serialization
│   └── error_tracker.rs       # Global error tracking and statistics
│
├── Packets/                   # Minecraft protocol reference documentation
│   ├── Connecting.md
│   ├── Chunk_format.md
│   ├── Data_Types.md
│   ├── Entity_metadata.md
│   ├── Registries.md
│   └── [...]
│
├── Protocols/                 # Protocol definitions (reserved)
│
├── build/                     # Compiled artifacts
│
├── Cargo.toml                 # Project manifest & dependencies
├── Cargo.lock                 # Locked dependency versions
├── rust-toolchain.toml        # Rust version specification
├── CHANGES.md                 # Recent session changes
├── NEXT_STEPS.md              # Development roadmap
├── terrain.md                 # Terrain generation design
├── Makefile.toml              # Build automation
└── README.md                  # This file
```

---

## Architecture

### Connection Flow

```
TCP Connection
    ↓
Handshake State
  - Client sends protocol version, address, port, next state
    ↓
Login State (if next_state == Login)
  - Start packet (client → server)
  - Login Success packet (server → client)
  - Transition to Configuration
    ↓
Configuration State
  - Plugin channels
  - Feature flags
  - Registry data (dimensions, biomes, block states, etc.)
  - Configuration Finish packet (client → server)
  - Transition to Play
    ↓
Play State
  - Join Game packet (world data, gamemode, difficulty)
  - Spawn Position packet (respawn location)
  - Synchronize Player Position packet
  - Chunks around player start loading
  - Player can move, interact, see entities
```

### Server Architecture

```
┌─────────────────────────────────────────┐
│   TCP Listener (0.0.0.0:25565)          │
└────────────────────┬────────────────────┘
                     │ accepts connections
                     ↓
        ┌────────────────────────┐
        │  handle_client task    │ (per connection)
        │   (tokio::spawn)       │
        └────────┬───────────────┘
                 │
        ┌────────────────────────┐
        │   Player State Machine │
        │  (Handshake→Login→     │
        │   Config→Play)         │
        └────────┬───────────────┘
                 │
        ┌────────┴──────────────────────┐
        ↓                               ↓
   ChunkStorage              GameLoop (50ms tick)
   - LRU Cache               - Player updates
   - Disk I/O                - Entity updates
   - Gen Queue               - Event processing
        │
        ↓
   ChunkGenThreadPool
   - Worker threads
   - Noise generation
   - Persist to disk
```

### Packet Processing Pipeline

```
Raw TCP bytes → Frame decoder (read_varint) → Packet ID
                                                  ↓
                                          State-based handler
                                          - Login Handler
                                          - Configuration Handler
                                          - Play State Handler
                                                  ↓
                                          Protocol validation
                                                  ↓
                                          Player action execution
                                                  ↓
                                          Response packet sending
```

---

## Core Components

### Server (src/core/server.rs)
- TCP listener on configurable address
- Accepts new connections asynchronously
- Spawns per-player handler tasks
- Manages world initialization
- Coordinates game loop and chunk generation

**Key Methods**:
- `MinecraftServer::new()` - Initialize server
- `MinecraftServer::run()` - Accept connections forever
- `handle_client()` - Process individual player connection

### Player (src/player/player.rs)
Represents a connected player with state transitions:
- **Handshake** → Initial connection negotiation
- **Login** → Authentication and profile validation
- **Configuration** → Registry, feature, dimension data exchange
- **Play** → Active gameplay

**Key Fields**:
- `uuid` - Player UUID
- `username` - Player name
- `socket` - TCP connection
- `state` - Current connection state
- `x, y, z` - Player position
- `loaded_chunks` - Cached visible chunks

**Position Tracking**:
- `last_chunk_x`, `last_chunk_z` - Previous chunk coordinates
- Chunks load/unload when player moves to new chunk

### Chunk Storage (src/chunk/chunk_storage.rs)
Advanced caching and memory management:
- **LRU Cache**: Configurable initial/max size
- **Memory Budgeting**: ~232 KB per chunk, 256MB→2GB range
- **Eviction Policy**: Least-recently-used chunks removed when capacity exceeded
- **Hit Count Tracking**: Reset every 5 minutes to track hot chunks
- **Disk Persistence**: Chunks saved to region files
- **Generation Queue**: Chunks generated asynchronously in thread pool

**Key Methods**:
- `get_chunk(pos)` - Fetch or generate chunk
- `pregenerate_spawn_area()` - Pre-generate 64x64 grid at startup
- `start_hit_reset_task()` - Background task for cache statistics

### Game Loop (src/core/game_loop.rs)
50ms server tick for world updates:
- Player movement validation
- Entity updates
- Event processing
- Chunk loading/unloading decisions

### Terrain Generation (src/terrain/)
Deterministic, seeded terrain:
- Multi-scale noise-based heightmaps
- Mountain ranges, valleys, plains
- Rivers and lake systems
- Biome-aware feature placement

---

## Protocol Compliance

### Implemented Packets

#### Handshake State
- ✅ Handshake (0x00)

#### Login State
- ✅ Start (0x00)
- ✅ Login Success (0x02)
- ✅ Set Compression (0x03) [optional]

#### Configuration State
- ✅ Plugin Message (0x00)
- ✅ Disconnect (0x01)
- ✅ Finish Configuration (0x02)
- ✅ Registry Data (0x05)
- ✅ Resource Pack (0x06) [minimal]
- ✅ Feature Flags (0x09)

#### Play State
- ✅ Spawn Position (0x4D)
- ✅ Synchronize Player Position (0x31)
- ✅ Entity Status (0x01)
- ✅ Chunk Data (0x21)
- ✅ Update View Position (0x40)
- ✅ Unload Chunk (0x1C)

### Data Structures
- ✅ VarInt encoding/decoding
- ✅ NBT (Named Binary Tag) support
- ✅ Position (3x i32) encoding
- ✅ UUID handling
- ✅ Chat component serialization
- ✅ Metadata serialization

For detailed packet status, see `PACKET_IMPLEMENTATION.md` (generated in recent sessions).

---

## Dependencies

| Crate | Version | Purpose |
|-------|---------|---------|
| **tokio** | 1.42 | Async runtime with full features (TCP, mutex, timers) |
| **bytes** | 1.7 | Efficient byte buffer operations |
| **uuid** | 1.10 | UUID v3/v4 generation and serialization |
| **serde** | 1.0 | Serialization framework (derive macros) |
| **serde_json** | 1.0 | JSON support for registries |
| **bincode** | 1.3 | Binary encoding for chunks |
| **anyhow** | 1.0 | Flexible error handling |
| **thiserror** | 1.0 | Derive macros for custom errors |
| **tracing** | 0.1 | Structured logging framework |
| **tracing-subscriber** | 0.3 | Logging output formatting |
| **parking_lot** | 0.12 | High-performance RwLock/Mutex |
| **futures** | 0.3.31 | Async utilities (SelectAll, etc.) |
| **smallvec** | 1.15.1 | Stack-allocated vectors |

**No unsafe code** - Project forbids all unsafe blocks by default (see `Cargo.toml` lints).

---

## Build Configuration

### Profiles
The project uses custom compilation profiles for development and release:

**Development** (`profile.dev`):
- Optimization level: 1 (balanced)
- Codegen backend: Cranelift (fast compilation)
- Debug info: Line tables only
- Debug assertions: enabled
- Overflow checks: enabled
- Build time: ~39s first build

**Release** (`profile.release`):
- Optimization level: 3 (maximum)
- Codegen backend: Cranelift
- Strip symbols from binary
- No debug assertions
- No overflow checks
- Smaller, faster binary

### Formatting
- **rustfmt** config: `.rustfmt.toml` (column width: 120)
- **TOML** formatting: `.taplo.toml` (standard)

---

## Development Guide

### Logging

The server uses `tracing` crate for structured, hierarchical logging:

```rust
use tracing::{info, debug, warn, error};

info!("[STARTUP] Server listening on 0.0.0.0:25565");
debug!("[PACKET] Read VarInt: {}", value);
warn!("[CHUNK] Evicting chunk {}", pos);
error!("[ERROR] Failed to load chunk: {}", err);
```

**Log Levels** (configured in main.rs):
- **ERROR** - Critical failures
- **WARN** - Unusual conditions
- **INFO** - Important events (startup, connections, state changes)
- **DEBUG** - Detailed operational info (packet details, chunk loading)
- **TRACE** - Very detailed (disabled by default)

### Adding New Packets

1. Define packet structure in appropriate handler module
2. Implement serialization using `BytesMut` and helper functions
3. Add packet ID constant (see protocol docs)
4. Write to socket with proper frame format: `[packet_length] [packet_id] [data]`
5. Handle state validation (can packet occur in current state?)

Example (position update):
```rust
pub async fn send_synchronize_player_position(
    socket: &mut TcpStream,
    x: f64, y: f64, z: f64,
    yaw: f32, pitch: f32,
) -> Result<()> {
    const PACKET_ID: i32 = 0x31;
    
    let mut data = BytesMut::new();
    data.put_f64(x);
    data.put_f64(y);
    data.put_f64(z);
    data.put_f32(yaw);
    data.put_f32(pitch);
    
    // Additional validation flag
    data.put_u8(0x00); // no flags
    
    write_packet(socket, PACKET_ID, data.freeze()).await?;
    Ok(())
}
```

### Testing

Currently **0% test coverage** - focus on protocol compliance testing with actual client.

**Test Strategy** (from NEXT_STEPS.md):
1. **Unit Tests**: VarInt encoding, NBT parsing
2. **Integration Tests**: Packet roundtrips, serialization
3. **Client Tests**: Connect real Minecraft client, verify:
   - Successful login/spawn
   - Chunk loading
   - Player position sync
   - Entity visibility
   - Chat messages
   - Disconnect/reconnect

Run tests:
```bash
cargo test          # Run all tests
cargo test --lib   # Library tests only
cargo test --doc   # Documentation tests
```

### Performance Tuning

**Memory Configuration** (src/chunk/chunk_storage.rs):
```rust
const INITIAL_BUFFER_MB: usize = 256;  // Initial cache size
const MAX_BUFFER_MB: usize = 2048;      // Maximum cache size
const CHUNK_SIZE_BYTES: usize = 232 * 1024;  // Est. per-chunk overhead
```

**Game Loop Tick Rate** (src/core/server.rs):
```rust
tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;  // 20 TPS
```

**Estimated Performance**:
- Chunk generation: 5-10ms per chunk
- Registry data: 1-2ms per registry
- Position sync: <1ms
- Total login: 100-200ms

---

## Compilation & Diagnostics

### Successful Compilation
```bash
$ cargo build
   Compiling rustbukkit v0.1.0
    Finished dev [optimized] profile [unoptimized + debuginfo] in 39.24s
```

**Current Status**: 
- ✅ 0 errors
- ⚠️ 16 non-critical warnings (dead code from legacy functions)
- ✅ All async/await properly handled
- ✅ No unsafe code blocks

### Common Issues

**"Connection refused"**
- Ensure the server is running and listening on `0.0.0.0:25565`
- For external connections, check the host firewall and router TCP port forwarding
- Check for port conflicts: `netstat -an | findstr 25565`

**"Chunk not loading"**
- Verify `ChunkStorage::pregenerate_spawn_area()` completed
- Check world directory exists: `./world/`
- Enable debug logging to trace chunk loading

**"Packet parsing failed"**
- Check VarInt encoding
- Verify state machine position (handshake→login→config→play)
- Review packet IDs against Minecraft wiki

---

## File Formats

### World Structure
```
world/
├── r.0.0.mca          # Region file (32×32 chunks)
├── r.1.0.mca
├── r.-1.0.mca
└── [...]
```

Each region file contains 32×32 chunks. Chunks encoded in NBT format with block data and metadata.

### Chunk Encoding
Modern format (1.21.7):
- Heightmap NBT (MOTION_BLOCKING)
- Block data NBT (per section)
- Biome data
- Lighting data (not implemented)

---

## Roadmap & Next Steps

See `NEXT_STEPS.md` for detailed development phases:

### Phase 1 (Current)
- ✅ Basic protocol compliance
- ✅ Chunk loading/unloading
- ⏳ Real-time client testing

### Phase 2 (Planned)
- Actual block data (currently all air)
- Lighting calculations
- Multi-player synchronization

### Phase 3 (Planned)
- Entity spawning and movement
- Player interactions (blocks, items)
- Inventory management

### Phase 4 (Planned)
- Chat and messaging
- Commands
- World persistence improvements

### Phase 5 (Planned)
- Performance optimizations
- Render distance scaling
- Advanced terrain features

---

## Recent Changes

**Session: January 4, 2026**

Major improvements:
- ✅ Fixed Registry Data packet structure (critical bug)
- ✅ Added Play state packet handlers
- ✅ Improved chunk_data_packet module
- ✅ Re-enabled chunk loading

**Files Modified**: 6 | **Files Created**: 5 | **Net Change**: +650 lines

See `CHANGES.md` for complete details.

---

## Contributing

Contributions welcome! Guidelines:
1. No unsafe code (forbidden by lints)
2. Use structured logging with `tracing`
3. Add comments for complex packet logic
4. Follow Minecraft protocol spec (wiki.vg)
5. Test with actual client before submitting
6. Update documentation in code and README

---

## Resources

### Protocol Documentation
- **wiki.vg** - Official Minecraft protocol specification
- `Packets/` directory - Local protocol reference
- `PACKET_IMPLEMENTATION.md` - Project's implemented packets

### Minecraft Version
- Target: **1.21.7** (latest stable)
- Protocol version: **769**

### Performance References
- Chunk size: ~232 KB
- Cache capacity: 256 MB initial → 2 GB maximum
- Game tick: 50 ms (20 TPS)
- Render distance: Dynamic based on chunk loading

---

## Troubleshooting

### "World already exists, skipping generation"
This is normal after first run. World chunks are persistent in `./world/`.

### "Waiting for world initialization..."
Server is pre-generating spawn area. Wait for "World initialization complete" message.

### Build fails with "cranelift not available"
Update Rust: `rustup update`. Cranelift backend requires recent toolchain.

### Port 25565 already in use
Stop the process using the port, or change `SERVER_PORT` in `crates/rustbukkit_bin/src/consts.rs` and update your firewall and port forwarding rules.

---

## License

See repository for license information.

## Acknowledgments

- Minecraft protocol: wiki.vg community
- Async Rust patterns: Tokio documentation
- Terrain algorithms: Various procedural generation research

---

**Last Updated**: January 5, 2026  
**Version**: 0.1.0 (Alpha)  
**Repository**: https://github.com/ConnerAdamsMaine/RustBukkit-Server
