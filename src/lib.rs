//! # kittentts
//!
//! Rust port of [KittenTTS](https://github.com/KittenML/KittenTTS) —
//! an ultra-lightweight ONNX-based text-to-speech engine.
//!
//! ## Quick start
//!
//! ```no_run
//! # // generate / generate_to_file require the `espeak` feature.
//! # // Hidden cfg guards keep this example compiling in both configurations.
//! # #[cfg(not(feature = "espeak"))] fn main() {}
//! # #[cfg(feature = "espeak")] fn main() {
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
//! # }
//! ```
//!
//! ## Mobile (iOS / Android)
//!
//! Phonemisation uses the pure-Rust `espeak-ng` crate with bundled data.
//! The same [`generate`](KittenTTS::generate) API works on every platform
//! with no extra setup — no C library, no system dependencies.
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
//! | All platforms      | None — the `espeak-ng` crate is pure Rust with bundled data |
//!
//! ## Pipeline (matches Python implementation)
//! 1. **Text preprocessing** — numbers, currencies, abbreviations → spoken words.
//! 2. **Chunking** — long texts split into ≤ 400-char sentence chunks.
//! 3. **Phonemisation** — pure-Rust `espeak-ng` converts text to IPA phonemes.
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

// Streaming support
pub mod encoding;
pub mod streaming;

// Re-exports for convenience
pub use encoding::{AudioEncoder, AudioFormat, EncoderFactory};

// ─── Re-exports for convenience ─────────────────────────────────────────────

/// The main TTS model handle — use [`download::load_from_hub`] to obtain one.
pub use model::KittenTtsOnnx as KittenTTS;

/// Audio sample rate produced by the model.
pub use model::SAMPLE_RATE;
