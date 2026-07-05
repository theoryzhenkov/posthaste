//! The rule evaluator: subscribe to the domain-event bus, match a triggering
//! fact's message against each rule's WHEN-clause, and dispatch the action.
//!
//! Concurrency (the M27 discipline): a non-blocking **forwarder** drains the
//! lossy broadcast into a bounded `mpsc` (drop-newest on overflow, so the bus is
//! never back-pressured by a slow rule), and a single **evaluator** task processes
//! the queue in order. One evaluator ⇒ rules never race each other; the bounded
//! queue ⇒ a slow webhook cannot grow memory without bound.

use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};

use posthaste_authority_server_link::AuthorityServerApi;
use posthaste_contract_core::mutation_args::{MessageReplaceMailboxesArgs, MessageSetUserTagsArgs};
use posthaste_contract_core::MailOperation;
use posthaste_domain_model::{
    now_iso8601, AccountId, DomainEvent, MailboxId, MessageId, MessageSortField, MessageSummary,
    Rule, RuleAction, RuleDeliveryFailed, RuleFired, RuleOutcome, SmartMailboxCondition,
    SmartMailboxField, SmartMailboxGroup, SmartMailboxGroupOperator, SmartMailboxOperator,
    SmartMailboxRule, SmartMailboxRuleNode, SmartMailboxValue, SortDirection,
    EVENT_TOPIC_RULE_DELIVERY_FAILED, EVENT_TOPIC_RULE_FIRED,
};
use posthaste_link_far_end::down::FactLog;
use posthaste_observability::{events, ph_info, ph_warn};
use posthaste_provider_call::{ExecutorConfig, ProviderCallExecutor};
use tokio::sync::{broadcast, mpsc};

use crate::authority_server::AuthorityServer;
use crate::fact_log::AuthorityServerFactLog;
use crate::local_authority_server::LocalAuthorityServer;
use crate::rules::SharedMinter;
use posthaste_domain_service::{MailService, MailStore};

/// Bound on the in-flight rule-evaluation queue. A backlog beyond this drops the
/// newest fact rather than back-pressuring the event bus (at-least-once means a
/// dropped fact is recoverable by the next matching update; the tap is not the
/// durable path here).
const RULE_EVENT_QUEUE_CAPACITY: usize = 1024;

/// A running rule engine. Dropping it aborts the forwarder + evaluator tasks
/// (they also end on their own when the event bus closes at shutdown).
///
/// Holds a [`ManagedRulesHandle`] so the composition root can hand the REST
/// write surface a live controller: a created/edited/deleted rule re-loads the
/// merged ruleset and hot-swaps the evaluator's active rules WITHOUT a restart
/// (the reload path, prerequisite 2) — the forwarder's bus subscription is never
/// touched, so no event is missed across a reload.
pub struct RuleEngineHandle {
    tasks: Vec<tokio::task::JoinHandle<()>>,
    managed: ManagedRulesHandle,
}

impl RuleEngineHandle {
    /// A clone of the live managed-rules controller (create/update/delete +
    /// reload), for wiring into the REST write routes.
    pub fn managed_rules(&self) -> ManagedRulesHandle {
        self.managed.clone()
    }
}

impl Drop for RuleEngineHandle {
    fn drop(&mut self) {
        for task in &self.tasks {
            task.abort();
        }
    }
}

