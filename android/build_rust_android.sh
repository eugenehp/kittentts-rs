#!/usr/bin/env bash
# -----------------------------------------------------------------------------
# build_rust_android.sh — Build kittentts-rs for Android (arm64-v8a)
#
# What this script does:
#   1. Clones + cross-compiles espeak-ng as a shared library using the NDK.
#   2. Compiles espeak-ng phoneme data with a native host build.
#   3. Cross-compiles kittentts-rs (Rust staticlib) for aarch64-linux-android.
#   4. Compiles kittentts_jni.c into libkittentts_jni.so (links the above).
#   5. Copies the three .so files into KittenTTSApp/app/src/main/jniLibs/.
#   6. Zips espeak-ng-data into KittenTTSApp/app/src/main/assets/.
#   7. Downloads model files from HuggingFace into
#      KittenTTSApp/app/src/main/assets/models/.
#
# Prerequisites:
#   - Android NDK r25+ (set ANDROID_NDK_HOME or ANDROID_HOME/ndk/<ver>)
#   - Rust with target aarch64-linux-android
#       rustup target add aarch64-linux-android
#   - cmake, git, curl, zip, python3
#
# Quick start:
#   export ANDROID_NDK_HOME=/path/to/ndk   # or set ANDROID_HOME
#   bash android/build_rust_android.sh
# -----------------------------------------------------------------------------
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
BUILD_TMP="/tmp/kittentts-android-build"
mkdir -p "${BUILD_TMP}"

ANDROID_API=24          # Minimum API level (Android 7.0) — first with full arm64 support
ANDROID_ABI=arm64-v8a
RUST_TRIPLE=aarch64-linux-android

log() { printf '[..] %s\n' "$*"; }
ok()  { printf '[ok] %s\n' "$*"; }
die() { printf '[!!] %s\n' "$*" >&2; exit 1; }
chk() { command -v "$1" &>/dev/null || die "'$1' not found — $2"; }

# -----------------------------------------------------------------------------
# 0. Pre-flight checks
# -----------------------------------------------------------------------------
log "Checking prerequisites..."
chk cmake   "install cmake (brew install cmake / apt install cmake)"
chk cargo   "install Rust: https://rustup.rs"
chk rustup  "install Rust: https://rustup.rs"
chk git     "install git"
chk curl    "install curl"
chk zip     "install zip"
chk python3 "install python3"

# ── Locate a JDK for Gradle ───────────────────────────────────────────────────
# Gradle needs a JVM.  We try (in order):
#   1. JAVA_HOME already set in the environment
#   2. Android Studio's bundled JBR  (no separate install needed)
#   3. /usr/libexec/java_home        (macOS standard helper)
#   4. Common Homebrew / system paths
setup_java() {
    # Already valid
    if [ -n "${JAVA_HOME:-}" ] && [ -x "${JAVA_HOME}/bin/java" ]; then
        ok "JAVA_HOME: ${JAVA_HOME}"
        return 0
    fi

    # Android Studio bundled JDK (macOS)
    local as_jbr="/Applications/Android Studio.app/Contents/jbr/Contents/Home"
    if [ -x "${as_jbr}/bin/java" ]; then
        export JAVA_HOME="${as_jbr}"
        ok "JAVA_HOME: ${JAVA_HOME}  (Android Studio JBR)"
        return 0
    fi

    # macOS java_home helper
    if command -v /usr/libexec/java_home &>/dev/null; then
        local jh
        jh="$(/usr/libexec/java_home 2>/dev/null)" && [ -n "${jh}" ] && {
            export JAVA_HOME="${jh}"
            ok "JAVA_HOME: ${JAVA_HOME}"
            return 0
        }
    fi

    # Common install paths (Homebrew temurin, sdkman, etc.)
    for candidate in \
        "/opt/homebrew/opt/openjdk/libexec/openjdk.jdk/Contents/Home" \
        "/usr/local/opt/openjdk/libexec/openjdk.jdk/Contents/Home" \
        "/opt/homebrew/opt/temurin@17/libexec/openjdk.jdk/Contents/Home" \
        "/usr/lib/jvm/java-17-openjdk-amd64" \
        "/usr/lib/jvm/java-17-openjdk"; do
        if [ -x "${candidate}/bin/java" ]; then
            export JAVA_HOME="${candidate}"
            ok "JAVA_HOME: ${JAVA_HOME}"
            return 0
        fi
    done

    echo ""
    echo "  [!!] No JDK found.  Gradle requires a JDK to run."
    echo "       Quickest fix — use Android Studio's bundled JDK:"
    echo ""
    echo "         export JAVA_HOME=\"/Applications/Android Studio.app/Contents/jbr/Contents/Home\""
    echo ""
    echo "       Or install a standalone JDK:"
    echo "         brew install --cask temurin@17   # macOS"
    echo "         sudo apt install openjdk-17-jdk  # Linux"
    echo ""
    return 1
}

setup_java || true   # non-fatal here; gradle wrapper step will warn again if needed

