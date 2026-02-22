#!/usr/bin/env bash
# -----------------------------------------------------------------------------
# build_rust_ios.sh -- Compile kittentts-rs into KittenTTS.xcframework
#
# Intermediate build artefacts (espeak-ng clone, CMake dirs, cargo output,
# merged .a files) all go under /tmp/kittentts-ios-build/ so the source
# tree stays clean during the build.
#
# Only two things are written back into ios/:
#   KittenTTS.xcframework/   -- the finished static xcframework
#   espeak-ng-data/          -- phoneme data (drag into Xcode as folder ref)
#
# Prerequisites (install once on macOS):
#   brew install cmake rustup
#   rustup-init -y
#   rustup target add aarch64-apple-ios aarch64-apple-ios-sim
# -----------------------------------------------------------------------------
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"

# All intermediate artefacts go here; final outputs go into SCRIPT_DIR (ios/).
BUILD_TMP="/tmp/kittentts-ios-build"
mkdir -p "${BUILD_TMP}"

log() { printf '[..] %s\n' "$*"; }
ok()  { printf '[ok] %s\n' "$*"; }
die() { printf '[!!] %s\n' "$*" >&2; exit 1; }
chk() { command -v "$1" &>/dev/null || die "'$1' not found -- $2"; }

# -----------------------------------------------------------------------------
# 0. Pre-flight
# -----------------------------------------------------------------------------
log "Checking prerequisites..."
chk xcodebuild "install Xcode from the App Store"
chk cmake      "brew install cmake"
chk cargo      "curl https://sh.rustup.rs | sh"
chk rustup     "curl https://sh.rustup.rs | sh"
chk curl       "should be pre-installed on macOS"
chk libtool    "xcode-select --install"
chk git        "xcode-select --install"
chk python3    "brew install python3"

XCODE_PATH="$(xcode-select -p 2>/dev/null)" \
    || die "Xcode not found -- install Xcode from the App Store"
log "Xcode: ${XCODE_PATH}"

log "Adding iOS Rust targets..."
rustup target add aarch64-apple-ios aarch64-apple-ios-sim 2>/dev/null || true
ok "Rust targets ready"

# -----------------------------------------------------------------------------
# 1. Build espeak-ng static library for iOS device and Simulator
#
#  Everything lives under BUILD_TMP:
#    /tmp/kittentts-ios-build/espeak-ng-src/
#    /tmp/kittentts-ios-build/espeak-build-device/
#    /tmp/kittentts-ios-build/espeak-build-simulator/
#    /tmp/kittentts-ios-build/espeak-libs/device/libespeak-ng.a
#    /tmp/kittentts-ios-build/espeak-libs/simulator/libespeak-ng.a
#
#  build.rs expects ESPEAK_LIB_DIR to be a directory containing exactly
#  "libespeak-ng.a" (the name passed to the linker).
# -----------------------------------------------------------------------------
ESPEAK_REPO="https://github.com/espeak-ng/espeak-ng.git"
ESPEAK_TAG="1.52.0"
ESPEAK_SRC="${BUILD_TMP}/espeak-ng-src"
ESPEAK_DEVICE_DIR="${BUILD_TMP}/espeak-libs/device"
ESPEAK_SIM_DIR="${BUILD_TMP}/espeak-libs/simulator"
ESPEAK_DATA_OUT="${SCRIPT_DIR}/espeak-ng-data"    # final location in ios/
# Stamp written only after a fully successful espeak-ng build+merge.
# If it is absent the entire espeak-ng section is re-run on the next
# invocation, making partial/stale builds self-healing.
ESPEAK_STAMP="${BUILD_TMP}/espeak_build_ok.stamp"