/// Everything the evaluator needs, shared across the tasks.
///
/// `rules` is an [`RwLock`] over an [`Arc`] snapshot (an ArcSwap-shaped design
/// without the dependency): the evaluator reads a cheap `Arc` clone at the top
/// of each event and iterates THAT immutable snapshot, so an in-flight
/// evaluation is never disturbed by a concurrent reload; a reload takes the
/// write lock for the microseconds it needs to store a fresh `Arc` and releases
/// it. This is the race-safety argument: readers hold a consistent snapshot,
/// the writer swaps a pointer, and neither blocks on the other's real work.
pub(crate) struct EngineContext {
    pub(crate) service: Arc<MailService>,
    pub(crate) rules: RwLock<Arc<Vec<Rule>>>,
    pub(crate) minter: Option<SharedMinter>,
    pub(crate) executor: Arc<ProviderCallExecutor>,
    pub(crate) local: Arc<LocalAuthorityServer>,
    /// The authority server's writable `FactLog` binding (RFC-L2-scripting
    /// D52/S3): the durable authoring path meta-facts (`rule.fired`,
    /// `rule.delivery.failed`) append through, so they land in the SAME
    /// `event_log` the existing `/v1/events` tap already replays from — no
    /// second tap, just the missing durable write path for AS-origin facts
    /// that used to go straight to the live broadcast (`seq: 0`) and vanish on
    /// reconnect.
    pub(crate) fact_log: Arc<AuthorityServerFactLog>,
}

impl EngineContext {
    /// A cheap `Arc`-clone snapshot of the active rules — the evaluator's
    /// consistent view for one event.
    fn rules_snapshot(&self) -> Arc<Vec<Rule>> {
        self.rules.read().expect("rules lock poisoned").clone()
    }

    /// Atomically replace the active rules (the reload swap). Only the pointer
    /// swap holds the lock; in-flight evaluations keep their prior snapshot.
    fn swap_rules(&self, rules: Vec<Rule>) {
        *self.rules.write().expect("rules lock poisoned") = Arc::new(rules);
    }

    /// Evict a single rule id from the live snapshot without a full disk reload.
    /// The delete-path fallback: guarantees a deleted rule stops firing even if
    /// a concurrently-broken `rules.toml` makes the post-delete reload fail
    /// (review finding, 2026-07-03 — a token-minting rule must always be
    /// stoppable).
    fn remove_rule(&self, id: &str) {
        let mut guard = self.rules.write().expect("rules lock poisoned");
        if let Some(kept) = rules_without(&guard, id) {
            *guard = Arc::new(kept);
        }
    }
}

/// Return the rule set with `id` removed, or `None` if `id` isn't present (so
/// the caller can skip a needless snapshot swap). Pure — unit-testable without
/// an [`EngineContext`].
fn rules_without(rules: &[Rule], id: &str) -> Option<Vec<Rule>> {
    if rules.iter().any(|rule| rule.id == id) {
        Some(rules.iter().filter(|rule| rule.id != id).cloned().collect())
    } else {
        None
    }
}

/// A live controller for the GUI-managed rule store, handed to the REST write
/// routes (RFC-L2-scripting ruling 23). Every write persists a `rules.d/<id>.toml`
/// file AND hot-swaps the running evaluator's rules, so a created rule fires on
/// the next matching event without a restart. Cloneable and cheap (an `Arc`).
///
/// A process-local mutex serialises the write→reload critical section so
/// concurrent CRUD calls can never interleave a file write with another's reload
/// and leave the in-memory rules disagreeing with disk.
#[derive(Clone)]
pub struct ManagedRulesHandle {
    inner: Arc<ManagedRulesInner>,
}

struct ManagedRulesInner {
    config_root: PathBuf,
    ctx: Arc<EngineContext>,
    write_lock: Mutex<()>,
}

impl ManagedRulesHandle {
    fn new(config_root: PathBuf, ctx: Arc<EngineContext>) -> Self {
        Self {
            inner: Arc::new(ManagedRulesInner {
                config_root,
                ctx,
                write_lock: Mutex::new(()),
            }),
        }
    }

    /// The config root, for the read route (which lists straight off disk).
    pub fn config_root(&self) -> &std::path::Path {
        &self.inner.config_root
    }

    /// Create a NEW managed rule. Fails [`RuleWriteError::Conflict`] if the id is
    /// already used by a managed rule OR a hand-authored `rules.toml` rule (the
    /// GUI must not shadow an authored rule). Persists then reloads.
    pub fn create(&self, rule: Rule) -> Result<Rule, super::writer::RuleWriteError> {
        let _guard = self.inner.write_lock.lock().expect("write lock poisoned");
        // Conflict if the id is already used ANYWHERE in the merged ruleset — a
        // managed file or a hand-authored `rules.toml` rule (no shadowing).
        if self.existing_rule_ids().contains(&rule.id) {
            return Err(super::writer::RuleWriteError::Conflict(rule.id));
        }
        super::writer::write_managed_rule(&self.inner.config_root, &rule)?;
        self.reload_locked();
        Ok(rule)
    }