# ── Locate NDK ────────────────────────────────────────────────────────────────
find_ndk() {
    # 1. Explicit env var — highest priority
    if [ -n "${ANDROID_NDK_HOME:-}" ] && [ -d "${ANDROID_NDK_HOME}" ]; then
        echo "${ANDROID_NDK_HOME}"
        return 0
    fi

    # 2. ANDROID_HOME/ndk/<highest version> — set by some CI environments
    if [ -n "${ANDROID_HOME:-}" ] && [ -d "${ANDROID_HOME}/ndk" ]; then
        local ndk
        ndk="$(ls -1 "${ANDROID_HOME}/ndk" | sort -V | tail -1)"
        if [ -n "${ndk}" ]; then
            echo "${ANDROID_HOME}/ndk/${ndk}"
            return 0
        fi
    fi

    # 3. Well-known default SDK locations (Android Studio installs here by default)
    #
    #   macOS:  ~/Library/Android/sdk          (Android Studio default)
    #   Linux:  ~/Android/Sdk                  (Android Studio default)
    #   Linux:  /opt/android-sdk               (system-wide installs)
    #   Linux:  /usr/lib/android-sdk           (Debian/Ubuntu apt package)
    for candidate in \
        "${HOME}/Library/Android/sdk/ndk" \
        "${HOME}/Android/Sdk/ndk" \
        "/opt/android-sdk/ndk" \
        "/usr/lib/android-sdk/ndk" \
        "/usr/local/lib/android/sdk/ndk"; do
        if [ -d "${candidate}" ]; then
            local ndk
            ndk="$(ls -1 "${candidate}" | sort -V | tail -1)"
            if [ -n "${ndk}" ]; then
                echo "${candidate}/${ndk}"
                return 0
            fi
        fi
    done
    return 1
}

NDK="$(find_ndk)" || die \
    "Android NDK not found.
  Checked:
    \$ANDROID_NDK_HOME, \$ANDROID_HOME/ndk/
    ~/Library/Android/sdk/ndk/   (macOS — Android Studio default)
    ~/Android/Sdk/ndk/           (Linux  — Android Studio default)
    /opt/android-sdk/ndk/        (Linux system-wide)
  Install via Android Studio → SDK Manager → SDK Tools → NDK (Side by side)
  Then set: export ANDROID_NDK_HOME=~/Library/Android/sdk/ndk/<version>
  NDK r25+ required."
log "NDK: ${NDK}"

# Detect host OS tag used by the NDK's prebuilt toolchain directory.
# NDK r24+ on Apple Silicon ships darwin-arm64; older NDKs and Intel Macs
# use darwin-x86_64 (which also runs via Rosetta 2 on Apple Silicon).
# We probe both and use whichever actually exists.
case "$(uname -s)" in
    Darwin)
        if [ -d "${NDK}/toolchains/llvm/prebuilt/darwin-arm64" ]; then
            HOST_TAG="darwin-arm64"
        else
            HOST_TAG="darwin-x86_64"
        fi
        ;;
    Linux)  HOST_TAG="linux-x86_64";;
    *)      die "Unsupported host OS: $(uname -s)";;
esac
log "Host toolchain tag: ${HOST_TAG}"

NDK_BIN="${NDK}/toolchains/llvm/prebuilt/${HOST_TAG}/bin"
[ -d "${NDK_BIN}" ] || die "NDK bin dir not found: ${NDK_BIN}
  Tried: ${NDK}/toolchains/llvm/prebuilt/${HOST_TAG}/
  NDK r25+ required."

CLANG="${NDK_BIN}/aarch64-linux-android${ANDROID_API}-clang"
[ -x "${CLANG}" ] || die "NDK clang not found: ${CLANG}\n  Ensure NDK r25+ is installed."

log "Adding Rust target ${RUST_TRIPLE}..."
rustup target add "${RUST_TRIPLE}" 2>/dev/null || true
ok "Rust target ready"

# Destination directories inside the Android project
JNILIBS_DIR="${SCRIPT_DIR}/KittenTTSApp/app/src/main/jniLibs/${ANDROID_ABI}"
ASSETS_DIR="${SCRIPT_DIR}/KittenTTSApp/app/src/main/assets"
MODELS_DIR="${ASSETS_DIR}/models"
mkdir -p "${JNILIBS_DIR}" "${ASSETS_DIR}" "${MODELS_DIR}"

# -----------------------------------------------------------------------------
# 1. Build espeak-ng for Android (shared library)
#
# We build as a shared library (.so) because build.rs emits
# rustc-link-lib=dylib=espeak-ng for non-iOS targets.
# libespeak-ng.so goes into jniLibs/ alongside the other .so files.
# -----------------------------------------------------------------------------
ESPEAK_REPO="https://github.com/espeak-ng/espeak-ng.git"
ESPEAK_TAG="1.52.0"
# ESPEAK_SRC  — the clean, unpatched clone used by the native host build.
#               Never modified after cloning so the native build can always
#               build espeak-ng-bin and run the add_custom_command blocks
#               that generate phoneme data (phontab, phondata, …).
ESPEAK_SRC="${BUILD_TMP}/espeak-ng-src"
# ESPEAK_ANDROID_SRC — a separate copy of the source tree used only for the
#               Android cross-build.  Android-specific patches are applied
#               here so they never touch ESPEAK_SRC.
ESPEAK_ANDROID_SRC="${BUILD_TMP}/espeak-ng-src-android"
ESPEAK_ANDROID_BDIR="${BUILD_TMP}/espeak-build-android"
ESPEAK_ANDROID_INSTALL="${BUILD_TMP}/espeak-install-android"
ESPEAK_STAMP="${BUILD_TMP}/espeak_android_ok.stamp"