build_espeak_slice() {
    local label="$1"   # device | simulator
    local sdk="$2"     # iphoneos | iphonesimulator
    local lib_dir="$3" # output dir (contains libespeak-ng.a)
    local bdir="${BUILD_TMP}/espeak-build-${label}"

    mkdir -p "${lib_dir}"

    local sdk_path
    sdk_path="$(xcrun --sdk "${sdk}" --show-sdk-path)" \
        || die "Cannot find SDK: ${sdk}"

    log "Configuring espeak-ng for iOS ${label} (sdk=${sdk})..."

    # Wipe stale CMake state from any previous failed run.
    rm -rf "${bdir}/CMakeCache.txt" "${bdir}/CMakeFiles" "${bdir}/products"

    local cmake_log="${bdir}/cmake_configure.log"
    mkdir -p "${bdir}"

    # Force-include the endian shim: iOS lacks <endian.h>; le16toh/le32toh
    # are in <libkern/OSByteOrder.h> under OSSwap* names.
    local compat_h="${SCRIPT_DIR}/ios_endian_compat.h"

    # CMAKE_TRY_COMPILE_TARGET_TYPE=STATIC_LIBRARY: required for iOS cross-
    # compilation so check_function_exists() etc. don't try to link an
    # executable for a non-host platform.
    #
    # PKG_CONFIG_PATH="": prevent CMake from finding macOS-only dylibs
    # (speech-player, portaudio ...) that can't be linked into an iOS binary.
    if ! PKG_CONFIG_PATH="" cmake -S "${ESPEAK_SRC}" -B "${bdir}" \
        -G Xcode \
        -DCMAKE_SYSTEM_NAME=iOS \
        -DCMAKE_OSX_SYSROOT="${sdk_path}" \
        -DCMAKE_OSX_DEPLOYMENT_TARGET=16.0 \
        -DCMAKE_OSX_ARCHITECTURES=arm64 \
        -DCMAKE_TRY_COMPILE_TARGET_TYPE=STATIC_LIBRARY \
        "-DCMAKE_C_FLAGS=-include ${compat_h}" \
        -DBUILD_SHARED_LIBS=OFF \
        -DUSE_ASYNC=OFF \
        -DWITH_ASYNC=OFF \
        -DWITH_PCAUDIOLIB=OFF \
        -DWITH_SPEECHPLAYER=OFF \
        -DWITH_SONIC=OFF \
        -DUSE_KLATT=OFF \
        "-DCMAKE_DISABLE_FIND_PACKAGE_SpeechPlayer=TRUE" \
        "-DCMAKE_DISABLE_FIND_PACKAGE_PcAudio=TRUE" \
        -DCMAKE_XCODE_ATTRIBUTE_ONLY_ACTIVE_ARCH=NO \
        -DCMAKE_XCODE_ATTRIBUTE_ENABLE_BITCODE=NO \
        -Wno-dev \
        > "${cmake_log}" 2>&1
    then
        echo ""
        echo "--- CMake configure failed for espeak-ng (${label}) ---"
        cat "${cmake_log}"
        echo "--- end of log ---"
        die "CMake configure failed.  See output above."
    fi
    grep -E "^-- " "${cmake_log}" | tail -6 || true

    local xcproj
    xcproj="$(find "${bdir}" -maxdepth 2 -name "*.xcodeproj" | head -1)"
    [ -n "${xcproj}" ] || die "No .xcodeproj found in ${bdir}"

    log "Building espeak-ng for iOS ${label}..."

    local build_log="${bdir}/xcodebuild.log"
    if ! xcodebuild \
        -project "${xcproj}" \
        -target espeak-ng \
        -configuration Release \
        -sdk "${sdk}" \
        ONLY_ACTIVE_ARCH=NO \
        ARCHS=arm64 \
        "SYMROOT=${bdir}/products" \
        build \
        > "${build_log}" 2>&1
    then
        echo ""
        echo "--- xcodebuild failed for espeak-ng (${label}) ---"
        grep -E "error:|BUILD FAILED" "${build_log}" | head -30
        echo "(full log: ${build_log})"
        die "xcodebuild failed for espeak-ng (${label})"
    fi
    ok "espeak-ng (${label}) built"

    # Collect EVERY static library the build produced anywhere under bdir:
    #   libespeak-ng.a  -- the main library
    #   libucd.a        -- Unicode helpers (ucd_isalpha etc.); built as a
    #                      separate CMake target, NOT contained in libespeak-ng.a
    #   libsonic.a      -- Klatt speed-scaling (if compiled)
    # We search the entire build tree, not just bdir/products, because
    # FetchContent and sub-project libs land in bdir/_deps/*/build/ etc.
    local all_libs=()
    while IFS= read -r l; do
        all_libs+=("$l")
    done < <(find "${bdir}" -name "*.a" 2>/dev/null | sort)

    [ "${#all_libs[@]}" -gt 0 ] || \
        die "No .a files found after building espeak-ng (${label})"

    log "Merging ${#all_libs[@]} espeak-ng lib(s) into libespeak-ng.a..."
    for l in "${all_libs[@]}"; do log "  ${l}"; done
    libtool -static -o "${lib_dir}/libespeak-ng.a" "${all_libs[@]}"
    ok "espeak-ng (${label}) -> ${lib_dir}/libespeak-ng.a  ($(du -sh "${lib_dir}/libespeak-ng.a" | cut -f1))"
}

