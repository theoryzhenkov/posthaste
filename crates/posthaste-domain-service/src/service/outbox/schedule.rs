//! Scheduled-send time arithmetic: `send_at` normalization and the
//! monotonic-anchored "now" the outbox compares due-ness against.
//!
//! One mechanism serves undo-send (`send_at = now + delay`) and send-later
//! (`send_at = the chosen time`): the enqueued send op carries `send_at` and
//! the flush simply refuses to push it before then. Everything here is pure
//! string/instant arithmetic — no I/O.
//!
//! Clock discipline (the same RFC-L2-lifecycle row 10 rider the snooze
//! scheduler applies): due-ness is compared against a monotonic-anchored
//! wall-clock sample, not a raw `SystemTime::now()`. A backward NTP correction
//! can therefore never re-open an already-due boundary, and a forward step
//! during this process's life cannot fire a held send early (eroding the
//! undo window) — the anchored "now" only advances by real elapsed time.
//! A restart re-anchors from the wall clock at that moment (the realistic
//! risk is drift correction on a long-lived process, not boot skew).
//!
//! @spec docs/L1-outbox#operation-model

use std::sync::OnceLock;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

/// Parse and normalize a caller-supplied `send_at` to the canonical stored
/// form: UTC, whole seconds, RFC 3339 with a trailing `Z`. Every stored
/// `send_at` and every comparison "now" passes through this one formatter, so
/// lexicographic string comparison in the store is exact chronological order.
///
/// Sub-second precision rounds UP: "not before `send_at`" must hold exactly,
/// so a 10.4s undo window becomes 11s, never 10s. An invalid timestamp is an
/// error (the caller rejects the request); a PAST timestamp is accepted and is
/// simply already due — it flushes on the next pass (the pinned choice:
/// past `send_at` sends immediately rather than rejecting, so a client whose
/// clock lags the runtime's can never have an "immediate" send bounce).
pub(crate) fn normalize_send_at(raw: &str) -> Result<String, String> {
    let parsed = OffsetDateTime::parse(raw, &Rfc3339)
        .map_err(|error| format!("invalid sendAt timestamp (want RFC 3339): {error}"))?;
    let mut epoch = parsed.unix_timestamp();
    if parsed.nanosecond() > 0 {
        epoch += 1;
    }
    format_epoch_rfc3339(epoch)
}

/// The outbox's due-comparison "now" in the canonical stored form (see
/// [`normalize_send_at`]), sampled from the monotonic-anchored clock.
pub(crate) fn outbox_now_rfc3339() -> Result<String, String> {
    format_epoch_rfc3339(monotonic_now_secs())
}

/// Format UNIX epoch seconds as canonical UTC whole-second RFC 3339 (`...Z`).
fn format_epoch_rfc3339(epoch: i64) -> Result<String, String> {
    OffsetDateTime::from_unix_timestamp(epoch)
        .map_err(|error| format!("timestamp out of range: {error}"))?
        .format(&Rfc3339)
        .map_err(|error| format!("timestamp failed to format: {error}"))
}

/// Monotonic-anchored wall-clock seconds (the supervisor snooze scheduler's
/// discipline, replicated for the outbox): one wall sample anchored against
/// `Instant::now()` at first use, then advanced only by monotonic elapsed
/// time, so the value never regresses and never jumps forward with an OS
/// clock correction for this process's lifetime.
fn monotonic_now_secs() -> i64 {
    static ANCHOR: OnceLock<(Instant, SystemTime)> = OnceLock::new();
    let &(anchor_instant, anchor_wall) = ANCHOR.get_or_init(|| (Instant::now(), SystemTime::now()));
    let elapsed = Instant::now().saturating_duration_since(anchor_instant);
    anchored_now_secs(anchor_wall, elapsed)
}

/// Pure core of [`monotonic_now_secs`], split out as the declared test seam
/// (mirrors `SupervisorShared::anchored_now_secs`).
pub(crate) fn anchored_now_secs(anchor_wall: SystemTime, elapsed: Duration) -> i64 {
    (anchor_wall + elapsed)
        .duration_since(UNIX_EPOCH)
        .map(|delta| delta.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_offsets_and_subseconds_to_canonical_utc() {
        // An offset time normalizes to the same instant in UTC.
        assert_eq!(
            normalize_send_at("2026-07-07T12:30:00+02:00").unwrap(),
            "2026-07-07T10:30:00Z"
        );
        // Sub-second precision rounds UP so the hold is never shorter than asked.
        assert_eq!(
            normalize_send_at("2026-07-07T10:30:00.250Z").unwrap(),
            "2026-07-07T10:30:01Z"
        );
        // Already-canonical input round-trips unchanged.
        assert_eq!(
            normalize_send_at("2026-07-07T10:30:00Z").unwrap(),
            "2026-07-07T10:30:00Z"
        );
    }

    #[test]
    fn rejects_non_rfc3339_input() {
        assert!(normalize_send_at("tomorrow 9am").is_err());
        assert!(normalize_send_at("2026-07-07 10:30:00").is_err());
        assert!(normalize_send_at("").is_err());
    }

    #[test]
    fn canonical_form_orders_lexicographically() {
        // The store compares send_at strings with `<=`; canonical fixed-width
        // UTC forms must therefore order exactly chronologically.
        let earlier = normalize_send_at("2026-07-07T09:59:59Z").unwrap();
        let later = normalize_send_at("2026-07-07T12:00:00+02:00").unwrap();
        assert!(earlier < later);
        let now = outbox_now_rfc3339().unwrap();
        assert_eq!(now.len(), "2026-07-07T10:30:00Z".len());
        assert!(now.ends_with('Z'));
    }

    #[test]
    fn anchored_now_only_advances_with_elapsed_time() {
        let wall = UNIX_EPOCH + Duration::from_secs(1_000_000);
        let at_zero = anchored_now_secs(wall, Duration::ZERO);
        let at_ten = anchored_now_secs(wall, Duration::from_secs(10));
        assert_eq!(at_ten - at_zero, 10);
    }
}
