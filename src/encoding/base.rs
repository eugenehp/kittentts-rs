//! Base audio encoding traits and types.

use crate::SAMPLE_RATE;

/// Supported audio output formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioFormat {
    /// WAV format (uncompressed, compatible)
    Wav,
    /// PCM format (raw 16-bit signed little-endian)
    Pcm,
    /// MP3 format (compressed, high compatibility)
    Mp3,
    /// Opus format (compressed, low latency)
    Opus,
    /// FLAC format (lossless compression)
    Flac,
}

impl AudioFormat {
    /// Parse format from string (case-insensitive).
    pub fn from_string(s: &str) -> Result<Self, String> {
        match s.to_lowercase().as_str() {
            "wav" => Ok(AudioFormat::Wav),
            "pcm" => Ok(AudioFormat::Pcm),
            "mp3" => Ok(AudioFormat::Mp3),
            "opus" => Ok(AudioFormat::Opus),
            "flac" => Ok(AudioFormat::Flac),
            _ => Err(format!(
                "Unsupported format '{}'. Supported: wav, pcm, mp3, opus, flac",
                s
            )),
        }
    }

    /// Get HTTP Content-Type header value.
    pub fn content_type(&self) -> &'static str {
        match self {
            AudioFormat::Wav => "audio/wav",
            AudioFormat::Pcm => "audio/pcm",
            AudioFormat::Mp3 => "audio/mpeg",
            AudioFormat::Opus => "audio/ogg",
            AudioFormat::Flac => "audio/flac",
        }
    }

    /// Get file extension for this format.
    pub fn extension(&self) -> &'static str {
        match self {
            AudioFormat::Wav => "wav",
            AudioFormat::Pcm => "pcm",
            AudioFormat::Mp3 => "mp3",
            AudioFormat::Opus => "opus",
            AudioFormat::Flac => "flac",
        }
    }

    /// Check if format is supported for streaming.
    pub fn supports_streaming(&self) -> bool {
        matches!(self, AudioFormat::Pcm)
    }
}

/// Trait for audio encoders.
pub trait AudioEncoder: Send + Sync {
    /// Encode audio samples to the target format.
    ///
    /// # Arguments
    /// * `samples` - Audio samples in f32 format [-1.0, 1.0] at 24kHz
    ///
    /// # Returns
    /// Encoded audio data
    fn encode(&self, samples: &[f32]) -> Result<Vec<u8>, String>;

    /// Get the content type for HTTP headers.
    fn content_type(&self) -> &'static str;

    /// Get the file extension for this format.
    fn extension(&self) -> &'static str;

    /// Get the sample rate this encoder works with.
    fn sample_rate(&self) -> u32 {
        SAMPLE_RATE
    }

    /// Check if this encoder supports streaming.
    fn supports_streaming(&self) -> bool {
        false
    }
}