patch_espeak_for_ios() {
    log "Patching espeak-ng CMakeLists.txt files for iOS..."
    cat > /tmp/patch_espeak_ios.py << 'PYEOF'
import re, os, sys

src_root = sys.argv[1]

def patch_file(path):
    text = open(path).read()
    orig = text

    # 1. src/CMakeLists.txt: remove install(TARGETS espeak-ng-bin ...).
    #    iOS treats every executable as a MACOSX_BUNDLE and CMake requires
    #    a BUNDLE DESTINATION clause that espeak-ng doesn't provide.
    text = re.sub(
        r'install\s*\(\s*TARGETS\s+espeak-ng-bin\b[^)]*\)',
        '# [iOS patch] install(espeak-ng-bin) removed -- no BUNDLE DESTINATION',
        text, flags=re.DOTALL
    )

    # 2. Any CMakeLists.txt: replace find_library/find_path for speech-player
    #    with hardcoded NOTFOUND values.
    #
    #    espeak-ng uses find_library(SPEECHPLAYER_LIB speechplayer) and
    #    find_path(SPEECHPLAYER_INC speechPlayer.h) -- NOT find_package().
    #    PKG_CONFIG_PATH="" and CMAKE_DISABLE_FIND_PACKAGE_SpeechPlayer have
    #    no effect on these calls.  CMake's find_library searches system
    #    paths (/usr/local/lib etc.) independently of pkg-config, so it
    #    finds the macOS dylib even on a cross-compile.  Patching the source
    #    is the only reliable way to disable it.
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

# Walk the entire source tree so we catch the top-level and src/ files
for root, dirs, files in os.walk(src_root):
    for fname in files:
        if fname == 'CMakeLists.txt':
            patch_file(os.path.join(root, fname))
PYEOF
    python3 /tmp/patch_espeak_ios.py "${ESPEAK_SRC}"
    ok "espeak-ng patched"
}

if [ ! -f "${ESPEAK_STAMP}" ]; then
    # Stamp absent = first run, or a previous run failed partway through.
    # Always start from a clean slate so stale partial libs are never reused.
    # If you get linker errors like "undefined _ucd_*" or "_speechPlayer_*",
    # delete the build tree and re-run:  rm -rf /tmp/kittentts-ios-build

    if [ ! -d "${ESPEAK_SRC}/.git" ]; then
        log "Cloning espeak-ng ${ESPEAK_TAG}..."
        git clone --depth 1 --branch "${ESPEAK_TAG}" \
            "${ESPEAK_REPO}" "${ESPEAK_SRC}"
    fi

    patch_espeak_for_ios

    build_espeak_slice device     iphoneos        "${ESPEAK_DEVICE_DIR}"
    build_espeak_slice simulator  iphonesimulator "${ESPEAK_SIM_DIR}"

    # Write the stamp only after everything succeeded.
    touch "${ESPEAK_STAMP}"
    ok "espeak-ng build stamp written"
else
    ok "espeak-ng already built (${ESPEAK_STAMP} present)"
    ok "  To force a rebuild: rm -rf /tmp/kittentts-ios-build"
fi

# ---------------------------------------------------------------------------
# Compile espeak-ng phoneme data using a native macOS build.
#
# The espeak-ng 1.52.0 source repo does NOT ship pre-compiled phoneme tables.
# phontab / phondata / phonindex / intonations are generated by running the
# espeak-ng binary with --compile-phonemes, reading from phsource/ and
# writing into espeak-ng-data/.
#
# The iOS cross-compile produces an arm64-ios binary that cannot run on the
# macOS build host, so we build a separate native macOS espeak-ng binary just
# to drive the compile step.
#
# cmake --install puts the compiled data at an install-prefix-dependent path
# (lib/espeak-ng-data, share/espeak-ng-data, etc.).  We use `find` to locate
# phontab wherever it lands rather than hardcoding the subdirectory.
# ---------------------------------------------------------------------------

