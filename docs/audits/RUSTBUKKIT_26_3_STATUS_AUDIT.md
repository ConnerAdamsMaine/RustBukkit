# RustBukkit — Minecraft Java 26.3 Status Audit

**Audit date:** 2026-11-10
**Repository revision:** `eefa97c` (`feat(RustBukkit): derive 26.3 packet and block-state IDs from the generated reports`)
**Auditor mode:** read-only production audit. No production source file was modified. The only repository addition is this report.
**Working tree at start and end of inspection:** clean (`git status --short` produced no output); the report itself is the sole new untracked file.

---

## 1. Executive Summary

RustBukkit has a **real, working modern protocol skeleton** and a **credible data-driven basis for its 26.3 constants**, but it is **not a usable 26.3 server** and **not fully migrated**.

What demonstrably works:

- The workspace compiles cleanly (`cargo check --workspace --all-targets`, exit 0, zero warnings).
- The server binary launches, binds, serves a correct status response (`26.3`, protocol `777`) and echoes ping payloads.
- A **synthetic protocol client** can complete handshake → login → configuration → play, receives Join Game, a position, 25 chunk packets at view distance 2, health/time/spawn packets, and can move, break and place blocks; two synthetic clients observe each other's block changes; player position is persisted to disk.
- All 118 executed tests pass (135 `#[test]` attributes exist; 17 live in orphaned, non-compiled modules).
- The 26.3 packet IDs used by the encoder are locked to an extracted official report (`registry_data/26.3/packet_ids.json`) and verified by `protocol_ids_26_3.rs`; the official jar's `version.json` confirms protocol `777` and version `26.3`.

What is broken or missing, and why this is not yet a vanilla server:

1. **Login Success is malformed for a real 26.3 client.** The official `ClientboundLoginFinishedPacket` carries `(GameProfile, sessionId)`, and `ByteBufCodecs.GAME_PROFILE` encodes `UUID + name + property count + properties`. RustBukkit writes `UUID + name + UUID` (`crates/rustbukkit_protocol/src/v26_3/protocol.rs:31-36`). A probe encoding the official codec produced a 42-byte payload containing a `0x00` property count; RustBukkit's payload is 41 bytes with no property count. The synthetic test client does not parse this packet, which is why tests pass. **This is a P0 blocker to Milestone A.**
2. **Server Data is malformed.** The official 26.3 packet is `(Component motd, Optional<byte[]> icon)`. RustBukkit sends two booleans followed by a JSON string (`crates/rustbukkit_bin/src/session.rs:326-332`). **P0 for a real client reaching a usable Play state.**
3. **No registry data is sent.** Configuration sends only brand, enabled features and a known-pack offer (`crates/rustbukkit_protocol/src/v26_3.rs:38-57`). The server relies entirely on the client's `minecraft:core` 26.3 known pack and never verifies that the client offered it. Dimension-type ID 0, biome ID 0 and world-clock ID 0 are unverified assumptions.
4. **Player presence, inventory and entity traffic are absent.** `ClientboundPacket` mappings exist for Player Info, Add Entity, entity metadata/velocity/teleport/head rotation, remove entities and respawn, but none are ever sent. `SessionRegistry` simulates item entities, pickups, combat and knockback, but no code subscribes to its entity/presence broadcasts (`session.rs` subscribes only to block events). Container clicks and attacks are parsed and discarded.
5. **All compatibility evidence is synthetic.** No unmodified 26.3 client was available in this environment. README/`session_e2e.rs` successes do not establish client interoperability, and the two payload defects above mean a real client would likely fail before the synthetic tests' endpoint.
6. **Roughly half of the source tree is dead code.** About 6.6 k lines in `crates/rustbukkit_bin/src/{player,chunk,terrain,world,network,core,sdk}` plus `rustbukkit_protocol/src/v1_19_2.rs` and `nbt.rs` are not referenced by any module root and reference dependencies missing from the binary manifest (e.g. `rayon`, `bincode`). The legacy 1.19.2/1.21.7 subsystems are not reachable from the runtime, but they inflate the repository, contradict the README and create duplicate implementations.

**Bottom line:** RustBukkit is at the end of the "protocol skeleton" stage, not the beginning of "vanilla gameplay". The shortest path to a usable server is: fix the two malformed clientbound payloads, add/verify configuration registry synchronization, and stand up a real-client conformance gate; only then do presence, inventory and survival work make sense.

---

## 2. Repository Architecture

### 2.1 Workspace inventory

Workspace members (`Cargo.toml`), all edition 2024:

| Crate | LOC (`*.rs`) | Role | Used by server binary? |
|---|---:|---|---|
| `rustbukkit_bin` | 8,227 | Binary, session loop, world/player persistence, tests, **plus legacy orphan modules** | Yes (partially) |
| `rustbukkit_protocol` | 5,388 | Version pack, packet codecs, 26.3 codecs, legacy 1.19.2 fixture | Yes (partially) |
| `rustbukkit_chunks` | 524 | Block kinds, procedural terrain, sparse world overrides | Yes |
| `rustbukkit_inventory` | 442 | Item kinds/stacks, 46-slot inventory, validated client changes | Only by `session_registry`; not exposed on the wire |
| `rustbukkit_decoding` | 357 | Bounded frame decoder, wire reader | Yes |
| `rustbukkit_encoding` | 209 | Bounded frame encoder, wire writer | Yes |
| `rustbukkit_config` | 132 | `server.conf` parsing and validation | Yes |
| `rustbukkit_pool` | 14 | Placeholder (`add(left, right)`) | No |
| `rustbukkit_sdk` | 14 | Placeholder (`add(left, right)`) | No |

`Protocols/` (≈21.6 k lines, vendored azalea crates) is excluded from the workspace and is not referenced by RustBukkit code. `versions/26.3/server-26.3.jar` and `libraries/` contain the official server and dependencies used only as audit/report-generation inputs.

### 2.2 Active runtime dependency graph

```
rustbukkit (bin: src/main.rs)
 └── rustbukkit_config::ServerConfig ── server.conf
 └── rustbukkit_protocol::VersionRegistry
       └── V26_3: VersionPack
             ├── rustbukkit_encoding::Writer
             ├── rustbukkit_decoding::Reader
             ├── rustbukkit_chunks::{BlockKind, BlockPos, ChunkSnapshot, World}
             └── rustbukkit_inventory::{InventoryAction, ItemStack}
 └── rustbukkit_bin::{server, session, session_registry, player_location, world_persistence}
       ├── tokio TCP listener, 50 ms tick, per-connection tasks
       ├── `World` (parking_lot RwLock over sparse overrides + procedural terrain)
       ├── `WorldStore` (custom `blocks.bin`)
       ├── `PlayerLocationStore` (JSON per UUID)
       └── `SessionRegistry` (players, item entities, combat, physics)
```

`rustbukkit_bin/src/lib.rs` exposes only `player_location`, `server`, `session`, `session_registry`, `test_client`, `world_persistence`. `src/_lib.rs` (legacy module root) is **not** a Rust module and is never compiled; therefore `src/player/`, `src/chunk/`, `src/terrain/`, `src/world/`, `src/network/`, `src/core/`, `src/serialization.rs`, `src/error_tracker.rs` and `src/sdk/` are unreachable.

