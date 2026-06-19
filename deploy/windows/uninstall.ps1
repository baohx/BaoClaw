# BaoClaw Windows Service - Uninstallation Script
# Run as Administrator
#
# Usage: PowerShell -ExecutionPolicy Bypass -File uninstall.ps1

#Requires -RunAsAdministrator

$ErrorActionPreference = "Stop"
$serviceName = "BaoClawDaemon"

Write-Host "════════════════════════════════════════════════════════════"
Write-Host "  BaoClaw Windows Service - Uninstallation" -ForegroundColor Yellow
Write-Host "════════════════════════════════════════════════════════════"

# Check if service exists
$svc = Get-Service -Name $serviceName -ErrorAction SilentlyContinue
if (-not $svc) {
    Write-Host "Service '$serviceName' is not installed." -ForegroundColor Yellow
    exit 0
}

# Find executable
$exePath = Join-Path $PSScriptRoot "..\..\baoclaw-core\target\release\baoclaw-core.exe"
$exePath = [System.IO.Path]::GetFullPath($exePath)

if (Test-Path $exePath) {
    & $exePath --uninstall-service
} else {
    # Fallback: manual sc delete
    Write-Host "Stopping service..." -ForegroundColor Cyan
    Stop-Service -Name $serviceName -Force -ErrorAction SilentlyContinue
    Start-Sleep -Seconds 2

    Write-Host "Deleting service..." -ForegroundColor Cyan
    & sc.exe delete $serviceName
}

Write-Host ""
Write-Host "[OK] Service uninstalled." -ForegroundColor Green
Write-Host ""
