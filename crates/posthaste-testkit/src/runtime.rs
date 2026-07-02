//! In-process authority runtime harness + view-settlement recorder.
//!
//! [`Harness::with_runtime`](crate::Harness::with_runtime) builds an authority
//! runtime against disposable roots; [`RuntimeHarness::settle`] drives a
//! mutation and captures the ordered `RuntimeFrame` stream through settlement,
//! which [`ViewSettlement`] asserts on. This is the layer that catches missed /
//! over-broad view recomputes.

use std::collections::{BTreeSet, HashMap};
use std::marker::PhantomData;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use tokio::sync::broadcast;

use posthaste_authority_runtime::AuthorityRuntimeBuild;
use posthaste_domain_service::{
    AccountDriver, AccountId, DomainEvent, MailStore, MailboxId, MailboxRecord, MessageRecord,
    ProviderAuthKind, ProviderHint, SecretRef, SecretStore, SecretStoreError, SyncBatch,
    SyncCursor, SyncObject,
};
use posthaste_client_link::{RuntimeFrameSubscription, RuntimeLinkOps};
use posthaste_contract_core::{
    AccountTransportMutation, ClientMutationId, CreateAccountMutation, MutationNotification,
    MutationReceipt, MutationRequest, RuntimeCaller, RuntimeFrame, RuntimeSessionSeq,
    SecretWriteMode, SecretWriteMutation, ViewDescriptor, ViewId, ViewSnapshot,
};
use posthaste_runtime::RuntimeHandle;
use posthaste_runtime_api::RuntimeAccountApi;

use crate::fixture::{Fixture, FixtureAccount, FixtureDriver, FixtureError, FixtureMessage};

/// Drain deadline for a mutation to settle + its view to recompute.
const SETTLE_TIMEOUT: Duration = Duration::from_secs(3);
/// Extra window after settlement to catch redundant recomputes.
const SETTLE_GRACE: Duration = Duration::from_millis(80);

/// An in-process authority runtime on disposable roots.
///
/// Built by [`Harness::with_runtime`](crate::Harness::with_runtime). Exposes
/// the [`RuntimeApi`](posthaste_runtime_api::RuntimeApi) handle, the
/// store (for direct seeding), the event bus, and the
/// [`settle`](Self::settle) recorder.
pub struct RuntimeHarness {
    build: AuthorityRuntimeBuild,
}

impl RuntimeHarness {
    pub(crate) fn new(build: AuthorityRuntimeBuild) -> Self {
        Self { build }
    }

    /// The cloneable runtime handle (implements the runtime-api + client-link surfaces).
    pub fn core(&self) -> RuntimeHandle {
        self.build.handle.clone()
    }

    /// The runtime's store, for direct seeding via `apply_sync_batch` etc.
    pub fn store(&self) -> Arc<dyn MailStore> {
        self.build.api_bridge.store.clone()
    }

    /// The runtime's domain-event bus (cloneable sender).
    pub fn event_sender(&self) -> broadcast::Sender<DomainEvent> {
        self.build.api_bridge.event_sender.clone()
    }

    /// Drive one synchronous sync cycle for an account and await its result.
    /// Used to re-sync after mutating an external fixture (e.g. delivering a
    /// QRESYNC delta to a [`GmailImapFixture`](crate::GmailImapFixture)).
    pub async fn sync_account(&self, account_id: &AccountId) -> usize {
        self.build
            .account_supervisor
            .sync_account(account_id)
            .await
            .expect("account should sync")
    }

    /// Create a mock-driver account, enable it, and sync it so its runtime is
    /// live. Returns the account id.
    pub async fn create_mock_account(&self, id: &str) -> AccountId {
        let mutation = CreateAccountMutation {
            id: Some(id.to_string()),
            name: id.to_string(),
            driver: Some(AccountDriver::Mock),
            enabled: Some(true),
            full_name: None,
            signature: None,
            email_patterns: Vec::new(),
            appearance: None,
            transport: AccountTransportMutation::default(),
            secret: SecretWriteMutation::default(),
        };
        let account = self
            .build
            .handle
            .create_account(RuntimeCaller::test(), mutation)
            .await
            .expect("mock account should create");
        self.build
            .account_supervisor
            .sync_account(&account.id)
            .await
            .expect("mock account should sync");
        account.id
    }

