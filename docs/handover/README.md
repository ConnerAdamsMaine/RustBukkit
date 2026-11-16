# RustBukkit 26.3 — Handover

**Snapshot:** 2026-11-10, base commit `eefa97c`, all implementation work uncommitted
in the working tree.
**Target:** Minecraft Java Edition **26.3**, protocol **777**, offline mode only.

This directory is the entry point for the next engineer/agent. It describes the
code as it exists now, what has actually been verified, and what to do next. If a
document disagrees with code or tests, **the code and tests win** — fix the doc.

## Status at a glance

| Gate | Status | Evidence |
|---|---|---|
| Milestone A — unmodified 26.3 client joins, enters Play, renders terrain | **PASS** | Official client (`RealClient`) connected to the rebuilt server, `RealClient joined entity=…`, chunk-section renderer activity, no decode errors; see `CURRENT_STATE.md` |
| Milestone B — shared persistent world, two clients see each other | **PARTIAL** | Live presence (Player Info / Add Entity / Move / Remove) verified with a real client plus a synthetic peer and two-`TestClient` tests; **two unmodified clients not launched**; blocks and player position survive restarts; items/inventory do not |
| Milestone C — core survival | **NOT STARTED** | Inventory/combat not on the wire; see `NEXT_STEPS.md` |
| Milestone D — broad parity | **NOT STARTED** | Placeholder lighting/biomes and block mechanics |

## Read in this order

1. **`CURRENT_STATE.md`** — what works, what does not, exact verification.
2. **`ARCHITECTURE.md`** — crates, session flow, world model, invariants.
3. **`DEVELOPMENT.md`** — build/run/test, real-client probe, troubleshooting.
4. **`PROTOCOL_26_3.md`** — verified 26.3 wire facts and their provenance.
5. **`NEXT_STEPS.md`** — dependency-ordered backlog and acceptance criteria.

Supporting material:

- `docs/audits/RUSTBUKKIT_26_3_IMPLEMENTATION_STATUS.md` — detailed evidence
  report for the last implementation cycle (packet fixes, fixtures, benchmark
  numbers, orphan deletions).
- `docs/audits/RUSTBUKKIT_26_3_STATUS_AUDIT.md` — original read-only audit; keep
  as historical context, not as current truth.
- `ORPHAN_DISPOSITION_LEDGER.csv` — per-path disposition of removed legacy code.
- `03_IMPLEMENTATION_ROADMAP.md`, `04_ORPHAN_CODE_TRIAGE.md`,
  `01_AGENT_EXECUTION_PROMPT.md` — the assignment and its rules.
- `registry_data/26.3/README.md` — provenance of captured 26.3 fixtures.

> **Warning:** the repository-root `README.md` is stale. It describes a 1.21.7
> protocol, a removed `src/` tree, a cache that does not exist, and says registry
> data is not sent. Do not use it for current behavior. `docs/chunk_perf.md` is
> historical legacy notes, not a current benchmark. `Protocols/` is excluded from
> the Cargo workspace (Azalea reference code) and is not built.

## Fast start

```bash
# Toolchain: nightly + rustc-codegen-cranelift (rust-toolchain.toml)
cargo test --workspace -j 6
cargo build -p rustbukkit_bin -j 6

cd target/rb-run          # the directory that holds server.conf
../../target/debug/rustbukkit.exe
# server listens on 127.0.0.1:25599 with the checked-in run config
```

The binary reads `server.conf` **relative to the current working directory** and
stores `world/` and `playerdata/` under it.

## Working tree state

- Protocol, session, chunk, registry fixture, presence, persistence and test
  changes are **uncommitted**; no commit was authorized.
- Pre-existing untracked documents (`01_…`, `03_…`, `04_…`, ledger,
  `docs/audits/`) belong to the earlier handoff; leave them in place.
- `target/probe/` contains disposable probe tooling and captured logs; it is
  ignored, not part of the product.
- Both `target/debug` and `target/release` binaries were rebuilt after the last
  protocol fix. If you run an older release binary, the Add Entity decode error
  reappears; see `DEVELOPMENT.md` → Troubleshooting.
