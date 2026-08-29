# Data Model: Runtime Binary and SIM Conveyor Demo

Entities below are the runtime/demo objects this feature introduces or wires. Existing frozen types (IR v0.1, `.spkg` v1, OpenAPI, Sparkplug) are referenced, not redefined.

## ControllerInstance

The single OS process started by `soft-plc-runtime`.

| Field | Type | Rules |
|-------|------|--------|
| config | DeviceConfig | Loaded from `--config`; must validate schema v1 |
| profile | `dev` \| `prod` | Sample is `dev` |
| data_dir | optional path | When set, remaps programs/retain/audit roots |
| mode | OperatingMode | STOP at boot; SIM/RUN/FAULT per existing transitions |
| program_phase | ProgramPhase | idle / validating / armed / swapping |
| scan_thread | std::thread | Named `plc-scan`; not a tokio task |
| telemetry | TelemetryHandle + worker | No-op when `telemetry.enabled=false` or broker down |
| retain_store | RetainStore | Under `paths.retain` |
| program_store | ProgramStore | Under `paths.programs`; pointers `current`, `armed` |

**Validation**: Missing/invalid config or bind failure → process exits non-zero, no scan of user logic. Non-sim `io.drivers` → start error.

## DeviceConfiguration (existing, sample filled)

Sample `samples/configs/sim-plant.yaml` plus new io-map file.

| Field | Sample value | Rules |
|-------|--------------|--------|
| profile | dev | Unsigned packages + optional auth allowed |
| scan.tasks | fast 20 ms `task.fast`; main 50 ms `task.main`; slow 500 ms `task.slow` | Must match package `task_entries` keys |
| io.drivers | `[sim]` | Binary supports only `sim` |
| program.require_signature | false | Dev only |
| auth.required | false | Anonymous lab ops |
| rest.bind | `127.0.0.1:8443` | Local-only; plaintext HTTP in dev with empty TLS paths |
| telemetry.enabled | true | Broker may be down |
| paths.io_map | `samples/configs/sim-plant-io-map.yaml` | Required for named conveyor slots |

## IoMap (sim plant)

New document `samples/configs/sim-plant-io-map.yaml`.

| Field | Rules |
|-------|--------|
| version | 1 |
| modules | Exactly one module `id=sim0`, `driver=sim` |
| bindings | See ConveyorTag; each has `image` I or Q, `type` BOOL, `slot` explicit 0..n-1 |
| RunFwd.safe_state | `false` (de-energize) |
| on_bad_quality | `force_safe` |

**Validation**: Duplicate tag or slot → config load fail. Slot counts define `ProcessImage` `%I`/`%Q` sizes (4 inputs, 2 outputs).

## DemoProgramPackage

Checked-in pair: listing + container.

| Field | Value |
|-------|--------|
| id | `demo-conveyor` |
| version | `0.1.0` |
| listing | `samples/programs/demo-conveyor/fixture.spasm` |
| package | `samples/programs/demo-conveyor/fixture.spkg` |
| signature | Unsigned (all-zero sentinel) |
| ir | v0.1 (`ir_major=0`, `ir_minor=1`) |
| primitive_abi | 1 |
| restart_policy | `safe_reset` |
| retain_symbols | empty |
| input_slots | 4 |
| output_slots | 2 |
| retain_size | 0 |
| task_entries | `fast→task.fast`, `main→task.main`, `slow→task.slow` |

**Validation**: Assembler + verifier must accept the listing. Packed bytes must match the listing (CI golden). Manifest image counts must equal `spbc` header (existing package validate).

### Instance layout (data segment)

| Instance | Primitive | Base offset | Size (aligned) |
|----------|-----------|-------------|----------------|
| run_latch | RS | 0x00 | 16 |
| start_delay | TON | 0x10 | 32 |
| start_edge | R_TRIG | 0x30 | 8 |
| data_size | — | — | ≥ 64 (use 128) |

## ConveyorTag

Named process points. Sparkplug metric name = tag name (`/` separator).