Likewise `rustbukkit_protocol/src/lib.rs` declares only `connection`, `v26_3`, `version`; `src/v1_19_2.rs` and `src/nbt.rs` are never compiled, despite the `legacy-test-fixture` feature declared in `Cargo.toml` and referenced by `rustbukkit_bin`'s dev-dependencies.

### 2.3 Integration status by subsystem

| Subsystem | Present on disk | Compiled | Reachable at runtime |
|---|---|---|---|
| Protocol/session state machine | Yes | Yes | Yes |
| Status/login/configuration | Yes | Yes | Yes |
| Play packet parsing/encoding | Yes | Yes | Yes (limited subset) |
| Registry data/tags | Encoder exists + tests | Yes | **No** (never sent) |
| Chunk encoding | Yes (26.3) | Yes | Yes |
| Legacy chunk cache/region storage | Yes | **No** | No |
| Legacy player/login handlers | Yes | **No** | No |
| Terrain generation | Yes (two implementations) | Only `rustbukkit_chunks::terrain` | Yes |
| World persistence | Yes | Yes | Yes (custom overlay only) |
| Player persistence | Yes | Yes | Position/rotation only |
| Inventory | Yes | Yes | Registry tests only; **no wire path** |
| Entities/items/combat simulation | Yes | Yes | Ticked, but **not sent or scripted** |
| Commands/permissions/chat | No | — | No |
| Observability | tracing | Yes | Yes (no metrics; tick duration logged at TRACE only) |

---

## 3. Minecraft 26.3 Migration Status

### 3.1 Version integrity

- `versions/26.3/server-26.3.jar` → `version.json`: `id/name 26.3`, `protocol_version 777`, `world_version 5023`, stable, Java 25. Verified by reading the jar; this is authoritative release metadata for the environment.
- `registry_data/26.3/packet_ids.json` and `block_states.json` are derived from the official data generator, per `registry_data/26.3/README.md`. `tests/protocol_ids_26_3.rs` asserts 20 clientbound IDs, 13 serverbound play IDs, all used configuration/status/login IDs and all eight block-state IDs against the report. **PASS.**
- No 1.19.2 packet ID is used by the runtime version pack; the 1.19.2 codec (`v1_19_2.rs`) is orphaned. The legacy `consts.rs:17` still declares protocol `772` for "1.21.7" but is in the orphaned module tree. Harmless historical reference; not reachable.

### 3.2 Migration matrix

| Subsystem | Migrated | Correct | Tested | Remaining work |
|---|---|---|---|---|
| Handshaking | Yes (ID 0, VarInt protocol, host, port, next state; trailing-byte check) | Yes (synthetic) | Unit/indirect e2e | Real-client; status version-mismatch behavior is close-without-Disconnect |
| Status | Yes (`status_response` 0, `pong_response` 1, JSON, ping echo) | Yes for JSON/pong; **online count hardcoded 0** | Measured live with a raw TCP client | Real online/sample counts; status rate limiting |
| Login | Partial (Login Start parsed as name + UUID; success ID 2; ack ID 3) | **No**: Login Success omits `GameProfile` property count | Synthetic only (does not parse Login Success) | Add `varint(0)` property count (or full property list); protocol-mismatch Disconnect |
| Configuration | Partial (brand 1, enabled features 13, select known packs 15, finish 3; inbound client info/custom payload/finish/keepalive/pong/known packs/code of conduct) | Partial: initial set has no Registry Data; hardcoded clientbound keepalive ID `0x04` | Synthetic + unit | Registry Data (or an explicit known-pack-only contract), tags, code-of-conduct flow, real client |
| Play protocol | Partial: 13 serverbound packet types parsed; 8 clientbound types actually sent | **No**: Server Data layout wrong; no presence/inventory/entity traffic | 19 decode tests; synthetic e2e | Fix Server Data; chat/commands; entity + inventory packets; batch/ack semantics |
| Registries | Encoder + NBT network writer implemented and tested | Encoder likely correct; **not synchronized** | 5 registry tests | Send correct 26.3 registries (or validate known pack), verify dimension type/biome/clock IDs |
| Serialization | VarInt/VarLong/position/UUID/strings/NBT encoder | Yes, with bounds checks | 11 NBT tests + wire tests | NBT **decoder** not present (only encoding needed today) |
| Chunk protocol | 26.3 `level_chunk_with_light` (46) with sections, heightmaps, light | Layout self-consistent; lighting is fabricated (full skylight, zero block light) | 4 chunk tests | Real-client validation; biomes; block entities; light propagation |
| Player state | Join/spawn/movement/keepalive/position persistence | Partial; no abilities/gamemode changes/respawn/death/dimension change | Unit + synthetic e2e | Presence, inventory, health/damage, respawn, dimensions |
| Entity metadata | IDs mapped only | **No** metadata encoder/usage | ID assertion only | Metadata codec, add/remove/teleport, item entities |

### 3.3 Specific legacy assumptions still visible

| Location | Legacy assumption | Reachable? | Severity |
|---|---|---|---|
| `crates/rustbukkit_bin/src/consts.rs:17` | Protocol `772` / 1.21.7 | No (orphan tree) | Informational |
| `rustbukkit_bin/src/chunk/chunk_data_packet.rs` | Chunk Data `0x20`, "1.21.7", hand-built NBT | No | Informational |
| `rustbukkit_bin/src/player/configuration.rs` | Registry Data `0x07`, finish `0x03`, `ConfigurationAckPacket` panics on unknown ID | No | Informational today; **must not be re-enabled as-is** |
| `rustbukkit_bin/src/world/region.rs` | `region_x_z_...dat` custom files, not Anvil `.mca` | No | Informational |
| `rustbukkit_protocol/src/v1_19_2.rs` | 1.19.2 block states, packet IDs, chat/signing assumptions | No | Dead code |
| `rustbukkit_bin/src/session.rs.bak` | Tests selecting `1.19.2` | No (not compiled) | Repo hygiene |
| `README.md` | 1.21.7 / protocol 769, LRU chunk cache, region files, "0% test coverage" | Documentation only | Misleading |

---

## 4. Vanilla Functionality Matrix

Classification: **Verified** = executed with meaningful evidence; **Implemented** = substantive code but unverified; **Partial**; **Stub**; **Missing**; **Broken**; **Unknown**.

### 4.1 Player systems

| Feature | Status | Evidence / gap |
|---|---|---|
| Connection/join | Verified (synthetic) | E2E reaches Play; real client blocked by Login Success layout |
| Spawn initialization | Verified (synthetic) | Spawn at `0.5,100,0.5` in measured run; spawn Y derived from terrain |
| Movement/rotation | Implemented | Finite/world-bounds validation only; no collision/speed checks; no client visibility |
| Teleportation | Partial | Server sends absolute position; teleport confirm validated against one ID |
| Gamemodes | Stub | Join Game always Survival; no abilities/change-gamemode packet |
| Player visibility | Missing | No Player Info / Add Entity / remove; `ClientboundPacket` variants unused |
| Health/damage | Stub | One hardcoded health/food/saturation packet; registry damage sim not wired |
| Hunger/saturation | Stub | Displayed once as 20/5; no simulation |
| Experience | Missing | No packets/state |
| Inventory/equipment | Missing on wire | Inventory crate exists; container clicks parsed then discarded |
| Death/respawn | Missing | `Attack` and `ClientCommand::PerformRespawn` are parsed; no handler |
| Sleeping | Missing | — |
| Dimension transitions | Missing | Overworld only; Respawn packet mapped but never sent |
| Player save/load | Verified (automated) | `player_location.rs` JSON; e2e test writes/reads position; no inventory |
| Disconnect cleanup | Verified (automated) | RAII `SessionRegistration`; presence/entity removal events |
| Multiple players | Partial | Duplicate login prevention and per-player tasks; no mutual visibility |

