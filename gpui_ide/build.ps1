$ErrorActionPreference = "Stop"
# Native tools (cargo, git, ...) routinely write progress to stderr; do not
# let PowerShell 7 treat that as a terminating error.
$PSNativeCommandUseErrorActionPreference = $false

$ideRoot = $PSScriptRoot
$repoRoot = Split-Path -Parent $ideRoot
Set-Location $ideRoot

# LLVM-MinGW discovery: use LLVM_MINGW when set, otherwise probe common roots.
$llvm = $env:LLVM_MINGW
if (-not $llvm) {
  $candidates = @(
    "C:\Program Files\LLVM-MinGW",
    (Join-Path $env:LOCALAPPDATA "Programs\LLVM-MinGW")
  )
  foreach ($candidate in $candidates) {
    if (Test-Path -LiteralPath (Join-Path $candidate "bin\x86_64-w64-mingw32-clang.exe")) {
      $llvm = $candidate
      break
    }
  }
}
if (-not $llvm) {
  throw "LLVM-MinGW was not found. Install it and set the LLVM_MINGW environment variable to its root."
}

$llvmBin = Join-Path $llvm "bin"
$llvmLib = Join-Path $llvm "x86_64-w64-mingw32\lib"
$env:Path = "$llvmBin;$env:Path"
$env:LIBRARY_PATH = $llvmLib
$env:LLVM_MINGW_LIB = $llvmLib
if (-not $env:MSPM0_LUAC) {
  $env:MSPM0_LUAC = Join-Path $repoRoot "tools\bin\luac_mspm0.exe"
}
if (-not $env:GPUI_FXC_PATH) {
  $env:GPUI_FXC_PATH = Join-Path $ideRoot "tools\fxc\fxc.exe"
}

# Copy OLED library and docs into the portable dist layout.
$oledSource = Join-Path $repoRoot "mspm0_lua\release\lua\oled.lua"
$oledDist = Join-Path $ideRoot "dist\firmware\release\lua\oled.lua"
New-Item -ItemType Directory -Force -Path (Split-Path $oledDist) | Out-Null
Copy-Item -LiteralPath $oledSource -Destination $oledDist -Force
$docCopies = @(
  @((Join-Path $ideRoot "docs\modular-run.md"), (Join-Path $ideRoot "dist\docs\modular-run.md")),
  @((Join-Path $repoRoot "mspm0_lua\docs\OLED_FONT.md"), (Join-Path $ideRoot "dist\firmware\release\docs\OLED_FONT.md"))
)
foreach ($copy in $docCopies) {
  New-Item -ItemType Directory -Force -Path (Split-Path $copy[1]) | Out-Null
  Copy-Item -LiteralPath $copy[0] -Destination $copy[1] -Force
}

# Static libunwind workaround for LLVM-MinGW (clang may otherwise pick the
# import library).
$libdir = $llvmLib
if (-not (Test-Path "$libdir\libgcc.a")) {
  Copy-Item "$libdir\libunwind.a" "$libdir\libgcc.a" -Force
  Copy-Item "$libdir\libunwind.a" "$libdir\libgcc_eh.a" -Force
}
$dllA = "$libdir\libunwind.dll.a"
$dllABak = "$libdir\libunwind.dll.a.bak"
$hidDllA = $false
if ((Test-Path "$libdir\libunwind.a") -and (Test-Path $dllA)) {
  Move-Item $dllA $dllABak -Force
  $hidDllA = $true
}

