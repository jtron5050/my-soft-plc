# Contract: `soft-plc-runtime` CLI

Process name: **`soft-plc-runtime`** (Cargo bin in `crates/plc-runtime`).

## Invocation

```text
soft-plc-runtime --config <path> [--data-dir <dir>]
```

| Argument | Required | Meaning |
|----------|----------|---------|
| `--config <path>` | yes | Device YAML/JSON (schema v1). Typical: `samples/configs/sim-plant.yaml` |
| `--data-dir <dir>` | no | Create/use `<dir>/programs`, `<dir>/retain`, `<dir>/audit` instead of `paths.*` in the file |

Unknown flags or missing `--config` → stderr message, exit **2**.

## Start behavior

1. Load and validate config. Failure → stderr, exit **1**.
2. If `io.drivers` ≠ `[sim]` only → stderr (“this build supports sim I/O only”), exit **1**.
3. Load io-map from `paths.io_map` (relative to process cwd). Failure → exit **1**.
4. Bind `rest.bind`. Address in use → exit **1**.
5. Spawn scan thread, REST, telemetry worker, retain flusher.
6. If program store `current` pointer exists, arm+activate that package; mode remains **STOP**.
7. Log (non-RT) listen address and profile.

## Shutdown

SIGINT / SIGTERM: stop accepting REST, request scan thread stop, one retain flush, join scan thread, exit **0**. Hung join after a few seconds still exits **0** after best-effort safe outputs (sim de-energize).

## Non-goals

- No subcommands (`run`, `pack`, …). Packing the demo package is a **test/golden** path, not an operator command.
- No compiler invocation.
- No `--mode SIM` auto-start (operator uses `POST /api/v1/mode`).
