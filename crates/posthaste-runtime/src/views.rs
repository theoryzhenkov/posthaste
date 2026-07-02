//! The runtime's view **projection** parts (RFC D37): what a view *is*
//! (family-specific [`ViewKind`] identity, descriptor parsing, scope
//! validation), when a domain event affects it, and how its snapshot is
//! recomputed — base rows + coverage + the pending-outbox overlay → windowed
//! view state. Wire-agnostic: no frames, sessions or pagination here — the
//! serving half lives in the far-end grouping
//! ([`crate::far_end::view_registry`], RFC D39).

// projector: merges into link-replica projection layer (RFC D38, M9)

use posthaste_contract_core::{
    CoverageRange, MailListAnchorState, MailListContinuation, MailListProjectionKind,
    MailListRowState, MailListViewState, MailPresentationRequest, MailQueryPage, MailQueryRequest,
    ReadWatermark, RuntimeCoverage, RuntimeError, RuntimeErrorCode, ViewDescriptor, ViewId,
    ViewLifecycle, ViewRevision, ViewSnapshot,
};
use posthaste_domain_model::{
    AccountId, ConversationId, DomainEvent, MessageId, EVENT_TOPIC_ACCOUNT_CREATED,
    EVENT_TOPIC_ACCOUNT_DELETED, EVENT_TOPIC_ACCOUNT_STATUS_CHANGED, EVENT_TOPIC_ACCOUNT_UPDATED,
    EVENT_TOPIC_MESSAGE_UPDATED, EVENT_TOPIC_REV_LOG_APPENDED,
};
use serde::Deserialize;
use serde_json::{json, Value};

