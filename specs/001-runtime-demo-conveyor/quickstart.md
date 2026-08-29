# Quickstart: SIM conveyor demo

Validation guide for PR-14. Full operator text belongs in `docs/runbook-sim-demo.md` (implementation deliverable). Tag names, CLI, and REST/MQTT deltas: [contracts/](contracts/).

## Prerequisites

- Linux x86_64, Rust **1.85** (workspace toolchain)
- Repo root as cwd
- Optional: MQTT broker on `127.0.0.1:1883` (demo works without it)
- **No** Structured Text compiler

```bash
cargo test --workspace
cargo build -p plc-runtime --bin soft-plc-runtime
```

## Lab start

```bash
mkdir -p /tmp/soft-plc-demo
./target/debug/soft-plc-runtime \
  --config samples/configs/sim-plant.yaml \
  --data-dir /tmp/soft-plc-demo
```

Expect: process stays up, stderr/log shows listen `127.0.0.1:8443` and `profile=dev`. `GET http://127.0.0.1:8443/api/v1/health` → 200. Mode STOP, no field drivers.

## Install demo (no compiler)

```bash
curl -sS -D- -o /tmp/prog.json \
  -X POST http://127.0.0.1:8443/api/v1/programs \
  --data-binary @samples/programs/demo-conveyor/fixture.spkg
curl -sS -X POST http://127.0.0.1:8443/api/v1/programs/demo-conveyor/arm
curl -sS -X POST http://127.0.0.1:8443/api/v1/programs/demo-conveyor/activate
# poll until current id is demo-conveyor and phase is idle
curl -sS http://127.0.0.1:8443/api/v1/status
curl -sS -X POST http://127.0.0.1:8443/api/v1/mode \
  -H 'content-type: application/json' \
  -d '{"mode":"SIM"}'
```

## Observe conveyor logic

Healthy permissives and start (URL-encoded `/`):

```bash
curl -sS -X PUT http://127.0.0.1:8443/api/v1/tags/Conveyor1%2FPullCordOK \
  -H 'content-type: application/json' -d '{"value":true}'
curl -sS -X PUT http://127.0.0.1:8443/api/v1/tags/Conveyor1%2FStart \
  -H 'content-type: application/json' -d '{"value":true}'
sleep 2.2
curl -sS http://127.0.0.1:8443/api/v1/tags/Conveyor1%2FRunFwd
# value true
```

Pull-cord drop (fast path):

```bash
curl -sS -X PUT http://127.0.0.1:8443/api/v1/tags/Conveyor1%2FPullCordOK \
  -H 'content-type: application/json' -d '{"value":false}'
curl -sS http://127.0.0.1:8443/api/v1/tags/Conveyor1%2FRunFwd
# value false within one fast period
```

Chute inhibit: Stop, reset, set `ChuteBlocked=true`, Start → `RunFwd` stays false.

STOP → outputs safe. SIGINT → process exits; sim outputs de-energized.

## Telemetry (optional)

If a broker is listening at the sample URL, a Sparkplug subscriber sees `Conveyor1/RunFwd` and `SYSTEM/Mode=SIM` after activate. Repeat the start sequence with the broker stopped: SIM and tag reads still work for ≥ 5 minutes.

## Automated stand-ins

Until the binary exists, `cargo test` must cover the logic oracle in [contracts/demo-conveyor.md](contracts/demo-conveyor.md) (L1–L7) via `VirtualClock` + sim inject, plus spasm/spkg golden.

## Done when

- Single process, no compiler, SIM demo cycles (SC-001/SC-002/SC-006)
- 2 s start delay and 20 ms pull-cord drop-out (SC-003/SC-004)
- Broker-down does not stall scan (SC-005)
- Listing and package stay equivalent (SC-007)
