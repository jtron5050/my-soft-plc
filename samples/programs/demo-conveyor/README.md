# demo-conveyor

Checked-in IR fixtures for the SIM plant demo (architecture PR-14). **Not** compiled from ST; PR-15 must round-trip this spasm.

| Slot | Tag | Plane |
|------|-----|-------|
| I0 | `Conveyor1/StartCmd` | `%I` |
| I1 | `Conveyor1/StopCmd` | `%I` |
| I2 | `Conveyor1/PullCordOK` | `%I` |
| I3 | `Conveyor1/BeltSlipOK` | `%I` |
| I4 | `Conveyor1/ChuteBlocked` | `%I` |
| I5 | `Conveyor1/LocalMode` | `%I` |
| Q0 | `Conveyor1/RunFwd` | `%Q` |
| Q1 | `Conveyor1/Fault` | `%Q` |
| Q2 | `Conveyor1/Ready` | `%Q` |
| R0 | `Conveyor1/RunHours` | retain REAL |

`fixture.spasm` is the review oracle. `fixture.spkg` is the unsigned packed package; `cargo test -p plc-runtime demo_conveyor_spkg_matches_spasm` checks they match.
