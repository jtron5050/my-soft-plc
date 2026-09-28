# demo-conveyor

SIM plant demo (architecture PR-14/PR-15).

| Artifact | Role |
|----------|------|
| `fixture.spasm` / `fixture.spkg` | Hand-written IR oracle (PR-14 runtime default) |
| `src/main.st` + `project.toml` | Appendix B ST sources (PR-15) |

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

```bash
cargo run -p plc-compiler -- compile samples/programs/demo-conveyor/project.toml -o /tmp/demo-conveyor.spkg
cargo test -p plc-compiler --test compile_samples
cargo test -p plc-runtime demo_conveyor_spkg_matches_spasm
```
