# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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
- espeak-ng subprocess wrapper for IPA phonemisation.
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

[0.2.0]: https://github.com/eugenehp/kittentts-rs/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/eugenehp/kittentts-rs/releases/tag/v0.1.0
