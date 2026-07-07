//! The event-bus automation **rule** model (RFC-L2-scripting S5, levels 0-1).
//!
//! A [`Rule`] runs *at the authority server*, in-process on the event bus — the
//! same fact-carrying tap every other consumer rides (RFC §8). It is distinct
//! from the ingestion-time [`AutomationRule`](super::AutomationRule): a `Rule`
//! reacts to a **fact** (a [`DomainEvent`](super::DomainEvent)) already on the
//! bus, evaluates its WHEN-clause against the message the fact names, and — on a
//! match — executes exactly one [`RuleAction`], up to a webhook or a local
//! script driven by a **per-invocation, attenuated** capability token (D53).
//!
//! One grammar (ruling 4): [`Rule::when`] is a [`MailQueryRule`], the shared
//! query grammar's output, reused verbatim — the same tree that powers smart
//! mailboxes, not a parallel rule language.

use super::*;
use crate::vocab::MailboxRole;

/// The default trigger topics when a [`Rule`] leaves [`on`](Rule::on) empty: the
/// message-update family. `message.updated` is the fact emitted whenever a
/// message is ingested or its state changes, so it is the natural default for
/// content rules ("when a message tagged X arrives, …").
pub const DEFAULT_RULE_TRIGGER_TOPICS: &[&str] = &[EVENT_TOPIC_MESSAGE_UPDATED];

/// An automation rule evaluated at the authority server against facts on the tap
/// (RFC-L2-scripting §8). The rule owns *what it reacts to* ([`on`](Rule::on))
/// and *what it matches* ([`when`](Rule::when)); a hook target is a pure handler
/// and never self-registers (ruling 13).
///
/// Rules are authored in a config-root `rules.toml` file (beta cut, ruling 6);
/// the REST surface is read-only list + preview. See the crate docs for the
/// **exec trust model**: [`RuleAction::Exec`] is config-file-only and can never
/// be set over REST — a REST-settable exec would be remote code execution.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Rule {
    pub id: String,
    pub name: String,
    /// The WHEN-clause: the shared query grammar's [`MailQueryRule`] output,
    /// reused (not wrapped). Evaluated against the message a triggering fact
    /// names — a match means the action runs.
    pub when: MailQueryRule,
    /// The event topics whose facts trigger evaluation. Empty ⇒
    /// [`DEFAULT_RULE_TRIGGER_TOPICS`] (the message-update family).
    #[serde(default)]
    pub on: Vec<String>,
    pub action: RuleAction,
    pub enabled: bool,
    /// Rule chaining (the explicit semantics): for one triggering fact, EVERY
    /// enabled rule whose topics + WHEN-clause match runs, in the engine's
    /// deterministic order (authored `rules.toml` rules first, in file order;
    /// then GUI-managed rules sorted by name, then id). A matched rule with
    /// `stop_processing = true` short-circuits that walk — no later rule is
    /// evaluated against this fact. Defaults to `false` (all matches run),
    /// which was the previous, implicit behaviour.
    #[serde(default)]
    pub stop_processing: bool,
}

impl Rule {
    /// The effective trigger topics: [`on`](Rule::on) if non-empty, else the
    /// message-update default.
    pub fn trigger_topics(&self) -> Vec<String> {
        if self.on.is_empty() {
            DEFAULT_RULE_TRIGGER_TOPICS
                .iter()
                .map(|topic| topic.to_string())
                .collect()
        } else {
            self.on.clone()
        }
    }
}