### 4.2 World systems

| Feature | Status | Evidence / gap |
|---|---|---|
| Chunk generation | Partial | Deterministic multi-scale noise in `rustbukkit_chunks::terrain`; 8 block kinds; no caves/ores/structures |
| Chunk loading | Implemented | Diamond radius streaming; unload with hysteresis |
| Chunk serialization | Implemented unverified | 60–63 KB per chunk in measured run; no real-client render test |
| Chunk unloading | Implemented | `forget_level_chunk` 38 at radius+1 |
| Block palettes | Implemented | 4-bit palette, 8 known block states |
| Biome palettes | **Broken/placeholder** | Every section is sent with `biome_id = 0`; no biome registry mapping |
| Heightmaps | Implemented | 9-bit packed, 3 types (IDs 1, 4, 5); real-client acceptance unverified |
| Lighting | **Placeholder** | Full skylight for all 26 sections, empty block light |
| World time | Stub | One `set_time` with gameTime/clock 1000; no tick advancement |
| Weather | Missing | — |
| World border | Missing | — |
| Dimensions | Missing | Overworld only; `dimension_type_id = 0` assumption |
| Scheduled/random ticks | Missing | — |
| World saving | Partial | Only sparse block overrides; saved at graceful shutdown only |
| Region-file compatibility | Missing | Custom `blocks.bin` (`RBWORLD1`), not Anvil |
| Restart persistence | Partial | `WorldStore` round-trip tested; no full-process restart test; crash loses unsaved blocks |

### 4.3 Block mechanics

| Feature | Status | Notes |
|---|---|---|
| Placement | Partial | Always places `GrassBlock`; no held item, no inventory consumption |
| Breaking | Partial | Any non-bedrock/non-air block instantly; no hardness, tool, or progress |
| Drops | Missing | `SessionRegistry::drop_from_block` exists but is never called |
| Harvest requirements | Missing | — |
| Hardness | Missing | — |
| Collision | Missing for players | Entity item physics has simple ground collision; players are not collided |
| Orientation | Missing | No block states/properties |
| Neighbor updates | Missing | — |
| Gravity (falling blocks) | Missing | — |
| Fluid flow | Missing | Water is static |
| Waterlogging | Missing | — |
| Doors/trapdoors/buttons/levers | Missing | — |
| Redstone | Missing | — |
| Containers/furnaces/crafting tables | Missing | — |
| Beds/signs | Missing | — |
| Block entities | Missing | Chunk packet sends zero block entities |

### 4.4 Items and inventories

| Feature | Status | Notes |
|---|---|---|
| Item registry | Stub | 5 enum kinds (`GrassBlock, Dirt, Stone, Cobblestone, Sand`) |
| Item stacks | Implemented | 1–64, validated serde |
| Data components | Missing | Parser stores opaque IDs only |
| Slot synchronization | Missing | No container content/slot packets |
| Container transactions | Partial | Validation logic + tests; `ClickContainer` discarded in session |
| Equipment | Missing | — |
| Item drops | Partial | Item entity simulation exists; no spawn packet; no drop on break |
| Item pickup | Partial | Simulation + inventory insertion tested; not wired to sessions |
| Crafting/smelting/food/durability/enchantments | Missing | — |
| Creative inventory | Missing | — |
| Inventory persistence | Missing | Only selected hotbar slot is persisted inside player location |

### 4.5 Entities and mobs

| Feature | Status | Notes |
|---|---|---|
| Entity spawning | Partial | `SessionRegistry` allocates IDs and item entities; no client spawn packets |
| Entity tracking | Partial | Chunk index inside registry; no view-based player tracking |
| Metadata | Missing | No encoder/serializer |
| Velocity/movement/gravity/collision | Partial | Item and knockback physics only; never sent |
| AI/navigation/mobs | Missing | — |
| Damage/knockback | Partial | Simulated + tested; attack packets discarded |
| Projectiles/XP orbs | Missing | — |
| Entity persistence | Missing | Item entities are in-memory only |

### 4.6 Commands and administration

| Feature | Status |
|---|---|
| Command parsing/registration/suggestions | Missing |
| Permissions/selectors | Missing |
| Administrative/world commands | Missing |
| Whitelist/bans/operators | Missing |
| Configuration loading | Verified (parsed/validated; tests) |
| Graceful shutdown | Verified (signal → session drain → world save) |
| Save command | Missing |
| Chat | Missing (no serverbound chat packet handling; no clientbound chat) |

### 4.7 26.3-specific content

Only eight block states and five item kinds are modeled. All 26.3 additions (new blocks/items/biomes/structures/recipes/tags/registries) are absent. Their absence limits content completeness; the **fatal** 26.3-specific gaps are protocol/registry, not content volume: missing registry synchronization, unverified dimension/biome/clock IDs, malformed Login Success and Server Data, and no real-client validation.

---

## 5. Runtime Compatibility Results

### 5.1 Environment and method

- Windows 11 host, Rust nightly 1.100.0, Java 25, Node 26 for raw-socket probes, official `server-26.3.jar` for codec inspection.
- Server run from a disposable `/tmp/rb-run` directory with its own `server.conf` (`127.0.0.1:25599`, view distance 2). No repository world or production config was touched.
- **No unmodified Minecraft 26.3 client was available.** Real-client interoperability is therefore **Untested**, not "pass".

### 5.2 Executed runtime checks

| Check | Result | Evidence |
|---|---|---|
| Server process starts | **PASS** | `[STARTUP] Listening on 127.0.0.1:25599 for Minecraft 26.3 (protocol 777)` |
| Idle memory (Windows tasklist) | ~10,000 K | Observed while listening; not a rigorous RSS measurement |
| Status request | **PASS** | `{"version":{"name":"26.3","protocol":777},"players":{"max":20,"online":0,"sample":[]},"motd":{"text":"AuditServer"},"secure":false}` |
| Ping/pong | **PASS** | Pong echoed `0102030405060708` |
| Handshake → Login Success (synthetic) | **PASS** | Login Success ID 2, 41-byte payload |
| Login Success layout vs official codec | **FAIL (static evidence)** | Official codec probe: 42 bytes = UUID + name + `0x00` property count + session UUID. RustBukkit: 41 bytes, no property count (`v26_3/protocol.rs:31-36`). |
| Configuration (synthetic) | **PASS** | Received brand 1, features 13, known packs 15, finish 3; sent client info/known packs/finish |
| Play entry (synthetic) | **PASS** | `JOIN_GAME len=70 entity=2`; position; 25 chunk packets; time/health/spawn/server-data |
| Server Data layout vs official codec | **FAIL (static evidence)** | RustBukkit payload starts `00 01 15 7b2274657874...` (two bools + JSON length/string). 26.3 record/codec: Component + Optional\<byte[]\> (`ClientboundServerDataPacket`). |
| Chunk data arrival (synthetic) | **PASS** | 25 × 60,570–62,620 bytes at view distance 2 |
| Movement (synthetic) | **PASS (server-side)** | Position accepted/persisted; no client-visible player entity feedback |
| Block placement/broadcast (synthetic) | **PASS** | `block_changes_reach_every_player_in_the_world` |
| Two-client world state | **PASS (synthetic)** | Both actor and peer receive the expected Block Update |
| Position persistence | **PASS (automated)** | JSON written and reloaded in test |
| Full server restart persistence | **NOT RUN** | No end-to-end process restart test; `WorldStore` round-trip only |
| Real 26.3 client | **NOT RUN** | Client unavailable in this environment |

