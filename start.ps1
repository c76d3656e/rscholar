$ErrorActionPreference = "Stop"

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$Port = if ($env:PORT) { $env:PORT } else { "3000" }
$FrontDir = Join-Path $ScriptDir "front"
$DistDir = Join-Path $FrontDir "dist"

function Require-Command {
    param([string]$Name)
    if (-not (Get-Command $Name -ErrorAction SilentlyContinue)) {
        throw "Missing command: $Name"
    }
}

function Resolve-CommandPath {
    param([string]$Name)
    $cmd = Get-Command $Name -ErrorAction SilentlyContinue
    if (-not $cmd) {
        throw "Missing command: $Name"
    }
    if ($cmd.Source) {
        return $cmd.Source
    }
    if ($cmd.Path) {
        return $cmd.Path
    }
    return $Name
}

function Write-Info {
    param([string]$Message)
    Write-Host "[start] $Message" -ForegroundColor Cyan
}

Require-Command cargo
Require-Command npm
$CargoExe = Resolve-CommandPath "cargo"
$NpmExe = Resolve-CommandPath "npm"

Write-Info "Installing frontend dependencies..."
Push-Location $FrontDir
if (Test-Path "package-lock.json") {
    & $NpmExe ci
} else {
    & $NpmExe install
}

Write-Info "Building frontend..."
& $NpmExe run build
Pop-Location

Write-Info "Building Rust release..."
Push-Location $ScriptDir
& $CargoExe build --release
Pop-Location

$BinCandidates = @(
    (Join-Path $ScriptDir "target\release\Rscholar.exe"),
    (Join-Path $ScriptDir "target\release\rscholar.exe"),
    (Join-Path $ScriptDir "target\release\Rscholar"),
    (Join-Path $ScriptDir "target\release\rscholar")
)
$BinPath = $BinCandidates | Where-Object { Test-Path $_ } | Select-Object -First 1
if (-not $BinPath) {
    throw "Backend binary not found under target/release (Rscholar/rscholar)."
}

Write-Host ""
Write-Host "Rscholar starting" -ForegroundColor Green
Write-Host "  URL   : http://localhost:$Port"
Write-Host "  Static: $DistDir"
Write-Host "  Bin   : $BinPath"
Write-Host ""

& $BinPath server --port $Port --serve-static $DistDir
