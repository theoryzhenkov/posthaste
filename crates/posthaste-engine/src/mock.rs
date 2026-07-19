use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use posthaste_domain_model::{
    AccountId, BlobId, FetchedBody, GatewayError, Identity, MailboxId, MailboxRecord, MessageId,
    MessageReadback, MessageRecord, MutationOutcome, Recipient, ReplyContext, SendMessageRequest,
    SetKeywordsCommand, SyncBatch, SyncCursor, SyncObject, ThreadId,
};
use posthaste_domain_service::{MailGateway, PushTransport};
use tokio::sync::Notify;

mod samples;
mod state;

use samples::{sample_attachment_bytes, sample_attachments, sample_mailboxes, sample_messages};
use state::{
    bump_revision, ensure_expected_state, mutation_outcome, reject_if_marked, validate_mailbox_role,
};

/// In-memory `MailGateway` for tests and local development.
///
/// Holds a fixed set of sample mailboxes and messages. Mutations update
/// internal state and bump a revision counter to simulate JMAP state strings.
pub struct MockJmapGateway {
    state: Mutex<MockState>,
    /// Test seam: when set, every `fetch_message_body` on this instance sleeps
    /// for the delay first — a slow/hung body source for cache-worker tests.
    body_fetch_delay: Mutex<Option<Duration>>,
    /// Test seam: count of `fetch_message_body` calls on this instance, so a
    /// test can assert a backed-off cache tick does NOT re-hit the provider.
    body_fetch_attempts: AtomicUsize,
}

/// Mutable inner state behind the `MockJmapGateway` mutex.
struct MockState {
    revision: u64,
    mailboxes: Vec<MailboxRecord>,
    messages: Vec<MessageRecord>,
    /// Message ids the mock should reject mutations for (test hook): the
    /// mutation returns `MutationRejected` with the unchanged record as readback.
    rejected: HashSet<MessageId>,
}

impl MockJmapGateway {
    /// Test hook: make subsequent message mutations on `message_id` reject
    /// (provider returns the unchanged state), exercising the revert + surface
    /// settle path.
    pub fn reject_message(&self, message_id: MessageId) {
        if let Ok(mut state) = self.state.lock() {
            state.rejected.insert(message_id);
        }
    }

    /// Test hook: make every subsequent `fetch_message_body` on this instance
    /// sleep for `delay` before answering — a slow or (with a large delay)
    /// effectively hung body source.
    pub fn set_body_fetch_delay_for_tests(&self, delay: Duration) {
        if let Ok(mut slot) = self.body_fetch_delay.lock() {
            *slot = Some(delay);
        }
    }

    /// Clear the per-instance body-fetch delay (the source "recovers").
    pub fn clear_body_fetch_delay_for_tests(&self) {
        if let Ok(mut slot) = self.body_fetch_delay.lock() {
            *slot = None;
        }
    }

    /// Number of `fetch_message_body` calls observed on this instance.
    pub fn body_fetch_attempts_for_tests(&self) -> usize {
        self.body_fetch_attempts.load(Ordering::SeqCst)
    }

    /// Test hook: strip the inline bodies from every mock message so a sync
    /// seeds body-cache candidates in the `wanted` state (messages that carry
    /// inline bodies are cached on arrival and generate no fetch work).
    pub fn strip_message_bodies_for_tests(&self) {
        if let Ok(mut state) = self.state.lock() {
            for message in &mut state.messages {
                message.body_html = None;
                message.body_text = None;
                message.raw_mime = None;
            }
        }
    }
}

impl Default for MockJmapGateway {
    fn default() -> Self {
        Self {
            state: Mutex::new(MockState {
                revision: 1,
                mailboxes: sample_mailboxes(),
                messages: sample_messages(),
                rejected: HashSet::new(),
            }),
            body_fetch_delay: Mutex::new(None),
            body_fetch_attempts: AtomicUsize::new(0),
        }
    }
}

/// Test-only hook: every `MockJmapGateway::sync` call will sleep for this many
/// milliseconds. Used by integration tests that need a slow provider sync so that
/// concurrent mutation triggers observe `is_syncing == true` and coalesce.
static SYNC_DELAY_MILLIS: AtomicUsize = AtomicUsize::new(0);

impl MockJmapGateway {
    /// Set a delay applied to all subsequent `sync` calls across all mock
    /// gateway instances. Call `clear_sync_delay` after the test.
    pub fn set_sync_delay_for_tests(millis: usize) {
        SYNC_DELAY_MILLIS.store(millis, Ordering::SeqCst);
    }

