# Contract: Management and telemetry wiring (PR-14)

This feature **reuses** frozen contracts. Deltas are the only new obligations.

## Management (REST)

Normative: [`docs/openapi/openapi.yaml`](../../../docs/openapi/openapi.yaml).

Demo runbook uses (dev, `auth.required=false`, plaintext HTTP):

| Step | Method | Path | Notes |
|------|--------|------|-------|
| Health | GET | `/api/v1/health` | Unauthenticated |
| Upload | POST | `/api/v1/programs` | Raw `.spkg` body |
| Arm | POST | `/api/v1/programs/demo-conveyor/arm` | Sync 200 |
| Activate | POST | `/api/v1/programs/demo-conveyor/activate` | 202; poll GET `/status` until `program.current.id=demo-conveyor` and `phase=idle` |
| SIM | POST | `/api/v1/mode` | `{"mode":"SIM"}` from STOP only |
| Tags | GET | `/api/v1/tags` | Must include Conveyor1/* after activate |
| Inject / force | PUT | `/api/v1/tags/{name}` | See delta |

URL-encode `/` in tag names (`Conveyor1%2FStart`).

### Delta — PUT `/tags/{name}` on `%I`

Existing OpenAPI describes force writes. PR-14 behavior:

- `%Q`: unchanged maintenance force overlay (`forced: true`).
- `%I`: when the live driver is **sim**, the write **injects** the sim input. Response may use `"forced": false` and a distinct field is **not** required; runbook treats it as “set simulated input.” Reject `%I` writes when the driver is not sim (400).
- Permissions: same as tag force (`operator`+). In dev with `auth.required=false`, anonymous Admin may call it.

Do **not** add a WebSocket path.

## Telemetry (Sparkplug B 3.0)

Normative: [`docs/sparkplug.md`](../../../docs/sparkplug.md).

### Delta — catalog after activate

When epoch install succeeds, the supervisor MUST `TelemetryHandle::set_catalog` with `%I`/`%Q` TagEntry rows:

- Metric **name** = tag name (`Conveyor1/RunFwd`, …).
- Aliases assigned lexicographically starting at 1.
- `engUnit` from io-map `unit` when non-empty.
- Replacing a non-empty catalog while born → DDEATH then DBIRTH (existing publisher rule).

Pre-arm, catalog may be empty or `I{n}`/`Q{n}` fallback; consumers should wait until after activate.

### Broker down

Process starts, scan and REST live. Publisher does not consume the scan SPSC until CONNACK (existing). Overflow increments `telemetry_drops`. No start failure.

## Thread / isolation contract

| Thread | Allowed |
|--------|---------|
| `plc-scan` | `run_due`, bounded memcpy to retain snapshot + telemetry SPSC; `try_lock` only |
| tokio REST | axum; brief `Mutex<Runtime>` without `.await` while held |
| tokio MQTT | `TelemetryService::run` |
| tokio retain | `RetainStore::flush` after snapshot read |

Forbidden: tokio/network crates on RT-path crate manifests; logging/format on the scan thread.
