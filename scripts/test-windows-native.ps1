# =============================================================================
# scripts/test-windows-native.ps1
# Build and test kittentts natively on Windows.
#
# Supports both MSVC (cargo default on Windows) and MinGW/MSYS2 toolchains.
# The `espeak` feature triggers the automatic espeak-ng build in build.rs
# (clones from GitHub and builds with cmake) — no pre-installed espeak required.
#
# ── QUICK START ───────────────────────────────────────────────────────────────
#
#   # Open a PowerShell prompt (not necessarily Developer PowerShell) and run:
#
#   # Basic build + test (no espeak):
#   powershell -ExecutionPolicy Bypass -File scripts\test-windows-native.ps1
#
#   # Full test including espeak feature (auto-builds espeak-ng):
#   powershell -ExecutionPolicy Bypass -File scripts\test-windows-native.ps1 -Espeak
#
#   # Verbose output:
#   powershell -ExecutionPolicy Bypass -File scripts\test-windows-native.ps1 -Espeak -Verbose
#
#   # Clean Cargo target and rebuild from scratch:
#   powershell -ExecutionPolicy Bypass -File scripts\test-windows-native.ps1 -Clean -Espeak
#
# ── PREREQUISITES ─────────────────────────────────────────────────────────────
#
#  Required (always):
#    Rust + cargo   https://rustup.rs  — run the installer and follow prompts
#
#  Required for -Espeak:
#    git            https://git-scm.com/download/win
#                   Or: winget install --id Git.Git
#    cmake          https://cmake.org/download/
#                   Or: winget install --id Kitware.CMake
#
#  One C compiler — choose A, B, or C:
#    A. MSVC   — Visual Studio 2019/2022 with "Desktop development with C++"
#               (already present on GitHub Actions windows-latest)
#    B. MinGW  — MSYS2 from https://www.msys2.org/
#               In MSYS2 MinGW64 shell:
#                 pacman -S mingw-w64-x86_64-toolchain mingw-w64-x86_64-cmake
#    C. winget — winget install --id MSYS2.MSYS2
#               then follow Option B above
#
# ── ENVIRONMENT VARIABLES ─────────────────────────────────────────────────────
#
#   ESPEAK_LIB_DIR     Pre-built espeak lib dir (skips auto-build)
#   ESPEAK_TAG         espeak-ng tag to build [default: 1.52.0]
#   ORT_LIB_LOCATION   Pre-built ORT dir (skips ort-sys download-binaries)
#   MSYS2_PATH         MSYS2 root [default: C:\msys64]
#
# =============================================================================
param(
    [switch]$Espeak,       # Also build and test with --features espeak
    [switch]$EspeakOnly,   # Build ONLY with espeak (skip the no-espeak run)
    [switch]$Clean,        # Run `cargo clean` before building
    [switch]$BuildOnly,    # Build only; do not run tests (cargo build instead of cargo test)
    [switch]$Verbose,      # Print full cargo output
    [switch]$Help
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

# ── Colour helpers ─────────────────────────────────────────────────────────────
function Log   { param([string]$Msg) Write-Host "[..] $Msg" -ForegroundColor Cyan }
function Ok    { param([string]$Msg) Write-Host "[ok] $Msg" -ForegroundColor Green }
function Warn  { param([string]$Msg) Write-Host "[!!] $Msg" -ForegroundColor Yellow }
function Die   { param([string]$Msg) Write-Host "[!!] $Msg" -ForegroundColor Red; exit 1 }
function Sep   { Write-Host ("─" * 60) -ForegroundColor DarkGray }

if ($Help) {
    Get-Help $MyInvocation.MyCommand.Path -Full
    exit 0
}

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$RepoRoot  = Split-Path -Parent $ScriptDir

$EspeakTag   = if ($env:ESPEAK_TAG)    { $env:ESPEAK_TAG }    else { "1.52.0" }
$Msys2Root   = if ($env:MSYS2_PATH)    { $env:MSYS2_PATH }    else { "C:\msys64" }
$Msys2Bin    = "$Msys2Root\mingw64\bin"

Write-Host ""
Write-Host "══════════════════════════════════════════════════" -ForegroundColor Green
Write-Host "  kittentts Windows Native Build & Test" -ForegroundColor White
Write-Host "══════════════════════════════════════════════════" -ForegroundColor Green
Log "Repo     : $RepoRoot"
Log "Espeak   : $($Espeak -or $EspeakOnly)"
Log "BuildOnly: $BuildOnly"
Log "Clean    : $Clean"
Write-Host ""

# ── Track results ──────────────────────────────────────────────────────────────
$Results = [System.Collections.Generic.List[PSCustomObject]]::new()

function Add-Result {
    param([string]$Step, [bool]$Passed, [string]$Detail = "")
    $Results.Add([PSCustomObject]@{ Step = $Step; Passed = $Passed; Detail = $Detail })
}

# ══════════════════════════════════════════════════════════════════════════════
# STEP 1 — Prerequisite checks
# ══════════════════════════════════════════════════════════════════════════════
Sep
Log "Checking prerequisites..."

# cargo / rustc
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    Die @"
'cargo' not found. Install Rust from https://rustup.rs and re-run.
  Quick install (PowerShell):
    Invoke-WebRequest https://win.rustup.rs -OutFile rustup-init.exe
    .\rustup-init.exe -y
    # Then open a new PowerShell window and re-run this script.
"@
}
$RustVersion = & rustc --version 2>$null
Ok "Rust: $RustVersion"

