//! Audio encoding — convert raw f32 samples to various audio formats.
//!
//! WAV and PCM encoders are always available. MP3, Opus, and FLAC encoders
//! are gated behind their respective feature flags.

use anyhow::{bail, Result};

/// Supported audio output formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioFormat {
    Wav,
    Mp3,
    Opus,
    Flac,
    Pcm,
}

impl AudioFormat {
    /// Parse from OpenAI API format strings (e.g. `"mp3"`, `"wav"`).
    pub fn from_str_openai(s: &str) -> Option<Self> {
        match s {
            "wav" => Some(Self::Wav),
            "mp3" => Some(Self::Mp3),
            "opus" => Some(Self::Opus),
            "flac" => Some(Self::Flac),
            "pcm" => Some(Self::Pcm),
            _ => None,
        }
    }

    /// MIME content-type for HTTP responses.
    pub fn content_type(&self) -> &'static str {
        match self {
            Self::Wav => "audio/wav",
            Self::Mp3 => "audio/mpeg",
            Self::Opus => "audio/ogg;codecs=opus",
            Self::Flac => "audio/flac",
            Self::Pcm => "audio/pcm",
        }
    }

    /// File extension (without dot).
    pub fn extension(&self) -> &'static str {
        match self {
            Self::Wav => "wav",
            Self::Mp3 => "mp3",
            Self::Opus => "ogg",
            Self::Flac => "flac",
            Self::Pcm => "pcm",
        }
    }
}

/// Trait for audio encoders.
pub trait AudioEncoder: Send + Sync {
    /// Encode f32 samples (mono, range [-1.0, 1.0]) to bytes.
    fn encode(&self, samples: &[f32], sample_rate: u32) -> Result<Vec<u8>>;

    /// The format this encoder produces.
    fn format(&self) -> AudioFormat;

    /// MIME content-type string.
    fn content_type(&self) -> &'static str {
        self.format().content_type()
    }
}

/// Factory to create encoders by format.
pub struct EncoderFactory;

impl EncoderFactory {
    pub fn create(format: AudioFormat) -> Result<Box<dyn AudioEncoder>> {
        match format {
            AudioFormat::Wav => Ok(Box::new(WavEncoder)),
            AudioFormat::Pcm => Ok(Box::new(PcmEncoder)),

            #[cfg(feature = "mp3")]
            AudioFormat::Mp3 => Ok(Box::new(Mp3Encoder)),
            #[cfg(not(feature = "mp3"))]
            AudioFormat::Mp3 => bail!("MP3 encoding requires the `mp3` feature"),

            #[cfg(feature = "opus")]
            AudioFormat::Opus => Ok(Box::new(OpusEncoder)),
            #[cfg(not(feature = "opus"))]
            AudioFormat::Opus => bail!("Opus encoding requires the `opus` feature"),

            #[cfg(feature = "flac")]
            AudioFormat::Flac => Ok(Box::new(FlacEncoder)),
            #[cfg(not(feature = "flac"))]
            AudioFormat::Flac => bail!("FLAC encoding requires the `flac` feature"),
        }
    }
}

// ─── Helpers ────────────────────────────────────────────────────────────────

/// Convert f32 [-1.0, 1.0] to i16 [-32768, 32767], matching model.rs logic.
fn f32_to_i16(s: f32) -> i16 {
    (s * i16::MAX as f32).clamp(i16::MIN as f32, i16::MAX as f32) as i16
}

// ─── WAV encoder ────────────────────────────────────────────────────────────

pub struct WavEncoder;

impl AudioEncoder for WavEncoder {
    fn encode(&self, samples: &[f32], sample_rate: u32) -> Result<Vec<u8>> {
        let mut buf = std::io::Cursor::new(Vec::new());
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::new(&mut buf, spec)?;
        for &s in samples {
            writer.write_sample(f32_to_i16(s))?;
        }
        writer.finalize()?;
        Ok(buf.into_inner())
    }

    fn format(&self) -> AudioFormat {
        AudioFormat::Wav
    }
}

// ─── PCM encoder ────────────────────────────────────────────────────────────