ESPEAK_NATIVE_BDIR="${BUILD_TMP}/espeak-build-native"
ESPEAK_NATIVE_INSTALL="${BUILD_TMP}/espeak-install-native"
ESPEAK_DATA_STAMP="${BUILD_TMP}/espeak_data_ok.stamp"

# Locate an already-compiled espeak-ng-data directory anywhere under BUILD_TMP.
# Returns the directory path (parent of phontab) on stdout, or returns 1.
find_compiled_data() {
    local hit
    hit="$(find "${BUILD_TMP}" -name "phontab" \
               -path "*/espeak-ng-data/*" 2>/dev/null | head -1)"
    [ -n "${hit}" ] || return 1
    dirname "${hit}"
}

if [ ! -f "${ESPEAK_DATA_STAMP}" ]; then
    # Fast path: a previous run already compiled the data somewhere under BUILD_TMP.
    if ESPEAK_COMPILED_DATA="$(find_compiled_data)"; then
        ok "Found existing compiled phoneme data: ${ESPEAK_COMPILED_DATA}"
    else
        [ -d "${ESPEAK_SRC}" ] \
            || die "espeak-ng source not found at ${ESPEAK_SRC} — re-run to trigger a fresh clone"

        log "Building native macOS espeak-ng to compile phoneme data..."

        rm -rf "${ESPEAK_NATIVE_BDIR}" "${ESPEAK_NATIVE_INSTALL}"
        mkdir -p "${ESPEAK_NATIVE_BDIR}" "${ESPEAK_NATIVE_INSTALL}"

        NCPU_NATIVE="$(sysctl -n hw.logicalcpu 2>/dev/null || echo 4)"

        NATIVE_CMAKE_LOG="${ESPEAK_NATIVE_BDIR}/cmake.log"
        if ! PKG_CONFIG_PATH="" cmake -S "${ESPEAK_SRC}" -B "${ESPEAK_NATIVE_BDIR}" \
            -G "Unix Makefiles" \
            -DCMAKE_BUILD_TYPE=Release \
            -DCMAKE_INSTALL_PREFIX="${ESPEAK_NATIVE_INSTALL}" \
            -DBUILD_SHARED_LIBS=OFF \
            -DUSE_ASYNC=OFF \
            -DWITH_PCAUDIOLIB=OFF \
            -DWITH_SPEECHPLAYER=OFF \
            -DWITH_SONIC=OFF \
            -DUSE_KLATT=OFF \
            -Wno-dev \
            > "${NATIVE_CMAKE_LOG}" 2>&1
        then
            cat "${NATIVE_CMAKE_LOG}"
            die "Native espeak-ng CMake configure failed"
        fi

        NATIVE_BUILD_LOG="${ESPEAK_NATIVE_BDIR}/build.log"
        if ! cmake --build "${ESPEAK_NATIVE_BDIR}" -- -j"${NCPU_NATIVE}" \
            > "${NATIVE_BUILD_LOG}" 2>&1
        then
            grep -E "error:" "${NATIVE_BUILD_LOG}" | head -20
            echo "(full log: ${NATIVE_BUILD_LOG})"
            die "Native espeak-ng build failed"
        fi

        if ! cmake --install "${ESPEAK_NATIVE_BDIR}" \
            >> "${NATIVE_BUILD_LOG}" 2>&1
        then
            grep -E "error:" "${NATIVE_BUILD_LOG}" | head -20
            die "Native espeak-ng install failed"
        fi

        # Locate the compiled data (install may put it under lib/, share/, etc.)
        ESPEAK_COMPILED_DATA="$(find_compiled_data)" \
            || die "phontab not found anywhere under ${BUILD_TMP} after native build+install"

        ok "Phoneme data compiled: ${ESPEAK_COMPILED_DATA}"
    fi

    touch "${ESPEAK_DATA_STAMP}"
else
    ok "Phoneme data already compiled (${ESPEAK_DATA_STAMP} present)"
    ESPEAK_COMPILED_DATA="$(find_compiled_data)" \
        || die "Stamp present but phontab not found under ${BUILD_TMP} — delete ${ESPEAK_DATA_STAMP} and re-run"
fi

# Sync compiled phoneme data into ios/espeak-ng-data/ (always runs so that
# a deleted or stale ios/espeak-ng-data/ is always repaired on re-run).
log "Syncing espeak-ng phoneme data -> ios/espeak-ng-data/ ..."
log "  source: ${ESPEAK_COMPILED_DATA}"
rm -rf "${ESPEAK_DATA_OUT}"
cp -r "${ESPEAK_COMPILED_DATA}" "${ESPEAK_DATA_OUT}"
[ -f "${ESPEAK_DATA_OUT}/phontab" ] \
    || die "phontab missing after sync — source was: ${ESPEAK_COMPILED_DATA}"
ok "espeak-ng-data synced ($(find "${ESPEAK_DATA_OUT}" -not -name '.DS_Store' | wc -l | tr -d ' ') entries)"

# -----------------------------------------------------------------------------
# 2. Build kittentts-rs  (cargo output -> BUILD_TMP/rust-target/)
# -----------------------------------------------------------------------------
RUST_TARGET_DIR="${BUILD_TMP}/rust-target"

build_kittentts() {
    local triple="$1"
    local espeak_dir="$2"

    log "cargo build --target ${triple}..."
    ESPEAK_LIB_DIR="${espeak_dir}" \
    cargo build \
        --manifest-path "${ROOT_DIR}/Cargo.toml" \
        --target "${triple}" \
        --target-dir "${RUST_TARGET_DIR}" \
        --release \
        --lib \
        2>&1 | grep -E "^error|Compiling kittentts|Finished"

    ok "kittentts built for ${triple}"
}

build_kittentts aarch64-apple-ios     "${ESPEAK_DEVICE_DIR}"
build_kittentts aarch64-apple-ios-sim "${ESPEAK_SIM_DIR}"

# -----------------------------------------------------------------------------
# 3. Locate ORT static libs (ort-sys downloads + caches them automatically)
#
#  ort-sys >= rc.9 caches at:
#    ~/Library/Caches/ort.pyke.io/dfbin/<triple>/<hash>/libonnxruntime.a
#  Older versions used:
#    ~/Library/Caches/ort.pyke.io/<hash>/libonnxruntime.a
# -----------------------------------------------------------------------------
ORT_CACHE="${HOME}/Library/Caches/ort.pyke.io"
ORT_ARM64_HASH="e8ec605a27072a31086407d6eb2c6bcc5d4f567dc090f380000326d740ca72dc"
ORT_SIM_HASH="8ae2dfc164b21d0a9f7b8ad046193eb20b4e9c198a21ba8dba0a71e962e15776"

find_ort_lib() {
    local triple="$1"
    local hash="$2"
    local label="$3"
    local lib

    lib="$(find "${ORT_CACHE}/dfbin/${triple}/${hash}" \
               -name "libonnxruntime.a" 2>/dev/null | head -1)"
    [ -n "${lib}" ] || \
    lib="$(find "${ORT_CACHE}/${hash}" \
               -name "libonnxruntime.a" 2>/dev/null | head -1)"
    [ -n "${lib}" ] || \
    lib="$(find "${ORT_CACHE}" \
               -path "*/${hash}/libonnxruntime.a" 2>/dev/null | head -1)"

    if [ -z "${lib}" ]; then
        echo ""
        echo "ORT not found for ${label}."
        echo "Hash: ${hash}"
        echo "Searched: ${ORT_CACHE}"
        echo "Cache contents:"
        find "${ORT_CACHE}" -name "libonnxruntime.a" 2>/dev/null \
            | sed 's|^|  |' || echo "  (empty)"
        die "ORT static lib missing -- did cargo build succeed?"
    fi
    echo "${lib}"
}