/// The parsed, family-specific identity of a runtime view. The registry is
/// generic over families: each carries what [`build_snapshot`] and the event
/// pump need, so adding a family is a new variant rather than new registry
/// machinery.
///
/// @spec docs/runtime/adapter/L1#view-descriptors
#[derive(Clone)]
pub(crate) enum ViewKind {
    MailList(MailQueryRequest),
    MessageDetail {
        source_id: String,
        message_id: String,
    },
    Conversation {
        conversation_id: String,
    },
    /// Folded account overview(s): `None` serves the full account list, `Some`
    /// serves one account. @spec docs/runtime/adapter/L2#account-status-views
    AccountStatus {
        account_id: Option<String>,
    },
    /// Phase 2 undo/redo: the per-account reversible-op log + cursor, mirrored
    /// to every device. @spec docs/eph/DESIGN-L2-undo-redo-revlog-contract
    RevLog {
        account_id: String,
    },
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct MessageDetailDescriptor {
    source_id: String,
    message_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConversationDescriptor {
    conversation_id: String,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct AccountStatusDescriptor {
    #[serde(default)]
    account_id: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RevLogDescriptor {
    account_id: String,
}

/// Recompute one view's snapshot: read the base through the near node's read
/// path, project it per family, and (for mail lists) fold the runtime→authority
/// server outbox over the served rows so views are optimistic over
/// forwarded-but-unconfirmed mutations.
pub(crate) async fn build_snapshot(
    reads: &crate::read::ReadCache,
    outbox: &crate::near_node::RuntimeAuthorityServerOutbox,
    view_id: ViewId,
    descriptor: ViewDescriptor,
    kind: &ViewKind,
    revision: ViewRevision,
) -> Result<ViewSnapshot, RuntimeError> {
    let (data, read_watermark, coverage) = match kind {
        ViewKind::MailList(request) => {
            let page = reads.query_mail_page(request.clone()).await?;
            let mut state = mail_list_state(request, page)?;
            // Fold the runtime→authority server outbox: served rows are optimistic
            // over forwarded-but-unconfirmed mutations. A no-op when the
            // outbox is empty (the in-process default), so co-located
            // behavior is unchanged (`colocated-unchanged`).
            crate::near_node::apply_outbox_overlay(&mut state, &outbox.snapshot());
            let read_watermark = state.read_watermark.clone();
            let coverage = state.coverage.clone();
            let data = serde_json::to_value(state)
                .map_err(|error| RuntimeError::new(RuntimeErrorCode::Internal, error.to_string()))?;
            (data, read_watermark, coverage)
        }
        ViewKind::MessageDetail {
            source_id,
            message_id,
        } => {
            // Body-free by construction (message_detail uses the header read);
            // the body is the separate sanitized `/body` lazy resource, so the
            // view never serves the (unsanitized) cached body.
            let detail = reads
                .message_detail(
                    &AccountId::from(source_id.clone()),
                    &MessageId::from(message_id.clone()),
                )
                .await?
                .ok_or_else(|| RuntimeError::not_found("message not found"))?;
            let data = serde_json::to_value(detail)
                .map_err(|error| RuntimeError::new(RuntimeErrorCode::Internal, error.to_string()))?;
            (data, local_watermark(), complete_coverage())
        }
        ViewKind::Conversation { conversation_id } => {
            let conversation = reads
                .conversation(&ConversationId::from(conversation_id.clone()))
                .await?;
            let data = serde_json::to_value(conversation)
                .map_err(|error| RuntimeError::new(RuntimeErrorCode::Internal, error.to_string()))?;
            (data, local_watermark(), complete_coverage())
        }
        ViewKind::AccountStatus { account_id } => {
            // Account status reads through the link (the same `reads` path
            // as every other view), so an authority-server-less runtime serves it too.
            let data = match account_id {
                Some(account_id) => {
                    let overview = reads.get_account(AccountId::from(account_id.clone())).await?;
                    serde_json::to_value(overview).map_err(|error| {
                        RuntimeError::new(RuntimeErrorCode::Internal, error.to_string())
                    })?
                }
                None => {
                    let list = reads.list_accounts().await?;
                    serde_json::to_value(list.items).map_err(|error| {
                        RuntimeError::new(RuntimeErrorCode::Internal, error.to_string())
                    })?
                }
            };
            (data, local_watermark(), complete_coverage())
        }
        ViewKind::RevLog { account_id } => {
            let snapshot = reads
                .rev_log_snapshot(&AccountId::from(account_id.clone()))
                .await?;
            let data = serde_json::to_value(snapshot)
                .map_err(|error| RuntimeError::new(RuntimeErrorCode::Internal, error.to_string()))?;
            (data, local_watermark(), complete_coverage())
        }
    };
    Ok(ViewSnapshot {
        view_id,
        descriptor,
        revision,
        lifecycle: ViewLifecycle::Ready,
        read_watermark,
        coverage,
        data,
        error: None,
    })
}

/// Parse a view descriptor into its family-specific [`ViewKind`].
pub(crate) fn parse_view_kind(descriptor: &ViewDescriptor) -> Result<ViewKind, RuntimeError> {
    match descriptor.family.as_str() {
        "mailList" => {
            let request = serde_json::from_value(descriptor.payload.clone()).map_err(|error| {
                RuntimeError::invalid_descriptor(format!("invalid mailList descriptor: {error}"))
            })?;
            Ok(ViewKind::MailList(request))
        }
        "messageDetail" => {
            let descriptor: MessageDetailDescriptor =
                serde_json::from_value(descriptor.payload.clone()).map_err(|error| {
                    RuntimeError::invalid_descriptor(format!(
                        "invalid messageDetail descriptor: {error}"
                    ))
                })?;
            Ok(ViewKind::MessageDetail {
                source_id: descriptor.source_id,
                message_id: descriptor.message_id,
            })
        }
        "conversation" => {
            let descriptor: ConversationDescriptor =
                serde_json::from_value(descriptor.payload.clone()).map_err(|error| {
                    RuntimeError::invalid_descriptor(format!(
                        "invalid conversation descriptor: {error}"
                    ))
                })?;
            Ok(ViewKind::Conversation {
                conversation_id: descriptor.conversation_id,
            })
        }
        "accountStatus" => {
            // An empty/absent payload is the all-accounts variant.
            let descriptor: AccountStatusDescriptor = if descriptor.payload.is_null() {
                AccountStatusDescriptor::default()
            } else {
                serde_json::from_value(descriptor.payload.clone()).map_err(|error| {
                    RuntimeError::invalid_descriptor(format!(
                        "invalid accountStatus descriptor: {error}"
                    ))
                })?
            };
            Ok(ViewKind::AccountStatus {
                account_id: descriptor.account_id,
            })
        }
        "revLog" => {
            let descriptor: RevLogDescriptor = serde_json::from_value(descriptor.payload.clone())
                .map_err(|error| {
                RuntimeError::invalid_descriptor(format!("invalid revLog descriptor: {error}"))
            })?;
            Ok(ViewKind::RevLog {
                account_id: descriptor.account_id,
            })
        }
        other => Err(RuntimeError::invalid_descriptor(format!(
            "unsupported view family '{other}'"
        ))),
    }
}

/// Grow a mail-query window by `count` rows so an extend re-queries the larger
/// first-N window (consistent with how event recompute already re-reads it).
pub(crate) fn grow_message_window(request: &mut MailQueryRequest, count: usize) {
    match &mut request.presentation {
        MailPresentationRequest::Messages { limit, .. } => {
            *limit = Some(limit.unwrap_or(0).saturating_add(count));
        }
        MailPresentationRequest::CollapsedByConversation { limit, .. } => {
            *limit = limit.saturating_add(count);
        }
    }
}

fn local_watermark() -> Option<ReadWatermark> {
    Some(ReadWatermark {
        value: "local".to_string(),
    })
}

fn complete_coverage() -> RuntimeCoverage {
    // A single range spanning TOP..BOTTOM — the degenerate complete case, used
    // for single-object views (message detail, conversation) and the all-accounts
    // status, which genuinely hold their whole result.
    RuntimeCoverage {
        ranges: vec![CoverageRange {
            from: None,
            to: None,
        }],
    }
}

/// Whether a `message.updated` event can change which rows a mail list shows or
/// their order: a keyword or mailbox-membership change, a newly arrived message,
/// or a deletion. The diff payload always carries the `changes` flags and
/// `created`; deletion events carry `deleted`.
fn message_event_affects_list(event: &DomainEvent) -> bool {
    let changes = &event.payload["changes"];
    changes["keywords"] == true
        || changes["mailboxes"] == true
        || event.payload["created"] == true
        || event.payload["deleted"] == true
}

/// Whether a domain event should trigger a recompute for a view of this kind.
/// mailList recomputes when message membership/ordering may change (keyword
/// assertions); messageDetail recomputes on any update to its own message.
pub(crate) fn event_affects_view(kind: &ViewKind, event: &DomainEvent) -> bool {
    match kind {
        // A mail list is derived from message membership, ordering, and keyword
        // state. It must recompute on every change that can add, remove, or
        // reorder a row: keyword assertions (read/flag), mailbox membership
        // (archive/move/trash), arrival of a new message, and deletion. The
        // earlier keywords-only gate dropped archives and new arrivals, so the
        // list went stale until the view was reopened. The snapshot equality
        // check in `recompute_view_if_changed` suppresses no-op recomputes, so a
        // broad trigger costs at most a wasted query.
        ViewKind::MailList(_) => {
            event.topic == EVENT_TOPIC_MESSAGE_UPDATED && message_event_affects_list(event)
        }
        ViewKind::MessageDetail {
            source_id,
            message_id,
        } => {
            event.topic == EVENT_TOPIC_MESSAGE_UPDATED
                && event.account_id.as_str() == source_id
                && event.message_id.as_ref().map(MessageId::as_str) == Some(message_id.as_str())
        }
        // Conversations are derived from messages; recompute on any message
        // update and let the data-equality check suppress no-op replacements.
        ViewKind::Conversation { .. } => event.topic == EVENT_TOPIC_MESSAGE_UPDATED,
        // Account config + runtime status events; the all-accounts variant
        // recomputes on any account event, the per-account variant on its own.
        // Data-equality suppression elides no-op recomputes.
        ViewKind::AccountStatus { account_id } => {
            let is_account_event = matches!(
                event.topic.as_str(),
                EVENT_TOPIC_ACCOUNT_STATUS_CHANGED
                    | EVENT_TOPIC_ACCOUNT_CREATED
                    | EVENT_TOPIC_ACCOUNT_UPDATED
                    | EVENT_TOPIC_ACCOUNT_DELETED
            );
            is_account_event
                && account_id
                    .as_ref()
                    .is_none_or(|id| event.account_id.as_str() == id)
        }
        // Phase 2: the RevLog view recomputes on `rev_log.appended` for its
        // account (a forward action confirmed + appended) — not on the
        // `message.updated` firehose (a per-message update would needlessly
        // re-fetch the whole log). Cursor moves (Slice 4) emit the same topic.
        ViewKind::RevLog { account_id } => {
            event.topic == EVENT_TOPIC_REV_LOG_APPENDED && event.account_id.as_str() == account_id
        }
    }
}

pub(crate) fn validate_kind_account_scope(
    kind: &ViewKind,
    account_scope: Option<&[String]>,
) -> Result<(), RuntimeError> {
    let Some(account_scope) = account_scope else {
        return Ok(());
    };
    let in_scope = match kind {
        ViewKind::MailList(request) => {
            account_scope.is_empty()
                || account_scope
                    .iter()
                    .any(|source_id| mail_query_contains_source_scope(&request.query, source_id))
        }
        ViewKind::MessageDetail { source_id, .. } => {
            account_scope.is_empty() || account_scope.iter().any(|id| id == source_id)
        }
        // The conversation id is opaque (it does not name an account); access is
        // gated at the API capability layer. A finer runtime-side scope check
        // would require reading the conversation first.
        ViewKind::Conversation { .. } => true,
        // The all-accounts view is global (settings shows every account); a
        // per-account view must be in scope.
        ViewKind::AccountStatus { account_id } => match account_id {
            None => true,
            Some(account_id) => {
                account_scope.is_empty() || account_scope.iter().any(|id| id == account_id)
            }
        },
        ViewKind::RevLog { account_id } => {
            account_scope.is_empty() || account_scope.iter().any(|id| id == account_id)
        }
    };
    if in_scope {
        return Ok(());
    }
    Err(RuntimeError::invalid_descriptor(
        "view descriptor is outside the caller account scope",
    ))
}

fn mail_query_contains_source_scope(query: &str, source_id: &str) -> bool {
    query.split_whitespace().any(|token| {
        let token = token
            .trim_start_matches('!')
            .trim_start_matches('-')
            .trim_start_matches('+');
        let Some(selector) = token
            .strip_prefix("in:")
            .or_else(|| token.strip_prefix("IN:"))
        else {
            return false;
        };
        selector
            .split_once('/')
            .is_some_and(|(account, _mailbox)| account == source_id)
    })
}

fn mail_list_state(
    request: &MailQueryRequest,
    page: MailQueryPage,
) -> Result<MailListViewState, RuntimeError> {
    let MailQueryPage::Messages(page) = page else {
        return Err(RuntimeError::invalid_descriptor(
            "mailList views require a message presentation",
        ));
    };
    let rows = page
        .items
        .iter()
        .enumerate()
        .map(|(index, message)| {
            let projection = serde_json::to_value(message).unwrap_or(Value::Null);
            MailListRowState {
                row_key: format!("{}:{}", message.source_id.as_str(), message.id.as_str()),
                resource_ref: Some(format!(
                    "message:{}:{}",
                    message.source_id.as_str(),
                    message.id.as_str()
                )),
                sort_key: json!([message.received_at, message.id.as_str()]),
                projection,
                order_key: format!("{index:08}"),
            }
        })
        .collect();
    // Honest coverage over the query's sort order: a top-anchored window from
    // TOP down to the last held row's sort key, reaching BOTTOM only when there
    // is no next page. Replaces the hardcoded `Complete` that could not
    // distinguish "absent because unchanged" from "absent because not held."
    let has_after = page.next_cursor.is_some();
    let coverage = if has_after {
        let to = page
            .items
            .last()
            .map(|message| json!([message.received_at, message.id.as_str()]));
        RuntimeCoverage {
            ranges: vec![CoverageRange { from: None, to }],
        }
    } else {
        complete_coverage()
    };
    Ok(MailListViewState {
        scope: json!({ "query": request.query }),
        projection_kind: MailListProjectionKind::Message,
        sort: presentation_sort(&request.presentation),
        window_request: presentation_window(&request.presentation),
        rows,
        continuation: MailListContinuation {
            before_cursor: None,
            after_cursor: page
                .next_cursor
                .as_ref()
                .and_then(|cursor| serde_json::to_string(cursor).ok()),
            has_before: false,
            has_after: page.next_cursor.is_some(),
        },
        read_watermark: Some(ReadWatermark {
            value: "local".to_string(),
        }),
        coverage,
        known_total_count: None,
        anchor: MailListAnchorState::NotRequested,
    })
}

fn presentation_sort(presentation: &MailPresentationRequest) -> Value {
    match presentation {
        MailPresentationRequest::Messages {
            sort_field,
            sort_direction,
            ..
        } => json!({ "field": sort_field, "direction": sort_direction }),
        MailPresentationRequest::CollapsedByConversation {
            sort_field,
            sort_direction,
            ..
        } => json!({ "field": sort_field, "direction": sort_direction }),
    }
}

fn presentation_window(presentation: &MailPresentationRequest) -> Value {
    match presentation {
        MailPresentationRequest::Messages { limit, cursor, .. } => {
            json!({ "limit": limit, "cursor": cursor })
        }
        MailPresentationRequest::CollapsedByConversation { limit, cursor, .. } => {
            json!({ "limit": limit, "cursor": cursor })
        }
    }
}

#[cfg(test)]
mod recompute_trigger_tests {
    use super::*;
    use posthaste_domain_model::EVENT_TOPIC_MESSAGE_UPDATED;
    use serde_json::json;

    fn message_event(payload: serde_json::Value) -> DomainEvent {
        DomainEvent {
            seq: 1,
            account_id: AccountId::from("acct"),
            topic: EVENT_TOPIC_MESSAGE_UPDATED.to_string(),
            occurred_at: "2026-06-23T00:00:00Z".to_string(),
            mailbox_id: None,
            message_id: Some(MessageId::from("m1")),
            payload,
        }
    }

    #[test]
    fn keyword_change_affects_list() {
        assert!(message_event_affects_list(&message_event(
            json!({ "changes": { "keywords": true, "mailboxes": false } })
        )));
    }

    #[test]
    fn mailbox_change_affects_list() {
        // Archive / move: membership changed, keywords did not. Regression guard
        // for the keywords-only gate that left archived rows in the list.
        assert!(message_event_affects_list(&message_event(
            json!({ "changes": { "keywords": false, "mailboxes": true } })
        )));
    }

    #[test]
    fn new_message_affects_list() {
        assert!(message_event_affects_list(&message_event(
            json!({ "created": true, "changes": { "keywords": true, "mailboxes": true } })
        )));
    }

    #[test]
    fn deletion_affects_list() {
        assert!(message_event_affects_list(&message_event(
            json!({ "deleted": true })
        )));
    }

    #[test]
    fn unrelated_payload_does_not_affect_list() {
        assert!(!message_event_affects_list(&message_event(
            json!({ "changes": { "keywords": false, "mailboxes": false } })
        )));
    }
}
