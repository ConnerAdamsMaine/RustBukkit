# Autonomous Progress Journal

Target: Minecraft Java Edition **26.3** (protocol 777). Base commit `eefa97c`.
Read `docs/handover/README.md` first for entry-point documentation.

## Session log

### 2026-11-10 — Session 1 (this session)

**Recovered state (verified):**

- `cargo test --workspace -j 6` — PASS, 126 tests, 0 failed.
- `cargo clippy --workspace --all-targets -j 6` — PASS (2 pre-existing
  `unusual_byte_groupings` warnings in `configuration_packets_26_3` test).
- Working tree: legacy 1.19.2/orphan modules already deleted (uncommitted);
  see `ORPHAN_DISPOSITION_LEDGER.csv`.
- Milestone A (real client joins/renders): previously PASS per
  `docs/handover/CURRENT_STATE.md`.

**Changed:**

- `crates/rustbukkit_bin/tests/restart_e2e.rs`: removed sleep-based
  synchronization. Placement is proven by waiting for the authoritative Block
  Update sent to the actor; movement is proven by waiting for the Set Chunk
  Cache Center packet emitted when the move crosses a chunk boundary. Fixes a
  flaky failure under full-workspace load where the last movement was not
  processed before the abrupt disconnect (position restored as spawn).

**Verified:**

- PASS — `cargo test -p rustbukkit_bin --test restart_e2e` × 6 consecutive runs.
- PASS — full workspace test suite after fix; `cargo fmt --all` applied.

**Next priorities (from `docs/handover/NEXT_STEPS.md`):**

1. N1 — Inventory/container wire path (per-session `Inventory`, initial sync on
   join, full `ClickContainer` decode + authoritative apply, persist inventory).
2. N3 — Combat and health (attack dispatch, health metadata, death/respawn).
3. N4 — Block mechanics (held item → placed block, inventory consumption, drops).

### 2026-11-10 — Session 2 (this session)

**Baseline (verified):** `cargo test --workspace -j 6` PASS (126 tests) before
new work; workspace clean of regressions throughout.

**Changed:**

- `crates/rustbukkit_inventory/src/lib.rs`: fixed the four `apply_click`
  compile errors from session 1 (cursor-drop rejection mapping, `InventorySlot`
  construction in the Throw branch, cursor/slot swap binding, `quick_move`
  stack binding) and rewrote the five new click tests against the real
  `Option<ItemStack>` API with corrected vanilla semantics (right-click takes
  the larger half, merges fill to 64). Added `Inventory::window()` (46 slots).
  14/14 crate tests pass.
- `crates/rustbukkit_bin/src/player_location.rs`: `StoredPlayer` now persists an
  optional `inventory` (serde default = empty, so legacy saves load); `load`
  returns `PlayerSnapshot { location, inventory }`; `save` takes the inventory.
- `crates/rustbukkit_bin/src/session.rs`: per-session `Inventory` carried
  through `play()`; initial `container_set_content` (window 0, state 0, 46
  slots) sent on join; `ClickContainer` handled authoritatively — server
  applies the click, spawns `dropped` stacks via `drop_from_player` (re-insert
  if entity allocation fails so nothing is silently deleted) and sends a
  full-window resync with a bumped state id on every click (applied or
  rejected); `HeldSlot` also syncs `inventory.select`; a 100 ms pickup interval
  calls `registry.pickup_near` and resyncs on change; saves carry the inventory.
- `crates/rustbukkit_bin/src/session.rs` (N4 starter): `break_block` now
  spawns block drops via `registry.drop_from_block`; **Water is no longer
  breakable** (it previously could be broken and destroyed with no drop).
- `crates/rustbukkit_bin/src/test_client.rs`: `container_click` sender
  (serverbound ID 18, empty predictions) and `wait_for_container_content`
  decoder for `container_set_content` (ID 18) with `ContainerContent`.
- `crates/rustbukkit_bin/tests/session_e2e.rs`: new
  `inventory_clicks_move_items_and_survive_reconnect` — place/dig a grass
  stack near spawn (water sits at the spawn column and drops sink through it),
  assert pickup lands dirt in the window, left-click to cursor, place into
  hotbar slot 36, rejected craft-region click leaves contents unchanged, and
  the inventory survives disconnect/reconnect (bounded join retry because the
  old session releases its registration asynchronously).
- `crates/rustbukkit_protocol/tests/configuration_packets_26_3.rs`: fixed the
  two pre-existing `unusual_byte_groupings` clippy lints (0x0123_4567).
- `cargo fmt --all` applied (session-1 files were not fmt-clean).

**Verified:**

- PASS — `cargo test --workspace -j 6` (142 tests, 0 failed).
- PASS — `cargo clippy --workspace --all-targets -j 6 -- -D warnings`.
- PASS — `cargo fmt --all -- --check`.