ORT_ARM64_LIB="$(find_ort_lib "aarch64-apple-ios"     "${ORT_ARM64_HASH}" "iOS device")"
ORT_SIM_LIB="$(find_ort_lib   "aarch64-apple-ios-sim" "${ORT_SIM_HASH}"   "iOS Simulator")"
ok "ORT device    -> ${ORT_ARM64_LIB}"
ok "ORT simulator -> ${ORT_SIM_LIB}"

# -----------------------------------------------------------------------------
# 4. Merge Rust + ORT + espeak-ng into one .a per slice  (in BUILD_TMP)
# -----------------------------------------------------------------------------
merge_slice() {
    local triple="$1"
    local ort_lib="$2"
    local espeak_lib="$3"
    local out_lib="$4"

    local rust_lib="${RUST_TARGET_DIR}/${triple}/release/libkittentts.a"
    [ -f "${rust_lib}" ] || die "Rust lib not found: ${rust_lib}"

    log "Merging static libs for ${triple}..."
    libtool -static -o "${out_lib}" \
        "${rust_lib}" "${ort_lib}" "${espeak_lib}"
    ok "merged -> $(basename "${out_lib}")  ($(du -sh "${out_lib}" | cut -f1))"
}

COMBINED_ARM64="${BUILD_TMP}/libKittenTTS-device.a"
COMBINED_SIM="${BUILD_TMP}/libKittenTTS-simulator.a"