### 5.3 Milestones

| Milestone | Status | Basis |
|---|---|---|
| M0 Workspace compiles | **PASS** | `cargo check --workspace --all-targets` exit 0 |
| M1 Server launches | **PASS** | Live launch + listener |
| M2 Status ping works | **PASS** | Live raw-socket status/pong |
| M3 Login completes | **FAIL for unmodified client** (synthetic PASS) | Login Success missing GameProfile properties |
| M4 Configuration completes | **PARTIAL / UNVERIFIED for real client** | Synthetic completes; no registry data; real client blocked at M3 |
| M5 Client reaches Play | **FAIL for unmodified client** (synthetic PASS) | Server Data malformed |
| M6 World/chunks render | **UNTESTED** | Synthetic receives chunks; no render/client check |
| M7 Movement synchronizes | **PARTIAL** | Position stored; no player entity sync |
| M8 Place/break blocks | **PARTIAL** | Server-authoritative, synthetic-verified; no item/drop/hardness semantics |
| M9 Two players share world state | **PASS (synthetic only)** | Block broadcast e2e |
| M10 Inventory/basic gameplay | **FAIL** | No wire path |
| M11 Persistence across restart | **PARTIAL** | Overlay + position I/O tested; no process-level restart |
| M12 Core survival | **FAIL** | No hunger/health/crafting/tools/drops |
| M13 Major vanilla systems | **FAIL** | No entities/mobs/commands/redstone/dimensions |
| M14 Broad 26.3 compatibility | **FAIL** | Content and systems overwhelmingly absent |

---

## 6. Compilation and Testing Results

All commands were run with `-j 6` at most, from the repository root.

| Command | Result | Detail |
|---|---|---|
| `cargo metadata --format-version 1 --no-deps` | Not run separately | Manifests read directly |
| `cargo check --workspace --all-targets -j 6` | **PASS** (exit 0) | Zero warnings |
| `cargo build -p rustbukkit_bin -j 6` | **PASS** (exit 0) | Binary produced, ~4.8 s incremental |
| `cargo test --workspace -j 6 --no-fail-fast` | **PASS** (exit 0) | **118 tests passed, 0 failed, 0 ignored** across 17 suites |
| `cargo clippy --workspace --all-targets -j 6` | **PASS** (exit 0) | 2 `unusual_byte_groupings` warnings in `configuration_packets_26_3` test |
| `cargo fmt --all -- --check` | **PASS** (exit 0) | No diff |

Test distribution (executed):

| Area | Tests |
|---|---:|
| `rustbukkit_protocol` (unit + 6 integration files) | 81 |
| `rustbukkit_bin` (unit + 3 synthetic e2e) | 25 |
| `rustbukkit_inventory` | 8 |
| `rustbukkit_decoding` | 7 |
| `rustbukkit_chunks` | 6 |
| `rustbukkit_encoding` | 4 |
| `rustbukkit_config` | 2 |
| `rustbukkit_pool`, `rustbukkit_sdk` | 1 + 1 placeholder tests |

Coverage gaps and caveats:

- **Zero real-client tests**; the central compatibility claim is unverified.
- No protocol fuzz/property tests over the frame decoder or play decoder (unit boundary tests exist).
- No integration test for two *players seeing each other*; only block broadcast.
- No test for restarting the server process and reloading `blocks.bin`.
- 135 `#[test]` attributes exist but 17 are in orphaned modules; they never execute and would not compile (missing `rayon`/`bincode`).
- Placeholder tests (`it_works` in `rustbukkit_pool` and `rustbukkit_sdk`) count toward the total but assert nothing about the server.
- Clientbound packet IDs are tested against extracted reports; **payload layouts for Login Success and Server Data are not**, and both are wrong.
- All synthetic-client tests bypass the very parsing a real client performs, so they cannot detect those defects.

---

## 7. Security and Reliability Findings

### Confirmed or high-confidence findings

| # | Severity | Finding | Affected code | Preconditions | Recommendation |
|---|---|---|---|---|---|
| S1 | **High** | No read timeout before Play. A client can open a connection and send nothing (or a partial frame) forever; status connections are unlimited and hold a task and buffer. | `rustbukkit_protocol/src/connection.rs:109-141`; `session.rs:42-46, 75-99`; `server.rs:67-90` | Network reachability only | Enforce a handshake/status/login/configuration read deadline; cap concurrent pre-play connections; drop on timeout |
| S2 | **High (design)** | No authentication/encryption; offline mode is mandatory (`onlineMode=true` rejected) and usernames are client-chosen. | `rustbukkit_config/src/server.rs:66-72`; `session.rs:118-190` | Network reachability | Document explicitly (README does); add Mojang session auth + AES/CFB8 before any public use |
| S3 | **Medium** | No movement plausibility checks: arbitrary finite positions within ±30M and Y −64..320 are accepted (fly/teleport/speed). | `session.rs:490-533`; `player_location.rs:47-58` | Authenticated (offline) client | Add per-tick displacement limits, collision/ground checks, teleport reconciliation |
| S4 | **Medium** | No block-break hardness/tool/progress validation and placement is free (always grass, no inventory). Clients can reshape the world at will within reach. | `session.rs:410-440, 570-610` | Play state | Validate break progress/tool, require held item, implement authoritative inventory |
| S5 | **Medium** | Duplicate/oversized inbound data is only bounded, not rate-limited; malformed play packets are silently ignored, enabling spam without disconnect. | `session.rs:388-396` | Play state | Rate-limit, count malformed packets, disconnect on threshold |
| S6 | **Low** | Unsupported protocol version closes without a Login Disconnect packet. | `session.rs:86-95` | Login | Send a proper disconnect reason |
| S7 | **Low** | Status reports `online: 0` regardless of connected players. | `session.rs:114` | — | Report registry player count/sample |
| S8 | **Low** | Best-effort disconnect write drops its error. | `session.rs:196-210` | — | Log at debug; acceptable |

### Positive controls observed

- Frame length bounded to `(1<<21)-1`; receive buffer capped at `2 × MAX + 10`; zero/oversized frame lengths rejected before allocation (`decoding/src/frame.rs`).
- VarInt/VarLong strictly validated (5/10 bytes, high-bit checks); strings length-checked and UTF-8 validated.
- Play collection counts bounded by both a maximum and remaining bytes (`play.rs::read_count`), preventing allocation amplification.
- NBT writer enforces payload budget, depth ≤ 512, duplicate-field rejection, Java modified UTF-8 rules (encoder only; no untrusted NBT decoding path).
- Chunk light masks/arrays consistency-checked; block-state IDs validated unique/nonnegative.
- World overlay loader validates magic, count, exact record length, coordinates and block discriminants; save uses `sync_all` + atomic rename.
- Duplicate login prevented via reservation state; max-player semaphore acquired at login and held for the session.
- No `unsafe` blocks found in compiled crates; lints deny/forbid unsafe in most crates.
- No compression is negotiated, so compression-bomb risk is currently absent (it will return when Set Compression is added).

