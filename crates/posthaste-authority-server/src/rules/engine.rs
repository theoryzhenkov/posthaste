//! The rule evaluator: subscribe to the domain-event bus, match a triggering
//! fact's message against each rule's WHEN-clause, and dispatch the action.
//!
//! Concurrency (the M27 discipline): a non-blocking **forwarder** drains the
//! lossy broadcast into a bounded `mpsc` (drop-newest on overflow, so the bus is
//! never back-pressured by a slow rule), and a single **evaluator** task processes
//! the queue in order. One evaluator ⇒ rules never race each other; the bounded
//! queue ⇒ a slow webhook cannot grow memory without bound.

use std::sync::Arc;

use posthaste_contract_core::mutation_args::{
    MessageReplaceMailboxesArgs, MessageSetUserTagsArgs,
};
use posthaste_contract_core::MailOperation;
use posthaste_domain_model::{
    now_iso8601, AccountId, DomainEvent, MailboxId, MessageId, MessageSortField, MessageSummary,
    Rule, RuleAction, RuleDeliveryFailed, RuleFired, RuleOutcome, SmartMailboxCondition,
    SmartMailboxField, SmartMailboxGroup, SmartMailboxGroupOperator, SmartMailboxOperator,
    SmartMailboxRule, SmartMailboxRuleNode, SmartMailboxValue, SortDirection,
    EVENT_TOPIC_RULE_DELIVERY_FAILED, EVENT_TOPIC_RULE_FIRED,
};
use posthaste_observability::{events, ph_info, ph_warn};
use posthaste_provider_call::{ExecutorConfig, ProviderCallExecutor};
use posthaste_authority_server_link::AuthorityServerApi;
use tokio::sync::{broadcast, mpsc};

use crate::authority_server::AuthorityServer;
use crate::local_authority_server::LocalAuthorityServer;
use crate::rules::SharedMinter;
use posthaste_domain_service::MailService;

/// Bound on the in-flight rule-evaluation queue. A backlog beyond this drops the
/// newest fact rather than back-pressuring the event bus (at-least-once means a
/// dropped fact is recoverable by the next matching update; the tap is not the
/// durable path here).
const RULE_EVENT_QUEUE_CAPACITY: usize = 1024;

/// A running rule engine. Dropping it aborts the forwarder + evaluator tasks
/// (they also end on their own when the event bus closes at shutdown).
pub struct RuleEngineHandle {
    tasks: Vec<tokio::task::JoinHandle<()>>,
}

impl Drop for RuleEngineHandle {
    fn drop(&mut self) {
        for task in &self.tasks {
            task.abort();
        }
    }
}

/// Everything the evaluator needs, shared across the tasks.
pub(crate) struct EngineContext {
    pub(crate) service: Arc<MailService>,
    pub(crate) event_sender: broadcast::Sender<DomainEvent>,
    pub(crate) rules: Vec<Rule>,
    pub(crate) minter: Option<SharedMinter>,
    pub(crate) executor: Arc<ProviderCallExecutor>,
    pub(crate) local: Arc<LocalAuthorityServer>,
}

/// Spawn the engine over the authority server's event bus. Returns `None`-free:
/// the caller only spawns when there is at least one enabled rule.
pub(crate) fn spawn(
    authority_server: Arc<AuthorityServer>,
    service: Arc<MailService>,
    event_sender: broadcast::Sender<DomainEvent>,
    rules: Vec<Rule>,
    minter: Option<SharedMinter>,
) -> RuleEngineHandle {
    let executor = Arc::new(
        ProviderCallExecutor::new(ExecutorConfig::default())
            .expect("failed to build the rule webhook HTTP client"),
    );
    let local = Arc::new(LocalAuthorityServer::new(authority_server));
    let rule_count = rules.len();
    let ctx = Arc::new(EngineContext {
        service,
        event_sender: event_sender.clone(),
        rules,
        minter,
        executor,
        local,
    });

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
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    });

    let eval_ctx = ctx.clone();
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
    }
}

impl EngineContext {
    /// Evaluate every rule against one triggering fact. Only message-scoped facts
    /// (those naming a message) drive content rules today.
    async fn handle_event(&self, event: DomainEvent) {
        let Some(message_id) = event.message_id.clone() else {
            return;
        };
        // Snapshot which rules trigger on this topic before any async work, so we
        // do not borrow `self.rules` across an await.
        for index in 0..self.rules.len() {
            let rule = &self.rules[index];
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
            // Clone what the async action needs so no borrow of `self.rules`
            // crosses the await.
            let rule = self.rules[index].clone();
            self.execute(&rule, &event, &summary).await;
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
        self.emit_rule_fired(rule, event.seq, rule.action.kind_str(), outcome, summary);
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
    /// fact). Bus-only, like the other meta-facts (`seq: 0`, the tap stamps it).
    fn emit_rule_fired(
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
        let _ = self.event_sender.send(DomainEvent {
            seq: 0,
            account_id: summary.source_id.clone(),
            topic: EVENT_TOPIC_RULE_FIRED.to_string(),
            occurred_at: now_iso8601().unwrap_or_default(),
            mailbox_id: None,
            message_id: Some(summary.id.clone()),
            payload: serde_json::to_value(payload).unwrap_or(serde_json::Value::Null),
        });
    }

    /// Emit the `rule.delivery.failed` dead-letter fact (ruling 5): a hook whose
    /// bounded retries were exhausted (or that could not be attempted).
    pub(crate) fn emit_delivery_failed(
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
        let _ = self.event_sender.send(DomainEvent {
            seq: 0,
            account_id: summary.source_id.clone(),
            topic: EVENT_TOPIC_RULE_DELIVERY_FAILED.to_string(),
            occurred_at: now_iso8601().unwrap_or_default(),
            mailbox_id: None,
            message_id: Some(summary.id.clone()),
            payload: serde_json::to_value(payload).unwrap_or(serde_json::Value::Null),
        });
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
        assert_eq!(
            idempotency_key("tagger", 42),
            idempotency_key("tagger", 42)
        );
        assert_ne!(idempotency_key("tagger", 42), idempotency_key("tagger", 43));
    }
}
