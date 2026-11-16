# RustBukkit — 26.3 Implementation Status

**Date:** 2026-11-10
**Base revision:** `eefa97c` (plus the pre-existing untracked audit/handoff documents)
**Scope:** P0 protocol repair, configuration synchronization, real-client gate,
live presence wiring, orphan disposition, restart persistence, hardening.

This is an implementation report, not an audit. Every item states how it was
verified; anything not executed is labelled **NOT RUN**.

---

## 1. Milestone A — unmodified 26.3 client

**Status: PASS (automated evidence, single player + synthetic peer).**

An unmodified Minecraft Java Edition 26.3 client (official `26.3.jar`, Java 25,
asset index 34, offline profile `RealClient`) was launched against RustBukkit
and reached Play. Evidence:

- Server: `[PLAY] RealClient joined entity=2 at 0.5,100.0,0.5`.
- Client: `Connecting to 127.0.0.1, 25599`, then chunk-section renderer
  allocations (`Resizing Chunk Sections Command Buffer/Instanced`), no
  `Failed to decode packet`, no disconnect other than the launcher timeout that
  killed the client (`[PLAY] RealClient left ...`).
- A later run with a live synthetic peer (`PeerAI`) exercised Player Info, Add
  Entity and Move Entity traffic to the real client. The first attempt failed
  with `add_entity ... found 5 bytes extra`; the next exposed
  `move_entity_pos_rot` decode failure. Both layouts were corrected from the
  official bytecode (below) and the final run kept the real client connected
  through continuous peer movement with no decode error and 2 players ticking.

This required four protocol fixes, each validated against either official
server bytes or the official bytecode, plus regression tests:

| Fix | Evidence | Test |
|---|---|---|
| Login Success now `UUID, name, VarInt property count, session UUID` | Official server capture: 39-byte payload for `Probe`; previous encoder omitted the property count (37 bytes) | `golden_payloads_26_3::login_success_is_byte_identical_to_the_official_26_3_server` |
| Server Data now network-NBT text Component + `Optional<byte[]>` icon | Official capture: `08 00 0c "VanillaProbe" 00`; previous encoder sent two booleans + JSON | `golden_payloads_26_3::server_data_matches_the_official_26_3_component_and_icon_optional` |
| Configuration sends the full Registry Data snapshot and Update Tags | Client aborted configuration with `Missing tag: 'minecraft:sulfur_cube_archetype/slow_sliding' in 'minecraft:item'` until tags were sent | `golden_payloads_26_3::configuration_registry_snapshot_locks_biome_dimension_and_clock_ids`, `configuration_tags_cover_item_tags_referenced_by_registries` |
| Chunk light masks use 26.3 `BitSet.toByteArray()` | Client rejected every chunk with `Failed to decode packet clientbound/minecraft:level_chunk_with_light`; official bytecode shows `ByteBufCodecs.BIT_SET` = VarInt byte length + little-endian bytes | `v26_3::chunk::tests::light_masks_use_the_26_3_bitset_byte_array_encoding`, updated `chunk_packets_26_3` |
| Add Entity movement uses `LpVec3` | Client: `add_entity ... found 5 bytes extra`; bytecode shows `LpVec3.read` returns `Vec3.ZERO` after a single zero byte | `clientbound::tests::player_info_and_add_entity_match_the_captured_26_3_layout` |
| Move Entity Pos/Rot uses packed properties + `VecDelta` | Client: `Failed to decode packet 'clientbound/minecraft:move_entity_pos_rot'`; bytecode shows `entityId, VarInt(onGround | stepCount << 1), VecDelta, yRot, xRot` | covered by `players_receive_each_others_presence_and_movement`, plus a real-client run with a live peer and 8 synthetic load peers |

### Identifier corrections

- `minecraft:plains` biome network ID is **41** (was hardcoded `0`, which is
  `minecraft:badlands`).
- `minecraft:dimension_type` overworld is **0** (verified, unchanged).
- `minecraft:world_clock` overworld is **0** (verified, unchanged).
- Join Game now advertises the same three dimensions as vanilla:
  overworld, the_end, the_nether.

## 2. Configuration synchronization

RustBukkit now replays the official server's 32 Configuration Registry Data
payloads (9,943 bytes: biome, chat_type, trim patterns/materials, villager and
animal variants, painting, dimension_type, damage_type, banner_pattern,
enchantment, jukebox_song, instrument, dialog, world_clock, timeline, ...), then
the official Update Tags payload (58,754 bytes), then Finish Configuration.
Registry indices therefore match vanilla exactly. The snapshot is derived
protocol data checked into `registry_data/26.3/` with provenance, and the client
no longer depends on its own known-pack contents.

## 3. Live gameplay wiring (presence)

`SessionRegistry` events are now consumed by live sessions:

- Player Info Update (`0x09` = ADD_PLAYER | UPDATE_LISTED) for self, existing
  players on join, and later joiners.
- Add Entity for player avatars (entity type `120`, zero `LpVec3` movement).
- Move Entity Pos/Rot (fixed-point deltas) from `PresenceEvent::Moved`.
- Remove Entities on leave.
- Sessions learn presence from a session-layer broadcast (own join) plus the
  registry presence stream (movement), and existing players are snapshotted on
  join, so no ghost or missing avatars.

Verified by `players_receive_each_others_presence_and_movement` (two synthetic
clients) and by the real-client run with `PeerAI`.

## 4. Persistence across a full server restart

