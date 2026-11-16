# Architecture

## Workspace

Cargo workspace (edition 2024, nightly, Cranelift codegen). `Protocols/` is
excluded and is only Azalea reference material.

| Crate | Role |
|---|---|
| `rustbukkit_bin` | Executable, TCP server, session state machine, presence registry, world/player persistence, test client |
| `rustbukkit_protocol` | Version-independent traits (`VersionPack`) and the complete `v26_3` implementation: encoders, decoders, chunk packet, registry NBT, packet IDs |
| `rustbukkit_chunks` | Deterministic terrain generator, sparse world overlay, chunk snapshots, block kinds |
| `rustbukkit_decoding` | Bounds-checked frame decoding and wire reader (`FrameDecoder`, `Reader`) |
| `rustbukkit_encoding` | Frame encoder and wire writer (`Writer`) |
| `rustbukkit_inventory` | Item stacks, 46-slot inventory, validated client change application (not network-wired) |
| `rustbukkit_config` | `server.conf` parsing and validation |
| `rustbukkit_pool` | Template stub (`add()` only); unused by the runtime |
| `rustbukkit_sdk` | Template stub (`add()` only); unused by the runtime |

`rustbukkit_bin` modules:

- `main.rs` — loads `server.conf`, selects the version, starts `server::run`.
- `server.rs` — listener, 20 TPS tick, per-connection task, graceful shutdown,
  world save, presence/block broadcast channels.
- `session.rs` — handshake → status/login → configuration → play, packet loop,
  chunk view, movement validation, dig/place, keepalive, disconnect save.
- `session_registry.rs` — entity IDs, player snapshots, presence broadcast,
  reservation/promotion, player damage/living state, item-entity types.
- `world_persistence.rs` — `RBWORLD1` sparse block overlay.
- `player_location.rs` — per-player JSON position store.
- `test_client.rs` — in-process protocol client used by integration tests.
- `player_location.rs` / `test_client.rs` are compiled into the library so tests
  exercise production connection handling rather than a parallel server.

## Version boundary

Gameplay code receives `&'static dyn VersionPack` and asks it for payloads and
packet IDs. **Do not hand-assemble clientbound packets in `session.rs`.** IDs and
layouts are version facts owned by `crates/rustbukkit_protocol/src/v26_3/`:

- `protocol.rs` — `V26_3` pack impl, Join Game, login, status.
- `clientbound.rs` — Server Data, player info, add/remove/move entity, health,
  time, spawn position, block update.
- `chunk.rs` — chunk-with-light encoding and chunk packet IDs.
- `registry.rs` — network NBT component encoder and registry payload helpers.
- `play.rs` — serverbound play packet parsing (movement, dig, place, attack,
  click container, use item, player actions).

## Connection lifecycle

```
TcpListener::accept
  → per-connection task (Connection::new)
  → session::run
      handshake  → status (JSON) or login
      login      → Login Success, Login Acknowledged → Configuration
      configuration:
          initial packets: brand(1), features(13), select_known_packs(15)
          client known_packs → registry_data(7) × 32
                             → update_tags(14)
                             → finish_configuration(3)
          client finish(3) → promote session, load position
      play:
          Join Game(50), Server Data, Update Health, Time(115),
          Spawn Position(99), Player Position(73),
          self Player Info, online players' info + Add Entity,
          chunk view, loop
```

Each phase has a read deadline derived from `connectionTimeoutSeconds`.

## Play loop

`session::play` selects over:

- `connection.read_frame()` — serverbound packets. Movement is validated by
  `moved()` (finite, bounds, teleport distance limits) and rejected movement
  triggers a fresh Player Position teleport. Dig/place require reach and update
  `World`. Keepalive replies must match the outstanding id.
- `block_events` — other sessions' block changes, sent as Block Update.
- `presence_events` — session-layer join/leave broadcast, converted to Player
  Info + Add Entity / Remove Entities.
- registry presence stream — `Moved` snapshots converted to Move Entity with
  deltas relative to `tracked_positions`.
- keepalive interval — sends KeepAlive, enforces timeout.
- shutdown watch — break, then save position and announce leave.

Acceptance of a teleport id is strict: an unexpected Teleport Confirm ends the
session.

## Concurrency

- One Tokio task per connection; writes are sequential per session.
- `World` uses `parking_lot::RwLock` around a sparse override map; terrain is
  generated on demand from a deterministic seed. `set_block_if` performs
  compare-and-swap under one write lock.
- `SessionRegistry` lives behind `Arc`, reserves slots before Play, and
  publishes presence snapshots. Broadcast channels are bounded (1024); lagged
  receivers are logged and skipped.
- The 20 TPS loop calls `registry.tick(&world)` and records slow ticks
  (warn at ≥ 50 ms, summary every 200 ticks).
- Blocks are shared state; packets never mutate authoritative state directly
  without validation in the session layer.

## World model

- Terrain is procedural (`TerrainGenerator`, default seed `12345`, sea level
  100) — **not** vanilla worldgen. Only changed blocks are stored.
- `World` = terrain function + sparse overrides; `ChunkSnapshot` materializes 16×16
  terrain columns plus overrides for encoding.
- Coordinate helpers use Euclidean division and validate the −29,999,984 ..
  29,999,984 range and −64..320 height.

### On-disk formats

`world/blocks.bin`:

```
"RBWORLD1" (8 bytes) | u32 BE record count | records
record = x:i32 BE | y:i32 BE | z:i32 BE | kind:u8
kind: 0 air, 1 bedrock, 2 dirt, 3 grass, 4 stone, 5 cobblestone, 6 sand, 7 water
```

Load rejects bad magic, counts over 1,000,000 and length mismatches. Save writes
a UUID-named temporary file, `sync_all`, then renames over `blocks.bin`.

`world/playerdata/<uuid>.json`:

```json
{"uuid":"…","username":"…","dimension":"minecraft:overworld",
 "location":{"x":0.5,"y":100.0,"z":0.5,"yaw":0.0,"pitch":0.0,
             "on_ground":true,"selected_hotbar_slot":0}}
```

Legacy location-only JSON is accepted; identity mismatch falls back to spawn.
Coordinates are validated before use.

## Design rules to preserve

1. Server authoritative movement, blocks and (future) inventory/combat.
2. Validate every untrusted length, string, count and index before allocation.
3. No packet IDs or layouts outside the version pack.
4. No side effects from decoders; state changes happen in session/registry code.
5. Keep packet construction testable and byte-level tested against 26.3 data.
6. Do not introduce JNI/Bukkit/plugin layers before the vanilla gate.
