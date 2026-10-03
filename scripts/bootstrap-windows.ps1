# Mimicway - build from source on Windows (PowerShell).
# Usage: .\scripts\bootstrap-windows.ps1
# Safe to run several times: installed tools are kept.

$ErrorActionPreference = "Stop"

function Write-Step($msg) { Write-Host "`n=== $msg ===" -ForegroundColor Cyan }
function Write-Ok($msg)   { Write-Host "  OK: $msg" -ForegroundColor Green }
function Write-Skip($msg)  { Write-Host "  SKIP: $msg" -ForegroundColor Yellow }

Write-Step "1/6 - Rust toolchain"
if (Get-Command rustc -ErrorAction SilentlyContinue) {
    $v = (rustc --version)
    Write-Ok "rustc already installed ($v)"
} else {
    Write-Host "  Installing Rust with rustup..."
    Invoke-WebRequest -Uri "https://win.rustup.rs/x86_64" -OutFile "$env:TEMP\rustup-init.exe" -UseBasicParsing
    & "$env:TEMP\rustup-init.exe" -y --default-toolchain stable
    $env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"
    Write-Ok "Rust installed ($(rustc --version))"
}
$env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"

Write-Step "2/6 - Visual Studio Build Tools (MSVC linker)"
$vsWhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
$hasVS = $false
if (Test-Path $vsWhere) {
    $vsPath = & $vsWhere -all -products * -property installationPath 2>$null
    if ($vsPath) { $hasVS = $true }
}
if ($hasVS) {
    Write-Ok "Build Tools already installed"
} else {
    if (Get-Command winget -ErrorAction SilentlyContinue) {
        Write-Host "  Installing the Build Tools with winget..."
        winget install Microsoft.VisualStudio.2022.BuildTools --override "--quiet --wait --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended" --accept-source-agreements --accept-package-agreements
        Write-Ok "Build Tools installed"
    } else {
        Write-Host "  WARN: winget is not available. Install the Visual Studio Build Tools with the C++ workload yourself." -ForegroundColor Red
    }
}

Write-Step "3/6 - Node.js"
if (Get-Command node -ErrorAction SilentlyContinue) {
    Write-Ok "Node.js already installed ($(node --version))"
} else {
    if (Get-Command winget -ErrorAction SilentlyContinue) {
        winget install OpenJS.NodeJS.LTS --accept-source-agreements --accept-package-agreements
        Write-Ok "Node.js installed"
    } else {
        Write-Host "  WARN: install Node.js 22.12 or later yourself." -ForegroundColor Red
    }
}

Write-Step "4/6 - UI dependencies"
Set-Location "$PSScriptRoot\..\frontend"
# Exactly the versions of package-lock.json, without running package install scripts.
npm ci --ignore-scripts
Write-Ok "npm ci done"

Write-Step "5/6 - UI build"
npm run build
Write-Ok "UI built in frontend\dist\"

Write-Step "6/6 - Server build"
Set-Location "$PSScriptRoot\.."
$vcvars = "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvarsall.bat"
if (Test-Path $vcvars) {
    cmd /c "`"$vcvars`" x64 >nul 2>&1 && cargo build --release --locked 2>&1"
} else {
    cargo build --release --locked
}
if ($LASTEXITCODE -ne 0) {
    Write-Host "  The server build failed. Behind a proxy that blocks certificate revocation lists, crate downloads fail" -ForegroundColor Red
    Write-Host "  with a CRL error: ask your network team to allow them, or, knowingly, set CARGO_HTTP_CHECK_REVOKE=false." -ForegroundColor Red
    exit 1
}
Write-Ok "Server built in target\release\"

Write-Host "`n" -NoNewline
Write-Host "================================================================" -ForegroundColor Green
Write-Host "  Mimicway is ready. Start it with:" -ForegroundColor Green
Write-Host '  .\target\release\mimicway.exe' -ForegroundColor White
Write-Host "  then open http://localhost:7342" -ForegroundColor Green
Write-Host "================================================================" -ForegroundColor Green
