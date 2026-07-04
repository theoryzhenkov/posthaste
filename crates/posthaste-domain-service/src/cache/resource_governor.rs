use super::*;

/// Stateful token-bucket governor for optional-content cache maintenance.
///
/// Re-score tokens bound local CPU/SQLite work. Fetch request and byte tokens
/// bound provider/network pressure. Failures lower the network multiplier and
/// enter short backoff; successful fetches gradually restore the multiplier.
///
/// @spec docs/L1-sync#cache-resource-governor
#[derive(Clone, Debug)]
pub struct CacheResourceGovernor {
    policy: CacheResourcePolicy,
    rescore_tokens: f64,
    fetch_request_tokens: f64,
    fetch_byte_tokens: f64,
    last_refill: Instant,
    network_rate_multiplier: f64,
    consecutive_fetch_failures: u32,
    backoff_until: Option<Instant>,
}

impl CacheResourceGovernor {
    pub fn new(now: Instant, policy: CacheResourcePolicy) -> Self {
        Self {
            policy,
            rescore_tokens: policy.rescore_burst_rows as f64,
            fetch_request_tokens: policy.fetch_request_burst as f64,
            fetch_byte_tokens: policy.fetch_byte_burst as f64,
            last_refill: now,
            network_rate_multiplier: 1.0,
            consecutive_fetch_failures: 0,
            backoff_until: None,
        }
    }

    pub fn grant(&mut self, now: Instant, interactive_pressure: f64) -> CacheMaintenanceLease {
        let interactive_pressure = clamp_unit(interactive_pressure);
        self.refill(now, interactive_pressure);
        if interactive_pressure > 0.0 {
            self.rescore_tokens = self
                .rescore_tokens
                .max(self.policy.interactive_min_rescore_rows as f64)
                .min(self.policy.rescore_burst_rows as f64);
            self.fetch_request_tokens = self.fetch_request_tokens.max(1.0);
            self.fetch_byte_tokens = self
                .fetch_byte_tokens
                .max(self.policy.interactive_min_fetch_bytes as f64)
                .min(self.policy.fetch_byte_burst as f64);
        }

        let max_rescore = if interactive_pressure > 0.0 {
            self.policy
                .max_rescore_rows_per_lease
                .max(self.policy.interactive_min_rescore_rows)
        } else {
            self.policy.max_rescore_rows_per_lease
        };
        let rescore_total = self.take_whole_rescore_tokens(max_rescore);
        let stale_rescore_limit = if interactive_pressure > 0.0 {
            0
        } else {
            ((rescore_total as f64) * clamp_unit(self.policy.stale_rescore_fraction)).floor()
                as usize
        };
        let rescore_limit = rescore_total.saturating_sub(stale_rescore_limit);

        let in_backoff = self
            .backoff_until
            .is_some_and(|backoff_until| now < backoff_until);
        let fetch = if in_backoff {
            CacheFetchLease::none(interactive_pressure)
        } else {
            self.take_fetch_lease(interactive_pressure)
        };

        CacheMaintenanceLease {
            stale_rescore_limit,
            rescore_limit,
            fetch,
            network_rate_multiplier: self.network_rate_multiplier,
            in_backoff,
        }
    }

    pub fn record_feedback(
        &mut self,
        now: Instant,
        lease: &CacheMaintenanceLease,
        feedback: CacheMaintenanceFeedback,
    ) {
        self.refund_unused(lease, feedback);
        let fetch_failed = feedback.fetch_failed > 0 || feedback.had_fetch_error;
        if fetch_failed {
            self.record_fetch_failure(now, feedback.fetch_cached > 0);
            return;
        }

        if feedback.fetch_attempted > 0 {
            self.consecutive_fetch_failures = 0;
            self.backoff_until = None;
            self.network_rate_multiplier = (self.network_rate_multiplier + 0.1).min(1.0);
        }
    }

    /// A cache-maintenance slice was cancelled out from under the worker (the
    /// supervisor's arm-budget backstop dropped the future), so
    /// [`Self::record_feedback`] never ran for it. Treat the cancellation as a
    /// no-progress fetch failure: without this, the arm-drop path leaves the
    /// governor pristine and the short cache tick immediately re-hits the slow
    /// provider — the perpetual-recurrence half of the "stuck until reload"
    /// wedge. Consecutive cancellations escalate the backoff exactly like
    /// consecutive fetch failures.
    pub fn record_cancelled_slice(&mut self, now: Instant) {
        self.record_fetch_failure(now, false);
    }

