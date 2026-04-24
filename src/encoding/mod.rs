//! Audio encoding module for multiple audio formats.
//!
//! Provides a unified interface for encoding TTS audio output
//! in various formats: WAV, PCM, MP3, Opus, FLAC.

pub mod base;
pub mod pcm;
pub mod wav;
pub mod resample;
pub mod performance;
pub mod monitor;

#[cfg(feature = "mp3")]
pub mod mp3;

#[cfg(feature = "opus")]
pub mod opus;

#[cfg(feature = "flac")]
pub mod flac;

// Re-exports for convenience
pub use base::{AudioEncoder, AudioFormat};
pub use pcm::{PcmEncoder, encode_pcm};
pub use wav::{WavEncoder, audio_to_wav};
pub use performance::{get_cached_encoder, ENCODER_CACHE};
pub use monitor::{EncodingMetrics, EncodingTimer};

#[cfg(feature = "mp3")]
pub use mp3::{Mp3Encoder, encode_mp3};

#[cfg(feature = "opus")]
pub use opus::{OpusEncoder, encode_opus};

#[cfg(feature = "flac")]
pub use flac::{FlacEncoder, encode_flac};

/// Encoder factory for creating format-specific encoders.
pub struct EncoderFactory;

impl EncoderFactory {
    /// Create an encoder for the specified format.
    pub fn create(format: AudioFormat) -> Box<dyn AudioEncoder> {
        match format {
            AudioFormat::Wav => Box::new(WavEncoder::new()),
            AudioFormat::Pcm => Box::new(PcmEncoder::new()),
            #[cfg(feature = "mp3")]
            AudioFormat::Mp3 => Box::new(Mp3Encoder::new()),
            #[cfg(not(feature = "mp3"))]
            AudioFormat::Mp3 => panic!("MP3 encoding requires the 'mp3' feature"),
            #[cfg(feature = "opus")]
            AudioFormat::Opus => Box::new(OpusEncoder::new()),
            #[cfg(not(feature = "opus"))]
            AudioFormat::Opus => panic!("Opus encoding requires the 'opus' feature"),
            #[cfg(feature = "flac")]
            AudioFormat::Flac => Box::new(FlacEncoder::new()),
            #[cfg(not(feature = "flac"))]
            AudioFormat::Flac => panic!("FLAC encoding requires the 'flac' feature"),
        }
    }

    /// Create encoder from format string.
    pub fn from_string(format_str: &str) -> Result<Box<dyn AudioEncoder>, String> {
        let format = AudioFormat::from_string(format_str)?;
        Ok(Self::create(format))
    }
}