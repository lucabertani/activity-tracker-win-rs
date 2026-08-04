# WinRing0 low-level access driver — installer (run as Administrator)

param(
    [switch]$Uninstall
)

$ErrorActionPreference = 'Stop'
$serviceName = 'WinRing0_1_2_0'
$dst = Join-Path $env:SystemRoot 'System32\drivers\WinRing0x64.sys'

if ($Uninstall) {
    Write-Host 'Stopping and removing the driver service...'
    & sc.exe stop $serviceName 2>$null | Out-Null
    & sc.exe delete $serviceName 2>$null | Out-Null
    Remove-Item $dst -Force -ErrorAction SilentlyContinue
    Write-Host 'Done.'
    exit 0
}

$src = Join-Path $PSScriptRoot 'WinRing0x64.sys'
if (-not (Test-Path $src)) {
    Write-Host "Driver file not found next to this script: $src" -ForegroundColor Red
    exit 1
}

Write-Host 'Copying driver...'
Copy-Item $src $dst -Force

$exists = $false
& sc.exe query $serviceName 2>$null | Out-Null
if ($LASTEXITCODE -eq 0) { $exists = $true }
if ($exists) {
    Write-Host "Service $serviceName exists: reconfiguring (start= system) and starting..."
    & sc.exe config $serviceName start= system binPath= "`"$dst`""
    & sc.exe start $serviceName 2>$null | Out-Null
} else {
    Write-Host 'Creating service...'
    & sc.exe create $serviceName type= kernel start= system binPath= "`"$dst`"" DisplayName= "WinRing0 Low Level Access Driver"
    if ($LASTEXITCODE -ne 0) { exit 1 }
    & sc.exe start $serviceName 2>$null | Out-Null
}

Start-Sleep -Milliseconds 500
& sc.exe query $serviceName
Write-Host ''
Write-Host 'start= system means the driver loads at every boot, no admin needed afterwards.' -ForegroundColor Green
Write-Host 'If STATE is not RUNNING, check: Secure Boot / driver signature blocklist / antivirus.' -ForegroundColor Yellow
