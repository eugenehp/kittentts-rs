//! ONNX model runner — mirrors Python's `KittenTTS_1_Onnx`.
//!
//! Uses [`ort`] (ONNX Runtime Rust bindings) for inference.
//! The three model inputs are:
//!
//! | Name        | Shape         | dtype   |
//! |-------------|---------------|---------|
//! | `input_ids` | `[1, seq_len]`| int64   |
//! | `style`     | `[1, style_d]`| float32 |
//! | `speed`     | `[1]`         | float32 |

use std::{collections::HashMap, path::Path, sync::Mutex};

use anyhow::{Context, Result};
use ort::{session::Session, value::Tensor};

use crate::{
    npz::{load_npz, NpyArray},
    phonemize::phonemize,
    preprocess::TextPreprocessor,
    tokenize::ipa_to_ids,
};

/// Samples trimmed from the tail of every generated waveform (matches Python).
const TAIL_TRIM: usize = 5_000;

/// Audio sample rate produced by the model.
pub const SAMPLE_RATE: u32 = 24_000;

/// Maximum characters per text chunk before splitting.
const CHUNK_MAX_CHARS: usize = 400;

// ─────────────────────────────────────────────────────────────────────────────
// Text chunker (mirrors `chunk_text` in onnx_model.py)
// ─────────────────────────────────────────────────────────────────────────────

fn ensure_punctuation(text: &str) -> String {
    let text = text.trim();
    if text.is_empty() {
        return text.to_string();
    }
    match text.chars().last() {
        Some(c) if ".!?,;:".contains(c) => text.to_string(),
        _ => format!("{},", text),
    }
}

fn chunk_text(text: &str, max_len: usize) -> Vec<String> {
    let mut chunks = Vec::new();
    for sentence in text.split_terminator(['.', '!', '?']) {
        let sentence = sentence.trim();
        if sentence.is_empty() {
            continue;
        }
        if sentence.len() <= max_len {
            chunks.push(ensure_punctuation(sentence));
        } else {
            let mut current = String::new();
            for word in sentence.split_whitespace() {
                if !current.is_empty() && current.len() + 1 + word.len() > max_len {
                    chunks.push(ensure_punctuation(current.trim()));
                    current = word.to_string();
                } else {
                    if !current.is_empty() {
                        current.push(' ');
                    }
                    current.push_str(word);
                }
            }
            if !current.trim().is_empty() {
                chunks.push(ensure_punctuation(current.trim()));
            }
        }
    }
    chunks
}

// ─────────────────────────────────────────────────────────────────────────────
// Voice embedding store
// ─────────────────────────────────────────────────────────────────────────────

struct Voice {
    nrows: usize,
    ncols: usize,
    data: Vec<f32>, // flat, row-major
}

impl Voice {
    fn from_npy(arr: NpyArray) -> Self {
        Self { nrows: arr.nrows(), ncols: arr.ncols(), data: arr.data }
    }