---

## 8. Performance Findings

### Measured

| Measurement | Result | Conditions |
|---|---|---|
| Startup to listener | Well under 1 s after process start (log emitted ~0.1 s after launch in the measured run; the probe waited 1.0–1.5 s before connecting) | Debug profile, Windows, release metadata not built |
| Idle memory | ~10,000 K (tasklist) | Listening, no players |
| Status round trip | Immediate (< 10 ms observed) | Localhost |
| Login/config/play (synthetic) | Reached Play and received 25 chunks within the 3 s probe window | Localhost, view distance 2 |
| Chunk payload size | 60,570–62,620 bytes/chunk | Terrain with 8 block kinds, 24 sections, full light arrays |
| Join chunk burst | ≈1.52 MB for 25 chunks at view distance 2 | Default `viewDistance = 4` would be 81 chunks ≈ 5 MB |

### Not measured / blocked

- No Criterion benchmarks, no flamegraphs, no `perf`, no tick p99, no per-player memory, no concurrent connection scaling, no chunk serialization throughput. The README's "estimated performance" numbers are not measurements.
- Tick duration is only emitted at `TRACE` (`server.rs:61-65`) and is not exported as a metric; no overrun detection despite the 20 TPS goal.
- Gameplay performance cannot be meaningfully assessed because the server cannot be driven by a real client and most gameplay systems are absent.

### Likely bottlenecks (hypotheses requiring measurement)

1. **Per-player chunk re-encoding**: `update_chunk_view` calls `version.chunk_payload` per chunk per player; no shared encoded-chunk cache. Each payload allocates a fresh `Writer` and per-section palette/word buffers. With N players and radius 4, the same chunk is encoded N times.
2. **Snapshot copy cost**: `World::snapshot_chunk` builds 256 terrain columns and clones the entire per-chunk override `HashMap` for every send.
3. **Single global overrides lock**: all block reads/writes contend on one `parking_lot::RwLock`; chunk snapshots hold a read lock only while cloning, but high player counts will contend.
4. **Serial writes inside sessions**: each session awaits its own socket write with a 30 s timeout; a slow client blocks only itself, but the join burst is fully serialized and unbounded in bytes.
5. **Tick under one mutex**: `SessionRegistry::tick` holds the global entity `Mutex` while iterating all items/players and calling into `World` for collision reads; this is a plausible p99 risk at scale.
6. **Block event broadcast fan-out**: every session receives every block change; no interest management beyond "same world".

None of these are confirmed regressions; they are candidates for profiling after a real client can connect.

---

## 9. Code Quality and Technical Debt

### 9.1 Dead/orphaned code (confirmed)

- `rustbukkit_bin/src/{player,chunk,terrain,world,network,core,sdk}`, `serialization.rs`, `error_tracker.rs`, and `_lib.rs`: about **6,635 lines**, not referenced by `lib.rs`/`main.rs`.
- These files import crates absent from `rustbukkit_bin/Cargo.toml` (`rayon`, `bincode`), so they would not compile if re-enabled; they also duplicate the active terrain/world/chunk concepts.
- `rustbukkit_protocol/src/v1_19_2.rs` (1,018 lines) and `nbt.rs` are not declared in `lib.rs`; the `legacy-test-fixture` feature is declared but unused.
- `rustbukkit_pool` and `rustbukkit_sdk` are placeholder crates (`add(2,2)`) and are not dependencies of any crate.

### 9.2 TODOs, panics, stubs

| Location | Marker | Reachable? | Assessment |
|---|---|---|---|
| `rustbukkit_bin/src/core/game_loop.rs:48,53,58` | `todo!()` player/entity/physics updates | No | Dead code |
| `rustbukkit_bin/src/core/server.rs:97` | `panic!()` on missing world dir | No | Dead code; also bad practice |
| `rustbukkit_bin/src/player/configuration.rs:34` | `panic!` on unknown config ack ID | No | Dead code; would be remotely triggerable if re-enabled |
| `rustbukkit_protocol/src/v26_3/protocol.rs` | `.expect(...)` for Join Game/chunk/time payload construction | Yes | Safe for current static inputs, but converts encoder bugs into task panics; prefer `Result` |
| `rustbukkit_bin/src/session.rs:326-332` | Placeholder Server Data fields | Yes | Functional defect (Section 3/5) |
| `session.rs:597` | Every placement is `GrassBlock` | Yes | Placeholder mechanic |
| `session.rs:420-424` | Held slot accepted but not echoed/used | Yes | Incomplete |
| `session.rs:113-118` | Status online count hardcoded | Yes | Placeholder |

### 9.3 Architecture assessment

**Critical architectural blockers (evidence-based):**

1. **Protocol/gameplay coupling.** `rustbukkit_protocol` depends on `rustbukkit_chunks` and `rustbukkit_inventory`, and `VersionPack` owns gameplay payload construction (`join_game_payload`, `chunk_payload`, `block_update_payload`, `time_payload`) as well as parsing. This makes independent protocol conformance testing harder and forces gameplay types into the wire layer.
2. **Monolithic sessions.** `session.rs` is 733 lines with an 11-argument `run`, 12-argument `play`, and several `#[allow(clippy::too_many_arguments)]`; gameplay dispatch, codec calls, persistence and networking interleave. Adding entities/inventory/chat will worsen this.
3. **Two worlds of code.** The active world is a procedural generator + sparse override map; the legacy chunk/region/thread-pool subsystem implements an incompatible second world model. Half the tree documents a server that does not exist at runtime.

**Important maintainability problems:**

- Duplicate NBT/registry implementations (legacy `network/protocol.rs` vs `v26_3/registry.rs`).
- Duplicate terrain generators (`rustbukkit_chunks::terrain` vs orphan `bin/terrain/*`).
- A 1,030-line `SessionRegistry` containing entity simulation with no wire integration or scheduling abstraction.
- Tests over-report confidence: synthetic clients, placeholder `it_works` tests, and ID-only assertions; no payload-layout assertions against official codecs.
- 27 tracked scratch files (`.bak`, `session_*.rs`, PowerShell/Python patch scripts, `temp.hex`, `protocol.rs.txt`) and a stale README that contradicts the code.
- `Cargo.toml` dev profile uses Cranelift and `panic = "abort"`; tests run, but a panic in any task aborts the process rather than unwinding (relevant to robustness; not a current defect).

**Future plugin-compatibility observations (no implementation):**

- Rust-owned UUID/entity IDs and broadcast events are a reasonable base, but `EntityId` has **no generation counter**, so stale handles cannot be distinguished (see `session_registry.rs::EntityId`). A future plugin API would need generational handles, a scheduler, and transactional inventory APIs.
- Per-connection async tasks with direct `&mut Connection` writes are a costly-to-reverse choice for a plugin event model; an event bus and world tick scheduler would need to be introduced before Bukkit-style hooks.
- No JNI/Java/plugin code exists, consistent with the project's phase boundary.

---

## 10. Quantitative Completion Assessment

### Method

Feature inventory per subsystem, weighted by functional importance for a usable vanilla multiplayer server (weights sum to 110). Each subsystem is scored twice:

- **Implementation level**: 0 missing, 0.25 stub, 0.5 partial, 0.75 implemented-unverified, 1.0 complete.
- **Verified level**: only behavior proven by executed evidence (automated or live), with real-client evidence weighted highest. Synthetic evidence cannot verify client interoperability.

