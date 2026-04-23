//! Tests for the audio encoding module.

use kittentts::encoding::{AudioFormat, EncoderFactory};
use std::f32::consts::PI;

/// Generate a 1-second 440Hz sine wave at the given sample rate.
fn sine_wave(sample_rate: u32) -> Vec<f32> {
    (0..sample_rate)
        .map(|i| (i as f32 * 440.0 * 2.0 * PI / sample_rate as f32).sin())
        .collect()
}

// ─── AudioFormat parsing ────────────────────────────────────────────────────

#[test]
fn format_from_str_valid() {
    assert_eq!(AudioFormat::from_str_openai("wav"), Some(AudioFormat::Wav));
    assert_eq!(AudioFormat::from_str_openai("mp3"), Some(AudioFormat::Mp3));
    assert_eq!(AudioFormat::from_str_openai("opus"), Some(AudioFormat::Opus));
    assert_eq!(AudioFormat::from_str_openai("flac"), Some(AudioFormat::Flac));
    assert_eq!(AudioFormat::from_str_openai("pcm"), Some(AudioFormat::Pcm));
}

#[test]
fn format_from_str_invalid() {
    assert_eq!(AudioFormat::from_str_openai("aac"), None);
    assert_eq!(AudioFormat::from_str_openai(""), None);
    assert_eq!(AudioFormat::from_str_openai("WAV"), None); // case-sensitive
}

#[test]
fn format_content_types() {
    assert_eq!(AudioFormat::Wav.content_type(), "audio/wav");
    assert_eq!(AudioFormat::Mp3.content_type(), "audio/mpeg");
    assert_eq!(AudioFormat::Opus.content_type(), "audio/ogg;codecs=opus");
    assert_eq!(AudioFormat::Flac.content_type(), "audio/flac");
    assert_eq!(AudioFormat::Pcm.content_type(), "audio/pcm");
}

#[test]
fn format_extensions() {
    assert_eq!(AudioFormat::Wav.extension(), "wav");
    assert_eq!(AudioFormat::Mp3.extension(), "mp3");
    assert_eq!(AudioFormat::Opus.extension(), "ogg");
    assert_eq!(AudioFormat::Flac.extension(), "flac");
    assert_eq!(AudioFormat::Pcm.extension(), "pcm");
}

// ─── WAV encoder ────────────────────────────────────────────────────────────

#[test]
fn wav_encoder_produces_valid_riff_header() {
    let samples = sine_wave(24000);
    let encoder = EncoderFactory::create(AudioFormat::Wav).unwrap();
    let bytes = encoder.encode(&samples, 24000).unwrap();

    assert_eq!(&bytes[0..4], b"RIFF");
    assert_eq!(&bytes[8..12], b"WAVE");
    assert!(bytes.len() > 44); // header + data
}

#[test]
fn wav_encoder_empty_input() {
    let encoder = EncoderFactory::create(AudioFormat::Wav).unwrap();
    let bytes = encoder.encode(&[], 24000).unwrap();

    assert_eq!(&bytes[0..4], b"RIFF");
    assert_eq!(&bytes[8..12], b"WAVE");
}

#[test]
fn wav_encoder_data_size_matches() {
    let n = 1000;
    let samples = vec![0.5f32; n];
    let encoder = EncoderFactory::create(AudioFormat::Wav).unwrap();
    let bytes = encoder.encode(&samples, 24000).unwrap();

    // WAV data chunk should contain n * 2 bytes (16-bit samples)
    // Total = 44 byte header + n * 2 data bytes
    assert_eq!(bytes.len(), 44 + n * 2);
}

// ─── PCM encoder ────────────────────────────────────────────────────────────

#[test]
fn pcm_encoder_correct_length() {
    let samples = vec![0.0f32; 1000];
    let encoder = EncoderFactory::create(AudioFormat::Pcm).unwrap();
    let bytes = encoder.encode(&samples, 24000).unwrap();

    assert_eq!(bytes.len(), 2000); // 1000 * 2 bytes/i16
}

#[test]
fn pcm_encoder_silence_is_zeros() {
    let samples = vec![0.0f32; 100];
    let encoder = EncoderFactory::create(AudioFormat::Pcm).unwrap();
    let bytes = encoder.encode(&samples, 24000).unwrap();

    assert!(bytes.iter().all(|&b| b == 0));
}