    /// Clear the global sync delay.
    pub fn clear_sync_delay_for_tests() {
        SYNC_DELAY_MILLIS.store(0, Ordering::SeqCst);
    }

    /// Gate the next `sync` call for `account_id` at method entry.
    ///
    /// The test waits on `entered` to know the sync has begun (i.e. the
    /// `flush_account` phase has finished and the pull is about to start), then
    /// releases via `release` to let the sync complete. The returned guard
    /// removes the gate on drop so a panicking test cannot poison later tests.
    pub fn gate_sync_at_entry(
        account_id: &AccountId,
        entered: Arc<Notify>,
        release: Arc<Notify>,
    ) -> SyncGateGuard {
        let mut gates = SYNC_GATES.lock().expect("sync gates mutex poisoned");
        gates.insert(
            account_id.as_str().to_string(),
            SyncGate { entered, release },
        );
        SyncGateGuard {
            account_id: account_id.as_str().to_string(),
        }
    }

    /// Clear every installed sync gate. Mostly useful as a defensive reset in
    /// tests that do not use the `SyncGateGuard` RAII helper.
    pub fn clear_sync_gates_for_tests() {
        let mut gates = SYNC_GATES.lock().expect("sync gates mutex poisoned");
        gates.clear();
    }
}

/// Account-scoped sync gates for deterministic coalescing tests. A single
/// static map is acceptable because tests set gates for account ids they own
/// and the RAII guard clears them on drop.
static SYNC_GATES: LazyLock<Mutex<HashMap<String, SyncGate>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Prefix-scoped probe that records the peak number of concurrent `sync` calls
/// whose account id starts with `prefix`. Used by the boot-storm test to assert
/// the supervisor's global concurrent-sync cap holds. Scoping by prefix keeps
/// the (process-wide) probe isolated from other tests' mock syncs running in
/// parallel in the same binary.
struct SyncConcurrencyProbe {
    prefix: String,
    current: usize,
    peak: usize,
}

static SYNC_CONCURRENCY_PROBE: LazyLock<Mutex<Option<SyncConcurrencyProbe>>> =
    LazyLock::new(|| Mutex::new(None));

impl MockJmapGateway {
    /// Install a peak-concurrency probe for `sync` calls on accounts whose id
    /// starts with `prefix`. The returned guard uninstalls it on drop.
    pub fn install_sync_concurrency_probe_for_tests(prefix: &str) -> SyncConcurrencyProbeGuard {
        *SYNC_CONCURRENCY_PROBE
            .lock()
            .expect("sync concurrency probe mutex poisoned") = Some(SyncConcurrencyProbe {
            prefix: prefix.to_string(),
            current: 0,
            peak: 0,
        });
        SyncConcurrencyProbeGuard
    }

    /// The peak concurrent `sync` count observed by the installed probe.
    pub fn observed_peak_concurrent_syncs_for_tests() -> usize {
        SYNC_CONCURRENCY_PROBE
            .lock()
            .expect("sync concurrency probe mutex poisoned")
            .as_ref()
            .map(|probe| probe.peak)
            .unwrap_or(0)
    }
}

/// RAII guard returned by [`MockJmapGateway::install_sync_concurrency_probe_for_tests`].
pub struct SyncConcurrencyProbeGuard;

impl Drop for SyncConcurrencyProbeGuard {
    fn drop(&mut self) {
        *SYNC_CONCURRENCY_PROBE
            .lock()
            .expect("sync concurrency probe mutex poisoned") = None;
    }
}

/// RAII counter: on construction it increments the installed probe's in-flight
/// count (and updates the peak) if `account_id` matches the probed prefix; on
/// drop it decrements. Holding it for the whole `sync` body means an in-flight
/// sync counts across its `.await` points, and a sync future dropped mid-flight
/// (e.g. cancelled by the supervisor stop) still releases its count.
struct SyncProbeCount {
    counted: bool,
}

impl SyncProbeCount {
    fn enter(account_id: &AccountId) -> Self {
        let mut probe = SYNC_CONCURRENCY_PROBE
            .lock()
            .expect("sync concurrency probe mutex poisoned");
        let counted = match probe.as_mut() {
            Some(probe) if account_id.as_str().starts_with(&probe.prefix) => {
                probe.current += 1;
                probe.peak = probe.peak.max(probe.current);
                true
            }
            _ => false,
        };
        Self { counted }
    }
}

