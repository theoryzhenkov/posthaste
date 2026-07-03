//! `Retry-After` parsing (D80/F1) — both HTTP forms into the `Duration` the
//! policy's [`retry_delay`](posthaste_call_policy::BackoffSchedule::retry_delay)
//! expects.
//!
//! RFC 9110 allows `Retry-After` as either a **delta-seconds** integer
//! (`Retry-After: 120`) or an **HTTP-date** (`Retry-After: Wed, 21 Oct 2026
//! 07:28:00 GMT`). Parsing lives here at the native edge — not in the wasm-pure
//! policy core — because the http-date form needs a wall clock to turn an
//! absolute instant into a delay. The `now` argument is injected so the http-date
//! arithmetic is deterministic in tests.

use std::time::Duration;

use reqwest::header::{HeaderMap, RETRY_AFTER};
use time::OffsetDateTime;

/// Parse a `Retry-After` header into a delay relative to `now`.
///
/// Returns `None` when the header is absent or unparseable (the caller then
/// falls back to plain jittered backoff). An HTTP-date already in the past
/// yields `Duration::ZERO` (retry immediately, subject to backoff).
pub(crate) fn parse_retry_after(headers: &HeaderMap, now: OffsetDateTime) -> Option<Duration> {
    let raw = headers.get(RETRY_AFTER)?.to_str().ok()?.trim();

    // Form 1: delta-seconds.
    if let Ok(seconds) = raw.parse::<u64>() {
        return Some(Duration::from_secs(seconds));
    }

    // Form 2: an HTTP-date (IMF-fixdate, the RFC 9110 "preferred" form).
    let target = parse_imf_fixdate(raw)?;
    let delta = target - now;
    if delta.is_positive() {
        Some(delta.unsigned_abs())
    } else {
        Some(Duration::ZERO)
    }
}

/// Parse an IMF-fixdate (`Sun, 06 Nov 1994 08:49:37 GMT`). The `time` crate's
/// `Rfc2822` well-known form rejects the literal `GMT` zone, so the fixed
/// grammar is described explicitly and parsed as UTC.
fn parse_imf_fixdate(value: &str) -> Option<OffsetDateTime> {
    // Built per call (only on a 429/503 with a date form — cold path).
    let description = time::format_description::parse(
        "[weekday repr:short], [day] [month repr:short] [year] \
         [hour]:[minute]:[second] GMT",
    )
    .ok()?;
    let parsed = time::PrimitiveDateTime::parse(value, &description).ok()?;
    Some(parsed.assume_utc())
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::header::HeaderValue;

    fn headers_with(value: &str) -> HeaderMap {
        let mut map = HeaderMap::new();
        map.insert(RETRY_AFTER, HeaderValue::from_str(value).unwrap());
        map
    }

    #[test]
    fn parses_delta_seconds() {
        let now = OffsetDateTime::UNIX_EPOCH;
        assert_eq!(
            parse_retry_after(&headers_with("120"), now),
            Some(Duration::from_secs(120))
        );
    }

    #[test]
    fn parses_http_date_relative_to_now() {
        // now = 1994-11-06 08:49:07 UTC; target 30 s later.
        let now = OffsetDateTime::from_unix_timestamp(784111747).unwrap();
        assert_eq!(
            parse_retry_after(&headers_with("Sun, 06 Nov 1994 08:49:37 GMT"), now),
            Some(Duration::from_secs(30))
        );
    }

    #[test]
    fn past_http_date_is_zero_not_negative() {
        let now = OffsetDateTime::from_unix_timestamp(784111747).unwrap();
        assert_eq!(
            parse_retry_after(&headers_with("Sun, 06 Nov 1994 08:49:07 GMT"), now),
            Some(Duration::ZERO)
        );
    }

    #[test]
    fn absent_or_garbage_is_none() {
        let now = OffsetDateTime::UNIX_EPOCH;
        assert_eq!(parse_retry_after(&HeaderMap::new(), now), None);
        assert_eq!(parse_retry_after(&headers_with("soon-ish"), now), None);
    }
}