/// What a matched [`Rule`] does. Level 0 (`Tag`/`Move`/`Notify`) acts through the
/// authority server's own Api surface in-process. Level 1 (`Webhook`/`Exec`)
/// reaches outside the process under a per-invocation attenuated token minted to
/// exactly the rule's [`grants`](RuleAction::Webhook::grants) and expiry (D53).
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "kind"
)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum RuleAction {
    /// Level 0: add a user tag to the matched message (through `apply`).
    Tag { tag: String },
    /// Level 0: move the matched message to a single mailbox (through `apply`).
    Move {
        #[cfg_attr(feature = "openapi", schema(rename = "mailboxId"))]
        mailbox_id: MailboxId,
    },
    /// Level 0: move the matched message to the account's mailbox carrying a
    /// well-known role — `archive`, `junk`, `trash`, or `inbox` (see
    /// [`ASSIGNABLE_RULE_MOVE_ROLES`]). This is how a rule archives, junks, or
    /// trashes a message WITHOUT naming a per-account mailbox id, so one rule
    /// works across every account. Trash is a recoverable move (the provider's
    /// Trash semantics) — permanent deletion is the separate, explicit
    /// [`Destroy`](Self::Destroy).
    MoveToRole { role: MailboxRole },
    /// Level 0: set the matched message's read state (mark read / unread).
    MarkRead { read: bool },
    /// Level 0: set the matched message's flagged state (flag / unflag).
    Flag { flagged: bool },
    /// Level 0: raise a notification fact for the matched message. The notice
    /// surfaces on the tap (`rule.fired`); no external side effect.
    Notify {
        title: String,
        #[serde(default)]
        body: Option<String>,
    },
    /// Level 0, **mail-destructive**: permanently destroy the matched message
    /// through the existing delete-permanently machinery (`message.destroy` —
    /// the same command the GUI's delete-permanently uses; no new deletion
    /// path). This is NOT "move to Trash" (that is
    /// [`MoveToRole`](Self::MoveToRole) with `trash`): a destroyed message is
    /// unrecoverable.
    ///
    /// Guard rails: the wire name is the distinct, explicit `destroy` (never
    /// overloaded onto move/trash), and [`validate_rule_action`] refuses a
    /// destroy rule whose WHEN-clause has no conditions — a condition-free
    /// destroy would match EVERY message the trigger topic names, and a rules
    /// engine that silently destroys all mail on an empty clause is the exact
    /// failure mode to make unrepresentable. It executes only on a message the
    /// boundary-validated WHEN-clause query matched (the engine re-runs the
    /// scoped query per fact — no bypass around the stored, validated rule).
    Destroy,
    /// Level 0: emit ONLY the `rule.fired` fact and take no other action — the
    /// **central-evaluate / edge-execute** primitive (RFC §7.19). Pair an `emit`
    /// rule at the always-on authority server with a client-side, rule-filtered
    /// `watch` on the edge: the AS decides *whether* to act (the WHEN-clause runs
    /// once, centrally), and the client machine decides *how* (it handles the
    /// `rule.fired` fact filtered to this rule). No token is minted — the fact is
    /// the whole output.
    Emit,
    /// Level 1: POST the fact + a message summary + a scoped token to a URL. The
    /// handler acts back through `apply`/`send` under that token — its authority
    /// cannot exceed [`grants`](RuleAction::Webhook::grants). At-least-once with
    /// bounded retry; an exhausted delivery dead-letters as a `rule.delivery.failed`
    /// fact (ruling 5).
    Webhook {
        url: String,
        #[serde(default)]
        grants: Vec<RuleGrant>,
        #[serde(default = "default_hook_expiry_seconds")]
        expiry_seconds: u64,
    },
    /// Level 1: run a LOCAL script on the authority-server host with the payload
    /// delivered as JSON **on stdin** and the scoped token in the environment.
    ///
    /// PAYLOAD-IS-DATA (RFC-L2-scripting §7.20) — there is deliberately **no
    /// argument template**. Event/message data reaches the script only as the
    /// JSON stdin document, never interpolated into a command string or argv, so
    /// a malicious sender cannot inject a command. `command` names a fixed host
    /// binary; the payload is pure data.
    ///
    /// TRUST MODEL — exec is **config-file-only**. It is set only by editing
    /// `rules.toml` on the host; it is NEVER settable over REST. A REST-settable
    /// exec action would be remote code execution: anyone who could write a rule
    /// could run arbitrary commands on the server. The read-only REST surface
    /// lists exec rules but cannot create or edit them.
    Exec {
        command: String,
        #[serde(default)]
        grants: Vec<RuleGrant>,
        #[serde(default = "default_hook_expiry_seconds")]
        expiry_seconds: u64,
    },
}

