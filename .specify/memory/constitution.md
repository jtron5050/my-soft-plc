<!--
Sync Impact Report
==================
Version change: unratified template → 1.0.0
Modified principles:
  - [PRINCIPLE_1_NAME] → I. Real-Time Path Isolation (NON-NEGOTIABLE)
  - [PRINCIPLE_2_NAME] → II. Cyclic Scan and Cooperative Execution
  - [PRINCIPLE_3_NAME] → III. Verified Bytecode, Never Native User Logic
  - [PRINCIPLE_4_NAME] → IV. Process Image, Quality, and Fail-Safe I/O
  - [PRINCIPLE_5_NAME] → V. Signed Packages and Least-Privilege Control Plane
Added principles:
  - VI. Architecture Contract and Crate Boundaries
  - VII. Testable Contracts and Frozen Interfaces
  - VIII. Observability Without RT Side Effects
Added sections:
  - Product, Language, and Stack Boundaries (replaces [SECTION_2_NAME])
  - Development Workflow and Quality Gates (replaces [SECTION_3_NAME])
Removed sections: none (template placeholders replaced in place)
Follow-up TODOs: none
Source of values: docs/architecture.md (Draft Rev 2.2, 2026-08-14),
  AGENTS.md, README.md, rust-toolchain.toml, scripts/check-rt-deps.sh
-->

# Soft PLC Constitution

## Core Principles

### I. Real-Time Path Isolation (NON-NEGOTIABLE)

The scan thread is a hard isolation boundary. Code that runs on, or is
callable from, the RT scan path MUST NOT:

- depend on `tokio` or any network, TLS, HTTP, or MQTT crate
- perform blocking I/O, filesystem access, or unbounded lock waits
- allocate after first entry to `RUN` (cold-path alloc only in `STOP`,
  validate, or arm)
- format log strings or write logs on the RT thread
- hold a `Mutex` that a non-RT thread can take during I/O
- use types whose `Drop` frees heap on the RT path; prefer `Copy`
  state and arena-bump instance data allocated at arm

RT-path crates are `plc-scan`, `plc-vm`, `plc-io`, `plc-types`,
`plc-fb-primitives`, `plc-retain`, and `plc-ir`. Non-RT work (REST,
MQTT, remote I/O workers, retain flush) MUST run on tokio workers or
dedicated non-RT threads. The RT scan thread MUST NOT be a tokio task.
RT MAY communicate with non-RT only via lock-free SPSC queues and
atomics.

CI MUST fail if an RT-path crate gains a forbidden dependency
(`scripts/check-rt-deps.sh`). `unsafe` is confined to `plc-io-*` FFI
and explicitly allowlisted modules. The IR verifier and package parser
MUST remain safe Rust.

Rationale: 10–20 ms Fast-task budgets cannot absorb TCP retransmit,
allocator churn, or Drop glue (KD-5a, KD-8, KD-13).

### II. Cyclic Scan and Cooperative Execution

The runtime MUST execute a classic IEC 61131-style cyclic scan
(Input → Logic → Output) on a single cooperative RT thread (KD-2,
KD-11). When multiple tasks are due, highest priority runs first
(rate-monotonic). A task invocation MUST run to completion; the
runtime MUST NOT preempt mid-function-block or mid-instruction.

Timers MUST use `CLOCK_MONOTONIC` sampled once per task invocation;
`TIME` resolution is 1 ms. Wall clock / NTP MUST NOT feed TON/TOF/TP
(KD-16). Telemetry timestamps use the system/NTP clock and MUST mark
quality Uncertain when unsynchronized; v1 MUST NOT depend on PTP
(KD-19).

Operator `mode` ∈ {STOP, RUN, FAULT, SIM} is orthogonal to
`program.phase` ∈ {idle, validating, armed, swapping}. There is no
operator mode named LOAD. Validation failures MUST NOT enter FAULT
(KD-17). Consecutive logic overruns beyond the configured limit
(default 2) MUST enter FAULT and force outputs to `safe_state`.
I/O degraded quality MUST NOT by itself enter FAULT.

Program hot-swap MUST follow the Program Epoch Protocol (KD-4,
KD-4a): one closed program image; swap only at invocation boundaries;
finish any in-flight invocation before install (join time is outside
the critical section); install deadline ≤ `min_task_period` starting
when install begins; missed deadline MUST defer (remain armed), not
FAULT. v1 is one application / one active program package per device
(KD-14).

### III. Verified Bytecode, Never Native User Logic

User logic MUST be compiled offline to signed IR v0.1 bytecode
(`spbc` inside `.spkg`). The controller MUST NOT interpret ST source
on device, MUST NOT JIT or AOT-compile native code on device, MUST
NOT load user `.so` plugins, and MUST NOT execute WASM in v1 (KD-3).

The VM MUST be a bounded stack machine: verifier-enforced max stack
256, max call depth 32, no user-FB recursion, and no dynamic
allocation in the `RUN` hot loop. Primitive FBs (TON/TOF/TP, CTU/CTD,
RS/SR, edges, PID) are Rust builtins; library FBs are ST-subset
compositions shipped inside the closed package. The engineering
language is the constrained ST-subset in architecture Appendix B
(KD-12). Full IEC 61131-3 (LD/FBD/SFC) is out of scope for v1.

