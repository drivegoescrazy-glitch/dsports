[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$RomPath,

    [string]$OutputPath = (Join-Path ([Environment]::GetFolderPath("Desktop")) "SoulSilver-ssrecomp.apk"),

    [ValidateRange(1, 64)]
    [int]$Jobs = 4
)

$ErrorActionPreference = "Stop"

function Require-Command([string]$Name, [string]$InstallHint) {
    if (-not (Get-Command $Name -ErrorAction SilentlyContinue)) {
        throw "Missing '$Name'. $InstallHint"
    }
}

$romItem = Get-Item -LiteralPath $RomPath -ErrorAction Stop
if ($romItem.PSIsContainer) {
    throw "RomPath must point to your own SoulSilver .nds file, not a folder."
}
$rom = $romItem.FullName
if ([IO.Path]::GetExtension($rom) -ne ".nds") {
    Write-Warning "The selected file does not end in .nds; the native builder will validate whether it is compatible."
}

Require-Command "git" "Install Git for Windows and reopen PowerShell."
Require-Command "python" "Install Python 3 and ensure 'python' is on PATH."
Require-Command "cmake" "Install CMake and ensure it is on PATH."
Require-Command "ninja" "Install Ninja and ensure it is on PATH."

$root = Join-Path $env:LOCALAPPDATA "DSports\ssrecomp-v0.3.0"
$parent = Split-Path -Parent $root
New-Item -ItemType Directory -Force -Path $parent | Out-Null

if (-not (Test-Path (Join-Path $root ".git"))) {
    if (Test-Path $root) {
        throw "The path '$root' exists but is not a Git checkout. Move it aside and rerun."
    }
    Write-Host "Downloading the SSRecomp source into your private LocalAppData folder..."
    & git clone --depth 1 --branch "v0.3.0" "https://github.com/ClickingMaster/ssrecomp.git" $root
    if ($LASTEXITCODE -ne 0) { throw "Could not download the SSRecomp source." }
} else {
    $remote = (& git -C $root remote get-url origin).Trim()
    if ($remote -ne "https://github.com/ClickingMaster/ssrecomp.git") {
        throw "The existing source folder has an unexpected Git remote: $remote"
    }
    & git -C $root checkout "v0.3.0"
    if ($LASTEXITCODE -ne 0) { throw "Could not select SSRecomp v0.3.0." }
}

Write-Host ""
Write-Host "Building a native SoulSilver Android APK locally."
Write-Host "The ROM will not be copied into the DSports Git repository or uploaded by this helper."
Write-Host "The resulting APK contains the game; keep it private and do not upload/share it."
Write-Host "The first build can take 20-40 minutes and download the Android NDK and SDL2."
Write-Host ""

& python (Join-Path $root "ssrecomp_setup.py") --rom $rom --android --jobs $Jobs
if ($LASTEXITCODE -ne 0) {
    throw "SSRecomp's Android build failed. Review the build output above; no APK was copied to the requested output path."
}

$apk = Join-Path $root "SoulSilver-ssrecomp.apk"
if (-not (Test-Path $apk)) {
    throw "The build command finished but the expected APK was not found at '$apk'."
}

$destination = [IO.Path]::GetFullPath($OutputPath)
$destinationDirectory = Split-Path -Parent $destination
New-Item -ItemType Directory -Force -Path $destinationDirectory | Out-Null
Copy-Item -LiteralPath $apk -Destination $destination -Force

Write-Host ""
Write-Host "Native APK created: $destination"
Write-Host "Keep this APK private: it contains your game data."