    /// Update an EXISTING managed rule (matched by id). Fails
    /// [`RuleWriteError::NotFound`] if no managed file has that id (a
    /// hand-authored rule is not editable here). Persists then reloads.
    pub fn update(&self, rule: Rule) -> Result<Rule, super::writer::RuleWriteError> {
        let _guard = self.inner.write_lock.lock().expect("write lock poisoned");
        if !super::writer::managed_rule_exists(&self.inner.config_root, &rule.id) {
            return Err(super::writer::RuleWriteError::NotFound(rule.id));
        }
        super::writer::write_managed_rule(&self.inner.config_root, &rule)?;
        self.reload_locked();
        Ok(rule)
    }

    /// Delete a managed rule by id. Persists then reloads.
    pub fn delete(&self, id: &str) -> Result<(), super::writer::RuleWriteError> {
        let _guard = self.inner.write_lock.lock().expect("write lock poisoned");
        super::writer::delete_managed_rule(&self.inner.config_root, id)?;
        // A delete must ALWAYS stop the rule. If the post-delete reload fails
        // (e.g. rules.toml is concurrently parse-broken), evict the id from the
        // live snapshot directly so a token-minting rule can never survive its
        // own deletion (review finding, 2026-07-03).
        if !self.reload_locked() {
            self.inner.ctx.remove_rule(id);
        }
        Ok(())
    }

    /// Re-load the merged ruleset from disk and hot-swap the evaluator's active
    /// rules. Called under the write lock. A load error is logged and leaves the
    /// prior (good) rules in place rather than blanking the engine.
    fn reload_locked(&self) -> bool {
        match super::config::load_rules(&self.inner.config_root) {
            Ok(rules) => {
                let enabled = rules.into_iter().filter(|rule| rule.enabled).collect();
                self.inner.ctx.swap_rules(enabled);
                true
            }
            Err(error) => {
                ph_warn!(
                    events::RULE_ENGINE_STARTED,
                    error = %error,
                    "rule reload failed; keeping the previously loaded rules"
                );
                false
            }
        }
    }

    /// Every rule id in the merged ruleset on disk (for create collision checks).
    fn existing_rule_ids(&self) -> std::collections::HashSet<String> {
        super::config::load_rules(&self.inner.config_root)
            .map(|rules| rules.into_iter().map(|rule| rule.id).collect())
            .unwrap_or_default()
    }
}