build_espeak_android() {
    # This function is called as found_so="$(build_espeak_android)" so the
    # shell captures its stdout.  The ONLY thing that should go to stdout is
    # the final path echo at the bottom; everything else must go to stderr so
    # it appears in the terminal rather than being swallowed by $().
    #
    # Save the captured stdout as fd 3, then redirect stdout → stderr for
    # the entire body of this function.
    exec 3>&1 1>&2

    if [ ! -d "${ESPEAK_SRC}/.git" ]; then
        log "Cloning espeak-ng ${ESPEAK_TAG}..."
        git clone --depth 1 --branch "${ESPEAK_TAG}" "${ESPEAK_REPO}" "${ESPEAK_SRC}"
    fi

    # ── Copy source tree for Android-only patching ────────────────────────────
    #
    # ESPEAK_SRC is kept clean and unpatched so the native host build (section 2)
    # can build espeak-ng-bin and run the add_custom_command phoneme-data steps.
    # ESPEAK_ANDROID_SRC is a separate copy where Android patches are applied.
    if [ ! -d "${ESPEAK_ANDROID_SRC}" ]; then
        log "Copying espeak-ng source for Android patch..."
        cp -a "${ESPEAK_SRC}" "${ESPEAK_ANDROID_SRC}"
    fi

    # ── Patch espeak-ng CMakeLists.txt files for Android ─────────────────────
    #
    # espeak-ng's build defines an `espeak-ng-bin` executable that:
    #   • includes <wordexp.h>  — absent on Android
    #   • links audio libs      — absent on Android even with all WITH_* OFF
    #
    # We must remove every CMake command that declares or configures
    # espeak-ng-bin (add_executable, target_link_libraries, …).
    # Just removing the install() call — as the iOS patch does — is NOT
    # enough: cmake --build still compiles the binary and fails on
    # Android-incompatible headers.
    #
    # We also guard the speech-player find_library/find_path calls that
    # can leak macOS host paths into the Android cross-build.
    #
    # Patches are applied to ESPEAK_ANDROID_SRC, not ESPEAK_SRC, so the
    # native host build always has the full unmodified cmake files.
    log "Patching espeak-ng CMakeLists.txt for Android cross-compile..."
    python3 - "${ESPEAK_ANDROID_SRC}" << 'PYEOF'
import re, os, sys

src_root = sys.argv[1]

# Every CMake command that takes espeak-ng-bin as its FIRST argument
# (i.e. it is *defining* or *configuring* that target).
BINARY_TARGET_CMDS = [
    'add_executable',
    'target_link_libraries',
    'target_compile_definitions',
    'target_compile_options',
    'target_include_directories',
    'target_sources',
    'set_target_properties',
    'add_dependencies',
]

def patch_file(path):
    text = open(path).read()
    orig = text

    # 1. Remove every command that defines/configures the binary target.
    #    Pattern: <command>( espeak-ng-bin  ...closing-paren )
    #    [^)]* matches across newlines (newline is not ')').
    for cmd in BINARY_TARGET_CMDS:
        text = re.sub(
            r'\b' + re.escape(cmd) + r'\s*\(\s*espeak-ng-bin\b[^)]*\)',
            f'# [Android patch] {cmd}(espeak-ng-bin ...) removed',
            text, flags=re.DOTALL
        )

    # 2. install(TARGETS espeak-ng-bin ...) — TARGETS is the first keyword,
    #    espeak-ng-bin comes after it; handle separately.
    text = re.sub(
        r'install\s*\(\s*TARGETS\s+espeak-ng-bin\b[^)]*\)',
        '# [Android patch] install(TARGETS espeak-ng-bin ...) removed',
        text, flags=re.DOTALL
    )

    # 3. add_custom_command blocks that reference espeak-ng-bin anywhere inside
    #    their argument list (e.g. cmake/data.cmake uses the binary to compile
    #    phoneme dictionaries at build time via a generator expression:
    #        COMMAND $<TARGET_FILE:espeak-ng-bin> --compile-mbrola …
    #    The binary target no longer exists, so CMake's generate step fails
    #    with "No target espeak-ng-bin" for every such command.
    #    We compile phoneme data separately with a native host build anyway.
    text = re.sub(
        r'add_custom_command\s*\([^)]*espeak-ng-bin[^)]*\)',
        '# [Android patch] add_custom_command(... espeak-ng-bin ...) removed',
        text, flags=re.DOTALL
    )

    # 4. Exclude the tests/ subdirectory — every test references espeak-ng-bin
    #    via $<TARGET_FILE:espeak-ng-bin> in add_test() calls, which again
    #    makes the CMake generate step fail with "No target espeak-ng-bin".
    text = re.sub(
        r'add_subdirectory\s*\(\s*tests\s*\)',
        '# [Android patch] add_subdirectory(tests) removed',
        text
    )

    # 5. Disable speech-player discovery — macOS host paths leak into the
    #    Android cross-build via find_library / find_path.
    text = re.sub(
        r'find_library\s*\(\s*SPEECHPLAYER_LIB\b[^)]*\)',
        'set(SPEECHPLAYER_LIB SPEECHPLAYER_LIB-NOTFOUND CACHE PATH "" FORCE)',
        text, flags=re.DOTALL
    )
    text = re.sub(
        r'find_path\s*\(\s*SPEECHPLAYER_INC\b[^)]*\)',
        'set(SPEECHPLAYER_INC SPEECHPLAYER_INC-NOTFOUND CACHE PATH "" FORCE)',
        text, flags=re.DOTALL
    )

    if text != orig:
        open(path, 'w').write(text)
        print("  Patched:", path)

for root, dirs, files in os.walk(src_root):
    for fname in files:
        # Patch both CMakeLists.txt and *.cmake files (e.g. cmake/data.cmake
        # contains add_custom_command blocks that reference espeak-ng-bin).
        if fname == 'CMakeLists.txt' or fname.endswith('.cmake'):
            patch_file(os.path.join(root, fname))
PYEOF
    ok "espeak-ng patched"

    log "Configuring espeak-ng for Android ${ANDROID_ABI} (API ${ANDROID_API})..."
    rm -rf "${ESPEAK_ANDROID_BDIR}"
    mkdir -p "${ESPEAK_ANDROID_BDIR}" "${ESPEAK_ANDROID_INSTALL}"

    local cmake_log="${ESPEAK_ANDROID_BDIR}/cmake.log"
    local toolchain_file="${NDK}/build/cmake/android.toolchain.cmake"

    log "cmake version: $(cmake --version | head -1)"
    log "NDK toolchain: ${toolchain_file}"
    [ -f "${toolchain_file}" ] \
        || die "NDK toolchain file not found: ${toolchain_file}\n  NDK r25+ required."

    # Key flags for Android cross-compilation:
    #
    # CMAKE_TRY_COMPILE_TARGET_TYPE=STATIC_LIBRARY
    #   Prevents CMake's feature-detection macros (check_c_source_compiles,
    #   check_function_exists, …) from trying to link and run an executable.
    #   Running a cross-compiled executable on the build host is impossible,
    #   which would otherwise cause every configure check to fail silently or
    #   abort cmake entirely.  The NDK toolchain file *should* set this, but
    #   espeak-ng's CMakeLists.txt resets CMAKE_TRY_COMPILE_TARGET_TYPE in
    #   some versions, so we force it here as well.
    #
    # ANDROID_STL=c++_static
    #   Embeds the C++ runtime inside libespeak-ng.so so we do not need to
    #   ship libc++_shared.so as a separate espeak-ng dependency.
    #
    # BUILD_SHARED_LIBS=ON
    #   Produces libespeak-ng.so.  build.rs emits rustc-link-lib=dylib=espeak-ng
    #   for Android targets, so the Rust staticlib expects a shared library.
    if ! cmake -S "${ESPEAK_ANDROID_SRC}" -B "${ESPEAK_ANDROID_BDIR}" \
        -DCMAKE_TOOLCHAIN_FILE="${toolchain_file}" \
        -DANDROID_ABI="${ANDROID_ABI}" \
        -DANDROID_PLATFORM="android-${ANDROID_API}" \
        -DANDROID_STL=c++_static \
        -DCMAKE_TRY_COMPILE_TARGET_TYPE=STATIC_LIBRARY \
        -DCMAKE_BUILD_TYPE=Release \
        -DCMAKE_INSTALL_PREFIX="${ESPEAK_ANDROID_INSTALL}" \
        -DBUILD_SHARED_LIBS=ON \
        -DUSE_ASYNC=OFF \
        -DWITH_ASYNC=OFF \
        -DWITH_PCAUDIOLIB=OFF \
        -DWITH_SPEECHPLAYER=OFF \
        -DUSE_SONIC=OFF \
        -DWITH_SONIC=OFF \
        -DUSE_KLATT=OFF \
        -DSPEECHPLAYER_FOUND=FALSE \
        -DCMAKE_DISABLE_FIND_PACKAGE_SpeechPlayer=TRUE \
        -DCMAKE_DISABLE_FIND_PACKAGE_PcAudio=TRUE \
        -Wno-dev \
        > "${cmake_log}" 2>&1
    then
        echo ""
        echo "=== espeak-ng CMake configure FAILED ==="
        echo "--- errors / warnings ---"
        grep -E "CMake Error|CMake Warning|error:|warning:" "${cmake_log}" | head -40 || true
        echo "--- last 30 lines of full log ---"
        tail -30 "${cmake_log}"
        echo "=== full log: ${cmake_log} ==="
        die "espeak-ng CMake configure failed for Android"
    fi
    grep -E "^-- " "${cmake_log}" | tail -8 || true

    log "Building espeak-ng library for Android (target: espeak-ng)..."
    local build_log="${ESPEAK_ANDROID_BDIR}/build.log"
    local ncpu; ncpu="$(nproc 2>/dev/null || sysctl -n hw.logicalcpu 2>/dev/null || echo 4)"

    # Build only the library target — the executable (espeak-ng-bin) is
    # already removed from CMakeLists.txt by the patch above, but we also
    # restrict cmake --build to the library target as a safety net.
    if ! cmake --build "${ESPEAK_ANDROID_BDIR}" \
            --target espeak-ng \
            -- -j"${ncpu}" \
        > "${build_log}" 2>&1
    then
        echo "--- last 60 lines of build log ---"
        tail -60 "${build_log}"
        echo "--- full log: ${build_log} ---"
        die "espeak-ng build failed for Android"
    fi

    cmake --install "${ESPEAK_ANDROID_BDIR}" >> "${build_log}" 2>&1 || true

    # Find the .so — it may be in the build dir or the install prefix
    local so_path
    so_path="$(find "${ESPEAK_ANDROID_BDIR}" "${ESPEAK_ANDROID_INSTALL}" \
                    -name "libespeak-ng.so" 2>/dev/null | head -1)"
    [ -n "${so_path}" ] || die "libespeak-ng.so not found after Android build"

    ok "espeak-ng built: ${so_path}"
    # Write the path to fd 3 (the original stdout captured by the caller's $()).
    echo "${so_path}" >&3
}

