# Development Guide

## Prerequisites

- Rust **nightly** with `rustc-codegen-cranelift` (see `rust-toolchain.toml`).
  The workspace dev/release profiles use the Cranelift backend.
- Windows was the development host for all current verification; Linux/macOS are
  expected to work but are untested.
- Optional for the real-client gate: an installed Minecraft Java **26.3** client
  under `%APPDATA%\.minecraft` (asset index 34) and Java 25.

## Build, test, check

All Cargo commands in this repository must use **`-j 6` or fewer**.

```bash
cargo build -p rustbukkit_bin -j 6
cargo test --workspace -j 6
cargo check --workspace --all-targets -j 6
cargo clippy --workspace --all-targets -j 6
cargo fmt --all -- --check
cargo build --release -p rustbukkit_bin -j 6
```

Useful focused runs:

```bash
cargo test -p rustbukkit_protocol -j 6          # protocol golden/unit tests
cargo test -p rustbukkit_bin --test session_e2e -j 6
cargo test -p rustbukkit_bin --test restart_e2e -j 6
cargo test -p rustbukkit_bin --test session_e2e -j 6 presence
```

The binary is `target/debug/rustbukkit.exe` (Windows) / `target/debug/rustbukkit`.

## Running a server

`main` loads `server.conf` **from the current working directory**; `world/` and
`playerdata/` are created there too. Run from a disposable directory, not the
repository root:

```bash
cd target/rb-run
../../target/debug/rustbukkit.exe     # or ../../target/release/rustbukkit.exe
```

The checked-in run directory binds `127.0.0.1:25599`, online mode off, view
distance 2.

### `server.conf`

```toml
mcVersion = "26.3"
bind = "127.0.0.1:25599"
onlineMode = false
maxPlayers = 20
viewDistance = 2
simulationDistance = 2
motd = "RustBukkit"
keepAliveIntervalSeconds = 10
connectionTimeoutSeconds = 30
```

Validation rules (`crates/rustbukkit_config`):

- `onlineMode = true` is rejected (not implemented).
- `maxPlayers` 1..=100000.
- `viewDistance` 2..=32; `simulationDistance` 2..=`viewDistance`.
- `keepAliveIntervalSeconds` 1..=300.
- `connectionTimeoutSeconds` 2..=600 and greater than the keepalive interval.
- `mcVersion` must be exactly `26.3`; other versions are rejected.

Logging uses `tracing`. `RUST_LOG=debug` enables ignored-packet/movement details;
`trace` additionally logs every movement update.

When the process exits gracefully (Ctrl+C), it saves the world and logs
`[SHUTDOWN] world saved`.

## Real-client probe

Assets (disposable, already present on the development host):

```
target/probe/launch-client.sh   # launches installed 26.3 client
target/probe/client-classpath.txt
target/probe/natives/           # extracted LWJGL natives
target/probe/client-run/        # client game dir
```

Run the server on `127.0.0.1:25599`, then:

```bash
target/probe/launch-client.sh 127.0.0.1 25599 RealClient
```

Notes:

- The client takes roughly 30–40 s to boot (datafixer) before connecting.
- `--server`/`--port` are ignored by 26.3; the script uses
  `--quickPlayMultiplayer`. The client still logs the ignored arguments.
- The offline access token triggers `401 InvalidCredentialsException` /
  Realms noise; it is not fatal.
- Kill the client with its timeout (`timeout 110 …`) or close the window. A
  client launched under `timeout` disconnects non-gracefully; the server logs
  `An existing connection was forcibly closed ... (os error 10054)`.
- The client needs ~2 GB heap; do not run two clients plus the official server
  on a small host.

Use the probe only to establish the real-client gate. Synthetic clients
(`target/probe/peer.js`, `TestClient`) are useful for traffic but are **not**
evidence of unmodified-client compatibility.

### Synthetic peer

```bash
target/probe/peer.js 25599 127.0.0.1 PeerAI
```

`peer.js` performs handshake/login/configuration, answers keepalives, and walks
so the real client receives presence and movement. It writes nothing on success.

## Fixtures and provenance

- `registry_data/26.3/configuration_registries.bin` — 32 length-prefixed
  Registry Data payloads from the official server.
- `registry_data/26.3/configuration_tags.bin` — official Update Tags payload.
- `registry_data/26.3/golden_packets.json` — captured login/server-data/add-entity
  payloads (hex) used by golden tests.
- `registry_data/26.3/packet_ids.json` — generated 26.3 packet IDs.
- Regeneration tooling lives in `target/probe/` (`capture.js`, `make-fixtures.js`,
  `dump.js`) and requires the official server jar and its library classpath.

Do not reconstruct registry contents by hand; replay the captured payloads and
keep their provenance documented.

## Troubleshooting

| Symptom | Cause / action |
|---|---|
| `add_entity ... found 5 bytes extra` | Old binary: movement was encoded as three shorts instead of `LpVec3`. Rebuild and restart; the fixed encoder writes one `0x00` byte. |
| `Failed to decode packet 'move_entity_pos_rot'` | Old Move Entity layout. 26.3 needs packed properties + `VecDelta`. Rebuild. |
| `Failed to decode packet ... level_chunk_with_light` | Light mask must be `BitSet.toByteArray()` (VarInt byte length + little-endian trimmed bytes), not long count + longs. |
| `Missing tag ... in 'minecraft:item'` during configuration | Update Tags fixture not sent, or sent before registries. |
| Build fails `failed to remove ... rustbukkit.exe` / `Access is denied` | A server is still running and holds the binary. `taskkill //F //IM rustbukkit.exe`, then rebuild. |
| Client stuck at “Loading Terrain” with a decode error | Verify the server binary timestamp and that you did not start an old release build. |
| Client connects but no terrain | Check `viewDistance`, chunk logs, and that the world seed/spawn is valid; look for encoder errors in the server log. |
| Client disconnects on join | Check the server log for the first decode/validation error; compare packet layouts with `PROTOCOL_26_3.md`. |
| `Connection refused` | Wrong port/directory; the binary binds the address in `server.conf`, not a CLI flag. |

## Working tree and commits

The current work is uncommitted. Do not commit, push, rewrite history or clean
the tree without explicit authorization. Preserve the pre-existing handoff
documents and unrelated user files at the repository root.