impl Drop for SyncProbeCount {
    fn drop(&mut self) {
        if !self.counted {
            return;
        }
        let mut probe = SYNC_CONCURRENCY_PROBE
            .lock()
            .expect("sync concurrency probe mutex poisoned");
        if let Some(probe) = probe.as_mut() {
            probe.current = probe.current.saturating_sub(1);
        }
    }
}

#[derive(Clone)]
struct SyncGate {
    entered: Arc<Notify>,
    release: Arc<Notify>,
}

/// RAII guard returned by [`MockJmapGateway::gate_sync_at_entry`].
pub struct SyncGateGuard {
    account_id: String,
}

impl Drop for SyncGateGuard {
    fn drop(&mut self) {
        let mut gates = SYNC_GATES.lock().expect("sync gates mutex poisoned");
        gates.remove(&self.account_id);
    }
}

/// Build a mock mutation outcome from the current revision.
#[async_trait]
impl MailGateway for MockJmapGateway {
    /// Return a full snapshot of all mock mailboxes and messages.
    async fn sync(
        &self,
        account_id: &AccountId,
        _cursors: &[SyncCursor],
        progress: Option<posthaste_domain_service::SyncProgressReporter>,
    ) -> Result<SyncBatch, GatewayError> {
        use posthaste_domain_model::{SyncProgress, SyncProgressStage, SyncTrigger};
        let report = |stage: SyncProgressStage, detail: &str, message_count: Option<usize>| {
            if let Some(progress) = progress.as_ref() {
                progress.report(SyncProgress {
                    sync_id: String::new(),
                    trigger: SyncTrigger::Manual,
                    started_at: String::new(),
                    stage,
                    detail: detail.to_string(),
                    mailbox_name: None,
                    mailbox_index: None,
                    mailbox_count: None,
                    message_count,
                    total_count: None,
                });
            }
        };
        report(SyncProgressStage::Discovering, "Listing mailboxes", None);
        // Boot-storm test seam: count concurrent syncs for the probed prefix for
        // the whole `sync` body (held across the gate + delay below), so a test
        // can assert the supervisor's global concurrent-sync cap holds. Placed
        // before the gate so a sync that is admitted (has acquired its global
        // slot) but then blocks at the gate still counts as concurrent.
        let _probe = SyncProbeCount::enter(account_id);
        // Account-scoped gate: lets a test block the pull phase until the test
        // has enqueued more local mutations, deterministically reproducing the
        // case where a mutation's provider flush is delayed by sync coalescing.
        let gate = SYNC_GATES
            .lock()
            .expect("sync gates mutex poisoned")
            .get(account_id.as_str())
            .cloned();
        if let Some(gate) = gate {
            gate.entered.notify_one();
            gate.release.notified().await;
        }
        let delay_millis = SYNC_DELAY_MILLIS.load(Ordering::SeqCst);
        if delay_millis > 0 {
            report(SyncProgressStage::Fetching, "Fetching messages", None);
            tokio::time::sleep(Duration::from_millis(delay_millis as u64)).await;
        }
        let state = self
            .state
            .lock()
            .map_err(|_| GatewayError::Rejected("mock state poisoned".to_string()))?;
        report(
            SyncProgressStage::Storing,
            "Applying synced changes",
            Some(state.messages.len()),
        );
        Ok(SyncBatch {
            mailboxes: state.mailboxes.clone(),
            messages: state.messages.clone(),
            imap_mailbox_states: Vec::new(),
            imap_message_locations: Vec::new(),
            deleted_imap_message_locations: Vec::new(),
            deleted_mailbox_ids: Vec::new(),
            deleted_message_ids: Vec::new(),
            absence_deleted_imap_message_locations: Vec::new(),
            absence_deleted_message_ids: Vec::new(),
            replace_all_mailboxes: true,
            replace_all_messages: true,
            cursors: vec![
                SyncCursor {
                    object_type: SyncObject::Mailbox,
                    state: format!("mailbox-{}", state.revision),
                    updated_at: "2026-03-31T10:00:00Z".to_string(),
                },
                SyncCursor {
                    object_type: SyncObject::Message,
                    state: format!("message-{}", state.revision),
                    updated_at: "2026-03-31T10:00:00Z".to_string(),
                },
            ],
        })
    }

