//! Integration tests for kittentts — exercises the public API end-to-end.
//!
//! Model files are loaded from the bundled iOS app models directory so no
//! network access is required.  The ONNX model path can be overridden via
//! the `KITTENTTS_MODEL_DIR` environment variable.
//!
//! Run with:
//!   cargo test                              # pure-Rust modules
//!   cargo test --features espeak            # + phonemisation tests
//!   KITTENTTS_MODEL_DIR=… cargo test        # + inference tests

use std::path::{Path, PathBuf};

// ── Helper: locate model directory ───────────────────────────────────────────

/// Return the path to a bundled model directory that contains:
///   - `kitten_tts_mini_v0_8.onnx`
///   - `voices.npz`
///   - `config.json`
///
/// Search order:
///   1. `$KITTENTTS_MODEL_DIR` environment variable
///   2. `ios/KittenTTSApp/KittenTTSApp/Models/` relative to workspace root
///   3. `android/KittenTTSApp/app/src/main/assets/models/` relative to workspace root
fn model_dir() -> Option<PathBuf> {
    // 1. Explicit override
    if let Ok(dir) = std::env::var("KITTENTTS_MODEL_DIR") {
        let p = PathBuf::from(dir);
        if p.join("kitten_tts_mini_v0_8.onnx").exists() {
            return Some(p);
        }
    }

    // 2. Workspace-relative paths
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let candidates = [
        manifest.join("ios/KittenTTSApp/KittenTTSApp/Models"),
        manifest.join("android/KittenTTSApp/app/src/main/assets/models"),
    ];
    candidates
        .iter()
        .find(|p| p.join("kitten_tts_mini_v0_8.onnx").exists())
        .cloned()
}

// ─────────────────────────────────────────────────────────────────────────────
// § tokenize
// ─────────────────────────────────────────────────────────────────────────────

mod tokenize {
    use kittentts::tokenize::{basic_english_tokenize, char_to_id, ipa_to_ids};

    #[test]
    fn vocab_is_loaded() {
        assert!(char_to_id('$').is_some(), "pad token '$' must be in vocab");
        assert_eq!(char_to_id('$'), Some(0), "pad must map to index 0");
    }

    #[test]
    fn all_ascii_letters_present() {
        for c in b'A'..=b'Z' {
            let ch = c as char;
            assert!(char_to_id(ch).is_some(), "uppercase {ch} missing from vocab");
        }
        for c in b'a'..=b'z' {
            let ch = c as char;
            assert!(char_to_id(ch).is_some(), "lowercase {ch} missing from vocab");
        }
    }

    #[test]
    fn ipa_to_ids_wraps_with_pads() {
        let ids = ipa_to_ids("hɛloʊ");
        assert_eq!(ids[0], 0, "first element must be pad (0)");
        assert_eq!(*ids.last().unwrap(), 0, "last element must be pad (0)");
        assert!(ids.len() >= 3, "must have at least start pad + 1 token + end pad");
    }

    #[test]
    fn ipa_to_ids_empty_string() {
        let ids = ipa_to_ids("");
        // Only the two pad tokens.
        assert_eq!(ids, vec![0, 0]);
    }

    #[test]
    fn tokenize_splits_words_and_punctuation() {
        let out = basic_english_tokenize("hello,world!");
        // Punctuation should be separated from words.
        assert!(out.contains("hello"), "word 'hello' should appear: {out}");
        assert!(out.contains(','), "comma should appear: {out}");
        assert!(out.contains("world"), "word 'world' should appear: {out}");
        assert!(out.contains('!'), "exclamation should appear: {out}");
    }

    #[test]
    fn ids_are_non_negative() {
        let ids = ipa_to_ids("ɑɐɒæɓʙβɔ");
        assert!(ids.iter().all(|&id| id >= 0), "all IDs must be non-negative");
    }

