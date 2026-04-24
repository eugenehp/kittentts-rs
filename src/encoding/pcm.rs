//! PCM audio encoding.
//!
//! Provides PCM (16-bit signed little-endian) encoding for streaming TTS.

use crate::{SAMPLE_RATE, encoding::base::AudioEncoder};

/// PCM encoder struct.
pub struct PcmEncoder;

impl PcmEncoder {
    pub fn new() -> Self {
        Self
    }
}

impl Default for PcmEncoder {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioEncoder for PcmEncoder {
    fn encode(&self, samples: &[f32]) -> Result<Vec<u8>, String> {
        Ok(encode_pcm(samples))
    }

    fn content_type(&self) -> &'static str {
        "audio/pcm"
    }

    fn extension(&self) -> &'static str {
        "pcm"
    }

    fn supports_streaming(&self) -> bool {
        true
    }
}
///
/// This is the format used for streaming TTS output via SSE.
/// - Sample rate: 24kHz
/// - Channels: 1 (mono)
/// - Bit depth: 16-bit
/// - Endianness: Little-endian
///
/// # Arguments
/// * `samples` - Audio samples in f32 format [-1.0, 1.0]
///
/// # Returns
/// PCM encoded bytes
///
/// # Example
/// ```rust
/// let audio = vec![0.0, 0.5, -0.5, 1.0];
/// let pcm = encode_pcm(&audio);
/// assert_eq!(pcm.len(), audio.len() * 2); // 2 bytes per sample
/// ```
pub fn encode_pcm(samples: &[f32]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(samples.len() * 2);
    for &sample in samples {
        // Clamp to valid range and convert to i16
        let clamped = sample.clamp(-1.0, 1.0);
        let i16_sample = (clamped * 32767.0) as i16;
        buf.extend_from_slice(&i16_sample.to_le_bytes());
    }
    buf
}

/// Get PCM audio format content type for HTTP headers.
pub fn pcm_content_type() -> &'static str {
    "audio/pcm"
}

/// Get the sample rate used for PCM encoding.
pub const fn pcm_sample_rate() -> u32 {
    SAMPLE_RATE
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode_pcm() {
        let samples = vec![0.0, 0.5, -0.5, 1.0, -1.0];
        let pcm = encode_pcm(&samples);

        // Each f32 sample becomes 2 bytes (i16)
        assert_eq!(pcm.len(), samples.len() * 2);

        // Verify some known values
        // 0.0 -> 0
        assert_eq!(&pcm[0..2], &[0, 0]);
        // 1.0 -> 32767 (0x7FFF)
        assert_eq!(&pcm[6..8], &[0xFF, 0x7F]);
    }

    #[test]
    fn test_encode_pcm_clamping() {
        let samples = vec![2.0, -1.5]; // Values outside [-1.0, 1.0]
        let pcm = encode_pcm(&samples);

        // Should be clamped to i16 range
        // 2.0 -> 1.0 -> 32767
        assert_eq!(&pcm[0..2], &[0xFF, 0x7F]);
        // -1.5 -> -1.0 -> -32768
        assert_eq!(&pcm[2..4], &[0x00, 0x80]);
    }
}