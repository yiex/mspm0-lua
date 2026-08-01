$ErrorActionPreference = "Stop"
$root = Split-Path (Split-Path $PSScriptRoot -Parent) -ErrorAction SilentlyContinue
$proj = Join-Path $PSScriptRoot "..\mspm0_lua" | Resolve-Path
$tc = Join-Path $PSScriptRoot "arm-gnu-toolchain\bin" | Resolve-Path
$env:Path = "$tc;" + $env:Path

$build = Join-Path $proj "build"
New-Item -ItemType Directory -Force -Path $build | Out-Null

# Prefer make if available, else python build driver
$make = Get-Command make -ErrorAction SilentlyContinue
if ($make) {
  Push-Location $proj
  make -j4
  Pop-Location
  exit $LASTEXITCODE
}

Write-Host "make not found; using python builder"
python (Join-Path $PSScriptRoot "build_fw.py")