    #[test]
    fn unknown_chars_skipped() {
        // Chinese characters are not in the vocabulary.
        let ids_with_unknown = ipa_to_ids("你好");
        let ids_empty = ipa_to_ids("");
        // Only the two pad tokens should survive (unknown chars skipped).
        assert_eq!(ids_with_unknown, ids_empty);
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// § preprocess
// ─────────────────────────────────────────────────────────────────────────────

mod preprocess {
    use kittentts::preprocess::{
        TextPreprocessor, expand_contractions, expand_currency, expand_ordinals,
        expand_percentages, expand_units, number_to_words, float_to_words,
        remove_extra_whitespace,
    };

    #[test]
    fn number_words_zero_to_twenty() {
        assert_eq!(number_to_words(0), "zero");
        assert_eq!(number_to_words(1), "one");
        assert_eq!(number_to_words(11), "eleven");
        assert_eq!(number_to_words(19), "nineteen");
        assert_eq!(number_to_words(20), "twenty");
    }

    #[test]
    fn number_words_large() {
        assert_eq!(number_to_words(1_000), "one thousand");
        assert_eq!(number_to_words(1_000_000), "one million");
        assert_eq!(number_to_words(1_200), "twelve hundred");
    }

    #[test]
    fn number_words_negative() {
        assert_eq!(number_to_words(-5), "negative five");
        assert_eq!(number_to_words(-42), "negative forty-two");
    }

    #[test]
    fn float_words_decimal() {
        assert_eq!(float_to_words("3.14"), "three point one four");
        assert_eq!(float_to_words("-0.5"), "negative zero point five");
    }

    #[test]
    fn ordinals_expanded() {
        let out = expand_ordinals("1st place, 2nd runner, 3rd prize");
        assert!(out.contains("first"), "1st → first: {out}");
        assert!(out.contains("second"), "2nd → second: {out}");
        assert!(out.contains("third"), "3rd → third: {out}");
    }

    #[test]
    fn percentages_expanded() {
        let out = expand_percentages("50% and 3.5%");
        assert!(out.contains("fifty percent"), "50% → fifty percent: {out}");
        assert!(out.contains("percent"), "3.5% → … percent: {out}");
    }

    #[test]
    fn currency_dollar() {
        let out = expand_currency("$9.99");
        assert!(out.contains("nine dollar"), "got: {out}");
        assert!(out.contains("cent"), "got: {out}");
    }

    #[test]
    fn currency_large_scale() {
        let out = expand_currency("$1B");
        assert!(out.contains("one billion"), "got: {out}");
        assert!(out.contains("dollar"), "got: {out}");
    }

    #[test]
    fn contractions_expanded() {
        assert!(expand_contractions("can't").contains("cannot"));
        assert!(expand_contractions("won't").contains("will not"));
        assert!(expand_contractions("I'm").contains("I am"));
        assert!(expand_contractions("they've").contains("they have"));
    }

    #[test]
    fn units_expanded() {
        let out = expand_units("100 km and 5 kg");
        assert!(out.contains("kilometers"), "got: {out}");
        assert!(out.contains("kilograms"), "got: {out}");
    }

    #[test]
    fn whitespace_normalised() {
        assert_eq!(remove_extra_whitespace("  hello   world  "), "hello world");
    }

    #[test]
    fn full_pipeline_lowercase_no_punctuation() {
        let pp = TextPreprocessor::new();
        let out = pp.process("Hello, World! 42% at $3.50.");
        assert!(out.chars().all(|c| c.is_lowercase() || c == ' '),
            "output should be all-lowercase no-punctuation: {out}");
        assert!(!out.contains(','), "no commas: {out}");
    }

    #[test]
    fn pipeline_expands_numbers_and_currency() {
        let pp = TextPreprocessor::new();
        let out = pp.process("She earned $1,200 last month.");
        assert!(out.contains("twelve hundred dollar"), "got: {out}");
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// § npz
// ─────────────────────────────────────────────────────────────────────────────

mod npz {
    use kittentts::npz::{NpyArray, load_npz, parse_npy};

    /// Build a minimal v1.0 NPY byte buffer for unit testing.
    fn make_npy(shape: &[usize], values: &[f32]) -> Vec<u8> {
        let shape_str = shape
            .iter()
            .map(|d| d.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        let header_dict = format!(
            "{{'descr': '<f4', 'fortran_order': False, 'shape': ({},), }}",
            shape_str
        );
        let raw_len = header_dict.len() + 1; // +1 for trailing \n
        let padded_len = ((raw_len + 63) / 64) * 64;
        let pad_spaces = padded_len - raw_len;
        let mut header = header_dict;
        header.extend(std::iter::repeat(' ').take(pad_spaces));
        header.push('\n');

        let mut buf: Vec<u8> = Vec::new();
        buf.extend_from_slice(b"\x93NUMPY");
        buf.push(1);
        buf.push(0);
        buf.extend_from_slice(&(header.len() as u16).to_le_bytes());
        buf.extend_from_slice(header.as_bytes());
        for &v in values {
            buf.extend_from_slice(&v.to_le_bytes());
        }
        buf
    }

    #[test]
    fn parse_npy_1d_roundtrip() {
        let values = vec![1.0f32, 2.0, 3.0];
        let buf = make_npy(&[3], &values);
        let (shape, data) = parse_npy(&buf).unwrap();
        assert_eq!(shape, vec![3]);
        assert_eq!(data, values);
    }

    #[test]
    fn parse_npy_2d_roundtrip() {
        let values: Vec<f32> = (0..12).map(|x| x as f32).collect();
        let buf = make_npy(&[3, 4], &values);
        let (shape, data) = parse_npy(&buf).unwrap();
        assert_eq!(shape, vec![3, 4]);
        assert_eq!(data, values);
    }

    #[test]
    fn npy_array_row_access() {
        let values: Vec<f32> = (0..6).map(|x| x as f32).collect();
        let buf = make_npy(&[2, 3], &values);
        let (shape, data) = parse_npy(&buf).unwrap();
        let arr = NpyArray { shape, data };
        assert_eq!(arr.row(0), &[0.0f32, 1.0, 2.0]);
        assert_eq!(arr.row(1), &[3.0f32, 4.0, 5.0]);
        assert_eq!(arr.nrows(), 2);
        assert_eq!(arr.ncols(), 3);
    }

    #[test]
    fn parse_npy_bad_magic_errors() {
        let result = parse_npy(b"NOT_A_NUMPY_FILE");
        assert!(result.is_err(), "should error on invalid magic");
    }

    #[test]
    fn parse_npy_truncated_errors() {
        let result = parse_npy(b"\x93NUMPY\x01\x00");
        assert!(result.is_err(), "should error on truncated file");
    }

    #[test]
    fn load_npz_bundled_voices() {
        let Some(model_dir) = super::model_dir() else {
            eprintln!("SKIP load_npz_bundled_voices: model directory not found");
            return;
        };
        let voices_path = model_dir.join("voices.npz");
        let arrays = load_npz(&voices_path).expect("failed to load voices.npz");
        assert!(!arrays.is_empty(), "voices.npz should contain at least one array");

        // Every array must have exactly 2 dimensions (voices × style_dim).
        for (name, arr) in &arrays {
            assert_eq!(arr.shape.len(), 2,
                "array '{}' should be 2-D, got shape {:?}", name, arr.shape);
            assert!(arr.nrows() > 0, "array '{}' must have rows", name);
            assert!(arr.ncols() > 0, "array '{}' must have columns", name);
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// § phonemize (requires `espeak` feature)
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(feature = "espeak")]
mod phonemize {
    use kittentts::phonemize::{is_espeak_available, phonemize};

    #[test]
    fn espeak_available() {
        assert!(is_espeak_available(), "espeak-ng must initialise successfully");
    }

    #[test]
    fn hello_world_produces_ipa() {
        let ipa = phonemize("Hello, world!").expect("phonemize failed");
        assert!(!ipa.is_empty(), "IPA for 'Hello, world!' must not be empty");
        // IPA for 'hello' contains at minimum 'h' or some vowel
        assert!(
            ipa.contains('h') || ipa.contains('ɛ') || ipa.contains('l'),
            "unexpected IPA for 'Hello, world!': {ipa}"
        );
    }

    #[test]
    fn empty_input_gives_empty_ipa() {
        let ipa = phonemize("").expect("phonemize of empty should succeed");
        assert!(ipa.trim().is_empty(), "empty input → empty IPA, got: {ipa}");
    }

    #[test]
    fn numbers_are_phonemised() {
        let ipa = phonemize("123").expect("phonemize failed");
        assert!(!ipa.is_empty(), "123 should produce IPA");
    }

    #[test]
    fn long_text_phonemised() {
        let text = "The quick brown fox jumps over the lazy dog.";
        let ipa = phonemize(text).expect("phonemize failed");
        assert!(!ipa.is_empty(), "long text should produce non-empty IPA");
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// § model inference (e2e)
// ─────────────────────────────────────────────────────────────────────────────

mod model {
    use kittentts::model::{KittenTtsOnnx, SAMPLE_RATE};
    use std::collections::HashMap;

    fn load_bundled_model() -> Option<KittenTtsOnnx> {
        let model_dir = super::model_dir()?;
        let onnx  = model_dir.join("kitten_tts_mini_v0_8.onnx");
        let voices = model_dir.join("voices.npz");
        if !onnx.exists() || !voices.exists() {
            return None;
        }
        KittenTtsOnnx::load(
            &onnx,
            &voices,
            HashMap::new(),
            HashMap::new(),
        ).ok()
    }

    #[test]
    fn sample_rate_is_24k() {
        assert_eq!(SAMPLE_RATE, 24_000, "model should produce 24 kHz audio");
    }

    #[test]
    fn model_loads_and_lists_voices() {
        let Some(tts) = load_bundled_model() else {
            eprintln!("SKIP model_loads_and_lists_voices: model files not found");
            return;
        };
        assert!(!tts.available_voices.is_empty(), "model should expose at least one voice");
        eprintln!("Available voices: {:?}", tts.available_voices);
    }

    #[test]
    fn generate_from_ipa_produces_audio() {
        let Some(tts) = load_bundled_model() else {
            eprintln!("SKIP generate_from_ipa_produces_audio: model files not found");
            return;
        };
        // IPA for "hello" in en-us
        let ipa = "həloʊ";
        let voice = tts.available_voices.first().expect("at least one voice");
        let audio = tts
            .generate_from_ipa(ipa, voice, 1.0, ipa.len())
            .expect("inference should succeed");
        assert!(!audio.is_empty(), "audio output must not be empty");
        // Sanity: at least 100 ms of audio @ 24 kHz
        assert!(audio.len() >= 2_400, "expected ≥ 100 ms of audio, got {} samples", audio.len());
        // Samples should be in [-1, 1]
        let max_amp = audio.iter().copied().fold(0.0f32, f32::max);
        let min_amp = audio.iter().copied().fold(0.0f32, f32::min);
        assert!(max_amp <= 1.01, "max amplitude out of range: {max_amp}");
        assert!(min_amp >= -1.01, "min amplitude out of range: {min_amp}");
    }

    #[test]
    fn generate_from_ipa_chunks_concatenates() {
        let Some(tts) = load_bundled_model() else {
            eprintln!("SKIP generate_from_ipa_chunks_concatenates: model files not found");
            return;
        };
        let voice = tts.available_voices.first().expect("at least one voice");
        let chunks = ["həloʊ", "wɜːld"];
        let full = tts
            .generate_from_ipa_chunks(&chunks, voice, 1.0)
            .expect("chunk inference should succeed");
        let part1 = tts
            .generate_from_ipa(chunks[0], voice, 1.0, chunks[0].len())
            .expect("single chunk inference should succeed");
        // Concatenated audio must be longer than just one chunk.
        assert!(full.len() > part1.len(),
            "concatenated audio ({}) should be longer than single chunk ({})",
            full.len(), part1.len());
    }

    #[test]
    fn write_wav_creates_valid_file() {
        let Some(tts) = load_bundled_model() else {
            eprintln!("SKIP write_wav_creates_valid_file: model files not found");
            return;
        };
        let voice = tts.available_voices.first().expect("at least one voice");
        let audio = tts
            .generate_from_ipa("həloʊ", voice, 1.0, 5)
            .expect("inference should succeed");

        let tmp = std::env::temp_dir().join("kittentts_test_output.wav");
        tts.write_wav(&audio, &tmp).expect("WAV write should succeed");

        assert!(tmp.exists(), "WAV file should exist at {}", tmp.display());
        let meta = std::fs::metadata(&tmp).expect("metadata read failed");
        // WAV header is 44 bytes; with audio it must be larger.
        assert!(meta.len() > 44, "WAV file must be larger than 44 bytes (header only)");

        // Clean up.
        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn unknown_voice_returns_error() {
        let Some(tts) = load_bundled_model() else {
            eprintln!("SKIP unknown_voice_returns_error: model files not found");
            return;
        };
        let result = tts.generate_from_ipa("həloʊ", "NonExistentVoice_XYZ", 1.0, 5);
        assert!(result.is_err(), "unknown voice should return an error");
    }

    // ── espeak-gated inference tests ─────────────────────────────────────────

    #[cfg(feature = "espeak")]
    #[test]
    fn generate_text_to_audio() {
        let Some(tts) = load_bundled_model() else {
            eprintln!("SKIP generate_text_to_audio: model files not found");
            return;
        };
        let voice = tts.available_voices.first().expect("at least one voice");
        let audio = tts
            .generate("Hello world.", voice, 1.0, true)
            .expect("generate should succeed");
        assert!(!audio.is_empty(), "audio must not be empty");
        // At least 100 ms of audio
        assert!(audio.len() >= 2_400, "expected ≥ 100 ms of audio, got {}", audio.len());
    }

    #[cfg(feature = "espeak")]
    #[test]
    fn generate_to_file_creates_wav() {
        let Some(tts) = load_bundled_model() else {
            eprintln!("SKIP generate_to_file_creates_wav: model files not found");
            return;
        };
        let voice = tts.available_voices.first().expect("at least one voice");
        let tmp = std::env::temp_dir().join("kittentts_e2e_generate.wav");
        tts.generate_to_file("Hello.", &tmp, voice, 1.0, true)
            .expect("generate_to_file should succeed");
        assert!(tmp.exists(), "WAV file must exist at {}", tmp.display());
        let size = std::fs::metadata(&tmp).unwrap().len();
        assert!(size > 44, "WAV file must contain audio data (size: {size})");
        let _ = std::fs::remove_file(&tmp);
    }

    #[cfg(feature = "espeak")]
    #[test]
    fn generate_chunk_produces_audio() {
        let Some(tts) = load_bundled_model() else {
            eprintln!("SKIP generate_chunk_produces_audio: model files not found");
            return;
        };
        let voice = tts.available_voices.first().expect("at least one voice");
        let audio = tts
            .generate_chunk("Testing one two three.", voice, 1.0)
            .expect("generate_chunk should succeed");
        assert!(!audio.is_empty(), "chunk audio must not be empty");
    }
}
