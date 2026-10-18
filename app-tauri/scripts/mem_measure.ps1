# M0 measurement: measure total working set (MB) of the Tauri app (or any
# process) on Windows. Identical method to mem_baseline.ps1 (sum of
# WorkingSet64 of the target process and all descendants, one sample after
# an idle wait) so the numbers are directly comparable.
#
# Usage:
#   .\mem_measure.ps1 [-ProcessName OctoPad] [-IdleSeconds 4]
#
# -ProcessName defaults to "OctoPad" (the release bundle binary name;
# the debug/dev binary is "octopad-tauri").
#
# One-line output (machine-readable):
#   octopad_mem_mb=155.2 procs=2 pattern=OctoPad idle_s=4 ts=<ISO-8601>

param(
    [string]$ProcessName = "OctoPad",
    [int]$IdleSeconds = 4
)

$ErrorActionPreference = "Stop"

$procs = Get-CimInstance Win32_Process
$byParent = @{}
foreach ($p in $procs) {
    if (-not $byParent.ContainsKey($p.ParentProcessId)) {
        $byParent[$p.ParentProcessId] = @()
    }
    $byParent[$p.ParentProcessId] += $p.ProcessId
}

$roots = @($procs | Where-Object { $_.Name -like "$ProcessName*" } | Select-Object -ExpandProperty ProcessId)
if ($roots.Count -eq 0) {
    Write-Error "no process matches name '$ProcessName*' (start the app first)"
    exit 1
}

$ids = [System.Collections.Generic.HashSet[int]]::new()
$queue = [System.Collections.Generic.Queue[int]]::new()
foreach ($r in $roots) { $queue.Enqueue($r) }
while ($queue.Count -gt 0) {
    $id = $queue.Dequeue()
    if (-not $ids.Add($id)) { continue }
    if ($byParent.ContainsKey($id)) {
        foreach ($child in $byParent[$id]) { $queue.Enqueue($child) }
    }
}

Start-Sleep -Seconds $IdleSeconds

$total = 0
$count = 0
foreach ($id in $ids) {
    $proc = Get-Process -Id $id -ErrorAction SilentlyContinue
    if ($null -ne $proc) {
        $total += $proc.WorkingSet64
        $count++
    }
}

$mb = [math]::Round($total / 1MB, 1)
$ts = (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ssZ")
Write-Output "octopad_mem_mb=$mb procs=$count pattern=$ProcessName idle_s=$IdleSeconds ts=$ts"
