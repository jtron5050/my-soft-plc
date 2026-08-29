# Sparkplug B 3.0 telemetry contract (v1)

Normative MQTT egress for the soft PLC. Frozen for PR-13 / PR-14. Source of
product decisions: `docs/architecture.md` (KD-7, KD-19, KD-21).

This crate (`plc-telemetry`) is **non-RT**. The scan thread only enqueues
`TelemetrySample` values on a drop-oldest SPSC (`plc-scan`). The publisher
never blocks that path.

## Non-goals

- No WebSocket server (KD-21).
- No OPC UA.
- No Sparkplug primary-host `STATE` topic.
- No process-lifetime persistence of `bdSeq` (in-memory; restarts at 0).
- Eclipse Tahu `sparkplug_b.proto` is **not** vendored (EPL-2.0). The encoder
  uses the same proto2 field numbers, written in-tree.

## Identity and topics

Role: **Edge Node + one Device**.

| Token | Config |
|-------|--------|
| `group_id` | `telemetry.group_id` |
| `edge_node_id` | `device.id` |
| `device_id` | `telemetry.device_id` |

Namespace: `spBv1.0`.

| Verb | Topic |
|------|--------|
| NBIRTH | `spBv1.0/{group}/NBIRTH/{edge}` |
| NDATA | `spBv1.0/{group}/NDATA/{edge}` |
| NDEATH | `spBv1.0/{group}/NDEATH/{edge}` (MQTT Will) |
| NCMD | `spBv1.0/{group}/NCMD/{edge}` |
| DBIRTH | `spBv1.0/{group}/DBIRTH/{edge}/{device}` |
| DDATA | `spBv1.0/{group}/DDATA/{edge}/{device}` |

The architecture one-liner `spBv1.0/plantA/NDATA/softplc-01/line` is
**token-order illustration**. NDATA must **not** include `device_id`
(Sparkplug: device id is only legal on D* verbs). Host tools (Ignition,
Node-RED) expect the table above.

IDs must not contain `/`.

## MQTT 5 session

| Item | Value |
|------|--------|
| Clean start | `false` |
| Session expiry | `3600` s |
| Keep-alive | `30` s |
| Client id | `device.id` |
| Broker URL | `telemetry.broker_url` (`mqtt://` or `mqtts://`) |
| QoS | **1** on NBIRTH/NDATA/NDEATH/DBIRTH/DDATA/NCMD (including Will) |
| Retain | `false` (no STATE) |

**QoS note:** Eclipse Sparkplug TCK wants QoS 0 for non-STATE messages. This
product's frozen contract is QoS 1. Follow this document.

If rumqttc auto-reconnects without a new CONNECT, the Will `bdSeq` may lag the
in-memory counter. `TelemetryService::run` rebuilds the client (new Will) when
the event loop returns an error. On `SessionStateMismatch` after a process
restart while the broker still holds the 3600 s session, recreate the event
loop and treat it as a new Sparkplug session (`bdSeq++`).

TLS: `mqtts://` uses rumqttc's rustls default (native roots). No extra YAML
keys in PR-13.

## Node vs device metrics

**Node** (NBIRTH / NDATA / NDEATH):

| Name | Type | When |
|------|------|------|
| `bdSeq` | UInt64 | Birth and death (Will) |
| `Node Control/Rebirth` | Boolean | Birth (`false`); writable via NCMD |
| `SYSTEM/Mode` | String (`STOP`/`RUN`/`FAULT`/`SIM`) | Birth + change |
| `telemetry_drops` | Int64 | Birth + when the scan SPSC drop counter changes |

Node DATA metrics include **names** (few of them).

**Device** (DBIRTH / DDATA): process-image tags from `TagCatalog`.

- Aliases: sort metric **names** lexicographically; assign `u32` starting at **1**.
- DBIRTH: `name` + `alias` + `datatype` + default value + properties.
- DDATA: **alias only** (no name) + live value + properties.
- Unknown `(is_input, tag_hint)` samples are dropped.
- Empty catalog → no DBIRTH/DDATA until `TelemetryService::set_catalog`
  (PR-14 maps `TagEntry` + io-map `unit` after activate).

`TagCatalog::from_image_slots` names tags `I{n}` / `Q{n}` for tests / pre-arm.

## Type map

| PLC | Sparkplug DataType | Protobuf value |
|-----|--------------------|----------------|
| BOOL | Boolean (11) | `boolean_value` |
| INT | Int16 (2) | `int_value` |
| DINT | Int32 (3) | `int_value` |
| REAL | Float (9) | `float_value` |
| TIME | Int32 (3), milliseconds | `int_value` |
| LINT | Int64 (4) | `long_value` (catalog only) |
| STRING | String (12) | `string_value` (`SYSTEM/Mode` only) |

## Properties

| Key | Type | Rule |
|-----|------|------|
| `Quality` | Int32 | OPC DA: Good=192, Uncertain=64, Bad=0 |
| `Forced` | Boolean | Present and `true` when a maintenance force overlay is active |
| `engUnit` | String | Birth only, when the catalog `unit` is non-empty |

KD-19: Sparkplug timestamps are **Unix ms from the system/NTP clock**, not
scan `now_ms` (monotonic). If `ntp_adjtime` reports `STA_UNSYNC` (or the
query fails), published metric quality is at least Uncertain (never Good).
Bad remains Bad.

## Sequence numbers

- `bdSeq`: starts at 0; increments on every new MQTT session (reconnect).
  In-memory only. NBIRTH and NDEATH Will **must** carry the same `bdSeq`.
- Node `seq` and device `seq`: `u8`, NBIRTH/DBIRTH always `0`, then increment
  on each NDATA/DDATA, wrapping 255 → 0.
- NCMD `Node Control/Rebirth = true`: republish NBIRTH + DBIRTH, reset both
  seq counters, **keep** `bdSeq`.

## Backpressure

1. Scan SPSC: drop **oldest**, count `TelemetrySource::drops()` /
   `ScanStatusSnapshot.telemetry_drops`. Scan `step` never waits.
2. MQTT client channel full: drop **this publish batch**, count
   `Publisher::mqtt_drops()`. Drain `try_recv` continues.

CoS / analog period (20 ms digital min, 500 ms analog default) is applied
**on the scan thread** already (`plc-scan`). This crate does not re-filter.

## `telemetry.enabled = false`

`TelemetryService::run` returns immediately and publishes nothing.
