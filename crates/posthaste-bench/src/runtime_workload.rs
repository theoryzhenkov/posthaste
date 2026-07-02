//! Runtime-tier profiling workload.
//!
//! The [`crate::workloads`] module bottoms out at `posthaste-store`. This module
//! drives the **full co-located application path** — the authority runtime's
//! session/view machinery, the named-mutation pipeline, and the view recompute
//! that produces a frame — so the flamegraph harness can see the
//! mutation -> view-recompute -> frame hot path the runtime<->authority server "link bus"
//! introduced, not just the store floor.
//!
//! It uses the **co-located in-process default** (an `AccountDriver::Mock`
//! account, the same offline path `build_connection` selects for a mock driver),
//! so it reproduces the bundled-application configuration the user actually runs.
//! Everything is offline and deterministic: no network, temp roots only.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use futures_util::StreamExt;
use posthaste_authority_server::build_authority_server;
use posthaste_domain_service::{
    AccountDriver, AccountId, MessageId, MessageSortField, SecretRef, SecretStore,
    SecretStoreError, SetKeywordsCommand, SortDirection,
};
use posthaste_client_link::{RuntimeFrameSubscription, RuntimeLink};
use posthaste_contract_core::{
    AccountTransportMutation, CreateAccountMutation, MailListViewState, MailPresentationRequest,
    MailQueryRequest, RuntimeCaller, RuntimeFrame, RuntimeSessionId, RuntimeSessionSeq,
    SecretWriteMutation, ViewDescriptor, ViewId,
};
use posthaste_runtime::RuntimeBuildConfig;
use posthaste_runtime_api::RuntimeAccountApi;
use tempfile::TempDir;

use crate::workloads;

/// The synthetic account whose inbox is profiled.
const ACCOUNT: &str = "bench-account";
/// Mail-list window size (a full page; the recompute cost scales with it).
const PAGE_LIMIT: usize = 50;

/// A ready-to-drive runtime fixture: a built authority runtime with a Mock
/// account, a seeded inbox, and an open mail-list view + frame subscription.
/// Holds the temp dir and the runtime build so they live as long as the fixture.
pub struct RuntimeInbox {
    // Field order matters for drop: the runtime build shuts down its tasks when
    // dropped; the temp dir is removed last.
    build: posthaste_authority_server::AuthorityServerBuild,
    account_id: AccountId,
    session_id: RuntimeSessionId,
    view_id: ViewId,
    subscription: RuntimeFrameSubscription,
    /// A message id known to be inside the view window, toggled each iteration.
    visible_id: String,
    /// Alternates the keyword add/remove so every iteration changes view state
    /// (and therefore forces a real recompute + frame).
    toggle: bool,
    _temp: TempDir,
}

/// Build the runtime, create a Mock account, seed `message_count` synthetic
/// messages into its inbox, then open a mail-list view + frame subscription.
pub async fn open_runtime_inbox(message_count: usize) -> Result<RuntimeInbox> {
    let temp = TempDir::new().context("create temp dir")?;
    let config = RuntimeBuildConfig::new(
        temp.path().join("config"),
        temp.path().join("state"),
        temp.path().join("cache"),
    )
    .with_secret_store(Arc::new(InMemorySecretStore::default()))
    // Bound the broadcast so a long profiling loop cannot lag the frame stream.
    .with_event_channel_capacity(16_384)
    // Suppress background re-sync (a mock sync would otherwise replace the seed).
    .with_poll_interval(Duration::from_secs(86_400));

    let build = build_authority_server(config)
        .await
        .map_err(|error| anyhow!("build authority runtime: {error}"))?;

    let account = build
        .handle
        .create_account(RuntimeCaller::test(), mock_account_mutation(ACCOUNT))
        .await
        .map_err(|error| anyhow!("create account: {error}"))?;

    // Bring the mock account runtime up so forward_mutation routes through it.
    build
        .account_supervisor
        .sync_account(&account.id)
        .await
        .map_err(|error| anyhow!("sync account: {error}"))?;

    // Seed a realistic inbox directly into the store (reuse the store fixtures).
    let batch = workloads::sync_batch(workloads::synthetic_messages(message_count));
    build
        .api_bridge
        .store
        .apply_sync_batch(&account.id, &batch)
        .map_err(|error| anyhow!("seed inbox: {error}"))?;

    let session = build
        .handle
        .open_session(RuntimeCaller::test())
        .await
        .map_err(|error| anyhow!("open session: {error}"))?;

    let snapshot = build
        .handle
        .open_session_view(
            RuntimeCaller::test(),
            session.session_id.clone(),
            mail_list_descriptor(&format!("in:{ACCOUNT}/inbox"), PAGE_LIMIT),
        )
        .await
        .map_err(|error| anyhow!("open mail-list view: {error}"))?;

    let state: MailListViewState =
        serde_json::from_value(snapshot.data.clone()).context("decode mail-list view state")?;
    let visible_id = state
        .rows
        .first()
        .and_then(|row| row.projection.get("id"))
        .and_then(|id| id.as_str())
        .ok_or_else(|| anyhow!("seeded inbox view returned no visible rows"))?
        .to_string();

    let subscription = build
        .handle
        .subscribe_runtime_frames(
            RuntimeCaller::test(),
            session.session_id.clone(),
            Some(RuntimeSessionSeq::new(0)),
        )
        .await
        .map_err(|error| anyhow!("subscribe runtime frames: {error}"))?;

    Ok(RuntimeInbox {
        account_id: account.id,
        session_id: session.session_id.clone(),
        view_id: snapshot.view_id.clone(),
        subscription,
        visible_id,
        toggle: false,
        build,
        _temp: temp,
    })
}

