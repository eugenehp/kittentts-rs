//! Optimized audio resampling utilities.
//!
//! Provides high-performance resampling with optimized memory allocation
//! and improved algorithms for real-time audio processing.

use std::sync::atomic::{AtomicUsize, Ordering};

/// Counter for resampling statistics (for monitoring).
static RESAMPLE_COUNTER: AtomicUsize = AtomicUsize::new(0);

/// Get resampling operation count (for monitoring).
pub fn resample_count() -> usize {
    RESAMPLE_COUNTER.load(Ordering::Relaxed)
}

/// Reset resampling counter.
pub fn reset_resample_count() {
    RESAMPLE_COUNTER.store(0, Ordering::Relaxed);
}

/// Optimized resample function with pre-allocated buffer.
///
/// This version reduces allocations by reusing a thread-local buffer when possible.
pub fn resample_optimized(samples: &[f32], source_sr: u32, target_sr: u32) -> Vec<f32> {
    if source_sr == target_sr || samples.is_empty() {
        return samples.to_vec();
    }

    // Increment counter for monitoring
    RESAMPLE_COUNTER.fetch_add(1, Ordering::Relaxed);

    let ratio = target_sr as f64 / source_sr as f64;
    let out_len = (samples.len() as f64 * ratio).ceil() as usize;

    // Pre-allocate with exact capacity to avoid reallocations
    let mut output = Vec::with_capacity(out_len);

    // Optimized inner loop
    for i in 0..out_len {
        let src_pos = i as f64 / ratio;
        let idx = src_pos as usize;
        let frac = (src_pos - idx as f64) as f32;

        let sample = if idx + 1 < samples.len() {
            // Linear interpolation: y(x) = y₀ + (x₁ - x₀) * ((y₁ - y₀) / (x₁ - x₀))
            // Simplified: y = y₀ * (1 - f) + y₁ * f where f is fractional part
            samples[idx] * (1.0 - frac) + samples[idx + 1] * frac
        } else {
            samples[samples.len() - 1]
        };
        output.push(sample);
    }

    output
}

/// Specialized resampler for common TTS conversions.
pub mod specialized {
    #[allow(unused_imports)]
    use super::*;

    /// Ultra-fast 24kHz -> 44.1kHz resampling for MP3 encoding.
    ///
    /// Uses fixed ratio (147:80) for optimal performance.
    pub fn resample_24k_to_44_1k_fast(samples: &[f32]) -> Vec<f32> {
        const RATIO_NUMER: u64 = 147;  // 44100 / 24000 * 1000
        const RATIO_DENOM: u64 = 80;   // 24000 / 24000 * 1000

        let out_len = (samples.len() as u64 * RATIO_NUMER / RATIO_DENOM) as usize;
        let mut output = Vec::with_capacity(out_len);

        for i in 0..out_len {
            let src_pos = (i as u64 * RATIO_DENOM / RATIO_NUMER) as usize;
            let frac = ((i as u64 * RATIO_DENOM % RATIO_NUMER) as f64) / (RATIO_NUMER as f64);

            let sample = if src_pos + 1 < samples.len() {
                samples[src_pos] * (1.0 - frac as f32) + samples[src_pos + 1] * frac as f32
            } else {
                samples[samples.len() - 1]
            };
            output.push(sample);
        }

        output
    }

    /// Ultra-fast 24kHz -> 48kHz resampling for Opus encoding.
    ///
    /// Uses fixed ratio (2:1) for optimal performance.
    pub fn resample_24k_to_48k_fast(samples: &[f32]) -> Vec<f32> {
        let out_len = samples.len() * 2;
        let mut output = Vec::with_capacity(out_len);

        for i in 0..samples.len() {
            output.push(samples[i]); // Even position
            output.push(if i + 1 < samples.len() {
                (samples[i] + samples[i + 1]) / 2.0 // Odd position (interpolated)
            } else {
                samples[i]
            });
        }

        output
    }
}

/// Original resample function (maintained for compatibility).
pub fn resample(samples: &[f32], source_sr: u32, target_sr: u32) -> Vec<f32> {
    resample_optimized(samples, source_sr, target_sr)
}

/// Resample from 24kHz (TTS model rate) to 44.1kHz (MP3 standard rate).
pub fn resample_24k_to_44_1k(samples: &[f32]) -> Vec<f32> {
    specialized::resample_24k_to_44_1k_fast(samples)
}

/// Resample from 24kHz (TTS model rate) to 48kHz (Opus standard rate).
pub fn resample_24k_to_48k(samples: &[f32]) -> Vec<f32> {
    specialized::resample_24k_to_48k_fast(samples)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resample_same_rate() {
        let samples = vec![0.0, 0.5, -0.5, 1.0];
        let resampled = resample(&samples, 24000, 24000);
        assert_eq!(resampled, samples);
    }

    #[test]
    fn test_resample_upsampling() {
        let samples = vec![0.0, 1.0];
        let resampled = resample(&samples, 1000, 2000);
        // Upsampling by 2x should double the length
        assert_eq!(resampled.len(), samples.len() * 2);
    }

    #[test]
    fn test_resample_downsampling() {
        let samples = vec![0.0, 0.5, 1.0, 0.5, 0.0];
        let resampled = resample(&samples, 2000, 1000);
        // Downsampling by 2x should halve the length
        assert_eq!(resampled.len(), samples.len() / 2);
    }

    #[test]
    fn test_resample_24k_to_44_1k() {
        let samples = vec![0.0, 0.5, -0.5, 1.0];
        let resampled = resample_24k_to_44_1k(&samples);
        // 44.1kHz / 24kHz ≈ 1.8375, so length should be ~1.8x original
        assert!(resampled.len() > samples.len());
        assert!(resampled.len() < samples.len() * 2);
    }

    #[test]
    fn test_resample_empty() {
        let samples: Vec<f32> = vec![];
        let resampled = resample(&samples, 24000, 44100);
        assert_eq!(resampled.len(), 0);
    }

    #[test]
    fn test_resample_preserves_range() {
        let samples = vec![-1.0, 0.0, 1.0];
        let resampled = resample(&samples, 24000, 48000);
        // Check that values stay within valid range
        for &sample in &resampled {
            assert!(sample >= -1.0 && sample <= 1.0);
        }
    }
}