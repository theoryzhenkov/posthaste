//! Typed logging primitives shared by Posthaste crates.
//!
//! Application logs should use `ph_info!`, `ph_debug!`, etc. with one of the
//! constants in [`events`]. That makes the stable `event` field a typed input
//! instead of an ad-hoc string at each log call.

pub use tracing;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LogEvent(&'static str);

impl LogEvent {
    const fn new(name: &'static str) -> Self {
        Self(name)
    }

    pub const fn name(self) -> &'static str {
        self.0
    }
}

#[doc(hidden)]
pub const fn event_name(event: LogEvent) -> &'static str {
    event.name()
}

/// Generate the five `ph_<level>!` event-logging macros. They differ only by the
/// inner `tracing::<level>!`, so they are emitted from one template. The `$d:tt`
/// parameter (always invoked as `$`) lets the *generated* macro use `$` for its
/// own metavariables — the stable-Rust workaround for nested `macro_rules!`.
macro_rules! define_ph_event_macro {
    ($d:tt $name:ident => $level:ident) => {
        #[macro_export]
        macro_rules! $name {
            (target: $d target:expr, $d event:expr, $d($d fields:tt)+) => {
                $crate::tracing::$level!(target: $d target, event = $crate::event_name($d event), $d($d fields)+)
            };
            (parent: $d parent:expr, $d event:expr, $d($d fields:tt)+) => {
                $crate::tracing::$level!(parent: $d parent, event = $crate::event_name($d event), $d($d fields)+)
            };
            ($d event:expr, $d($d fields:tt)+) => {
                $crate::tracing::$level!(event = $crate::event_name($d event), $d($d fields)+)
            };
        }
    };
}

define_ph_event_macro!($ ph_trace => trace);
define_ph_event_macro!($ ph_debug => debug);
define_ph_event_macro!($ ph_info => info);
define_ph_event_macro!($ ph_warn => warn);
define_ph_event_macro!($ ph_error => error);

/// Generate the `ph_forwarded_<level>!` macros, which forward an already-typed
/// `event` field verbatim (the desktop frontend bridge re-emits frontend logs).
/// Single-arm; same template trick as [`define_ph_event_macro`].
macro_rules! define_ph_forwarded_macro {
    ($d:tt $name:ident => $level:ident) => {
        #[macro_export]
        macro_rules! $name {
            (target: $d target:expr, event: $d event:expr, $d($d fields:tt)+) => {
                $crate::tracing::$level!(target: $d target, event = $d event, $d($d fields)+)
            };
        }
    };
}

define_ph_forwarded_macro!($ ph_forwarded_trace => trace);
define_ph_forwarded_macro!($ ph_forwarded_debug => debug);
define_ph_forwarded_macro!($ ph_forwarded_info => info);
define_ph_forwarded_macro!($ ph_forwarded_warn => warn);
define_ph_forwarded_macro!($ ph_forwarded_error => error);

pub mod events;

#[cfg(test)]
mod tests;