# Detect target (MSVC or GNU)
$CargoTarget = & rustc -vV 2>$null | Select-String "^host:" | ForEach-Object { ($_ -split "\s+")[1] }
if (-not $CargoTarget) { $CargoTarget = "x86_64-pc-windows-msvc" }
Log "Host target: $CargoTarget"
$IsMsvc = $CargoTarget -match "msvc"

# For espeak auto-build, cmake and git are required.
if ($Espeak -or $EspeakOnly) {
    # git
    if (-not (Get-Command git -ErrorAction SilentlyContinue)) {
        Die @"
'git' not found — required for the espeak-ng auto-build.
Install options:
  winget install --id Git.Git
  OR download from https://git-scm.com/download/win
"@
    }
    Ok "git: $(& git --version 2>$null)"

    # cmake — check system PATH first, then MSYS2.
    $CmakeExe = $null
    if (Get-Command cmake -ErrorAction SilentlyContinue) {
        $CmakeExe = "cmake"
    } elseif (Test-Path "$Msys2Bin\cmake.exe") {
        $CmakeExe = "$Msys2Bin\cmake.exe"
        $env:PATH = "$Msys2Bin;$env:PATH"
    }
    if (-not $CmakeExe) {
        Die @"
'cmake' not found — required for the espeak-ng auto-build.
Install options:
  winget install --id Kitware.CMake
  OR install via MSYS2: pacman -S mingw-w64-x86_64-cmake
  OR download from https://cmake.org/download/
"@
    }
    Ok "cmake: $(& $CmakeExe --version 2>$null | Select-Object -First 1)"

    # C compiler: MSVC (cl.exe) or MinGW (gcc.exe)
    $HaveCompiler = $false
    if (Get-Command cl -ErrorAction SilentlyContinue) {
        Ok "C compiler: MSVC (cl.exe in PATH)"
        $HaveCompiler = $true
    } elseif (Test-Path "$Msys2Bin\gcc.exe") {
        Ok "C compiler: MinGW-w64 gcc at $Msys2Bin"
        $env:PATH = "$Msys2Bin;$env:PATH"
        $HaveCompiler = $true
    } else {
        # Try to locate and activate the latest VS installation.
        $VsWhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
        if (Test-Path $VsWhere) {
            $VsPath = & $VsWhere -latest -property installationPath 2>$null
            if ($VsPath) {
                $VcVarsAll = "$VsPath\VC\Auxiliary\Build\vcvars64.bat"
                if (Test-Path $VcVarsAll) {
                    Log "Activating MSVC environment from VS at $VsPath ..."
                    $TmpEnv = [System.IO.Path]::GetTempFileName() + ".txt"
                    cmd /c "`"$VcVarsAll`" && set > `"$TmpEnv`"" | Out-Null
                    Get-Content $TmpEnv | ForEach-Object {
                        if ($_ -match "^([^=]+)=(.*)$") {
                            [System.Environment]::SetEnvironmentVariable($Matches[1], $Matches[2], "Process")
                        }
                    }
                    Remove-Item $TmpEnv -ErrorAction SilentlyContinue
                    Ok "C compiler: MSVC activated from $VcVarsAll"
                    $HaveCompiler = $true
                }
            }
        }
        if (-not $HaveCompiler) {
            Die @"
No C compiler found — required for the espeak-ng auto-build.
Options:
  A) Install Visual Studio 2019/2022 with 'Desktop development with C++'
     https://visualstudio.microsoft.com/
  B) Install MSYS2 from https://www.msys2.org/  then in MSYS2 MinGW64 shell:
     pacman -S mingw-w64-x86_64-toolchain mingw-w64-x86_64-cmake
  C) Run this script from a 'Developer PowerShell for VS' prompt