/// The **write-surface** projection of [`RuleAction`]: every variant EXCEPT
/// [`RuleAction::Exec`] (RFC-L2-scripting ruling 23).
///
/// This is the body type the REST rule-write routes (`POST`/`PUT` `/v1/rules`)
/// deserialize into. Because `Exec` is *not a variant here*, a request body of
/// `{"kind":"exec", …}` is **unrepresentable**: it fails at the serde boundary
/// (a 422 deserialize error), never reaching a handler. The
/// exec-is-config-file-only invariant (a REST-settable exec = RCE, threat 3) is
/// therefore **structural** — the GUI/REST path cannot create an exec rule
/// because the type it parses into has no exec case. This replaces a fragile
/// runtime `if kind == "exec" { reject }` guard with a type the compiler and
/// serde enforce for us.
///
/// A [`From<WritableRuleAction>`] lifts a validated write action back into the
/// full [`RuleAction`] the engine and persistence layer speak.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "kind"
)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum WritableRuleAction {
    /// See [`RuleAction::Tag`].
    Tag { tag: String },
    /// See [`RuleAction::Move`].
    Move {
        #[cfg_attr(feature = "openapi", schema(rename = "mailboxId"))]
        mailbox_id: MailboxId,
    },
    /// See [`RuleAction::MoveToRole`].
    MoveToRole { role: MailboxRole },
    /// See [`RuleAction::MarkRead`].
    MarkRead { read: bool },
    /// See [`RuleAction::Flag`].
    Flag { flagged: bool },
    /// See [`RuleAction::Notify`].
    Notify {
        title: String,
        #[serde(default)]
        body: Option<String>,
    },
    /// See [`RuleAction::Destroy`] — mail-destructive, guarded by
    /// [`validate_rule_action`] (a destroy rule must carry a non-empty
    /// WHEN-clause).
    Destroy,
    /// See [`RuleAction::Emit`].
    Emit,
    /// See [`RuleAction::Webhook`].
    Webhook {
        url: String,
        #[serde(default)]
        grants: Vec<RuleGrant>,
        #[serde(default = "default_hook_expiry_seconds")]
        expiry_seconds: u64,
    },
    // NOTE: there is deliberately NO `Exec` variant — that is the whole point of
    // this type. Do not add one; exec stays config-file-only (ruling 23).
}

impl From<WritableRuleAction> for RuleAction {
    fn from(action: WritableRuleAction) -> Self {
        match action {
            WritableRuleAction::Tag { tag } => RuleAction::Tag { tag },
            WritableRuleAction::Move { mailbox_id } => RuleAction::Move { mailbox_id },
            WritableRuleAction::MoveToRole { role } => RuleAction::MoveToRole { role },
            WritableRuleAction::MarkRead { read } => RuleAction::MarkRead { read },
            WritableRuleAction::Flag { flagged } => RuleAction::Flag { flagged },
            WritableRuleAction::Notify { title, body } => RuleAction::Notify { title, body },
            WritableRuleAction::Destroy => RuleAction::Destroy,
            WritableRuleAction::Emit => RuleAction::Emit,
            WritableRuleAction::Webhook {
                url,
                grants,
                expiry_seconds,
            } => RuleAction::Webhook {
                url,
                grants,
                expiry_seconds,
            },
        }
    }
}

/// The default per-invocation token lifetime for a hook action when the rule
/// omits `expirySeconds`: one hour (the worked-example grant in RFC §9).
pub fn default_hook_expiry_seconds() -> u64 {
    3600
}

impl RuleAction {
    /// A stable discriminator for the `rule.fired` fact's `actionKind` field and
    /// for logs — always the serde wire tag of the variant.
    pub fn kind_str(&self) -> &'static str {
        match self {
            RuleAction::Tag { .. } => "tag",
            RuleAction::Move { .. } => "move",
            RuleAction::MoveToRole { .. } => "moveToRole",
            RuleAction::MarkRead { .. } => "markRead",
            RuleAction::Flag { .. } => "flag",
            RuleAction::Notify { .. } => "notify",
            RuleAction::Destroy => "destroy",
            RuleAction::Emit => "emit",
            RuleAction::Webhook { .. } => "webhook",
            RuleAction::Exec { .. } => "exec",
        }
    }

    /// Whether this action reaches outside the process (Level 1) and therefore
    /// mints a per-invocation capability token.
    pub fn is_hook(&self) -> bool {
        matches!(self, RuleAction::Webhook { .. } | RuleAction::Exec { .. })
    }

    /// Whether this action irreversibly destroys mail. Drives the extra
    /// WHEN-clause guard in [`validate_rule_action`] and the editor's
    /// destructive labelling.
    pub fn is_destructive(&self) -> bool {
        matches!(self, RuleAction::Destroy)
    }
}

/// The mailbox roles a [`RuleAction::MoveToRole`] may target. Deliberately a
/// subset of [`MailboxRole::ALL`]: `drafts`/`sent` are provider-managed
/// surfaces a content rule has no business filing into, and `snooze` requires
/// the paired return-time record the snooze command carries (a bare role move
/// there would hide mail forever) — so all three are refused at validation.
pub const ASSIGNABLE_RULE_MOVE_ROLES: &[MailboxRole] = &[
    MailboxRole::Archive,
    MailboxRole::Junk,
    MailboxRole::Trash,
    MailboxRole::Inbox,
];

