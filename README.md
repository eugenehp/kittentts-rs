# kittentts-rs

A Rust port of [KittenTTS](https://github.com/KittenML/KittenTTS) — an ultra-lightweight, CPU-only text-to-speech engine based on ONNX models.

## Screenshots

| iOS | Android |
|:---:|:---:|
| ![iOS](ios/ios.png) | ![Android](android/android.png) |

## Features

- **ONNX Runtime inference** — uses [`ort`](https://github.com/pykeio/ort) (ORT 2.0 bindings) for fast CPU inference
- **Full text preprocessing** — numbers, currencies, abbreviations, ordinals, units, etc. → spoken words
- **Pure-Rust phonemisation** — IPA output via the [`espeak-ng`](https://crates.io/crates/espeak-ng) crate (no C library, no system dependencies)
- **114 bundled languages** — English and 113 other languages ship as embedded data (no runtime downloads)
- **Same ONNX models** — works with all KittenTTS HuggingFace checkpoints
- **Automatic chunking** — long texts split into ≤ 400-char sentence chunks, then concatenated
- **Cross-platform** — macOS, Linux, Windows, iOS, Android — all from pure Rust
- **Zero native dependencies** — no `cmake`, no `pkg-config`, no `brew install`, no `apt install`

## Prerequisites

**None!** The `espeak` feature uses the pure-Rust [`espeak-ng`](https://crates.io/crates/espeak-ng) crate
with bundled data for all 114 supported languages. No system library installation is required on any platform.

The `espeak` feature is **opt-in**. Without it every API that accepts raw IPA
input still works; only text-to-IPA conversion (and therefore the high-level
`generate` / `generate_to_file` functions) is unavailable.

## Installation

```toml
# Cargo.toml

# Without espeak (IPA input only)
[dependencies]
kittentts = "0.3.0"

# With espeak (full text input — pure Rust, no system deps)
[dependencies]
kittentts = { version = "0.3.0", features = ["espeak"] }
```

Or add it with cargo:

```sh
cargo add kittentts
cargo add kittentts --features espeak
```

### From GitHub

```toml
[dependencies]
kittentts = { git = "https://github.com/eugenehp/kittentts-rs", tag = "v0.3.0" }
kittentts = { git = "https://github.com/eugenehp/kittentts-rs", branch = "main" }
```

## Quick Start

```rust
use kittentts::download;
use std::path::Path;

fn main() -> anyhow::Result<()> {
    // Downloads model from HuggingFace (cached after first run)
    let tts = download::load_from_hub("KittenML/kitten-tts-mini-0.8")?;

    println!("Available voices: {:?}", tts.available_voices);

    // Generate and save as WAV (24 kHz, float32, mono)
    tts.generate_to_file(
        "Hello from Rust! This high quality TTS model works without a GPU.",
        Path::new("output.wav"),
        "Jasper",
        1.0,   // speed (1.0 = normal)
        true,  // clean_text: run number/abbreviation expansion
    )?;

    Ok(())
}
```

Run the bundled example:

```sh
cargo run --example basic --features espeak
cargo run --example basic --features espeak -- --voice Luna --text "Hello world" --output hello.wav
```

## Available Models

| Model | Params | Size |
|---|---|---|
| `KittenML/kitten-tts-mini-0.8` | 80M | 80 MB |
| `KittenML/kitten-tts-micro-0.8` | 40M | 41 MB |
| `KittenML/kitten-tts-nano-0.8-fp32` | 15M | 56 MB |
| `KittenML/kitten-tts-nano-0.8-int8` | 15M | 25 MB |

## Available Voices (v0.8)

`Bella`, `Jasper`, `Luna`, `Bruno`, `Rosie`, `Hugo`, `Kiki`, `Leo`

## Bundled Languages (114)

The `espeak` feature bundles phoneme data for all 114 espeak-ng languages:

`af`, `am`, `an`, `ar`, `as`, `az`, `ba`, `be`, `bg`, `bn`, `bpy`, `bs`, `ca`, `chr`, `cmn`, `cs`, `cv`, `cy`, `da`, `de`, `el`, `en`, `eo`, `es`, `et`, `eu`, `fa`, `fi`, `fr`, `ga`, `gd`, `gn`, `grc`, `gu`, `hak`, `haw`, `he`, `hi`, `hr`, `ht`, `hu`, `hy`, `ia`, `id`, `io`, `is`, `it`, `ja`, `jbo`, `ka`, `kk`, `kl`, `kn`, `ko`, `kok`, `ku`, `ky`, `la`, `lb`, `lfn`, `lt`, `lv`, `mi`, `mk`, `ml`, `mr`, `ms`, `mt`, `mto`, `my`, `nci`, `ne`, `nl`, `no`, `nog`, `om`, `or`, `pa`, `pap`, `piqd`, `pl`, `pt`, `py`, `qdb`, `qu`, `quc`, `qya`, `ro`, `ru`, `sd`, `shn`, `si`, `sjn`, `sk`, `sl`, `smj`, `sq`, `sr`, `sv`, `sw`, `ta`, `te`, `th`, `ti`, `tk`, `tn`, `tr`, `tt`, `ug`, `uk`, `ur`, `uz`, `vi`, `yue`

> **Note**: 4 languages (`bs`, `io`, `lfn`, `pap`) have missing phoneme tables in `espeak-ng` 0.1.0.
> 17 languages with non-Latin scripts may return empty IPA for some inputs (upstream limitation).

## API

```rust
// Load from HuggingFace Hub
let tts = kittentts::download::load_from_hub("KittenML/kitten-tts-mini-0.8")?;

// Load from local files
let tts = kittentts::model::KittenTtsOnnx::load(
    Path::new("model.onnx"),
    Path::new("voices.npz"),
    Default::default(), // speed_priors
    Default::default(), // voice_aliases
)?;

// Generate audio → Vec<f32> at 24 kHz
let audio: Vec<f32> = tts.generate("Hello!", "Jasper", 1.0, true)?;

// Generate and save to WAV
tts.generate_to_file("Hello!", Path::new("out.wav"), "Jasper", 1.0, true)?;

// Generate from pre-computed IPA (no espeak feature needed)
let audio = tts.generate_from_ipa("həloʊ", "Jasper", 1.0, 5)?;

// Available voices
println!("{:?}", tts.available_voices);
```

## Cross-Platform Build

Since phonemisation is now pure Rust, cross-compilation is straightforward:

```sh
# iOS
rustup target add aarch64-apple-ios
cargo build --target aarch64-apple-ios --features espeak

# Android
rustup target add aarch64-linux-android
cargo build --target aarch64-linux-android --features espeak

# Linux aarch64
rustup target add aarch64-unknown-linux-gnu
cargo build --target aarch64-unknown-linux-gnu --features espeak

# Windows (from any host)
rustup target add x86_64-pc-windows-gnu
cargo build --target x86_64-pc-windows-gnu --features espeak
```

No `ESPEAK_LIB_DIR`, no sysroot, no cross-compiled C library needed.

### iOS

```sh
bash ios/build_rust_ios.sh
```

### Android

```sh
export ANDROID_NDK_HOME=/path/to/ndk
bash android/build_rust_android.sh
```

### With `cargo cross`

```sh
cargo install cross --git https://github.com/cross-rs/cross
cross build --target aarch64-unknown-linux-gnu --features espeak
cross build --target x86_64-unknown-linux-musl --features espeak
```

## Architecture

```
Input text
    ↓  TextPreprocessor  (preprocess.rs)
       • numbers / currency / percentages / ordinals → words
       • contractions, units, scientific notation, fractions, …
    ↓  chunk_text()  (model.rs)
       • split into ≤ 400-char sentence chunks
    ↓  espeak-ng (pure Rust)  (phonemize.rs)
       • text → IPA phoneme string (en, with stress)
       • requires `espeak` feature
    ↓  ipa_to_ids()  (tokenize.rs)
       • IPA chars → integer token IDs  (fixed vocab, same as Python)
       • prepend/append pad token 0
    ↓  ONNX Runtime inference  (model.rs)
       • inputs:  input_ids [1, T], style [1, D], speed [1]
       • output:  audio waveform [samples]
    ↓  tail-trim (–2 000 samples) + chunk concatenation
    ↓  Vec<f32> @ 24 kHz  or  WAV file
```

## Crate Structure

| File | Role |
|---|---|
| `src/lib.rs` | Public API & re-exports |
| `src/preprocess.rs` | Text preprocessing pipeline |
| `src/phonemize.rs` | Pure-Rust espeak-ng phonemisation (bundled data, no C FFI) |
| `src/tokenize.rs` | IPA character → token ID |
| `src/npz.rs` | Hand-written NPY/NPZ loader |
| `src/model.rs` | ONNX inference, chunking, WAV output |
| `src/download.rs` | HuggingFace Hub model download |
| `src/ffi.rs` | C FFI layer for iOS/Android |
| `build.rs` | Build script (minimal — no native library linking needed) |
| `tests/integration_tests.rs` | Integration & e2e test suite (40 tests, model-file based) |
| `ios/build_rust_ios.sh` | Full iOS XCFramework build (device + simulator) |
| `android/build_rust_android.sh` | Full Android arm64 build (JNI bridge) |
| `examples/basic.rs` | CLI example |

## Running Tests

```sh
# All unit tests (no espeak — 20 tests)
cargo test

# All unit + phonemisation tests (29 tests, including 114-language coverage)
cargo test --features espeak

# Integration and e2e tests using bundled model files (32 tests)
cargo test --test integration_tests

# Integration + espeak + full inference e2e tests (40 tests)
cargo test --test integration_tests --features espeak

# Full test suite (72 tests)
cargo test --features espeak

# Point at a custom model directory
KITTENTTS_MODEL_DIR=/path/to/models cargo test --test integration_tests
```

### Test counts at a glance

| Suite | `--features espeak` | Tests |
|---|:---:|---|
| Unit tests (`src/**`) | no | 20 |
| Unit tests (`src/**`) | yes | 29 |
| Integration tests | no | 32 |
| Integration tests | yes | 40 |
| Doc-tests | — | 3 |
| **Total** | **yes** | **72** |

## Migration from C `libespeak-ng`

This crate previously used C FFI bindings to `libespeak-ng` with a 1200-line
`build.rs` for native library detection and cross-compilation. It now uses the
pure-Rust [`espeak-ng`](https://crates.io/crates/espeak-ng) crate instead:

- **No system library required** — `brew install espeak-ng` / `apt install libespeak-ng-dev` no longer needed
- **No C compiler needed** — no `cmake`, no `gcc`, no build scripts
- **No unsafe code** in phonemisation — the entire FFI layer was removed
- **build.rs reduced from 1200 lines to 8** — no pkg-config, no platform path walk, no Windows auto-build
- **Cross-compilation just works** — no `ESPEAK_LIB_DIR`, no `ESPEAK_SYSROOT`, no NDK toolchain setup

## Citation

```bibtex
@software{kittentts_rs_2026,
  author    = {Eugene Hauptmann},
  title     = {kittentts-rs: A Rust Port of KittenTTS},
  year      = {2026},
  url       = {https://github.com/eugenehp/kittentts-rs},
  note      = {Ultra-lightweight, CPU-only text-to-speech engine based on ONNX models}
}
```

```bibtex
@software{kittentts_2024,
  author    = {KittenML},
  title     = {KittenTTS},
  year      = {2024},
  url       = {https://github.com/eugenehp/KittenTTS}
}
```

## Changelog

See [CHANGELOG.md](CHANGELOG.md) for a full history of releases and changes.

## License

This project is licensed under the [Apache License 2.0](LICENSE).
