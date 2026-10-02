# plc-io-gpio

In-RT Linux GPIO driver for digital inputs and outputs (PR-17, KD-5a). The scan thread calls `poll_inputs` and `apply_outputs` directly. Each call issues one [GPIO character-device](https://docs.kernel.org/userspace-api/gpio/chardev.html) ioctl per module (`GPIO_V2_LINE_GET_VALUES` / `GPIO_V2_LINE_SET_VALUES`). Those paths do not allocate. Kernel 5.10 or newer (uAPI v2). libgpiod and sysfs GPIO are not used.

Sleeping controllers (I2C or SPI expanders) are out of scope. The WCET assumption is that the chip's get/set ioctl does not sleep. There is no measured microsecond budget here; PR-19 is the timing harness.

## Map

```yaml
- id: local_di_1
  driver: gpio
  config:
    chip: gpiochip0          # or /dev/gpiochip0
    lines: [0, 1, 2, 3]      # 1..=64 offsets
    active_low: false        # optional, default false
    bias: as_is              # as_is | pull_up | pull_down | disabled
  bindings:
    - tag: Conveyor1.PullCordOK
      image: I
      type: BOOL
      bit: 0                 # index into lines
- id: local_do_1
  driver: gpio
  config:
    chip: gpiochip1
    lines: [0, 1]
    drive: open_drain        # default; or push_pull
  on_bad_quality: force_safe # or hold_last
  bindings:
    - tag: Conveyor1.RunFwd
      image: Q
      bit: 0
      safe_state: false
```

`bit` selects `lines[bit]`. Omit `bit` to take the next free index. A module is all `%I` or all `%Q`. Bindings are BOOL. Registers, scale, and offset are rejected. The same chip offset cannot appear twice. Lines listed without a binding are still requested; outputs among them are held inactive.

## Quality and fail-safe

A successful read is Good. A failed read keeps the last sample (false until the first success), marks that module Bad, and does not fail the scan. A failed write marks that output module Bad. The next scan applies that module's `on_bad_quality`. A bad input module does not mark a different output module Bad. `hold_last` skips output ioctls while the module stays Bad. STOP, FAULT, or `force_safe` still writes `safe_state`.

Outputs are requested already at `safe_state`. The default drive is open-drain. In SIM the driver writes `safe_state` and does not copy pin reads over injected inputs.

`stop` writes `safe_state` and closes the request fd. Process death closes that fd too, and the kernel releases the lines. Release returns the pin to the controller default. Open-drain or de-energize-on-float hardware is what makes that electrically safe. A systemd `ExecStop=` write is not the fail-safe. Push-pull with an external pull-up can stay energized after release.

## Bench check

Jumper two lines on an x86 gpiochip, bind one as DO and one as DI, and confirm the input follows the output. Enter FAULT, or kill the process, and confirm the output is inactive. This is not part of CI.
