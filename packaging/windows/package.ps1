<#
.SYNOPSIS
  Build, sign and package ProjectCraft for Windows.

.DESCRIPTION
  Produces, in $env:DIST (default: dist/release):
    projectcraft-<version>-windows-<arch>.msi            per-machine installer (WiX v5)
    projectcraft-<version>-windows-<arch>-portable.zip   projectcraft.exe + projectcraft-cli.exe + projectcraft-sync.exe
#>
param(
  [ValidateSet('x64', 'x86', 'arm64')] [string] $Arch = 'x64',
  [switch] $SkipBuild
)
$ErrorActionPreference = 'Stop'
$Root = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path

function Invoke-Native([string] $What, [scriptblock] $Block) {
  Write-Output "==> $What"
  & $Block
  if ($LASTEXITCODE -ne 0) { throw "$What failed with exit code $LASTEXITCODE" }
}

$Version = $env:PROJECTCRAFT_VERSION
if (-not $Version) {
  $inPkg = $false
  foreach ($line in Get-Content (Join-Path $Root 'Cargo.toml')) {
    if ($line -match '^\s*\[') { $inPkg = ($line.Trim() -eq '[workspace.package]'); continue }
    if ($inPkg -and $line -match '^\s*version\s*=\s*"([^"]+)"') { $Version = $Matches[1]; break }
  }
}
if (-not $Version) { throw 'could not read [workspace.package] version from Cargo.toml' }
$MsiVersion = ($Version -split '-')[0]

$InstalledTargets = (rustup target list --installed) -join " "
$Target = switch ($Arch) {
  'x64' { if ($InstalledTargets -match 'x86_64-pc-windows-msvc') { 'x86_64-pc-windows-msvc' } else { 'x86_64-pc-windows-gnu' } }
  'x86' { 'i686-pc-windows-msvc' }
  'arm64' { 'aarch64-pc-windows-msvc' }
}
$Dist = if ($env:DIST) { $env:DIST } else { Join-Path $Root 'dist\release' }
$TargetDir = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $Root 'target' }
New-Item -ItemType Directory -Force -Path $Dist | Out-Null

Write-Output "ProjectCraft $Version for Windows $Arch ($Target)"

if (-not $SkipBuild) {
  if ($Target -like '*-msvc') {
    $flagVar = 'CARGO_TARGET_' + ($Target.ToUpper() -replace '-', '_') + '_RUSTFLAGS'
    [Environment]::SetEnvironmentVariable($flagVar, '-C target-feature=+crt-static')
  }
  Invoke-Native "cargo build ($Target)" { cargo build --release --locked -p projectcraft -p projectcraft-cli -p projectcraft-sync --target $Target }
}

$Bin = Join-Path $TargetDir "$Target\release"

function Get-PeHeader([string] $Path) {
  $bytes = [System.IO.File]::ReadAllBytes($Path)
  $pe = [BitConverter]::ToInt32($bytes, 0x3C)
  return @{ Machine = [BitConverter]::ToUInt16($bytes, $pe + 4); Subsystem = [BitConverter]::ToUInt16($bytes, $pe + 0x5C) }
}
$Machine = switch ($Arch) { 'x64' { 0x8664 } 'x86' { 0x14C } 'arm64' { 0xAA64 } }
foreach ($check in @(@('projectcraft.exe', 2), @('projectcraft-cli.exe', 3), @('projectcraft-sync.exe', 3))) {
  $h = Get-PeHeader (Join-Path $Bin $check[0])
  if ($h.Machine -ne $Machine) { throw "$($check[0]) is for machine 0x$('{0:X}' -f $h.Machine), expected 0x$('{0:X}' -f $Machine) ($Arch)" }
  if ($h.Subsystem -ne $check[1]) { throw "$($check[0]) has PE subsystem $($h.Subsystem), expected $($check[1])" }
  Write-Output "ok $($check[0]): $Arch, PE subsystem $($h.Subsystem)"
}

$Stage = Join-Path $TargetDir "windows-package\$Arch"
Remove-Item -Recurse -Force $Stage -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force -Path $Stage | Out-Null
Copy-Item (Join-Path $Bin 'projectcraft.exe'), (Join-Path $Bin 'projectcraft-cli.exe'), (Join-Path $Bin 'projectcraft-sync.exe') $Stage

& (Join-Path $PSScriptRoot 'sign.ps1') (Join-Path $Stage 'projectcraft.exe') (Join-Path $Stage 'projectcraft-cli.exe') (Join-Path $Stage 'projectcraft-sync.exe')

$Msi = Join-Path $Dist "projectcraft-$Version-windows-$Arch.msi"
Invoke-Native 'wix build' {
  wix build (Join-Path $PSScriptRoot 'projectcraft.wxs') -arch $Arch -ext WixToolset.UI.wixext `
    -d "Version=$MsiVersion" -d "BinDir=$Stage" -d "IconPath=$(Join-Path $Root 'assets\app-icon\projectcraft.ico')" `
    -o $Msi
}
Remove-Item -Force -ErrorAction SilentlyContinue ([IO.Path]::ChangeExtension($Msi, '.wixpdb'))
& (Join-Path $PSScriptRoot 'sign.ps1') $Msi

$Portable = Join-Path $TargetDir "windows-package\projectcraft-$Version-windows-$Arch-portable"
Remove-Item -Recurse -Force $Portable -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force -Path $Portable | Out-Null
Copy-Item (Join-Path $Stage '*.exe') $Portable
foreach ($f in 'README.md', 'LICENSE-MIT', 'LICENSE-APACHE') {
  $p = Join-Path $Root $f
  if (Test-Path $p) { Copy-Item $p $Portable }
}
$Zip = Join-Path $Dist "projectcraft-$Version-windows-$Arch-portable.zip"
Remove-Item -Force $Zip -ErrorAction SilentlyContinue
Compress-Archive -Path $Portable -DestinationPath $Zip

Invoke-Native 'projectcraft-cli --version' { & (Join-Path $Stage 'projectcraft-cli.exe') --version }
Get-Item $Msi, $Zip | Format-Table Name, Length
