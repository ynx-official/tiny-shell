param(
    [string]$Version = "",
    [string]$Target = "x86_64-pc-windows-msvc",
    [string]$OutputDir = "dist",
    [string]$RuntimeDir = "",
    [switch]$SkipBuild
)

$ErrorActionPreference = "Stop"

$root = Split-Path -Parent $PSScriptRoot
if ([string]::IsNullOrWhiteSpace($Version)) {
    $manifest = Get-Content (Join-Path $root "Cargo.toml") -Raw
    $match = [regex]::Match($manifest, '(?m)^version\s*=\s*"([^"]+)"')
    if (-not $match.Success) {
        throw "Unable to read the package version from Cargo.toml"
    }
    $Version = "v$($match.Groups[1].Value)"
}

if (-not $SkipBuild) {
    cargo build --locked --release --target $Target
    if ($LASTEXITCODE -ne 0) {
        throw "cargo build failed with exit code $LASTEXITCODE"
    }
}

$sourceExe = Join-Path $root "target\$Target\release\tiny-shell.exe"
if (-not (Test-Path -LiteralPath $sourceExe -PathType Leaf)) {
    throw "Compiled executable not found: $sourceExe"
}

if ([string]::IsNullOrWhiteSpace($RuntimeDir)) {
    $buildRoot = Join-Path $root "target\$Target\release\build"
    $coreRuntimeFiles = @("freerdp-client3.dll", "freerdp3.dll", "winpr3.dll")
    $buildOutputs = Get-ChildItem -LiteralPath $buildRoot -Directory -Filter "tiny-shell-*" -ErrorAction SilentlyContinue |
        Sort-Object LastWriteTime -Descending
    foreach ($buildOutput in $buildOutputs) {
        $candidate = Join-Path $buildOutput.FullName "out"
        $hasCoreRuntime = $true
        foreach ($runtimeFile in $coreRuntimeFiles) {
            if (-not (Test-Path -LiteralPath (Join-Path $candidate $runtimeFile) -PathType Leaf)) {
                $hasCoreRuntime = $false
                break
            }
        }
        if ($hasCoreRuntime) {
            $RuntimeDir = $candidate
            Write-Host "Using FreeRDP runtime copied by Cargo: $RuntimeDir"
            break
        }
    }
}

if (-not [System.IO.Path]::IsPathRooted($OutputDir)) {
    $OutputDir = Join-Path $root $OutputDir
}
$OutputDir = [System.IO.Path]::GetFullPath($OutputDir)
New-Item -ItemType Directory -Path $OutputDir -Force | Out-Null

$platform = "windows-x86_64"
$portableBaseName = "tiny-shell-$Version-$platform-portable"
$portableDir = Join-Path $OutputDir $portableBaseName
$portableArchive = Join-Path $OutputDir "$portableBaseName.zip"
$portableDir = [System.IO.Path]::GetFullPath($portableDir)
$outputPrefix = $OutputDir.TrimEnd(
    [System.IO.Path]::DirectorySeparatorChar,
    [System.IO.Path]::AltDirectorySeparatorChar
) + [System.IO.Path]::DirectorySeparatorChar
if (-not $portableDir.StartsWith($outputPrefix, [System.StringComparison]::OrdinalIgnoreCase)) {
    throw "Portable staging directory must stay inside the output directory"
}

Remove-Item -LiteralPath $portableDir -Recurse -Force -ErrorAction SilentlyContinue
Remove-Item -LiteralPath $portableArchive -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Path $portableDir -Force | Out-Null
Copy-Item -LiteralPath $sourceExe -Destination (Join-Path $portableDir "tiny-shell.exe")
Copy-Item -LiteralPath (Join-Path $root "LICENSE") -Destination $portableDir
if (-not [string]::IsNullOrWhiteSpace($RuntimeDir)) {
    $RuntimeDir = [System.IO.Path]::GetFullPath($RuntimeDir)
    if (-not (Test-Path -LiteralPath $RuntimeDir -PathType Container)) {
        throw "Runtime directory not found: $RuntimeDir"
    }
    $runtimeFiles = Get-ChildItem -LiteralPath $RuntimeDir -File -Filter "*.dll"
    if (-not $runtimeFiles) {
        throw "Runtime directory does not contain DLL files: $RuntimeDir"
    }
    $runtimeFiles | Copy-Item -Destination $portableDir
}
Compress-Archive -LiteralPath $portableDir -DestinationPath $portableArchive -CompressionLevel Optimal

$innoCandidates = @()
if (-not [string]::IsNullOrWhiteSpace(${env:ProgramFiles(x86)})) {
    $innoCandidates += Join-Path ${env:ProgramFiles(x86)} "Inno Setup 6\ISCC.exe"
}
if (-not [string]::IsNullOrWhiteSpace($env:ProgramFiles)) {
    $innoCandidates += Join-Path $env:ProgramFiles "Inno Setup 6\ISCC.exe"
}
$iscc = $innoCandidates | Where-Object { Test-Path -LiteralPath $_ -PathType Leaf } | Select-Object -First 1
if (-not $iscc) {
    throw "Inno Setup 6 was not found. Install it from https://jrsoftware.org/isinfo.php"
}

$normalizedVersion = $Version.TrimStart('v')
$installerBaseName = "tiny-shell-$Version-$platform-setup"
$installerPath = Join-Path $OutputDir "$installerBaseName.exe"
Remove-Item -LiteralPath $installerPath -Force -ErrorAction SilentlyContinue

$issFile = Join-Path $root "scripts\windows\tiny-shell.iss"
$setupIcon = Join-Path $root "assets\icons\tiny-shell.ico"
$licenseFile = Join-Path $root "LICENSE"
$isccArgs = @(
    "/DMyAppVersion=$normalizedVersion",
    "/DSourceExe=$sourceExe",
    "/DSetupIcon=$setupIcon",
    "/DLicenseFile=$licenseFile",
    "/DOutputDir=$OutputDir",
    "/DOutputBaseFilename=$installerBaseName"
)
if (-not [string]::IsNullOrWhiteSpace($RuntimeDir)) {
    $isccArgs += "/DRuntimeDir=$RuntimeDir"
}
$isccArgs += $issFile
& $iscc @isccArgs
if ($LASTEXITCODE -ne 0) {
    throw "Inno Setup failed with exit code $LASTEXITCODE"
}

Write-Host "Windows packages created:"
Write-Host "  Portable: $portableArchive"
Write-Host "  Installer: $installerPath"