    /// Create a JMAP account pointed at a [`StalwartFixture`], enabled, with its
    /// password written to the secret store, and run an initial sync (which
    /// establishes push + fetches seeded mail). The app's real sync path then
    /// observes later [`StalwartFixture::inject`] deliveries.
    pub async fn create_jmap_account(
        &self,
        id: &str,
        stalwart: &crate::StalwartFixture,
    ) -> AccountId {
        let mutation = CreateAccountMutation {
            id: Some(id.to_string()),
            name: id.to_string(),
            driver: Some(AccountDriver::Jmap),
            enabled: Some(true),
            full_name: Some("Dev Account".to_string()),
            signature: None,
            email_patterns: vec![stalwart.email()],
            appearance: None,
            transport: AccountTransportMutation {
                provider: Some(ProviderHint::Generic),
                auth: Some(ProviderAuthKind::Password),
                base_url: Some(stalwart.http_url.clone()),
                username: Some("dev".to_string()),
                imap: None,
                smtp: None,
            },
            secret: SecretWriteMutation {
                mode: SecretWriteMode::Replace,
                password: Some(stalwart.password.clone()),
            },
        };
        let account = self
            .build
            .handle
            .create_account(RuntimeCaller::test(), mutation)
            .await
            .expect("jmap account should create");
        self.build
            .account_supervisor
            .sync_account(&account.id)
            .await
            .expect("jmap account should sync");
        account.id
    }

    /// Seed `(message_id, mailbox_id)` pairs into an account via a direct store
    /// batch (bypasses sync — for unit/integration setup). Convenience wrapper
    /// around [`seed_messages_typed`](Self::seed_messages_typed) for specs with
    /// no field overrides.
    pub fn seed_messages(&self, account_id: &AccountId, messages: &[(&str, &str)]) {
        let typed: Vec<FixtureMessage> = messages
            .iter()
            .map(|(id, mailbox)| FixtureMessage {
                id: (*id).to_string(),
                mailbox: (*mailbox).to_string(),
                subject: None,
                from_name: None,
                from_email: None,
                preview: None,
                received_at: None,
                size: None,
                keywords: None,
                thread_id: None,
                rfc_message_id: None,
            })
            .collect();
        self.seed_messages_typed(account_id, typed);
    }

    /// Seed typed fixture messages into an account via a direct store batch
    /// (bypasses sync). Each message's declared fields override the
    /// [`default_message`](crate::fixture::default_message) baseline.
    pub fn seed_messages_typed(&self, account_id: &AccountId, messages: Vec<FixtureMessage>) {
        let mailbox_ids: BTreeSet<&str> = messages.iter().map(|m| m.mailbox.as_str()).collect();
        // The mailbox INSERT in apply_sync_batch persists only
        // (account_id, id, name, role); unread_emails/total_emails are
        // SQL-trigger-maintained from message rows and read directly by
        // list_mailboxes, so the values set here are informational only.
        let mailboxes: Vec<MailboxRecord> = mailbox_ids
            .iter()
            .map(|mb| MailboxRecord {
                id: MailboxId::from(*mb),
                name: mb.to_string(),
                role: Some((*mb).to_string()),
                unread_emails: 0,
                total_emails: messages.iter().filter(|m| m.mailbox == *mb).count() as i64,
            })
            .collect();
        let msgs: Vec<MessageRecord> = messages
            .into_iter()
            .map(FixtureMessage::into_record)
            .collect();
        let batch = SyncBatch {
            mailboxes,
            messages: msgs,
            imap_mailbox_states: Vec::new(),
            imap_message_locations: Vec::new(),
            deleted_imap_message_locations: Vec::new(),
            deleted_mailbox_ids: Vec::new(),
            deleted_message_ids: Vec::new(),
            replace_all_mailboxes: false,
            replace_all_messages: false,
            cursors: vec![SyncCursor {
                object_type: SyncObject::Message,
                state: "testkit-seed".to_string(),
                updated_at: "2026-03-31T10:00:00Z".to_string(),
            }],
        };
        self.build
            .api_bridge
            .store
            .apply_sync_batch(account_id, &batch)
            .expect("seed batch should apply");
    }

    /// Load a declarative TOML [`Fixture`](crate::fixture::Fixture) from a
    /// string, creating each account and seeding its messages. Returns the
    /// created account ids in declaration order. Only `driver = "mock"` is
    /// supported; JMAP / provider-state fixtures land with the live read-path.
    pub async fn load_fixture_toml(&self, toml: &str) -> Result<Vec<AccountId>, FixtureError> {
        let fixture = Fixture::parse(toml)?;
        let mut accounts = Vec::with_capacity(fixture.accounts.len());
        for account in fixture.accounts {
            let id = self.load_fixture_account(account).await?;
            accounts.push(id);
        }
        Ok(accounts)
    }

