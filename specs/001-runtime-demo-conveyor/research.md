# Research: Runtime Binary and SIM Conveyor Demo

**Feature**: `001-runtime-demo-conveyor` (architecture PR-14)  
**Date**: 2026-08-28

All Technical Context unknowns are resolved below. No `[NEEDS CLARIFICATION]` remain.

## R-01 — Single process layout (binary crate)

- **Decision**: Add `[[bin]] name = "soft-plc-runtime"` in existing `crates/plc-runtime` (`src/bin/soft-plc-runtime.rs`). Do **not** add a new workspace crate.
- **Rationale**: Architecture PR-14 lists `crates/plc-runtime` as the component; constitution Principle VI forbids organizational-only crates. `plc-runtime` is already non-RT glue (not on the RT-dep denylist). Tokio may be a **binary/optional** dependency of `plc-runtime` without pulling tokio into `plc-scan` / `plc-vm`.
- **Alternatives considered**: New `soft-plc-runtime` crate (contradicts architecture layout); binary in `plc-api` (wrong owner — API is the HTTP surface, not the supervisor).

## R-02 — Thread model (T0–T5)

- **Decision**: Supervisor is a tokio multi-thread runtime (T0). Scan loop is a dedicated `std::thread` named `plc-scan` (T1), **not** a tokio task. REST (`plc_api::serve`) and MQTT (`TelemetryService::run`) are tokio tasks (T3/T4). Retain flusher is a tokio task (T5) that only does encode + `fsync`. Remote I/O workers (T2) are unused in this feature (sim driver only).
- **Rationale**: Constitution I / KD-13: RT scan must not be a tokio task and must not depend on network crates. Architecture process diagram already names these threads.
- **Alternatives considered**: Entire process as one tokio runtime with `spawn_blocking` for scan (easy to accidentally `.await` on RT); two OS processes (rejected for v1, KD-10).

## R-03 — Sharing `Runtime` between REST and scan

- **Decision**: Keep `AppState.runtime: Arc<Mutex<Runtime>>`. The scan thread calls `Mutex::try_lock()` around `Runtime::run_due()` and **skips the cycle** if the lock is held. REST handlers already drop the lock before `.await`. Arm validation stays unlocked (`prepare_arm`).
- **Rationale**: Smallest change that satisfies “RT MUST NOT wait on a mutex a non-RT thread can take.” Contended skip of one 20 ms tick is acceptable for SIM lab. A lock-free command SPSC can wait for a later PR if soak shows contention.
- **Alternatives considered**: Scan-thread-owned engine + mpsc commands (cleaner isolation, larger rewrite of every `plc-api` handler); blocking `lock()` on RT (constitution violation).

## R-04 — Demo artifacts (no compiler)

- **Decision**: Check in `samples/programs/demo-conveyor/fixture.spasm` (source of truth) and `samples/programs/demo-conveyor/fixture.spkg` (unsigned `.spkg` v1 from `plc_ir::assemble` + `PackageBuilder::unsigned`). A workspace test reassembles the spasm and asserts byte-equivalence (or structural equivalence of parsed package) with the checked-in `.spkg`. Packing uses existing `plc-package` APIs; **never** `plc-compiler`.
- **Rationale**: Architecture PR-14: fixtures, not ST compiler. Principle III/VII: text-reviewable IR; PR-15 later must round-trip to these fixtures.
- **Alternatives considered**: Generate `.spkg` only in CI (breaks “checked-in package” and offline lab); ship ST sources now (depends on PR-15).

## R-05 — Conveyor program semantics

- **Decision**: One conveyor `Conveyor1`. Fast task (20 ms) drop-out: if `PullCordOK` is false, de-energize `RunFwd` immediately. Main task (50 ms): reset-dominant run latch (RS), 2000 ms TON start delay, chute-blocked start inhibit, stop command. Slow task (500 ms): `HALT` only (no extra logic). Primitive FBs: `RS`, `TON` (optional `R_TRIG` on Start). `retain_size = 0`.
- **Rationale**: Spec FR-005–FR-009 and materials-plant interlock mental model. Native primitives already exist (PR-05). Zero retain keeps T5 a no-op path while still wiring the flusher for non-empty future programs.
- **Alternatives considered**: Main-task-only demo (fails 20 ms drop-out); ST-style libraries in `libs/materials-common` (PR-15).

## R-06 — Tag names and I/O map

- **Decision**: Sparkplug-style slash names (already used in `plc-api` tests: `Conveyor1/RunFwd`), not dots. Bindings live in new `samples/configs/sim-plant-io-map.yaml` (path already referenced by `sim-plant.yaml`). Image sized from the map; slots assigned in file order.
- **Rationale**: Spec allows “equivalent documented names.” Telemetry contract uses `/`. Existing REST tag test already uses `Conveyor1/RunFwd`.
- **Alternatives considered**: Dotted IEC paths (`Conveyor1.Start`) — would fork telemetry names; anonymous `I0`/`Q0` — fails named-tag FRs.

## R-07 — Injecting simulated commands

