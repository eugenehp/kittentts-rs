//! Criterion benchmarks for audio encoding performance.
//!
//! Measures throughput and latency for all encoder implementations.

use criterion::{black_box, criterion_group, criterion_main, Criterion, BenchmarkId};
use kittentts::encoding::{
    AudioEncoder, WavEncoder, PcmEncoder,
    get_cached_encoder,
};

#[cfg(feature = "mp3")]
use kittentts::encoding::{Mp3Encoder, encode_mp3};

#[cfg(feature = "opus")]
use kittentts::encoding::{OpusEncoder, encode_opus};

#[cfg(feature = "flac")]
use kittentts::encoding::{FlacEncoder, encode_flac};

/// Generate test audio samples (1 second at 24kHz).
fn generate_test_samples(duration_ms: usize) -> Vec<f32> {
    let sample_count = 24_000 * duration_ms / 1000;
    (0..sample_count)
        .map(|i| (i as f32 * 0.0001).sin())
        .collect()
}

fn benchmark_wav_encoder(c: &mut Criterion) {
    let mut group = c.benchmark_group("wav_encoder");
    let encoder = WavEncoder::new();

    for duration in [100, 500, 1000].iter() {
        let samples = generate_test_samples(*duration);
        group.bench_with_input(
            BenchmarkId::from_parameter(duration),
            duration,
            |b, _| b.iter(|| encoder.encode(black_box(&samples))),
        );
    }
    group.finish();
}

fn benchmark_pcm_encoder(c: &mut Criterion) {
    let mut group = c.benchmark_group("pcm_encoder");
    let encoder = PcmEncoder::new();

    for duration in [100, 500, 1000].iter() {
        let samples = generate_test_samples(*duration);
        group.bench_with_input(
            BenchmarkId::from_parameter(duration),
            duration,
            |b, _| b.iter(|| encoder.encode(black_box(&samples))),
        );
    }
    group.finish();
}

fn benchmark_encoder_cache(c: &mut Criterion) {
    let mut group = c.benchmark_group("encoder_cache");

    group.bench_function("cache_hit_wav", |b| {
        b.iter(|| {
            let encoder = get_cached_encoder("wav");
            black_box(&encoder);
        })
    });

    group.finish();
}

#[cfg(feature = "mp3")]
fn benchmark_mp3_encoder(c: &mut Criterion) {
    let mut group = c.benchmark_group("mp3_encoder");
    let encoder = Mp3Encoder::new();

    for duration in [100, 500, 1000].iter() {
        let samples = generate_test_samples(*duration);
        group.bench_with_input(
            BenchmarkId::from_parameter(duration),
            duration,
            |b, _| b.iter(|| encoder.encode(black_box(&samples))),
        );
    }
    group.finish();
}

#[cfg(feature = "opus")]
fn benchmark_opus_encoder(c: &mut Criterion) {
    let mut group = c.benchmark_group("opus_encoder");
    let encoder = OpusEncoder::new();

    for duration in [100, 500, 1000].iter() {
        let samples = generate_test_samples(*duration);
        group.bench_with_input(
            BenchmarkId::from_parameter(duration),
            duration,
            |b, _| b.iter(|| encoder.encode(black_box(&samples))),
        );
    }
    group.finish();
}

#[cfg(feature = "flac")]
fn benchmark_flac_encoder(c: &mut Criterion) {
    let mut group = c.benchmark_group("flac_encoder");
    let encoder = FlacEncoder::new();

    for duration in [100, 500, 1000].iter() {
        let samples = generate_test_samples(*duration);
        group.bench_with_input(
            BenchmarkId::from_parameter(duration),
            duration,
            |b, _| b.iter(|| encoder.encode(black_box(&samples))),
        );
    }
    group.finish();
}

// Base benchmarks (always run)
criterion_group!(
    benches,
    benchmark_wav_encoder,
    benchmark_pcm_encoder,
    benchmark_encoder_cache,
);

criterion_main!(benches);
