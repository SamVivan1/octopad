#!/usr/bin/env bash
# M0 measurement: measure total RSS (MB) of the Tauri app (or any process)
# on Linux. Identical method to mem_baseline.sh (sum of RSS of the target
# process and all descendants, one sample after an idle wait) so the numbers
# are directly comparable.
#
# Usage:
#   ./mem_measure.sh [process-pattern] [idle-seconds]
#
# Defaults: process-pattern="octopad" idle-seconds=4
# Examples:
#   ./mem_measure.sh octopad-tauri          # debug/dev binary name
#   ./mem_measure.sh OctoPad                # release bundle binary name
#
# One-line output (machine-readable):
#   octopad_mem_mb=142.0 procs=2 pattern=octopad-tauri idle_s=4 ts=<ISO8601>

set -euo pipefail

PATTERN="${1:-octopad}"
IDLE="${2:-4}"

mapfile -t ROOTS < <(pgrep -f -- "$PATTERN" || true)
if [[ ${#ROOTS[@]} -eq 0 ]]; then
  echo "error: no process matches pattern '$PATTERN' (start the app first)" >&2
  exit 1
fi

pids=()
queue=("${ROOTS[@]}")
while [[ ${#queue[@]} -gt 0 ]]; do
  pid="${queue[0]}"; queue=("${queue[@]:1}")
  pids+=("$pid")
  while read -r child; do
    [[ -n "$child" ]] && queue+=("$child")
  done < <(ps -eo pid=,ppid= | awk -v p="$pid" '$2 == p {print $1}')
done

sleep "$IDLE"

total_kb=0
for pid in "${pids[@]}"; do
  rss_kb="$(awk '/^VmRSS:/ {print $2}' "/proc/$pid/status" 2>/dev/null || echo 0)"
  total_kb=$((total_kb + ${rss_kb:-0}))
done

mb=$(awk -v kb="$total_kb" 'BEGIN {printf "%.1f", kb / 1024}')
ts=$(date -Iseconds)
echo "octopad_mem_mb=$mb procs=${#pids[@]} pattern=$PATTERN idle_s=$IDLE ts=$ts"