/// THE PER-EVENT COST AFTER option iii: write a keyword change to the store and
/// broadcast the event, then drain until the `message.updated` **notification**
/// (the firehose) lands. This is what an affecting event now costs the runtime
/// for a mail-list view: a store write + an event-bus forward — NO per-event
/// `build_snapshot`/recompute (the client self-maintains the view from the
/// firehose, single-source-view-membership). Pair its iterations/3s with
/// [`recompute_and_await_view`] (the recompute cost option iii removed per event)
/// to read the win.
///
/// Awaits the notification, not a view frame: post-option-iii the runtime emits
/// no `ViewReplace`/`ViewDelta` for a mail-list on a keyword change, so the old
/// "drain until view frame" would hang.
///
/// It triggers via `store.set_keywords` + manual event broadcast (the pattern
/// the runtime's own `authority_server_handle` tests use) rather than
/// `forward_mutation`: the full named-mutation pipeline's existence check
/// (`get_message_mailboxes`) rejects bulk-seeded messages, and that pipeline is
/// upstream of — not part of — the path we are profiling.
pub async fn mutate_and_await_view(inbox: &mut RuntimeInbox, _index: usize) -> Result<()> {
    inbox.toggle = !inbox.toggle;
    // Alternate add/remove so every iteration changes view state and forces a
    // real recompute + frame (an idempotent op would yield no frame).
    let command = if inbox.toggle {
        SetKeywordsCommand {
            add: vec!["$flagged".to_string()],
            remove: Vec::new(),
        }
    } else {
        SetKeywordsCommand {
            add: Vec::new(),
            remove: vec!["$flagged".to_string()],
        }
    };

    let ack = inbox
        .build
        .api_bridge
        .store
        .set_keywords(
            &inbox.account_id,
            &MessageId::from(inbox.visible_id.clone()),
            None,
            &command,
        )
        .map_err(|error| anyhow!("set_keywords: {error}"))?;
    for event in ack.events {
        inbox
            .build
            .api_bridge
            .event_sender
            .send(event)
            .map_err(|error| anyhow!("broadcast event: {error}"))?;
    }

    // Drain frames until the message.updated notification (the client's
    // self-maintenance input) lands. No view frame arrives for a mail-list under
    // option iii.
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let frame = inbox
                .subscription
                .live
                .next()
                .await
                .ok_or_else(|| anyhow!("runtime frame stream closed"))?;
            if matches!(
                frame,
                RuntimeFrame::Notification { ref kind, .. } if kind == "message.updated"
            ) {
                return Ok::<(), anyhow::Error>(());
            }
        }
    })
    .await
    .map_err(|_| anyhow!("timed out awaiting message.updated notification"))?
}