    /// Load a declarative TOML fixture from a file. See
    /// [`load_fixture_toml`](Self::load_fixture_toml).
    pub async fn load_fixture(
        &self,
        path: impl AsRef<std::path::Path>,
    ) -> Result<Vec<AccountId>, FixtureError> {
        let contents = std::fs::read_to_string(path)?;
        self.load_fixture_toml(&contents).await
    }

    async fn load_fixture_account(
        &self,
        account: FixtureAccount,
    ) -> Result<AccountId, FixtureError> {
        match account.driver {
            FixtureDriver::Mock => {
                let id = self.create_mock_account(&account.id).await;
                if !account.messages.is_empty() {
                    self.seed_messages_typed(&id, account.messages);
                }
                Ok(id)
            }
            FixtureDriver::Jmap => Err(FixtureError::UnsupportedDriver { driver: "jmap" }),
        }
    }

    /// Open a session + view, subscribe to the runtime frame stream, run a
    /// mutation, and drain the ordered frames through settlement and the view's
    /// recompute. `mutation.session_id` is set to the opened session.
    pub async fn settle(
        &self,
        mut mutation: MutationRequest,
        view: ViewDescriptor,
    ) -> ViewSettlement {
        let caller = RuntimeCaller::test();
        let session = self
            .build
            .handle
            .open_session(caller.clone())
            .await
            .expect("session should open");
        mutation.session_id = Some(session.session_id.clone());

        let snapshot = self
            .build
            .handle
            .open_session_view(caller.clone(), session.session_id.clone(), view)
            .await
            .expect("session view should open");
        let view_id = snapshot.view_id.clone();

        let mut subscription = self
            .build
            .handle
            .subscribe_runtime_frames(
                caller.clone(),
                session.session_id.clone(),
                Some(RuntimeSessionSeq::new(0)),
            )
            .await
            .expect("runtime stream should subscribe");

        let receipt = self
            .build
            .handle
            .run_mutation(caller.clone(), mutation)
            .await
            .expect("mutation should run");
        let client_mutation_id = receipt.client_mutation_id.clone();

        let mut frames: Vec<RuntimeFrame> = std::mem::take(&mut subscription.catch_up);
        let mut saw_confirmed = false;
        let mut saw_recompute = false;
        let deadline = Instant::now() + SETTLE_TIMEOUT;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                break;
            }
            match tokio::time::timeout(remaining, subscription.live.next()).await {
                Ok(Some(frame)) => {
                    if is_mutation_notification(&frame, &client_mutation_id) {
                        saw_confirmed = true;
                    }
                    if is_view_recompute(&frame, &view_id) {
                        saw_recompute = true;
                    }
                    frames.push(frame);
                    if saw_confirmed && saw_recompute {
                        drain_grace(&mut subscription, &mut frames, SETTLE_GRACE).await;
                        break;
                    }
                }
                Ok(None) => break, // stream closed
                Err(_) => break,   // timeout
            }
        }

        ViewSettlement {
            client_mutation_id,
            view_id,
            receipt,
            frames,
        }
    }

    /// Open a session + view and subscribe to its frame stream, returning a
    /// [`ViewWatch`] that stays subscribed across an external action (e.g.
    /// [`StalwartFixture::inject`]) and drains until a snapshot satisfies a
    /// predicate. The sync-driven counterpart to mutation-centric [`settle`].
    pub async fn watch_view(&self, view: ViewDescriptor) -> ViewWatch<'_> {
        let caller = RuntimeCaller::test();
        let session = self
            .build
            .handle
            .open_session(caller.clone())
            .await
            .expect("session should open");
        let snapshot = self
            .build
            .handle
            .open_session_view(caller.clone(), session.session_id.clone(), view)
            .await
            .expect("session view should open");
        let mut subscription = self
            .build
            .handle
            .subscribe_runtime_frames(caller, session.session_id, Some(RuntimeSessionSeq::new(0)))
            .await
            .expect("runtime stream should subscribe");
        let frames = std::mem::take(&mut subscription.catch_up);
        let view_id = snapshot.view_id.clone();
        ViewWatch {
            view_id,
            subscription,
            frames,
            last_snapshot: Some(snapshot),
            _phantom: PhantomData,
        }
    }
}

