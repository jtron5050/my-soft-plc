# plc-io-modbus

Non-RT Modbus TCP poll worker for the soft PLC (PR-16, KD-20). The scan thread only copies a sequence-numbered snapshot. Sockets stay on this worker.

## Addressing

Bindings use 5-digit Modicon references. `register_type` is inferred when omitted and must agree when set.

| Reference | Table | PDU address |
|-----------|--------|-------------|
| 1–9999 | coil | `register - 1` |
| 10001–19999 | discrete input | `register - 10001` |
| 30001–39999 | input register | `register - 30001` |
| 40001–49999 | holding register | `register - 40001` |

`40001` is holding PDU 0. Values outside those ranges, including `0` and 6-digit references, are map errors. 32-bit values are big-endian (high word at the lower address). There is no byte-swap or word-swap setting.

## Map

```yaml
- id: remote_rack_a
  driver: modbus_tcp
  config:
    endpoint: "192.168.10.20:502"  # host:port, required
    unit: 1                          # 1..=255, default 1
    poll_ms: 50                      # default 100
    stale_ms: 150                    # default 3 × poll_ms
    timeout_ms: 50                   # default poll_ms
  on_bad_quality: force_safe         # or hold_last
```

Inputs use `eng = raw * scale + offset`, then optional clamp. Outputs invert scale and saturate into the raw type. `scale: 0` on an output is rejected.

Coils and discrete inputs are BOOL. A BOOL holding or input register needs `bit` (0 = least significant bit). `TIME` and `%M` are rejected.

## Quality and fail-safe

Until the first successful poll, inputs are Bad and values are type defaults. Timeout, exception, disconnect, or age greater than `stale_ms` sets that module Bad and holds the last value.

`force_safe` (and Bad quality with `on_bad_quality: force_safe`) writes each output's `safe_state`. `hold_last` sends no write while the module is Bad. STOP and FAULT still write `safe_state`. SIM publishes no field writes and does not overlay field inputs onto the process image.

This driver does not pulse a vendor watchdog register. Production racks must de-energize outputs when the TCP session drops. Closing the socket on `stop` is the master's side of that contract.

Function codes: 1, 2, 3, 4, 5, 6, 15, and 16. Adjacent points are coalesced. EtherCAT, PROFINET, and Modbus RTU are out of scope.
