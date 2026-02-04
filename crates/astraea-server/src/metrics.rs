//! Observability: metrics collection and health endpoints for AstraeaDB.
//!
//! Provides request counters, duration histograms, and system gauges
//! in a format compatible with Prometheus scraping.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::RwLock;
use std::time::{Duration, Instant, SystemTime};

/// Collected metrics for the AstraeaDB server.
pub struct ServerMetrics {
    /// Total requests by type.
    request_counts: RwLock<HashMap<String, AtomicU64>>,
    /// Total errors by type.
    error_counts: RwLock<HashMap<String, AtomicU64>>,
    /// Request durations in microseconds (for histogram approximation).
    request_durations: RwLock<Vec<(String, u64)>>,
    /// Current active connections.
    active_connections: AtomicU64,
    /// Total connections accepted since startup.
    total_connections: AtomicU64,
    /// Server start time.
    start_time: Instant,
    /// Start timestamp for uptime reporting.
    start_timestamp: u64,
    /// Maximum duration entries before compaction.
    max_duration_entries: usize,
}

impl ServerMetrics {
    /// Create a new metrics collector.
    pub fn new() -> Self {
        Self {
            request_counts: RwLock::new(HashMap::new()),
            error_counts: RwLock::new(HashMap::new()),
            request_durations: RwLock::new(Vec::new()),
            active_connections: AtomicU64::new(0),
            total_connections: AtomicU64::new(0),
            start_time: Instant::now(),
            start_timestamp: SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            max_duration_entries: 100_000,
        }
    }

    /// Record a request of the given type.
    pub fn record_request(&self, request_type: &str) {
        let counts = self.request_counts.read().unwrap();
        if let Some(counter) = counts.get(request_type) {
            counter.fetch_add(1, Ordering::Relaxed);
            return;
        }
        drop(counts);

        let mut counts = self.request_counts.write().unwrap();
        counts
            .entry(request_type.to_string())
            .or_insert_with(|| AtomicU64::new(0))
            .fetch_add(1, Ordering::Relaxed);
    }

    /// Record a request error.
    pub fn record_error(&self, request_type: &str) {
        let counts = self.error_counts.read().unwrap();
        if let Some(counter) = counts.get(request_type) {
            counter.fetch_add(1, Ordering::Relaxed);
            return;
        }
        drop(counts);

        let mut counts = self.error_counts.write().unwrap();
        counts
            .entry(request_type.to_string())
            .or_insert_with(|| AtomicU64::new(0))
            .fetch_add(1, Ordering::Relaxed);
    }

    /// Record the duration of a request in microseconds.
    pub fn record_duration(&self, request_type: &str, duration: Duration) {
        let micros = duration.as_micros() as u64;
        let mut durations = self.request_durations.write().unwrap();
        durations.push((request_type.to_string(), micros));
        if durations.len() > self.max_duration_entries {
            // Keep the most recent half.
            let keep_from = durations.len() / 2;
            durations.drain(..keep_from);
        }
    }

    /// Increment active connection count.
    pub fn connection_opened(&self) {
        self.active_connections.fetch_add(1, Ordering::Relaxed);
        self.total_connections.fetch_add(1, Ordering::Relaxed);
    }

    /// Decrement active connection count.
    pub fn connection_closed(&self) {
        self.active_connections.fetch_sub(1, Ordering::Relaxed);
    }

    /// Get current active connection count.
    pub fn active_connections(&self) -> u64 {
        self.active_connections.load(Ordering::Relaxed)
    }

    /// Get total connections since startup.
    pub fn total_connections(&self) -> u64 {
        self.total_connections.load(Ordering::Relaxed)
    }

    /// Get server uptime.
    pub fn uptime(&self) -> Duration {
        self.start_time.elapsed()
    }

