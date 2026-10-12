#!/usr/bin/env bash
#
# Run the same checks that CI runs, locally:
#   - pyflakes + compileall for both desktop apps
#   - pytest suite
#   - firmware compile (skipped unless arduino-cli is available, pass --firmware
#     to fail when it is missing)
#
# Usage:
#   scripts/check.sh                 # python checks + firmware if arduino-cli exists
#   scripts/check.sh --no-firmware   # skip the firmware compile
#   scripts/check.sh --firmware      # require arduino-cli and compile firmware
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

FIRMWARE=auto
for arg in "$@"; do
  case "$arg" in
    --no-firmware) FIRMWARE=off ;;
    --firmware) FIRMWARE=on ;;
    -h|--help) sed -n '2,8p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) echo "Unknown option: $arg" >&2; exit 2 ;;
  esac
done

PY="${PYTHON:-python3}"

echo "==> Python checks (Linux app)"
(cd "$REPO_ROOT/linux-app" && "$PY" -m pyflakes ./*.py ./ui/*.py && "$PY" -m compileall -q .)
echo "==> Python checks (Windows app)"
(cd "$REPO_ROOT/windows-app" && "$PY" -m pyflakes ./*.py ./ui/*.py && "$PY" -m compileall -q .)

echo "==> Test suite"
(cd "$REPO_ROOT" && "$PY" -m pytest -q)

if [ "$FIRMWARE" = off ]; then
  echo "==> Firmware compile skipped"
  exit 0
fi

if command -v arduino-cli >/dev/null 2>&1; then
  echo "==> Firmware compile (esp32:esp32:esp32c3)"
  arduino-cli compile --fqbn "${FQBN:-esp32:esp32:esp32c3}" "$REPO_ROOT"
else
  if [ "$FIRMWARE" = on ]; then
    echo "arduino-cli not found; install it to compile firmware." >&2
    exit 1
  fi
  echo "==> arduino-cli not found — skipping firmware compile (use --firmware to require it)"
fi

echo "==> All checks passed"
