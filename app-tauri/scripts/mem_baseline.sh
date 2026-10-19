#!/usr/bin/env bash
# M0 baseline: measure total RSS (MB) of a RUNNING legacy Python OctoPad app
# on Linux. Use the same method as mem_measure.sh so numbers are comparable:
# sum of RSS of the target process and all its descendants, sampled once
# after an idle wait.
#
# Usage:
#   ./mem_baseline.sh [process-pattern] [idle-seconds]
#
# Defaults: process-pattern="linux-app/main.py" idle-seconds=4
# The pattern is matched with `pgrep -f`, so it must uniquely identify the
# legacy app's main process (not this shell).
#
# One-line output (machine-readable):
#   octopad_mem_mb=123.4 procs=3 pattern=linux-app/main.py idle_s=4 ts=<ISO8601>

set -euo pipefail

PATTERN="${1:-linux-app/main.py}"
IDLE="${2:-4}"

# Resolve the root PID(s) matching the pattern.
mapfile -t ROOTS < <(pgrep -f -- "$PATTERN" || true)
if [[ ${#ROOTS[@]} -eq 0 ]]; then
  echo "error: no process matches pattern '$PATTERN' (is the legacy app running?)" >&2
  exit 1
fi

# Collect each root plus all descendants (BFS over /proc ppid).
pids=()
queue=("${ROOTS[@]}")
while [[ ${#queue[@]} -gt 0 ]]; do
  pid="${queue[0]}"; queue=("${queue[@]:1}")
  pids+=("$pid")
  while read -r child; do
    [[ -n "$child" ]] && queue+=("$child")
  done < <(ps -eo pid=,ppid= | awk -v p="$pid" '$2 == p {print $1}')
done

# Wait for idle (window shown, settle), then sample RSS once.
sleep "$IDLE"

total_kb=0
for pid in "${pids[@]}"; do
  rss_kb="$(awk '/^VmRSS:/ {print $2}' "/proc/$pid/status" 2>/dev/null || echo 0)"
  total_kb=$((total_kb + ${rss_kb:-0}))
done

mb=$(awk -v kb="$total_kb" 'BEGIN {printf "%.1f", kb / 1024}')
ts=$(date -Iseconds)
echo "octopad_mem_mb=$mb procs=${#pids[@]} pattern=$PATTERN idle_s=$IDLE ts=$ts"
