# Protocol Notes — Minecraft 26.3 (protocol 777)

Everything here was verified against the official 26.3 server rather than assumed
from older versions. Sources:

1. Bytecode of `versions/26.3/server-26.3.jar` (`javap -c`) — authoritative for
   reader/writer order.
2. Captured packets from an official server (`registry_data/26.3/golden_packets.json`,
   `configuration_registries.bin`, `configuration_tags.bin`).
3. The official client’s reaction (decode errors are excellent oracles).
4. `registry_data/26.3/packet_ids.json` and `tests/protocol_ids_26_3.rs`.

Never copy 1.19.2 or 1.21.x IDs/layouts without re-verifying against 26.3.

## Configuration phase

Order used by RustBukkit (matches vanilla):

| Step | Packet ID | Payload |
|---|---|---|
| Brand | 1 (`custom_payload`) | `minecraft:brand` + `RustBukkit` |
| Feature flags | 13 | one flag: `minecraft:vanilla` |
| Select known packs | 15 | `minecraft` / `core` / `26.3` |
| After client known-packs reply | 7 (`registry_data`) × 32 | captured vanilla registries |
| | 14 (`update_tags`) | captured vanilla tags (58,754 bytes) |
| | 3 (`finish_configuration`) | empty |
| Client → server finish | 3 | ends configuration |

Serverbound configuration IDs used: client information 0, custom payload 2,
finish 3, keepalive 4, pong 5, select known packs 7, accept code of conduct 9.

The client aborts with `Missing tag: 'minecraft:sulfur_cube_archetype/slow_sliding'
in 'minecraft:item'` if registry data is sent without Update Tags. Registries must
arrive after the client’s known-pack selection.

## Play packet IDs (verified subset)

| Packet | ID |
|---|---|
| Add Entity | 1 |
| Block Update | 8 |
| Forget Chunk | 38 |
| Level Chunk With Light | 46 |
| Login (Join Game) | 50 |
| Move Entity Pos/Rot | 55 |
| Player Position | 73 |
| Respawn | 84 |
| Set Chunk Cache Center | 96 |
| Spawn Position | 99 |
| Update Time | 115 |
| Player entity type (`minecraft:player`) | 120 |

`packet_ids.json` is the full generated list. Assertions live in
`tests/protocol_ids_26_3.rs`; update both together if a 26.3.x revision changes
IDs.

## Verified layouts

### Login Success

```
UUID                 16 bytes
username             string (VarInt length + UTF-8)
properties           VarInt count, then per property: name, value, bool signed, optional signature
session UUID         16 bytes
```

Captured official payload for `Probe` is 39 bytes:
`… 05 "Probe" 00 <session uuid>`. Omitting the property count (the old bug)
produces 37 bytes and a decode failure. Golden test:
`golden_payloads_26_3::login_success_is_byte_identical_to_the_official_26_3_server`.

### Server Data

```
MOTD     network-NBT text Component (TAG_String root: 08 00 len "text")
icon     Optional<byte[]> (bool + VarInt length + PNG bytes)
```

Captured: `08 00 0c "VanillaProbe" 00`. Do not send JSON strings or booleans.
Golden test covers the absent-icon form; extend it for a present icon before
shipping icons.

### Add Entity (player)

```
entity id            VarInt
UUID                 16 bytes
entity type          VarInt (120 for player)
x, y, z              f64 × 3
movement             Vec3 LP_STREAM_CODEC (`LpVec3`)
xRot, yRot, headRot  i8 × 3
data                 VarInt
```

**Movement encoding is the trap.** `net.minecraft.network.LpVec3.read` first
reads one unsigned byte; `0` means `Vec3.ZERO` and consumes nothing else.
Otherwise it reads a scale byte and a 4-byte quantized value. Writing three zero
shorts (the old code) leaves exactly **5 extra bytes**, which is the client error
`found 5 bytes extra whilst reading packet clientbound/minecraft:add_entity`.
RustBukkit currently always sends zero velocity, so the field is the single byte
`0x00`. Total packet for a player: 47 bytes (with a 1-byte entity id).

Golden test:
`clientbound::tests::player_info_and_add_entity_match_the_captured_26_3_layout`.

### Move Entity Pos/Rot

```
entity id     VarInt
properties    VarInt = (onGround ? 1 : 0) | (stepCount << 1)
delta         VecDelta.write
yRot          i8
xRot          i8
```

`VecDelta.write` with step count 0 writes three **big-endian i16** fixed-point
deltas (1/4096 block). A non-zero step count writes a stepped path instead;
RustBukkit always uses the linear form. The old layout
`entityId, shorts, yaw, pitch, bool onGround` is invalid and produces
`Failed to decode packet 'clientbound/minecraft:move_entity_pos_rot'`.

### Level Chunk With Light

The 26.3 light mask is **`ByteBufCodecs.BIT_SET`**:
`VarInt byte-length` followed by that many little-endian bytes from
`BitSet.toByteArray()`. It is **not** a VarInt long-count followed by longs.
Sky light uses 26 arrays of 2048 bytes; block light uses 26 empty arrays for
full skylight. Chunk sections are single-valued palettes (air / stone etc.) with
plains biome id **41**.

Golden tests: `chunk_packets_26_3` and
`v26_3::chunk::tests::light_masks_use_the_26_3_bitset_byte_array_encoding`.

### Join Game identifiers

- Dimensions: `minecraft:overworld`, `minecraft:the_end`, `minecraft:the_nether`.
- `minecraft:dimension_type` overworld network id **0**.
- `minecraft:world_clock` overworld id **0**.
- `minecraft:plains` biome id **41** (0 is badlands).
- Sea level 100 is reported by the server’s own config, not a vanilla constant.

These are asserted by `golden_payloads_26_3::configuration_registry_snapshot_locks_biome_dimension_and_clock_ids`
and `configuration_packets_26_3`.

### Player Info Update

Starts with an `EnumSet` action mask byte. `0x09` = ADD_PLAYER | UPDATE_LISTED.
Per entry: UUID, name, property count/list, listed bool. A later metadata/listed
change needs the corresponding action bits; do not reuse `0x09` blindly.

## Other 26.3 facts worth remembering

- There is no `add_player` packet: players are spawned with Add Entity (id 1).
- There is no `entity_velocity`/`combat_event`; equivalents are
  `set_entity_motion` (103) and the damage/health packets.
- `set_entity_data` is id 101, `teleport_entity` 128, `rotate_head` 85,
  `set_simulation_distance` 113.
- Status response is JSON with `version.name = "26.3"`, `protocol = 777`,
  `players.online/max/sample`, and a Component-derived description.
- Compression and encryption are not implemented; configuration/play frames are
  uncompressed.

## Fixture regeneration

Captured fixtures are derived protocol data, not Mojang code. `target/probe/`
contains the capture/analysis scripts:

- `capture.js`, `dump.js` — connect to an official server and dump packets.
- `make-fixtures.js`, `analyze.js` — turn captures into the checked-in binaries
  and `golden_packets.json`.
- `chunks-official.json` / `chunks-rust.json` — chunk comparison data.

Record the source version and date in `registry_data/26.3/README.md` when
regenerating. Do not check in the official jar or its libraries.
