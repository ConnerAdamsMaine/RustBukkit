# Next Steps

Dependency-ordered. Each item names an observable acceptance criterion; do not
mark an item done from compilation alone. The primary gate remains: an
unmodified 26.3 client connects, enters Play, renders terrain, moves and
interacts without protocol errors.

## Priority queue

| ID | Priority | Task | Depends on | Acceptance |
|---|---|---|---|---|
| N1 | P0 | Inventory/container wire path | — | Real client sees inventory contents, can move/split/drop items; server validates `ClickContainer`; state persists and survives reconnect |
| N2 | P0 | Two unmodified clients | N1 not required | Two real clients see each other, move, break/place blocks and stay connected; no ghosts after leave |
| N3 | P1 | Combat and health | N2 | Attack dispatch applies damage, health updates reach owner, hurt animation/motion/knockback and death/respawn reach observers |
| N4 | P1 | Block mechanics | N1 | Break/place honors block type, reach, face, selected item; drops spawn as item entities or enter inventory; no duplication |
| N5 | P1 | Lighting and biomes | — | Real client sees correct biome colors and propagated skylight/block light, not uniform plains/full sky |
| N6 | P2 | Entity metadata/equipment | N3 | Skin parts, sneaking, held item, pose and velocity visible to other clients |
| N7 | P2 | Chunk streaming performance | N2 | Measured chunk encode/send cost, backpressure handling, no tick overrun while flying |
| N8 | P2 | Reproducible benchmark harness | N2 | Scripted N-client load reporting tick p50/p95/p99, RSS, chunk bytes/s, with a checked-in command |
| N9 | P3 | Authentication/encryption/compression | — | Online-mode login verified, or explicitly documented as out of scope |
| N10 | P3 | World save durability | — | Crash/kill between saves loses at most a bounded window (journal or periodic snapshot), tested |

Do **not** start Bukkit/Paper/JVM/plugin work before the vanilla gate.

## N1 — Inventory wire path (highest value)

**Status: implemented unverified with TestClient (2026-11-10).** The network
path and session ownership now exist:

1. ✅ `Inventory` stored per session and persisted in playerdata JSON
   (`inventory` field with a serde default, so legacy saves still load).
2. ✅ Initial `container_set_content` (window 0, state 0, 46 slots) sent on
   join; encoders byte-tested against the official 26.3 codec probe.
3. ✅ `ClickContainer` fully decoded upstream; the server ignores the client's
   changed-slot/carried predictions and computes the result from
   `(slot, button, mode)` via `Inventory::apply_click`. Unimplemented modes
   (`QuickCraft`, `PickupAll`, `Clone`) and craft/armor slots are rejected with
   a full-window resync; state ids bump on every click response.
4. ✅ Authoritative resync after every applied or rejected click; drops become
   item entities (`drop_from_player`); pickup resyncs the window (100 ms
   interval via `registry.pickup_near`).
5. Partially verified: `session_e2e::inventory_clicks_move_items_and_survive_
   reconnect` covers pickup → cursor → hotbar → rejection → reconnect with the
   wire-protocol TestClient. **A real unmodified 26.3 client has not exercised
   this yet**, and item entities have no Add Entity/metadata wire presence
   (invisible drops; N6). `restart_e2e` inventory-persistence extension pending.

Watch for: 26.3 changed container packet IDs; slot index conventions differ
between menu slots and network slots; the crate models 46 slots with 0–8 reserved.

## N2 — Two real clients

The host has limited memory; run one client with a reduced render distance or use
a second machine if possible. Capture both logs and the server presence trace.
Acceptance: both spawn, see each other's avatar, movement and block changes, and
disconnect cleanly. This is the missing proof for Milestone B.

## N3 — Combat

**Status: implemented unverified with TestClient (2026-11-16).** Serverbound
attack dispatch validates target/reach/cooldown in `SessionRegistry::attack_player`
(i-frames, 10-tick cooldown, 0.4/0.4 knockback, vanilla hurt yaw). Victim
receives `set_health`; victim and observers receive `hurt_animation` (43),
`entity_event` (34, hurt=2, death=3) and `set_entity_motion` (103, LpVec3).
Death empties the inventory into item entities; `client_command`
PerformRespawn sends Respawn + health + inventory resync. Dead players neither
move nor pick up items. Water physics for players and item entities now follow
vanilla drag/friction. Remaining: real 26.3 client verification and `Set Entity
Data` health metadata (N6).

Do not trust client attack range/cooldown; enforce server-side.

## N4 — Block mechanics

Replace the current placeholder (instant break, always grass) with block hardness,
tool checks (can be minimal but explicit), correct placed block from held item,
inventory consumption, and drops. Keep `within_reach` and `set_block_if`
compare-and-swap semantics. Broadcast only committed changes.

## N5 — Lighting and biomes

The chunk encoder currently writes full skylight and a single plains biome per
section. Implement light propagation and per-section biome selection from the
terrain generator, or move to real vanilla worldgen data. Keep the light-mask
encoding exactly as documented in `PROTOCOL_26_3.md`.

## N6–N8 — Metadata, performance, benchmarks

- Add/consume entity metadata for player pose, skin parts, held item and health;
  ensure Add Entity + metadata are consistent for late joiners.
- Profile before optimizing: chunk encode, per-tick allocations, broadcast write
  paths, and per-player memory.
- Build a deterministic harness (reuse `peer.js` or `TestClient`) that reports
  p50/p95/p99 tick time, RSS, packet/byte rates and chunk encode time. Record the
  command and environment in the report; never claim speedups without numbers.

## Explicit non-goals this phase

- JNI, embedded JVM, Bukkit/Paper classes, plugin APIs, `.jar` execution.
- Multi-version support (1.19.2 code paths), old packet IDs, or reviving deleted
  legacy trees.
- Copying Mojang code/assets into the repository.

## Working agreements

- Cargo commands: `-j 6` or fewer; release checks at milestones.
- Every protocol change needs a byte-level test or a captured golden fixture.
- Keep the orphan ledger updated if code is removed/ported.
- Update `docs/audits/RUSTBUKKIT_26_3_IMPLEMENTATION_STATUS.md` and this handover
  when status changes; mark unverified work as unverified.
- No commits/pushes without explicit authorization.