Weighted completion = `Σ(weight × level) / Σ(weight)`. These are **judgments**, not line counts; the exact weights are arguable and shown explicitly. Percentages are provisional because the real-client dimension is untested and 26.3 content coverage is only partially enumerable.

### Scores

| # | Subsystem | Weight | Implementation | Verified | Key evidence |
|---|---|---:|---:|---:|---|
| 1 | Protocol migration | 10 | 0.70 | 0.45 | IDs verified; two clientbound layouts wrong |
| 2 | Packet codecs/parsing | 10 | 0.60 | 0.35 | 13/69 serverbound, 8/144 clientbound used; layout defects |
| 3 | Connection/session management | 8 | 0.65 | 0.55 | State machine, keepalive, limits; no pre-play timeout/auth |
| 4 | World and chunk systems | 10 | 0.35 | 0.20 | Terrain/chunk encode; fake light/biome; no vanilla storage |
| 5 | Player systems | 9 | 0.35 | 0.20 | Join/move/save; no visibility/inventory/death |
| 6 | Block systems | 9 | 0.10 | 0.08 | Instant break/grass placement only |
| 7 | Items and inventory | 8 | 0.15 | 0.03 | Validated library; not wired |
| 8 | Entities and mobs | 8 | 0.08 | 0.02 | Simulation only; no packets/AI |
| 9 | World generation | 7 | 0.08 | 0.03 | Noise heightmap, 8 blocks, no vanilla features |
| 10 | Commands and administration | 6 | 0.03 | 0.00 | Config load/shutdown only |
| 11 | Persistence | 7 | 0.30 | 0.15 | Player JSON + custom world overlay |
| 12 | Security and reliability | 7 | 0.45 | 0.25 | Strong framing/bounds; no timeout/auth/anti-cheat |
| 13 | Automated testing | 6 | 0.40 | 0.40 | 118 passing; no real client/fuzz/bench |
| 14 | Performance readiness | 5 | 0.10 | 0.05 | No benchmarks/profiling/metrics |
| | **Total** | **110** | **≈33.6%** | **≈21.3%** | |

### Separate views (do not combine into one headline number)

| View | Estimate | Basis |
|---|---:|---|
| 26.3 protocol **constant** migration | ~95–100% for constants in use | Official report assertions pass; all used IDs correct |
| 26.3 protocol **behavioral** migration | ~45% | State machine complete for a happy path; layouts/registries/entities absent |
| Basic connection/session | ~60% | Works synthetically; auth/timeouts/compression absent |
| Core vanilla gameplay | ~8% | Movement + block edit; no survival loop |
| Implemented but unverified | Large (chunk encoding, world persistence, inventory library, entity sim) | No real client or integration |
| Automated test coverage of critical path | ~40% | Good unit coverage, weak conformance/e2e |
| **Release readiness as a vanilla 26.3 server** | **~5–10%** | Real client cannot be expected to log in; no survival content |

---

## 11. Prioritized Backlog

Legend: P0 critical (blocks connections/essential gameplay), P1 essential multiplayer vanilla, P2 important vanilla breadth, P3 extended content, P4 optional/tooling. Complexity XS/S/M/L/XL.

| ID | Task | Priority | Current state | Required change | Files/crates | Depends on | Acceptance criteria | Suggested tests | Complexity | Blocks |
|---|---|---|---|---|---|---|---|---|---|---|
| B1 | Fix Login Success GameProfile | **P0** | Missing property count | Encode `UUID, name, varint propertyCount=0, sessionUuid` (or full properties) | `v26_3/protocol.rs` | — | Official codec and real client accept login; byte-for-byte match with probe for empty profile | Golden-byte test vs official codec; real-client login | XS | All gameplay |
| B2 | Fix Server Data layout | **P0** | Two bools + JSON | Encode Component (NBT chat) + `Optional<byte[]>` icon (`false`) | `session.rs`; add encoder in `v26_3` | B1 | Client accepts packet; no play-state decode error | Golden-byte test; real-client play entry | XS | Play state |
| B3 | Real-client conformance gate | **P0** | None | Documented manual checklist + scripted launch of unmodified 26.3 client; capture logs/screenshots; add CI-able protocol trace diff | `docs/`, `tests/`, dev tooling | B1, B2 | Unmodified client reaches Play and renders terrain | Manual acceptance plus trace comparison | M | Verification of everything |
| B4 | Registry synchronization | **P0** | No Registry Data sent | Send 26.3 synchronized registries (or explicitly validate `minecraft:core` known pack and skip correctly); verify dimension type/biome/clock IDs against official data | `v26_3/registry.rs`, `v26_3.rs`, `session.rs` | B1/B2 | Client resolves overworld dimension/biome without disconnect | Registry packet golden tests; real-client join | L | World rendering, dimensions |
| B5 | Pre-play read deadlines + connection cap | **P1** | No read timeout before Play | Add per-state read deadline and pre-play connection limit | `connection.rs`, `session.rs`, `server.rs` | — | Idle connections dropped; cap enforced | Tokio time tests; load probe | S | DoS resistance |
| B6 | Player visibility | **P1** | Missing | Send Player Info Update, Add Entity for players, remove on leave, spawn/despawn by view | `session.rs`, `session_registry.rs`, `v26_3/clientbound.rs` | B1–B4 | Two clients see each other's avatars and movement | Two synthetic clients parse add/remove; real-client two-player test | M | Multiplayer |
| B7 | Inventory wire path | **P1** | Missing | Send container content/slot/held-item; handle container click with authoritative validation; persist inventory | `session_registry.rs`, `session.rs`, `inventory`, `v26_3/play.rs` | B6 | Pick up, move, drop, hotbar sync; no duplication | Inventory transaction property tests; reconnect test | L | Survival |
| B8 | Entity event wiring | **P1** | Simulation only | Subscribe sessions to entity/presence events; encode add/update/remove/velocity/head rotation; wire attacks and drops | `session_registry.rs`, `session.rs`, `v26_3/clientbound.rs` | B6 | Item drops/pickups and combat visible to clients | Entity packet golden tests; two-client item test | M | Entities/combat |
| B9 | Real lighting and biomes | **P1** | Constant full sky, biome 0 | Compute skylight/blocklight and biome IDs per section; send correct light masks | `v26_3/chunk.rs`, `rustbukkit_chunks` | B4 | Client renders day/night-appropriate light and biome colors | Chunk light fixtures; real-client screenshots | L | World correctness |
| B10 | Correct block mechanics | **P2** | Instant break/free grass | Held-item validation, hardness/progress, drops, inventory consumption, place selected block | `session.rs`, `session_registry.rs`, `inventory` | B7 | Survival-accurate break/place with drops | Mechanic unit tests; real-client interaction | L | Survival |
| B11 | Player physics/collision | **P2** | Bounds-only validation | Server collision, gravity, step-up, water/lava basics; reconcile client predictions | `session_registry.rs`, `session.rs`, `chunks` | B9 | Player cannot fly/clip; falls and lands correctly | Physics fixtures; real-client movement | L | Survival |
| B12 | Health/damage/respawn/death | **P2** | Stub | Wire damage simulation to health packets, death screen, respawn packet, drops | `session_registry.rs`, `session.rs` | B8, B11 | Player can die, drop items, respawn | Damage/respawn tests; real client | M | Survival |
| B13 | World time/weather ticking | **P2** | Static time | Advance game time/clocks per tick; weather state | `server.rs`, `v26_3/clientbound.rs` | B9 | Day/night progresses for clients | Clock test; real client | S | Vanilla breadth |
| B14 | Chat + command framework | **P2** | Missing | Parse 26.3 chat/command packets, Brigadier tree, dispatch, permissions | new crate/module | B6 | `/help`, chat visible to all; command tree accepted | Command parser tests; real client | L | Administration |
| B15 | Persistent world format | **P2** | Custom overlay, shutdown-only save | Decide Anvil compatibility vs documented custom format; periodic/crash-safe save; block entities | `world_persistence.rs` | — | Restart preserves all edits; format documented | Process restart test; corruption tests | M | Production use |
| B16 | Vanilla terrain/content | **P3** | Noise + 8 blocks | Biomes, caves, ores, structures, features, 26.3 blocks/items/recipes/tags | `rustbukkit_chunks`, registry data | B4/B9 | Generated world resembles vanilla for a defined radius | Determinism/coverage tests; differential comparison | XL | Content parity |
| B17 | Mobs/AI | **P3** | Missing | Entity types, metadata, spawn rules, AI/navigation, persistence | new modules | B8/B11 | Passive/hostile mobs behave and persist | AI fixtures; real-client scenarios | XL | Vanilla breadth |
| B18 | Benchmark/profiling harness | **P4** | None | Criterion benches for chunk encode/world tick; metrics for tick p99/players | `benches/`, `server.rs` | B3 | Reproducible baseline with p50/p95/p99 | Bench CI | M | Performance work |
| B19 | Remove/relocate legacy code | **P4** | 6.6 k orphan lines + scratch files | Delete or move to an `archive/` outside the workspace; fix README claims | repo-wide | — | No uncompiled modules in crates; README matches behavior | `cargo check`; repo review | S | Maintainability |
| B20 | Online mode + encryption | **P4** | Offline only | Mojang session auth, AES/CFB8, compression negotiation | protocol/network | B1 | Authenticated joins; compression supported | Auth/compression tests | L | Public deployment |