merge_slice aarch64-apple-ios \
    "${ORT_ARM64_LIB}" \
    "${ESPEAK_DEVICE_DIR}/libespeak-ng.a" \
    "${COMBINED_ARM64}"

merge_slice aarch64-apple-ios-sim \
    "${ORT_SIM_LIB}" \
    "${ESPEAK_SIM_DIR}/libespeak-ng.a" \
    "${COMBINED_SIM}"

# -----------------------------------------------------------------------------
# 5. Create KittenTTS.xcframework and copy it into ios/
# -----------------------------------------------------------------------------
XCFWK_TMP="${BUILD_TMP}/KittenTTS.xcframework"
XCFWK_DEST="${SCRIPT_DIR}/KittenTTS.xcframework"

log "Creating KittenTTS.xcframework..."
rm -rf "${XCFWK_TMP}"

xcodebuild -create-xcframework \
    -library "${COMBINED_ARM64}" -headers "${ROOT_DIR}/include" \
    -library "${COMBINED_SIM}"   -headers "${ROOT_DIR}/include" \
    -output  "${XCFWK_TMP}"

log "Copying KittenTTS.xcframework -> ios/..."
rm -rf "${XCFWK_DEST}"
cp -r "${XCFWK_TMP}" "${XCFWK_DEST}"
ok "KittenTTS.xcframework -> ${XCFWK_DEST}  ($(du -sh "${XCFWK_DEST}" | cut -f1))"

# -----------------------------------------------------------------------------
# 6. Download model files
# -----------------------------------------------------------------------------
MODEL_DIR="${SCRIPT_DIR}/KittenTTSApp/KittenTTSApp/Models"
mkdir -p "${MODEL_DIR}"

HF_BASE="https://huggingface.co/KittenML/kitten-tts-mini-0.8/resolve/main"
for fname in kitten_tts_mini_v0_8.onnx voices.npz config.json; do
    dest="${MODEL_DIR}/${fname}"
    if [ ! -f "${dest}" ]; then
        log "Downloading ${fname}..."
        curl -fL --progress-bar "${HF_BASE}/${fname}" -o "${dest}"
        ok "${fname}  ($(du -sh "${dest}" | cut -f1))"
    else
        ok "${fname} already present"
    fi
done

# -----------------------------------------------------------------------------
echo ""
echo "Build complete!"
echo ""
echo "  Written to ios/:"
echo "    KittenTTS.xcframework/   (link this in Xcode)"
echo "    espeak-ng-data/          (drag into Xcode as folder reference)"
echo "    KittenTTSApp/KittenTTSApp/Models/"
echo ""
echo "  Intermediate build artefacts are in ${BUILD_TMP}/"
echo "  (safe to rm -rf after a successful build)"
echo ""
echo "  Next steps in Xcode:"
echo "  1. open ios/KittenTTSApp/KittenTTSApp.xcodeproj"
echo "  2. Drag ios/espeak-ng-data/ -> 'Create folder references'."
echo "  3. Signing & Capabilities -> set your development team."
echo "  4. Product -> Run."
