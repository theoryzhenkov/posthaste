/// Provider policy for turning remote push/IDLE signals into local sync work.
///
/// Push transports only deliver hints. The policy records what remote surface
/// is observed and whether a hint must be followed by an account-level
/// observation rather than trusted as a complete change description.
///
/// @spec docs/L0-providers#imap-smtp-sync-strategy
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RemoteObservationPolicy {
    idle_scope: RemoteIdleScope,
    empty_hint: EmptyRemoteHintPolicy,
    hint_completeness: RemoteHintCompleteness,
}

impl RemoteObservationPolicy {
    pub fn account_push() -> Self {
        Self {
            idle_scope: RemoteIdleScope::Account,
            empty_hint: EmptyRemoteHintPolicy::Ignore,
            hint_completeness: RemoteHintCompleteness::Complete,
        }
    }

    pub fn selected_mailbox_idle() -> Self {
        Self {
            idle_scope: RemoteIdleScope::SelectedMailbox,
            empty_hint: EmptyRemoteHintPolicy::Sync,
            hint_completeness: RemoteHintCompleteness::Complete,
        }
    }

    pub fn disabled() -> Self {
        Self {
            idle_scope: RemoteIdleScope::None,
            empty_hint: EmptyRemoteHintPolicy::Ignore,
            hint_completeness: RemoteHintCompleteness::Complete,
        }
    }

    pub fn with_incomplete_hints(mut self) -> Self {
        self.hint_completeness = RemoteHintCompleteness::Incomplete;
        self
    }

    pub fn idle_scope(self) -> RemoteIdleScope {
        self.idle_scope
    }

    pub fn observes_empty_hints(self) -> bool {
        self.empty_hint == EmptyRemoteHintPolicy::Sync
    }

    pub fn treats_hints_as_incomplete(self) -> bool {
        self.hint_completeness == RemoteHintCompleteness::Incomplete
    }
}

impl Default for RemoteObservationPolicy {
    fn default() -> Self {
        Self::disabled()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RemoteIdleScope {
    None,
    Account,
    SelectedMailbox,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum EmptyRemoteHintPolicy {
    Ignore,
    Sync,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RemoteHintCompleteness {
    Complete,
    Incomplete,
}