/// Spawn the engine over the authority server's event bus. Always spawns (even
/// with zero enabled rules) so the returned [`ManagedRulesHandle`] can populate
/// the evaluator via the reload path when the GUI creates the FIRST rule — the
/// bus subscription must already be live, or a just-created rule would not fire
/// until the next restart.
pub(crate) fn spawn(
    authority_server: Arc<AuthorityServer>,
    service: Arc<MailService>,
    store: Arc<dyn MailStore>,
    event_sender: broadcast::Sender<DomainEvent>,
    config_root: PathBuf,
    rules: Vec<Rule>,
    minter: Option<SharedMinter>,
) -> RuleEngineHandle {
    let executor = Arc::new(
        ProviderCallExecutor::new(ExecutorConfig::default())
            .expect("failed to build the rule webhook HTTP client"),
    );
    let local = Arc::new(LocalAuthorityServer::new(authority_server));
    let fact_log = Arc::new(AuthorityServerFactLog::new(store, event_sender.clone()));
    let rule_count = rules.len();
    let ctx = Arc::new(EngineContext {
        service,
        rules: RwLock::new(Arc::new(rules)),
        minter,
        executor,
        local,
        fact_log,
    });
    let managed = ManagedRulesHandle::new(config_root, ctx.clone());

    let (tx, mut rx) = mpsc::channel::<DomainEvent>(RULE_EVENT_QUEUE_CAPACITY);

    // Forwarder: subscribe to the bus, never block it. `try_send` drops the
    // newest fact when the evaluator is behind; a `Lagged` broadcast is skipped
    // (the next matching update recovers the rule outcome).
    let mut bus = event_sender.subscribe();
    let forwarder = tokio::spawn(async move {
        loop {
            match bus.recv().await {
                Ok(event) => {
                    let _ = tx.try_send(event);
                }
                // Lag: skip the dropped fact and keep looping (the match is the
                // loop's tail, so falling through re-iterates).
                Err(broadcast::error::RecvError::Lagged(_)) => {}
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    });

    let eval_ctx = ctx;
    let evaluator = tokio::spawn(async move {
        while let Some(event) = rx.recv().await {
            eval_ctx.handle_event(event).await;
        }
    });

    ph_info!(
        events::RULE_ENGINE_STARTED,
        rule_count,
        "rule engine started"
    );

    RuleEngineHandle {
        tasks: vec![forwarder, evaluator],
        managed,
    }
}

impl EngineContext {
    /// Evaluate every rule against one triggering fact. Only message-scoped facts
    /// (those naming a message) drive content rules today.
    async fn handle_event(&self, event: DomainEvent) {
        let Some(message_id) = event.message_id.clone() else {
            return;
        };
        // Take one consistent Arc snapshot of the active rules for this event: a
        // concurrent reload swaps the pointer but never mutates the snapshot this
        // evaluation holds (the race-safety argument on `EngineContext.rules`).
        let rules = self.rules_snapshot();
        for rule in rules.iter() {
            if !rule
                .trigger_topics()
                .iter()
                .any(|topic| topic == &event.topic)
            {
                continue;
            }
            let matched =
                self.match_message(&rule.when, &event.account_id, &message_id, &rule.action);
            let summary = match matched {
                Ok(Some(summary)) => summary,
                Ok(None) => continue, // WHEN-clause did not match this message
                Err(error) => {
                    ph_warn!(
                        events::RULE_EVALUATION_FAILED,
                        rule_id = %rule.id,
                        error = %error,
                        "rule match query failed"
                    );
                    continue;
                }
            };
            self.execute(rule, &event, &summary).await;
        }
    }

    /// Does `when` match the one message the fact names? Reuses the smart-mailbox
    /// query path (the codebase's predicate path): AND the account + this message
    /// id + the WHEN tree + an action precondition, then run the indexed query
    /// scoped to a single row. A non-empty page ⇒ the rule matches.
    ///
    /// The action precondition (e.g. "not already tagged") both prevents a
    /// no-op re-apply and breaks the self-trigger loop a Level-0 action's own
    /// `message.updated` would otherwise cause.
    fn match_message(
        &self,
        when: &SmartMailboxRule,
        account_id: &AccountId,
        message_id: &MessageId,
        action: &RuleAction,
    ) -> Result<Option<MessageSummary>, posthaste_contract_core::RuntimeError> {
        let scoped = scoped_match_rule(when, account_id, message_id, action);
        let page = self.service.query_message_page_by_rule(
            &scoped,
            1,
            None,
            MessageSortField::Date,
            SortDirection::Desc,
        )?;
        Ok(page.items.into_iter().next())
    }

    /// Execute a matched rule's action and emit the `rule.fired` fact.
    async fn execute(&self, rule: &Rule, event: &DomainEvent, summary: &MessageSummary) {
        let outcome = match &rule.action {
            RuleAction::Tag { tag } => self.apply_tag(summary, tag).await,
            RuleAction::Move { mailbox_id } => self.apply_move(summary, mailbox_id).await,
            RuleAction::Notify { title, body } => self.apply_notify(rule, summary, title, body),
            // Emit takes no action beyond the `rule.fired` fact the caller emits.
            RuleAction::Emit => RuleOutcome::Applied,
            RuleAction::Webhook { .. } | RuleAction::Exec { .. } => {
                self.run_hook(rule, event, summary).await
            }
        };
        self.emit_rule_fired(rule, event.seq, rule.action.kind_str(), outcome, summary)
            .await;
    }

    async fn apply_tag(&self, summary: &MessageSummary, tag: &str) -> RuleOutcome {
        let op = MailOperation::SetUserTags(MessageSetUserTagsArgs {
            source_id: summary.source_id.to_string(),
            message_id: summary.id.to_string(),
            add: vec![tag.to_string()],
            remove: Vec::new(),
        });
        self.apply(op).await
    }

    async fn apply_move(&self, summary: &MessageSummary, mailbox_id: &MailboxId) -> RuleOutcome {
        let op = MailOperation::ReplaceMailboxes(MessageReplaceMailboxesArgs {
            source_id: summary.source_id.to_string(),
            message_id: summary.id.to_string(),
            mailbox_ids: vec![mailbox_id.to_string()],
        });
        self.apply(op).await
    }

    async fn apply(&self, op: MailOperation) -> RuleOutcome {
        match self.local.apply(op).await {
            Ok(_) => RuleOutcome::Applied,
            Err(error) => {
                ph_warn!(
                    events::RULE_ACTION_APPLY_FAILED,
                    error = %error,
                    "rule level-0 action apply failed"
                );
                RuleOutcome::Failed
            }
        }
    }

    fn apply_notify(
        &self,
        rule: &Rule,
        summary: &MessageSummary,
        title: &str,
        body: &Option<String>,
    ) -> RuleOutcome {
        // `notify` surfaces on the tap: the `rule.fired` fact (emitted by the
        // caller) carries the outcome, and the title/body are logged for the
        // notification surface to consume. No external side effect.
        ph_info!(
            events::RULE_NOTIFY,
            rule_id = %rule.id,
            message_id = %summary.id,
            title = %title,
            body = body.as_deref().unwrap_or(""),
            "rule notify"
        );
        RuleOutcome::Applied
    }

    /// Emit the `rule.fired` fact (RFC §8: a rule-action invocation is itself a
    /// fact — "scriptable and auditable through the same tap"). Durable (RFC-
    /// L2-scripting D52/S3): appends through the authority server's own
    /// [`FactLog`] binding, which assigns a real seq and persists into
    /// `event_log` before broadcasting — the same durable path `/v1/events`
    /// already replays from, so a subscriber that reconnects after this fires
    /// still sees it (previously it was bus-only with `seq: 0` and simply
    /// vanished on reconnect — no fact, no gap frame, silent data loss).
    async fn emit_rule_fired(
        &self,
        rule: &Rule,
        event_seq: i64,
        action_kind: &str,
        outcome: RuleOutcome,
        summary: &MessageSummary,
    ) {
        let payload = RuleFired {
            rule_id: rule.id.clone(),
            event_seq,
            action_kind: action_kind.to_string(),
            outcome,
        };
        if let Err(error) = self
            .fact_log
            .append(DomainEvent {
                seq: 0,
                account_id: summary.source_id.clone(),
                topic: EVENT_TOPIC_RULE_FIRED.to_string(),
                occurred_at: now_iso8601().unwrap_or_default(),
                mailbox_id: None,
                message_id: Some(summary.id.clone()),
                payload: serde_json::to_value(payload).unwrap_or(serde_json::Value::Null),
            })
            .await
        {
            ph_warn!(
                events::RULE_ACTION_APPLY_FAILED,
                rule_id = %rule.id,
                error = %error,
                "failed to durably append the rule.fired fact; the action ran but is unobservable on the tap"
            );
        }
    }

    /// Emit the `rule.delivery.failed` dead-letter fact (ruling 5): a hook whose
    /// bounded retries were exhausted (or that could not be attempted). Durable
    /// for the same reason [`emit_rule_fired`](Self::emit_rule_fired) is: dead-
    /// letter state must survive a reconnect, not just a live subscriber.
    pub(crate) async fn emit_delivery_failed(
        &self,
        rule: &Rule,
        event_seq: i64,
        reason: String,
        attempts: u32,
        summary: &MessageSummary,
    ) {
        ph_warn!(
            events::RULE_DELIVERY_FAILED,
            rule_id = %rule.id,
            reason = %reason,
            attempts,
            "rule hook delivery failed"
        );
        let payload = RuleDeliveryFailed {
            rule_id: rule.id.clone(),
            event_seq,
            reason,
            attempts,
        };
        if let Err(error) = self
            .fact_log
            .append(DomainEvent {
                seq: 0,
                account_id: summary.source_id.clone(),
                topic: EVENT_TOPIC_RULE_DELIVERY_FAILED.to_string(),
                occurred_at: now_iso8601().unwrap_or_default(),
                mailbox_id: None,
                message_id: Some(summary.id.clone()),
                payload: serde_json::to_value(payload).unwrap_or(serde_json::Value::Null),
            })
            .await
        {
            ph_warn!(
                events::RULE_DELIVERY_FAILED,
                rule_id = %rule.id,
                error = %error,
                "failed to durably append the rule.delivery.failed fact"
            );
        }
    }
}

/// The deterministic idempotency key a hook payload carries so an at-least-once
/// redelivery reproduces it exactly: `f(rule_id, event_seq)` (D53). The handler
/// passes it as `Idempotency-Key` on its write-back.
pub(crate) fn idempotency_key(rule_id: &str, event_seq: i64) -> String {
    format!("rule:{rule_id}:{event_seq}")
}

/// Build the single-message match query: `SourceId == account AND MessageId IN
/// [id] AND (when) AND precondition(action)`. This mirrors the ingestion-time
/// automation matcher (`automation_query_rule`) — the established predicate path.
fn scoped_match_rule(
    when: &SmartMailboxRule,
    account_id: &AccountId,
    message_id: &MessageId,
    action: &RuleAction,
) -> SmartMailboxRule {
    let mut nodes = vec![
        condition(
            SmartMailboxField::SourceId,
            SmartMailboxOperator::Equals,
            SmartMailboxValue::String(account_id.to_string()),
        ),
        condition(
            SmartMailboxField::MessageId,
            SmartMailboxOperator::In,
            SmartMailboxValue::Strings(vec![message_id.to_string()]),
        ),
        SmartMailboxRuleNode::Group(when.root.clone()),
    ];
    if let Some(precondition) = action_precondition(action) {
        nodes.push(precondition);
    }
    SmartMailboxRule {
        root: SmartMailboxGroup {
            operator: SmartMailboxGroupOperator::All,
            negated: false,
            nodes,
        },
    }
}

/// A precondition that makes a Level-0 action idempotent and loop-free: skip the
/// message when the effect already holds. Level-1 hooks have no precondition —
/// they fire once per triggering fact and dedupe downstream via the idempotency
/// key.
fn action_precondition(action: &RuleAction) -> Option<SmartMailboxRuleNode> {
    match action {
        RuleAction::Tag { tag } => Some(negated_condition(
            SmartMailboxField::Keyword,
            SmartMailboxOperator::Equals,
            SmartMailboxValue::String(tag.clone()),
        )),
        RuleAction::Move { mailbox_id } => Some(negated_condition(
            SmartMailboxField::MailboxId,
            SmartMailboxOperator::Equals,
            SmartMailboxValue::String(mailbox_id.to_string()),
        )),
        RuleAction::Notify { .. }
        | RuleAction::Emit
        | RuleAction::Webhook { .. }
        | RuleAction::Exec { .. } => None,
    }
}

fn condition(
    field: SmartMailboxField,
    operator: SmartMailboxOperator,
    value: SmartMailboxValue,
) -> SmartMailboxRuleNode {
    SmartMailboxRuleNode::Condition(SmartMailboxCondition {
        field,
        operator,
        negated: false,
        value,
    })
}

fn negated_condition(
    field: SmartMailboxField,
    operator: SmartMailboxOperator,
    value: SmartMailboxValue,
) -> SmartMailboxRuleNode {
    SmartMailboxRuleNode::Condition(SmartMailboxCondition {
        field,
        operator,
        negated: true,
        value,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use posthaste_domain_model::RuleGrant;

    fn when_subject_contains(term: &str) -> SmartMailboxRule {
        posthaste_query_grammar::parse_query(term).expect("parse when")
    }

    /// The single-message match query ANDs the account, this message id, and the
    /// WHEN tree — the established predicate path — so the store query is scoped
    /// to exactly one candidate row.
    fn rule_with_id(id: &str) -> Rule {
        Rule {
            id: id.into(),
            name: id.into(),
            when: when_subject_contains("subject:x"),
            on: Vec::new(),
            action: RuleAction::Notify {
                title: "t".into(),
                body: None,
            },
            enabled: true,
        }
    }

    #[test]
    fn rules_without_evicts_the_id_so_a_delete_always_stops_the_rule() {
        // The delete-path fallback (review finding): when a post-delete reload
        // fails, the deleted rule must still be evicted from the live snapshot.
        let rules = vec![rule_with_id("a"), rule_with_id("b"), rule_with_id("c")];
        let kept = rules_without(&rules, "b").expect("b was present");
        assert_eq!(
            kept.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
            ["a", "c"]
        );
        // Absent id → None (no needless swap).
        assert!(rules_without(&rules, "missing").is_none());
    }

    #[test]
    fn scoped_match_binds_account_and_single_message() {
        let when = when_subject_contains("subject:invoice");
        let scoped = scoped_match_rule(
            &when,
            &AccountId::from("acct-1"),
            &MessageId::from("msg-1"),
            &RuleAction::Notify {
                title: "t".into(),
                body: None,
            },
        );
        assert_eq!(scoped.root.operator, SmartMailboxGroupOperator::All);
        // account + message + when group (notify has no precondition).
        assert_eq!(scoped.root.nodes.len(), 3);
        let has_source = scoped.root.nodes.iter().any(|node| {
            matches!(node, SmartMailboxRuleNode::Condition(c)
                if c.field == SmartMailboxField::SourceId
                && c.value == SmartMailboxValue::String("acct-1".into()))
        });
        let has_message = scoped.root.nodes.iter().any(|node| {
            matches!(node, SmartMailboxRuleNode::Condition(c)
                if c.field == SmartMailboxField::MessageId
                && c.value == SmartMailboxValue::Strings(vec!["msg-1".into()]))
        });
        assert!(has_source && has_message);
    }

    /// A Level-0 tag action gets a "not already tagged" precondition — both
    /// idempotent (no re-apply) and loop-free (the action's own
    /// `message.updated` no longer matches).
    #[test]
    fn tag_action_adds_negated_keyword_precondition() {
        let precondition = action_precondition(&RuleAction::Tag { tag: "done".into() })
            .expect("tag has a precondition");
        match precondition {
            SmartMailboxRuleNode::Condition(condition) => {
                assert_eq!(condition.field, SmartMailboxField::Keyword);
                assert!(condition.negated);
                assert_eq!(condition.value, SmartMailboxValue::String("done".into()));
            }
            other => panic!("expected a condition, got {other:?}"),
        }
    }

    /// Level-1 hooks have no precondition: they fire once per triggering fact and
    /// dedupe downstream via the idempotency key.
    #[test]
    fn hook_actions_have_no_precondition() {
        assert!(action_precondition(&RuleAction::Webhook {
            url: "https://x".into(),
            grants: vec![RuleGrant::Read],
            expiry_seconds: 60,
        })
        .is_none());
    }

    /// The deterministic idempotency key is `f(rule_id, event_seq)`, so a
    /// redelivery reproduces it exactly.
    #[test]
    fn idempotency_key_is_deterministic() {
        assert_eq!(idempotency_key("tagger", 42), "rule:tagger:42");
        assert_eq!(idempotency_key("tagger", 42), idempotency_key("tagger", 42));
        assert_ne!(idempotency_key("tagger", 42), idempotency_key("tagger", 43));
    }
}