/// THE COST option iii REMOVED per event: force one mail-list view recompute +
/// frame — exactly the `build_snapshot` (query_mail_page -> `serde_json::to_value`
/// of the whole state -> `mail_list_delta`) the runtime USED to run on every
/// affecting `message.updated` per open mail-list view, and no longer does.
///
/// Measured via `extend_session_view` by **0 rows**: a fixed-window recompute
/// (same window as the event pump's `recompute_view_if_changed`) that always
/// emits a `ViewReplace`/`ViewDelta`, so its iterations/3s is the per-event
/// runtime cost option iii eliminated. (The extend path is retained — pagination;
/// only the per-event re-serve was retired.)
pub async fn recompute_and_await_view(inbox: &mut RuntimeInbox, _index: usize) -> Result<()> {
    inbox
        .build
        .handle
        .extend_session_view(
            RuntimeCaller::test(),
            inbox.session_id.clone(),
            inbox.view_id.clone(),
            0,
        )
        .await
        .map_err(|error| anyhow!("extend_session_view: {error}"))?;

    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let frame = inbox
                .subscription
                .live
                .next()
                .await
                .ok_or_else(|| anyhow!("runtime frame stream closed"))?;
            if matches!(
                frame,
                RuntimeFrame::ViewReplace { .. } | RuntimeFrame::ViewDelta { .. }
            ) {
                return Ok::<(), anyhow::Error>(());
            }
        }
    })
    .await
    .map_err(|_| anyhow!("timed out awaiting view recompute frame"))?
}

/// A Mock-driver account mutation (offline; no secret/transport needed).
fn mock_account_mutation(account_id: &str) -> CreateAccountMutation {
    CreateAccountMutation {
        id: Some(account_id.to_string()),
        name: account_id.to_string(),
        driver: Some(AccountDriver::Mock),
        // Enabled so forward_mutation routes through the account runtime.
        enabled: Some(true),
        full_name: None,
        signature: None,
        email_patterns: Vec::new(),
        appearance: None,
        transport: AccountTransportMutation::default(),
        secret: SecretWriteMutation::default(),
    }
}

/// A mail-list view descriptor over `query` with a `limit`-row window.
fn mail_list_descriptor(query: &str, limit: usize) -> ViewDescriptor {
    let request = MailQueryRequest {
        query: query.to_string(),
        presentation: MailPresentationRequest::Messages {
            limit: Some(limit),
            cursor: None,
            sort_field: MessageSortField::Date,
            sort_direction: SortDirection::Desc,
        },
        visibility: None,
    };
    ViewDescriptor {
        family: "mailList".to_string(),
        payload: serde_json::to_value(request).expect("mail query request should serialize"),
        // Self-maintained: the bench measures the post-option-iii per-event
        // cost (notification only, no re-serve) for `runtime_mutation_notify`,
        // and the eliminated recompute separately via `runtime_view_recompute`.
        client_self_maintained: true,
    }
}

/// A trivial in-memory secret store. The mock driver never resolves secrets, but
/// the runtime build expects a store to be present.
#[derive(Default)]
struct InMemorySecretStore {
    values: Mutex<HashMap<String, String>>,
}

impl SecretStore for InMemorySecretStore {
    fn resolve(&self, secret_ref: &SecretRef) -> Result<String, SecretStoreError> {
        self.values
            .lock()
            .expect("secret store mutex")
            .get(&secret_key(secret_ref))
            .cloned()
            .ok_or_else(|| SecretStoreError::Unavailable("secret not found".to_string()))
    }

    fn save(&self, secret_ref: &SecretRef, value: &str) -> Result<(), SecretStoreError> {
        self.values
            .lock()
            .expect("secret store mutex")
            .insert(secret_key(secret_ref), value.to_string());
        Ok(())
    }

    fn update(&self, secret_ref: &SecretRef, value: &str) -> Result<(), SecretStoreError> {
        self.save(secret_ref, value)
    }

    fn delete(&self, secret_ref: &SecretRef) -> Result<(), SecretStoreError> {
        self.values
            .lock()
            .expect("secret store mutex")
            .remove(&secret_key(secret_ref));
        Ok(())
    }
}

fn secret_key(secret_ref: &SecretRef) -> String {
    format!("{:?}:{}", secret_ref.kind, secret_ref.key)
}