---

## 12. Recommended Development Cycles

Each cycle should end with a real-client check, not only unit tests.

### Cycle 0 — "A real client can log in and stand in the world" (P0)

Tasks: B1, B2, B3, B4 (minimum registry correctness), B5.
Exit criteria: unmodified 26.3 client completes login, configuration and play; Join Game accepted; spawn position applied; chunks render; no decode errors in client log. This cycle should not add gameplay. Its output is a trustworthy baseline and a trace/golden-byte harness that prevents payload regressions.

### Cycle 1 — "Multiplayer presence and inventory basics" (P1)

Tasks: B6, B7 (read-only sync + validated clicks), B8 (item entities), B5 hardening.
Exit criteria: two real clients see each other, see held items and inventory changes, pick up drops, and observe block changes. No duplication under concurrent clicks.

### Cycle 2 — "World correctness" (P1/P2)

Tasks: B9, B11, B13, B15.
Exit criteria: correct lighting/biomes, server-side collision/gravity, advancing time, restart-safe persistence. This is the first cycle where survival mechanics become meaningful.

### Cycle 3 — "Survival core" (P2)

Tasks: B10, B12, plus eating/health regeneration and basic crafting from the item registry.
Exit criteria: break/place respects tools and inventory; drops/pickup; damage/death/respawn; a player can survive a short session.

### Cycle 4 — "Commands and administration" (P2/P3)

Tasks: B14, whitelist/ops/bans, save command, MOTD/player counts.
Exit criteria: operators can manage a small server; command tree accepted by the client.

### Cycle 5 — "Vanilla breadth" (P3+)

Tasks: B16, B17, dimensions, redstone/containers/block entities, 26.3 content registries.
Exit criteria: defined vanilla feature matrix passes differential tests; not required for Milestones A–C.

### Critical path

- **Milestone A (client connects and enters world):** B1 → B2 → B4 → B3. B5 should ship with it. Without B1/B2 this milestone is impossible; without B4 the client may reject the dimension/biome state.
- **Milestone B (two players in a persistent shared world):** A + B6 + B8 + B15. B7 is needed for a meaningful shared-world experience but not strictly for presence.
- **Milestone C (core survival):** B + B9 + B11 + B7 + B10 + B12.
- **Milestone D (broad 26.3 compatibility):** C + B14 + B16 + B17 + dimensions/redstone/containers.

---

## 13. Critical Blockers

1. **Login Success payload is missing the GameProfile property count.** High confidence from the official record/codec and a direct encode probe. Blocks every real client at Login. (B1)
2. **Server Data payload has the wrong structure.** High confidence from the official record/field codec and the emitted bytes. Blocks a clean Play-state entry. (B2)
3. **Configuration registry synchronization is absent and dimension/biome/clock IDs are assumptions.** Even after B1/B2, the client may reject or misrender the world; only a real client can settle this. (B4)
4. **No real-client conformance harness.** All current "working" claims are synthetic; regressions in payload layout are invisible to the test suite. (B3)
5. **Presence and inventory are not on the wire.** Even a client that reaches Play sees an empty world of blocks with no other players, items, or HUD state. (B6, B7, B8)

---

## 14. Unknowns and Validation Gaps

| Unknown | Why it matters | How to verify |
|---|---|---|
| Whether a real 26.3 client accepts the current chunk section/heightmap/biome encoding | Self-tests only prove internal consistency | Real-client join + visual check; capture client log |
| Correct 26.3 dimension-type, biome and world-clock registry IDs | `dimension_type_id=0`, `biome_id=0`, `clock=0` are unverified; wrong IDs can disconnect or misrender | Extract the official dynamic registry dump/data generator output; compare |
| Whether omitting Registry Data is acceptable with `minecraft:core` known pack, and whether the client must be validated | Current server assumes it; modded/alternate clients may fail | Real-client tests with and without known packs; server-side known-pack enforcement |
| Correct `ClientboundLevelChunkPacketData` section field order (block count, fluid count, biome container) | Layout could be subtly wrong without a client | Golden encode against official codec or real client |
| Heightmap type IDs `[1,4,5]` | Wrong IDs lead to client heightmap errors | Official `Heightmap.Types` ID extraction / real client |
| Whether Login Success requires a non-empty property list for offline profiles | We verified the count byte is required; empty list should be valid | Real client |
| Performance at scale | No benchmarks; per-player chunk encoding and global locks are unmeasured | Criterion benches, flamegraphs, multi-bot load |
| Cross-platform behavior (Linux/macOS) | Only Windows was tested | CI matrix |
| Crash recovery and world corruption resistance | Only graceful shutdown is tested | Kill -9 during edits; reload; validate checksums |
| Behavior under malformed/fuzzed packets | Only boundary unit tests | `cargo-fuzz` on frame/play decoders |
| Bukkit/Paper compatibility obstacles | No design yet | Deferred by project phase |

---

## 15. Final Assessment

**1. Is RustBukkit fully migrated to Minecraft 26.3?**
No. Version selection, status/login/configuration/play state machines, packet IDs and block-state IDs are migrated and the IDs are verified against the official report. Two clientbound payloads (Login Success, Server Data) are structurally wrong, registry synchronization is absent, and the 1.19.2 codec remains in-tree (orphaned but tracked). Behavioral migration is roughly half done; content migration has barely started.

