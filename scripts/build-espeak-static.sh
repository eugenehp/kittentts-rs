#!/usr/bin/env bash
# =============================================================================
# scripts/build-espeak-static.sh
# Build libespeak-ng.a for any supported host/target combination.
#
# Invoked automatically by build.rs when ESPEAK_LIB_DIR is set but the
# expected static archive is absent, and ESPEAK_BUILD_SCRIPT points here.
# Can also be run manually for any target (see examples below).
#
# ── Quick-start examples ──────────────────────────────────────────────────────
#
#  Native (macOS / Linux):
#    ESPEAK_LIB_DIR=$PWD/espeak-static/lib \
#      bash scripts/build-espeak-static.sh
#
#  Linux → Linux aarch64  (requires: apt install gcc-aarch64-linux-gnu):
#    ESPEAK_LIB_DIR=$PWD/espeak-static/aarch64/lib \
#    ESPEAK_TARGET=aarch64-unknown-linux-gnu \
#      bash scripts/build-espeak-static.sh
#
#  Linux → Linux armv7    (requires: apt install gcc-arm-linux-gnueabihf):
#    ESPEAK_LIB_DIR=$PWD/espeak-static/armv7/lib \
#    ESPEAK_TARGET=armv7-unknown-linux-gnueabihf \
#      bash scripts/build-espeak-static.sh
#
#  Linux → Linux x86_64 musl  (requires: apt install musl-tools):
#    ESPEAK_LIB_DIR=$PWD/espeak-static/musl/lib \
#    ESPEAK_TARGET=x86_64-unknown-linux-musl \
#      bash scripts/build-espeak-static.sh
#
#  Linux → Android arm64  (requires: ANDROID_NDK_HOME set):
#    ESPEAK_LIB_DIR=$PWD/espeak-static/android/lib \
#    ESPEAK_TARGET=aarch64-linux-android \
#    ANDROID_NDK_HOME=/path/to/ndk \
#      bash scripts/build-espeak-static.sh
#
#  macOS → macOS arm64 (native on Apple Silicon, or cross on Intel):
#    ESPEAK_LIB_DIR=$PWD/espeak-static/lib \
#    ESPEAK_TARGET=aarch64-apple-darwin \
#      bash scripts/build-espeak-static.sh
#
# ── Environment variables ─────────────────────────────────────────────────────
#   ESPEAK_LIB_DIR      Output directory for libespeak-ng.a  [REQUIRED]
#   ESPEAK_TARGET       Cargo target triple                  [default: host]
#   ESPEAK_TARGET_OS    OS component  (set by build.rs)      [derived]
#   ESPEAK_TARGET_ARCH  Arch component (set by build.rs)     [derived]
#   ESPEAK_SYSROOT      Sysroot for cross-compilation        [optional]
#   ESPEAK_TAG          espeak-ng release tag                [default: 1.52.0]
#   BUILD_TMP           Scratch directory                    [default: /tmp/espeak-static-build]
#   ANDROID_NDK_HOME    NDK root (also ANDROID_NDK_ROOT / NDK_HOME)
#   ANDROID_API         Android min-API level                [default: 24]
#   JOBS                Parallel make jobs                   [default: nproc]
# =============================================================================
set -euo pipefail

# ── Helpers ────────────────────────────────────────────────────────────────────
log() { printf '\033[0;36m[..]\033[0m %s\n' "$*" >&2; }
ok()  { printf '\033[0;32m[ok]\033[0m %s\n' "$*" >&2; }
die() { printf '\033[0;31m[!!]\033[0m %s\n' "$*" >&2; exit 1; }
chk() { command -v "$1" &>/dev/null || die "'$1' not found — $2"; }

# ── Resolve script and repo root ───────────────────────────────────────────────
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

# ── Configuration from env ─────────────────────────────────────────────────────
ESPEAK_TAG="${ESPEAK_TAG:-1.52.0}"
ESPEAK_REPO_URL="https://github.com/espeak-ng/espeak-ng.git"
BUILD_TMP="${BUILD_TMP:-/tmp/espeak-static-build}"
ANDROID_API="${ANDROID_API:-24}"
JOBS="${JOBS:-$(nproc 2>/dev/null || sysctl -n hw.logicalcpu 2>/dev/null || echo 4)}"

