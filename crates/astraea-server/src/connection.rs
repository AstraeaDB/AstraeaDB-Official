//! Connection management: pooling, backpressure, timeouts, and graceful shutdown.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::Semaphore;

/// Configuration for connection management.
#[derive(Debug, Clone)]
pub struct ConnectionConfig {
    /// Maximum concurrent connections. New connections beyond this are rejected.
    pub max_connections: usize,
    /// Maximum concurrent request processing. Requests beyond this wait in queue.
    pub max_concurrent_requests: usize,
    /// Close connections idle for longer than this duration.
    pub idle_timeout: Duration,
    /// Abort requests that take longer than this duration.
    pub request_timeout: Duration,
    /// Time to wait for in-flight requests during shutdown.
    pub drain_timeout: Duration,
}

impl Default for ConnectionConfig {
    fn default() -> Self {
        Self {
            max_connections: 1024,
            max_concurrent_requests: 256,
            idle_timeout: Duration::from_secs(300),   // 5 minutes
            request_timeout: Duration::from_secs(30),  // 30 seconds
            drain_timeout: Duration::from_secs(10),    // 10 seconds
        }
    }
}

/// Manages connection limits, request queuing, and shutdown coordination.
pub struct ConnectionManager {
    config: ConnectionConfig,
    /// Semaphore for limiting concurrent connections.
    connection_semaphore: Arc<Semaphore>,
    /// Semaphore for limiting concurrent request processing.
    request_semaphore: Arc<Semaphore>,
    /// Flag indicating the server is shutting down.
    shutting_down: Arc<AtomicBool>,
    /// Count of currently active connections.
    active_connections: Arc<AtomicU64>,
    /// Total connections rejected due to limits.
    rejected_connections: AtomicU64,
}

impl ConnectionManager {
    /// Create a new connection manager with the given configuration.
    pub fn new(config: ConnectionConfig) -> Self {
        Self {
            connection_semaphore: Arc::new(Semaphore::new(config.max_connections)),
            request_semaphore: Arc::new(Semaphore::new(config.max_concurrent_requests)),
            shutting_down: Arc::new(AtomicBool::new(false)),
            active_connections: Arc::new(AtomicU64::new(0)),
            rejected_connections: AtomicU64::new(0),
            config,
        }
    }

    /// Try to accept a new connection. Returns a guard that releases the slot on drop.
    /// Returns None if the connection limit is reached or server is shutting down.
    pub fn try_accept(&self) -> Option<ConnectionGuard> {
        if self.shutting_down.load(Ordering::Relaxed) {
            return None;
        }

        match self.connection_semaphore.clone().try_acquire_owned() {
            Ok(permit) => {
                self.active_connections.fetch_add(1, Ordering::Relaxed);
                Some(ConnectionGuard {
                    _permit: permit,
                    active_connections: Arc::clone(&self.active_connections),
                })
            }
            Err(_) => {
                self.rejected_connections.fetch_add(1, Ordering::Relaxed);
                None
            }
        }
    }

    /// Get a permit for processing a request. Blocks if at capacity.
    /// Returns None if the request times out waiting for a slot.
    pub async fn acquire_request_permit(&self) -> Option<tokio::sync::OwnedSemaphorePermit> {
        tokio::time::timeout(
            self.config.request_timeout,
            self.request_semaphore.clone().acquire_owned(),
        )
        .await
        .ok()
        .and_then(|r| r.ok())
    }

    /// Get the idle timeout duration.
    pub fn idle_timeout(&self) -> Duration {
        self.config.idle_timeout
    }

    /// Get the request timeout duration.
    pub fn request_timeout(&self) -> Duration {
        self.config.request_timeout
    }

    /// Check if the server is shutting down.
    pub fn is_shutting_down(&self) -> bool {
        self.shutting_down.load(Ordering::Relaxed)
    }

    /// Initiate graceful shutdown.
    pub fn initiate_shutdown(&self) {
        self.shutting_down.store(true, Ordering::Relaxed);
    }

    /// Get current active connection count.
    pub fn active_connections(&self) -> u64 {
        self.active_connections.load(Ordering::Relaxed)
    }

    /// Get total rejected connections.
    pub fn rejected_connections(&self) -> u64 {
        self.rejected_connections.load(Ordering::Relaxed)
    }

    /// Get the drain timeout for graceful shutdown.
    pub fn drain_timeout(&self) -> Duration {
        self.config.drain_timeout
    }

    /// Wait until all connections are closed or drain timeout expires.
    pub async fn wait_for_drain(&self) {
        let start = tokio::time::Instant::now();
        while self.active_connections() > 0 {
            if start.elapsed() >= self.config.drain_timeout {
                tracing::warn!(
                    "Drain timeout expired with {} active connections",
                    self.active_connections()
                );
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }
}

/// RAII guard that tracks an active connection. Decrements the counter on drop.
pub struct ConnectionGuard {
    _permit: tokio::sync::OwnedSemaphorePermit,
    active_connections: Arc<AtomicU64>,
}

impl Drop for ConnectionGuard {
    fn drop(&mut self) {
        self.active_connections.fetch_sub(1, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config() {
        let config = ConnectionConfig::default();
        assert_eq!(config.max_connections, 1024);
        assert_eq!(config.max_concurrent_requests, 256);
        assert_eq!(config.idle_timeout, Duration::from_secs(300));
    }

    #[test]
    fn connection_limit_enforced() {
        let config = ConnectionConfig {
            max_connections: 2,
            max_concurrent_requests: 10,
            ..Default::default()
        };
        let mgr = ConnectionManager::new(config);

        let _g1 = mgr.try_accept().expect("first connection should succeed");
        let _g2 = mgr.try_accept().expect("second connection should succeed");
        assert!(mgr.try_accept().is_none(), "third connection should be rejected");
        assert_eq!(mgr.active_connections(), 2);
        assert_eq!(mgr.rejected_connections(), 1);
    }

    #[test]
    fn connection_released_on_drop() {
        let config = ConnectionConfig {
            max_connections: 1,
            ..Default::default()
        };
        let mgr = ConnectionManager::new(config);

        {
            let _g = mgr.try_accept().expect("should succeed");
            assert_eq!(mgr.active_connections(), 1);
        }
        // Guard dropped, slot freed.
        assert_eq!(mgr.active_connections(), 0);
        let _g = mgr.try_accept().expect("should succeed after release");
    }

    #[test]
    fn shutdown_rejects_new_connections() {
        let mgr = ConnectionManager::new(ConnectionConfig::default());
        assert!(!mgr.is_shutting_down());

        mgr.initiate_shutdown();
        assert!(mgr.is_shutting_down());
        assert!(mgr.try_accept().is_none());
    }

    #[tokio::test]
    async fn request_permit_works() {
        let config = ConnectionConfig {
            max_concurrent_requests: 2,
            ..Default::default()
        };
        let mgr = ConnectionManager::new(config);

        let _p1 = mgr.acquire_request_permit().await.expect("should get permit");
        let _p2 = mgr.acquire_request_permit().await.expect("should get permit");
        // Third should timeout quickly in test but we don't want to wait long
    }

    #[tokio::test]
    async fn drain_completes_when_no_connections() {
        let mgr = ConnectionManager::new(ConnectionConfig {
            drain_timeout: Duration::from_millis(100),
            ..Default::default()
        });
        // No active connections, drain should complete immediately.
        mgr.wait_for_drain().await;
    }
}
