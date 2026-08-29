# Feature Specification: Runtime Binary and SIM Conveyor Demo

**Feature Branch**: `001-runtime-demo-conveyor`

**Created**: 2026-08-28

**Status**: Draft

**Input**: User description: "read @docs/architecture.md and build PR-14"

## User Scenarios & Testing *(mandatory)*

This feature delivers the first **runnable controller** for a lab SIM plant: one process an engineer can start, a checked-in conveyor demo program that does not need a compiler, and a runbook that walks through simulation, status, and plant telemetry. It depends on already-landed scan, package load, management, and telemetry work. It does **not** wait for the Structured Text compiler.

### User Story 1 - Start a single controller and cycle the demo conveyor in SIM (Priority: P1)

A control engineer on a lab workstation starts the controller as **one process** using the sample SIM plant configuration. They install the checked-in demo conveyor program (a human-reviewable listing plus a ready-to-load package) without compiling source. They place the controller in **SIM**, and the conveyor logic cycles: simulated start/stop, a start delay, and a pull-cord interlock that drops the run output.

**Why this priority**: Until a single process runs a recognizable plant demo from fixtures, the product cannot be shown, soaked, or used as the base for later field I/O and hardening. This is the Stage 1 rollout success criterion: a demo program cycles in SIM from a checked-in package.

**Independent Test**: Start from the sample SIM configuration, load only the checked-in demo artifacts (no compiler present), enter SIM, and observe the conveyor run output follow start/stop and pull-cord conditions.

**Acceptance Scenarios**:

1. **Given** the sample SIM plant configuration and the checked-in demo conveyor artifacts, **When** the engineer starts the controller, **Then** a single process comes up, loads configuration, and exposes status (mode, program phase, scan health) without requiring a second companion process.
2. **Given** a freshly started controller with no previously activated program, **When** the engineer follows the runbook to install the demo conveyor package, **Then** the package validates, arms, and activates, and status shows the demo as the current program.
3. **Given** the demo program is current, **When** the engineer sets mode to SIM, **Then** cyclic logic runs against simulated I/O only and does not write to field devices.
4. **Given** SIM is running and simulated permissives are healthy, **When** a start command is asserted, **Then** the conveyor run output becomes true after the documented start delay and remains true while permissives hold.
5. **Given** no Structured Text compiler is installed, **When** the engineer repeats the demo start path, **Then** the demo still installs and cycles from the checked-in artifacts.

---

### User Story 2 - Exercise conveyor interlocks and operator modes (Priority: P2)

An operator (or engineer acting as operator) uses the existing configuration/status interface to stop, simulate, and inspect conveyor tags. Pull-cord loss must drop the run output promptly (fast interlock), not after the start-sequence timer. A blocked chute or equivalent start-permissive failure must prevent a start. STOP de-energizes outputs to the configured safe state.

**Why this priority**: A demo that only “runs a scan” without recognizable conveyor behavior does not prove the product to materials-plant stakeholders.

**Independent Test**: With the demo active in SIM, inject pull-cord loss and start-permissive failure via simulated inputs or forced tags, and confirm run-output and mode behavior without field hardware.

**Acceptance Scenarios**:

1. **Given** the conveyor is running in SIM, **When** the pull-cord OK input becomes false, **Then** the run output de-energizes within one fast-task period (20 ms class) even if the start-sequence timer has not elapsed.
2. **Given** the conveyor is stopped in SIM and a start-permissive is false (for example chute blocked), **When** a start command is asserted, **Then** the run output stays de-energized.
3. **Given** the controller is in SIM, **When** the operator commands STOP, **Then** logic stops cycling as an executing program and outputs go to the configured safe state (sample config: safe / de-energize).
4. **Given** the controller is in STOP with the demo armed or current, **When** the operator commands SIM, **Then** mode becomes SIM and logic runs. **Given** mode is RUN, **When** the operator commands SIM, **Then** the request is rejected until STOP (existing mode rule).
5. **Given** the demo is active, **When** the operator reads the tag dictionary and individual conveyor tags, **Then** named tags for permissives, commands, run, and fault/ready are present and match live logic state.

---

### User Story 3 - Watch the demo on the plant telemetry stream (Priority: P3)

