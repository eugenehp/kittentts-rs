# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.2.5] - 2026-03-10

### Added

- **Windows zero-config espeak-ng auto-build** (`build.rs` step 4): when the
  `espeak` feature is enabled on a native Windows host and no
  `ESPEAK_LIB_DIR` is set, the build script automatically:
  - Clones `espeak-ng` from GitHub (tag `1.52.0`, shallow clone) into
    `$OUT_DIR/espeak-auto/`.
  - Compiles it as a static library with cmake, using either the MSVC
    generator (when `cl.exe` is reachable) or `"MinGW Makefiles"` (when
    MSYS2 gcc is present).
  - Links `espeak-ng.lib` / `libespeak-ng.a` statically.
  - Bakes the path to `espeak-ng-data/` as a compile-time env var
    (`KITTENTTS_ESPEAK_DATA_DIR`) so `cargo run` / `cargo test` work
    immediately without any manual data setup.
  - Uses a stamp file (`built-<tag>.stamp`) to skip the clone + build on
    subsequent `cargo build` calls.
  - Override version with `ESPEAK_TAG`; bypass entirely with
    `ESPEAK_LIB_DIR` pointing at a pre-built archive.

- **espeak-ng data path fallback chain** (`src/phonemize.rs`): `do_init()`
  now resolves the data directory through four levels:
  1. `set_data_path()` / `DATA_PATH` OnceCell (runtime override via the
     public API).
  2. `KITTENTTS_ESPEAK_DATA_DIR` compile-time env var (baked in by the
     Windows auto-build).
  3. `<exe_dir>/espeak-ng-data/` checked at runtime (supports shipping the
     data folder next to the `.exe`).
  4. `NULL` — lets espeak-ng use its compiled-in default (works on
     Linux/macOS where the package manager places data at a known path).

- **`scripts/test-windows-cross.sh`** — shell script for testing the Windows
  cross-compilation from Linux or macOS:
  - Uses `cargo-zigbuild` + `zig` as the cross-linker.
  - Downloads the official Microsoft ORT Windows ZIP automatically, parses
    the DLL's PE export table with an embedded Python script (no extra deps),
    and creates a GNU import library (`libonnxruntime.dll.a`) via
    `zig dlltool` / `llvm-dlltool` / `x86_64-w64-mingw32-dlltool`.
  - `--espeak`: cross-compiles espeak-ng for Windows using
    `x86_64-w64-mingw32-gcc` and cmake.
  - `--wine`: runs the compiled test binary under Wine after a successful
    build, with the ORT DLL on `WINEPATH`.
  - `--clean`: deletes cached ORT download and espeak build directories.
  - Caches downloads and espeak builds between runs (stamp files).

- **`scripts/test-windows-native.ps1`** — PowerShell script for building
  and testing kittentts natively on Windows:
  - Checks prerequisites: `cargo`, `git`, `cmake`, and a C compiler (MSVC
    or MinGW).  Auto-activates MSVC via `vswhere.exe` + `vcvars64.bat` when
    not already in PATH; auto-detects MSYS2/MinGW64 at `$env:MSYS2_PATH`
    (default `C:\msys64`).
  - `-Espeak`: runs `cargo test --features espeak`, which triggers the
    zero-config auto-build of espeak-ng (see above).
  - `-EspeakOnly`, `-BuildOnly`, `-Clean`, `-Verbose` flags for CI flexibility.
  - Prints a colour-coded pass/fail summary; exits non-zero on any failure.
  - Reminds the user to ship `espeak-ng-data/` next to the `.exe` when
    distributing.

### Changed

- `build.rs`: added `find_espeak_data_near_lib()` and `emit_espeak_data()`
  helper functions; called at all three existing successful lib-location
  paths on Windows (explicit `ESPEAK_LIB_DIR`, static platform-path-walk
  branch, dynamic platform-path-walk branch) so
  `KITTENTTS_ESPEAK_DATA_DIR` is always set on Windows regardless of how
  espeak-ng was found.

---

## [0.2.4] - 2026-03-10

### Added

