//! WAV audio encoding.
//!
//! Provides WAV (RIFF WAVE) encoding for TTS output.

use crate::{SAMPLE_RATE, encoding::base::AudioEncoder};

/// WAV encoder struct.
pub struct WavEncoder;

impl WavEncoder {
    pub fn new() -> Self {
        Self
    }
}

impl Default for WavEncoder {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioEncoder for WavEncoder {
    fn encode(&self, samples: &[f32]) -> Result<Vec<u8>, String> {
        Ok(audio_to_wav(samples))
    }

    fn content_type(&self) -> &'static str {
        "audio/wav"
    }

    fn extension(&self) -> &'static str {
        "wav"
    }
}

/// Convert f32 audio samples to WAV file format.
///
/// Creates a WAV file with:
/// - Sample rate: 24kHz
/// - Channels: 1 (mono)
/// - Bit depth: 16-bit PCM
/// - Endianness: Little-endian
///
/// # Arguments
/// * `audio` - Audio samples in f32 format [-1.0, 1.0]
///
/// # Returns
/// Complete WAV file as byte vector
pub fn audio_to_wav(audio: &[f32]) -> Vec<u8> {
    let mut buffer = Vec::new();

    // RIFF header
    buffer.extend_from_slice(b"RIFF");

    // File size - 8 (will update later)
    let file_size_pos = buffer.len();
    buffer.extend_from_slice(&[0u8; 4]);

    // WAVE format
    buffer.extend_from_slice(b"WAVE");

    // fmt chunk
    buffer.extend_from_slice(b"fmt ");

    // Chunk size (16 for PCM)
    buffer.extend_from_slice(&16u32.to_le_bytes());

    // Audio format (1 = PCM)
    buffer.extend_from_slice(&1u16.to_le_bytes());

    // Number of channels (1 = mono)
    buffer.extend_from_slice(&1u16.to_le_bytes());

    // Sample rate
    buffer.extend_from_slice(&(SAMPLE_RATE).to_le_bytes());

    // Byte rate = SampleRate * NumChannels * BitsPerSample/8
    let byte_rate = SAMPLE_RATE * 1 * 16 / 8;
    buffer.extend_from_slice(&byte_rate.to_le_bytes());

    // Block align = NumChannels * BitsPerSample/8
    buffer.extend_from_slice(&2u16.to_le_bytes());

    // Bits per sample
    buffer.extend_from_slice(&16u16.to_le_bytes());

    // data chunk
    buffer.extend_from_slice(b"data");

    // Data size (will update later)
    let data_size_pos = buffer.len();
    buffer.extend_from_slice(&[0u8; 4]);

    // Audio data (convert f32 [-1.0, 1.0] to i16)
    let data_start = buffer.len();
    for &sample in audio {
        let clamped = sample.clamp(-1.0, 1.0);
        let i16_sample = (clamped * 32767.0) as i16;
        buffer.extend_from_slice(&i16_sample.to_le_bytes());
    }

    // Update sizes
    let data_size = (buffer.len() - data_start) as u32;
    let file_size = (buffer.len() - 8) as u32;

    buffer[data_size_pos..data_size_pos + 4].copy_from_slice(&data_size.to_le_bytes());
    buffer[file_size_pos..file_size_pos + 4].copy_from_slice(&file_size.to_le_bytes());

    buffer
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_audio_to_wav() {
        let audio = vec![0.0, 0.5, -0.5, 1.0];
        let wav = audio_to_wav(&audio);

        // Check RIFF header
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");

        // Check fmt chunk
        assert_eq!(&wav[12..16], b"fmt ");
        assert_eq!(&wav[20..22], 16u32.to_le_bytes()); // Chunk size
        assert_eq!(&wav[22..24], 1u16.to_le_bytes());  // Audio format (PCM)
        assert_eq!(&wav[24..26], 1u16.to_le_bytes());  // Channels

        // Check data chunk
        assert!(wav.len() > 44); // At least header + some data
    }

    #[test]
    fn test_wav_encoder() {
        let encoder = WavEncoder::new();
        let audio = vec![0.0, 0.5, -0.5, 1.0];
        let result = encoder.encode(&audio);

        assert!(result.is_ok());
        let wav_data = result.unwrap();
        assert_eq!(&wav_data[0..4], b"RIFF");
        assert_eq!(encoder.content_type(), "audio/wav");
        assert_eq!(encoder.extension(), "wav");
    }
}