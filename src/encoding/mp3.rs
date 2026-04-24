//! MP3 audio encoding.
//!
//! Provides MP3 encoding using mp3lame-encoder v0.2.
//! Requires the 'mp3' feature to be enabled.

#[cfg(feature = "mp3")]
use crate::encoding::base::AudioEncoder;

/// MP3 encoder struct.
#[cfg(feature = "mp3")]
pub struct Mp3Encoder;

#[cfg(feature = "mp3")]
impl Mp3Encoder {
    pub fn new() -> Self {
        Self
    }
}

#[cfg(feature = "mp3")]
impl Default for Mp3Encoder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(feature = "mp3")]
impl AudioEncoder for Mp3Encoder {
    fn encode(&self, samples: &[f32]) -> Result<Vec<u8>, String> {
        encode_mp3(samples)
    }

    fn content_type(&self) -> &'static str {
        "audio/mpeg"
    }

    fn extension(&self) -> &'static str {
        "mp3"
    }

    fn sample_rate(&self) -> u32 {
        44100 // MP3 standard sample rate
    }
}

/// Encode f32 audio samples as MP3 (mono, 128 kbps CBR).
///
/// MP3 encoding requires 44.1kHz sample rate, so input audio
/// is resampled from 24kHz (TTS model rate) to 44.1kHz.
///
/// # Arguments
/// * `samples` - Audio samples in f32 format [-1.0, 1.0] at 24kHz
///
/// # Returns
/// MP3 encoded audio data
#[cfg(feature = "mp3")]
pub fn encode_mp3(samples: &[f32]) -> Result<Vec<u8>, String> {
    use mp3lame_encoder::{Builder, FlushNoGap, MonoPcm};

    // Step 1: Optimized resampling from 24kHz to 44.1kHz
    let samples_44k = crate::encoding::resample::resample_24k_to_44_1k(samples);

    // Step 2: Create MP3 encoder
    let mut encoder = Builder::new()
        .ok_or_else(|| format!("Failed to create MP3 encoder"))?;

    encoder
        .set_num_channels(1)
        .map_err(|e| format!("MP3 config error: {:?}", e))?;

    encoder
        .set_sample_rate(44100)
        .map_err(|e| format!("MP3 config error: {:?}", e))?;

    encoder
        .set_brate(mp3lame_encoder::Bitrate::Kbps128)
        .map_err(|e| format!("MP3 config error: {:?}", e))?;

    encoder
        .set_quality(mp3lame_encoder::Quality::Best)
        .map_err(|e| format!("MP3 config error: {:?}", e))?;

    let mut encoder = encoder
        .build()
        .map_err(|e| format!("MP3 build error: {:?}", e))?;

    // Step 3: Convert f32 samples to i16
    let pcm: Vec<i16> = samples_44k
        .iter()
        .map(|&s| (s.clamp(-1.0, 1.0) * 32767.0) as i16)
        .collect();

    // Step 4: Encode to MP3
    let input = MonoPcm(&pcm);
    let mut mp3_out = Vec::with_capacity(mp3lame_encoder::max_required_buffer_size(pcm.len()));

    encoder
        .encode_to_vec(input, &mut mp3_out)
        .map_err(|e| format!("MP3 encode error: {:?}", e))?;

    encoder
        .flush_to_vec::<FlushNoGap>(&mut mp3_out)
        .map_err(|e| format!("MP3 flush error: {:?}", e))?;

    Ok(mp3_out)
}

#[cfg(test)]
mod tests {
    #[cfg(feature = "mp3")]
    use super::*;

    #[test]
    #[cfg(feature = "mp3")]
    fn test_mp3_encoder() {
        let encoder = Mp3Encoder::new();
        let audio = vec![0.0, 0.5, -0.5, 1.0];
        let result = encoder.encode(&audio);

        assert!(result.is_ok());
        let mp3_data = result.unwrap();

        // MP3 data should exist
        assert!(mp3_data.len() > 0);
        assert_eq!(encoder.content_type(), "audio/mpeg");
        assert_eq!(encoder.extension(), "mp3");
        assert_eq!(encoder.sample_rate(), 44100);
    }

    #[test]
    #[cfg(feature = "mp3")]
    fn test_encode_mp3() {
        let audio = vec![0.0, 0.5, -0.5, 1.0, -1.0];
        let result = encode_mp3(&audio);

        assert!(result.is_ok());
        let mp3_data = result.unwrap();
        assert!(mp3_data.len() > 0);
    }
}

// Stub implementations when feature is not enabled
#[cfg(not(feature = "mp3"))]
pub struct Mp3Encoder;

#[cfg(not(feature = "mp3"))]
impl Mp3Encoder {
    pub fn new() -> Self {
        panic!("MP3 encoding requires the 'mp3' feature to be enabled");
    }
}