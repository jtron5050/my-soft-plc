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
| [`plc-io`](crates/plc-io) | Process image, quality, IoDriver trait, double-buffer, force priority |
| [`plc-io-sim`](crates/plc-io-sim) | Simulation I/O driver |
| [`plc-ir`](crates/plc-ir) | IR v0.1 types, `spbc` framing, verifier, `spasm` assembler |
| [`plc-fb-primitives`](crates/plc-fb-primitives) | Native FBs: TON/TOF/TP, CTU/CTD, RS/SR, edges, PID |

Further crates (`plc-scan`, `plc-vm`, …) land in later PRs per the architecture plan.

Sample config: [`samples/configs/sim-plant.yaml`](samples/configs/sim-plant.yaml).

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

PR-01–PR-05 are in place (workspace through native FB primitives). Next: IR VM (PR-06) per the architecture plan.
