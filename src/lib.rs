//! # kittentts
//!
//! Rust port of [KittenTTS](https://github.com/KittenML/KittenTTS) —
//! an ultra-lightweight ONNX-based text-to-speech engine.
//!
//! ## Quick start
//!
//! ```no_run
//! use kittentts::{KittenTTS, download};
//!
//! // Download the model from HuggingFace (cached after first run)
//! let tts = download::load_from_hub("KittenML/kitten-tts-mini-0.8").unwrap();
//!
//! // Generate audio samples (Vec<f32>, 24 kHz mono)
//! let audio = tts.generate("Hello from Rust!", "Jasper", 1.0, true).unwrap();
//!
//! // Or write directly to a WAV file
//! tts.generate_to_file(
//!     "Hello from Rust!",
//!     std::path::Path::new("output.wav"),
//!     "Jasper",
//!     1.0,
//!     true,
//! ).unwrap();
//! ```
//!
//! ## Mobile (iOS / Android)
//!
//! Phonemisation uses `libespeak-ng` directly (C library, no subprocess).
//! The same [`generate`](KittenTTS::generate) API works on every platform.
//! The only extra step on mobile is pointing espeak-ng at its bundled data:
//!
//! ```no_run
//! // Call once at app startup, before any TTS call.
//! // Bundle `espeak-ng-data/` with the app and pass its runtime path here.
//! kittentts::phonemize::set_data_path(std::path::Path::new("/data/user/0/com.example/files/espeak-ng-data"));
//! ```
//!
//! You can also skip phonemisation entirely and pass pre-computed IPA:
//!
//! ```no_run
//! use kittentts::{KittenTTS, download};
//!
//! let tts = download::load_from_hub("KittenML/kitten-tts-mini-0.8").unwrap();
//! let audio = tts.generate_from_ipa("həloʊ fɹʌm ɹʌst", "Jasper", 1.0, 20).unwrap();
//! ```
//!
//! ## Build requirements
//! | Platform           | Requirement                                          |
//! |--------------------|------------------------------------------------------|
//! | Alpine / Linux     | `apk add espeak-ng-dev` / `apt install libespeak-ng-dev` |
//! | macOS (Homebrew)   | `brew install espeak-ng`                             |
//! | iOS / Android      | Cross-compiled `libespeak-ng.{a,so}`; set `ESPEAK_LIB_DIR` at build time |
//!
//! ## Pipeline (matches Python implementation)
//! 1. **Text preprocessing** — numbers, currencies, abbreviations → spoken words.
//! 2. **Chunking** — long texts split into ≤ 400-char sentence chunks.
//! 3. **Phonemisation** — `libespeak-ng` converts text to IPA phonemes.
//! 4. **Tokenisation** — IPA characters mapped to integer token IDs.
//! 5. **ONNX inference** — model takes `(input_ids, style, speed)`, outputs audio.
//! 6. **Tail trim** — last 5 000 samples removed (silence artifact).
//! 7. **Concat** — per-chunk audio concatenated into a single waveform.

// Model download from HuggingFace Hub is desktop-only: hf-hub's native-tls
// dependency requires OpenSSL which cannot be cross-compiled for iOS/Android
// without a full SDK.  Mobile apps bundle models via KittenTtsOnnx::load().
#[cfg(not(any(target_os = "ios", target_os = "android")))]
pub mod download;

// C FFI for iOS / Android — exposes kittentts_model_load / synthesize / free.
pub mod ffi;

pub mod model;
pub mod npz;
pub mod phonemize;
pub mod preprocess;
pub mod tokenize;

// ─── Re-exports for convenience ─────────────────────────────────────────────

/// The main TTS model handle — use [`download::load_from_hub`] to obtain one.
pub use model::KittenTtsOnnx as KittenTTS;

/// Audio sample rate produced by the model.
pub use model::SAMPLE_RATE;
