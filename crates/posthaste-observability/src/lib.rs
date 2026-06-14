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

#[macro_export]
macro_rules! ph_trace {
    (target: $target:expr, $event:expr, $($fields:tt)+) => {
        $crate::tracing::trace!(target: $target, event = $crate::event_name($event), $($fields)+)
    };
    (parent: $parent:expr, $event:expr, $($fields:tt)+) => {
        $crate::tracing::trace!(parent: $parent, event = $crate::event_name($event), $($fields)+)
    };
    ($event:expr, $($fields:tt)+) => {
        $crate::tracing::trace!(event = $crate::event_name($event), $($fields)+)
    };
}

#[macro_export]
macro_rules! ph_debug {
    (target: $target:expr, $event:expr, $($fields:tt)+) => {
        $crate::tracing::debug!(target: $target, event = $crate::event_name($event), $($fields)+)
    };
    (parent: $parent:expr, $event:expr, $($fields:tt)+) => {
        $crate::tracing::debug!(parent: $parent, event = $crate::event_name($event), $($fields)+)
    };
    ($event:expr, $($fields:tt)+) => {
        $crate::tracing::debug!(event = $crate::event_name($event), $($fields)+)
    };
}

#[macro_export]
macro_rules! ph_info {
    (target: $target:expr, $event:expr, $($fields:tt)+) => {
        $crate::tracing::info!(target: $target, event = $crate::event_name($event), $($fields)+)
    };
    (parent: $parent:expr, $event:expr, $($fields:tt)+) => {
        $crate::tracing::info!(parent: $parent, event = $crate::event_name($event), $($fields)+)
    };
    ($event:expr, $($fields:tt)+) => {
        $crate::tracing::info!(event = $crate::event_name($event), $($fields)+)
    };
}

#[macro_export]
macro_rules! ph_warn {
    (target: $target:expr, $event:expr, $($fields:tt)+) => {
        $crate::tracing::warn!(target: $target, event = $crate::event_name($event), $($fields)+)
    };
    (parent: $parent:expr, $event:expr, $($fields:tt)+) => {
        $crate::tracing::warn!(parent: $parent, event = $crate::event_name($event), $($fields)+)
    };
    ($event:expr, $($fields:tt)+) => {
        $crate::tracing::warn!(event = $crate::event_name($event), $($fields)+)
    };
}

#[macro_export]
macro_rules! ph_error {
    (target: $target:expr, $event:expr, $($fields:tt)+) => {
        $crate::tracing::error!(target: $target, event = $crate::event_name($event), $($fields)+)
    };
    (parent: $parent:expr, $event:expr, $($fields:tt)+) => {
        $crate::tracing::error!(parent: $parent, event = $crate::event_name($event), $($fields)+)
    };
    ($event:expr, $($fields:tt)+) => {
        $crate::tracing::error!(event = $crate::event_name($event), $($fields)+)
    };
}

#[macro_export]
macro_rules! ph_forwarded_trace {
    (target: $target:expr, event: $event:expr, $($fields:tt)+) => {
        $crate::tracing::trace!(target: $target, event = $event, $($fields)+)
    };
}

#[macro_export]
macro_rules! ph_forwarded_debug {
    (target: $target:expr, event: $event:expr, $($fields:tt)+) => {
        $crate::tracing::debug!(target: $target, event = $event, $($fields)+)
    };
}

#[macro_export]
macro_rules! ph_forwarded_info {
    (target: $target:expr, event: $event:expr, $($fields:tt)+) => {
        $crate::tracing::info!(target: $target, event = $event, $($fields)+)
    };
}

#[macro_export]
macro_rules! ph_forwarded_warn {
    (target: $target:expr, event: $event:expr, $($fields:tt)+) => {
        $crate::tracing::warn!(target: $target, event = $event, $($fields)+)
    };
}

#[macro_export]
macro_rules! ph_forwarded_error {
    (target: $target:expr, event: $event:expr, $($fields:tt)+) => {
        $crate::tracing::error!(target: $target, event = $event, $($fields)+)
    };
}

pub mod events;

#[cfg(test)]
mod tests;