A dashboard or SCADA gateway consumer subscribed to the existing plant telemetry stream sees conveyor tags and controller mode while the demo runs in SIM. If the telemetry destination is unavailable, the scan continues and the demo still runs.

**Why this priority**: Plant visuals in v1 leave the controller over the plant telemetry stream only — not a built-in operator screen on the device. The runnable demo must populate that stream so a lab dashboard can show the conveyor.

**Independent Test**: With a local telemetry destination available, activate the demo in SIM and confirm conveyor metrics and mode appear. Repeat with the destination down and confirm scan/mode still operate.

**Acceptance Scenarios**:

1. **Given** telemetry is enabled in the sample SIM configuration and a local destination is reachable, **When** the demo program is activated, **Then** conveyor tags from the program’s tag dictionary (plus I/O-map units where configured) are published on the plant telemetry stream.
2. **Given** the demo is running in SIM, **When** the conveyor run output or mode changes, **Then** a telemetry consumer observes the corresponding metric change without polling the management interface for cyclic process data.
3. **Given** the telemetry destination is down or unreachable, **When** the demo runs in SIM, **Then** the scan continues, SIM remains available, and the controller does not stall waiting for telemetry.

---

### User Story 4 - Lab-friendly development profile with a documented production posture (Priority: P4)

The sample SIM configuration uses the **development** profile so a local engineer can run unsigned demo packages and optional authentication on the local machine. A runbook documents how to start, load, observe, and shut down the demo, and states what the **production** profile will require (signed programs, authentication). The demo remains runnable in development; full production hardening that refuses remaining insecure production combinations is a later feature.

**Why this priority**: Lab convenience unblocks demos; undocumented insecure defaults are a known field risk. This feature must make the split explicit without blocking the demo on hardening work.

**Independent Test**: Follow the runbook on a clean lab machine using only development-profile sample config and checked-in artifacts; confirm the runbook states production signature and authentication expectations.

**Acceptance Scenarios**:

1. **Given** the sample SIM configuration (`profile: dev`), **When** the engineer loads the unsigned demo package, **Then** validation and arm succeed without a signature requirement.
2. **Given** the sample SIM configuration, **When** the engineer uses the management interface without authentication, **Then** local lab operations (status, mode, program load) succeed as allowed by development defaults.
3. **Given** the runbook, **When** a reviewer reads the production-profile section, **Then** it states that production requires signed programs and authentication, that the demo is intended for development/SIM, and that remaining production refusals ship in a later hardening feature.
4. **Given** the runbook steps, **When** an engineer who has not seen the codebase follows them, **Then** they can start the controller, load the demo, enter SIM, observe tags or telemetry, and shut down without inventing extra procedures.

---

### Edge Cases

