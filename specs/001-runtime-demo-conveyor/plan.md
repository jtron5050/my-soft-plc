# Implementation Plan: Runtime Binary and SIM Conveyor Demo

**Branch**: `001-runtime-demo-conveyor` | **Date**: 2026-08-28 | **Spec**: [spec.md](./spec.md)

**Input**: Feature specification from `/specs/001-runtime-demo-conveyor/spec.md`

**Note**: This template is filled in by the `/speckit-plan` command; its definition describes the execution workflow.

## Summary

Ship the first **runnable** soft-PLC process (`soft-plc-runtime`) that loads the sample SIM plant config, cycles a checked-in conveyor program in **SIM** (human-reviewable `fixture.spasm` + unsigned `fixture.spkg`, no ST compiler), exposes existing REST + Sparkplug surfaces, and documents development vs production profiles. Glue lives in `crates/plc-runtime`; scan stays a dedicated `std::thread` using `try_lock` around `Runtime::run_due`. See [research.md](./research.md).

## Technical Context

**Language/Version**: Rust 1.85 (`rust-toolchain.toml`)

**Primary Dependencies**: Existing workspace crates (`plc-runtime`, `plc-scan`, `plc-vm`, `plc-ir`, `plc-package`, `plc-api`, `plc-telemetry`, `plc-config`, `plc-io` / `plc-io-sim`, `plc-retain`, `plc-auth`, `plc-fb-primitives`, `plc-types`). Binary: tokio (already workspace). No new third-party framework. No `plc-compiler`.

**Storage**: Filesystem program store (`paths.programs`, pointers `current`/`armed`), retain A/B store (`paths.retain`), audit dir. Lab override via `--data-dir`.

**Testing**: `cargo test --workspace`, clippy `-D warnings`, `scripts/check-rt-deps.sh`. Golden spasm↔spkg; VirtualClock logic oracle; in-process or binary HTTP SIM inject; telemetry catalog + broker-down.

**Target Platform**: Linux x86_64 lab PC / NUC. Best-effort `SCHED_FIFO`; PREEMPT_RT bench is out of scope (PR-19).

**Project Type**: Single-process industrial controller binary + sample config/program artifacts + runbook.

**Performance Goals**: Fast 20 ms / main 50 ms / slow 500 ms cooperative scan. Pull-cord drop-out within one fast period. Start delay 2000 ms ± one main period. Not a jitter certification.

**Constraints**: Constitution I–VIII. RT crates must not gain tokio/network. No WebSocket. SIM-only I/O in this PR. Unsigned demo only under `profile: dev`. Validation failure must not FAULT. One active program per device.

**Scale/Scope**: One conveyor, six BOOL tags, one process, one package. No GPIO/Modbus, no compiler, no PR-20 hardening, no PR-18 metrics polish.

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| Gate | Pre-research | Post-design |
|------|----------------|-------------|
| **I. RT path isolation** | Pass: scan is `std::thread`; tokio only in binary/API/telemetry/flusher; RT uses `try_lock` (never blocks on REST). | Pass: [research R-02/R-03](./research.md), [contracts/management-and-telemetry.md](./contracts/management-and-telemetry.md) thread table. `check-rt-deps.sh` still required. |
| **II. Cyclic scan / modes** | Pass: reuse `ScanEngine` I→L→Q, SIM from STOP, overrun FAULT. | Pass: no new execution model. |
| **III. Verified bytecode, never native user logic** | Pass: checked-in spasm/spkg; no compiler, JIT, `.so`, WASM. | Pass: [contracts/demo-conveyor.md](./contracts/demo-conveyor.md). |
| **IV. Process image, quality, fail-safe** | Pass: sim driver + io-map; RunFwd safe_state false; SIM cannot instantiate field drivers. | Pass: R-11; binary refuses non-sim `io.drivers`. |
| **V. Signed packages / least privilege** | Pass: dev unsigned + optional auth; prod still requires signature/auth at config validate; no WebSocket. | Pass: R-08; runbook documents prod. |
| **VI. Architecture crate boundaries** | Pass: binary in `plc-runtime`, no new crate. | Pass: R-01. |
| **VII. Frozen contracts + fixture tests** | Pass: OpenAPI + Sparkplug reused; spasm golden; logic oracle tests. | Pass: contracts/ + data-model test oracles L1–L7. |
| **VIII. Observability without RT side effects** | Pass: tracing on non-RT only; RT counters/SPSC. | Pass: R-14. T5 flusher off RT (R-12). |

**Gate result**: PASS. No unjustified violations. Complexity Tracking left empty.

Post-design note: optional `IoDriver::inject_input` is a trait extension on a frozen conceptual interface — default method keeps GPIO/Modbus implementors valid when they land. Catalog mapping after activate is already assigned to PR-14 in `docs/sparkplug.md`.

## Project Structure

### Documentation (this feature)

```text
specs/001-runtime-demo-conveyor/
├── plan.md              # This file (/speckit-plan command output)
├── research.md          # Phase 0 output (/speckit-plan command)
├── data-model.md        # Phase 1 output (/speckit-plan command)
├── quickstart.md        # Phase 1 output (/speckit-plan command)
├── contracts/           # Phase 1 output (/speckit-plan command)
│   ├── cli.md
│   ├── demo-conveyor.md
│   └── management-and-telemetry.md
└── tasks.md             # Phase 2 output (/speckit-tasks command - NOT created by /speckit-plan)
```

### Source Code (repository root)

```text
crates/plc-runtime/
├── Cargo.toml                 # add [[bin]] soft-plc-runtime; tokio/plc-api/plc-telemetry/plc-io-sim for bin
├── src/lib.rs                 # supervisor + sim inject helpers (non-RT)
├── src/loader.rs              # existing arm/activate; inject_input / restore current
├── src/bin/soft-plc-runtime.rs
└── tests/
    ├── conveyor_logic.rs      # VirtualClock oracle L1–L7
    ├── pack_demo.rs           # spasm ↔ spkg golden
    └── supervisor_sim.rs      # HTTP + SIM inject (optional in-process)

crates/plc-io/src/driver.rs    # IoDriver::inject_input default
crates/plc-io/src/map.rs       # load IoMap YAML; image-from-map helper
crates/plc-io-sim/src/lib.rs   # implement inject_input
crates/plc-api/src/routes/tags.rs  # %I sim inject on PUT
crates/plc-telemetry/          # no protocol change; supervisor calls TelemetryHandle::set_catalog
crates/plc-retain/             # wire RetainSnapshotBuffer from supervisor/scan

samples/configs/sim-plant.yaml           # existing (dev SIM plant)
samples/configs/sim-plant-io-map.yaml    # new
samples/programs/demo-conveyor/
├── fixture.spasm
└── fixture.spkg

docs/runbook-sim-demo.md
README.md                      # point at binary + runbook
```

**Structure Decision**: Stay in the existing Rust workspace. The deliverable is a bin target on `plc-runtime` plus sample artifacts and a runbook. Tests live beside the crates they exercise (`plc-runtime` for demo/supervisor, `plc-io` for map load, `plc-api` for tag inject). No `src/` at repo root, no new crates.

## Complexity Tracking

> **Fill ONLY if Constitution Check has violations that must be justified**

None.