- **Integration and end-to-end test suite** (`tests/integration_tests.rs`) — 40 tests covering
  every public module, runnable without network access using the bundled iOS/Android model files:
  - `tokenize` — vocab completeness, IPA→ID encoding, pad handling, unknown-char
    skip, empty-string edge cases.
  - `preprocess` — number/float words, ordinals, percentages, currency (dollar,
    scaling suffixes), contractions, unit expansion, whitespace normalisation,
    full pipeline round-trip.
  - `npz` — NPY round-trips (1-D and 2-D), `NpyArray` row access, bad-magic and
    truncated-file error paths, real `voices.npz` loading and shape validation.
  - `phonemize` *(requires `--features espeak`)* — espeak availability, en-us
    IPA for words/numbers/long sentences, empty-string edge case.
  - `model` — sample-rate constant, voice listing, IPA→audio generation
    (amplitude range, minimum duration), multi-chunk concatenation length,
    WAV file output (existence, size > 44 bytes), unknown-voice error,
    text→audio (`generate`, `generate_chunk`, `generate_to_file`) gated on
    `espeak` feature.
  - Model directory auto-discovery: checks `$KITTENTTS_MODEL_DIR`, then the
    bundled iOS and Android asset paths; skips inference tests gracefully when
    no model files are present.

- **`KITTENTTS_MODEL_DIR` environment variable** — allows pointing the
  integration tests at an arbitrary directory containing `kitten_tts_mini_v0_8.onnx`,
  `voices.npz`, and `config.json` without modifying the source tree.

### Changed

- **`hf-hub` dependency**: switched from the default `native-tls` backend
  (which pulls in `openssl-sys`) to
  `{ default-features = false, features = ["ureq"] }`.  `ureq` v3 uses
  `rustls` by default — pure-Rust TLS, no OpenSSL required, fully compatible
  with `cargo-zigbuild` cross-compilation.

- **`ort` TLS feature**: changed from `tls-native` to `tls-rustls` for the
  same reason — eliminates the OpenSSL dependency across all desktop targets
  and makes cross-compilation to Windows and Linux x86_64 work out-of-the-box.

- **Cross-compilation to Windows x64** (`x86_64-pc-windows-gnu`) now works
  with `cargo-zigbuild`:
  - ORT Windows import library generated via `llvm-dlltool` from the official
    ORT Windows DLL; pointed at via `ORT_LIB_LOCATION` + `ORT_PREFER_DYNAMIC_LINK=1`.
  - The produced `libkittentts.a` contains genuine x86-64 COFF object files
    (verified with `file`), ready to link into a Windows binary alongside
    `onnxruntime.dll`.

- **Cross-compilation to Linux x86_64** (`x86_64-unknown-linux-gnu`) now works
  with `cargo-zigbuild` — ORT downloads its prebuilt `x86_64-linux-gnu` static
  library automatically via the `download-binaries` feature.

- **`.cargo/config.toml`** — documented `cargo-zigbuild` usage and per-target
  linker settings for `x86_64-pc-windows-gnu` and `x86_64-unknown-linux-gnu`.

## [0.2.3] - 2026-03-10

### Added

- **Windows support in `build.rs`** — the build script now fully supports
  MSVC and MinGW toolchains on Windows:
  - `static_lib_name()` returns `espeak-ng.lib` for MSVC, `libespeak-ng.a`
    for GNU/MinGW.
  - `link_cxx()` is a no-op on MSVC (the runtime is linked automatically) and
    emits `libstdc++` on MinGW, matching the behaviour on Linux.
  - `has_dylib()` detects `espeak-ng.dll`, `espeak-ng.dll.a`, and
    `espeak-ng.lib` as dynamic fallbacks on Windows.
  - `pkg_config_libdir()` uses `;` as the `PKG_CONFIG_PATH` separator on
    Windows.
  - `run_espeak_build_script()` dispatches to PowerShell (`.ps1`) or
    `cmd` (`.bat`/`.cmd`) on Windows instead of `bash`.
  - `candidate_dirs()` searches the eSpeak NG installer default path,
    vcpkg triplets (arch-aware, `x64-windows-static` preferred),
    MSYS2/MinGW64 (via `MSYS2_PATH` or `C:\msys64`), and Chocolatey.
  - Two new `rerun-if-env-changed` triggers: `VCPKG_ROOT` and `MSYS2_PATH`.