/// Raw 16-bit signed little-endian PCM (no header).
/// Matches OpenAI's `pcm` format: 24kHz 16-bit signed LE mono.
pub struct PcmEncoder;

impl AudioEncoder for PcmEncoder {
    fn encode(&self, samples: &[f32], _sample_rate: u32) -> Result<Vec<u8>> {
        let mut buf = Vec::with_capacity(samples.len() * 2);
        for &s in samples {
            buf.extend_from_slice(&f32_to_i16(s).to_le_bytes());
        }
        Ok(buf)
    }

    fn format(&self) -> AudioFormat {
        AudioFormat::Pcm
    }
}

// ─── MP3 encoder ────────────────────────────────────────────────────────────

#[cfg(feature = "mp3")]
pub struct Mp3Encoder;

#[cfg(feature = "mp3")]
impl AudioEncoder for Mp3Encoder {
    fn encode(&self, samples: &[f32], sample_rate: u32) -> Result<Vec<u8>> {
        use mp3lame_encoder::{Builder, FlushNoGap, InterleavedPcm};

        let mut builder = Builder::new().ok_or_else(|| anyhow::anyhow!("Failed to create MP3 encoder"))?;
        builder.set_sample_rate(sample_rate).map_err(|e| anyhow::anyhow!("MP3 sample rate error: {:?}", e))?;
        builder.set_num_channels(1).map_err(|e| anyhow::anyhow!("MP3 channels error: {:?}", e))?;
        builder.set_brate(mp3lame_encoder::Bitrate::Kbps128).map_err(|e| anyhow::anyhow!("MP3 bitrate error: {:?}", e))?;
        builder.set_quality(mp3lame_encoder::Quality::Best).map_err(|e| anyhow::anyhow!("MP3 quality error: {:?}", e))?;
        let mut encoder = builder.build().map_err(|e| anyhow::anyhow!("MP3 build error: {:?}", e))?;

        // Convert f32 to i16
        let pcm: Vec<i16> = samples.iter().map(|&s| f32_to_i16(s)).collect();

        // LAME needs at least 1152 samples per frame; pad short inputs.
        let min_samples = 1152;
        let padded: Vec<i16>;
        let pcm_ref = if pcm.len() < min_samples {
            padded = {
                let mut p = pcm.clone();
                p.resize(min_samples, 0);
                p
            };
            &padded
        } else {
            &pcm
        };

        let input = InterleavedPcm(pcm_ref);
        // Ensure output vec has enough capacity for LAME's internal buffer.
        let mut output = Vec::with_capacity(pcm_ref.len() * 5 / 4 + 7200);
        encoder.encode_to_vec(input, &mut output).map_err(|e| anyhow::anyhow!("MP3 encode error: {:?}", e))?;
        encoder.flush_to_vec::<FlushNoGap>(&mut output).map_err(|e| anyhow::anyhow!("MP3 flush error: {:?}", e))?;

        Ok(output)
    }

    fn format(&self) -> AudioFormat {
        AudioFormat::Mp3
    }
}

// ─── Opus encoder ───────────────────────────────────────────────────────────

#[cfg(feature = "opus")]
pub struct OpusEncoder;

