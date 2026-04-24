//! Opus audio encoding.
//!
//! Provides Opus encoding using audiopus with OGG container.
//! Requires the 'opus' feature to be enabled.

#[cfg(feature = "opus")]
use crate::encoding::base::AudioEncoder;

/// Opus encoder struct.
#[cfg(feature = "opus")]
pub struct OpusEncoder;

#[cfg(feature = "opus")]
impl OpusEncoder {
    pub fn new() -> Self {
        Self
    }
}

#[cfg(feature = "opus")]
impl Default for OpusEncoder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(feature = "opus")]
impl AudioEncoder for OpusEncoder {
    fn encode(&self, samples: &[f32]) -> Result<Vec<u8>, String> {
        encode_opus(samples)
    }

    fn content_type(&self) -> &'static str {
        "audio/ogg"
    }

    fn extension(&self) -> &'static str {
        "opus"
    }

    fn sample_rate(&self) -> u32 {
        48000 // Opus standard sample rate
    }
}

/// Encode f32 audio samples as Opus in OGG container.
///
/// Opus encoding requires 48kHz sample rate, so input audio
/// is resampled from 24kHz (TTS model rate) to 48kHz.
///
/// # Arguments
/// * `samples` - Audio samples in f32 format [-1.0, 1.0] at 24kHz
///
/// # Returns
/// OGG container with Opus encoded audio data
///
/// # Configuration
/// - Sample rate: 48kHz (resampled from 24kHz)
/// - Frame size: 20ms (960 samples)
/// - Application: VoIP (optimized for speech)
#[cfg(feature = "opus")]
pub fn encode_opus(samples: &[f32]) -> Result<Vec<u8>, String> {
    use audiopus::coder::Encoder as OpusEncoder;
    use audiopus::{Application, Channels, SampleRate};
    use ogg::writing::{PacketWriteEndInfo, PacketWriter};

    // Step 1: Optimized resampling from 24kHz to 48kHz
    let samples_48k = crate::encoding::resample::resample_24k_to_48k(samples);

    // Step 2: Create Opus encoder
    let encoder = OpusEncoder::new(SampleRate::Hz48000, Channels::Mono, Application::Voip)
        .map_err(|e| format!("Opus encoder init error: {:?}", e))?;

    // Step 3: Set up OGG container
    let frame_size: usize = 960; // 20ms at 48kHz
    let mut ogg_buf = Vec::new();
    let serial = 1u32;

    {
        let mut writer = PacketWriter::new(&mut ogg_buf);

        // OpusHead identification header (RFC 7845)
        let mut head = Vec::with_capacity(19);
        head.extend_from_slice(b"OpusHead");
        head.push(1); // version
        head.push(1); // channel count (mono)
        head.extend_from_slice(&0u16.to_le_bytes()); // pre-skip
        head.extend_from_slice(&48000u32.to_le_bytes()); // input sample rate
        head.extend_from_slice(&0i16.to_le_bytes()); // output gain
        head.push(0); // channel mapping family 0

        writer.write_packet(head.into(), serial, PacketWriteEndInfo::EndPage, 0)
            .map_err(|e| format!("OGG write error: {:?}", e))?;

        // OpusTags comment header
        let vendor = b"kitten-tts-rs";
        let mut tags = Vec::new();
        tags.extend_from_slice(b"OpusTags");
        tags.extend_from_slice(&(vendor.len() as u32).to_le_bytes());
        tags.extend_from_slice(vendor);
        tags.extend_from_slice(&0u32.to_le_bytes()); // no additional tags

        writer.write_packet(tags.into(), serial, PacketWriteEndInfo::EndPage, 0)
            .map_err(|e| format!("OGG write error: {:?}", e))?;

        // Step 4: Encode audio in 20ms frames
        let mut opus_out = vec![0u8; 4000];
        let mut granule: u64 = 0;
        let total_frames = samples_48k.len().div_ceil(frame_size);

        for (i, chunk) in samples_48k.chunks(frame_size).enumerate() {
            let frame: Vec<f32> = if chunk.len() < frame_size {
                let mut padded = chunk.to_vec();
                padded.resize(frame_size, 0.0);
                padded
            } else {
                chunk.to_vec()
            };

            let encoded_len = encoder
                .encode_float(&frame, &mut opus_out)
                .map_err(|e| format!("Opus encode error: {:?}", e))?;

            granule += frame_size as u64;
            let end_info = if i == total_frames - 1 {
                PacketWriteEndInfo::EndStream
            } else {
                PacketWriteEndInfo::NormalPacket
            };

            writer.write_packet(opus_out[..encoded_len].to_vec().into(), serial, end_info, granule)
                .map_err(|e| format!("OGG write error: {:?}", e))?;
        }
    }

    Ok(ogg_buf)
}

#[cfg(test)]
mod tests {
    #[cfg(feature = "opus")]
    use super::*;

    #[test]
    #[cfg(feature = "opus")]
    fn test_opus_encoder() {
        let encoder = OpusEncoder::new();
        let audio = vec![0.0, 0.5, -0.5, 1.0];
        let result = encoder.encode(&audio);

        assert!(result.is_ok());
        let opus_data = result.unwrap();

        // OGG/Opus data should exist and start with OGG magic
        assert!(opus_data.len() > 0);
        assert_eq!(&opus_data[0..4], b"OggS");
        assert_eq!(encoder.content_type(), "audio/ogg");
        assert_eq!(encoder.extension(), "opus");
        assert_eq!(encoder.sample_rate(), 48000);
    }

    #[test]
    #[cfg(feature = "opus")]
    fn test_encode_opus_longer() {
        // Create a longer audio sample (simulating real TTS output)
        let audio: Vec<f32> = (0..2000)
            .map(|i| (i as f32 / 2000.0 * 2.0 - 1.0) * 0.8)
            .collect();

        let result = encode_opus(&audio);
        assert!(result.is_ok());

        let opus_data = result.unwrap();
        // OGG container should be created
        assert!(opus_data.len() > 0);
        assert_eq!(&opus_data[0..4], b"OggS");
    }
}

// Stub implementations when feature is not enabled
#[cfg(not(feature = "opus"))]
pub struct OpusEncoder;

#[cfg(not(feature = "opus"))]
impl OpusEncoder {
    pub fn new() -> Self {
        panic!("Opus encoding requires the 'opus' feature to be enabled");
    }
}