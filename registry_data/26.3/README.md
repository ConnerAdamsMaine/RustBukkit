# Minecraft Java Edition 26.3 protocol data

These files are **derived data**, not Mojang code or game assets. They were
extracted from the official Minecraft 26.3 server jar at
`versions/26.3/server-26.3.jar` (whose `version.json` reports protocol `777`,
world/data version `5023`) by running its data generator:

```bash
cd target/mcreports
CP="server.jar"; while IFS= read -r j; do CP="$CP;$j"; done < <(find ../../libraries -name '*.jar')
java -cp "$CP" net.minecraft.data.Main --reports
# produces generated/reports/{packets,blocks,registries}.json
```

The extracts checked in here are small subsets used as the single source of
truth for RustBukkit's protocol constants:

| File | Contents |
|---|---|
| `packet_ids.json` | Complete 26.3 handshake/status/login/configuration/play packet IDs per direction, keyed by registry name with the `minecraft:` prefix removed. |
| `block_states.json` | Default block-state IDs for the block kinds RustBukkit currently implements. |

`crates/rustbukkit_protocol/tests/protocol_ids_26_3.rs` loads both files and
asserts that the encoder constants and `VersionPack::clientbound_id` mapping
match them, so an accidental ID drift fails the test suite instead of a client.

Known 26.3 changes captured here that older code got wrong:

- `block_update` is **8** (not 9), `keep_alive` **45**, `server_data` **88**,
  `set_health` **106**, `player_info_update` **71**, `remove_entities` **78**,
  `set_entity_data` **101**, `set_entity_motion` **103**, `teleport_entity`
  **128**, `rotate_head` **85**, `set_simulation_distance` **113**.
- There is **no `add_player` packet**: players are spawned with `add_entity`
  (ID 1). `ClientboundPacket::SpawnPlayer` was removed accordingly.
- There is no `entity_velocity` or `combat_event` packet; the equivalents are
  `set_entity_motion` (103) and `player_combat_kill` (69).
- Block states shift (`grass_block` = 9, `dirt` = 10, `cobblestone` = 14,
  `bedrock` = 88, `water` = 89, `sand` = 121), so the old 0..7 compact guess
  would have made clients render the wrong blocks.

Regenerating requires the official server jar and its `libraries/` directory;
both already exist in this repository.