/// A live subscription to one view's frame stream, kept open across an external
/// action (e.g. message injection) so a sync-driven recompute can be observed.
pub struct ViewWatch<'a> {
    view_id: ViewId,
    subscription: RuntimeFrameSubscription,
    frames: Vec<RuntimeFrame>,
    last_snapshot: Option<ViewSnapshot>,
    _phantom: PhantomData<&'a ()>,
}

impl<'a> ViewWatch<'a> {
    /// Drain until a snapshot for the watched view satisfies `predicate`, or
    /// `timeout` elapses. Returns whether the predicate was satisfied.
    pub async fn wait_until<F>(&mut self, predicate: F, timeout: Duration) -> bool
    where
        F: Fn(&ViewSnapshot) -> bool,
    {
        if let Some(snapshot) = &self.last_snapshot {
            if predicate(snapshot) {
                return true;
            }
        }
        let deadline = Instant::now() + timeout;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return false;
            }
            match tokio::time::timeout(remaining, self.subscription.live.next()).await {
                Ok(Some(frame)) => {
                    let mut satisfied = false;
                    match &frame {
                        RuntimeFrame::ViewSnapshot {
                            view_id, snapshot, ..
                        }
                        | RuntimeFrame::ViewReplace {
                            view_id, snapshot, ..
                        } if view_id == &self.view_id => {
                            self.last_snapshot = Some(snapshot.clone());
                            if predicate(snapshot) {
                                satisfied = true;
                            }
                        }
                        _ => {}
                    }
                    self.frames.push(frame);
                    if satisfied {
                        return true;
                    }
                }
                _ => return false,
            }
        }
    }

    /// The most recent snapshot observed for the watched view.
    pub fn snapshot(&self) -> &ViewSnapshot {
        self.last_snapshot
            .as_ref()
            .expect("at least the initial snapshot should be present")
    }

    /// Assert no `ViewError` frame was observed.
    pub fn assert_no_view_errors(&self) {
        let errors: Vec<&RuntimeFrame> = self
            .frames
            .iter()
            .filter(|frame| matches!(frame, RuntimeFrame::ViewError { .. }))
            .collect();
        assert!(
            errors.is_empty(),
            "observed {} view error frame(s)",
            errors.len()
        );
    }

    /// Assert `session_seq` is strictly increasing across the captured frames.
    pub fn assert_seq_monotonic(&self) {
        assert_frames_seq_monotonic(&self.frames);
    }
}

/// The captured frame sequence for one mutation, from subscription open through
/// its terminal `MutationNotification` (+ a grace window for redundant
/// recomputes).
pub struct ViewSettlement {
    pub client_mutation_id: ClientMutationId,
    pub view_id: ViewId,
    pub receipt: MutationReceipt,
    pub frames: Vec<RuntimeFrame>,
}

impl ViewSettlement {
    /// The terminal `MutationNotification` verdict for this mutation, if observed
    /// (`Confirmed` or `Rejected`). Replaces the former `MutationSettlement`
    /// frame — the runtime no longer emits non-terminal acks.
    pub fn settlement(&self) -> Option<&MutationNotification> {
        self.frames.iter().find_map(|frame| match frame {
            RuntimeFrame::MutationNotification {
                client_mutation_id,
                notification,
                ..
            } if client_mutation_id == &self.client_mutation_id => Some(notification),
            _ => None,
        })
    }

    /// Assert the mutation settled `Confirmed` (fails if no terminal verdict
    /// arrived — the missed-settlement case).
    pub fn assert_confirmed(&self) {
        let notification = self.settlement().unwrap_or_else(|| {
            panic!(
                "no terminal mutation notification observed for {:?}",
                self.client_mutation_id
            )
        });
        assert_eq!(
            notification,
            &MutationNotification::Confirmed,
            "mutation did not settle Confirmed"
        );
    }

    /// Assert at least one recompute frame (`ViewReplace`/`ViewDelta`) arrived for
    /// the settled view — the missed-recompute detector. (The initial
    /// `ViewSnapshot` from opening the view is not counted.)
    pub fn assert_view_recomputed_at_least_once(&self) {
        let count = self
            .frames
            .iter()
            .filter(|frame| is_view_recompute(frame, &self.view_id))
            .count();
        assert!(
            count >= 1,
            "expected at least one view recompute for {:?}, got none",
            self.view_id
        );
    }