/// Rule-action validation shared by EVERY path that persists or loads a rule —
/// the authored `rules.toml` loader, the managed `rules.d` loader, and the
/// managed write path the REST routes call. One definition, so no store can
/// smuggle in a rule another store would refuse:
///
/// * a hook action (webhook/exec) must declare at least one grant (the F1
///   security-review rule — empty grants would mint an action-unrestricted
///   token);
/// * a `moveToRole` must target an assignable role
///   ([`ASSIGNABLE_RULE_MOVE_ROLES`]);
/// * a `destroy` must be paired with a WHEN-clause that has at least one
///   condition — a condition-free destroy matches every message on the
///   trigger topic, and permanent mail destruction must never be one empty
///   clause away.
pub fn validate_rule_action(action: &RuleAction, when: &MailQueryRule) -> Result<(), String> {
    match action {
        RuleAction::Webhook { grants, .. } | RuleAction::Exec { grants, .. } => {
            if grants.is_empty() {
                return Err("a webhook/exec rule must declare at least one grant".to_string());
            }
        }
        RuleAction::MoveToRole { role } => {
            if !ASSIGNABLE_RULE_MOVE_ROLES.contains(role) {
                return Err(format!(
                    "a rule cannot move messages to the '{}' role (allowed: archive, junk, trash, inbox)",
                    role.as_str()
                ));
            }
        }
        RuleAction::Destroy if !group_has_condition(&when.root) => {
            return Err(
                "a destroy rule must have at least one condition in its when-clause — \
                 an unconditional destroy would permanently delete every matching-topic message"
                    .to_string(),
            );
        }
        _ => {}
    }
    Ok(())
}

/// Does this group (recursively) contain at least one leaf condition?
fn group_has_condition(group: &MailQueryGroup) -> bool {
    group.nodes.iter().any(|node| match node {
        MailQueryRuleNode::Condition(_) => true,
        MailQueryRuleNode::Group(inner) => group_has_condition(inner),
    })
}

/// A single capability a hook action's per-invocation token carries. These are
/// the substantive authz verbs (never `mint`/`manage`): a hook token is a
/// least-privilege credential scoped to exactly what the handler needs, over
/// exactly the matched message.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum RuleGrant {
    Read,
    Send,
    Tag,
    Move,
    Delete,
}

impl RuleGrant {
    /// The authz caveat verb string (matches the `Action` vocabulary the token
    /// perimeter evaluates). See the token caveat format in the http adapter's
    /// `build_token_caveats`.
    pub fn verb(&self) -> &'static str {
        match self {
            RuleGrant::Read => "read",
            RuleGrant::Send => "send",
            RuleGrant::Tag => "tag",
            RuleGrant::Move => "move",
            RuleGrant::Delete => "delete",
        }
    }
}

/// Outcome of one rule-action execution, carried on the `rule.fired` fact.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum RuleOutcome {
    /// A Level-0 action applied (or was a no-op because the effect already held).
    Applied,
    /// A Level-1 hook was delivered (webhook 2xx, or the script exited 0).
    Delivered,
    /// The action did not run to a side effect (e.g. no capability minter for a
    /// hook action); see the `rule.delivery.failed` fact for the reason.
    Failed,
}

/// Payload of the [`EVENT_TOPIC_RULE_FIRED`] fact: emitted every time a rule's
/// WHEN-clause matched and its action executed.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct RuleFired {
    pub rule_id: String,
    /// The seq of the triggering fact — half of the deterministic idempotency
    /// key `f(rule_id, event_seq)` the hook payload carries.
    pub event_seq: i64,
    pub action_kind: String,
    pub outcome: RuleOutcome,
}

