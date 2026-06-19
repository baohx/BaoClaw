# BaoClaw Windows Service - Installation Script
# Run as Administrator
#
# Usage: PowerShell -ExecutionPolicy Bypass -File install.ps1

#Requires -RunAsAdministrator

$ErrorActionPreference = "Stop"
$serviceName = "BaoClawDaemon"

Write-Host "════════════════════════════════════════════════════════════"
Write-Host "  BaoClaw Windows Service - Installation" -ForegroundColor Cyan
Write-Host "════════════════════════════════════════════════════════════"

# 1. Find executable
$exePath = Join-Path $PSScriptRoot "..\..\baoclaw-core\target\release\baoclaw-core.exe"
$exePath = [System.IO.Path]::GetFullPath($exePath)

if (-not (Test-Path $exePath)) {
    Write-Host "[X] Daemon executable not found at: $exePath" -ForegroundColor Red
    Write-Host "  Please build first: cd baoclaw-core && cargo build --release" -ForegroundColor Yellow
    exit 1
}

Write-Host "[OK] Found executable: $exePath" -ForegroundColor Green

# 2. Check if service already exists
$existing = Get-Service -Name $serviceName -ErrorAction SilentlyContinue
if ($existing) {
    Write-Host "[!] Service '$serviceName' already exists." -ForegroundColor Yellow
    $response = Read-Host "Uninstall existing service first? (y/N)"
    if ($response -eq 'y' -or $response -eq 'Y') {
        & $exePath --uninstall-service
    } else {
        Write-Host "Aborted." -ForegroundColor Red
        exit 1
    }
}

# 3. Install via the binary's self-install
Write-Host "Installing service..." -ForegroundColor Cyan
& $exePath --install-service
if ($LASTEXITCODE -ne 0) {
    Write-Host "[X] Installation failed." -ForegroundColor Red
    exit 1
}

# 4. Start
Write-Host "Starting service..." -ForegroundColor Cyan
Start-Service -Name $serviceName
Start-Sleep -Seconds 2

# 5. Verify
$svc = Get-Service -Name $serviceName
if ($svc.Status -eq 'Running') {
    Write-Host "[OK] Service is running." -ForegroundColor Green
} else {
    Write-Host "[!] Service status: $($svc.Status)" -ForegroundColor Yellow
}

Write-Host ""
Write-Host "════════════════════════════════════════════════════════════"
Write-Host "  Installation Complete" -ForegroundColor Green
Write-Host "════════════════════════════════════════════════════════════"
Write-Host ""
Write-Host "Service name:   $serviceName"
Write-Host "Display name:   $($svc.DisplayName)"
Write-Host "Auto-start:     Yes (on boot)"
Write-Host "Socket path:    $env:TEMP\baoclaw-sockets\baoclaw.sock"
Write-Host ""
Write-Host "Manage with:"
Write-Host "  Start:   Start-Service $serviceName"
Write-Host "  Stop:    Stop-Service $serviceName"
Write-Host "  Status:  Get-Service $serviceName"
Write-Host "  Restart: Restart-Service $serviceName"
Write-Host "  Uninstall: PowerShell -ExecutionPolicy Bypass -File uninstall.ps1"
Write-Host ""
Write-Host "Or via services.msc (search 'services' in Start menu)."
