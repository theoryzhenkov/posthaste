//! In-process authority runtime harness + view-settlement recorder.
//!
//! [`Harness::with_runtime`](crate::Harness::with_runtime) builds an authority
//! runtime against disposable roots; [`RuntimeHarness::settle`] drives a
//! mutation and captures the ordered `RuntimeFrame` stream through settlement,
//! which [`ViewSettlement`] asserts on. This is the layer that catches missed /
//! over-broad view recomputes.

use std::collections::{BTreeSet, HashMap};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use tokio::sync::broadcast;

use posthaste_authority_runtime::{AuthorityRuntimeBuild, RuntimeHandle};
use posthaste_domain::{
    AccountDriver, AccountId, DomainEvent, MailStore, MailboxId, MailboxRecord, MessageId,
    MessageRecord, SecretStore, SecretStoreError, SecretRef, SyncBatch, SyncCursor, SyncObject,
    ThreadId,
};
use posthaste_runtime_contract::{
    AccountTransportMutation, CreateAccountMutation, MutationReceipt, MutationRequest,
    MutationSettlementState, RuntimeCaller, RuntimeCore, RuntimeFrame, RuntimeFrameSubscription,
    RuntimeMutationId, RuntimeSessionSeq, SecretWriteMutation, ViewDescriptor, ViewId,
};

/// Drain deadline for a mutation to settle + its view to recompute.
const SETTLE_TIMEOUT: Duration = Duration::from_secs(3);
/// Extra window after settlement to catch redundant recomputes.
const SETTLE_GRACE: Duration = Duration::from_millis(80);

/// An in-process authority runtime on disposable roots.
///
/// Built by [`Harness::with_runtime`](crate::Harness::with_runtime). Exposes
/// the [`RuntimeCore`](posthaste_runtime_contract::RuntimeCore) handle, the
/// store (for direct seeding), the event bus, and the
/// [`settle`](Self::settle) recorder.
pub struct RuntimeHarness {
    build: AuthorityRuntimeBuild,
}

impl RuntimeHarness {
    pub(crate) fn new(build: AuthorityRuntimeBuild) -> Self {
        Self { build }
    }

    /// The cloneable runtime handle (implements `RuntimeCore`).
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

    /// Create a mock-driver account, enable it, and sync it so its runtime is
    /// live. Returns the account id.
    pub async fn create_mock_account(&self, id: &str) -> AccountId {
        let mutation = CreateAccountMutation {
            id: Some(id.to_string()),
            name: id.to_string(),
            driver: Some(AccountDriver::Mock),
            enabled: Some(true),
            full_name: None,
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

    /// Seed `(message_id, mailbox_id)` pairs into an account via a direct store
    /// batch (bypasses sync — for unit/integration setup).
    pub fn seed_messages(&self, account_id: &AccountId, messages: &[(&str, &str)]) {
        let mailbox_ids: BTreeSet<&str> = messages.iter().map(|(_, mb)| *mb).collect();
        let mailboxes: Vec<MailboxRecord> = mailbox_ids
            .iter()
            .map(|mb| MailboxRecord {
                id: MailboxId::from(*mb),
                name: mb.to_string(),
                role: Some((*mb).to_string()),
                unread_emails: 0,
                total_emails: messages.iter().filter(|(_, m)| m == mb).count() as i64,
            })
            .collect();
        let msgs: Vec<MessageRecord> = messages.iter().map(|(m, mb)| seeded_message(m, mb)).collect();
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

    /// Open a session + view, subscribe to the runtime frame stream, run a
    /// mutation, and drain the ordered frames through settlement and the view's
    /// recompute. `mutation.session_id` is set to the opened session.
    pub async fn settle(&self, mut mutation: MutationRequest, view: ViewDescriptor) -> ViewSettlement {
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
        let mutation_id = receipt
            .runtime_mutation_id
            .clone()
            .expect("runtime mutation id should be assigned");

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
                    if is_terminal_settlement(&frame, &mutation_id) {
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
                Err(_) => break,    // timeout
            }
        }

        ViewSettlement {
            mutation_id,
            view_id,
            receipt,
            frames,
        }
    }
}

/// The captured frame sequence for one mutation, from subscription open through
/// terminal settlement (+ a grace window for redundant recomputes).
pub struct ViewSettlement {
    pub mutation_id: RuntimeMutationId,
    pub view_id: ViewId,
    pub receipt: MutationReceipt,
    pub frames: Vec<RuntimeFrame>,
}

impl ViewSettlement {
    /// The terminal `MutationSettlement` frame for this mutation, if observed.
    pub fn settlement(&self) -> Option<&posthaste_runtime_contract::RuntimeMutationSettlement> {
        self.frames.iter().find_map(|frame| match frame {
            RuntimeFrame::MutationSettlement {
                mutation_id,
                state,
                ..
            } if mutation_id == &self.mutation_id && state.status.is_terminal() => Some(state),
            _ => None,
        })
    }

    /// Assert the mutation settled `Confirmed` (fails if no terminal settlement
    /// arrived — the missed-settlement case).
    pub fn assert_confirmed(&self) {
        let settlement = self.settlement().unwrap_or_else(|| {
            panic!(
                "no terminal settlement frame observed for mutation {:?}",
                self.mutation_id
            )
        });
        assert_eq!(
            settlement.status,
            MutationSettlementState::Confirmed,
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
        let mut last: Option<u64> = None;
        for frame in &self.frames {
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
}

fn is_terminal_settlement(frame: &RuntimeFrame, mutation_id: &RuntimeMutationId) -> bool {
    matches!(
        frame,
        RuntimeFrame::MutationSettlement {
            mutation_id: mid,
            state,
            ..
        } if mid == mutation_id && state.status.is_terminal()
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

fn seeded_message(message_id: &str, mailbox_id: &str) -> MessageRecord {
    MessageRecord {
        id: MessageId::from(message_id),
        source_thread_id: ThreadId::from(format!("thread-{message_id}")),
        subject: Some(format!("Subject {message_id}")),
        from_name: Some("Alice".to_string()),
        from_email: Some("alice@example.com".to_string()),
        preview: Some("Preview".to_string()),
        received_at: "2026-03-31T10:00:00Z".to_string(),
        size: 42,
        mailbox_ids: vec![MailboxId::from(mailbox_id)],
        keywords: vec!["$seen".to_string()],
        rfc_message_id: Some(format!("<{message_id}@example.test>")),
        ..Default::default()
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
