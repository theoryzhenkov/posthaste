use posthaste_domain::{GatewayError, MutationOutcome, SyncCursor, SyncObject};

use super::MockState;

pub(super) fn mutation_outcome(state: &MockState, object_type: SyncObject) -> MutationOutcome {
    let prefix = object_type.as_str();
    MutationOutcome {
        cursor: Some(SyncCursor {
            object_type,
            state: format!("{prefix}-{}", state.revision),
            updated_at: "2026-03-31T10:00:00Z".to_string(),
        }),
        message: None,
    }
}

pub(super) fn ensure_expected_state(
    state: &MockState,
    expected_state: Option<&str>,
    object_type: SyncObject,
) -> Result<(), GatewayError> {
    if let Some(expected_state) = expected_state {
        let current = format!("{}-{}", object_type.as_str(), state.revision);
        if expected_state != current {
            return Err(GatewayError::StateMismatch);
        }
    }
    Ok(())
}

pub(super) fn validate_mailbox_role(role: Option<&str>) -> Result<(), GatewayError> {
    match role {
        None | Some("archive") | Some("drafts") | Some("inbox") | Some("junk") | Some("sent")
        | Some("trash") => Ok(()),
        Some(other) => Err(GatewayError::Rejected(format!(
            "unsupported mailbox role: {other}"
        ))),
    }
}

/// Advance the mock revision counter to simulate a new JMAP state string.
pub(super) fn bump_revision(state: &mut MockState) {
    state.revision += 1;
}
