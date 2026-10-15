# Recreates docs\screenshots from the demo data (nothing real is read or written).
#
#   powershell -ExecutionPolicy Bypass -File scripts\screenshots.ps1 [-Exe target\release\Clipvault.exe]
param([string]$Exe = "target\release\Clipvault.exe")
$ErrorActionPreference = "Stop"
Set-Location (Split-Path -Parent $PSScriptRoot)
$out = Join-Path (Get-Location) "docs\screenshots"
Get-ChildItem $out -Filter *.png -ErrorAction SilentlyContinue | Remove-Item -Force
$p = Start-Process (Resolve-Path $Exe) -ArgumentList "--screenshots", $out -Wait -PassThru
if ($p.ExitCode -ne 0) { throw "screenshot run failed: $($p.ExitCode)" }
Get-ChildItem $out | Format-Table Name, Length
