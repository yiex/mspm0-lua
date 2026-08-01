$ErrorActionPreference = "Stop"

# The IDE consumes only the signed-by-manifest modular catalog. Recreate the
# generated directory every time so old module binaries cannot survive a build.
$ideRoot = $PSScriptRoot
$repoRoot = Split-Path -Parent $ideRoot
$firmwareRoot = Join-Path $repoRoot "mspm0_lua"
$dist = Join-Path $ideRoot "dist"
$fwDir = Join-Path $dist "firmware"
$indexPath = Join-Path $firmwareRoot "build_modules\index.json"
$manifestPath = Join-Path $firmwareRoot "release\catalog_manifest.json"
$corePath = Join-Path $firmwareRoot "build_modular\mspm0_lua_modular.bin"

foreach ($required in @($indexPath, $manifestPath, $corePath)) {
  if (-not (Test-Path -LiteralPath $required -PathType Leaf)) {
    throw "firmware package input is missing: $required"
  }
}

# A full image intentionally contains the current core plus erased module
# slots. It is the only default for a core reflash, avoiding stale modules from
# an older catalog remaining in internal Flash.
$coreImage = Join-Path $firmwareRoot "build_composed\firmware_core.bin"
& py -3 (Join-Path $repoRoot "tools\compose_firmware.py") --modules --output $coreImage
if ($LASTEXITCODE -ne 0 -or -not (Test-Path -LiteralPath $coreImage -PathType Leaf)) {
  throw "failed to compose the clean modular core image"
}

$index = Get-Content -LiteralPath $indexPath -Raw | ConvertFrom-Json
$images = @()
foreach ($moduleProperty in $index.modules.PSObject.Properties) {
  foreach ($variant in $moduleProperty.Value.variants) {
    $relative = ([string]$variant.image).Replace('\', '/')
    if ($relative -notmatch '^build_modules/[A-Za-z0-9_]+/slot[0-7]/[A-Za-z0-9_]+\.bin$') {
      throw "invalid indexed module path: $relative"
    }
    $source = Join-Path $firmwareRoot $relative
    if (-not (Test-Path -LiteralPath $source -PathType Leaf)) {
      throw "indexed module image is missing: $source"
    }
    $images += @{ Relative = $relative; Source = $source }
  }
}

if (Test-Path -LiteralPath $fwDir) {
  Remove-Item -LiteralPath $fwDir -Recurse -Force
}
New-Item -ItemType Directory -Force -Path $fwDir | Out-Null
Copy-Item -LiteralPath (Join-Path $firmwareRoot "release") -Destination (Join-Path $fwDir "release") -Recurse -Force
New-Item -ItemType Directory -Force -Path (Join-Path $fwDir "modules") | Out-Null
Copy-Item -LiteralPath (Join-Path $firmwareRoot "modules\modules.json") -Destination (Join-Path $fwDir "modules\modules.json") -Force
New-Item -ItemType Directory -Force -Path (Join-Path $fwDir "build_modules") | Out-Null
Copy-Item -LiteralPath $indexPath -Destination (Join-Path $fwDir "build_modules\index.json") -Force
foreach ($image in $images) {
  $target = Join-Path $fwDir $image.Relative
  New-Item -ItemType Directory -Force -Path (Split-Path -Parent $target) | Out-Null
  Copy-Item -LiteralPath $image.Source -Destination $target -Force
}
New-Item -ItemType Directory -Force -Path (Join-Path $fwDir "build_modular") | Out-Null
Copy-Item -LiteralPath $corePath -Destination (Join-Path $fwDir "build_modular\mspm0_lua_modular.bin") -Force
New-Item -ItemType Directory -Force -Path (Join-Path $fwDir "build_composed") | Out-Null
Copy-Item -LiteralPath $coreImage -Destination (Join-Path $fwDir "build_composed\firmware_core.bin") -Force

# The IDE also loads these fallback locations and the UART BSL menu searches
# build_bytecode/. Bundle them so the packaged IDE is self-contained.
$bytecode = Join-Path $firmwareRoot "build_bytecode\mspm0_lua_bytecode.bin"
if (Test-Path -LiteralPath $bytecode -PathType Leaf) {
  New-Item -ItemType Directory -Force -Path (Join-Path $fwDir "build_bytecode") | Out-Null
  Copy-Item -LiteralPath $bytecode -Destination (Join-Path $fwDir "build_bytecode\mspm0_lua_bytecode.bin") -Force
  Copy-Item -LiteralPath $bytecode -Destination (Join-Path $fwDir "mspm0_lua_bytecode.bin") -Force
}
Copy-Item -LiteralPath $corePath -Destination (Join-Path $fwDir "mspm0_lua_modular.bin") -Force

$actual = [string[]]@(Get-ChildItem -LiteralPath (Join-Path $fwDir "build_modules") -Recurse -File -Filter "*.bin" |
  ForEach-Object { "build_modules/" + $_.FullName.Substring((Join-Path $fwDir "build_modules").Length).TrimStart('\').Replace('\', '/') } |
  Sort-Object)
$expected = [string[]]@($images | ForEach-Object { $_.Relative } | Sort-Object)
$expectedList = [string]::Join("`n", $expected)
$actualList = [string]::Join("`n", $actual)
if ($expectedList -cne $actualList) {
  throw "packaged module binaries differ from index.json"
}
Write-Output "FIRMWARE_PACKAGE_OK modules=$($images.Count) path=$fwDir"