**Not verified / limitations:**

- No real unmodified 26.3 client has exercised the inventory path yet
  (TestClient only); item entities have no Add Entity/metadata wire presence,
  so drops are invisible to real clients until N6 — pickup still works.
- `QuickCraft`, `PickupAll`, `Clone` clicks are still rejected (safe resync).
- Per-slot `container_set_slot` resync optimization deferred (full-window
  resync after every click, correctness first).

**Next priorities:**

1. N3 — combat/health: wire `registry.attack_player` into `PlayInbound::Attack`,
   health/respawn packets, death handling.
2. N4 — remaining block mechanics: held item selection affects placement and
   consumption (place consumes from inventory, no duplication).
3. N5 — lighting/biomes per backlog.

### 2026-11-16 — Session 3 (this session)

**Baseline (verified):** workspace compiled; combat/health (N3) pending. Two
delegated child-agent attempts were interrupted mid-write and one **zeroed
`crates/rustbukkit_bin/src/session.rs` with NUL bytes**; the file was
reconstructed from git HEAD plus the re-derived inventory/presence deltas, then
the partially-completed combat work (registry `CombatEvent` broadcasts, attack
dispatch, hurt/death/respawn clientbound path, `VersionPack` payload methods)
was recovered from the child thread's remaining writes and reconciled by hand.

**Changed:**

- `crates/rustbukkit_protocol/src/v26_3/clientbound.rs`: 26.3 combat encoders —
  `set_health` (106), `hurt_animation` (43), `entity_event` (34, hurt=2/death=3),
  `set_entity_motion` (103) with the vanilla `Vec3.LP_STREAM_CODEC` (LpVec3)
  adapted from HorizonX `horizon-protocol/src/buf.rs`; byte-level golden tests.
- `crates/rustbukkit_protocol/src/{version.rs, v26_3/protocol.rs}`:
  `ClientboundPacket::{HurtAnimation, EntityEvent}`, trait payload methods
  (`update_health_payload`, `hurt_animation_payload`, `entity_event_payload`,
  `entity_velocity_payload`, `respawn_payload`) wired into the V26_3 pack.
- `crates/rustbukkit_bin/src/session_registry.rs`: `CombatEvent::{Hurt,Death}`
  broadcast channel (`subscribe_combat`), vanilla hurt-yaw (`indicateDamage`
  semantics) and 0.4/0.4 knockback in `attack_player`, dead-player respawn path;
  **vanilla physics fixes** — players now get water drag (slow sink) instead of
  falling through the ocean at air terminal velocity, and item entities gained
  water physics plus ground friction (resting items no longer slide forever and
  off ledges).
- `crates/rustbukkit_bin/src/session.rs`: `PlayInbound::Attack` dispatch,
  combat-event fan-out (victim set_health + hurt feedback, observers hurt
  feedback), death handling (EntityEvent 3, inventory emptied into item
  entities), client-driven respawn (Respawn packet + health + inventory resync),
  dead players do not move and do not pick up items.
- `crates/rustbukkit_bin/src/test_client.rs`: background reader task keeps the
  socket drained while tests idle (server chunk writes no longer stall
  multi-second); `Drop` aborts the reader so the socket closes on drop; added
  `attack`, `respawn_request`, `drain_for` helpers.
- `crates/rustbukkit_bin/tests/combat_e2e.rs` (new): attack validation
  (unknown target / out of range rejected), damage + hurt animation + entity
  event + LpVec3 knockback on the wire, fight-to-death with inventory drop and
  respawn restoring health and re-pickup. Tests build a solid platform because
  the flat world is a water ocean above bedrock.
- Versions bumped to 0.2.0 (`rustbukkit_bin`, `rustbukkit_inventory`,
  `rustbukkit_protocol`, workspace metadata).

**Verified:**

- PASS — `cargo test --workspace -j 6` (149 tests, 0 failed; 3 runs).
- PASS — `cargo clippy --workspace --all-targets -j 6 -- -D warnings`.
- PASS — `cargo fmt --all -- --check`.
- NOT RUN — unmodified 26.3 client combat session (no client available in this
  environment; the TestClient harness is not a real-client pass).

**Next priorities:**

1. N4 — block mechanics: held-item-dependent placement/drops, inventory
   consumption.
2. Real-client combat gate when a 26.3 client is available.
3. N6 — entity metadata so item drops become visible on the wire.

## Resumption instructions

1. Read root `AGENTS.md`, `docs/handover/README.md`, `CURRENT_STATE.md`,
   `NEXT_STEPS.md`, then the newest session entry above.
2. `cargo test --workspace -j 6` must pass before starting new work.
3. No commits without explicit operator authorization.