# Build or reuse
ESPEAK_SO_FILE="${BUILD_TMP}/libespeak-ng.so"

if [ ! -f "${ESPEAK_STAMP}" ]; then
    found_so="$(build_espeak_android)"
    cp "${found_so}" "${ESPEAK_SO_FILE}"
    touch "${ESPEAK_STAMP}"
    ok "espeak-ng Android stamp written"
else
    ok "espeak-ng already built (${ESPEAK_STAMP} present)"
    ok "  To rebuild: rm -f ${ESPEAK_STAMP}"
fi

[ -f "${ESPEAK_SO_FILE}" ] || die "libespeak-ng.so missing at ${ESPEAK_SO_FILE}"

# -----------------------------------------------------------------------------
# 2. Compile espeak-ng phoneme data with a native host build
#
# The phoneme tables (phontab, phondata, phonindex, intonations) are generated
# by running the native espeak-ng binary with --compile-phonemes.  We build a
# host-arch binary separately for this step, then package the data directory.
# -----------------------------------------------------------------------------
ESPEAK_NATIVE_BDIR="${BUILD_TMP}/espeak-build-native"
ESPEAK_NATIVE_INSTALL="${BUILD_TMP}/espeak-install-native"
ESPEAK_DATA_STAMP="${BUILD_TMP}/espeak_data_ok.stamp"