# Output directory — required.
OUT_DIR="${ESPEAK_LIB_DIR:-}"
[ -n "${OUT_DIR}" ] || die \
    "ESPEAK_LIB_DIR is not set.\n\
Set it to the directory where libespeak-ng.a should be placed, e.g.:\n\
  ESPEAK_LIB_DIR=\$PWD/espeak-static/lib bash scripts/build-espeak-static.sh"

# Target triple.  Defaults to the host triple.
TARGET="${ESPEAK_TARGET:-}"
if [ -z "${TARGET}" ]; then
    # Derive host triple from rustc if available, else uname.
    if command -v rustc &>/dev/null; then
        TARGET="$(rustc -vV 2>/dev/null | awk '/^host:/{print $2}')"
    fi
    TARGET="${TARGET:-$(uname -m)-unknown-$(uname -s | tr '[:upper:]' '[:lower:]')-gnu}"
fi

TARGET_OS="${ESPEAK_TARGET_OS:-}"
TARGET_ARCH="${ESPEAK_TARGET_ARCH:-}"

# Derive TARGET_OS and TARGET_ARCH from the triple if not set by build.rs.
if [ -z "${TARGET_OS}" ]; then
    case "${TARGET}" in
        *-apple-darwin*)      TARGET_OS="macos"   ;;
        *-apple-ios*)         TARGET_OS="ios"     ;;
        *-linux-android*)     TARGET_OS="android" ;;
        *-linux-gnu*|*-linux-musl*) TARGET_OS="linux" ;;
        *-windows-*)          TARGET_OS="windows" ;;
        *)                    TARGET_OS="linux"   ;;
    esac
fi
if [ -z "${TARGET_ARCH}" ]; then
    TARGET_ARCH="${TARGET%%-*}"
fi

HOST_TRIPLE="$(rustc -vV 2>/dev/null | awk '/^host:/{print $2}')" \
    || HOST_TRIPLE="$(uname -m)-unknown-$(uname -s | tr '[:upper:]' '[:lower:]')-gnu"

IS_CROSS=false
[ "${HOST_TRIPLE}" = "${TARGET}" ] || IS_CROSS=true

ESPEAK_SRC="${BUILD_TMP}/espeak-ng-src"
BUILD_DIR="${BUILD_TMP}/espeak-build-${TARGET}"
INSTALL_DIR="${BUILD_TMP}/espeak-install-${TARGET}"

log "espeak-ng static build"
log "  tag      : ${ESPEAK_TAG}"
log "  target   : ${TARGET}"
log "  host     : ${HOST_TRIPLE}"
log "  cross    : ${IS_CROSS}"
log "  out dir  : ${OUT_DIR}"
log "  build tmp: ${BUILD_TMP}"
mkdir -p "${BUILD_TMP}" "${OUT_DIR}"

# ── Pre-flight ─────────────────────────────────────────────────────────────────
chk cmake "install cmake (brew install cmake / apt install cmake / dnf install cmake)"
chk git   "install git"

# ── Clone / update espeak-ng source ───────────────────────────────────────────
ESPEAK_STAMP="${BUILD_TMP}/espeak-cloned-${ESPEAK_TAG}.stamp"
if [ ! -f "${ESPEAK_STAMP}" ]; then
    if [ -d "${ESPEAK_SRC}/.git" ]; then
        log "Updating espeak-ng clone to ${ESPEAK_TAG}..."
        git -C "${ESPEAK_SRC}" fetch --depth 1 origin "refs/tags/${ESPEAK_TAG}"
        git -C "${ESPEAK_SRC}" checkout FETCH_HEAD
    else
        log "Cloning espeak-ng ${ESPEAK_TAG}..."
        rm -rf "${ESPEAK_SRC}"
        git clone --depth 1 --branch "${ESPEAK_TAG}" \
            "${ESPEAK_REPO_URL}" "${ESPEAK_SRC}"
    fi
    touch "${ESPEAK_STAMP}"
    ok "espeak-ng source ready"
else
    ok "espeak-ng ${ESPEAK_TAG} already cloned"
fi

# ── Determine CMake toolchain parameters ──────────────────────────────────────
# Filled in by the target-specific sections below.
CMAKE_EXTRA_ARGS=()
TOOLCHAIN_FILE=""      # path to a generated toolchain file, if needed
PATCH_FOR_CROSS=false  # whether to strip the espeak-ng-bin executable target

