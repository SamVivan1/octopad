#!/usr/bin/env pwsh
# OctoPad - remove shortcuts (and optionally the virtual environment).

$ErrorActionPreference = "Stop"

$startMenu = Join-Path ([Environment]::GetFolderPath("Programs")) "OctoPad.lnk"
$startup = Join-Path ([Environment]::GetFolderPath("Startup")) "OctoPad.lnk"

foreach ($link in @($startMenu, $startup)) {
    if (Test-Path $link) {
        Remove-Item $link -Force
        Write-Host "Removed $link"
    }
}

$venv = Join-Path $PSScriptRoot ".venv"
if (Test-Path $venv) {
    $answer = Read-Host "Delete the virtual environment at $venv? [y/N]"
    if ($answer -match '^[Yy]$') {
        Remove-Item $venv -Recurse -Force
        Write-Host "Removed $venv"
    }
}

Write-Host "OctoPad uninstalled. Your configuration in %APPDATA%\\OctoPad was kept."
