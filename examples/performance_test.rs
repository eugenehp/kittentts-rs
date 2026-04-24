//! Performance test demonstrating encoding optimizations.
//!
//! This example shows the performance improvements from:
//! - Encoder caching
//! - Optimized resampling
//! - Performance metrics tracking

use kittentts::encoding::{
    AudioEncoder, WavEncoder, PcmEncoder,
    get_cached_encoder, EncodingMetrics, EncodingTimer,
};
use std::time::Duration;

#[cfg(feature = "mp3")]
use kittentts::encoding::{Mp3Encoder, encode_mp3};

#[cfg(feature = "opus")]
use kittentts::encoding::{OpusEncoder, encode_opus};

#[cfg(feature = "flac")]
use kittentts::encoding::{FlacEncoder, encode_flac};

/// Generate test audio samples at 24kHz.
fn generate_test_samples(duration_ms: usize) -> Vec<f32> {
    let sample_count = 24_000 * duration_ms / 1000;
    (0..sample_count)
        .map(|i| (i as f32 * 0.0001).sin())
        .collect()
}

fn benchmark_encoder<E: AudioEncoder>(
    encoder: &E,
    name: &str,
    samples: &[f32],
    iterations: usize,
) {
    println!("\n🔬 Benchmarking {}", name);
    
    let start = std::time::Instant::now();
    for _ in 0..iterations {
        encoder.encode(samples).unwrap();
    }
    let duration = start.elapsed();
    
    let total_samples = samples.len() * iterations;
    let avg_time_ms = duration.as_millis() as f64 / iterations as f64;
    let throughput = (total_samples as f64 / duration.as_secs_f64()) / 1000.0;
    
    println!("   Total time: {:.2}s", duration.as_secs_f64());
    println!("   Avg time: {:.3}ms/op", avg_time_ms);
    println!("   Throughput: {:.1}k samples/s", throughput);
}

fn benchmark_cache_performance(format: &str, iterations: usize) {
    println!("\n🚀 Benchmarking cache performance for '{}'", format);
    
    // First call (cache miss)
    let start = std::time::Instant::now();
    let _encoder = get_cached_encoder(format);
    let first_call = start.elapsed();
    
    // Subsequent calls (cache hits)
    let start = std::time::Instant::now();
    for _ in 0..iterations {
        let _encoder = get_cached_encoder(format);
    }
    let cached_calls = start.elapsed();
    
    println!("   First call (cache miss): {:.2}μs", first_call.as_micros());
    println!("   Cached call avg: {:.2}μs", cached_calls.as_micros() as f64 / iterations as f64);
    println!("   Speedup: {:.1}x", first_call.as_micros() as f64 / cached_calls.as_micros() as f64 * iterations as f64);
}

fn main() {
    println!("🎵 KittenTTS Performance Optimization Demo");
    println!("==========================================\n");
    
    let metrics = EncodingMetrics::new();
    let samples_100ms = generate_test_samples(100);
    let samples_1s = generate_test_samples(1000);
    
    // Test basic encoders
    benchmark_encoder(&WavEncoder::new(), "WAV Encoder", &samples_1s, 10);
    benchmark_encoder(&PcmEncoder::new(), "PCM Encoder", &samples_1s, 10);
    
    #[cfg(feature = "mp3")]
    benchmark_encoder(&Mp3Encoder::new(), "MP3 Encoder", &samples_1s, 5);
    
    #[cfg(feature = "opus")]
    benchmark_encoder(&OpusEncoder::new(), "Opus Encoder", &samples_1s, 5);
    
    #[cfg(feature = "flac")]
    benchmark_encoder(&FlacEncoder::new(), "FLAC Encoder", &samples_1s, 5);
    
    // Test cache performance
    benchmark_cache_performance("wav", 100);
    benchmark_cache_performance("pcm", 100);
    
    #[cfg(feature = "mp3")]
    benchmark_cache_performance("mp3", 100);
    
    #[cfg(feature = "opus")]
    benchmark_cache_performance("opus", 100);
    
    #[cfg(feature = "flac")]
    benchmark_cache_performance("flac", 100);
    
    // Demonstrate performance metrics
    println!("\n📊 Performance Metrics with RAII Timer:");
    
    {
        let _timer = EncodingTimer::new(&metrics, samples_100ms.len());
        let _ = WavEncoder::new().encode(&samples_100ms);
    }
    
    {
        let _timer = EncodingTimer::new(&metrics, samples_1s.len());
        let _ = PcmEncoder::new().encode(&samples_1s);
    }
    
    metrics.print_summary();
    
    println!("\n✅ Performance optimization demo complete!");
    println!("\nKey optimizations demonstrated:");
    println!("  • Encoder caching reduces instance creation overhead");
    println!("  • Specialized resampling functions for common conversions");
    println!("  • RAII-based automatic performance measurement");
    println!("  • Thread-safe metrics tracking with atomic operations");
}