determine_toolchain() {
    local arch="${TARGET_ARCH}"
    local os="${TARGET_OS}"
    local env_part  # musl / gnu / gnueabihf / msvc / …
    env_part="$(echo "${TARGET}" | cut -d- -f4 2>/dev/null || echo "")"
    [ -n "${env_part}" ] || env_part="$(echo "${TARGET}" | cut -d- -f3 2>/dev/null || echo "gnu")"

    case "${os}" in
    # ── macOS ────────────────────────────────────────────────────────────────
    macos)
        local macos_arch
        case "${arch}" in
            aarch64) macos_arch="arm64"  ;;
            x86_64)  macos_arch="x86_64" ;;
            *)       macos_arch="${arch}" ;;
        esac
        CMAKE_EXTRA_ARGS+=(
            "-DCMAKE_OSX_ARCHITECTURES=${macos_arch}"
            "-DCMAKE_OSX_DEPLOYMENT_TARGET=13.4"
        )
        ;;

    # ── Android ──────────────────────────────────────────────────────────────
    android)
        local ndk=""
        for v in ANDROID_NDK_HOME ANDROID_NDK_ROOT NDK_HOME; do
            ndk="${!v:-}" && [ -d "${ndk}" ] && break || ndk=""
        done
        # Auto-detect from Android Studio default location.
        if [ -z "${ndk}" ]; then
            for candidate in \
                "${HOME}/Library/Android/sdk/ndk" \
                "${HOME}/Android/Sdk/ndk" \
                "/opt/android-sdk/ndk" \
                "/usr/lib/android-sdk/ndk"; do
                if [ -d "${candidate}" ]; then
                    ndk="${candidate}/$(ls -1 "${candidate}" | sort -V | tail -1)"
                    break
                fi
            done
        fi
        [ -n "${ndk}" ] || die \
            "Android NDK not found.\n\
Set ANDROID_NDK_HOME to your NDK root (r25+ required), e.g.:\n\
  ANDROID_NDK_HOME=~/Library/Android/sdk/ndk/26.1.10909125"

        log "NDK: ${ndk}"

        local host_tag
        case "$(uname -s)" in
            Darwin) host_tag="darwin-x86_64"
                    [ -d "${ndk}/toolchains/llvm/prebuilt/darwin-arm64" ] \
                        && host_tag="darwin-arm64" ;;
            *) host_tag="linux-x86_64" ;;
        esac
        local ndk_tc="${ndk}/toolchains/llvm/prebuilt/${host_tag}"
        local clang_cc="${ndk_tc}/bin/${arch}-linux-android${ANDROID_API}-clang"

        # Normalise arch to the NDK naming convention.
        local ndk_abi
        case "${arch}" in
            aarch64) ndk_abi="arm64-v8a"   ;;
            arm)     ndk_abi="armeabi-v7a" ;;
            x86_64)  ndk_abi="x86_64"      ;;
            x86|i686)ndk_abi="x86"         ;;
            riscv64) ndk_abi="riscv64"     ;;
            *)       ndk_abi="${arch}"      ;;
        esac

        [ -x "${clang_cc}" ] || die \
            "NDK clang not found: ${clang_cc}\n  Check NDK version (r25+) and ANDROID_API (${ANDROID_API})."

        CMAKE_EXTRA_ARGS+=(
            "-DCMAKE_TOOLCHAIN_FILE=${ndk}/build/cmake/android.toolchain.cmake"
            "-DANDROID_ABI=${ndk_abi}"
            "-DANDROID_PLATFORM=android-${ANDROID_API}"
            "-DANDROID_STL=c++_static"
            "-DCMAKE_TRY_COMPILE_TARGET_TYPE=STATIC_LIBRARY"
        )
        PATCH_FOR_CROSS=true
        ;;

    # ── Linux GNU ─────────────────────────────────────────────────────────────
    linux)
        if "${IS_CROSS}"; then
            # Derive the GNU cross-compiler prefix.
            local cc_prefix
            case "${arch}-${env_part}" in
                aarch64-*gnu*)     cc_prefix="aarch64-linux-gnu"        ;;
                arm-gnueabihf)     cc_prefix="arm-linux-gnueabihf"      ;;
                arm-gnueabi)       cc_prefix="arm-linux-gnueabi"        ;;
                armv7-gnueabihf)   cc_prefix="arm-linux-gnueabihf"      ;;
                i686-*gnu*)        cc_prefix="i686-linux-gnu"           ;;
                riscv64-*)         cc_prefix="riscv64-linux-gnu"        ;;
                s390x-*)           cc_prefix="s390x-linux-gnu"          ;;
                powerpc64le-*)     cc_prefix="powerpc64le-linux-gnu"    ;;
                loongarch64-*)     cc_prefix="loongarch64-linux-gnu"    ;;
                x86_64-musl)       cc_prefix="x86_64-linux-musl"        ;;
                aarch64-musl)      cc_prefix="aarch64-linux-musl"       ;;
                arm-musl*)         cc_prefix="arm-linux-musleabihf"     ;;
                i686-musl)         cc_prefix="i686-linux-musl"          ;;
                *)                 cc_prefix="" ;;
            esac

            if [ -n "${cc_prefix}" ]; then
                local cc="${cc_prefix}-gcc"
                local cxx="${cc_prefix}-g++"

                # For musl, also try *-cc naming (musl-cross-make).
                if [[ "${env_part}" == *musl* ]] && ! command -v "${cc}" &>/dev/null; then
                    cc="${cc_prefix}-cc"
                    cxx="${cc_prefix}-c++"
                fi

                command -v "${cc}" &>/dev/null || die \
                    "Cross-compiler '${cc}' not found.\n\
Install it for your host platform:\n\
  Ubuntu/Debian:  sudo apt install gcc-${cc_prefix}\n\
  Fedora:         sudo dnf install gcc-${cc_prefix}\n\
  Arch:           sudo pacman -S ${cc_prefix}-gcc\n\
  macOS:          brew tap messense/macos-cross-toolchains && brew install ${cc_prefix}"

                # Generate a minimal CMake toolchain file.
                local processor
                case "${arch}" in
                    aarch64)    processor="aarch64" ;;
                    arm|armv7)  processor="armv7"   ;;
                    riscv64)    processor="riscv64" ;;
                    x86_64)     processor="x86_64"  ;;
                    i686)       processor="i686"    ;;
                    *)          processor="${arch}" ;;
                esac

                TOOLCHAIN_FILE="${BUILD_TMP}/toolchain-${TARGET}.cmake"
                cat > "${TOOLCHAIN_FILE}" << EOF