"@
        }
    }
}

# ══════════════════════════════════════════════════════════════════════════════
# STEP 2 — Optional: cargo clean
# ══════════════════════════════════════════════════════════════════════════════
if ($Clean) {
    Sep
    Log "Running cargo clean..."
    Set-Location $RepoRoot
    & cargo clean
    if ($LASTEXITCODE -ne 0) { Die "cargo clean failed" }
    Ok "cargo clean done"
}

# ══════════════════════════════════════════════════════════════════════════════
# Helper: run a cargo command with optional live output
# ══════════════════════════════════════════════════════════════════════════════
function Invoke-Cargo {
    param(
        [string[]]$CargoArgs,
        [string]$Label,
        [hashtable]$ExtraEnv = @{}
    )

    Sep
    Log "── cargo $($CargoArgs -join ' ')  [$Label]"

    # Save and set extra env vars.
    $SavedEnv = @{}
    foreach ($kv in $ExtraEnv.GetEnumerator()) {
        $SavedEnv[$kv.Key] = [System.Environment]::GetEnvironmentVariable($kv.Key, "Process")
        [System.Environment]::SetEnvironmentVariable($kv.Key, $kv.Value, "Process")
    }

    Set-Location $RepoRoot
    $StartTime = Get-Date

    if ($Verbose) {
        & cargo @CargoArgs
    } else {
        # Filter to errors and summary lines only.
        & cargo @CargoArgs 2>&1 | ForEach-Object {
            if ($_ -match "error\[|error:|^test |FAILED|PASSED|ok$|running \d|warning:") {
                Write-Host $_
            }
        }
    }
    $ExitCode = $LASTEXITCODE
    $Elapsed  = [math]::Round(((Get-Date) - $StartTime).TotalSeconds, 1)

    # Restore env.
    foreach ($kv in $SavedEnv.GetEnumerator()) {
        [System.Environment]::SetEnvironmentVariable($kv.Key, $kv.Value, "Process")
    }

    if ($ExitCode -ne 0) {
        Warn "$Label FAILED (exit $ExitCode, ${Elapsed}s)"
        Add-Result -Step $Label -Passed $false -Detail "exit code $ExitCode"
        return $false
    }
    Ok "$Label passed (${Elapsed}s)"
    Add-Result -Step $Label -Passed $true -Detail "${Elapsed}s"
    return $true
}

# ══════════════════════════════════════════════════════════════════════════════
# STEP 3 — Build / test WITHOUT espeak (unless -EspeakOnly)
# ══════════════════════════════════════════════════════════════════════════════
if (-not $EspeakOnly) {
    $Cmd = if ($BuildOnly) { "build" } else { "test" }
    $Label = "kittentts (no espeak)"

    $CargoArgs = @($Cmd)
    if (-not $BuildOnly) {
        # Run all tests and show output for failures.
        $CargoArgs += "--no-fail-fast"
    }

    $ExtraEnv = @{}
    if ($env:ORT_LIB_LOCATION) { $ExtraEnv["ORT_LIB_LOCATION"] = $env:ORT_LIB_LOCATION }

    $ok = Invoke-Cargo -CargoArgs $CargoArgs -Label $Label -ExtraEnv $ExtraEnv
    if (-not $ok -and -not ($Espeak -or $EspeakOnly)) {
        # If no espeak run is planned, fail fast.
        Write-Host ""
        Die "Build failed — see errors above"
    }
}