/// Payload of the [`EVENT_TOPIC_RULE_DELIVERY_FAILED`] dead-letter fact: emitted
/// when a hook delivery is abandoned after its bounded retry schedule is
/// exhausted (ruling 5).
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct RuleDeliveryFailed {
    pub rule_id: String,
    pub event_seq: i64,
    pub reason: String,
    pub attempts: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_on_defaults_to_message_update_family() {
        let rule = Rule {
            id: "r".into(),
            name: "r".into(),
            when: MailQueryRule {
                root: MailQueryGroup {
                    operator: MailQueryGroupOperator::All,
                    negated: false,
                    nodes: Vec::new(),
                },
            },
            on: Vec::new(),
            action: RuleAction::Tag { tag: "x".into() },
            enabled: true,
            stop_processing: false,
        };
        assert_eq!(
            rule.trigger_topics(),
            vec![EVENT_TOPIC_MESSAGE_UPDATED.to_string()]
        );
    }

    /// `stopProcessing` is additive and optional on the wire: a legacy rule
    /// document without the field parses with the old (run-everything)
    /// semantics.
    #[test]
    fn stop_processing_defaults_to_false_on_the_wire() {
        let rule: Rule = serde_json::from_value(serde_json::json!({
            "id": "r", "name": "r",
            "when": { "root": { "operator": "all", "negated": false, "nodes": [] } },
            "action": { "kind": "emit" },
            "enabled": true,
        }))
        .expect("legacy rule without stopProcessing parses");
        assert!(!rule.stop_processing);

        let with_flag: Rule = serde_json::from_value(serde_json::json!({
            "id": "r", "name": "r",
            "when": { "root": { "operator": "all", "negated": false, "nodes": [] } },
            "action": { "kind": "emit" },
            "enabled": true,
            "stopProcessing": true,
        }))
        .expect("rule with stopProcessing parses");
        assert!(with_flag.stop_processing);
    }

    /// The security gate (ruling 23): a `{"kind":"exec",…}` body is
    /// UNREPRESENTABLE as a [`WritableRuleAction`] — it fails at the serde
    /// boundary, so a REST/GUI write can never carry an exec action. This is the
    /// structural form of the exec-is-config-file-only invariant.
    #[test]
    fn writable_action_rejects_exec_at_the_serde_boundary() {
        let exec = serde_json::json!({
            "kind": "exec",
            "command": "/bin/rm",
            "grants": ["read"],
        });
        let result: Result<WritableRuleAction, _> = serde_json::from_value(exec);
        assert!(
            result.is_err(),
            "an exec action must not deserialize into WritableRuleAction"
        );
    }

    /// Every safe variant round-trips into the full [`RuleAction`] via `From`,
    /// so the write surface loses nothing but exec.
    #[test]
    fn writable_action_lifts_into_rule_action() {
        let writable = WritableRuleAction::Webhook {
            url: "https://vm/agent".into(),
            grants: vec![RuleGrant::Read],
            expiry_seconds: 900,
        };
        let lifted: RuleAction = writable.into();
        assert_eq!(lifted.kind_str(), "webhook");
        assert!(lifted.is_hook());

        let tag: RuleAction = WritableRuleAction::Tag { tag: "x".into() }.into();
        assert_eq!(tag, RuleAction::Tag { tag: "x".into() });
        let emit: RuleAction = WritableRuleAction::Emit.into();
        assert_eq!(emit, RuleAction::Emit);
    }

    /// A safe action deserializes into `WritableRuleAction` exactly as it would
    /// into `RuleAction` — the two share the wire shape for every safe kind.
    #[test]
    fn writable_action_accepts_safe_kinds() {
        for body in [
            serde_json::json!({"kind": "tag", "tag": "x"}),
            serde_json::json!({"kind": "move", "mailboxId": "inbox"}),
            serde_json::json!({"kind": "moveToRole", "role": "archive"}),
            serde_json::json!({"kind": "markRead", "read": true}),
            serde_json::json!({"kind": "markRead", "read": false}),
            serde_json::json!({"kind": "flag", "flagged": true}),
            serde_json::json!({"kind": "notify", "title": "hi"}),
            serde_json::json!({"kind": "destroy"}),
            serde_json::json!({"kind": "emit"}),
            serde_json::json!({"kind": "webhook", "url": "http://127.0.0.1/h", "grants": ["read"]}),
        ] {
            let parsed: Result<WritableRuleAction, _> = serde_json::from_value(body.clone());
            assert!(parsed.is_ok(), "safe action {body} must deserialize");
            // Wire parity: the lifted RuleAction re-serializes with the same
            // `kind` tag the writable body carried, and `kind_str` matches it.
            let lifted: RuleAction = parsed.expect("parsed").into();
            let round = serde_json::to_value(&lifted).expect("serialize");
            assert_eq!(round["kind"], body["kind"], "kind tag drift for {body}");
            assert_eq!(lifted.kind_str(), body["kind"].as_str().expect("kind"));
        }
    }

    fn when_with_condition() -> MailQueryRule {
        MailQueryRule {
            root: MailQueryGroup {
                operator: MailQueryGroupOperator::All,
                negated: false,
                nodes: vec![MailQueryRuleNode::Condition(MailQueryCondition {
                    field: MailQueryField::Keyword,
                    operator: MailQueryOperator::Equals,
                    negated: false,
                    value: MailQueryValue::String("x".into()),
                })],
            },
        }
    }

    fn empty_when() -> MailQueryRule {
        MailQueryRule {
            root: MailQueryGroup {
                operator: MailQueryGroupOperator::All,
                negated: false,
                nodes: Vec::new(),
            },
        }
    }

    /// The destroy guard rail: a destroy action with a condition-free
    /// WHEN-clause is refused (it would permanently delete every message on the
    /// trigger topic); with at least one condition — including one nested in a
    /// sub-group — it validates.
    #[test]
    fn destroy_requires_a_non_empty_when_clause() {
        assert!(validate_rule_action(&RuleAction::Destroy, &empty_when()).is_err());
        assert!(validate_rule_action(&RuleAction::Destroy, &when_with_condition()).is_ok());

        // A nested condition (inside a sub-group) also satisfies the guard.
        let nested = MailQueryRule {
            root: MailQueryGroup {
                operator: MailQueryGroupOperator::Any,
                negated: false,
                nodes: vec![MailQueryRuleNode::Group(when_with_condition().root)],
            },
        };
        assert!(validate_rule_action(&RuleAction::Destroy, &nested).is_ok());

        // An empty sub-group does NOT satisfy it — no leaf condition anywhere.
        let empty_nested = MailQueryRule {
            root: MailQueryGroup {
                operator: MailQueryGroupOperator::Any,
                negated: false,
                nodes: vec![MailQueryRuleNode::Group(empty_when().root)],
            },
        };
        assert!(validate_rule_action(&RuleAction::Destroy, &empty_nested).is_err());
    }

    /// A non-destructive action is fine with an empty WHEN-clause (matches the
    /// pre-existing behaviour — e.g. "emit on every update").
    #[test]
    fn non_destructive_actions_allow_an_empty_when_clause() {
        assert!(validate_rule_action(&RuleAction::Emit, &empty_when()).is_ok());
        assert!(validate_rule_action(&RuleAction::MarkRead { read: true }, &empty_when()).is_ok());
    }

    /// `moveToRole` only accepts the assignable roles: archive/junk/trash/inbox.
    /// Drafts/sent are provider-managed; snooze needs the paired return time.
    #[test]
    fn move_to_role_is_restricted_to_assignable_roles() {
        for role in [
            MailboxRole::Archive,
            MailboxRole::Junk,
            MailboxRole::Trash,
            MailboxRole::Inbox,
        ] {
            assert!(
                validate_rule_action(&RuleAction::MoveToRole { role }, &when_with_condition())
                    .is_ok(),
                "{role:?} must be assignable"
            );
        }
        for role in [MailboxRole::Drafts, MailboxRole::Sent, MailboxRole::Snooze] {
            assert!(
                validate_rule_action(&RuleAction::MoveToRole { role }, &when_with_condition())
                    .is_err(),
                "{role:?} must be refused"
            );
        }
    }

    /// Destroy is the ONLY destructive action, and it is Level 0 (no hook token).
    #[test]
    fn destroy_is_the_only_destructive_kind() {
        assert!(RuleAction::Destroy.is_destructive());
        assert!(!RuleAction::Destroy.is_hook());
        for action in [
            RuleAction::Tag { tag: "x".into() },
            RuleAction::MoveToRole {
                role: MailboxRole::Trash,
            },
            RuleAction::MarkRead { read: true },
            RuleAction::Flag { flagged: false },
            RuleAction::Emit,
        ] {
            assert!(
                !action.is_destructive(),
                "{} is not destructive",
                action.kind_str()
            );
        }
    }

    #[test]
    fn action_tag_serializes_with_kind_discriminant() {
        let action = RuleAction::Webhook {
            url: "https://vm/agent".into(),
            grants: vec![RuleGrant::Read, RuleGrant::Tag],
            expiry_seconds: 3600,
        };
        let json = serde_json::to_value(&action).expect("serialize");
        assert_eq!(json["kind"], "webhook");
        assert_eq!(json["grants"][0], "read");
        assert_eq!(action.kind_str(), "webhook");
        assert!(action.is_hook());
    }
}