    /// Export metrics in Prometheus text exposition format.
    pub fn to_prometheus(&self) -> String {
        let mut output = String::new();

        // Request counts
        output.push_str("# HELP astraea_requests_total Total number of requests by type.\n");
        output.push_str("# TYPE astraea_requests_total counter\n");
        let counts = self.request_counts.read().unwrap();
        for (req_type, count) in counts.iter() {
            let val = count.load(Ordering::Relaxed);
            output.push_str(&format!(
                "astraea_requests_total{{type=\"{}\"}} {}\n",
                req_type, val
            ));
        }

        // Error counts
        output.push_str("# HELP astraea_errors_total Total number of errors by type.\n");
        output.push_str("# TYPE astraea_errors_total counter\n");
        let errors = self.error_counts.read().unwrap();
        for (req_type, count) in errors.iter() {
            let val = count.load(Ordering::Relaxed);
            output.push_str(&format!(
                "astraea_errors_total{{type=\"{}\"}} {}\n",
                req_type, val
            ));
        }

        // Connection gauges
        output.push_str("# HELP astraea_active_connections Current active connections.\n");
        output.push_str("# TYPE astraea_active_connections gauge\n");
        output.push_str(&format!(
            "astraea_active_connections {}\n",
            self.active_connections()
        ));

        output.push_str("# HELP astraea_connections_total Total connections since startup.\n");
        output.push_str("# TYPE astraea_connections_total counter\n");
        output.push_str(&format!(
            "astraea_connections_total {}\n",
            self.total_connections()
        ));

        // Uptime
        output.push_str("# HELP astraea_uptime_seconds Server uptime in seconds.\n");
        output.push_str("# TYPE astraea_uptime_seconds gauge\n");
        output.push_str(&format!(
            "astraea_uptime_seconds {}\n",
            self.uptime().as_secs()
        ));

        // Duration summary (p50, p90, p99)
        let durations = self.request_durations.read().unwrap();
        if !durations.is_empty() {
            output
                .push_str("# HELP astraea_request_duration_us Request duration in microseconds.\n");
            output.push_str("# TYPE astraea_request_duration_us summary\n");

            // Aggregate by type
            let mut by_type: HashMap<&str, Vec<u64>> = HashMap::new();
            for (t, d) in durations.iter() {
                by_type.entry(t.as_str()).or_default().push(*d);
            }

            for (req_type, mut vals) in by_type {
                vals.sort_unstable();
                let len = vals.len();
                let p50 = vals[len / 2];
                let p90 = vals[(len as f64 * 0.9) as usize];
                let p99 = vals[((len as f64 * 0.99) as usize).min(len - 1)];
                output.push_str(&format!(
                    "astraea_request_duration_us{{type=\"{}\",quantile=\"0.5\"}} {}\n",
                    req_type, p50
                ));
                output.push_str(&format!(
                    "astraea_request_duration_us{{type=\"{}\",quantile=\"0.9\"}} {}\n",
                    req_type, p90
                ));
                output.push_str(&format!(
                    "astraea_request_duration_us{{type=\"{}\",quantile=\"0.99\"}} {}\n",
                    req_type, p99
                ));
            }
        }

        output
    }

    /// Get a health check response.
    pub fn health(&self) -> serde_json::Value {
        serde_json::json!({
            "status": "healthy",
            "uptime_seconds": self.uptime().as_secs(),
            "active_connections": self.active_connections(),
            "total_connections": self.total_connections(),
            "start_time": self.start_timestamp,
        })
    }
}

impl Default for ServerMetrics {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn record_and_count_requests() {
        let metrics = ServerMetrics::new();
        metrics.record_request("CreateNode");
        metrics.record_request("CreateNode");
        metrics.record_request("GetNode");

        let prom = metrics.to_prometheus();
        assert!(prom.contains("astraea_requests_total{type=\"CreateNode\"} 2"));
        assert!(prom.contains("astraea_requests_total{type=\"GetNode\"} 1"));
    }

    #[test]
    fn record_errors() {
        let metrics = ServerMetrics::new();
        metrics.record_error("CreateNode");
        let prom = metrics.to_prometheus();
        assert!(prom.contains("astraea_errors_total{type=\"CreateNode\"} 1"));
    }

    #[test]
    fn connection_tracking() {
        let metrics = ServerMetrics::new();
        assert_eq!(metrics.active_connections(), 0);
        metrics.connection_opened();
        metrics.connection_opened();
        assert_eq!(metrics.active_connections(), 2);
        assert_eq!(metrics.total_connections(), 2);
        metrics.connection_closed();
        assert_eq!(metrics.active_connections(), 1);
        assert_eq!(metrics.total_connections(), 2);
    }

    #[test]
    fn duration_recording() {
        let metrics = ServerMetrics::new();
        for i in 0..100 {
            metrics.record_duration("Query", Duration::from_micros(i * 10));
        }
        let prom = metrics.to_prometheus();
        assert!(prom.contains("astraea_request_duration_us{type=\"Query\",quantile=\"0.5\"}"));
        assert!(prom.contains("astraea_request_duration_us{type=\"Query\",quantile=\"0.9\"}"));
        assert!(prom.contains("astraea_request_duration_us{type=\"Query\",quantile=\"0.99\"}"));
    }

    #[test]
    fn health_check() {
        let metrics = ServerMetrics::new();
        let health = metrics.health();
        assert_eq!(health["status"], "healthy");
        assert!(health["uptime_seconds"].as_u64().is_some());
    }

    #[test]
    fn uptime_increases() {
        let metrics = ServerMetrics::new();
        thread::sleep(Duration::from_millis(10));
        assert!(metrics.uptime() >= Duration::from_millis(10));
    }

    #[test]
    fn prometheus_format_valid() {
        let metrics = ServerMetrics::new();
        metrics.record_request("Ping");
        let prom = metrics.to_prometheus();
        // Should have proper Prometheus format with # HELP and # TYPE lines
        assert!(prom.contains("# HELP astraea_requests_total"));
        assert!(prom.contains("# TYPE astraea_requests_total counter"));
        assert!(prom.contains("# HELP astraea_active_connections"));
        assert!(prom.contains("# TYPE astraea_active_connections gauge"));
    }
}