find_compiled_data() {
    local hit
    hit="$(find "${BUILD_TMP}" -name "phontab" \
               -path "*/espeak-ng-data/*" 2>/dev/null | head -1)"
    [ -n "${hit}" ] || return 1
    dirname "${hit}"
}

if [ ! -f "${ESPEAK_DATA_STAMP}" ]; then
    if ESPEAK_COMPILED_DATA="$(find_compiled_data)"; then
        ok "Found existing compiled phoneme data: ${ESPEAK_COMPILED_DATA}"
    else
        [ -d "${ESPEAK_SRC}" ] \
            || die "espeak-ng source not found — re-run to trigger a fresh clone"

        log "Building native espeak-ng to compile phoneme data..."
        rm -rf "${ESPEAK_NATIVE_BDIR}" "${ESPEAK_NATIVE_INSTALL}"
        mkdir -p "${ESPEAK_NATIVE_BDIR}" "${ESPEAK_NATIVE_INSTALL}"

        NCPU="$(nproc 2>/dev/null || sysctl -n hw.logicalcpu 2>/dev/null || echo 4)"

        # Provide sonic source to the native cmake build so it never does a
        # live git-fetch during configure (which hangs on slow networks or
        # when the Android build disabled sonic and nothing was downloaded).
        #
        # Strategy:
        #   1. If the Android build already fetched sonic, copy sonic.c/sonic.h
        #      into a plain non-git directory (cmake won't attempt git ops on it).
        #   2. Otherwise, read the GIT_REPOSITORY URL straight from espeak-ng's
        #      own cmake/deps.cmake and do a one-time shallow clone ourselves,
        #      then copy only sonic.c/sonic.h to a plain directory.
        #
        # We always pass FETCHCONTENT_SOURCE_DIR_SONIC pointing at the plain dir.
        # Without FETCHCONTENT_FULLY_DISCONNECTED (which broke populate), cmake
        # uses our directory as-is and skips any network access for sonic.
        SONIC_PLAIN="${BUILD_TMP}/sonic-src-plain"
        SONIC_CACHE_ARGS=()

        if [ ! -f "${SONIC_PLAIN}/sonic.c" ]; then
            SONIC_SRC="${ESPEAK_ANDROID_BDIR}/_deps/sonic-git-src"
            if [ -f "${SONIC_SRC}/sonic.c" ]; then
                log "Copying sonic source from Android build to plain dir..."
                mkdir -p "${SONIC_PLAIN}"
                cp "${SONIC_SRC}/sonic.c" "${SONIC_SRC}/sonic.h" "${SONIC_PLAIN}/"
            else
                # Android build didn't fetch sonic (USE_SONIC=OFF was honoured).
                # Read the repo URL from espeak-ng's own cmake so we use the
                # exact same version espeak-ng expects.
                SONIC_URL="$(grep -A 10 -i 'FetchContent_Declare.*sonic' \
                    "${ESPEAK_SRC}/cmake/deps.cmake" 2>/dev/null \
                    | grep 'GIT_REPOSITORY' \
                    | awk '{print $2}' | head -1)"
                # Fallback to the well-known upstream if grep found nothing.
                SONIC_URL="${SONIC_URL:-https://github.com/waywardgeek/sonic.git}"

                log "Cloning sonic for native build: ${SONIC_URL}"
                SONIC_GIT_TMP="${BUILD_TMP}/sonic-git-tmp"
                rm -rf "${SONIC_GIT_TMP}"
                git clone --depth 1 "${SONIC_URL}" "${SONIC_GIT_TMP}"
                mkdir -p "${SONIC_PLAIN}"
                cp "${SONIC_GIT_TMP}/sonic.c" "${SONIC_GIT_TMP}/sonic.h" "${SONIC_PLAIN}/"
                rm -rf "${SONIC_GIT_TMP}"
            fi
        fi

        if [ -f "${SONIC_PLAIN}/sonic.c" ]; then
            log "Using sonic plain source: ${SONIC_PLAIN}"
            SONIC_CACHE_ARGS=(
                "-DFETCHCONTENT_SOURCE_DIR_SONIC=${SONIC_PLAIN}"
            )
        fi

        if ! PKG_CONFIG_PATH="" cmake -S "${ESPEAK_SRC}" -B "${ESPEAK_NATIVE_BDIR}" \
            -G "Unix Makefiles" \
            -DCMAKE_BUILD_TYPE=Release \
            -DCMAKE_INSTALL_PREFIX="${ESPEAK_NATIVE_INSTALL}" \
            -DBUILD_SHARED_LIBS=OFF \
            -DUSE_ASYNC=OFF \
            -DUSE_SONIC=OFF \
            -DWITH_SONIC=OFF \
            -DWITH_PCAUDIOLIB=OFF \
            -DWITH_SPEECHPLAYER=OFF \
            -DUSE_KLATT=OFF \
            -DCMAKE_DISABLE_FIND_PACKAGE_PcAudio=TRUE \
            -DCMAKE_DISABLE_FIND_PACKAGE_SpeechPlayer=TRUE \
            "${SONIC_CACHE_ARGS[@]}" \
            -Wno-dev \
            > "${ESPEAK_NATIVE_BDIR}/cmake.log" 2>&1
        then
            cat "${ESPEAK_NATIVE_BDIR}/cmake.log"
            die "Native espeak-ng CMake configure failed"
        fi

        if ! cmake --build "${ESPEAK_NATIVE_BDIR}" -- -j"${NCPU}" \
            > "${ESPEAK_NATIVE_BDIR}/build.log" 2>&1
        then
            grep -E "error:" "${ESPEAK_NATIVE_BDIR}/build.log" | head -20
            die "Native espeak-ng build failed"
        fi

        cmake --install "${ESPEAK_NATIVE_BDIR}" \
            >> "${ESPEAK_NATIVE_BDIR}/build.log" 2>&1 || true

        ESPEAK_COMPILED_DATA="$(find_compiled_data)" \
            || die "phontab not found after native build — see ${ESPEAK_NATIVE_BDIR}/build.log"
        ok "Phoneme data compiled: ${ESPEAK_COMPILED_DATA}"
    fi
    touch "${ESPEAK_DATA_STAMP}"
else
    ok "Phoneme data already compiled (stamp present)"
    ESPEAK_COMPILED_DATA="$(find_compiled_data)" \
        || die "Stamp present but phontab not found — delete ${ESPEAK_DATA_STAMP} and re-run"
fi

# Package espeak-ng-data as a zip file for the Android assets directory.
# The app extracts it to internal storage on first launch.
ESPEAK_DATA_ZIP="${ASSETS_DIR}/espeak-ng-data.zip"

log "Packaging espeak-ng-data -> ${ESPEAK_DATA_ZIP}..."
(
    cd "$(dirname "${ESPEAK_COMPILED_DATA}")"
    zip -r -q "${ESPEAK_DATA_ZIP}" "$(basename "${ESPEAK_COMPILED_DATA}")"
)
ok "espeak-ng-data.zip  ($(du -sh "${ESPEAK_DATA_ZIP}" | cut -f1))"

# -----------------------------------------------------------------------------
# 3. Fetch ORT shared library from Maven Central
#
# Microsoft's official ORT Android AAR is compiled with the Android NDK's
# Clang + libc++ (LLVM).  Its shared library has no undefined __cxx11 / GCC
# libstdc++ symbol references — it is safe to load on any modern Android.
#
# We fetch it once and cache it.  Version must match ort-sys 2.0.0-rc.11's
# expected ORT release (ms@1.23.2 seen in the cargo build log).
# Override via:
#   ORT_PREBUILT_VERSION=1.22.0 bash android/build_rust_android.sh
# -----------------------------------------------------------------------------
ORT_PREBUILT_VERSION="${ORT_PREBUILT_VERSION:-1.23.2}"
ORT_SHARED_DIR="${BUILD_TMP}/ort-shared"
ORT_SO="${ORT_SHARED_DIR}/libonnxruntime.so"
ORT_AAR_CACHE="${BUILD_TMP}/onnxruntime-android-${ORT_PREBUILT_VERSION}.aar"
NDK_SYSROOT="${NDK}/toolchains/llvm/prebuilt/${HOST_TAG}/sysroot"

# ── ORT_LIB_DIR override — user supplies an already-extracted .so ─────────
if [ -n "${ORT_LIB_DIR:-}" ]; then
    [ -f "${ORT_LIB_DIR}/libonnxruntime.so" ] \
        || die "ORT_LIB_DIR='${ORT_LIB_DIR}' but libonnxruntime.so not found there"
    ORT_SO="${ORT_LIB_DIR}/libonnxruntime.so"
    ORT_SHARED_DIR="${ORT_LIB_DIR}"
    ok "ORT_LIB_DIR override: ${ORT_SO}"
else
    if [ ! -f "${ORT_SO}" ]; then
        mkdir -p "${ORT_SHARED_DIR}"
        if [ ! -f "${ORT_AAR_CACHE}" ]; then
            log "Downloading ORT ${ORT_PREBUILT_VERSION} Android AAR from Maven Central..."
            AAR_URL="https://repo1.maven.org/maven2/com/microsoft/onnxruntime/onnxruntime-android/${ORT_PREBUILT_VERSION}/onnxruntime-android-${ORT_PREBUILT_VERSION}.aar"
            curl -fL --progress-bar "${AAR_URL}" -o "${ORT_AAR_CACHE}" \
                || die "Failed to download ORT ${ORT_PREBUILT_VERSION} AAR.
  URL: ${AAR_URL}
  Try: ORT_PREBUILT_VERSION=1.22.0 bash android/build_rust_android.sh"
        fi
        log "Extracting libonnxruntime.so (arm64-v8a) from AAR..."
        unzip -p "${ORT_AAR_CACHE}" "jni/arm64-v8a/libonnxruntime.so" \
            > "${ORT_SO}" \
            || die "Failed to extract jni/arm64-v8a/libonnxruntime.so from ${ORT_AAR_CACHE}"
    fi
    ok "ORT shared lib: ${ORT_SO}  ($(du -sh "${ORT_SO}" | cut -f1))"
fi

# -----------------------------------------------------------------------------
# 4. Build kittentts-rs as a static library for Android
#
# ROOT CAUSE OF THE __cxx11 CRASH (confirmed by reading ort-sys source):
#
#   ort-sys 2.0.0-rc.11 build/main.rs — download-binaries path:
#     println!("cargo:rustc-link-search=native={}", bin_extract_dir.display());
#     println!("cargo:rustc-link-lib=static=onnxruntime");
#
#   For a Rust staticlib, cargo embeds every native static library declared
#   via cargo:rustc-link-lib=static=xxx into the output archive.  So
#   libkittentts.a ends up containing the full content of pyke.io's
#   libonnxruntime.a.  That archive was compiled against GCC libstdc++ and
#   carries undefined __cxx11-namespace VTT references
#   (_ZTTNSt7__cxx1119basic_ostringstreamIcSt11char_traitsIcESaIcEEE etc.).
#   Android ships libc++ (__1 namespace), not libstdc++ (__cxx11 namespace),
#   so dlopen fails at runtime.
#
# THE FIX (confirmed by reading ort-sys build/main.rs and build/vars.rs):
#
#   ort-sys checks ORT_LIB_LOCATION + ORT_PREFER_DYNAMIC_LINK:
#
#     if let Some(lib_dir) = vars::get(vars::SYSTEM_LIB_LOCATION) {
#         if dynamic_link::prefer_dynamic_linking() {
#             println!("cargo:rustc-link-lib=onnxruntime");   // dylib — no embed!
#             println!("cargo:rustc-link-search=native={}", lib_dir.display());
#             return;   // early return: no download, no bindgen, no headers needed
#         }
#
#   Setting these two env vars makes ort-sys skip the pyke.io download entirely
#   and emit a dylib link to Maven Central's libonnxruntime.so instead.
#   libkittentts.a then contains only Rust object files with *undefined* ORT
#   symbol references.  Our clang JNI link step (section 5) resolves those
#   symbols dynamically from the Maven Central .so — no GCC ABI code embedded,
#   no __cxx11 symbols, no crash.
#
#   Previous ORT_LIB_LOCATION attempts failed because they did NOT set
#   ORT_PREFER_DYNAMIC_LINK=1, so ort-sys took the static-link path, which
#   requires ORT headers for the full static_link() logic — headers that the
#   Maven Central AAR does not include.  With ORT_PREFER_DYNAMIC_LINK=1 the
#   build script returns immediately after emitting the dylib flag.
# -----------------------------------------------------------------------------
RUST_TARGET_DIR="${BUILD_TMP}/rust-target"

log "Building kittentts-rs for ${RUST_TRIPLE}..."

# Point build.rs to the espeak-ng shared lib so it can emit the correct
# link-search and link-lib directives.  For Android, build.rs uses dylib kind.
ESPEAK_SO_DIR="$(dirname "${ESPEAK_SO_FILE}")"

CARGO_BUILD_LOG="${BUILD_TMP}/cargo_build.log"

CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="${CLANG}" \
CC_aarch64_linux_android="${CLANG}" \
AR_aarch64_linux_android="${NDK_BIN}/llvm-ar" \
ESPEAK_LIB_DIR="${ESPEAK_SO_DIR}" \
ORT_LIB_LOCATION="${ORT_SHARED_DIR}" \
ORT_PREFER_DYNAMIC_LINK=1 \
    cargo build \
        --manifest-path "${ROOT_DIR}/Cargo.toml" \
        --target "${RUST_TRIPLE}" \
        --target-dir "${RUST_TARGET_DIR}" \
        --release \
        --lib \
        2>&1 | tee "${CARGO_BUILD_LOG}" \
              | grep -E "^error|Compiling kittentts|Finished|ort|onnx|download|Downloading"

RUST_LIB="${RUST_TARGET_DIR}/${RUST_TRIPLE}/release/libkittentts.a"
[ -f "${RUST_LIB}" ] || {
    echo "--- last 40 lines of cargo log ---"
    tail -40 "${CARGO_BUILD_LOG}"
    die "Rust staticlib not found: ${RUST_LIB}"
}
ok "kittentts built: ${RUST_LIB}  ($(du -sh "${RUST_LIB}" | cut -f1))"
ok "ORT shared lib: ${ORT_SO}"

# -----------------------------------------------------------------------------
# 5. Compile JNI bridge: kittentts_jni.c → libkittentts_jni.so
#
# Links the Rust staticlib + ORT shared lib + espeak-ng shared lib.
# The resulting .so declares dynamic dependencies on libonnxruntime.so and
# libespeak-ng.so; Android resolves them from jniLibs/ at load time.
# -----------------------------------------------------------------------------
JNI_C="${SCRIPT_DIR}/kittentts_jni.c"
[ -f "${JNI_C}" ] || die "JNI bridge source not found: ${JNI_C}"

JNI_SO="${BUILD_TMP}/libkittentts_jni.so"

log "Compiling JNI bridge -> libkittentts_jni.so..."

# We need the espeak-ng public headers for the kittentts.h include chain.
# espeak-ng installs them under include/espeak-ng/; the NDK sysroot provides
# jni.h and android/log.h.
# NDK_SYSROOT was set in section 3.

"${CLANG}" \
    -shared -fPIC \
    -o "${JNI_SO}" \
    "${JNI_C}" \
    "${RUST_LIB}" \
    -I "${ROOT_DIR}/include" \
    -L "$(dirname "${ORT_SO}")"  -lonnxruntime \
    -L "${ESPEAK_SO_DIR}"        -lespeak-ng \
    --sysroot "${NDK_SYSROOT}" \
    -lc++_shared -llog -lc -lm \
    -Wl,-rpath,'$ORIGIN' \
    -Wl,--build-id

ok "JNI bridge: ${JNI_SO}  ($(du -sh "${JNI_SO}" | cut -f1))"

# -----------------------------------------------------------------------------
# 6. Install .so files into jniLibs/arm64-v8a/
# -----------------------------------------------------------------------------
log "Copying .so files -> ${JNILIBS_DIR}/..."
cp "${JNI_SO}"         "${JNILIBS_DIR}/libkittentts_jni.so"
cp "${ORT_SO}"         "${JNILIBS_DIR}/libonnxruntime.so"
cp "${ESPEAK_SO_FILE}" "${JNILIBS_DIR}/libespeak-ng.so"

# libc++_shared.so — the NDK C++ shared runtime.
# libonnxruntime.so is built by Microsoft with c++_shared, so it has a
# runtime dependency on this library.  Copy it from the NDK sysroot so
# the APK's jniLibs directory is self-contained.
#
# Use find rather than a hardcoded HOST_TAG path — the prebuilt host tag
# varies across NDK versions and macOS architectures (darwin-x86_64 vs
# darwin-arm64) and the hardcoded approach silently fails on Apple Silicon.
LIBCXX_SO="$(find "${NDK}/toolchains/llvm/prebuilt" \
                  -name "libc++_shared.so" \
                  -path "*aarch64-linux-android*" \
                  2>/dev/null | head -1)"

if [ -n "${LIBCXX_SO}" ] && [ -f "${LIBCXX_SO}" ]; then
    cp "${LIBCXX_SO}" "${JNILIBS_DIR}/libc++_shared.so"
    ok "libc++_shared.so  ← ${LIBCXX_SO}"
else
    die "libc++_shared.so not found under ${NDK}/toolchains/llvm/prebuilt/." \
        "Check that your NDK installation is complete."
fi

ok "jniLibs ($(du -sh "${JNILIBS_DIR}" | cut -f1) total):"
ls -lh "${JNILIBS_DIR}/"*.so | awk '{print "    " $NF "\t" $5}'

# -----------------------------------------------------------------------------
# 7. Download model files into assets/models/
# -----------------------------------------------------------------------------
HF_BASE="https://huggingface.co/KittenML/kitten-tts-mini-0.8/resolve/main"
for fname in kitten_tts_mini_v0_8.onnx voices.npz config.json; do
    dest="${MODELS_DIR}/${fname}"
    if [ ! -f "${dest}" ]; then
        log "Downloading ${fname}..."
        curl -fL --progress-bar "${HF_BASE}/${fname}" -o "${dest}"
        ok "${fname}  ($(du -sh "${dest}" | cut -f1))"
    else
        ok "${fname} already present"
    fi
done

# -----------------------------------------------------------------------------
# 8. Generate Gradle wrapper (gradlew + gradle-wrapper.jar)
#
# These files are not stored in the repository because gradle-wrapper.jar is
# a binary.  We generate them here with `gradle wrapper` if Gradle is
# available, or print instructions for doing it manually.
# -----------------------------------------------------------------------------
GRADLEW="${SCRIPT_DIR}/KittenTTSApp/gradlew"

if [ -f "${GRADLEW}" ]; then
    ok "Gradle wrapper already present"
else
    # Ensure JAVA_HOME is set — gradle needs it even for the wrapper task.
    setup_java || {
        echo "  Skipping Gradle wrapper generation (no JDK)."
        echo "  Set JAVA_HOME or install a JDK, then re-run the script."
    }

    if command -v gradle &>/dev/null && [ -n "${JAVA_HOME:-}" ]; then
        log "Generating Gradle wrapper..."
        (
            cd "${SCRIPT_DIR}/KittenTTSApp"
            gradle wrapper --gradle-version 9.3.1 --distribution-type bin --quiet
        )
        chmod +x "${GRADLEW}"
        ok "Gradle wrapper generated -> KittenTTSApp/gradlew"
    else
        echo ""
        echo "  [!!] Gradle wrapper not generated.  Fix:"
        echo ""
        echo "  1. Ensure a JDK is available (see JAVA_HOME messages above)"
        echo "  2. Install Gradle:  brew install gradle"
        echo "  3. Then run:"
        echo "       cd android/KittenTTSApp"
        echo "       gradle wrapper --gradle-version 9.3.1 --distribution-type bin"
        echo "       chmod +x gradlew"
        echo ""
    fi
fi

# -----------------------------------------------------------------------------
echo ""
echo "Build complete!"
echo ""
echo "  Written to android/KittenTTSApp/:"
echo "    app/src/main/jniLibs/${ANDROID_ABI}/  (libkittentts_jni.so, libonnxruntime.so, libespeak-ng.so)"
echo "    app/src/main/assets/espeak-ng-data.zip"
echo "    app/src/main/assets/models/           (ONNX model, voices.npz, config.json)"
echo ""
echo "  Intermediate build artefacts are in ${BUILD_TMP}/"
echo "  (safe to rm -rf after a successful build)"
echo ""
echo "  Next steps:"
echo "    cd android/KittenTTSApp"
echo "    ./gradlew installDebug   # or open in Android Studio"
