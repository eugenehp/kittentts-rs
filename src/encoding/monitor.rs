//! Performance monitoring and metrics for audio encoding.
//!
//! Provides real-time performance tracking and optimization insights.

use std::time::{Duration, Instant};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

/// Performance metrics for encoding operations.
#[derive(Debug, Default)]
pub struct EncodingMetrics {
    /// Total encoding time (microseconds)
    total_time_us: AtomicU64,
    /// Total samples processed
    total_samples: AtomicU64,
    /// Number of encoding operations
    operation_count: AtomicUsize,
    /// Peak memory usage (bytes)
    peak_memory: AtomicUsize,
}

impl EncodingMetrics {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record an encoding operation.
    pub fn record_operation(&self, duration: Duration, sample_count: usize, memory_used: usize) {
        self.total_time_us.fetch_add(duration.as_micros() as u64, Ordering::Relaxed);
        self.total_samples.fetch_add(sample_count as u64, Ordering::Relaxed);
        self.operation_count.fetch_add(1, Ordering::Relaxed);

        // Update peak memory
        let mut current_peak = self.peak_memory.load(Ordering::Relaxed);
        loop {
            let old_peak = current_peak;
            let new_peak = old_peak.max(memory_used);
            match self.peak_memory.compare_exchange_weak(
                old_peak,
                new_peak,
                Ordering::SeqCst,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(actual) => current_peak = actual,
            }
        }
    }

    /// Get average encoding speed (samples per second).
    pub fn average_speed(&self) -> f64 {
        let total_time_us = self.total_time_us.load(Ordering::Relaxed);
        let total_samples = self.total_samples.load(Ordering::Relaxed);

        if total_time_us == 0 {
            0.0
        } else {
            (total_samples as f64 * 1_000_000.0) / total_time_us as f64
        }
    }

    /// Get average encoding time (milliseconds per operation).
    pub fn average_time_ms(&self) -> f64 {
        let count = self.operation_count.load(Ordering::Relaxed);
        let total_time_us = self.total_time_us.load(Ordering::Relaxed);

        if count == 0 {
            0.0
        } else {
            (total_time_us as f64) / (count as f64 * 1000.0)
        }
    }

    /// Get throughput (MB/s).
    pub fn throughput(&self) -> f64 {
        let speed = self.average_speed();
        let samples_per_mb = 1_000_000.0 / 4.0; // f32 samples per MB
        speed / samples_per_mb
    }

    /// Reset all metrics.
    pub fn reset(&self) {
        self.total_time_us.store(0, Ordering::Relaxed);
        self.total_samples.store(0, Ordering::Relaxed);
        self.operation_count.store(0, Ordering::Relaxed);
        self.peak_memory.store(0, Ordering::Relaxed);
    }

    /// Print metrics summary.
    pub fn print_summary(&self) {
        println!("📊 Encoding Performance Metrics:");
        println!("   Operations: {}", self.operation_count.load(Ordering::Relaxed));
        println!("   Total samples: {}", self.total_samples.load(Ordering::Relaxed));
        println!("   Total time: {:.2} ms", self.total_time_us.load(Ordering::Relaxed) as f64 / 1000.0);
        println!("   Avg time: {:.3} ms/op", self.average_time_ms());
        println!("   Speed: {:.1} samples/s", self.average_speed());
        println!("   Throughput: {:.2} MB/s", self.throughput());
        println!("   Peak memory: {} bytes", self.peak_memory.load(Ordering::Relaxed));
    }
}

/// RAII timer for measuring encoding operations.
pub struct EncodingTimer<'a> {
    metrics: &'a EncodingMetrics,
    start: Instant,
    sample_count: usize,
}

impl<'a> EncodingTimer<'a> {
    pub fn new(metrics: &'a EncodingMetrics, sample_count: usize) -> Self {
        Self {
            metrics,
            start: Instant::now(),
            sample_count,
        }
    }
}

impl<'a> Drop for EncodingTimer<'a> {
    fn drop(&mut self) {
        let duration = self.start.elapsed();
        // Estimate memory usage (4 bytes per f32 sample + overhead)
        let memory_used = self.sample_count * 4 + 1024; // Rough estimate

        self.metrics.record_operation(duration, self.sample_count, memory_used);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metrics() {
        let metrics = EncodingMetrics::new();

        // Simulate some operations
        metrics.record_operation(Duration::from_millis(10), 24000, 100 * 1024);
        metrics.record_operation(Duration::from_millis(20), 48000, 200 * 1024);

        assert_eq!(metrics.operation_count.load(Ordering::Relaxed), 2);
        assert!(metrics.total_samples.load(Ordering::Relaxed) == 72000);

        let speed = metrics.average_speed();
        assert!(speed > 2000.0); // Should be > 2000 samples/s

        let throughput = metrics.throughput();
        assert!(throughput > 0.1); // Should be > 0.1 MB/s
    }

    #[test]
    fn test_timer() {
        let metrics = EncodingMetrics::new();

        {
            let _timer = EncodingTimer::new(&metrics, 24000);
            // Simulate some work
            std::thread::sleep(Duration::from_millis(5));
        }

        assert_eq!(metrics.operation_count.load(Ordering::Relaxed), 1);
    }
}