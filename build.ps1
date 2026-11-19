param([ValidatePattern('^[A-Za-z0-9_-]+$')][string]$OutputFolder = 'bin')

$ErrorActionPreference = 'Stop'
$projectRoot = $PSScriptRoot
$toolchain = Join-Path $projectRoot '.tools\rustup\toolchains\stable-x86_64-pc-windows-msvc\bin'
$localCargo = Join-Path $toolchain 'cargo.exe'
if (Test-Path -LiteralPath $localCargo) {
    $env:RUSTUP_HOME = Join-Path $projectRoot '.tools\rustup'
    $env:CARGO_HOME = Join-Path $projectRoot '.tools\cargo'
    $env:RUSTC = Join-Path $toolchain 'rustc.exe'
    $env:RUSTDOC = Join-Path $toolchain 'rustdoc.exe'
    $env:Path = $toolchain + ';' + $env:Path
    $cargo = $localCargo
} else {
    $cargo = (Get-Command cargo.exe -ErrorAction Stop).Source
}
$vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
$visualStudio = & $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
if (-not $visualStudio) { throw 'Microsoft Visual Studio x86 build tools were not found' }
$vcDirectory = Join-Path $visualStudio 'VC\Tools\MSVC'
$vc = Get-ChildItem -LiteralPath $vcDirectory -Directory | Sort-Object Name -Descending | Select-Object -First 1
$sdkDirectory = Join-Path ${env:ProgramFiles(x86)} 'Windows Kits\10\Lib'
$sdk = Get-ChildItem -LiteralPath $sdkDirectory -Directory | Where-Object { Test-Path -LiteralPath (Join-Path $_.FullName 'um\x86\kernel32.lib') } | Sort-Object Name -Descending | Select-Object -First 1
if (-not $vc -or -not $sdk) { throw 'Microsoft x86 linker or Windows SDK was not found' }
$env:Path = (Join-Path $vc.FullName 'bin\Hostx64\x86') + ';' + $env:Path
$env:LIB = (Join-Path $vc.FullName 'lib\x86') + ';' + (Join-Path $sdk.FullName 'um\x86') + ';' + (Join-Path $sdk.FullName 'ucrt\x86')
$targetDirectory = Join-Path $projectRoot 'target'
Push-Location -LiteralPath $projectRoot
try {
    & $cargo build --release --locked --target i686-pc-windows-msvc --target-dir $targetDirectory --lib --bin OldDialogsLauncher
    if ($LASTEXITCODE -ne 0) { throw "Cargo failed with code $LASTEXITCODE" }
    $releaseDirectory = Join-Path $targetDirectory 'i686-pc-windows-msvc\release'
    $outputDirectory = Join-Path $projectRoot $OutputFolder
    [IO.Directory]::CreateDirectory($outputDirectory) | Out-Null
    $library = Join-Path $releaseDirectory 'OldDialogs.dll'
    Copy-Item -LiteralPath $library -Destination (Join-Path $outputDirectory 'OldDialogs.dll') -Force
    Copy-Item -LiteralPath $library -Destination (Join-Path $outputDirectory 'OldDialogs.asi') -Force
    Copy-Item -LiteralPath (Join-Path $releaseDirectory 'OldDialogsLauncher.exe') -Destination (Join-Path $outputDirectory 'OldDialogsLauncher.exe') -Force
    Write-Output "Ready: $outputDirectory"
} finally {
    Pop-Location
}
