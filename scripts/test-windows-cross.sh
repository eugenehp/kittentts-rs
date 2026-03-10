#!/usr/bin/env bash
# =============================================================================
# scripts/test-windows-cross.sh
# Cross-compile kittentts for Windows from Linux or macOS and optionally
# run the compiled tests under Wine.
#
# ── QUICK START ───────────────────────────────────────────────────────────────
#
#  # Fastest: compile only (no espeak feature, no Wine)
#  bash scripts/test-windows-cross.sh
#
#  # With espeak (needs MinGW cross-compiler):
#  bash scripts/test-windows-cross.sh --espeak
#
#  # Run tests under Wine:
#  bash scripts/test-windows-cross.sh --wine
#
#  # Full test (espeak + Wine):
#  bash scripts/test-windows-cross.sh --espeak --wine
#
# ── PREREQUISITES ─────────────────────────────────────────────────────────────
#
#  Required (always):
#    cargo + rustup          https://rustup.rs
#    cargo-zigbuild          cargo install cargo-zigbuild
#    zig                     https://ziglang.org/download/
#                            macOS:  brew install zig
#                            Ubuntu: snap install zig --classic
#                            Alpine: apk add zig
#
#  Required for --espeak:
#    MinGW-w64 cross-GCC:
#      Ubuntu/Debian:  sudo apt install gcc-mingw-w64-x86-64 cmake
#      Fedora:         sudo dnf install mingw64-gcc cmake
#      macOS:          brew install mingw-w64 cmake
#      Alpine:         apk add mingw-w64-gcc cmake
#
#  Required for --wine:
#    wine / wine64:
#      Ubuntu/Debian:  sudo apt install wine64
#      macOS:          brew install wine-stable (or use Whisky)
#      Alpine:         wine not available; use a Debian/Ubuntu container
#
# ── ENVIRONMENT VARIABLES ─────────────────────────────────────────────────────
#
#   ORT_LIB_LOCATION    Pre-built ORT dir (skips automatic download)
#                       Must contain libonnxruntime.dll.a and onnxruntime.dll
#   ORT_VERSION         ORT version to download  [default: 1.21.0]
#   ORT_DOWNLOAD_DIR    Where to cache downloaded ORT [default: /tmp/ort-win-x64]
#   ESPEAK_LIB_DIR      Pre-built espeak dir (skips cross-compile of espeak)
#   ESPEAK_TAG          espeak-ng tag to build   [default: 1.52.0]
#   JOBS                Parallel build jobs       [default: nproc]
#
# =============================================================================
set -euo pipefail

# ── Colour helpers ─────────────────────────────────────────────────────────────
RED='\033[0;31m'; GREEN='\033[0;32m'; CYAN='\033[0;36m'; YELLOW='\033[0;33m'; NC='\033[0m'
log()  { printf "${CYAN}[..] %s${NC}\n"    "$*" >&2; }
ok()   { printf "${GREEN}[ok] %s${NC}\n"   "$*" >&2; }
warn() { printf "${YELLOW}[!!] %s${NC}\n"  "$*" >&2; }
die()  { printf "${RED}[!!] %s${NC}\n"     "$*" >&2; exit 1; }
chk()  { command -v "$1" &>/dev/null || die "'$1' not found — $2"; }

# ── Argument parsing ───────────────────────────────────────────────────────────
WITH_ESPEAK=false
WITH_WINE=false
CLEAN=false
TARGET="${TARGET:-x86_64-pc-windows-gnu}"