#[test]
fn pcm_encoder_empty_input() {
    let encoder = EncoderFactory::create(AudioFormat::Pcm).unwrap();
    let bytes = encoder.encode(&[], 24000).unwrap();

    assert!(bytes.is_empty());
}

#[test]
fn pcm_encoder_max_amplitude() {
    // +1.0 should map to i16::MAX = 32767 = 0x7FFF (LE: 0xFF, 0x7F)
    let samples = vec![1.0f32];
    let encoder = EncoderFactory::create(AudioFormat::Pcm).unwrap();
    let bytes = encoder.encode(&samples, 24000).unwrap();

    assert_eq!(bytes.len(), 2);
    let val = i16::from_le_bytes([bytes[0], bytes[1]]);
    assert_eq!(val, i16::MAX);
}

// ─── MP3 encoder ────────────────────────────────────────────────────────────

#[cfg(feature = "mp3")]
#[test]
fn mp3_encoder_produces_output() {
    let samples = sine_wave(24000);
    let encoder = EncoderFactory::create(AudioFormat::Mp3).unwrap();
    let bytes = encoder.encode(&samples, 24000).unwrap();

    assert!(!bytes.is_empty());
}

#[cfg(feature = "mp3")]
#[test]
fn mp3_encoder_one_second() {
    // One second of silence
    let samples = vec![0.0f32; 24000];
    let encoder = EncoderFactory::create(AudioFormat::Mp3).unwrap();
    let bytes = encoder.encode(&samples, 24000).unwrap();

    assert!(!bytes.is_empty());
}

// ─── Opus encoder ───────────────────────────────────────────────────────────

#[cfg(feature = "opus")]
#[test]
fn opus_encoder_produces_valid_ogg() {
    let samples = sine_wave(24000);
    let encoder = EncoderFactory::create(AudioFormat::Opus).unwrap();
    let bytes = encoder.encode(&samples, 24000).unwrap();

    assert!(!bytes.is_empty());
    assert_eq!(&bytes[0..4], b"OggS");
}

#[cfg(feature = "opus")]
#[test]
fn opus_encoder_short_input() {
    let samples = vec![0.0f32; 480]; // 20ms at 24kHz
    let encoder = EncoderFactory::create(AudioFormat::Opus).unwrap();
    let bytes = encoder.encode(&samples, 24000).unwrap();

    assert!(!bytes.is_empty());
    assert_eq!(&bytes[0..4], b"OggS");
}

// ─── FLAC encoder ───────────────────────────────────────────────────────────

#[cfg(feature = "flac")]
#[test]
fn flac_encoder_produces_valid_header() {
    let samples = sine_wave(24000);
    let encoder = EncoderFactory::create(AudioFormat::Flac).unwrap();
    let bytes = encoder.encode(&samples, 24000).unwrap();

    assert!(!bytes.is_empty());
    assert_eq!(&bytes[0..4], b"fLaC");
}

#[cfg(feature = "flac")]
#[test]
fn flac_encoder_short_input() {
    let samples = vec![0.0f32; 100];
    let encoder = EncoderFactory::create(AudioFormat::Flac).unwrap();
    let bytes = encoder.encode(&samples, 24000).unwrap();

    assert!(!bytes.is_empty());
    assert_eq!(&bytes[0..4], b"fLaC");
}

// ─── EncoderFactory feature gate errors ─────────────────────────────────────

#[cfg(not(feature = "mp3"))]
#[test]
fn mp3_encoder_unavailable_without_feature() {
    let result = EncoderFactory::create(AudioFormat::Mp3);
    assert!(result.is_err());
}

#[cfg(not(feature = "opus"))]
#[test]
fn opus_encoder_unavailable_without_feature() {
    let result = EncoderFactory::create(AudioFormat::Opus);
    assert!(result.is_err());
}

#[cfg(not(feature = "flac"))]
#[test]
fn flac_encoder_unavailable_without_feature() {
    let result = EncoderFactory::create(AudioFormat::Flac);
    assert!(result.is_err());
}