    /// Return the pre-populated body for a mock message.
    async fn fetch_message_body(
        &self,
        _account_id: &AccountId,
        message_id: &MessageId,
    ) -> Result<FetchedBody, GatewayError> {
        self.body_fetch_attempts.fetch_add(1, Ordering::SeqCst);
        let delay = self.body_fetch_delay.lock().ok().and_then(|slot| *slot);
        if let Some(delay) = delay {
            tokio::time::sleep(delay).await;
        }
        let state = self
            .state
            .lock()
            .map_err(|_| GatewayError::Rejected("mock state poisoned".to_string()))?;
        let message = state
            .messages
            .iter()
            .find(|message| &message.id == message_id)
            .ok_or_else(|| GatewayError::Rejected("unknown message".to_string()))?;
        Ok(FetchedBody {
            body_html: message.body_html.clone(),
            body_text: message.body_text.clone(),
            raw_mime: message.raw_mime.clone(),
            attachments: sample_attachments(message.id.as_str()),
            list_unsubscribe: message.list_unsubscribe.clone(),
        })
    }

    async fn download_blob(
        &self,
        _account_id: &AccountId,
        blob_id: &BlobId,
    ) -> Result<Vec<u8>, GatewayError> {
        sample_attachment_bytes(blob_id.as_str())
            .ok_or_else(|| GatewayError::Rejected("unknown blob".to_string()))
    }