set(CMAKE_SYSTEM_NAME Linux)
set(CMAKE_SYSTEM_PROCESSOR ${processor})
set(CMAKE_C_COMPILER   ${cc})
set(CMAKE_CXX_COMPILER ${cxx})
set(CMAKE_TRY_COMPILE_TARGET_TYPE STATIC_LIBRARY)
EOF
                if [ -n "${ESPEAK_SYSROOT:-}" ]; then
                    cat >> "${TOOLCHAIN_FILE}" << EOF
set(CMAKE_SYSROOT "${ESPEAK_SYSROOT}")
set(CMAKE_FIND_ROOT_PATH "${ESPEAK_SYSROOT}")
set(CMAKE_FIND_ROOT_PATH_MODE_PROGRAM NEVER)
set(CMAKE_FIND_ROOT_PATH_MODE_LIBRARY ONLY)
set(CMAKE_FIND_ROOT_PATH_MODE_INCLUDE ONLY)
EOF
                fi

                CMAKE_EXTRA_ARGS+=("-DCMAKE_TOOLCHAIN_FILE=${TOOLCHAIN_FILE}")
                PATCH_FOR_CROSS=true
                log "Cross-compiler: ${cc}"
            else
                log "WARNING: unknown cross target '${TARGET}', attempting native-ish build"
            fi
        fi
        ;;

    # ── Other / fall-through ──────────────────────────────────────────────────
    *)
        log "WARNING: unrecognised target OS '${os}' — attempting native build"
        ;;
    esac
}

determine_toolchain

