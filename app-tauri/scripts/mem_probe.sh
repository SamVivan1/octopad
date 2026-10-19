#!/usr/bin/env bash
# Measure the real memory footprint of a running OctoPad instance.
#
# Why this exists: the naive `ps` RSS number misleads for a Tauri/WebKitGTK app,
# which runs several processes (main, WebKitWebProcess, WebKitNetworkProcess,
# sometimes a GPU process). Summing RSS counts pages shared between those
# processes more than once, so a summed RSS can look several times worse than the
# physical memory actually used. PSS (proportional set size) splits shared pages
# among their users, which is the honest number to compare against another app.
#
# Usage:
#   scripts/mem_probe.sh [--label NAME] -- <command> [args...]
# Examples:
#   scripts/mem_probe.sh -- app-tauri/src-tauri/target/release/octopad-tauri
#   WEBKIT_DISABLE_DMABUF_RENDERER=1 scripts/mem_probe.sh -- .../octopad-tauri
#
# Environment: IDLE_S (settle seconds, default 8), REPEATS (default 3),
# PROBE_PATTERN (process name to find, default octopad-tauri).
set -uo pipefail

LABEL="run"
IDLE_S="${IDLE_S:-8}"
REPEATS="${REPEATS:-3}"

if [[ "${1:-}" == "--label" ]]; then LABEL="$2"; shift 2; fi
if [[ "${1:-}" == "--" ]]; then shift; fi

if [[ $# -eq 0 ]]; then
  echo "usage: mem_probe.sh [--label NAME] -- <command> [args...]" >&2
  exit 2
fi

PATTERN="${PROBE_PATTERN:-octopad-tauri}"

# Match by *executable name* and walk the child tree. A `pgrep -f` pattern would
# also match the shell that merely mentions the binary on its command line, which
# is how a probe ends up measuring (and killing) its own parent.
pids() {
  python3 - "$PATTERN" <<'PY'
import os, sys
name = sys.argv[1]
procs = {}
for entry in os.listdir("/proc"):
    if not entry.isdigit():
        continue
    try:
        with open(f"/proc/{entry}/stat") as handle:
            data = handle.read()
        close = data.rindex(")")           # comm may contain spaces or parens
        comm = data[data.index("(") + 1:close]
        ppid = int(data[close + 2:].split()[1])
    except Exception:
        continue
    procs[int(entry)] = (comm, ppid)

roots = [pid for pid, (comm, _) in procs.items() if comm == name]
if not roots:
    sys.exit(0)
found = set(roots)
frontier = set(roots)
for _ in range(3):                        # WebKit web/network/gpu processes
    nxt = {pid for pid, (_, ppid) in procs.items() if ppid in frontier and pid not in found}
    if not nxt:
        break
    found |= nxt
    frontier = nxt
print("\n".join(str(pid) for pid in sorted(found)))
PY
}

# Prints the per-process breakdown on stderr and ONE summary line
# ("<count> <rss_kb> <pss_kb>") on stdout, so a caller reading stdout gets the
# totals without having to filter the detail out.
report() {
  local total_rss=0 total_pss=0 count=0
  local pid rss pss comm
  for pid in $(pids); do
    rss=0; pss=0; comm="?"
    if [[ -r "/proc/$pid/smaps_rollup" ]]; then
      rss=$(awk '/^Rss:/ {print $2; exit}' "/proc/$pid/smaps_rollup" 2>/dev/null || true)
      pss=$(awk '/^Pss:/ {print $2; exit}' "/proc/$pid/smaps_rollup" 2>/dev/null || true)
      rss=${rss:-0}; pss=${pss:-0}
      comm=$(ps -o comm= -p "$pid" 2>/dev/null | head -1)
      comm=${comm:-?}
    fi
    printf '      pid %-7s %-16s rss %6s MiB   pss %6s MiB\n' \
      "$pid" "${comm:0:16}" "$((rss / 1024))" "$((pss / 1024))" >&2
    total_rss=$((total_rss + rss))
    total_pss=$((total_pss + pss))
    count=$((count + 1))
  done
  echo "$count $total_rss $total_pss"
}

echo "== $LABEL"
overrides=$(env | grep -E '^(WEBKIT_|LIBGL_|GDK_)' | cut -d= -f1 | tr '\n' ' ')
echo "   webkit/gl overrides: ${overrides:-none}"

for run in $(seq 1 "$REPEATS"); do
  if [[ -z "$(pids)" ]]; then
    "$@" >/dev/null 2>&1 &
    disown 2>/dev/null || true
  fi
  sleep "$IDLE_S"

  worst_pss=0; worst_rss=0; worst_count=0
  for _ in 1 2 3; do                    # keep the worst sample, not a lucky one
    read -r c r ps <<<"$(report)"
    if (( ps > worst_pss )); then worst_pss=$ps; worst_rss=$r; worst_count=$c; fi
    sleep 1
  done

  printf '   run %s: %s processes | RSS sum %6s MiB | PSS sum %6s MiB\n' \
    "$run" "$worst_count" "$((worst_rss / 1024))" "$((worst_pss / 1024))"
  report >/dev/null
done

echo "   WORST for $LABEL: PSS sum $((worst_pss / 1024)) MiB  (RSS sum $((worst_rss / 1024)) MiB)"