`world_edits_and_player_position_survive_a_server_restart` starts a real server
instance, places a block through the network, moves, stops the server (graceful
drain + save), starts a second server instance on the same directory, asserts
the saved position is restored, breaks the pre-restart block (proving the
reloaded world contains it), and finally reloads `blocks.bin` to assert the
post-restart break persisted.

## 5. Hardening

- Pre-Play read deadlines (handshake, status, login, configuration) using
  `connectionTimeoutSeconds`; idle pre-play connections are dropped instead of
  holding a task and buffers forever.
- Status response now reports the live player count and a sample of names
  instead of a hardcoded `online: 0`.
- Disconnect reasons and Server Data now use 26.3 text Components (network NBT)
  rather than JSON strings.
- Tick instrumentation: every 200 ticks an INFO summary reports
  `ticks`, `slowest_tick` and `players`; a tick over 50 ms logs a WARN.

Measured on the development host (debug profile, 200-tick windows):

```
1 player   (join burst)  slowest_tick=3.7ms   players=1
1 player   (steady)      slowest_tick=52-92µs  players=1
2 players  (steady)      slowest_tick=42-92µs  players=2
10 players (steady)      slowest_tick=33-60µs  players=10
```

RSS was **11.3 MB** for one player and **14.5 MB** for ten players
(`tasklist`, debug build). The 2.5-3.7 ms join-window spike is the chunk burst
(synchronous encode and write of the view-distance chunks), not the tick loop;
steady-state ticks stayed well under 0.1 ms in this run.

Join and chunk burst: spawn to Play in ~0.6 s locally; chunks at view distance
2 encoded at roughly 60 KB each before the light-mask fix (the 26.3 mask
encoding removes the full-array ambiguity but keeps 26 sky arrays).

## 6. Orphan code disposition

`ORPHAN_DISPOSITION_LEDGER.csv` now contains per-path, evidence-backed rows.
All candidates were `DELETE_VERIFIED_DEAD`; no unique functionality justified a
port at this stage:

- Deleted **6,727 lines** of never-compiled legacy code: `bin/src/{player, chunk,
  terrain, world, network, core, sdk}/`, `_lib.rs`, `serialization.rs`,
  `error_tracker.rs`, `consts.rs`, `session.rs.bak`, and protocol `v1_19_2.rs`,
  `nbt.rs`, plus pre-existing `play.rs` scratch copies.
- Removed the empty `legacy-test-fixture` feature and the binary's duplicate
  dev-dependency on it.
- No `#[path]`, `include!`, feature gate or compiled module referenced the
  removed tree; the only `rayon` use in the workspace was inside it.
- `nbt.rs` was an encoder-only subset of the active `v26_3::registry` encoder
  and is now covered by golden-byte tests; an NBT decoder is still absent and
  should only be added when an untrusted NBT path exists.

## 7. Verification (executed)

```
cargo fmt --all -- --check                  PASS
cargo check --workspace --all-targets -j 6  PASS (no errors)
cargo test --workspace -j 6                 PASS (126 tests, 0 failed)
cargo clippy --workspace --all-targets -j 6 no errors; one pre-existing
                                            unusual_byte_groupings warning pair in
                                            configuration_packets_26_3
cargo build -p rustbukkit_bin -j 6          PASS
```

Real-client run (official 26.3 client, offline profile): **PASS** for login,
configuration, Join Game, spawn, chunk load/render and continued Play with
presence traffic. Final run: real client plus live peer for ~90 s, then eight
additional synthetic peers for 45 s (10 players total), no client disconnect or
decode error.

## 8. Not verified / limitations

- **Two unmodified clients** simultaneously were not launched (two 2 GB clients
  on this host); presence was validated with the real client plus a synthetic
  peer and with two synthetic clients.
- **Inventory / container wire path is not implemented.** `ClickContainer` is
  still parsed and discarded; no container content/slot/held-item packets are
  sent, so picking up or moving items is not visible to a client.
- **Combat is not on the wire.** Attack packets are not dispatched to
  `SessionRegistry::attack_player`, and health/metadata updates are not sent.
- **Block mechanics remain placeholder**: any breakable block breaks instantly,
  placement always creates grass, no drops, no inventory consumption.
- **Lighting and biomes are still placeholders**: full skylight and a single
  plains biome per section; no light propagation.
- **No benchmark harness / profiling**: the load pass was 8 short-lived
  synthetic peers, not a sustained throughput or p99 test; chunk encode cost,
  GC-free allocation behaviour and network backpressure are unmeasured.
- **Offline mode only**; no Mojang authentication, encryption or compression.
- Root-level scratch files from earlier work (`*.ps1`, `session_*.rs`,
  `play_full.txt`, `temp.hex`, ...) were left untouched as unrelated user files.
- `README.md` still describes older versions and should be rewritten separately.

## 9. Next priorities

1. Inventory wire path: `Set Container Content`, `Set Slot`, `Set Held Item`,
   `Set Player Inventory`, validated `ClickContainer`, then inventory persistence.
2. Combat: dispatch `Attack` to `SessionRegistry::attack_player`, send
   `Set Entity Data` (health) / `Hurt Animation` / `Set Entity Motion` /
   `Player Combat Kill`, and respawn.
3. Entity items: Add Entity for item stacks, pickup/despawn synchronization.
4. Real lighting/biome data instead of full skylight and constant plains.
5. A reproducible benchmark/profile harness (chunk encode, tick p99, per-player
   memory) before any optimization work.