$logFile = Join-Path $env:TEMP "mspm0_lua_ide_build.log"
# Do not merge stderr into the pipeline: Windows PowerShell 5.1 treats a
# native command's first stderr line as a terminating error under
# $ErrorActionPreference = "Stop".
cargo build --target x86_64-pc-windows-gnullvm
$exit = $LASTEXITCODE
if ($hidDllA -and (Test-Path $dllABak) -and -not (Test-Path $dllA)) {
  Move-Item $dllABak $dllA -Force
}
"EXIT=$exit" | Add-Content $logFile
if ($exit -eq 0) {
  $src = Join-Path $ideRoot "target\x86_64-pc-windows-gnullvm\debug\lua-ide.exe"
  if (-not (Test-Path -LiteralPath $src)) {
    $src = Join-Path $ideRoot "target\x86_64-pc-windows-gnullvm\debug\mspm0-lua-ide.exe"
  }
  $dist = Join-Path $ideRoot "dist"
  New-Item -ItemType Directory -Force -Path $dist | Out-Null
  $tmpOut = Join-Path $dist "_LuaIDE_pack.exe"
  $final = Join-Path $dist "Lua IDE.exe"
  $fallback = Join-Path $dist "Lua IDE_new.exe"
  Remove-Item -LiteralPath $tmpOut -Force -ErrorAction SilentlyContinue
  $stripOk = $false
  try {
    & (Join-Path $llvmBin "llvm-strip.exe") -o $tmpOut -- $src
    if ((Test-Path -LiteralPath $tmpOut) -and ((Get-Item -LiteralPath $tmpOut).Length -gt 1MB)) {
      $stripOk = $true
    }
  } catch {}
  if (-not $stripOk) {
    Copy-Item -LiteralPath $src -Destination $tmpOut -Force
  }
  $dest = $final
  try {
    if (Test-Path -LiteralPath $final) { Remove-Item -LiteralPath $final -Force -ErrorAction Stop }
    Rename-Item -LiteralPath $tmpOut -NewName "Lua IDE.exe" -Force -ErrorAction Stop
  } catch {
    $dest = $fallback
    if (Test-Path -LiteralPath $fallback) { Remove-Item -LiteralPath $fallback -Force -ErrorAction SilentlyContinue }
    Rename-Item -LiteralPath $tmpOut -NewName "Lua IDE_new.exe" -Force
    "PACK_WARN locked, wrote $fallback" | Add-Content $logFile
  }
  # Portable extras next to exe: example projects; keep firmware/ if placed.
  Remove-Item -LiteralPath (Join-Path $dist "libunwind.dll") -Force -ErrorAction SilentlyContinue
  Remove-Item -LiteralPath (Join-Path $dist "bin") -Recurse -Force -ErrorAction SilentlyContinue
  $packEx = Join-Path $ideRoot "example"
  $distEx = Join-Path $dist "example"
  if (Test-Path -LiteralPath $packEx) {
    if (Test-Path -LiteralPath $distEx) { Remove-Item -LiteralPath $distEx -Recurse -Force -ErrorAction SilentlyContinue }
    Copy-Item -LiteralPath $packEx -Destination $distEx -Recurse -Force
  }
  foreach ($name in @("chips", "boards", "apis", "font")) {
    $dataSrc = Join-Path $ideRoot $name
    $dataDst = Join-Path $dist $name
    if (Test-Path -LiteralPath $dataDst) {
      Remove-Item -LiteralPath $dataDst -Recurse -Force
    }
    Copy-Item -LiteralPath $dataSrc -Destination $dataDst -Recurse -Force
  }
  $docsSrc = Join-Path $ideRoot "docs"
  $docsDst = Join-Path $dist "docs"
  if (Test-Path -LiteralPath $docsDst) {
    Remove-Item -LiteralPath $docsDst -Recurse -Force
  }
  Copy-Item -LiteralPath $docsSrc -Destination $docsDst -Recurse -Force
  $configSrc = Join-Path $ideRoot "config.json"
  $configDst = Join-Path $dist "config.json"
  if (-not (Test-Path -LiteralPath $configDst)) {
    Copy-Item -LiteralPath $configSrc -Destination $configDst -Force
  }

  & (Join-Path $PSScriptRoot "package_firmware.ps1")
  if ($LASTEXITCODE -ne 0) { $exit = 1 }
  if (Test-Path -LiteralPath $dest) {
    $len = (Get-Item -LiteralPath $dest).Length
    "PACKAGED size=$len path=$dest stripOk=$stripOk" | Add-Content $logFile
  }
}
exit $exit