Packages are `.spkg` v1: JSON manifest only (CBOR MUST be rejected),
RFC 8785 JCS canonicalization, Ed25519 over SHA-256 of canonical
manifest concatenated with bytecode, maximum 8 MiB.

### IV. Process Image, Quality, and Fail-Safe I/O

I/O is a process image (`%I` / `%Q` / `%M` / `%R`) plus a per-tag
quality plane (`Good` / `Uncertain` / `Bad`) and pluggable `IoDriver`s
(KD-5). Network and remote I/O MUST run only on non-RT workers; the
RT thread copies sequence-numbered double buffers at I/Q phases
(KD-5a). Local GPIO MAY run in-RT only if WCET-measured and heap-free.

Every `%Q` channel MUST have `safe_state` (default de-energize).
Effective output priority is: FAULT or global force_safe →
maintenance force overlay → module Bad with force_safe → program `%Q`
→ `safe_state`. Force overlays MUST clear on STOP, FAULT, and
FAULT_RESET, and MUST be audited.

v1 field I/O is sim + GPIO + Modbus TCP only (KD-20). Process-death
fail-safe is a deployment constraint: remote modules MUST de-energize
on heartbeat or session loss; the runtime MUST NOT treat userspace
crash handling as sufficient. This product is process control only,
not a SIL-rated safety PLC (KD-15). Cold standby only; no hot standby
in v1.

### V. Signed Packages and Least-Privilege Control Plane

Programs MUST be Ed25519-signed; the controller MUST verify before
arm. The production profile MUST require signatures
(`program.require_signature: true`) (KD-9). Roles are `viewer`,
`operator`, `engineer`, and `admin`. Activate requires `engineer` or
above. Config and status use REST over HTTPS (OpenAPI-described);
REST MUST NOT carry cyclic process data (KD-6). Auth lockout is 5
failures per minute per IP then 60 s.

Telemetry is MQTT 5 + Sparkplug B 3.0. v1 MUST NOT ship a WebSocket
server (KD-7, KD-21). Telemetry MUST be non-blocking from the RT path
(SPSC; drop oldest; count `telemetry_drops`). OPC UA is Phase 2, not
a v1 requirement.

### VI. Architecture Contract and Crate Boundaries

`docs/architecture.md` is the design of record. New crates, public
APIs, execution models, or frozen contracts that contradict it MUST
NOT land without an explicit design change. If a Core Principle is
affected, this constitution MUST be amended in the same change.

The workspace is a Rust monorepo (`crates/*`) in a single process for
v1 (KD-10). Crates MUST have a clear purpose; organizational-only
crates are forbidden. The open-source core is licensed Apache-2.0
(KD-18). C is permitted only via FFI for mature fieldbus stacks and
optional external-driver ABIs. C++ MUST NOT be the primary language.

### VII. Testable Contracts and Frozen Interfaces

Changes that touch a frozen contract MUST include tests that fail if
the contract is violated. Frozen contracts are:

- OpenAPI 3 (`docs/openapi/openapi.yaml`)
- `.spkg` v1 and IR v0.1 (architecture Appendix A)
- `IoDriver`, quality plane, and double-buffer handoff
- Sparkplug B 3.0 (`docs/sparkplug.md`)
- `TelemetrySource` SPSC from `plc-scan`
- ST-subset allowlist (architecture Appendix B)

IR fixtures MUST be text-reviewable (`fixture.spasm`). Hot-swap,
retain map policy, FirstScan multi-rate, timer STOP→RUN preserve, and
telemetry backpressure MUST remain covered by automated tests.
Telemetry backpressure MUST NOT block the scan.

### VIII. Observability Without RT Side Effects

Non-RT paths MUST emit structured logs (`tracing`) and Prometheus
metrics. The RT path MUST expose counters and atomics only — never
format strings or perform log I/O. The diagnostics event ring is 4096
in-memory events (overwrite oldest). The audit file rotates at 16 MiB
and keeps 8 files. Privileged operations (mode changes, program
arm/activate, config writes, forced tags) MUST be audited.

## Product, Language, and Stack Boundaries

v1 is a greenfield soft PLC for heavy / bulk materials plants
(conveyors, crushers, screens, silos, weighers, stackers/reclaimers).
Scan cycles of 10–100 ms and jitter of a few milliseconds are the
performance envelope, not servo motion.

**Language and toolchain**

- Implementation language: Rust 1.85 (`rust-toolchain.toml`) (KD-1).
- Non-RT async runtime: tokio (HTTP/axum, MQTT, remote I/O, retain
  flusher) (KD-13).
- Deploy on Linux with optional `PREEMPT_RT`; isolate the RT scan
  thread (KD-8). Reference hardware is x86_64 industrial PC / NUC
  first (KD-22). ARM is not the pilot gate.