#[cfg(feature = "opus")]
impl AudioEncoder for OpusEncoder {
    fn encode(&self, samples: &[f32], sample_rate: u32) -> Result<Vec<u8>> {
        use audiopus::coder::Encoder;
        use audiopus::{Application, Channels, SampleRate as OpusSampleRate};
        use ogg::writing::PacketWriteEndInfo;

        // Opus requires one of: 8000, 12000, 16000, 24000, 48000
        let opus_rate = match sample_rate {
            8000 => OpusSampleRate::Hz8000,
            12000 => OpusSampleRate::Hz12000,
            16000 => OpusSampleRate::Hz16000,
            24000 => OpusSampleRate::Hz24000,
            48000 => OpusSampleRate::Hz48000,
            _ => bail!("Opus does not support sample rate {sample_rate}"),
        };

        let encoder = Encoder::new(opus_rate, Channels::Mono, Application::Audio)?;

        // Frame size: 20ms worth of samples
        let frame_samples = (sample_rate as usize) / 50; // 20ms
        let mut ogg_buf = Vec::new();

        // Write OGG stream
        {
            let serial = 1u32;
            let mut pkt_writer = ogg::writing::PacketWriter::new(&mut ogg_buf);

            // OpusHead header (RFC 7845)
            let mut head = Vec::with_capacity(19);
            head.extend_from_slice(b"OpusHead");
            head.push(1); // version
            head.push(1); // channel count
            head.extend_from_slice(&0u16.to_le_bytes()); // pre-skip
            head.extend_from_slice(&sample_rate.to_le_bytes()); // original sample rate
            head.extend_from_slice(&0i16.to_le_bytes()); // output gain
            head.push(0); // channel mapping family
            pkt_writer.write_packet(head.into_boxed_slice(), serial, PacketWriteEndInfo::EndPage, 0)?;

            // OpusTags header
            let vendor = b"kittentts";
            let mut tags = Vec::new();
            tags.extend_from_slice(b"OpusTags");
            tags.extend_from_slice(&(vendor.len() as u32).to_le_bytes());
            tags.extend_from_slice(vendor);
            tags.extend_from_slice(&0u32.to_le_bytes()); // no user comments
            pkt_writer.write_packet(tags.into_boxed_slice(), serial, PacketWriteEndInfo::EndPage, 0)?;

            // Encode audio frames
            let mut opus_buf = vec![0u8; 4000]; // max opus frame
            let mut granule: u64 = 0;
            let total_frames = (samples.len() + frame_samples - 1) / frame_samples;

            // Convert to i16 upfront
            let pcm: Vec<i16> = samples.iter().map(|&s| f32_to_i16(s)).collect();

            for i in 0..total_frames {
                let start = i * frame_samples;
                let end = (start + frame_samples).min(pcm.len());

                // Pad last frame if needed
                let frame: Vec<i16> = if end - start < frame_samples {
                    let mut f = pcm[start..end].to_vec();
                    f.resize(frame_samples, 0);
                    f
                } else {
                    pcm[start..end].to_vec()
                };

                let len = encoder.encode(&frame, &mut opus_buf)?;
                granule += frame_samples as u64;

                let info = if i == total_frames - 1 {
                    PacketWriteEndInfo::EndStream
                } else {
                    PacketWriteEndInfo::NormalPacket
                };

                pkt_writer.write_packet(
                    opus_buf[..len].to_vec().into_boxed_slice(),
                    serial,
                    info,
                    granule,
                )?;
            }
        }

        Ok(ogg_buf)
    }

    fn format(&self) -> AudioFormat {
        AudioFormat::Opus
    }
}

// ─── FLAC encoder ───────────────────────────────────────────────────────────

#[cfg(feature = "flac")]
pub struct FlacEncoder;

#[cfg(feature = "flac")]
impl AudioEncoder for FlacEncoder {
    fn encode(&self, samples: &[f32], sample_rate: u32) -> Result<Vec<u8>> {
        use flacenc::bitsink::MemSink;
        use flacenc::component::BitRepr;
        use flacenc::error::Verify;

        let pcm: Vec<i32> = samples.iter().map(|&s| f32_to_i16(s) as i32).collect();

        let config = flacenc::config::Encoder::default()
            .into_verified()
            .map_err(|(_, e)| anyhow::anyhow!("FLAC config error: {:?}", e))?;

        let source =
            flacenc::source::MemSource::from_samples(&pcm, 1, 16, sample_rate as usize);
        let flac_stream = flacenc::encode_with_fixed_block_size(&config, source, 4096)
            .map_err(|e| anyhow::anyhow!("FLAC encode error: {e}"))?;

        let mut sink = MemSink::<u8>::new();
        flac_stream
            .write(&mut sink)
            .map_err(|e| anyhow::anyhow!("FLAC write error: {:?}", e))?;

        Ok(sink.into_inner())
    }

