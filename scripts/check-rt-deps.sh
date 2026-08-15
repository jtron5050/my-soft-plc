#!/usr/bin/env bash
# Fail if any declared RT-path crate (see architecture / plc-types::rt_path)
# gains a forbidden direct dependency. Safe when those crates do not exist yet.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

RT_CRATES=(plc-scan plc-vm plc-fb-primitives plc-types plc-ir) # plc-vm is RT-callable
FORBIDDEN=(tokio tokio-util hyper hyper-util reqwest axum warp actix-web mio socket2 rustls native-tls openssl rumqttc paho-mqtt)

fail=0

for crate in "${RT_CRATES[@]}"; do
  manifest="crates/${crate}/Cargo.toml"
  if [[ ! -f "$manifest" ]]; then
    # Crate not in the workspace yet — skip.
    continue
  fi
  # Direct dependency names from the package manifest (rough but PR-01 friendly).
  deps=$(
    awk '
      /^\[dependencies\]/ { in_deps=1; next }
      /^\[/ { in_deps=0 }
      in_deps && /^[a-zA-Z0-9_-]+/ {
        split($0, a, /[ =]/)
        print a[1]
      }
    ' "$manifest"
  )
  for bad in "${FORBIDDEN[@]}"; do
    if printf '%s\n' "$deps" | grep -qx "$bad"; then
      echo "error: RT-path crate '${crate}' must not depend on '${bad}'" >&2
      fail=1
    fi
  done
done

if [[ "$fail" -ne 0 ]]; then
  exit 1
fi

echo "RT-path dependency check OK (scanned existing RT crates only)."
