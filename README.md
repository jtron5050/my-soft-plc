# Soft PLC

Greenfield soft PLC runtime for heavy materials / bulk materials handling plants.

**License:** [Apache-2.0](LICENSE)

## Documentation

- **[Architecture design](docs/architecture.md)** — system design (Rev 2.2): language choice, scan engine, IR/hot-swap, I/O, REST, MQTT Sparkplug, PR plan

## Workspace

Rust monorepo (`crates/*`). Current crates:

| Crate | Role |
|-------|------|
| [`plc-types`](crates/plc-types) | Shared types, modes, quality plane enums, errors |
| [`plc-config`](crates/plc-config) | Versioned device config schema, YAML/JSON load, validation |
| [`plc-auth`](crates/plc-auth) | Roles, bearer/mTLS identity, lockout, rate limit, permission checks |
| [`plc-io`](crates/plc-io) | Process image, quality, IoDriver trait, double-buffer, force priority |
| [`plc-io-sim`](crates/plc-io-sim) | Simulation I/O driver |
| [`plc-io-modbus`](crates/plc-io-modbus) | Modbus TCP poll worker (non-RT) |
| [`plc-ir`](crates/plc-ir) | IR v0.1 types, `spbc` framing, verifier, `spasm` assembler |
| [`plc-fb-primitives`](crates/plc-fb-primitives) | Native FBs: TON/TOF/TP, CTU/CTD, RS/SR, edges, PID |
| [`plc-vm`](crates/plc-vm) | IR v0.1 interpreter (no alloc in run loop) |
| [`plc-scan`](crates/plc-scan) | Cooperative scan scheduler, modes, software watchdog, TelemetrySource |
| [`plc-retain`](crates/plc-retain) | Symbolic retain map, A/B NV store, T5 flush API |
| [`plc-package`](crates/plc-package) | `.spkg` v1 container, JSON/JCS manifest, Ed25519 verify |
| [`plc-compiler`](crates/plc-compiler) | Host ST-subset (Appendix B) → IR → `.spkg` |
| [`plc-runtime`](crates/plc-runtime) | Dual-buffer load, epoch activate, hot-swap glue, scan thread |
| [`plc-api`](crates/plc-api) | REST config/status API (axum + tokio), OpenAPI at [`docs/openapi/openapi.yaml`](docs/openapi/openapi.yaml) |
| [`plc-telemetry`](crates/plc-telemetry) | MQTT 5 Sparkplug B 3.0 publisher (non-RT) |
| [`soft-plc-runtime`](crates/soft-plc-runtime) | Process binary: scan + REST + MQTT + retain flush |

Sparkplug contract: [`docs/sparkplug.md`](docs/sparkplug.md).

Sample programs (text-reviewable `fixture.spasm` and ST under `src/`): [`samples/programs/`](samples/programs/). Demo conveyor: [`samples/programs/demo-conveyor/`](samples/programs/demo-conveyor/). ST libraries: [`libs/materials-common/`](libs/materials-common/).

Sample config: [`samples/configs/sim-plant.yaml`](samples/configs/sim-plant.yaml). Runbook: [`docs/runbook.md`](docs/runbook.md).

```bash
# Compile ST → .spkg (PR-15)
cargo run -p plc-compiler -- compile samples/programs/demo-conveyor/project.toml \
  -o /tmp/demo-conveyor.spkg

cargo run -p soft-plc-runtime -- \
  --config samples/configs/sim-plant.yaml \
  --data-dir /tmp/soft-plc \
  --program samples/programs/demo-conveyor/fixture.spkg \
  --mode SIM
```

## Development

Requirements: Rust **1.85** (see `rust-toolchain.toml`).

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
bash scripts/check-rt-deps.sh   # RT path must not pull tokio / network crates
```

CI runs the same checks on every push and pull request.

## Status

PR-01–PR-14 are in place (workspace through the `soft-plc-runtime` binary and SIM demo conveyor).
PR-15 adds the host `plc-compiler` (Appendix B ST-subset → `.spkg`).
PR-16 adds the Modbus TCP poll worker (`plc-io-modbus`). GPIO remains PR-17.