    fn format(&self) -> AudioFormat {
        AudioFormat::Flac
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::PI;

    /// Generate a 1-second 440Hz sine wave.
    fn sine_wave(sample_rate: u32) -> Vec<f32> {
        (0..sample_rate)
            .map(|i| (i as f32 * 440.0 * 2.0 * PI / sample_rate as f32).sin())
            .collect()
    }

    #[test]
    fn format_parsing() {
        assert_eq!(AudioFormat::from_str_openai("wav"), Some(AudioFormat::Wav));
        assert_eq!(AudioFormat::from_str_openai("mp3"), Some(AudioFormat::Mp3));
        assert_eq!(AudioFormat::from_str_openai("opus"), Some(AudioFormat::Opus));
        assert_eq!(AudioFormat::from_str_openai("flac"), Some(AudioFormat::Flac));
        assert_eq!(AudioFormat::from_str_openai("pcm"), Some(AudioFormat::Pcm));
        assert_eq!(AudioFormat::from_str_openai("aac"), None);
        assert_eq!(AudioFormat::from_str_openai(""), None);
    }

    #[test]
    fn content_types() {
        assert_eq!(AudioFormat::Wav.content_type(), "audio/wav");
        assert_eq!(AudioFormat::Mp3.content_type(), "audio/mpeg");
        assert_eq!(AudioFormat::Opus.content_type(), "audio/ogg;codecs=opus");
        assert_eq!(AudioFormat::Flac.content_type(), "audio/flac");
        assert_eq!(AudioFormat::Pcm.content_type(), "audio/pcm");
    }

    #[test]
    fn wav_encoder_valid_header() {
        let samples = sine_wave(24000);
        let encoder = EncoderFactory::create(AudioFormat::Wav).unwrap();
        let bytes = encoder.encode(&samples, 24000).unwrap();
        assert_eq!(&bytes[0..4], b"RIFF");
        assert_eq!(&bytes[8..12], b"WAVE");
        assert!(bytes.len() > 44);
    }

    #[test]
    fn pcm_encoder_correct_length() {
        let samples = vec![0.0f32; 1000];
        let encoder = EncoderFactory::create(AudioFormat::Pcm).unwrap();
        let bytes = encoder.encode(&samples, 24000).unwrap();
        assert_eq!(bytes.len(), 2000); // 1000 samples * 2 bytes/i16
    }

    #[test]
    fn pcm_encoder_values() {
        // Silence should produce zero bytes
        let samples = vec![0.0f32; 10];
        let encoder = EncoderFactory::create(AudioFormat::Pcm).unwrap();
        let bytes = encoder.encode(&samples, 24000).unwrap();
        assert!(bytes.iter().all(|&b| b == 0));
    }

    #[test]
    fn wav_encoder_empty_input() {
        let encoder = EncoderFactory::create(AudioFormat::Wav).unwrap();
        let bytes = encoder.encode(&[], 24000).unwrap();
        // Should still produce a valid WAV header
        assert_eq!(&bytes[0..4], b"RIFF");
        assert_eq!(&bytes[8..12], b"WAVE");
    }

    #[test]
    fn pcm_encoder_empty_input() {
        let encoder = EncoderFactory::create(AudioFormat::Pcm).unwrap();
        let bytes = encoder.encode(&[], 24000).unwrap();
        assert!(bytes.is_empty());
    }

    #[cfg(feature = "mp3")]
    #[test]
    fn mp3_encoder_produces_output() {
        let samples = sine_wave(24000);
        let encoder = EncoderFactory::create(AudioFormat::Mp3).unwrap();
        let bytes = encoder.encode(&samples, 24000).unwrap();
        assert!(!bytes.is_empty());
    }

    #[cfg(feature = "opus")]
    #[test]
    fn opus_encoder_produces_valid_ogg() {
        let samples = sine_wave(24000);
        let encoder = EncoderFactory::create(AudioFormat::Opus).unwrap();
        let bytes = encoder.encode(&samples, 24000).unwrap();
        assert!(!bytes.is_empty());
        // OGG files start with "OggS"
        assert_eq!(&bytes[0..4], b"OggS");
    }

    #[cfg(feature = "flac")]
    #[test]
    fn flac_encoder_produces_output() {
        let samples = sine_wave(24000);
        let encoder = EncoderFactory::create(AudioFormat::Flac).unwrap();
        let bytes = encoder.encode(&samples, 24000).unwrap();
        assert!(!bytes.is_empty());
        // FLAC files start with "fLaC"
        assert_eq!(&bytes[0..4], b"fLaC");
    }
}
