//! FLAC audio encoding.
//!
//! Provides FLAC (Free Lossless Audio Codec) encoding using flacenc.
//! Requires the 'flac' feature to be enabled.

#[cfg(feature = "flac")]
use crate::{encoding::base::AudioEncoder, SAMPLE_RATE};

/// FLAC encoder struct.
#[cfg(feature = "flac")]
pub struct FlacEncoder;

#[cfg(feature = "flac")]
impl FlacEncoder {
    pub fn new() -> Self {
        Self
    }
}

#[cfg(feature = "flac")]
impl Default for FlacEncoder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(feature = "flac")]
impl AudioEncoder for FlacEncoder {
    fn encode(&self, samples: &[f32]) -> Result<Vec<u8>, String> {
        encode_flac(samples)
    }

    fn content_type(&self) -> &'static str {
        "audio/flac"
    }

    fn extension(&self) -> &'static str {
        "flac"
    }

    fn sample_rate(&self) -> u32 {
        SAMPLE_RATE // FLAC works with 24kHz natively
    }
}

/// Encode f32 audio samples as FLAC.
///
/// FLAC is a lossless codec, so it preserves the original audio quality
/// while still achieving significant compression.
///
/// # Arguments
/// * `samples` - Audio samples in f32 format [-1.0, 1.0] at 24kHz
///
/// # Returns
/// FLAC encoded audio data
///
/// # Configuration
/// - Sample rate: 24kHz (native, no resampling needed)
/// - Bit depth: 16-bit
/// - Channels: 1 (mono)
/// - Compression: Default level (5)
#[cfg(feature = "flac")]
pub fn encode_flac(samples: &[f32]) -> Result<Vec<u8>, String> {
    use flacenc::component::BitRepr;
    use flacenc::error::Verify;

    // Convert f32 samples to i32 for FLAC encoder
    let pcm_i32: Vec<i32> = samples
        .iter()
        .map(|&s| (s.clamp(-1.0, 1.0) * 32767.0) as i32)
        .collect();

    // Create source from samples
    let source = flacenc::source::MemSource::from_samples(
        &pcm_i32,
        1,              // channels
        16,             // bits per sample
        SAMPLE_RATE as usize
    );

    // Create FLAC encoder configuration
    let config = flacenc::config::Encoder::default();
    let block_size = config.block_size;
    let config = config
        .into_verified()
        .map_err(|(_cfg, e)| format!("FLAC config error: {:?}", e))?;

    // Encode the audio
    let flac_stream = flacenc::encode_with_fixed_block_size(&config, source, block_size)
        .map_err(|e| format!("FLAC encode error: {:?}", e))?;

    // Write to buffer
    let mut sink = flacenc::bitsink::ByteSink::new();
    flac_stream
        .write(&mut sink)
        .map_err(|e| format!("FLAC write error: {:?}", e))?;

    Ok(sink.as_slice().to_vec())
}

#[cfg(test)]
mod tests {
    #[cfg(feature = "flac")]
    use super::*;

    #[test]
    #[cfg(feature = "flac")]
    fn test_flac_encoder() {
        let encoder = FlacEncoder::new();
        let audio = vec![0.0, 0.5, -0.5, 1.0];
        let result = encoder.encode(&audio);

        assert!(result.is_ok());
        let flac_data = result.unwrap();

        // FLAC data should start with FLAC magic bytes
        assert!(flac_data.len() > 4);
        assert_eq!(&flac_data[0..4], b"fLaC");
        assert_eq!(encoder.content_type(), "audio/flac");
        assert_eq!(encoder.extension(), "flac");
        assert_eq!(encoder.sample_rate(), 24000);
    }

    #[test]
    #[cfg(feature = "flac")]
    fn test_encode_flac_longer() {
        // Create a longer audio sample
        let audio: Vec<f32> = (0..2000)
            .map(|i| (i as f32 / 2000.0 * 2.0 - 1.0) * 0.8)
            .collect();

        let result = encode_flac(&audio);
        assert!(result.is_ok());

        let flac_data = result.unwrap();
        // FLAC container should be created
        assert!(flac_data.len() > 4);
        assert_eq!(&flac_data[0..4], b"fLaC");

        // FLAC should be compressed but still lossless
        // Original would be: 2000 * 2 bytes = 4000 bytes
        // FLAC should be smaller due to compression
        assert!(flac_data.len() < 3500);
    }
}

// Stub implementations when feature is not enabled
#[cfg(not(feature = "flac"))]
pub struct FlacEncoder;

#[cfg(not(feature = "flac"))]
impl FlacEncoder {
    pub fn new() -> Self {
        panic!("FLAC encoding requires the 'flac' feature to be enabled");
    }
}