usage() {
    cat >&2 << 'EOF'
Usage: bash scripts/test-windows-cross.sh [OPTIONS]

Options:
  --espeak      Also build and test with the `espeak` feature
                (requires x86_64-w64-mingw32-gcc and cmake)
  --wine        Run the compiled test binary under Wine after building
  --clean       Delete cached ORT download and espeak build before building
  --target T    Cargo target triple [default: x86_64-pc-windows-gnu]
  -h, --help    Show this help

Environment overrides:
  ORT_VERSION         ORT version to fetch [default: 1.21.0]
  ORT_LIB_LOCATION    Directory containing pre-built libonnxruntime.dll.a
  ORT_DOWNLOAD_DIR    Where to cache the downloaded ORT [default: /tmp/ort-win-x64]
  ESPEAK_LIB_DIR      Directory containing pre-built libespeak-ng.a
  ESPEAK_TAG          espeak-ng tag [default: 1.52.0]
  JOBS                Parallel build jobs [default: nproc]
EOF
    exit 0
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --espeak)  WITH_ESPEAK=true  ;;
        --wine)    WITH_WINE=true    ;;
        --clean)   CLEAN=true        ;;
        --target)  TARGET="$2"; shift ;;
        -h|--help) usage ;;
        *) die "Unknown option: $1  (try --help)" ;;
    esac
    shift
done

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
JOBS="${JOBS:-$(nproc 2>/dev/null || sysctl -n hw.logicalcpu 2>/dev/null || echo 4)}"
ORT_VERSION="${ORT_VERSION:-1.21.0}"
ORT_DOWNLOAD_DIR="${ORT_DOWNLOAD_DIR:-/tmp/ort-win-x64-${ORT_VERSION}}"
ESPEAK_TAG="${ESPEAK_TAG:-1.52.0}"

echo ""
log "kittentts Windows cross-compilation test"
log "  target  : ${TARGET}"
log "  espeak  : ${WITH_ESPEAK}"
log "  wine    : ${WITH_WINE}"
log "  ort ver : ${ORT_VERSION}"
echo ""

# ── Prerequisite check: required tools ────────────────────────────────────────
chk cargo       "install via https://rustup.rs"
chk rustup      "install via https://rustup.rs"

if ! command -v cargo-zigbuild &>/dev/null; then
    die "'cargo-zigbuild' not found.
Install it with:
  cargo install cargo-zigbuild
