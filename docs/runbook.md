# Soft PLC runbook (PR-14)

Single process `soft-plc-runtime`. The SIM demo uses **checked-in IR fixtures** (`fixture.spasm` / `fixture.spkg`). There is no on-device ST compiler (PR-15).

## Dev demo (insecure local)

`profile: dev` allows unsigned packages, `auth.required=false`, and plaintext HTTP on loopback. Do not use this on a plant network.

```bash
cargo run -p soft-plc-runtime -- \
  --config samples/configs/sim-plant.yaml \
  --data-dir /tmp/soft-plc \
  --program samples/programs/demo-conveyor/fixture.spkg \
  --mode SIM
```

`--data-dir` remaps `paths.programs` / `retain` / `audit` so the process does not need `/var/lib/soft-plc`.

REST defaults to `http://127.0.0.1:8443`.

### Operator sequence

Tag names use `/`; percent-encode in URLs.

```bash
# Health / status
curl -sS http://127.0.0.1:8443/api/v1/health
curl -sS http://127.0.0.1:8443/api/v1/status

# Permissives (SIM %I inject)
curl -sS -X PUT http://127.0.0.1:8443/api/v1/tags/Conveyor1%2FPullCordOK \
  -H 'content-type: application/json' -d '{"value":true}'
curl -sS -X PUT http://127.0.0.1:8443/api/v1/tags/Conveyor1%2FBeltSlipOK \
  -H 'content-type: application/json' -d '{"value":true}'
curl -sS -X PUT http://127.0.0.1:8443/api/v1/tags/Conveyor1%2FChuteBlocked \
  -H 'content-type: application/json' -d '{"value":false}'

# Start (TON 1 s) then read RunFwd
curl -sS -X PUT http://127.0.0.1:8443/api/v1/tags/Conveyor1%2FStartCmd \
  -H 'content-type: application/json' -d '{"value":true}'
sleep 1.2
curl -sS http://127.0.0.1:8443/api/v1/tags/Conveyor1%2FRunFwd

# Stop
curl -sS -X PUT http://127.0.0.1:8443/api/v1/tags/Conveyor1%2FStopCmd \
  -H 'content-type: application/json' -d '{"value":true}'
```

`PUT /tags/{name}` on `%I` is **SIM only**. `%Q` writes remain force overlays.

### Optional MQTT

`samples/configs/sim-plant.yaml` has `telemetry.enabled: true` and `mqtt://127.0.0.1:1883`. If no broker is running, REST and scan still start; the MQTT worker retries. Node topic:

`spBv1.0/plantA/NDATA/softplc-sim-01`

Device metrics follow the demo tag names (`Conveyor1/RunFwd`, …).

## Prod profile

Set `profile: prod` in device YAML. Validation already requires:

- `program.require_signature: true`
- `auth.required: true`
- TLS cert/key (`auth.tls_cert_path` / `tls_key_path`); plaintext HTTP is refused

The unsigned demo `.spkg` will **not** arm. Sign packages with the PR-09 Ed25519 tooling. Remaining refuse-insecure hardening is PR-20.

## Process-death

SIM has no field actuators. Production deployments must still provide remote I/O heartbeat / hardware fail-safe as in `docs/architecture.md` (userspace crash must not leave outputs energized).

## SCHED_FIFO

The scan thread requests `SCHED_FIFO` and optional `scan.cpu_affinity`. If unprivileged, it logs a warning and continues as CFS. Full PREEMPT_RT guidance is PR-19.
