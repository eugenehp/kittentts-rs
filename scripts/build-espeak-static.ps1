# =============================================================================
# scripts/build-espeak-static.ps1
# Build espeak-ng.lib (MSVC) or libespeak-ng.a (MinGW) on Windows and place
# it in $env:ESPEAK_LIB_DIR.
#
# Invoked automatically by build.rs when ESPEAK_LIB_DIR is set but the
# expected static archive is absent, and ESPEAK_BUILD_SCRIPT points here.
# Can also be run manually:
#
#   $env:ESPEAK_LIB_DIR = "$PWD\espeak-static\lib"
#   powershell -ExecutionPolicy Bypass -File scripts\build-espeak-static.ps1
#
# ── Prerequisites (choose one toolchain) ──────────────────────────────────────
#
#  Option A — MSYS2 / MinGW-w64 (produces libespeak-ng.a):
#    1. Install MSYS2 from https://www.msys2.org/
#    2. In MSYS2 MinGW64 shell:
#         pacman -S mingw-w64-x86_64-cmake mingw-w64-x86_64-gcc
#    This script auto-detects MSYS2 at C:\msys64 or $env:MSYS2_PATH.
#
#  Option B — MSVC + CMake (produces espeak-ng.lib):
#    1. Install Visual Studio 2019+ with "Desktop development with C++"
#    2. Install CMake from https://cmake.org/
#    3. Open "Developer PowerShell for VS" before running this script,
#       or set $env:ESPEAK_USE_MSVC = "1" to trigger VS detection here.
#
#  Option C — vcpkg (easiest for MSVC, produces espeak-ng.lib):
#    vcpkg install espeak-ng:x64-windows-static
#    $env:ESPEAK_LIB_DIR = "$env:VCPKG_ROOT\installed\x64-windows-static\lib"
#    (No need to run this script — vcpkg places the lib directly.)
#
#  Option D — Pre-built binary from GitHub releases (DLL, not static):
#    https://github.com/espeak-ng/espeak-ng/releases
#    Extract and set ESPEAK_LIB_DIR to the lib directory.
#    Note: this gives a DLL import lib, not a self-contained static archive.
#
# ── Environment variables ─────────────────────────────────────────────────────
#   ESPEAK_LIB_DIR      Output directory [REQUIRED]
#   ESPEAK_TARGET       Cargo target triple [default: x86_64-pc-windows-msvc]
#   ESPEAK_TAG          espeak-ng release tag [default: 1.52.0]
#   BUILD_TMP           Scratch directory [default: $env:TEMP\espeak-static-build]
#   MSYS2_PATH          MSYS2 root [default: C:\msys64]
#   ESPEAK_USE_MSVC     Force MSVC toolchain [default: auto-detect]
#   JOBS                Parallel build jobs [default: $env:NUMBER_OF_PROCESSORS]
# =============================================================================
param()

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

# ── Helpers ────────────────────────────────────────────────────────────────────
function Log   { param([string]$Msg) Write-Host "[..] $Msg" -ForegroundColor Cyan }
function Ok    { param([string]$Msg) Write-Host "[ok] $Msg" -ForegroundColor Green }
function Die   {
    param([string]$Msg)
    Write-Host "[!!] $Msg" -ForegroundColor Red
    exit 1
}
function Chk   {
    param([string]$Cmd, [string]$Hint)
    if (-not (Get-Command $Cmd -ErrorAction SilentlyContinue)) {
        Die "'$Cmd' not found — $Hint"
    }
}

# ── Configuration ──────────────────────────────────────────────────────────────
$EspeakTag   = if ($env:ESPEAK_TAG)       { $env:ESPEAK_TAG }       else { "1.52.0" }
$EspeakRepo  = "https://github.com/espeak-ng/espeak-ng.git"
$BuildTmp    = if ($env:BUILD_TMP)        { $env:BUILD_TMP }        else { "$env:TEMP\espeak-static-build" }
$Jobs        = if ($env:JOBS)             { $env:JOBS }             else { $env:NUMBER_OF_PROCESSORS }
$Target      = if ($env:ESPEAK_TARGET)    { $env:ESPEAK_TARGET }    else { "x86_64-pc-windows-msvc" }

$OutDir = $env:ESPEAK_LIB_DIR
if (-not $OutDir) {
    Die "ESPEAK_LIB_DIR is not set.`nSet it to the directory where the static library should be placed, e.g.:`n  `$env:ESPEAK_LIB_DIR = `"`$PWD\espeak-static\lib`""
}

New-Item -ItemType Directory -Force -Path $BuildTmp | Out-Null
New-Item -ItemType Directory -Force -Path $OutDir   | Out-Null

$EspeakSrc   = "$BuildTmp\espeak-ng-src"
$BuildDir    = "$BuildTmp\espeak-build"
$InstallDir  = "$BuildTmp\espeak-install"

Log "espeak-ng static build (Windows)"
Log "  tag    : $EspeakTag"
Log "  target : $Target"
Log "  out    : $OutDir"