    /// Shared failure arithmetic: halve the network rate multiplier, and when
    /// the slice made no caching progress, enter (escalating) backoff.
    fn record_fetch_failure(&mut self, now: Instant, made_progress: bool) {
        self.consecutive_fetch_failures = self.consecutive_fetch_failures.saturating_add(1);
        self.network_rate_multiplier =
            (self.network_rate_multiplier * 0.5).max(self.policy.min_network_rate_multiplier);
        if !made_progress {
            let exponent = self.consecutive_fetch_failures.saturating_sub(1).min(6);
            let backoff_seconds = 5_u64.saturating_mul(1_u64 << exponent).min(300);
            self.backoff_until = Some(now + Duration::from_secs(backoff_seconds));
        }
    }

    pub fn network_rate_multiplier(&self) -> f64 {
        self.network_rate_multiplier
    }

    pub fn is_in_backoff(&self, now: Instant) -> bool {
        self.backoff_until
            .is_some_and(|backoff_until| now < backoff_until)
    }

    fn refill(&mut self, now: Instant, interactive_pressure: f64) {
        let elapsed = now
            .saturating_duration_since(self.last_refill)
            .as_secs_f64();
        self.last_refill = now;
        if elapsed <= 0.0 {
            return;
        }
        self.rescore_tokens = (self.rescore_tokens + elapsed * self.policy.rescore_rows_per_second)
            .min(self.policy.rescore_burst_rows as f64);

        let request_rate = lerp(
            self.policy.background_fetch_requests_per_second,
            self.policy.interactive_fetch_requests_per_second,
            interactive_pressure,
        ) * self.network_rate_multiplier;
        self.fetch_request_tokens = (self.fetch_request_tokens + elapsed * request_rate)
            .min(self.policy.fetch_request_burst as f64);

        let byte_rate = lerp(
            self.policy.background_fetch_bytes_per_second,
            self.policy.interactive_fetch_bytes_per_second,
            interactive_pressure,
        ) * self.network_rate_multiplier;
        self.fetch_byte_tokens =
            (self.fetch_byte_tokens + elapsed * byte_rate).min(self.policy.fetch_byte_burst as f64);
    }

    fn take_whole_rescore_tokens(&mut self, max_rows: usize) -> usize {
        let rows = (self.rescore_tokens.floor() as usize).min(max_rows);
        self.rescore_tokens -= rows as f64;
        rows
    }

    fn take_fetch_lease(&mut self, interactive_pressure: f64) -> CacheFetchLease {
        let max_requests = lerp(
            self.policy.max_background_fetch_requests_per_lease as f64,
            self.policy.max_interactive_fetch_requests_per_lease as f64,
            interactive_pressure,
        )
        .round() as usize;
        let max_bytes = lerp(
            self.policy.max_background_fetch_bytes_per_lease as f64,
            self.policy.max_interactive_fetch_bytes_per_lease as f64,
            interactive_pressure,
        )
        .round() as u64;
        let request_limit = (self.fetch_request_tokens.floor() as usize).min(max_requests);
        let byte_limit = (self.fetch_byte_tokens.floor() as u64).min(max_bytes);
        if request_limit == 0 || byte_limit < self.policy.min_fetch_bytes_per_lease {
            return CacheFetchLease::none(interactive_pressure);
        }
        self.fetch_request_tokens -= request_limit as f64;
        self.fetch_byte_tokens -= byte_limit as f64;
        CacheFetchLease::new(request_limit, byte_limit, interactive_pressure)
    }

    fn refund_unused(&mut self, lease: &CacheMaintenanceLease, feedback: CacheMaintenanceFeedback) {
        let reserved_rescore = lease
            .stale_rescore_limit
            .saturating_add(lease.rescore_limit);
        let used_rescore = feedback
            .stale_rescore_queued
            .saturating_add(feedback.rescore_scanned)
            .min(reserved_rescore);
        self.rescore_tokens = (self.rescore_tokens
            + reserved_rescore.saturating_sub(used_rescore) as f64)
            .min(self.policy.rescore_burst_rows as f64);

        let used_requests = feedback.fetch_attempted.min(lease.fetch.request_limit);
        self.fetch_request_tokens = (self.fetch_request_tokens
            + lease.fetch.request_limit.saturating_sub(used_requests) as f64)
            .min(self.policy.fetch_request_burst as f64);

        let used_bytes = feedback.fetch_attempted_bytes.min(lease.fetch.byte_limit);
        self.fetch_byte_tokens = (self.fetch_byte_tokens
            + lease.fetch.byte_limit.saturating_sub(used_bytes) as f64)
            .min(self.policy.fetch_byte_burst as f64);
    }
}