# ══════════════════════════════════════════════════════════════════════════════
# STEP 4 — Build / test WITH espeak
# ══════════════════════════════════════════════════════════════════════════════
if ($Espeak -or $EspeakOnly) {
    $Cmd   = if ($BuildOnly) { "build" } else { "test" }
    $Label = "kittentts --features espeak"

    $CargoArgs = @($Cmd, "--features", "espeak")
    if (-not $BuildOnly) {
        $CargoArgs += "--no-fail-fast"
    }

    $ExtraEnv = @{}
    if ($env:ESPEAK_LIB_DIR)    { $ExtraEnv["ESPEAK_LIB_DIR"]    = $env:ESPEAK_LIB_DIR    }
    if ($env:ESPEAK_TAG)        { $ExtraEnv["ESPEAK_TAG"]         = $env:ESPEAK_TAG         }
    if ($env:ORT_LIB_LOCATION)  { $ExtraEnv["ORT_LIB_LOCATION"]  = $env:ORT_LIB_LOCATION   }

    Sep
    Log "NOTE: The `espeak` feature will trigger the auto-build of espeak-ng from source."
    Log "      This clones https://github.com/espeak-ng/espeak-ng and compiles with cmake."
    Log "      First build takes ~3-10 minutes.  Subsequent builds are instant (stamp file)."
    Write-Host ""

    $ok = Invoke-Cargo -CargoArgs $CargoArgs -Label $Label -ExtraEnv $ExtraEnv
    if (-not $ok) {
        # Diagnose common failures.
        Write-Host ""
        Warn "espeak build/test failed.  Common causes:"
        Warn "  1) git not in PATH — install from https://git-scm.com/download/win"
        Warn "  2) cmake not in PATH — install from https://cmake.org/download/"
        Warn "  3) No C compiler — install Visual Studio or MSYS2 MinGW"
        Warn "  4) Network blocked — set ESPEAK_LIB_DIR to a pre-built espeak-ng.lib"
        Write-Host ""
        Warn "Alternatively, pre-build espeak-ng with:"
        Warn "  `$env:ESPEAK_LIB_DIR = `"`$PWD\espeak-static\lib`""
        Warn "  powershell -ExecutionPolicy Bypass -File scripts\build-espeak-static.ps1"
    }
}

# ══════════════════════════════════════════════════════════════════════════════
# STEP 5 — Print summary
# ══════════════════════════════════════════════════════════════════════════════
Write-Host ""
Write-Host "══════════════════════════════════════════════════" -ForegroundColor Green
Write-Host "  SUMMARY" -ForegroundColor White
Write-Host "══════════════════════════════════════════════════" -ForegroundColor Green

$AllPassed = $true
foreach ($r in $Results) {
    if ($r.Passed) {
        Write-Host "  [PASS] $($r.Step)  ($($r.Detail))" -ForegroundColor Green
    } else {
        Write-Host "  [FAIL] $($r.Step)  ($($r.Detail))" -ForegroundColor Red
        $AllPassed = $false
    }
}

Write-Host ""
if ($AllPassed -and $Results.Count -gt 0) {
    Write-Host "  All checks passed!" -ForegroundColor Green
} elseif ($Results.Count -eq 0) {
    Warn "  No checks ran — did you mean to pass -Espeak?"
} else {
    Write-Host "  Some checks FAILED — see output above." -ForegroundColor Red
}
Write-Host "══════════════════════════════════════════════════" -ForegroundColor Green
Write-Host ""

# Distribution reminder when espeak was used.
if ($Espeak -or $EspeakOnly) {
    Log "Distribution reminder:"
    Log "  When shipping the .exe, copy 'espeak-ng-data\' next to it."
    Log "  The build system placed the data at:"
    $CandidateDataDirs = @(
        "$RepoRoot\target\release\espeak-ng-data",
        "$RepoRoot\target\debug\espeak-ng-data"
    )
    foreach ($d in $CandidateDataDirs) {
        if (Test-Path $d) { Log "    $d" }
    }
    # Also check Cargo's OUT_DIR (auto-build installs data there).
    $AutoBuildBase = "$env:LOCALAPPDATA\Cargo\registry"  # approximate
    $CargoOutDirs = Get-ChildItem "$RepoRoot\target\x86_64-pc-windows-*\*\build\kittentts-*\out\espeak-auto\*\install\share\espeak-ng-data" `
        -ErrorAction SilentlyContinue
    foreach ($d in $CargoOutDirs) {
        Log "    $($d.FullName)"
    }
    Write-Host ""
}

if (-not $AllPassed -and $Results.Count -gt 0) { exit 1 }
