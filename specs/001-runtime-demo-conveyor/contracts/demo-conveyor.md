# Contract: Demo conveyor program and I/O map

## Files

| Path | Role |
|------|------|
| `samples/programs/demo-conveyor/fixture.spasm` | Human-reviewable IR listing (source of truth) |
| `samples/programs/demo-conveyor/fixture.spkg` | Unsigned `.spkg` v1 assembled from the listing |
| `samples/configs/sim-plant-io-map.yaml` | Sim module bindings |
| `samples/configs/sim-plant.yaml` | Device config (already present; io-map path already set) |

No `.st` sources in this feature.

## Manifest (package)

```text
id:                 demo-conveyor
version:            0.1.0
build_id:           fixture-pr14
ir_major / ir_minor: 0 / 1
primitive_abi:      1
restart_policy:     safe_reset
task_entries:
  fast: task.fast
  main: task.main
  slow: task.slow
input_slots:  4
output_slots: 2
retain_size:  0
signature:    unsigned (64 zero bytes)
```

Tag dictionary (must match I/O map names and slots):

| name | type | kind | slot |
|------|------|------|------|
| Conveyor1/Start | BOOL | I | 0 |
| Conveyor1/Stop | BOOL | I | 1 |
| Conveyor1/PullCordOK | BOOL | I | 2 |
| Conveyor1/ChuteBlocked | BOOL | I | 3 |
| Conveyor1/RunFwd | BOOL | Q | 0 |
| Conveyor1/Fault | BOOL | Q | 1 |

## I/O map (sim)

One module `sim0`, `driver: sim`. Bindings as the table above. `Conveyor1/RunFwd` and `Conveyor1/Fault` `safe_state: false`.

## Logic contract (test oracle)

Clock: monotonic ms. Fast period 20 ms, main 50 ms. Tests may use `VirtualClock`.

| # | Given | When | Then |
|---|--------|------|------|
| L1 | SIM, PullCordOK=true, ChuteBlocked=false, Stop=false | Start rising edge | After **2000 ms** of continuous permissive, RunFwd=true |
| L2 | Conditions of L1 but only 1990 ms elapsed | — | RunFwd still false |
| L3 | RunFwd true | PullCordOK=false | RunFwd false after **one fast invocation** (≤ 20 ms) |
| L4 | PullCordOK=true, ChuteBlocked=true | Start | RunFwd stays false |
| L5 | RunFwd true | Stop=true | RunFwd false on next main (and remains false) |
| L6 | PullCordOK=false | — | Fault=true (fast path) |
| L7 | STOP | — | RunFwd and Fault driven to safe (false) |

Listing must include `.entry task.fast`, `.entry task.main`, `.entry task.slow`, each ending in `HALT`. Use `CALL_FB prim=RS`, `prim=TON`, `prim=R_TRIG` only (no user FBs).

## Golden packing

`assemble(fixture.spasm)` + `PackageBuilder` with the manifest above + `unsigned()` must equal the checked-in `.spkg` (byte-for-byte or parsed-section equality including manifest JCS). CI fails if they drift.