**2. Can an unmodified 26.3 client connect successfully?**
There is no evidence that it can, and strong protocol evidence that it cannot: Login Success omits the `GameProfile` property count that the official codec writes. A real client was unavailable, so this is a high-confidence static finding rather than an observed disconnect. Treat Milestone A as **not achieved**.

**3. Can a player spawn and interact with the world?**
Only with the project's synthetic client. That client reaches Play, receives a spawn position and 25 chunks, can move, break and place blocks, and receives its own block confirmation. A real client is blocked earlier (Login Success), so player-visible spawn/interaction is **unverified**.

**4. Does multiplayer work reliably?**
Block changes are authoritative and broadcast; duplicate logins are rejected; two synthetic sessions observe each other's edits in an automated test. Players cannot see each other, inventories do not exist on the wire, and reliability under real concurrent clients is untested. **Partial, synthetic-only.**

**5. Does world persistence work?**
Partially. Player position/rotation/hotbar selection persists to JSON, and sparse block overrides persist to a custom `RBWORLD1` file with atomic writes and validation. Both are tested at the store level. There is no periodic world save, no crash recovery test, no inventory persistence, and no Anvil/region compatibility. **Partial, not production-grade.**

**6. Which vanilla mechanics are currently functional?**
Status/ping; login/configuration/play happy path for a synthetic client; movement acceptance with bounds checks; authoritative break/place of a small block set with broadcast; keepalives; player position persistence; item-entity/combat simulation inside `SessionRegistry` (not visible to clients). Everything else — survival, inventory, mobs, chat, commands, redstone, dimensions, weather, lighting, biomes, drops, crafting — is missing or placeholder.

**7. What proportion of required functionality is verified?**
Using the weighted method in Section 10: **≈34% implemented coverage** and **≈21% verified functional coverage** across 14 subsystems (denominator 110 weight units). Separately, protocol constants are ~100% verified, while release readiness as a vanilla server is ~5–10%. These are judgments with explicit weights, not line-count metrics; the real-client dimension is entirely unverified.

**8. What is the most serious architectural problem?**
The absence of a **protocol conformance boundary**: the version pack mixes wire layout with gameplay payload construction, there are no golden byte tests against official codecs, and the only end-to-end harness is a synthetic client that skips the client's parsing. That is why two P0 payload bugs coexist with a green test suite. The second serious problem is the split brain between the active small world model and ~6.6 k lines of unreachable legacy world/chunk/player code, which obscures the real implementation.

**9. What are the five highest-priority development tasks?**
1. Fix Login Success to encode `GameProfile` properties (B1).
2. Fix Server Data to encode Component + optional icon (B2).
3. Add correct 26.3 registry synchronization and verify dimension/biome/clock IDs (B4).
4. Establish a real-client conformance gate with golden-byte packet tests (B3).
5. Add pre-play read deadlines/connection caps, then wire player presence and inventory (B5, B6, B7).

**10. What must happen before RustBukkit qualifies as a usable vanilla server?**
At minimum: an unmodified 26.3 client logs in, completes configuration with correct registries, enters Play without decode errors, renders terrain with plausible lighting/biomes, moves with server-validated collision, sees other players, and can use an inventory that survives reconnect. Persistence must survive both graceful restart and a crash. Then survival mechanics (break/place with drops, health/damage/death/respawn, hunger, basic crafting), chat/commands, and a defined 26.3 content matrix must pass automated and real-client tests. The current repository is a solid protocol prototype on that path, not a server that satisfies it.

---

## Appendix A — Commands and raw evidence

```text
cargo check --workspace --all-targets -j 6      -> EXIT 0, 0 warnings
cargo test --workspace -j 6 --no-fail-fast      -> EXIT 0, 118 passed, 0 failed, 0 ignored
cargo clippy --workspace --all-targets -j 6     -> EXIT 0, 2 warnings (test: unusual_byte_groupings)
cargo fmt --all -- --check                      -> EXIT 0
cargo build -p rustbukkit_bin -j 6              -> EXIT 0, target/debug/rustbukkit.exe
```

Live server (disposable directory, port 25599):

```text
[STARTUP] Listening on 127.0.0.1:25599 for Minecraft 26.3 (protocol 777)
STATUS {"version":{"name":"26.3","protocol":777},"players":{"max":20,"online":0,"sample":[]},"motd":{"text":"AuditServer"},"secure":false}
PONG 0102030405060708
LOGIN_SUCCESS len=41 hex=2dae...084175646974426f747cc200ac8e3e4ec3af7432538fe766
CONFIG_FINISH
JOIN_GAME len=70 entity=2
SERVER_DATA hex=0001157b2274657874223a2252757374
PLAYER_POSITION len=61
CHUNK_DATA len=60570 ... (25 chunks, 60570–62620 bytes each)
```

Official codec probe (`ClientboundLoginFinishedPacket.STREAM_CODEC`):

```text
LoginFinished len=42 hex=65bc1c9d5c5d40faa4f92588ca4709f5084175646974426f74002e11756af0e94863af7066d660a051b6
                                     ^ uuid (16)                        ^ name (0x08 "AuditBot") ^ 0x00 property count ^ session uuid
```

`javap` on the official jar:

```text
ClientboundLoginFinishedPacket: fields GameProfile gameProfile, UUID sessionId
  STREAM_CODEC uses ByteBufCodecs.GAME_PROFILE + UUIDUtil.STREAM_CODEC
ByteBufCodecs.GAME_PROFILE = composite(UUIDUtil.STREAM_CODEC, PLAYER_NAME, GAME_PROFILE_PROPERTIES, ...)
ClientboundServerDataPacket: fields Component motd, Optional<byte[]> iconBytes
ClientboundSetHealthPacket: float health, int food, float saturation
ClientboundSetTimePacket: long gameTime, Map<Holder<WorldClock>, ClockNetworkState>
ClockNetworkState: long totalTicks, float partialTick, float rate
ClientboundLoginPacket: playerId, hardcore, Set<Level>, maxPlayers, chunkRadius,
  simulationDistance, reducedDebugInfo, showDeathScreen, doLimitedCrafting,
  CommonPlayerSpawnInfo, onlineMode, enforcesSecureChat
CommonPlayerSpawnInfo: dimensionType(Holder), dimension, seed, gameType,
  previousGameType(Optional), isDebug, isFlat, lastDeathLocation(Optional), portalCooldown, seaLevel
LevelData.RespawnData: GlobalPos globalPos, float yaw, float pitch
PositionMoveRotation: Vec3 position, Vec3 deltaMovement, float yRot, float xRot
ClientboundLevelChunkPacketData: Map<Heightmap.Types, long[]> heightmaps, byte[] buffer,
  List<BlockEntityInfo> blockEntitiesData
ClientboundLightUpdatePacketData: BitSets sky/block/emptySky/emptyBlock + sky/block updates
```

## Appendix B — Audit integrity statement

- No production source, test, manifest or configuration file was modified.
- Temporary Java/Node probes and logs were written under the system temp directory (`/tmp/rb-run`), not the repository.
- The server was run against a disposable temp world, not the repository `world/` directory.
- `git status --short` was empty before the audit and contains only this report afterward.
- No Git commit, push, reset, clean or other history operation was performed.
- All unexecuted checks are labelled **NOT RUN** or **UNVERIFIED**; no test result was inferred from compilation or from test names.