- **Full cross-compilation support in `build.rs`**:
  - Reads `HOST` and `TARGET` Cargo env vars; derives `host_os` and `is_cross`.
  - `ESPEAK_SYSROOT` env var: when set, all Unix candidate lib paths are
    prefixed with the sysroot (e.g.
    `$ESPEAK_SYSROOT/usr/lib/aarch64-linux-gnu`).
  - `target_to_multiarch()`: maps Cargo target fields to Debian/Ubuntu GNU
    multiarch tuples (e.g. `aarch64-unknown-linux-gnu` → `aarch64-linux-gnu`).
    Returns `None` for musl, Android, Windows, macOS, etc.
  - `os_from_triple()`: derives a plain OS string from any Cargo/Rust target
    triple string (used for host-OS detection without `cfg!()`).
  - `android_abi()`: maps `CARGO_CFG_TARGET_ARCH` to Android ABI directory
    names inside the NDK sysroot.
  - **Cross-aware pkg-config**: tries `<multiarch>-pkg-config` first (e.g.
    `aarch64-linux-gnu-pkg-config`, installed by
    `apt install pkg-config-aarch64-linux-gnu` on Debian/Ubuntu), then falls
    back to regular `pkg-config` with `PKG_CONFIG_ALLOW_CROSS=1` and the
    multiarch pkgconfig directory prepended to `PKG_CONFIG_PATH`.
    `PKG_CONFIG_SYSROOT_DIR` is set to `ESPEAK_SYSROOT` when provided.
  - **Sysroot-aware `candidate_dirs()`**: Linux GNU paths prefixed by
    `ESPEAK_SYSROOT`; Linux musl uses a flat `/usr/lib` layout; Android probes
    `$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/<host>/sysroot/usr/lib/<abi>/<api>`
    for API levels 21–35; iOS falls back to Homebrew only on a macOS host;
    macOS/Windows host tools (brew, MSYS2) guarded so they are never invoked
    when cross-compiling from a different host.
  - `run_espeak_build_script()` forwards `ESPEAK_TARGET`, `ESPEAK_TARGET_OS`,
    `ESPEAK_TARGET_ARCH`, `ESPEAK_SYSROOT`, and `ANDROID_NDK_HOME` into the
    build script process so it can set up the correct cross-compilation
    toolchain automatically.
  - New `rerun-if-env-changed` triggers: `ESPEAK_SYSROOT`,
    `PKG_CONFIG_ALLOW_CROSS`, `ANDROID_NDK_HOME`, `ANDROID_NDK_ROOT`,
    `NDK_HOME`.

