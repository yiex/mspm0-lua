$ErrorActionPreference = "Stop"
# Native tools (cargo, git, ...) routinely write progress to stderr; do not
# let PowerShell 7 treat that as a terminating error.
$PSNativeCommandUseErrorActionPreference = $false

$ideRoot = $PSScriptRoot
$repoRoot = Split-Path -Parent $ideRoot

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

$libdir = $llvmLib
if (-not (Test-Path "$libdir\libgcc.a")) {
  Copy-Item "$libdir\libunwind.a" "$libdir\libgcc.a" -Force
  Copy-Item "$libdir\libunwind.a" "$libdir\libgcc_eh.a" -Force
}
Set-Location $ideRoot

$logFile = Join-Path $env:TEMP "mspm0_lua_ide_release.log"
# Do not merge stderr into the pipeline: Windows PowerShell 5.1 treats a
# native command's first stderr line as a terminating error under
# $ErrorActionPreference = "Stop".
cargo build --release --target x86_64-pc-windows-gnullvm
$exit = $LASTEXITCODE
"EXIT=$exit" | Add-Content $logFile
if ($exit -eq 0) {
  $src = Join-Path $ideRoot "target\x86_64-pc-windows-gnullvm\release\lua-ide.exe"
  if (-not (Test-Path $src)) {
    $src = Join-Path $ideRoot "target\x86_64-pc-windows-gnullvm\release\mspm0-lua-ide.exe"
  }
  $dist = Join-Path $ideRoot "dist"
  New-Item -ItemType Directory -Force -Path "$dist\bin" | Out-Null
  $final = Join-Path $dist "Lua IDE.exe"
  Copy-Item -LiteralPath $src -Destination $final -Force
  Remove-Item -LiteralPath "$dist\MSPM0_Lua_IDE_GPUI.exe" -Force -ErrorAction SilentlyContinue
  Remove-Item -LiteralPath "$dist\MSPM0_Lua_IDE_GPUI_stripped.exe" -Force -ErrorAction SilentlyContinue
  Copy-Item (Join-Path $llvmBin "libunwind.dll") $dist -Force
  $luac = Join-Path $repoRoot "tools\bin\luac_mspm0.exe"
  if (Test-Path $luac) {
    Copy-Item $luac "$dist\bin\luac_mspm0.exe" -Force
  }
  & (Join-Path $PSScriptRoot "package_firmware.ps1")
  if ($LASTEXITCODE -ne 0) { $exit = 1 }

  # Portable IDE data: the runtime requires chips/, boards/, apis/ and font/
  # next to the executable (see src/metadata.rs::data_root).
  foreach ($name in @("chips", "boards", "apis", "font")) {
    $dataDst = Join-Path $dist $name
    if (Test-Path -LiteralPath $dataDst) {
      Remove-Item -LiteralPath $dataDst -Recurse -Force
    }
    Copy-Item -LiteralPath (Join-Path $ideRoot $name) -Destination $dataDst -Recurse -Force
  }
  $docsDst = Join-Path $dist "docs"
  if (Test-Path -LiteralPath $docsDst) {
    Remove-Item -LiteralPath $docsDst -Recurse -Force
  }
  Copy-Item -LiteralPath (Join-Path $ideRoot "docs") -Destination $docsDst -Recurse -Force
  $configDst = Join-Path $dist "config.json"
  if (-not (Test-Path -LiteralPath $configDst)) {
    Copy-Item -LiteralPath (Join-Path $ideRoot "config.json") -Destination $configDst -Force
  }
  $packEx = Join-Path $ideRoot "example"
  $distEx = Join-Path $dist "example"
  if (Test-Path -LiteralPath $packEx) {
    if (Test-Path -LiteralPath $distEx) {
      Remove-Item -LiteralPath $distEx -Recurse -Force
    }
    Copy-Item -LiteralPath $packEx -Destination $distEx -Recurse -Force
  }

  $len = (Get-Item -LiteralPath $final).Length
  "PACKAGED size=$len path=$final" | Add-Content $logFile
}
exit $exit
