#!/usr/bin/env bash
#
# Build (and optionally flash) the OctoPad ESP32-C3 firmware with arduino-cli.
#
# Usage:
#   scripts/build_firmware.sh                 # compile only
#   scripts/build_firmware.sh --upload        # compile and flash (auto port)
#   scripts/build_firmware.sh --upload --port /dev/ttyACM0
#
# Environment overrides:
#   FQBN        arduino-cli board FQBN (default: esp32:esp32:esp32c3)
#   CORE_VER    ESP32 core version to install (default: 3.3.5)
#   NIMBLE_VER  NimBLE-Arduino version to install (default: 2.5.1)
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SKETCH_DIR="$REPO_ROOT"
BUILD_DIR="$REPO_ROOT/build"

FQBN="${FQBN:-esp32:esp32:esp32c3}"
CORE_VER="${CORE_VER:-3.3.5}"
NIMBLE_VER="${NIMBLE_VER:-2.5.1}"

UPLOAD=0
PORT=""
while [ $# -gt 0 ]; do
  case "$1" in
    --upload) UPLOAD=1 ;;
    --port) PORT="${2:?--port needs a value}"; shift ;;
    -h|--help) sed -n '2,14p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) echo "Unknown option: $1" >&2; exit 2 ;;
  esac
  shift
done

if ! command -v arduino-cli >/dev/null 2>&1; then
  echo "arduino-cli not found." >&2
  echo "Install it first: https://arduino.github.io/arduino-cli/latest/installation/" >&2
  exit 1
fi

echo "==> Ensuring ESP32 core ${CORE_VER} is installed"
arduino-cli core update-index
arduino-cli core install "esp32:esp32@${CORE_VER}"

echo "==> Ensuring NimBLE-Arduino ${NIMBLE_VER} is installed"
arduino-cli lib install "NimBLE-Arduino@${NIMBLE_VER}"

echo "==> Compiling sketch ($FQBN)"
arduino-cli compile --fqbn "$FQBN" --output-dir "$BUILD_DIR" "$SKETCH_DIR"

BIN="$BUILD_DIR/octopad.ino.bin"
if [ -f "$BIN" ]; then
  echo "==> Firmware built: $BIN"
fi

if [ "$UPLOAD" -eq 1 ]; then
  echo "==> Uploading firmware"
  if [ -n "$PORT" ]; then
    arduino-cli upload --fqbn "$FQBN" --port "$PORT" "$SKETCH_DIR"
  else
    arduino-cli upload --fqbn "$FQBN" "$SKETCH_DIR"
  fi
fi