# ── Detect toolchain ───────────────────────────────────────────────────────────
$UseMsvc   = $false
$Msys2Root = if ($env:MSYS2_PATH) { $env:MSYS2_PATH } else { "C:\msys64" }
$Msys2Bin  = "$Msys2Root\mingw64\bin"

# Force MSVC if the user asked, or if the target is *-windows-msvc.
if ($env:ESPEAK_USE_MSVC -eq "1" -or $Target -match "msvc") {
    $UseMsvc = $true
} elseif (Test-Path "$Msys2Bin\gcc.exe") {
    $UseMsvc = $false
    Log "Using MSYS2/MinGW64 toolchain at $Msys2Root"
} elseif (Get-Command cl.exe -ErrorAction SilentlyContinue) {
    $UseMsvc = $true
    Log "Using MSVC toolchain (cl.exe found in PATH)"
} else {
    # Try to activate a VS installation.
    $VsWhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
    if (Test-Path $VsWhere) {
        $VsPath = & $VsWhere -latest -property installationPath 2>$null
        if ($VsPath) {
            $VcVarsAll = "$VsPath\VC\Auxiliary\Build\vcvars64.bat"
            if (Test-Path $VcVarsAll) {
                Log "Activating MSVC environment from $VcVarsAll ..."
                # Run vcvarsall and capture env changes.
                $TmpEnvFile = "$BuildTmp\vcvars_env.txt"
                cmd /c "`"$VcVarsAll`" && set > `"$TmpEnvFile`"" | Out-Null
                Get-Content $TmpEnvFile | ForEach-Object {
                    if ($_ -match "^([^=]+)=(.*)$") {
                        [System.Environment]::SetEnvironmentVariable($Matches[1], $Matches[2], "Process")
                    }
                }
                $UseMsvc = $true
                Log "MSVC environment activated"
            }
        }
    }
    if (-not $UseMsvc) {
        Die "No C++ compiler found.`nOptions:`n  A) Install MSYS2 and run: pacman -S mingw-w64-x86_64-gcc`n  B) Install Visual Studio with C++ workload`n  C) Run this script from 'Developer PowerShell for VS'"
    }
}

# ── CMake ──────────────────────────────────────────────────────────────────────
# For MSYS2, use the MinGW64 cmake to stay in the GNU toolchain world.
$CMakeExe = "cmake"
if (-not $UseMsvc -and (Test-Path "$Msys2Bin\cmake.exe")) {
    $CMakeExe = "$Msys2Bin\cmake.exe"
}
Chk $CMakeExe "install CMake from https://cmake.org/ or via MSYS2: pacman -S mingw-w64-x86_64-cmake"

# ── Git ────────────────────────────────────────────────────────────────────────
Chk git "install Git from https://git-scm.com/"

# ── Clone / update espeak-ng ───────────────────────────────────────────────────
$EspeakStamp = "$BuildTmp\espeak-cloned-$EspeakTag.stamp"
if (-not (Test-Path $EspeakStamp)) {
    if (Test-Path "$EspeakSrc\.git") {
        Log "Updating espeak-ng to $EspeakTag ..."
        Push-Location $EspeakSrc
        git fetch --depth 1 origin "refs/tags/$EspeakTag" 2>&1 | Out-Null
        git checkout FETCH_HEAD 2>&1 | Out-Null
        Pop-Location
    } else {
        Log "Cloning espeak-ng $EspeakTag ..."
        if (Test-Path $EspeakSrc) { Remove-Item -Recurse -Force $EspeakSrc }
        git clone --depth 1 --branch $EspeakTag $EspeakRepo $EspeakSrc
    }
    New-Item -ItemType File -Force -Path $EspeakStamp | Out-Null
    Ok "espeak-ng source ready"
} else {
    Ok "espeak-ng $EspeakTag already cloned"
}

