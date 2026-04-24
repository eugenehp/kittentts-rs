//! Audio encoding performance benchmarks.
//!
//! Compare performance of different audio encoding formats.

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use kittentts::encoding::{EncoderFactory, AudioFormat};

fn bench_audio_data() -> Vec<f32> {
    // Generate realistic audio data (10 seconds at 24kHz)
    (0..240_000) // 10 seconds * 24000 samples/second
        .map(|i| {
            let t = i as f32 / 24000.0;
            // Simulate speech-like audio
            (t * 440.0 * 2.0 * std::f32::consts::PI).sin() * 0.6f32
            + (t * 880.0 * 2.0 * std::f32::consts::PI).sin() * 0.3f32
        })
        .collect()
}

fn bench_wav(c: &mut Criterion) {
    let audio_data = bench_audio_data();

    c.bench_function("wav_encoding", |b| {
        let encoder = EncoderFactory::create(AudioFormat::Wav);
        b.iter(|| {
            black_box(encoder.encode(&audio_data).unwrap())
        })
    });
}

fn bench_mp3(c: &mut Criterion) {
    let audio_data = bench_audio_data();

    c.bench_function("mp3_encoding", |b| {
        let encoder = EncoderFactory::create(AudioFormat::Mp3);
        b.iter(|| {
            black_box(encoder.encode(&audio_data).unwrap())
        })
    });
}

fn bench_opus(c: &mut Criterion) {
    let audio_data = bench_audio_data();

    c.bench_function("opus_encoding", |b| {
        let encoder = EncoderFactory::create(AudioFormat::Opus);
        b.iter(|| {
            black_box(encoder.encode(&audio_data).unwrap())
        })
    });
}

fn bench_flac(c: &mut Criterion) {
    let audio_data = bench_audio_data();

    c.bench_function("flac_encoding", |b| {
        let encoder = EncoderFactory::create(AudioFormat::Flac);
        b.iter(|| {
            black_box(encoder.encode(&audio_data).unwrap())
        })
    });
}

fn bench_throughput(c: &mut Criterion) {
    let audio_data = bench_audio_data();

    let mut group = c.benchmark_group("encoding_throughput");

    group.through_duration(criterion::Duration::from_secs(10));

    group.bench_function("wav_throughput", |b| {
        let encoder = EncoderFactory::create(AudioFormat::Wav);
        b.iter(|| {
            let data = black_box(encoder.encode(&audio_data).unwrap());
            data.len()
        })
    });

    group.bench_function("mp3_throughput", |b| {
        let encoder = EncoderFactory::create(AudioFormat::Mp3);
        b.iter(|| {
            let data = black_box(encoder.encode(&audio_data).unwrap());
            data.len()
        })
    });

    group.finish();
}

criterion_group!(benches, bench_wav, bench_mp3, bench_opus, bench_flac);
criterion_group!(throughputs, bench_throughput);
criterion_main!(benches, throughputs);