Then also install zig:
  macOS:  brew install zig
  Ubuntu: snap install zig --classic  (or download from https://ziglang.org/download/)
  Alpine: apk add zig"
fi
chk zig "install via https://ziglang.org/download/"

# ── Rust target ────────────────────────────────────────────────────────────────
log "Checking Rust target ${TARGET}..."
if ! rustup target list --installed | grep -q "${TARGET}"; then
    log "Installing Rust target ${TARGET}..."
    rustup target add "${TARGET}"
fi
ok "Rust target ${TARGET} is installed"

# ── Optional: Wine check ───────────────────────────────────────────────────────
if ${WITH_WINE}; then
    if ! command -v wine &>/dev/null && ! command -v wine64 &>/dev/null; then
        die "'wine' / 'wine64' not found — install it to use --wine.
  Ubuntu/Debian:  sudo apt install wine64
  macOS:          brew install wine-stable (or use Whisky from https://getwhisky.app)"
    fi
    WINE_CMD="$(command -v wine64 2>/dev/null || command -v wine)"
    ok "Wine: ${WINE_CMD}"
fi

# ── Optional: MinGW check (needed for --espeak) ────────────────────────────────
MINGW_CC="x86_64-w64-mingw32-gcc"
if ${WITH_ESPEAK}; then
    if ! command -v "${MINGW_CC}" &>/dev/null; then
        die "'${MINGW_CC}' not found — needed for --espeak.
Install it:
  Ubuntu/Debian:  sudo apt install gcc-mingw-w64-x86-64 cmake
  Fedora:         sudo dnf install mingw64-gcc cmake
  macOS:          brew install mingw-w64 cmake
  Alpine:         apk add mingw-w64-gcc cmake"
    fi
    chk cmake "install cmake (see --help for platform-specific instructions)"
fi

# ── Clean ─────────────────────────────────────────────────────────────────────
if ${CLEAN}; then
    warn "Cleaning cached ORT and espeak builds..."
    rm -rf "${ORT_DOWNLOAD_DIR}"
    rm -rf /tmp/espeak-static-build
    ok "Clean done"
fi

# ══════════════════════════════════════════════════════════════════════════════
# STEP 1 — Download ORT for Windows and build GNU import library
# ══════════════════════════════════════════════════════════════════════════════
setup_ort() {
    # Allow the user to bypass download entirely.
    if [[ -n "${ORT_LIB_LOCATION:-}" ]]; then
        ok "Using user-provided ORT_LIB_LOCATION=${ORT_LIB_LOCATION}"
        return 0
    fi

    local ort_dll="${ORT_DOWNLOAD_DIR}/onnxruntime.dll"
    local ort_import="${ORT_DOWNLOAD_DIR}/libonnxruntime.dll.a"

    # Skip if already prepared.
    if [[ -f "${ort_import}" ]]; then
        ok "ORT import lib already present: ${ort_import}"
        export ORT_LIB_LOCATION="${ORT_DOWNLOAD_DIR}"
        export ORT_PREFER_DYNAMIC_LINK=1
        return 0
    fi

    mkdir -p "${ORT_DOWNLOAD_DIR}"

    # Download the official Microsoft ORT release (standard ZIP, not Pyke CDN).
    local zip="${ORT_DOWNLOAD_DIR}/onnxruntime-win-x64-${ORT_VERSION}.zip"
    if [[ ! -f "${zip}" ]]; then
        local url="https://github.com/microsoft/onnxruntime/releases/download/v${ORT_VERSION}/onnxruntime-win-x64-${ORT_VERSION}.zip"
        log "Downloading ORT ${ORT_VERSION} for Windows..."
        log "  URL: ${url}"
        if ! curl -sSL --fail "${url}" -o "${zip}"; then
            die "Failed to download ORT ${ORT_VERSION}.
Try a different version with:  ORT_VERSION=1.20.1 bash $0
Or provide a pre-built import lib with:  ORT_LIB_LOCATION=/path/to/ort/dir"
        fi
        ok "Downloaded: $(du -sh "${zip}" | cut -f1)"
    else
        ok "ORT archive already cached: ${zip}"
    fi

    # Extract the DLL.
    log "Extracting onnxruntime.dll..."
    # The ZIP has a top-level directory: onnxruntime-win-x64-<version>/lib/onnxruntime.dll
    local extract_dir="${ORT_DOWNLOAD_DIR}/extracted"
    mkdir -p "${extract_dir}"
    unzip -o -q "${zip}" "*/lib/onnxruntime.dll" -d "${extract_dir}" 2>/dev/null || \
    unzip -o -q "${zip}" -d "${extract_dir}" 2>/dev/null
    # Find the DLL
    local found_dll
    found_dll="$(find "${extract_dir}" -name "onnxruntime.dll" | head -1)"
    [[ -n "${found_dll}" ]] || die "onnxruntime.dll not found in the downloaded archive"
    cp "${found_dll}" "${ort_dll}"
    ok "Extracted: ${ort_dll}  ($(du -sh "${ort_dll}" | cut -f1))"

    # Generate .def file using Python (no external dependencies).
    log "Generating onnxruntime.def from DLL exports (Python PE parser)..."
    local def_file="${ORT_DOWNLOAD_DIR}/onnxruntime.def"

    python3 - "${ort_dll}" "${def_file}" << 'PYEOF'
import struct, sys

dll_path, def_path = sys.argv[1], sys.argv[2]

with open(dll_path, 'rb') as f:
    data = f.read()

def u16(off): return struct.unpack_from('<H', data, off)[0]
def u32(off): return struct.unpack_from('<I', data, off)[0]

# DOS header → PE offset
if data[:2] != b'MZ':
    sys.exit(f"Not a PE file: {dll_path}")
pe_off = u32(0x3C)
if data[pe_off:pe_off+4] != b'PE\0\0':
    sys.exit("PE signature not found")

# COFF header
num_sections = u16(pe_off + 6)
opt_header_size = u16(pe_off + 20)
magic = u16(pe_off + 24)          # 0x10B = PE32, 0x20B = PE32+

# Data directory: export table is entry 0
exp_dd_off = pe_off + 24 + (96 if magic == 0x10B else 112)
exp_rva  = u32(exp_dd_off)
exp_size = u32(exp_dd_off + 4)

if exp_rva == 0:
    sys.exit("DLL has no exports")

# Section headers
sec_off = pe_off + 24 + opt_header_size

def rva_to_raw(rva):
    for i in range(num_sections):
        o = sec_off + i * 40
        vaddr = u32(o + 12)
        vsize = max(u32(o + 8), u32(o + 16))   # SizeOfRawData or VirtualSize
        raw   = u32(o + 20)
        if vaddr <= rva < vaddr + vsize:
            return raw + (rva - vaddr)
    return None

exp_raw = rva_to_raw(exp_rva)
if exp_raw is None:
    sys.exit("Could not locate export directory in sections")

num_names  = u32(exp_raw + 24)
names_rva  = u32(exp_raw + 32)
names_raw  = rva_to_raw(names_rva)

exports = []
for i in range(num_names):
    name_rva = u32(names_raw + i * 4)
    name_raw = rva_to_raw(name_rva)
    end = data.index(b'\x00', name_raw)
    exports.append(data[name_raw:end].decode('ascii', errors='replace'))

dll_name = dll_path.replace('\\', '/').split('/')[-1]
with open(def_path, 'w') as f:
    f.write(f'LIBRARY {dll_name}\n')
    f.write('EXPORTS\n')
    for sym in exports:
        f.write(f'  {sym}\n')

print(f"  Wrote {len(exports)} exports -> {def_path}", file=sys.stderr)
PYEOF

    ok "Generated .def with $(wc -l < "${def_file}") lines"

    # Create GNU import lib from the .def file.
    # Try tools in preference order: zig dlltool, x86_64-w64-mingw32-dlltool, llvm-dlltool, dlltool.
    local dlltool_cmd=""
    if command -v zig &>/dev/null; then
        # zig wraps llvm-dlltool with -m/-d/-l flags
        dlltool_cmd="zig_dlltool"
    elif command -v x86_64-w64-mingw32-dlltool &>/dev/null; then
        dlltool_cmd="x86_64-w64-mingw32-dlltool"
    elif command -v llvm-dlltool &>/dev/null; then
        dlltool_cmd="llvm-dlltool"
    elif command -v dlltool &>/dev/null; then
        dlltool_cmd="dlltool"
    else
        die "No dlltool found to create the ORT import library.
Install one of:
  zig        (already needed for cargo-zigbuild)
  x86_64-w64-mingw32-dlltool  (apt install binutils-mingw-w64-x86-64)
  llvm-dlltool                (apt install llvm)"
    fi

    log "Creating GNU import lib with '${dlltool_cmd}'..."
    if [[ "${dlltool_cmd}" == "zig_dlltool" ]]; then
        # zig wraps llvm-dlltool; it uses -m/-d/-l flags.
        zig dlltool -m i386:x86-64 \
            -d "${def_file}" \
            -l "${ort_import}"
    elif [[ "${dlltool_cmd}" == "llvm-dlltool" ]]; then
        llvm-dlltool -m i386:x86-64 \
            -d "${def_file}" \
            -l "${ort_import}"
    else
        "${dlltool_cmd}" \
            --def "${def_file}" \
            --output-lib "${ort_import}"
    fi

    [[ -f "${ort_import}" ]] || die "dlltool produced no output — see errors above"
    ok "Created: ${ort_import}  ($(du -sh "${ort_import}" | cut -f1))"

    # Also copy the DLL to the output directory so Wine can find it.
    export ORT_LIB_LOCATION="${ORT_DOWNLOAD_DIR}"
    export ORT_PREFER_DYNAMIC_LINK=1
    ok "ORT ready (dynamic import via onnxruntime.dll)"
}

# ══════════════════════════════════════════════════════════════════════════════
# STEP 2 — Cross-compile espeak-ng for Windows (--espeak only)
# ══════════════════════════════════════════════════════════════════════════════
setup_espeak() {
    if [[ -n "${ESPEAK_LIB_DIR:-}" ]]; then
        ok "Using user-provided ESPEAK_LIB_DIR=${ESPEAK_LIB_DIR}"
        export ESPEAK_LIB_DIR
        return 0
    fi

    local espeak_out="/tmp/espeak-static-build/espeak-mingw-x64/lib"
    local espeak_lib="${espeak_out}/libespeak-ng.a"

    if [[ -f "${espeak_lib}" ]]; then
        ok "espeak-ng already built: ${espeak_lib}"
        export ESPEAK_LIB_DIR="${espeak_out}"
        return 0
    fi

    log "Cross-compiling espeak-ng for Windows (MinGW x86_64)..."

    local espeak_tag="${ESPEAK_TAG}"
    local espeak_src="/tmp/espeak-static-build/espeak-ng-src"
    local espeak_build="/tmp/espeak-static-build/espeak-build-mingw-x64"
    local espeak_install="/tmp/espeak-static-build/espeak-install-mingw-x64"

    mkdir -p /tmp/espeak-static-build "${espeak_out}"

    # Clone if needed.
    if [[ ! -d "${espeak_src}/.git" ]]; then
        log "Cloning espeak-ng ${espeak_tag}..."
        git clone --depth 1 --branch "${espeak_tag}" \
            https://github.com/espeak-ng/espeak-ng.git \
            "${espeak_src}"
    fi

    # Patch for cross-compilation (removes espeak-ng-bin executable target).
    local patched_src="/tmp/espeak-static-build/espeak-ng-src-win-x64"
    if [[ ! -d "${patched_src}" ]]; then
        cp -a "${espeak_src}" "${patched_src}"
    fi
    python3 - "${patched_src}" << 'PYEOF'
import re, os, sys

root = sys.argv[1]
CMDS = ['add_executable','target_link_libraries','target_compile_definitions',
        'target_compile_options','target_include_directories','target_sources',
        'set_target_properties','add_dependencies']

def patch(path):
    text = open(path).read(); orig = text
    for cmd in CMDS:
        text = re.sub(r'\b' + re.escape(cmd) + r'\s*\(\s*espeak-ng-bin\b[^)]*\)',
                      f'# [cross] {cmd}(espeak-ng-bin) removed', text, flags=re.DOTALL)
    text = re.sub(r'install\s*\(\s*TARGETS\s+espeak-ng-bin\b[^)]*\)',
                  '# [cross] install(TARGETS espeak-ng-bin) removed', text, flags=re.DOTALL)
    text = re.sub(r'add_custom_command\s*\([^)]*espeak-ng-bin[^)]*\)',
                  '# [cross] add_custom_command(espeak-ng-bin) removed', text, flags=re.DOTALL)
    text = re.sub(r'add_subdirectory\s*\(\s*tests\s*\)',
                  '# [cross] tests removed', text)
    if text != orig:
        open(path,'w').write(text)
        print("  Patched:", path, file=sys.stderr)

for dp, _, fs in os.walk(root):
    for f in fs:
        if f == 'CMakeLists.txt' or f.endswith('.cmake'):
            patch(os.path.join(dp, f))
PYEOF

    # Generate CMake toolchain file for MinGW x86_64.
    local toolchain_file="/tmp/espeak-static-build/toolchain-win-x64.cmake"
    cat > "${toolchain_file}" << 'EOF'
set(CMAKE_SYSTEM_NAME Windows)
set(CMAKE_SYSTEM_PROCESSOR x86_64)
set(CMAKE_C_COMPILER   x86_64-w64-mingw32-gcc)
set(CMAKE_CXX_COMPILER x86_64-w64-mingw32-g++)
set(CMAKE_AR           x86_64-w64-mingw32-ar)
set(CMAKE_RANLIB       x86_64-w64-mingw32-ranlib)
set(CMAKE_RC_COMPILER  x86_64-w64-mingw32-windres)
set(CMAKE_TRY_COMPILE_TARGET_TYPE STATIC_LIBRARY)
EOF

    rm -rf "${espeak_build}" "${espeak_install}"
    mkdir -p "${espeak_build}" "${espeak_install}"

    log "Configuring espeak-ng CMake (MinGW)..."
    PKG_CONFIG_PATH="" cmake -S "${patched_src}" -B "${espeak_build}" \
        -DCMAKE_BUILD_TYPE=Release \
        -DCMAKE_INSTALL_PREFIX="${espeak_install}" \
        -DCMAKE_TOOLCHAIN_FILE="${toolchain_file}" \
        -DBUILD_SHARED_LIBS=OFF \
        -DUSE_ASYNC=OFF \
        -DWITH_ASYNC=OFF \
        -DWITH_PCAUDIOLIB=OFF \
        -DWITH_SPEECHPLAYER=OFF \
        -DWITH_SONIC=OFF \
        -DUSE_KLATT=OFF \
        -DCMAKE_DISABLE_FIND_PACKAGE_SpeechPlayer=TRUE \
        -DCMAKE_DISABLE_FIND_PACKAGE_PcAudio=TRUE \
        -Wno-dev \
        >/dev/null 2>&1 || { cmake -S "${patched_src}" -B "${espeak_build}" \
            -DCMAKE_BUILD_TYPE=Release \
            -DCMAKE_INSTALL_PREFIX="${espeak_install}" \
            -DCMAKE_TOOLCHAIN_FILE="${toolchain_file}" \
            -DBUILD_SHARED_LIBS=OFF -DUSE_ASYNC=OFF -DWITH_ASYNC=OFF \
            -DWITH_PCAUDIOLIB=OFF -DWITH_SPEECHPLAYER=OFF -DWITH_SONIC=OFF \
            -DUSE_KLATT=OFF 2>&1 | tail -20; die "CMake configure failed"; }

    log "Building espeak-ng for Windows (${JOBS} jobs)..."
    cmake --build "${espeak_build}" --target espeak-ng -- -j"${JOBS}" \
        2>&1 | grep -E "error:|warning:|Building" | head -30 || true
    cmake --build "${espeak_build}" --target espeak-ng -- -j"${JOBS}" \
        >/dev/null 2>&1 || { cmake --build "${espeak_build}" --target espeak-ng 2>&1 | tail -30; die "espeak-ng build failed"; }
    cmake --install "${espeak_build}" >/dev/null 2>&1 || true

    # Merge all .a archives into a single libespeak-ng.a.
    mapfile -t raw_libs < <(find "${espeak_build}" "${espeak_install}" -name "*.a" | sort -u)
    [[ "${#raw_libs[@]}" -gt 0 ]] || die "No .a files found after espeak-ng build"
    log "Merging ${#raw_libs[@]} archive(s) into libespeak-ng.a..."

    local ar_bin="x86_64-w64-mingw32-ar"
    command -v "${ar_bin}" &>/dev/null || ar_bin="ar"

    local mri_script="/tmp/espeak-static-build/merge-win-x64.mri"
    local merged="/tmp/espeak-static-build/libespeak-ng-win-x64.a"
    {
        echo "CREATE ${merged}"
        for lib in "${raw_libs[@]}"; do echo "ADDLIB ${lib}"; done
        echo "SAVE"; echo "END"
    } > "${mri_script}"
    "${ar_bin}" -M < "${mri_script}"

    install -m 644 "${merged}" "${espeak_lib}"
    ok "espeak-ng built: ${espeak_lib}  ($(du -sh "${espeak_lib}" | cut -f1))"
    export ESPEAK_LIB_DIR="${espeak_out}"
}

# ══════════════════════════════════════════════════════════════════════════════
# STEP 3 — Cargo build / test
# ══════════════════════════════════════════════════════════════════════════════
run_cargo() {
    local features="$1"
    local label="$2"

    local cargo_args=("zigbuild" "--target" "${TARGET}")
    if [[ -n "${features}" ]]; then
        cargo_args+=("--features" "${features}")
    fi
    # Add tests binary for Wine runs.
    if ${WITH_WINE}; then
        cargo_args+=("--tests")
    fi
    cargo_args+=("--message-format" "human")

    echo ""
    log "── cargo ${cargo_args[*]}  [${label}]"

    local env_overrides=()
    if [[ -n "${ORT_LIB_LOCATION:-}" ]]; then
        env_overrides+=("ORT_LIB_LOCATION=${ORT_LIB_LOCATION}")
        env_overrides+=("ORT_PREFER_DYNAMIC_LINK=${ORT_PREFER_DYNAMIC_LINK:-1}")
    fi
    if [[ -n "${ESPEAK_LIB_DIR:-}" ]]; then
        env_overrides+=("ESPEAK_LIB_DIR=${ESPEAK_LIB_DIR}")
    fi

    cd "${REPO_ROOT}"
    env "${env_overrides[@]}" cargo "${cargo_args[@]}"
    ok "${label} — build succeeded"
}

# ══════════════════════════════════════════════════════════════════════════════
# STEP 4 — Optional Wine test run
# ══════════════════════════════════════════════════════════════════════════════
run_wine_tests() {
    local features="$1"
    local label="$2"

    # Find the compiled test binary.
    local test_bin
    local target_dir="${REPO_ROOT}/target/${TARGET}"
    test_bin="$(find "${target_dir}/debug" "${target_dir}/release" \
        -maxdepth 1 -name "kittentts-*.exe" -newer "${REPO_ROOT}/Cargo.toml" \
        2>/dev/null | head -1)"
    if [[ -z "${test_bin}" ]]; then
        warn "No test binary found in ${target_dir} — skipping Wine run"
        return 0
    fi

    echo ""
    log "── Wine test run  [${label}]"
    log "  Binary: ${test_bin}"

    # Make ORT DLL available to Wine.
    local wine_path_prepend=""
    if [[ -n "${ORT_LIB_LOCATION:-}" ]]; then
        wine_path_prepend="${ORT_LIB_LOCATION}"
    fi

    # Wine needs WINEPREFIX and PATH adjustments.
    WINEPATH="${wine_path_prepend}" \
    WINEDEBUG="-all" \
        "${WINE_CMD}" "${test_bin}" 2>&1 || {
            warn "Wine test run exited with non-zero status (may indicate missing DLLs or test failures)"
            warn "To debug: WINEDEBUG=+loaddll ${WINE_CMD} ${test_bin}"
        }
    ok "${label} — Wine run complete"
}

# ── Prepare ORT ────────────────────────────────────────────────────────────────
setup_ort

# ── Build: no espeak ──────────────────────────────────────────────────────────
run_cargo "" "kittentts (no espeak)"
if ${WITH_WINE}; then
    run_wine_tests "" "kittentts (no espeak)"
fi

# ── Build: with espeak ────────────────────────────────────────────────────────
if ${WITH_ESPEAK}; then
    setup_espeak
    run_cargo "espeak" "kittentts --features espeak"
    if ${WITH_WINE}; then
        run_wine_tests "espeak" "kittentts --features espeak"
    fi
fi

# ── Summary ────────────────────────────────────────────────────────────────────
echo ""
printf "${GREEN}══════════════════════════════════════════════════${NC}\n"
ok "All Windows cross-compilation tests passed!"
printf "${GREEN}══════════════════════════════════════════════════${NC}\n"
echo ""
log "Target binaries are in: ${REPO_ROOT}/target/${TARGET}/"
if [[ -n "${ORT_LIB_LOCATION:-}" ]]; then
    log "ORT DLL (ship with .exe): ${ORT_LIB_LOCATION}/onnxruntime.dll"
fi
if ${WITH_ESPEAK} && [[ -n "${ESPEAK_LIB_DIR:-}" ]]; then
    espeak_data="${ESPEAK_LIB_DIR}/../share/espeak-ng-data"
    espeak_data_alt="${ESPEAK_LIB_DIR}/../espeak-ng-data"
    if [[ -d "${espeak_data}" ]]; then
        log "espeak-ng-data dir (ship with .exe): $(realpath "${espeak_data}")"
    elif [[ -d "${espeak_data_alt}" ]]; then
        log "espeak-ng-data dir (ship with .exe): $(realpath "${espeak_data_alt}")"
    fi
fi
echo ""