# ── Patch CMakeLists.txt for cross targets ─────────────────────────────────────
# When cross-compiling, the espeak-ng-bin executable cannot be built or run on
# the host.  We remove all CMake commands that declare or configure that target.
# (The same technique used in build_rust_android.sh.)
patch_espeak_for_cross() {
    log "Patching espeak-ng source for cross-compilation..."
    python3 - "${ESPEAK_SRC}" << 'PYEOF'
import re, os, sys

root = sys.argv[1]

BINARY_CMDS = [
    'add_executable', 'target_link_libraries', 'target_compile_definitions',
    'target_compile_options', 'target_include_directories', 'target_sources',
    'set_target_properties', 'add_dependencies',
]

def patch(path):
    text = open(path).read()
    orig = text

    for cmd in BINARY_CMDS:
        text = re.sub(
            r'\b' + re.escape(cmd) + r'\s*\(\s*espeak-ng-bin\b[^)]*\)',
            f'# [cross patch] {cmd}(espeak-ng-bin) removed',
            text, flags=re.DOTALL)

    text = re.sub(
        r'install\s*\(\s*TARGETS\s+espeak-ng-bin\b[^)]*\)',
        '# [cross patch] install(TARGETS espeak-ng-bin) removed',
        text, flags=re.DOTALL)

    text = re.sub(
        r'add_custom_command\s*\([^)]*espeak-ng-bin[^)]*\)',
        '# [cross patch] add_custom_command(espeak-ng-bin) removed',
        text, flags=re.DOTALL)

    text = re.sub(
        r'add_subdirectory\s*\(\s*tests\s*\)',
        '# [cross patch] tests removed',
        text)

    # Prevent host speech-player from leaking into the cross build.
    text = re.sub(
        r'find_library\s*\(\s*SPEECHPLAYER_LIB\b[^)]*\)',
        'set(SPEECHPLAYER_LIB SPEECHPLAYER_LIB-NOTFOUND CACHE PATH "" FORCE)',
        text, flags=re.DOTALL)
    text = re.sub(
        r'find_path\s*\(\s*SPEECHPLAYER_INC\b[^)]*\)',
        'set(SPEECHPLAYER_INC SPEECHPLAYER_INC-NOTFOUND CACHE PATH "" FORCE)',
        text, flags=re.DOTALL)

    if text != orig:
        open(path, 'w').write(text)
        print("  Patched:", path)

for dirpath, _, files in os.walk(root):
    for f in files:
        if f == 'CMakeLists.txt' or f.endswith('.cmake'):
            patch(os.path.join(dirpath, f))
PYEOF
    ok "espeak-ng patched for cross-compilation"
}

# Work on a target-specific copy of the source so cross patches don't break
# a concurrent native build of the same checkout.
ESPEAK_BUILD_SRC="${ESPEAK_SRC}"
if "${PATCH_FOR_CROSS}"; then
    ESPEAK_BUILD_SRC="${BUILD_TMP}/espeak-ng-src-${TARGET}"
    if [ ! -d "${ESPEAK_BUILD_SRC}" ]; then
        log "Copying source tree for cross-patching (${TARGET})..."
        cp -a "${ESPEAK_SRC}" "${ESPEAK_BUILD_SRC}"
    fi
    patch_espeak_for_cross
fi

# ── CMake configure ────────────────────────────────────────────────────────────
BUILD_STAMP="${BUILD_TMP}/espeak-build-${TARGET}.stamp"

if [ ! -f "${BUILD_STAMP}" ]; then
    log "Configuring espeak-ng for ${TARGET}..."
    rm -rf "${BUILD_DIR}" "${INSTALL_DIR}"
    mkdir -p "${BUILD_DIR}" "${INSTALL_DIR}"

    CMAKE_LOG="${BUILD_DIR}/cmake.log"
    if ! PKG_CONFIG_PATH="" cmake -S "${ESPEAK_BUILD_SRC}" -B "${BUILD_DIR}" \
        -DCMAKE_BUILD_TYPE=Release \
        -DCMAKE_INSTALL_PREFIX="${INSTALL_DIR}" \
        -DBUILD_SHARED_LIBS=OFF \
        -DUSE_ASYNC=OFF \
        -DWITH_ASYNC=OFF \
        -DWITH_PCAUDIOLIB=OFF \
        -DWITH_SPEECHPLAYER=OFF \
        -DWITH_SONIC=OFF \
        -DUSE_KLATT=OFF \
        -DCMAKE_DISABLE_FIND_PACKAGE_SpeechPlayer=TRUE \
        -DCMAKE_DISABLE_FIND_PACKAGE_PcAudio=TRUE \
        "${CMAKE_EXTRA_ARGS[@]}" \
        -Wno-dev \
        > "${CMAKE_LOG}" 2>&1
    then
        echo ""
        echo "=== CMake configure FAILED ==="
        grep -E "CMake Error|error:" "${CMAKE_LOG}" | head -30 || tail -30 "${CMAKE_LOG}"
        echo "(full log: ${CMAKE_LOG})"
        die "CMake configure failed for ${TARGET}"
    fi
    grep -E "^-- " "${CMAKE_LOG}" | tail -6 >&2 || true
    ok "CMake configure done"

    # ── CMake build ───────────────────────────────────────────────────────────
    log "Building espeak-ng (${JOBS} jobs)..."
    BUILD_LOG="${BUILD_DIR}/build.log"

    # Build only the library target when cross-compiling to avoid trying to
    # compile the binary (which references host-incompatible headers).
    local_target_arg=()
    "${PATCH_FOR_CROSS}" && local_target_arg=("--target" "espeak-ng")

    if ! cmake --build "${BUILD_DIR}" \
            "${local_target_arg[@]}" \
            -- -j"${JOBS}" \
        > "${BUILD_LOG}" 2>&1
    then
        echo ""
        echo "=== Build FAILED ==="
        grep -E "error:" "${BUILD_LOG}" | head -20 || tail -40 "${BUILD_LOG}"
        echo "(full log: ${BUILD_LOG})"
        die "Build failed for ${TARGET}"
    fi

    cmake --install "${BUILD_DIR}" >> "${BUILD_LOG}" 2>&1 || true
    ok "Build complete"
    touch "${BUILD_STAMP}"