- **`scripts/build-espeak-static.sh`** — bash script that compiles
  `libespeak-ng.a` from source for any supported host/target combination.
  Invoked automatically by `build.rs` via `ESPEAK_BUILD_SCRIPT`; also usable
  standalone. Supports:
  - Native macOS and Linux builds.
  - Linux GNU cross-compilation (aarch64, armv7, arm, i686, riscv64, s390x,
    powerpc64le, loongarch64) via a generated CMake toolchain file and the
    host's cross-compiler (`aarch64-linux-gnu-gcc`, etc.).
  - Linux musl cross-compilation via musl-cross-make toolchains.
  - Android arm64/armv7/x86_64/x86 via the NDK's `android.toolchain.cmake`
    (NDK r25+ required; NDK auto-detected from `ANDROID_NDK_HOME`,
    `ANDROID_NDK_ROOT`, `NDK_HOME`, or Android Studio's default install path).
  - For cross targets, patches the espeak-ng source in a per-target copy to
    remove the `espeak-ng-bin` executable (which references
    `<wordexp.h>` / host-only headers) so the CMake configure step succeeds.
  - Merges `libespeak-ng.a` and all companion archives (`libucd.a`,
    `libsonic.a`, etc.) into a single self-contained archive using
    `libtool -static` (macOS) or a GNU `ar` MRI script (Linux/cross).
  - Stamp-file caching: skips clone and build on re-runs; delete the stamp
    to force a rebuild.

- **`scripts/build-espeak-static.ps1`** — PowerShell equivalent for Windows.
  Supports MSYS2/MinGW-w64 (auto-detected at `MSYS2_PATH` or `C:\msys64`,
  produces `libespeak-ng.a`) and MSVC (auto-detected via `vswhere.exe` or
  `cl.exe` in PATH, activates `vcvars64.bat` automatically, produces
  `espeak-ng.lib`). Merges companion archives with a GNU `ar` MRI script
  (MinGW) or `lib.exe /OUT:` (MSVC).

- **`Cross.toml`** — configuration for
  [`cargo cross`](https://github.com/cross-rs/cross). Adds `pre-build`
  commands for every supported Linux target that install `libespeak-ng-dev`
  (or `espeak-ng-static`) for the target architecture inside the cross Docker
  image so that `cross build --target <triple> --features espeak` works with
  zero extra setup:
  - **GNU targets** (aarch64, armv7, arm, i686, x86_64, riscv64, ppc64le,
    s390x): uses Debian multiarch (`dpkg --add-architecture` + `apt-get
    install libespeak-ng-dev:<arch>`).
  - **Musl targets** (x86_64, aarch64, armv7, arm, i686): uses Alpine
    `apk add espeak-ng-dev espeak-ng-static`.
  - **Android, Windows** targets documented with instructions for
    `ESPEAK_LIB_DIR`; no `pre-build` needed without the `espeak` feature.

- **`.cargo/config.toml`** — extended with a comprehensive commented-out
  catalogue of per-target cross-compilation linker settings covering all
  Linux GNU/musl, Android, Windows MinGW, and macOS (osxcross) targets.

### Changed

- `build.rs`: `link_cxx()` now has a three-way split: MSVC Windows (no-op),
  macOS/iOS (`libc++`), and Linux/MinGW/Android (`libstdc++`).
- `build.rs`: `candidate_dirs()` now covers FreeBSD/OpenBSD/NetBSD via a
  `_` fall-through arm (`/usr/local/lib`, sysroot-prefixed) instead of
  silently treating them as Linux.
- `build.rs`: `pkg_config_libdir()` no longer calls `brew` when the build
  host is not macOS (host-guarded).

---

## [0.2.0] - 2026-02-28

### Changed

- **Build script rewrite** (`build.rs`): replaced the simple `pkg-config`-based
  linker setup with a fully self-contained resolution pipeline:
  1. `ESPEAK_LIB_DIR` env var — explicit path for mobile cross-compilation.
  2. `pkg-config` — augmented on macOS with Homebrew's pkgconfig prefix paths
     so that `brew install espeak-ng` works out-of-the-box without any env vars.
     The libdir reported by pkg-config is always emitted as a
     `rustc-link-search` to handle Homebrew's non-standard prefix
     (`/opt/homebrew`, `/usr/local`).
  3. Platform path walk — probes known directories (Homebrew keg paths on
     macOS, Debian/Ubuntu multi-arch dirs on Linux) when pkg-config is absent.
- **Static-library preference**: at every resolution step the script now prefers
  `libespeak-ng.a` over the dynamic library, and automatically links the C++
  standard library when a static archive is selected (espeak-ng is a C++
  project).
- **Removed `pkg-config` build-dependency**: the build script now calls
  `pkg-config` and `brew` directly via `std::process::Command`, eliminating the
  compile-time dependency on the `pkg-config` crate.
- **Desktop `zip` dependency**: switched from the default full-featured `zip`
  to `deflate`-only (`default-features = false, features = ["deflate"]`) on all
  platforms. NPZ files only use deflate (or store), and this avoids the
  `lzma-sys` / `liblzma-sys` symbol conflict introduced by the tracel-llvm
  bundler.

## [0.1.0] - 2026-02-22

### Added

- Initial release — Rust port of [KittenTTS](https://github.com/KittenML/KittenTTS).
- ONNX-based TTS inference via `ort` (ORT 2.0.0-rc.11).
- Full text preprocessing pipeline: numbers, currencies, ordinals, units,
  abbreviations, contractions, scientific notation, fractions, and more.
- espeak-ng FFI phonemisation (`libespeak-ng` linked at build time).
- IPA-to-token-ID tokeniser matching the Python `TextCleaner` vocabulary.
- Hand-written NPY/NPZ loader — no `ndarray-npy` dependency.
- Automatic long-text chunking (≤ 400 chars, sentence boundaries) with
  concatenation of per-chunk audio.
- HuggingFace Hub model download (`hf-hub`) for desktop targets.
- WAV file output via `hound` (24 kHz, float32, mono).
- C FFI layer (`src/ffi.rs`) and C header (`include/kittentts.h`) for use from
  iOS and Android native code.
- iOS XCFramework build script (`ios/build_rust_ios.sh`) and sample SwiftUI app.
- Android NDK build script (`android/build_rust_android.sh`) and sample Kotlin
  Compose app.
- `examples/basic.rs` — CLI example with `--voice`, `--text`, and `--output`
  flags.

[0.2.5]: https://github.com/eugenehp/kittentts-rs/compare/v0.2.4...v0.2.5
[0.2.4]: https://github.com/eugenehp/kittentts-rs/compare/v0.2.3...v0.2.4
[0.2.3]: https://github.com/eugenehp/kittentts-rs/compare/v0.2.0...v0.2.3
[0.2.0]: https://github.com/eugenehp/kittentts-rs/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/eugenehp/kittentts-rs/releases/tag/v0.1.0
