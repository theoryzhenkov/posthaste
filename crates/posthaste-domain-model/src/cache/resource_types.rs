use super::*;

/// Resource policy for optional-content cache maintenance.
///
/// The priority queue decides which cache objects matter most. This policy
/// decides how much work a runtime may attempt before yielding to the app and
/// device.
///
/// @spec docs/L1-sync#cache-resource-governor
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CacheResourcePolicy {
    pub rescore_rows_per_second: f64,
    pub rescore_burst_rows: usize,
    pub max_rescore_rows_per_lease: usize,
    pub interactive_min_rescore_rows: usize,
    pub stale_rescore_fraction: f64,
    pub background_fetch_requests_per_second: f64,
    pub interactive_fetch_requests_per_second: f64,
    pub fetch_request_burst: usize,
    pub max_background_fetch_requests_per_lease: usize,
    pub max_interactive_fetch_requests_per_lease: usize,
    pub background_fetch_bytes_per_second: f64,
    pub interactive_fetch_bytes_per_second: f64,
    pub fetch_byte_burst: u64,
    pub max_background_fetch_bytes_per_lease: u64,
    pub max_interactive_fetch_bytes_per_lease: u64,
    pub interactive_min_fetch_bytes: u64,
    pub min_fetch_bytes_per_lease: u64,
    pub min_network_rate_multiplier: f64,
}

impl Default for CacheResourcePolicy {
    fn default() -> Self {
        Self {
            rescore_rows_per_second: 50.0,
            rescore_burst_rows: 500,
            max_rescore_rows_per_lease: 200,
            interactive_min_rescore_rows: 200,
            stale_rescore_fraction: 0.25,
            background_fetch_requests_per_second: 0.3,
            interactive_fetch_requests_per_second: 1.5,
            fetch_request_burst: 8,
            max_background_fetch_requests_per_lease: 2,
            max_interactive_fetch_requests_per_lease: 6,
            background_fetch_bytes_per_second: 256.0 * 1024.0,
            interactive_fetch_bytes_per_second: 2.0 * BYTES_PER_MIB,
            fetch_byte_burst: 64 * 1024 * 1024,
            max_background_fetch_bytes_per_lease: 4 * 1024 * 1024,
            max_interactive_fetch_bytes_per_lease: 32 * 1024 * 1024,
            interactive_min_fetch_bytes: 4 * 1024 * 1024,
            min_fetch_bytes_per_lease: 64 * 1024,
            min_network_rate_multiplier: 0.125,
        }
    }
}

/// Fetch-side lease granted to the cache worker.
///
/// @spec docs/L1-sync#cache-resource-governor
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CacheFetchLease {
    pub request_limit: usize,
    pub byte_limit: u64,
    pub interactive_pressure: f64,
}

impl CacheFetchLease {
    pub fn none(interactive_pressure: f64) -> Self {
        Self {
            request_limit: 0,
            byte_limit: 0,
            interactive_pressure: clamp_unit(interactive_pressure),
        }
    }

    pub fn new(request_limit: usize, byte_limit: u64, interactive_pressure: f64) -> Self {
        Self {
            request_limit,
            byte_limit,
            interactive_pressure: clamp_unit(interactive_pressure),
        }
    }

    pub fn has_fetch_budget(self) -> bool {
        self.request_limit > 0 && self.byte_limit > 0
    }
}

/// Cache-maintenance work allowance for one runtime slice.
///
/// @spec docs/L1-sync#cache-resource-governor
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CacheMaintenanceLease {
    pub stale_rescore_limit: usize,
    pub rescore_limit: usize,
    pub fetch: CacheFetchLease,
    pub network_rate_multiplier: f64,
    pub in_backoff: bool,
}

/// Observed work from one cache-maintenance slice.
///
/// @spec docs/L1-sync#cache-resource-governor
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CacheMaintenanceFeedback {
    pub stale_rescore_queued: usize,
    pub rescore_scanned: usize,
    pub fetch_attempted: usize,
    pub fetch_attempted_bytes: u64,
    pub fetch_cached: usize,
    pub fetch_failed: usize,
    pub elapsed: Duration,
    pub had_error: bool,
    pub had_fetch_error: bool,
}