    /// Assert exactly one recompute frame (`ViewReplace`/`ViewDelta`) arrived
    /// for the settled view — zero is a missed recompute, more than one is a
    /// redundant recompute. Use for scenarios with no follow-up sync; a
    /// `run_mutation` on a live account legitimately produces two (optimistic +
    /// sync-confirmed), so prefer [`assert_view_recomputed_at_least_once`] there.
    /// (The initial `ViewSnapshot` from opening the view is not counted.)
    ///
    /// [`assert_view_recomputed_at_least_once`]: Self::assert_view_recomputed_at_least_once
    pub fn assert_view_recomputed_exactly_once(&self) {
        let count = self
            .frames
            .iter()
            .filter(|frame| is_view_recompute(frame, &self.view_id))
            .count();
        assert_eq!(
            count, 1,
            "expected exactly one view recompute for {:?}, got {}",
            self.view_id, count
        );
    }

    /// Assert the settled view was NOT re-served by the runtime. Option iii
    /// ([single-source-view-membership]): an evaluable mail-list view is
    /// self-maintained by the client from the `message.updated` firehose, so the
    /// runtime emits no per-event recompute (`ViewReplace`/`ViewDelta`) for it.
    pub fn assert_view_not_recomputed(&self) {
        let count = self
            .frames
            .iter()
            .filter(|frame| is_view_recompute(frame, &self.view_id))
            .count();
        assert_eq!(
            count, 0,
            "expected no runtime re-serve for the self-maintained view {:?}, got {count}",
            self.view_id
        );
    }

    /// Assert a `message.updated` notification fired — the firehose input the
    /// client self-maintains the view from (the update path under option iii,
    /// replacing the runtime's per-event view recompute).
    pub fn assert_message_updated_notification(&self) {
        let found = self.frames.iter().any(|frame| {
            matches!(
                frame,
                RuntimeFrame::Notification { kind, .. } if kind == "message.updated"
            )
        });
        assert!(
            found,
            "expected a message.updated notification (the client's self-maintenance input)"
        );
    }

    /// Assert no view other than the settled one recomputed (over-broad
    /// invalidation across unrelated views).
    pub fn assert_only_view_recomputed(&self) {
        let others: Vec<ViewId> = self
            .frames
            .iter()
            .filter_map(|frame| match frame {
                RuntimeFrame::ViewReplace { view_id, .. }
                | RuntimeFrame::ViewDelta { view_id, .. }
                    if view_id != &self.view_id =>
                {
                    Some(view_id.clone())
                }
                _ => None,
            })
            .collect();
        assert!(
            others.is_empty(),
            "unexpected view recomputes for views other than {:?}: {others:?}",
            self.view_id
        );
    }

    /// Assert `session_seq` is strictly increasing across the captured frames.
    pub fn assert_seq_monotonic(&self) {
        assert_frames_seq_monotonic(&self.frames);
    }
}

fn assert_frames_seq_monotonic(frames: &[RuntimeFrame]) {
    let mut last: Option<u64> = None;
    for frame in frames {
        let seq = frame.session_seq().get();
        if let Some(prev) = last {
            assert!(
                seq > prev,
                "session_seq went backward or stalled: {prev} -> {seq}"
            );
        }
        last = Some(seq);
    }
}

fn is_mutation_notification(frame: &RuntimeFrame, client_mutation_id: &ClientMutationId) -> bool {
    matches!(
        frame,
        RuntimeFrame::MutationNotification {
            client_mutation_id: cmid,
            ..
        } if cmid == client_mutation_id
    )
}

fn is_view_recompute(frame: &RuntimeFrame, view_id: &ViewId) -> bool {
    matches!(
        frame,
        RuntimeFrame::ViewReplace { view_id: vid, .. }
        | RuntimeFrame::ViewDelta { view_id: vid, .. }
            if vid == view_id
    )
}

/// Drain any frames that arrive within `window` (for catching redundant
/// recomputes that trail the first settlement).
async fn drain_grace(
    subscription: &mut RuntimeFrameSubscription,
    frames: &mut Vec<RuntimeFrame>,
    window: Duration,
) {
    let end = Instant::now() + window;
    loop {
        let remaining = end.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break;
        }
        match tokio::time::timeout(remaining, subscription.live.next()).await {
            Ok(Some(frame)) => frames.push(frame),
            _ => break,
        }
    }
}

/// In-memory `SecretStore` for tests (lifted from `authority_runtime_handle`).
#[derive(Default)]
pub struct TestSecretStore {
    values: Mutex<HashMap<String, String>>,
}

impl SecretStore for TestSecretStore {
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