    /// Apply keyword changes to a mock message, respecting optimistic concurrency.
    async fn set_keywords(
        &self,
        _account_id: &AccountId,
        message_id: &MessageId,
        expected_state: Option<&str>,
        command: &SetKeywordsCommand,
    ) -> Result<MutationOutcome, GatewayError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| GatewayError::Rejected("mock state poisoned".to_string()))?;
        ensure_expected_state(&state, expected_state, SyncObject::Message)?;
        reject_if_marked(&state, message_id)?;
        let message = state
            .messages
            .iter_mut()
            .find(|message| &message.id == message_id)
            .ok_or_else(|| GatewayError::Rejected("unknown message".to_string()))?;
        for keyword in &command.add {
            if !message.keywords.contains(keyword) {
                message.keywords.push(keyword.clone());
            }
        }
        message
            .keywords
            .retain(|keyword| !command.remove.contains(keyword));
        bump_revision(&mut state);
        let updated = state.messages.iter().find(|m| &m.id == message_id).cloned();
        Ok(MutationOutcome {
            message: updated.map(MessageReadback::Present),
            ..mutation_outcome(&state, SyncObject::Message)
        })
    }

    /// Replace a mock message's mailbox membership.
    async fn replace_mailboxes(
        &self,
        _account_id: &AccountId,
        message_id: &MessageId,
        expected_state: Option<&str>,
        mailbox_ids: &[MailboxId],
    ) -> Result<MutationOutcome, GatewayError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| GatewayError::Rejected("mock state poisoned".to_string()))?;
        ensure_expected_state(&state, expected_state, SyncObject::Message)?;
        reject_if_marked(&state, message_id)?;
        let message = state
            .messages
            .iter_mut()
            .find(|message| &message.id == message_id)
            .ok_or_else(|| GatewayError::Rejected("unknown message".to_string()))?;
        message.mailbox_ids = mailbox_ids.to_vec();
        bump_revision(&mut state);
        let updated = state.messages.iter().find(|m| &m.id == message_id).cloned();
        Ok(MutationOutcome {
            message: updated.map(MessageReadback::Present),
            ..mutation_outcome(&state, SyncObject::Message)
        })
    }

    /// Remove a message from mock state.
    async fn destroy_message(
        &self,
        _account_id: &AccountId,
        message_id: &MessageId,
        expected_state: Option<&str>,
    ) -> Result<MutationOutcome, GatewayError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| GatewayError::Rejected("mock state poisoned".to_string()))?;
        ensure_expected_state(&state, expected_state, SyncObject::Message)?;
        reject_if_marked(&state, message_id)?;
        state.messages.retain(|message| &message.id != message_id);
        bump_revision(&mut state);
        Ok(MutationOutcome {
            message: Some(MessageReadback::Removed),
            ..mutation_outcome(&state, SyncObject::Message)
        })
    }

    /// Update a mock mailbox role.
    async fn set_mailbox_role(
        &self,
        _account_id: &AccountId,
        mailbox_id: &MailboxId,
        expected_state: Option<&str>,
        role: Option<&str>,
        clear_role_from: Option<&MailboxId>,
    ) -> Result<MutationOutcome, GatewayError> {
        validate_mailbox_role(role)?;
        let mut state = self
            .state
            .lock()
            .map_err(|_| GatewayError::Rejected("mock state poisoned".to_string()))?;
        ensure_expected_state(&state, expected_state, SyncObject::Mailbox)?;
        if let Some(clear_role_from) = clear_role_from.filter(|id| *id != mailbox_id) {
            let mailbox = state
                .mailboxes
                .iter_mut()
                .find(|mailbox| &mailbox.id == clear_role_from)
                .ok_or_else(|| GatewayError::Rejected("unknown mailbox".to_string()))?;
            mailbox.role = None;
        }
        let mailbox = state
            .mailboxes
            .iter_mut()
            .find(|mailbox| &mailbox.id == mailbox_id)
            .ok_or_else(|| GatewayError::Rejected("unknown mailbox".to_string()))?;
        mailbox.role = role.map(str::to_string);
        bump_revision(&mut state);
        Ok(mutation_outcome(&state, SyncObject::Mailbox))
    }

    /// Rename a mock mailbox in place — id, role, and contents untouched. The
    /// new name surfaces on the next `sync`, mirroring the readback the
    /// service performs after a rename.
    async fn rename_mailbox(
        &self,
        _account_id: &AccountId,
        mailbox_id: &MailboxId,
        expected_state: Option<&str>,
        name: &str,
    ) -> Result<MutationOutcome, GatewayError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| GatewayError::Rejected("mock state poisoned".to_string()))?;
        ensure_expected_state(&state, expected_state, SyncObject::Mailbox)?;
        let mailbox = state
            .mailboxes
            .iter_mut()
            .find(|mailbox| &mailbox.id == mailbox_id)
            .ok_or_else(|| GatewayError::Rejected("unknown mailbox".to_string()))?;
        mailbox.name = name.to_string();
        bump_revision(&mut state);
        Ok(mutation_outcome(&state, SyncObject::Mailbox))
    }

    /// Create a mock mailbox and return its id. The new record surfaces on the
    /// next `sync`, mirroring the readback the service performs after a create.
    async fn create_mailbox(
        &self,
        _account_id: &AccountId,
        name: &str,
    ) -> Result<MailboxId, GatewayError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| GatewayError::Rejected("mock state poisoned".to_string()))?;
        let id = MailboxId::from(format!("mb-{name}").as_str());
        state.mailboxes.push(MailboxRecord {
            id: id.clone(),
            name: name.to_string(),
            role: None,
            unread_emails: 0,
            total_emails: 0,
        });
        bump_revision(&mut state);
        Ok(id)
    }

    /// Remove a mock mailbox so the next `sync` reports it gone (mirroring the
    /// provider-side deletion the service reads back). Mirrors the JMAP
    /// `onDestroyRemoveEmails=false` backstop: a non-empty mailbox is refused
    /// with [`GatewayError::MailboxNotEmpty`] unless `remove_emails` is set, in
    /// which case the contained messages are dropped too.
    async fn destroy_mailbox(
        &self,
        _account_id: &AccountId,
        mailbox_id: &MailboxId,
        remove_emails: bool,
    ) -> Result<(), GatewayError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| GatewayError::Rejected("mock state poisoned".to_string()))?;
        let count = state
            .messages
            .iter()
            .filter(|message| message.mailbox_ids.contains(mailbox_id))
            .count() as i64;
        if count > 0 && !remove_emails {
            return Err(GatewayError::MailboxNotEmpty { count });
        }
        if remove_emails {
            state
                .messages
                .retain(|message| !message.mailbox_ids.contains(mailbox_id));
        }
        state.mailboxes.retain(|mailbox| &mailbox.id != mailbox_id);
        bump_revision(&mut state);
        Ok(())
    }

    /// Return a hard-coded mock sender identity.
    async fn fetch_identity(&self, _account_id: &AccountId) -> Result<Identity, GatewayError> {
        Ok(Identity {
            id: "mock-identity".to_string(),
            name: "Mock Sender".to_string(),
            email: "mock@example.com".to_string(),
        })
    }

    /// Build a reply context from mock message data.
    async fn fetch_reply_context(
        &self,
        _account_id: &AccountId,
        message_id: &MessageId,
    ) -> Result<ReplyContext, GatewayError> {
        let state = self
            .state
            .lock()
            .map_err(|_| GatewayError::Rejected("mock state poisoned".to_string()))?;
        let message = state
            .messages
            .iter()
            .find(|message| &message.id == message_id)
            .ok_or_else(|| GatewayError::Rejected("unknown message".to_string()))?;
        let original_from = vec![Recipient {
            name: message.from_name.clone(),
            email: message
                .from_email
                .clone()
                .unwrap_or_else(|| "unknown@example.com".to_string()),
        }];
        Ok(ReplyContext {
            to: original_from.clone(),
            cc: Vec::new(),
            original_to: Vec::new(),
            reply_subject: format!("Re: {}", message.subject.clone().unwrap_or_default()),
            forward_subject: format!("Fwd: {}", message.subject.clone().unwrap_or_default()),
            quoted_body: message.body_text.clone(),
            forwarded_body: message.body_text.clone(),
            in_reply_to: Some(format!("<{}@mock>", message.id.as_str())),
            references: Some(format!("<{}@mock>", message.id.as_str())),
            original_from,
            original_date: Some(message.received_at.clone()),
        })
    }

    /// No-op: accept the send request without side effects.
    async fn send_message(
        &self,
        _account_id: &AccountId,
        _request: &SendMessageRequest,
        _consume_draft: Option<&MessageId>,
        _idempotency_key: &str,
    ) -> Result<posthaste_domain_model::SendFiling, GatewayError> {
        Ok(posthaste_domain_model::SendFiling::Filed)
    }

    /// Store a draft message and return a deterministic created id. When
    /// `replace` is set, the prior draft is removed first (create-new +
    /// destroy-old), mirroring the live gateway.
    ///
    /// @spec docs/L1-outbox#operation-model
    async fn save_draft(
        &self,
        _account_id: &AccountId,
        request: &SendMessageRequest,
        replace: Option<&MessageId>,
        // The mock's replace always matches (or harmlessly no-ops), so the DS3
        // `notFound` discrimination has no distinct outcome here — the live JMAP
        // `Email/set` `destroyed` check is where it bites.
        _idempotent_redelivery: bool,
        // The mock never loses a response, so the deterministic create-id (DS2)
        // has no twin to dedup here — it is the live JMAP create-with-id that
        // enforces idempotent redelivery.
        _idempotency_key: &str,
    ) -> Result<MessageId, GatewayError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| GatewayError::Rejected("mock state poisoned".to_string()))?;
        if let Some(replace) = replace {
            state.messages.retain(|message| &message.id != replace);
        }
        bump_revision(&mut state);
        let new_id = MessageId::from(format!("draft-created-{}", state.revision));
        state.messages.push(MessageRecord {
            id: new_id.clone(),
            source_thread_id: ThreadId::from(new_id.as_str()),
            remote_blob_id: None,
            subject: Some(request.subject.clone()),
            from_name: request.from.as_ref().and_then(|from| from.name.clone()),
            from_email: request.from.as_ref().map(|from| from.email.clone()),
            to: request.to.clone(),
            preview: Some(request.body.clone()),
            received_at: "2026-03-31T10:00:00Z".to_string(),
            has_attachment: !request.attachments.is_empty(),
            size: request.body.len() as i64,
            mailbox_ids: Vec::new(),
            keywords: vec!["$draft".to_string()],
            body_html: None,
            body_text: Some(request.body.clone()),
            raw_mime: None,
            rfc_message_id: Some(format!("<{}@mock>", new_id.as_str())),
            in_reply_to: request.in_reply_to.clone(),
            references: request
                .references
                .as_ref()
                .map(|references| references.split_whitespace().map(str::to_string).collect())
                .unwrap_or_default(),
            // Simulate the provider round-tripping the X-Posthaste-Draft-Id header.
            draft_id: request.draft_id.clone(),
            list_unsubscribe: None,
        });
        Ok(new_id)
    }

    /// Remove a draft message by id.
    ///
    /// @spec docs/L1-outbox#operation-model
    async fn delete_draft(
        &self,
        _account_id: &AccountId,
        message_id: &MessageId,
        _idempotent_redelivery: bool,
    ) -> Result<(), GatewayError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| GatewayError::Rejected("mock state poisoned".to_string()))?;
        state.messages.retain(|message| &message.id != message_id);
        bump_revision(&mut state);
        Ok(())
    }

    /// Mock gateway has no push transports.
    fn push_transports(&self) -> Vec<Box<dyn PushTransport>> {
        vec![]
    }
}

#[cfg(test)]
mod tests;