# ── CMake configure ────────────────────────────────────────────────────────────
$BuildStamp = "$BuildTmp\espeak-build-windows.stamp"
if (-not (Test-Path $BuildStamp)) {
    Log "Configuring espeak-ng ..."
    if (Test-Path $BuildDir)   { Remove-Item -Recurse -Force $BuildDir }
    if (Test-Path $InstallDir) { Remove-Item -Recurse -Force $InstallDir }
    New-Item -ItemType Directory -Force -Path $BuildDir   | Out-Null
    New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null

    $CmakeArgs = @(
        "-S", $EspeakSrc,
        "-B", $BuildDir,
        "-DCMAKE_BUILD_TYPE=Release",
        "-DCMAKE_INSTALL_PREFIX=$InstallDir",
        "-DBUILD_SHARED_LIBS=OFF",
        "-DUSE_ASYNC=OFF",
        "-DWITH_ASYNC=OFF",
        "-DWITH_PCAUDIOLIB=OFF",
        "-DWITH_SPEECHPLAYER=OFF",
        "-DWITH_SONIC=OFF",
        "-DUSE_KLATT=OFF",
        "-DCMAKE_DISABLE_FIND_PACKAGE_SpeechPlayer=TRUE",
        "-DCMAKE_DISABLE_FIND_PACKAGE_PcAudio=TRUE",
        "-Wno-dev"
    )

    if (-not $UseMsvc) {
        # MinGW Makefiles — use the Ninja generator if available for speed.
        if (Get-Command ninja -ErrorAction SilentlyContinue) {
            $CmakeArgs += "-G", "Ninja"
        } else {
            $CmakeArgs += "-G", "MinGW Makefiles"
        }
        $CmakeArgs += "-DCMAKE_C_COMPILER=$Msys2Bin\gcc.exe"
        $CmakeArgs += "-DCMAKE_CXX_COMPILER=$Msys2Bin\g++.exe"
        $CmakeArgs += "-DCMAKE_AR=$Msys2Bin\ar.exe"
    } else {
        # MSVC — use the NMake or MSBuild generator.
        $CmakeArgs += "-G", "NMake Makefiles"
        $CmakeArgs += "-DCMAKE_C_COMPILER=cl"
        $CmakeArgs += "-DCMAKE_CXX_COMPILER=cl"
    }

    $CmakeLog = "$BuildDir\cmake.log"
    & $CMakeExe @CmakeArgs 2>&1 | Tee-Object -FilePath $CmakeLog | ForEach-Object {
        if ($_ -match "^-- ") { Write-Host $_ -ForegroundColor DarkGray }
    }
    if ($LASTEXITCODE -ne 0) {
        Get-Content $CmakeLog | Select-Object -Last 30
        Die "CMake configure failed (exit $LASTEXITCODE)"
    }
    Ok "CMake configured"

    # ── Build ─────────────────────────────────────────────────────────────────
    Log "Building espeak-ng (jobs: $Jobs) ..."
    $BuildLog = "$BuildDir\build.log"
    & $CMakeExe --build $BuildDir --config Release -- -j$Jobs 2>&1 | `
        Tee-Object -FilePath $BuildLog | `
        Where-Object { $_ -match "error:|warning:|Building" } | `
        ForEach-Object { Write-Host $_ }

    if ($LASTEXITCODE -ne 0) {
        Get-Content $BuildLog | Select-Object -Last 40
        Die "Build failed (exit $LASTEXITCODE)"
    }

    & $CMakeExe --install $BuildDir 2>&1 | Out-Null
    Ok "Build complete"
    New-Item -ItemType File -Force -Path $BuildStamp | Out-Null
} else {
    Ok "espeak-ng already built (delete $BuildStamp to rebuild)"
}

# ── Locate the static library ──────────────────────────────────────────────────
# MSVC produces espeak-ng.lib; MinGW produces libespeak-ng.a.
$LibName = if ($UseMsvc) { "espeak-ng.lib" } else { "libespeak-ng.a" }

$FoundLibs = @(Get-ChildItem -Path $BuildDir, $InstallDir -Recurse `
    -Include "*.lib", "*.a" -ErrorAction SilentlyContinue | Select-Object -ExpandProperty FullName)

if ($FoundLibs.Count -eq 0) {
    Die "No static library found after build under $BuildDir"
}

Log "Found $($FoundLibs.Count) static archive(s):"
$FoundLibs | ForEach-Object { Log "  $_" }

# ── Merge all companion libs into one ─────────────────────────────────────────
# On MinGW we use ar; on MSVC we use lib.exe.
$MergedLib = "$BuildTmp\${LibName}"

if ($UseMsvc) {
    $LibExe = Get-Command lib.exe -ErrorAction SilentlyContinue
    if ($LibExe) {
        Log "Merging with lib.exe -> $MergedLib"
        $LibArgs = @("/OUT:$MergedLib") + $FoundLibs
        & lib.exe @LibArgs
        if ($LASTEXITCODE -ne 0) { Die "lib.exe merge failed" }
    } else {
        # No lib.exe: just copy the main archive.
        $MainLib = $FoundLibs | Where-Object { $_ -match "espeak-ng.lib$" } | Select-Object -First 1
        if (-not $MainLib) { $MainLib = $FoundLibs[0] }
        Copy-Item -Force $MainLib $MergedLib
        Log "lib.exe not found — copied main archive only: $MainLib"
    }
} else {
    # GNU ar MRI script.
    $MriFile = "$BuildTmp\merge.mri"
    @("CREATE $MergedLib") + ($FoundLibs | ForEach-Object { "ADDLIB $_" }) + @("SAVE", "END") |
        Set-Content -Path $MriFile -Encoding ASCII

    $ArExe = if (Test-Path "$Msys2Bin\ar.exe") { "$Msys2Bin\ar.exe" } else { "ar" }
    & $ArExe -M "<$MriFile"
    if ($LASTEXITCODE -ne 0) { Die "ar MRI merge failed" }
}

Ok "Merged -> $MergedLib"

# ── Install to OutDir ──────────────────────────────────────────────────────────
Copy-Item -Force $MergedLib "$OutDir\$LibName"
$Size = (Get-Item "$OutDir\$LibName").Length / 1MB
Ok "Installed: $OutDir\$LibName  ($([math]::Round($Size, 1)) MB)"
Write-Host ""