    /// Row at `text_len`, clamped to valid range.
    fn style_row(&self, text_len: usize) -> &[f32] {
        let i = text_len.min(self.nrows.saturating_sub(1));
        &self.data[i * self.ncols..(i + 1) * self.ncols]
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// KittenTtsOnnx
// ─────────────────────────────────────────────────────────────────────────────

/// The main TTS model handle.
pub struct KittenTtsOnnx {
    session: Mutex<Session>,
    voices: HashMap<String, Voice>,
    speed_priors: HashMap<String, f32>,
    voice_aliases: HashMap<String, String>,
    preprocessor: TextPreprocessor,
    pub available_voices: Vec<String>,
}

impl KittenTtsOnnx {
    /// Load the model from an ONNX file and a voices NPZ file.
    pub fn load(
        model_path: &Path,
        voices_path: &Path,
        speed_priors: HashMap<String, f32>,
        voice_aliases: HashMap<String, String>,
    ) -> Result<Self> {
        // ── Load ONNX model with ONNX Runtime ───────────────────────────────
        let session = Session::builder()
            .context("Failed to create ORT session builder")?
            .commit_from_file(model_path)
            .with_context(|| format!("Cannot load ONNX model: {}", model_path.display()))?;

        // ── Voice embeddings ─────────────────────────────────────────────────
        let raw = load_npz(voices_path)
            .with_context(|| format!("Cannot load voices: {}", voices_path.display()))?;

        let available_voices: Vec<String> = raw.keys().cloned().collect();
        let voices: HashMap<String, Voice> =
            raw.into_iter().map(|(k, v)| (k, Voice::from_npy(v))).collect();

        Ok(Self {
            session: Mutex::new(session),
            voices,
            speed_priors,
            voice_aliases,
            preprocessor: TextPreprocessor::new(),
            available_voices,
        })
    }

    // ── Helpers ───────────────────────────────────────────────────────────────

    fn resolve_voice<'a>(&'a self, voice: &'a str) -> &'a str {
        self.voice_aliases.get(voice).map(String::as_str).unwrap_or(voice)
    }

    /// Core inference step: IPA string → audio samples.
    ///
    /// `style_idx` selects which row of the voice style matrix to use.
    /// Pass `text.len()` when the caller has the original text, or `ipa.len()`
    /// when only the IPA is available — both are clamped to the matrix bounds.
    fn infer_ipa(
        &self,
        ipa: &str,
        style_idx: usize,
        voice_key: &str,
        effective_speed: f32,
    ) -> Result<Vec<f32>> {
        let voice_data = self.voices.get(voice_key).with_context(|| {
            format!("Voice '{}' not found. Available: {:?}", voice_key, self.available_voices)
        })?;

        // ── Tokenise → [0, tok…, 0] ──────────────────────────────────────────
        let ids = ipa_to_ids(ipa);
        let seq_len = ids.len();

        // ── Style vector ──────────────────────────────────────────────────────
        let style_slice = voice_data.style_row(style_idx);
        let style_dim = style_slice.len();

        // ── Build ORT tensors ─────────────────────────────────────────────────
        //
        // Inputs are positional (matching the ONNX graph input order):
        //   0 → input_ids  [1, seq_len]  i64
        //   1 → style      [1, style_d]  f32
        //   2 → speed      [1]           f32

        let t_input_ids = Tensor::<i64>::from_array(([1usize, seq_len], ids))
            .context("Failed to build input_ids tensor")?;

        let t_style = Tensor::<f32>::from_array(([1usize, style_dim], style_slice.to_vec()))
            .context("Failed to build style tensor")?;

        let t_speed = Tensor::<f32>::from_array(([1usize], vec![effective_speed]))
            .context("Failed to build speed tensor")?;

        // ── Inference ─────────────────────────────────────────────────────────
        let mut session = self.session.lock().expect("ORT session mutex poisoned");
        let outputs = session
            .run(ort::inputs![t_input_ids, t_style, t_speed])
            .context("ONNX inference failed")?;

        // Output 0 is the raw waveform (shape e.g. [1, T] or [T]).
        let (_shape, audio_data) = outputs[0]
            .try_extract_tensor::<f32>()
            .context("Failed to extract audio tensor")?;

        let audio_flat: Vec<f32> = audio_data.to_vec();

        // Trim trailing silence (matches Python `audio[..., :-5000]`)
        let trimmed_len = audio_flat.len().saturating_sub(TAIL_TRIM);
        Ok(audio_flat[..trimmed_len].to_vec())
    }

    // ── Text → audio ──────────────────────────────────────────────────────────

    /// Phonemise `text` with espeak-ng (via C library) and run inference.
    pub fn generate_chunk(&self, text: &str, voice: &str, speed: f32) -> Result<Vec<f32>> {
        let voice_key = self.resolve_voice(voice);
        let effective_speed = speed * self.speed_priors.get(voice_key).copied().unwrap_or(1.0);

        let ipa = phonemize(text)
            .with_context(|| format!("Phonemisation failed for {:?}", text))?;

        self.infer_ipa(&ipa, text.len(), voice_key, effective_speed)
    }

    // ── IPA → audio (all platforms) ───────────────────────────────────────────

    /// Run inference directly from a pre-computed IPA phoneme string.
    ///
    /// Use this when you have already obtained IPA from another source:
    /// - a server round-trip (`POST /phonemize` → IPA string)
    /// - a pre-baked lookup table for known phrases
    /// - a different G2P library
    ///
    /// The IPA must use the same character set as espeak-ng's `en-us` backend
    /// (the same set this model was trained on).  Unknown characters are silently
    /// skipped by the tokeniser.
    ///
    /// `style_idx` selects which style-embedding row to use.  Pass the byte length
    /// of the original cleaned text if you have it, or `ipa.len()` otherwise —
    /// it is clamped to the valid range internally.
    pub fn generate_from_ipa(
        &self,
        ipa: &str,
        voice: &str,
        speed: f32,
        style_idx: usize,
    ) -> Result<Vec<f32>> {
        let voice_key = self.resolve_voice(voice);
        let effective_speed = speed * self.speed_priors.get(voice_key).copied().unwrap_or(1.0);
        self.infer_ipa(ipa, style_idx, voice_key, effective_speed)
    }

    /// Run inference on multiple pre-phonemized IPA chunks and concatenate.
    ///
    /// Mirrors [`generate`] but accepts IPA strings instead of raw text.
    /// Each element of `chunks` is one IPA string (typically one sentence).
    pub fn generate_from_ipa_chunks(
        &self,
        chunks: &[&str],
        voice: &str,
        speed: f32,
    ) -> Result<Vec<f32>> {
        let voice_key = self.resolve_voice(voice);
        if !self.voices.contains_key(voice_key) {
            anyhow::bail!(
                "Unknown voice '{}'. Available: {:?}",
                voice,
                self.available_voices
            );
        }
        let mut audio = Vec::new();
        for &ipa in chunks {
            audio.extend(self.generate_from_ipa(ipa, voice, speed, ipa.len())?);
        }
        Ok(audio)
    }

    /// Run inference from an IPA string and write a 32-bit float WAV file.
    ///
    /// Convenience wrapper around [`generate_from_ipa`] for the common case of
    /// a single pre-phonemized utterance on mobile.
    pub fn generate_to_file_from_ipa(
        &self,
        ipa: &str,
        output_path: &Path,
        voice: &str,
        speed: f32,
        style_idx: usize,
    ) -> Result<()> {
        let audio = self.generate_from_ipa(ipa, voice, speed, style_idx)?;
        self.write_wav(&audio, output_path)
    }

    // ── WAV writer ────────────────────────────────────────────────────────────

    /// Write `audio` samples to a 16-bit PCM WAV file at [`SAMPLE_RATE`] Hz.
    ///
    /// 16-bit PCM is chosen over 32-bit float because Android's `MediaPlayer`
    /// does not reliably decode IEEE-float WAV (it accepts the file header but
    /// produces silence at runtime).  All Android API levels and the emulator
    /// support PCM 16-bit without issue.
    pub fn write_wav(&self, audio: &[f32], output_path: &Path) -> Result<()> {
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: SAMPLE_RATE,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(output_path, spec)
            .with_context(|| format!("Cannot create WAV: {}", output_path.display()))?;
        for &s in audio {
            // Convert f32 [-1.0, 1.0] → i16 [-32768, 32767].
            let s16 = (s * i16::MAX as f32).clamp(i16::MIN as f32, i16::MAX as f32) as i16;
            writer.write_sample(s16).context("WAV write error")?;
        }
        writer.finalize().context("WAV finalise error")?;
        println!("Saved {} samples ({} s) to {}", audio.len(),
            audio.len() as f32 / SAMPLE_RATE as f32, output_path.display());
        Ok(())
    }

    // ── Text → audio (desktop only) ───────────────────────────────────────────

    /// Generate audio for `text`, splitting into sentence-level chunks.
    ///
    /// Returns a flat `Vec<f32>` at [`SAMPLE_RATE`] Hz (24 kHz).
    pub fn generate(
        &self,
        text: &str,
        voice: &str,
        speed: f32,
        clean_text: bool,
    ) -> Result<Vec<f32>> {
        let voice_key = self.resolve_voice(voice);
        if !self.voices.contains_key(voice_key) {
            anyhow::bail!(
                "Unknown voice '{}'. Available: {:?}",
                voice,
                self.available_voices
            );
        }

        let processed = if clean_text {
            self.preprocessor.process(text)
        } else {
            text.to_string()
        };

        let chunks = chunk_text(&processed, CHUNK_MAX_CHARS);
        if chunks.is_empty() {
            return Ok(Vec::new());
        }

        let mut audio = Vec::new();
        for chunk in &chunks {
            audio.extend(self.generate_chunk(chunk, voice, speed)?);
        }
        Ok(audio)
    }

    /// Generate audio from `text` and save it to a 32-bit float WAV file.
    pub fn generate_to_file(
        &self,
        text: &str,
        output_path: &Path,
        voice: &str,
        speed: f32,
        clean_text: bool,
    ) -> Result<()> {
        let audio = self.generate(text, voice, speed, clean_text)?;
        self.write_wav(&audio, output_path)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chunk_short() {
        // split_terminator('.') consumes the period; ensure_punctuation adds ','
        // (same behaviour as Python's re.split(r'[.!?]+', text) + ensure_punctuation)
        let c = chunk_text("Hello world.", 400);
        assert_eq!(c, vec!["Hello world,"]);
    }

    #[test]
    fn test_chunk_multiple_sentences() {
        let c = chunk_text("Hello. World. Foo.", 400);
        assert_eq!(c.len(), 3);
    }

    #[test]
    fn test_chunk_long_sentence() {
        let long = "word ".repeat(200);
        let c = chunk_text(long.trim(), 400);
        assert!(c.len() > 1);
        for chunk in &c {
            assert!(chunk.len() <= 405);
        }
    }

    #[test]
    fn test_ensure_punctuation() {
        assert_eq!(ensure_punctuation("hello"), "hello,");
        assert_eq!(ensure_punctuation("hello."), "hello.");
        assert_eq!(ensure_punctuation(""), "");
    }
}