- **Missing or invalid device configuration**: The controller must refuse to start and report a clear configuration error; it must not begin scanning user logic with an empty or guessed configuration.
- **Missing, corrupt, or incompatible demo package**: The controller must not activate the program; mode must not enter FAULT solely because validation failed; status must show a failed validation/arm with a readable reason.
- **No current program at boot**: The process still starts (management and status available); mode remains STOP (or equivalent idle); the engineer can install the demo.
- **Telemetry destination unavailable**: Scan, SIM, and management continue; telemetry drops or offline state is countable/visible; the scan must not block.
- **SIM requested while field drivers are configured**: The sample demo configuration uses simulation I/O only. If SIM is requested in a configuration that includes non-sim drivers, the controller must not write field outputs (SIM means simulated I/O only). The sample path does not require field drivers.
- **Development vs production package policy**: Unsigned packages are accepted in development when signatures are not required. Production configuration that disables signatures or authentication is already invalid at config load; this feature documents that posture and does not reopen it.
- **Process shutdown (stop signal or clean exit)**: The controller stops cyclic execution, applies safe outputs for simulation, and exits without leaving the management listener hanging as a zombie service.
- **Management bind address already in use**: Start fails with a clear error; a second copy must not silently take over.
- **Compiler present or absent**: Demo behavior must not change; this feature must not invoke or wait on a compiler.
- **Hot-swap of a second package while SIM is running**: Existing epoch/activate rules apply (arm then activate; deferred activate is not FAULT). The demo runbook may show a single load; swap is not required for the happy path.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: The product MUST ship a single controller process that an engineer can start from the sample SIM plant configuration and operate without a second runtime process.
- **FR-002**: That process MUST provide, together: cyclic scan of the active program, program install (validate / arm / activate), configuration and status for operators, plant telemetry egress, retained-value persistence, and simulated I/O — reusing the capabilities already delivered for those areas.
- **FR-003**: The product MUST ship a demo conveyor program as two checked-in artifacts that stay equivalent: a human-reviewable program listing and a ready-to-load program package.
- **FR-004**: Installing and running the demo MUST NOT require a Structured Text compiler, on-device compilation, or generation of the package at demo time.
- **FR-005**: The demo program MUST model one conveyor with at least: start command, stop command, pull-cord OK, a start-permissive (chute not blocked), a start-delay timer, a run-forward output, and a fault or not-ready indication.
- **FR-006**: While SIM is active and permissives are healthy, asserting start MUST energize the run-forward output only after the start delay (default **2 seconds** unless the listing documents another value).
- **FR-007**: While the conveyor is running or timing to start, loss of pull-cord OK MUST de-energize run-forward within one fast-task period (**20 ms** in the sample task table), without waiting for the start-delay timer.
- **FR-008**: A false start-permissive MUST prevent a start (run-forward stays de-energized).
- **FR-009**: The demo MUST execute interlock behavior on the fast task and start/stop sequencing on the main task, matching the sample task table (fast **20 ms**, main **50 ms**, slow **500 ms**). Slow may carry non-critical status only.
- **FR-010**: Sample SIM configuration MUST use the development profile, simulation I/O only, local-only management access, optional authentication, and unsigned-program allowance so the demo runs locally.
- **FR-011**: Sample SIM configuration MUST include an I/O map that binds the demo conveyor tags to simulated inputs and outputs, including safe-state de-energize for the run-forward output.
- **FR-012**: When mode is SIM, logic MUST run and the controller MUST NOT write field (non-sim) outputs.
- **FR-013**: Operator mode changes MUST follow existing rules: SIM only from STOP; STOP applies the configured stop-output policy (sample: safe); validation/arm failure MUST NOT enter FAULT.
- **FR-014**: After the demo package is activated, named conveyor tags MUST appear in the tag dictionary and MUST be readable for debug; telemetry MUST publish those tags (and mode) on the existing plant telemetry path when telemetry is enabled.
- **FR-015**: After activate, the set of process tags published on the plant telemetry stream MUST come from the activated program’s tag dictionary, applying I/O-map engineering units where the map defines them.
- **FR-016**: If the telemetry destination is unavailable, the scan MUST continue and MUST NOT block on telemetry.
- **FR-017**: If a current program exists in the program store at start, the controller MUST restore it according to existing cold-boot / retain policy. If none exists, the controller MUST start in STOP with no user logic running until the demo (or another package) is activated.
- **FR-018**: Start MUST fail closed on missing/invalid configuration or an unusable management bind, with a message that identifies the problem.
- **FR-019**: The product MUST include a runbook that covers: prerequisites, starting with the sample SIM configuration, installing the checked-in demo package, entering SIM, observing tags and telemetry, shut down, development vs production profile expectations, and explicit non-goals (compiler, fieldbus, production hardening).
- **FR-020**: The runbook MUST state that production requires signed programs and authentication, that insecure development defaults are for lab/SIM only, and that remaining production refusals are a later hardening feature. The demo MUST remain runnable under the development profile.
- **FR-021**: The demo listing MUST remain text-reviewable. The packaged demo MUST assemble from that listing so a later compiler can be judged by round-trip equivalence; this feature MUST NOT add Structured Text sources as a runtime dependency.
- **FR-022**: The controller MUST remain one application / one active program per device. The demo is that application for the SIM plant sample.
- **FR-023**: The controller MUST NOT host an on-device live-view socket for process data. Lab visuals MUST use the existing plant telemetry stream (or a bridge in front of that stream).
- **FR-024**: Clean shutdown MUST stop cyclic execution and leave simulated outputs in the safe (de-energized) state.

### Key Entities

