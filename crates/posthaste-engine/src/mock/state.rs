use posthaste_domain_model::{
    GatewayError, MessageId, MessageReadback, MutationOutcome, SyncCursor, SyncObject,
};

use super::MockState;

/// Test hook: if `message_id` is marked for rejection, return `MutationRejected`
/// carrying the unchanged record as the readback (the provider rejected the set
/// but the get reads current state).
pub(super) fn reject_if_marked(
    state: &MockState,
    message_id: &MessageId,
) -> Result<(), GatewayError> {
    if state.rejected.contains(message_id) {
        let record = state
            .messages
            .iter()
            .find(|message| &message.id == message_id)
            .cloned()
            .ok_or_else(|| GatewayError::Rejected("unknown message".to_string()))?;
        return Err(GatewayError::MutationRejected {
            readback: Box::new(MessageReadback::Present(record)),
            reason: "mock rejected the mutation".to_string(),
        });
    }
    Ok(())
}

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