| Name | Kind | Slot | Type | Default (sim) | Meaning |
|------|------|------|------|---------------|---------|
| `Conveyor1/Start` | I | 0 | BOOL | false | Start command (level or pulse; rising edge latches) |
| `Conveyor1/Stop` | I | 1 | BOOL | false | Stop command |
| `Conveyor1/PullCordOK` | I | 2 | BOOL | false | Interlock healthy when true |
| `Conveyor1/ChuteBlocked` | I | 3 | BOOL | false | Start-permissive fails when true |
| `Conveyor1/RunFwd` | Q | 0 | BOOL | false, safe=false | Motor run forward |
| `Conveyor1/Fault` | Q | 1 | BOOL | false, safe=false | Process fault / not-ready indication |

**Validation**: After activate, GET `/tags` lists these names. Telemetry catalog contains the same I/Q set.

## ConveyorLogic (behavioral)

State is implicit in RS.Q, TON.Q, and `%Q`.

**Derived conditions**

- `permissive := PullCordOK AND NOT ChuteBlocked`
- `estop := NOT PullCordOK`

**Fast (every 20 ms)**

1. Sample inputs.
2. If `estop`, `RunFwd := false` (ST_Q slot 0). Do not wait for TON.
3. If `estop`, `Fault := true`.

**Main (every 50 ms)**

1. `start_edge := R_TRIG(Start)`
2. `RS.S := start_edge AND permissive`; `RS.R := Stop OR estop`; evaluate RS (reset-dominant).
3. `TON.IN := RS.Q AND permissive AND NOT RunFwd`; `TON.PT := 2000 ms`.
4. `RunFwd := RS.Q AND permissive AND (TON.Q OR RunFwd)` except fast-path already cleared RunFwd on estop this cycle if fast ran first (rate-monotonic).
5. `Fault := estop OR (Start AND ChuteBlocked AND NOT RS.Q)` (or simpler: `Fault := estop OR NOT permissive` while a start is requested). Minimum required: Fault true when pull-cord lost; Fault or not-ready visible when chute blocks a start.

**Slow**: HALT.

**STOP**: outputs safe (RunFwd false, Fault false unless STOP policy later differs — sample is `safe`, so both de-energize).

## OperatingMode / ProgramPhase (existing)

Unchanged KD-17 machine. This feature only **uses** it.

```text
mode:    STOP ⇄ SIM
         STOP ⇄ RUN
         RUN  → FAULT (overrun / logic error)
         FAULT → STOP via FAULT_RESET
         SIM from RUN = reject
         SIM from STOP = accept

phase:   idle → validating → armed → swapping → idle
         validation failure → prior phase (never FAULT)
```

Boot: `mode=STOP`, `phase=idle` unless `current` is restored (then `phase=idle`, program current, **mode still STOP**).

## TelemetryCatalog (derived)

Built after successful activate.

| Field | Source |
|-------|--------|
| name | TagEntry.name (`Conveyor1/Start`, …) |
| is_input | kind == I |
| slot | TagEntry.slot |
| value_type | BOOL → Sparkplug Boolean |
| unit | IoMap binding `unit` if set, else omitted |
| alias | 1..=N lexicographic by name |

SYSTEM/Mode and node metrics remain publisher-owned (existing PR-13).

## LabInjectWrite

| Field | Rules |
|-------|--------|
| tag | Must exist in dictionary |
| kind | I (sim inject) or Q (force overlay) |
| mode | `%I` inject allowed when driver is sim (demo always); `%Q` force per existing operator permission |
| persistence | `%I` lives in SimDriver until overwritten; `%Q` force clears on STOP/FAULT/FAULT_RESET (existing) |

## Runbook

User-facing procedure entity (file `docs/runbook-sim-demo.md`).

Must cover: prerequisites, `--config`/`--data-dir`, start, POST package, arm, activate, POST mode SIM, inject Start/PullCordOK, observe RunFwd, optional MQTT, shutdown, dev vs prod, non-goals (compiler, fieldbus, hardening).