- **Decision**: Extend `IoDriver` with an optional `inject_input(slot, value)` (default: unsupported). `SimDriver` implements it. `PUT /api/v1/tags/{name}` on a `%I` tag in **SIM** (or whenever the live driver is sim) writes the sim input so the next Input phase observes it. `%Q` PUT remains the maintenance force overlay. `%I` inject is not a force overlay and does not set `forced=true` on telemetry.
- **Rationale**: Force table is `%Q`-only today; sim `poll_inputs` would overwrite a process-image-only write. Lab runbook must assert Start / drop PullCord without field hardware (spec US2).
- **Alternatives considered**: Commands as `%M` (not in I/O map; extra write API); new `/sim/inject` resource (extra OpenAPI surface); forcing `%Q` RunFwd to fake a start (bypasses logic).

## R-08 — Development vs production profile

- **Decision**: Sample `sim-plant.yaml` stays `profile: dev`, `auth.required: false`, `program.require_signature: false`, `rest.bind: 127.0.0.1:8443`, empty TLS paths → plaintext HTTP (already allowed by `plc_api::listen_mode` in dev). Binary does not implement PR-20 extra refusals. Runbook documents prod: signatures, auth, TLS; demo is lab/SIM only.
- **Rationale**: Spec FR-010/FR-020; existing config validation already rejects prod without signature/auth.
- **Alternatives considered**: Default the binary to prod (breaks unsigned demo); refuse plaintext even in dev (hurts 15-minute lab).

## R-09 — Boot, current program, data dirs

- **Decision**: CLI: `soft-plc-runtime --config <path> [--data-dir <dir>]`. `--data-dir` remaps `paths.programs` / `paths.retain` / `paths.audit` to subdirectories so lab users need not write `/var/lib/soft-plc`. On start: load config → build sim `ScanIo` from io-map → `Runtime::new` (STOP/idle) → if program-store pointer `current` exists, load bytes, `upload` (arm) + `activate` and wait until phase idle or timeout, still leaving **mode STOP** until the operator requests SIM. Missing current is not an error.
- **Rationale**: Spec FR-017/FR-018; architecture program store pointers; 15-minute lab without root.
- **Alternatives considered**: Auto-enter SIM (surprising; SIM is an operator mode); auto-seed fixture.spkg always (hides the install path the runbook must teach).

## R-10 — Telemetry catalog after activate

- **Decision**: After a successful epoch install (scan-thread install CS, observed when `program.phase` returns idle with a new current), map `TagEntry` `%I`/`%Q` + io-map `unit` into `TagCatalog` (`/` names, aliases 1..=N lexicographic) and call `TelemetryHandle::set_catalog`. Clone the handle before `TelemetryService::run`. Broker down: `run` already no-ops drain until CONNACK; scan SPSC drops oldest (`telemetry_drops`). Binary still starts if broker is down.
- **Rationale**: `docs/sparkplug.md` explicitly assigns this mapping to PR-14. Spec FR-014–FR-016 / SC-005.
- **Alternatives considered**: Leave `I{n}`/`Q{n}` fallback catalog (fails named conveyor metrics); block start on broker (fails “scan continues”).

## R-11 — Driver set for this feature

- **Decision**: Binary instantiates **sim only**. If `io.drivers` contains anything other than `sim`, start fails with a clear error (GPIO/Modbus are PR-16/PR-17). SIM mode therefore cannot write field outputs because no field driver is loaded.
- **Rationale**: Spec FR-012; those crates are not in the workspace yet.
- **Alternatives considered**: Silently ignore extra drivers (misleads operators); stub gpio/modbus (out of scope).

## R-12 — Retain flusher wiring

- **Decision**: On arm/activate, size a `RetainSnapshotBuffer` to the current retain image. After Logic, if `retain_dirty`, RT `publish`s a bounded memcpy. T5 waits on `RetainDirtyWatch` (poll 10–50 ms), `read`s the snapshot, `RetainStore::flush`. Demo retain size 0: buffer may be skipped; T5 still runs and no-ops. Graceful shutdown: one extra flush.
- **Rationale**: Architecture T5; Principle I (no NV I/O on RT). Buffer type already exists in `plc-retain` but is unwired.
- **Alternatives considered**: Flush from scan thread (forbidden); skip T5 entirely because demo has no retain (leaves binary incomplete vs FR-002).

## R-13 — Scheduling / RT niceness

- **Decision**: Best-effort `SCHED_FIFO` + optional `scan.cpu_affinity` from config. Permission errors are logged and ignored (lab without `CAP_SYS_NICE` still runs). No PREEMPT_RT jitter gate in this feature (PR-19).
- **Rationale**: Architecture KD-8/KD-22; spec assumes lab PC, not isolated cores.
- **Alternatives considered**: Hard-fail without FIFO (blocks desktop demo).

## R-14 — Observability in the binary

- **Decision**: Non-RT `tracing` subscriber (stderr / journald-friendly). RT path remains counters/atomics only. Existing `/metrics` and diagnostics routes stay as-is (full Prometheus/ring polish is PR-18).
- **Rationale**: Principle VIII.
- **Alternatives considered**: Log from the scan thread (forbidden).

## R-15 — Tests and runbook

- **Decision**: (1) Unit/golden: spasm → spkg equivalence; conveyor logic with `VirtualClock` (start delay, pull-cord drop within one fast period, chute inhibit). (2) Integration: spawn binary or in-process supervisor, HTTP SIM + tag inject, assert `RunFwd`. (3) Telemetry: catalog names after activate; scan continues with broker down. Runbook file: `docs/runbook-sim-demo.md` (also summarized in `quickstart.md`).
- **Rationale**: Spec SC-001–SC-007; constitution VII.
- **Alternatives considered**: Manual-only demo (not CI-gated).
