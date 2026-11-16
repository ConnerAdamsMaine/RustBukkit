# Current State

Verified on 2026-11-10 with the working tree at base `eefa97c` plus uncommitted
changes. Status vocabulary: **PASS**, **FAIL**, **NOT RUN**. A synthetic client
is never treated as an unmodified-client pass.

## Verification summary

| Check | Result |
|---|---|
| `cargo fmt --all -- --check` | **PASS** |
| `cargo check --workspace --all-targets -j 6` | **PASS** (no errors) |
| `cargo test --workspace -j 6` | **PASS** — 142 tests, 0 failed (2026-11-10 session 2) |
| `cargo clippy --workspace --all-targets -j 6` | **PASS** (no errors; `unusual_byte_groupings` lints fixed) |
| `cargo build -p rustbukkit_bin -j 6` (debug and release) | **PASS** |
| Unmodified 26.3 client login → configuration → Play → chunk render | **PASS** |
| Unmodified 26.3 client while another player entity is present | **PASS** (real client + synthetic peer, no decode errors) |
| Two unmodified clients at once | **NOT RUN** |
| Full process restart preserves blocks and player position | **PASS** (`restart_e2e`) |
| Inventory/container network round-trip | **PARTIAL — TestClient verified** (`session_e2e::inventory_clicks_move_items_and_survive_reconnect`); real 26.3 client and full restart e2e not yet run |
| Combat over the network | **NOT IMPLEMENTED** |

Test distribution (unit + integration): protocol 77, bin 20, inventory 8,
decoding 7, chunks 6, encoding 4, config 2, pool 1, sdk 1.

## What works end to end

### Connection and configuration

- Handshake, status (MOTD, protocol 777, live player count/sample), offline
  login, Configuration and Play state transitions.
- Login Success is byte-compatible with the official 26.3 payload:
  `UUID`, username, VarInt property count, session UUID.
- Server Data uses a network-NBT text Component plus an optional icon, matching
  the official encoding.
- Configuration sends brand, feature flags, select-known-packs; after the client
  selects `minecraft:core` it sends **32 Registry Data payloads** captured from
  the official server, then the official Update Tags payload, then Finish
  Configuration. All registry indices (biome, dimension type, world clock,
  damage type, …) therefore match vanilla.
- Pre-Play phases enforce `connectionTimeoutSeconds`; idle sockets do not pin
  buffers forever.

### Play entry and world streaming

- Join Game advertises the same three dimensions as vanilla: overworld, the_end,
  the_nether. Overworld dimension type `0`, world clock `0`.
- Server Data, Update Health, Update Time, Spawn Position and initial Player
  Position are sent before chunks.
- Chunks stream around the player at `viewDistance`, with Forget Chunk and
  Set Chunk Cache Center as the player crosses chunk borders.
- Chunk payloads use the 26.3 light mask encoding (`BitSet.toByteArray()`),
  full skylight, empty block light, and biome id **41** (`minecraft:plains`).
- A real client renders them (`Resizing Chunk Sections … Instanced`).

### Multiplayer presence

- Player Info Update (action mask `0x09` = ADD_PLAYER | UPDATE_LISTED).
- Add Entity uses entity type **120** (`minecraft:player`) and a zero `LpVec3`
  movement vector.
- Move Entity Pos/Rot (packet id 55) uses 26.3 packed properties and a linear
  `VecDelta`, with fixed-point deltas from the last broadcast position.
- Remove Entities on leave; no ghost avatars in the observed runs.
- Presence is announced twice by design: a session-layer broadcast for
  join/leave and the registry presence stream for movement. On join a session
  snapshots already-online players so late joiners see earlier ones.

### Blocks and persistence

- Dig/place are validated for reach, block state and face; updates are
  broadcast to all other sessions.
- World changes are stored as a sparse overlay (`world/blocks.bin`), with a
  magic header, record count and 13-byte records, written atomically via a
  temporary file and rename.
- Player position/rotation/hotbar selection are stored per UUID as JSON in
  `world/playerdata/<uuid>.json`, saved on leave and every 30 s.
- The world is saved on graceful shutdown (Ctrl+C). `restart_e2e` proves a block
  placed over the network survives stop/start, is breakable after restart, and
  the break is persisted again.

### Measured behavior (debug build, development host)

| Condition | Slowest tick in 200-tick window | RSS |
|---|---|---|
| 1 player, steady | 52–92 µs | 11.3 MB |
| 2 players, steady | 42–92 µs | — |
| 10 players, steady | 33–60 µs | 14.5 MB |
| Join burst | 2.5–3.7 ms | — |

The join spike is synchronous chunk encode/write, not the tick loop. These are
single-run observations, not a benchmark harness; see limitations.

## Known limitations

1. **Inventory is on the wire but not yet real-client verified.** Since session 2
   (2026-11-10): per-session `Inventory` synced on join, authoritative
   `container_click` handling with full-window resyncs, drops spawn as item
   entities, pickup resyncs the window, and the inventory persists in playerdata.
   Verified with the wire-protocol TestClient only; item entities render
   without icons/metadata until entity metadata support lands (N6), and
   `QuickCraft`/`PickupAll`/`Clone` clicks are rejected with a resync.
2. **Combat is not wired.** Attack packets are not dispatched to
   `SessionRegistry::attack_player`; no health/metadata/hurt/motion/respawn
   updates reach clients.
3. **Block mechanics are partially wired.** Breaks now leave item drops
   (`drop_from_block`) and Water is unbreakable, but placement always creates
   grass, ignores the held item, and does not consume inventory.
4. **Lighting and biomes are placeholders.** Full skylight, no propagation; every
   section reports plains regardless of generated terrain.
5. **Entity support is player-only.** No mobs, item entities, metadata,
   equipment, animation or velocity packets.
6. **No authentication/encryption/compression.** Offline mode only; anyone who
   can reach the port can choose a username. `onlineMode = true` is rejected.
7. **World save is shutdown-only.** A hard kill loses block edits since start
   (player state is saved every 30 s). There is no journaling.
8. **No cross-platform verification.** All runs were on Windows; the code avoids
   hard-coded separators but Linux/macOS are untested.
9. **Benchmarking is ad hoc.** No sustained load, p99, allocation or network
   backpressure measurement.
10. **Two real clients were never launched simultaneously** on this host; the
    presence path is covered by one real client + synthetic peer and by
    `session_e2e`.

## Milestone assessment

- **Milestone A: PASS.** Unmodified client joins, enters Play, renders terrain
  and remains connected without protocol errors. Client movement is exercised by
  automated tests; the latest real-client evidence run focused on render and
  presence.
- **Milestone B: PARTIAL.** Presence, shared blocks and restart persistence work;
  items, two-real-client proof and full state coverage are missing.
- **Milestone C/D: NOT STARTED.**
