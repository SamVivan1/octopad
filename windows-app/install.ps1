# OctoPad - Windows installer (PowerShell).
# Creates a virtual environment, installs dependencies and registers shortcuts.
param(
    [switch]$NoAutostart
)

$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $MyInvocation.MyCommand.Path
$Venv = Join-Path $root ".venv"

Write-Host "OctoPad installer" -ForegroundColor Cyan
Write-Host "Project: $root"

# --- Find a Python interpreter --------------------------------------------
$python = $null
foreach ($candidate in @("py -3", "python", "python3")) {
    $parts = $candidate.Split(" ")
    $exe = Get-Command $parts[0] -ErrorAction SilentlyContinue
    if ($exe) { $python = $candidate; break }
}
if (-not $python) {
    Write-Error "Python 3.10+ not found. Install it from https://python.org and retry."
    exit 1
}

# --- Virtual environment --------------------------------------------------
if (-not (Test-Path "$PSScriptRoot\.venv")) {
    Write-Host "Creating virtual environment..."
    & cmd /c "$python -m venv .venv"
}

$venvPython = Join-Path $PSScriptRoot ".venv\Scripts\python.exe"
Write-Host "Installing dependencies..."
& $venvPython -m pip install --upgrade pip
& $venvPython -m pip install -r (Join-Path $PSScriptRoot "requirements.txt")

# --- Start Menu shortcut --------------------------------------------------
$startMenu = [Environment]::GetFolderPath("Programs")
$startLink = Join-Path $startMenu "OctoPad.lnk"
$shell = New-Object -ComObject WScript.Shell
$shortcut = $shell.CreateShortcut($startLink)
$shortcut.TargetPath = $venvPython
$shortcut.Arguments = '"' + (Join-Path $PSScriptRoot "main.py") + '"'
$shortcut.WorkingDirectory = $PSScriptRoot
$shortcut.Description = "OctoPad macropad control"
$shortcut.Save()
Write-Host "Start Menu shortcut created: $startLink"

# --- Autostart shortcut (tray only) --------------------------------------
$startup = [Environment]::GetFolderPath("Startup")
$startupLink = Join-Path $startup "OctoPad.lnk"
$autostart = $shell.CreateShortcut($startupLink)
$autostart.TargetPath = $venvPython
$autostart.Arguments = '"' + (Join-Path $PSScriptRoot "main.py") + '" --hidden'
$autostart.WorkingDirectory = $PSScriptRoot
$autostart.Description = "OctoPad (background/tray)"
$autostart.Save()
Write-Host "Autostart shortcut created: $startupLink"

Write-Host ""
Write-Host "Done. Launch OctoPad from the Start Menu, or run run.bat." -ForegroundColor Green
