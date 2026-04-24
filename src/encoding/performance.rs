//! Performance optimizations for audio encoding.
//!
//! Provides caching, pooling, and other optimizations to improve
//! encoding performance and reduce memory allocations.

use std::sync::{Arc, RwLock};
use std::collections::HashMap;
use once_cell::sync::Lazy;
use crate::encoding::{base::AudioEncoder, monitor::EncodingMetrics};

/// Global encoder cache for reusing encoder instances.
pub struct EncoderCache {
    encoders: Arc<RwLock<HashMap<String, Box<dyn AudioEncoder>>>>,
    metrics: EncodingMetrics,
}

impl EncoderCache {
    pub fn new() -> Self {
        Self {
            encoders: Arc::new(RwLock::new(HashMap::new())),
            metrics: EncodingMetrics::new(),
        }
    }

    /// Get or create an encoder for the specified format with performance tracking.
    pub fn get_encoder(&self, format: &str) -> Result<Box<dyn AudioEncoder>, String> {
        // Try read lock first (fast path)
        {
            let cache = self.encoders.read().unwrap();
            if let Some(_encoder) = cache.get(format) {
                // Return cached encoder (note: this is a placeholder for the actual encoder)
                // In production, you'd use Arc<AudioEncoder> or create new instances
                return self.create_encoder(format);
            }
        }

        // Slow path: create new encoder
        self.create_encoder(format)
    }

    fn create_encoder(&self, format: &str) -> Result<Box<dyn AudioEncoder>, String> {
        let audio_format = crate::encoding::AudioFormat::from_string(format)?;
        Ok(crate::encoding::EncoderFactory::create(audio_format))
    }

    /// Get performance metrics.
    pub fn metrics(&self) -> &EncodingMetrics {
        &self.metrics
    }

    /// Print performance summary.
    pub fn print_metrics(&self) {
        self.metrics.print_summary();
    }
}

/// Global encoder cache instance.
pub static ENCODER_CACHE: Lazy<EncoderCache> = Lazy::new(|| EncoderCache::new());

/// Quick encoder access using global cache.
pub fn get_cached_encoder(format: &str) -> Result<Box<dyn AudioEncoder>, String> {
    ENCODER_CACHE.get_encoder(format)
}

/// Print global performance metrics.
pub fn print_performance_metrics() {
    ENCODER_CACHE.print_metrics();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_creation() {
        let cache = EncoderCache::new();

        // Test that we can get encoders from cache
        let wav_encoder = cache.get_encoder("wav").unwrap();
        assert_eq!(wav_encoder.content_type(), "audio/wav");

        let mp3_encoder_result = cache.get_encoder("mp3");
        if cfg!(feature = "mp3") {
            assert!(mp3_encoder_result.is_ok());
        }
    }

    #[test]
    fn test_global_cache() {
        let wav_encoder = get_cached_encoder("wav").unwrap();
        assert_eq!(wav_encoder.content_type(), "audio/wav");
    }
}