**v1 MUST deliver**

- Deterministic cyclic scan with prioritized tasks, watchdog, and
  retain memory.
- Runtime program download and hot-swap under the epoch protocol.
- Composable function-block / library model (ST-subset + primitives).
- I/O abstraction with quality; REST config/status; MQTT Sparkplug
  egress.
- Security posture: zoned networks, signed programs, role auth, safe
  I/O defaults, documented process-death assumptions.

**v1 MUST NOT include**

- Full IEC 61131-3 language suite or PLCopen certification.
- SIL-rated / IEC 61508 safety PLC functions (safety I/O stays
  external).
- Native motion/CNC or sub-millisecond hard RT as a product
  requirement.
- Built-in SCADA/HMI (data egress and config APIs only).
- IEC 61499 event mesh as the primary execution model.
- Online source-level debugging IDE (runtime accepts artifacts).
- Multiple applications per device.
- Hot standby redundancy.
- On-device native JIT or WASM user logic.
- EtherCAT / PROFINET in the v1 I/O set.
- On-device WebSocket telemetry.
- UPS/supercap as a hard runtime requirement; last-fsync retain dirty
  window is accepted and MUST be documented (KD-23).

**Performance targets (materials plants)**

| Metric | Target |
|--------|--------|
| Default continuous task period | 50 ms |
| Fast task period | 10–20 ms |
| Max consecutive logic overruns before FAULT | 2 (configurable) |
| RT jitter (PREEMPT_RT, isolated core) | p99 < 2 ms for 50 ms task |
| Logic budget per 50 ms task | ≤ 30 ms WCET |

**Retain**

RT sets dirty flags and enqueues retain pages via SPSC. The T5 retain
flusher performs encode + `fsync`. The Background task class MUST NOT
perform NV I/O.

## Development Workflow and Quality Gates

**Version control**

- All work ships through GitHub PRs. Local commits on `main` are not
  done.
- MUST NOT commit to `main`. MUST NOT push to `main`.
- Implement on a `feature/`, `fix/`, or `chore/` branch created from
  `origin/main`.
- Agents MUST NOT merge. Merging is a human decision. Prefer
  squash-merge.
- Commit messages are imperative and specific; optional `(PR-NN)`
  suffix for architecture plan items.

**Quality gates (required before commit or push)**

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

When changing RT-path crates, also:

```bash
bash scripts/check-rt-deps.sh
```

CI runs the same checks (`cargo fmt --check`, clippy with
`-D warnings`, workspace tests, RT-path dep check). A PR that fails
these gates MUST NOT merge.

**PR discipline**

- Incremental, independently reviewable PRs. Each MUST leave `main`
  buildable and tested.
- Do not invent crates or public APIs that contradict
  `docs/architecture.md`.
- Complexity on the RT path MUST be justified against WCET and the
  isolation rules in Principle I.
- Reviewers MUST verify: RT isolation when RT crates change; frozen
  contract tests when contracts change; signature/auth behavior when
  the control plane changes; fail-safe output policy when I/O or
  modes change.

**Guidance files**

- Design of record: `docs/architecture.md`
- Agent/runtime development rules: `AGENTS.md`
- Sparkplug contract: `docs/sparkplug.md`
- REST contract: `docs/openapi/openapi.yaml`

## Governance

This constitution supersedes informal practice, convenience, and
conflicting local habits. `docs/architecture.md` is the detailed
design of record; this file is the non-negotiable governance overlay.
If the two disagree on a Core Principle, that is a defect: amend one
or both in the same PR. Key Decisions KD-1 through KD-23 in the
architecture document are binding unless this constitution is amended
to retire them.

**Amendments**

1. Propose the change in a PR that updates this file (and architecture
   if design rules move).
2. State the semantic version bump and a Sync Impact Report
   (principles added/removed/renamed, sections changed, deferred
   TODOs).
3. MAJOR: backward-incompatible removal or redefinition of a
   principle or governance rule.
4. MINOR: new principle or section, or materially expanded guidance.
5. PATCH: clarification, wording, or non-semantic refinement.
6. Human review and squash-merge. Agents MUST NOT merge amendments.

**Compliance**

- Every PR and review MUST check applicable principles. Touching
  RT-path crates without the RT dep check is non-compliant.
- Adding `tokio`, network I/O, or logging to the scan path is a
  principle violation, not a style nit.
- Shipping unsigned-program-as-default in the production profile is a
  principle violation.
- Spec Kit specs, plans, and tasks MUST be consistent with this
  constitution. A spec that requires native user `.so` plugins, WASM
  user logic, WebSocket process egress in v1, SIL certification, or
  tokio on the scan thread is non-compliant and MUST be rejected or
  rewritten.

**Versioning policy**

The version line below is the constitution version, independent of
crate semver and IR `ir_major`/`ir_minor`. IR breaking changes still
require `ir_major ≥ 1` per architecture Appendix A and do not by
themselves bump this file unless a principle changes.

**Version**: 1.0.0 | **Ratified**: 2026-08-28 | **Last Amended**: 2026-08-28