- **Controller process**: The single runnable controller an engineer starts. Owns scan, management, telemetry, retain, and I/O workers for v1.
- **Device configuration**: Versioned plant/device document (sample SIM plant). Selects development vs production profile, task periods, telemetry enablement, I/O drivers, management bind, and store paths.
- **Deployment profile**: `dev` (lab: unsigned packages and optional authentication allowed on the local machine) vs `prod` (signed programs and authentication required). Sample demo uses `dev`.
- **Demo conveyor program**: Checked-in listing + package implementing one conveyor’s permissives, start delay, run output, and interlock. Human-reviewable listing is the review source of truth.
- **Program package**: The existing downloadable program container the controller already validates and activates. The demo ships a ready-to-load instance; a signature is not required under the development sample.
- **I/O map**: Bindings of conveyor tag names onto simulated process-image slots, with safe-state for outputs.
- **Operating mode**: STOP, RUN, FAULT, SIM — orthogonal to program phase (idle, validating, armed, swapping).
- **Conveyor tags**: Named process points the operator and telemetry consumer use (commands, permissives, run-forward, fault/not-ready).
- **Runbook**: Step-by-step lab procedure and profile notes; the user-facing contract for “can we run the demo?”

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: An engineer who has not contributed to the codebase can follow the runbook and, on a lab machine with no compiler, reach “demo conveyor cycling in SIM” in **under 15 minutes** (start, load, SIM, observe run-output change).
- **SC-002**: **100%** of primary runbook steps (start, load demo, enter SIM, observe a tag or telemetry change, shut down) complete without undocumented extra commands.
- **SC-003**: With healthy simulated permissives, a start command produces a run-forward true after the documented delay on **every** clean SIM trial (10/10 in lab verification).
- **SC-004**: Pull-cord loss while running drops run-forward within the fast-task period on **every** trial (10/10); start with chute blocked never energizes run-forward (10/10).
- **SC-005**: With the telemetry destination stopped, the controller still enters SIM and continues cycling for at least **5 minutes** without operator-visible stall; with the destination available, a consumer sees conveyor tags and mode without using the management interface as a cyclic data path.
- **SC-006**: Lab users identify the controller as **one process** to start and stop; no runbook step requires a second controller process.
- **SC-007**: Reviewers can audit demo behavior from the text listing alone; the packaged demo matches that listing (same conveyor behavior).
- **SC-008**: At least **90%** of first-time lab users following the runbook succeed on the first attempt to reach SIM with the demo current (measured in internal dry-runs; failures are runbook defects).

## Assumptions

- Architecture item **PR-14** (Runtime binary + demo from fixtures, not full compiler) is the design of record for this feature. Dependencies PR-12 (management API) and PR-13 (Sparkplug telemetry) are already landed. This feature does **not** wait on PR-15 (host compiler).
- Existing contracts are reused, not redesigned: configuration schema and sample SIM plant document, program package format, epoch arm/activate, operating modes, management interface, plant telemetry stream, simulation I/O, retain store.
- Reasonable demo defaults: one conveyor named `Conveyor1`; start delay **2 s**; tags at least `Conveyor1.Start`, `Conveyor1.Stop`, `Conveyor1.PullCordOK`, `Conveyor1.ChuteBlocked`, `Conveyor1.RunFwd`, and `Conveyor1.Fault` (or equivalent documented names). Fast task owns pull-cord drop-out; main task owns start/stop sequencing.
- Sample configuration remains development-profile, `io.drivers: [sim]`, management on loopback, `auth.required: false`, `program.require_signature: false`. Production profile already rejects missing signatures/auth at configuration load; **PR-20** still owns remaining secure-by-default refusals and rate-limit hardening. This feature documents that split.
- Telemetry in the sample may target a local broker; the runbook may use an optional local broker. The demo is valid without a dashboard product (none ships in v1).
- Field I/O drivers (Modbus TCP, GPIO), RT timing harness, diagnostics ring/Prometheus completeness, and Structured Text sources for the demo are **out of scope**. Demo Structured Text, if added later, is a compiler concern (PR-15) and must round-trip to these fixtures.
- Paths in the sample configuration may be overridden in the runbook to writable lab directories; production paths under `/var/lib/soft-plc/` remain the documented device layout.
- v1 remains process control only (not a SIL-rated safety PLC). The demo pull-cord is a **process interlock mirror**, not a certified safety function.
- Single process on Linux; reference hardware is an x86_64 lab PC or NUC. Optional real-time kernel tuning is **not** required to pass this feature’s demo.
- No on-device WebSocket, no native user plugins, no WASM user logic, no second application on the device.
