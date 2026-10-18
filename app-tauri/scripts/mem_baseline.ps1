# M0 baseline: measure total working set (MB) of a RUNNING legacy Python
# OctoPad app on Windows. Same method as mem_measure.ps1: sum of
# WorkingSet64 of the target process and all its descendants, sampled once
# after an idle wait.
#
# Usage:
#   .\mem_baseline.ps1 [-ProcessName python] [-IdleSeconds 4]
#
# -ProcessName defaults to "python" (the legacy app runs via python.exe /
# pythonw.exe). If several python processes are unrelated to OctoPad, stop
# them first or pass a more specific name without the .exe extension.
#
# One-line output (machine-readable):
#   octopad_mem_mb=210.3 procs=2 pattern=python idle_s=4 ts=<ISO-8601>

param(
    [string]$ProcessName = "python",
    [int]$IdleSeconds = 4
)

$ErrorActionPreference = "Stop"

# Build the process tree: all processes + parent links via CIM.
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
    Write-Error "no process matches name '$ProcessName*' (is the legacy app running?)"
    exit 1
}

# BFS over the parent links to collect the whole tree.
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