else
    ok "espeak-ng already built for ${TARGET} (delete ${BUILD_STAMP} to rebuild)"
fi

# ── Locate libespeak-ng.a and all companion static libs ───────────────────────
# espeak-ng's CMake build can produce libucd.a, libsonic.a etc. as separate
# targets.  We merge them all into a single self-contained libespeak-ng.a so
# callers only need to link one archive.

mapfile -t RAW_LIBS < <(
    find "${BUILD_DIR}" "${INSTALL_DIR}" \
         -name "*.a" 2>/dev/null | sort -u
)

[ "${#RAW_LIBS[@]}" -gt 0 ] || die "No .a files found after build — see ${BUILD_DIR}"

log "Found ${#RAW_LIBS[@]} static archive(s):"
for l in "${RAW_LIBS[@]}"; do log "  ${l}"; done

# ── Merge into a single libespeak-ng.a ────────────────────────────────────────
MERGED="${BUILD_TMP}/libespeak-ng-merged-${TARGET}.a"

merge_archives() {
    # libtool (macOS / GNU) path
    if command -v libtool &>/dev/null \
       && libtool --version 2>/dev/null | grep -q GNU; then
        libtool --mode=link ar crs "${MERGED}" "${RAW_LIBS[@]}"
        return
    fi
    # macOS libtool (BSD)
    if command -v libtool &>/dev/null \
       && uname -s | grep -qi darwin; then
        libtool -static -o "${MERGED}" "${RAW_LIBS[@]}"
        return
    fi
    # GNU ar MRI script — portable across host platforms.
    local mri_script="${BUILD_TMP}/merge-${TARGET}.mri"
    {
        echo "CREATE ${MERGED}"
        for lib in "${RAW_LIBS[@]}"; do echo "ADDLIB ${lib}"; done
        echo "SAVE"
        echo "END"
    } > "${mri_script}"

    # Use the cross-ar if it's in the toolchain cmake file.
    local ar_bin="ar"
    if [ -n "${TOOLCHAIN_FILE}" ]; then
        local cross_ar
        cross_ar="$(grep -E 'CMAKE_AR|CMAKE_C_COMPILER' "${TOOLCHAIN_FILE}" \
                    | grep 'CMAKE_C_COMPILER' | awk -F'"' '{print $2}')" || true
        if [ -n "${cross_ar}" ]; then
            local prefix="${cross_ar%-gcc}"
            command -v "${prefix}-ar" &>/dev/null && ar_bin="${prefix}-ar"
        fi
    fi

    "${ar_bin}" -M < "${mri_script}"
}

merge_archives
ok "Merged -> ${MERGED}  ($(du -sh "${MERGED}" | cut -f1))"

# ── Install to OUT_DIR ─────────────────────────────────────────────────────────
install -d "${OUT_DIR}"
install -m 644 "${MERGED}" "${OUT_DIR}/libespeak-ng.a"

ok "Installed:"
ok "  ${OUT_DIR}/libespeak-ng.a  ($(du -sh "${OUT_DIR}/libespeak-ng.a" | cut -f1))"
echo "" >&2
