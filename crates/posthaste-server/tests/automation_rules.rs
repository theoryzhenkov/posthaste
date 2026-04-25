use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use posthaste_config::TomlConfigRepository;
use posthaste_domain::{
    AccountDriver, AccountId, AccountSettings, AccountTransportSettings, AppSettings,
    AutomationAction, AutomationBackfillJobStatus, AutomationRule, AutomationTrigger, BlobId,
    FetchedBody, GatewayError, Identity, MailGateway, MailboxId, MailboxRecord, MessageId,
    MessageRecord, MutationOutcome, PushTransport, ReplyContext, SendMessageRequest,
    SetKeywordsCommand, SmartMailboxCondition, SmartMailboxField, SmartMailboxGroup,
    SmartMailboxGroupOperator, SmartMailboxOperator, SmartMailboxRule, SmartMailboxRuleNode,
    SmartMailboxValue, SyncBatch, SyncCursor, SyncObject, SyncTrigger, ThreadId, RFC3339_EPOCH,
};
use posthaste_domain::{ConfigRepository, MailService};
use posthaste_store::DatabaseStore;

static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

fn temp_root() -> PathBuf {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be after epoch")
        .as_nanos();
    let seq = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("posthaste-automation-test-{now}-{seq}"))
}

fn account(id: &str, name: &str) -> AccountSettings {
    AccountSettings {
        id: AccountId::from(id),
        name: name.to_string(),
        full_name: None,
        email_patterns: Vec::new(),
        driver: AccountDriver::Mock,
        enabled: true,
        appearance: None,
        transport: AccountTransportSettings::default(),
        created_at: RFC3339_EPOCH.to_string(),
        updated_at: RFC3339_EPOCH.to_string(),
    }
}

fn mailbox(id: &str, name: &str, role: Option<&str>) -> MailboxRecord {
    MailboxRecord {
        id: MailboxId::from(id),
        name: name.to_string(),
        role: role.map(str::to_string),
        unread_emails: 0,
        total_emails: 0,
    }
}

fn message(
    id: &str,
    mailbox_ids: &[&str],
    from_name: &str,
    from_email: &str,
    keywords: &[&str],
) -> MessageRecord {
    MessageRecord {
        id: MessageId::from(id),
        source_thread_id: ThreadId::from(format!("thread-{id}")),
        remote_blob_id: None,
        subject: Some(format!("Subject {id}")),
        from_name: Some(from_name.to_string()),
        from_email: Some(from_email.to_string()),
        preview: Some(format!("Preview {id}")),
        received_at: "2026-03-31T10:00:00Z".to_string(),
        has_attachment: false,
        size: 42,
        mailbox_ids: mailbox_ids.iter().map(|id| MailboxId::from(*id)).collect(),
        keywords: keywords.iter().map(|keyword| keyword.to_string()).collect(),
        body_html: None,
        body_text: Some(format!("Body {id}")),
        raw_mime: None,
        rfc_message_id: Some(format!("<{id}@example.test>")),
        in_reply_to: None,
        references: Vec::new(),
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

fn source_is(account_id: &str) -> SmartMailboxRuleNode {
    condition(
        SmartMailboxField::SourceId,
        SmartMailboxOperator::Equals,
        SmartMailboxValue::String(account_id.to_string()),
    )
}

fn from_contains(value: &str) -> SmartMailboxRuleNode {
    SmartMailboxRuleNode::Group(SmartMailboxGroup {
        operator: SmartMailboxGroupOperator::Any,
        negated: false,
        nodes: vec![
            condition(
                SmartMailboxField::FromName,
                SmartMailboxOperator::Contains,
                SmartMailboxValue::String(value.to_string()),
            ),
            condition(
                SmartMailboxField::FromEmail,
                SmartMailboxOperator::Contains,
                SmartMailboxValue::String(value.to_string()),
            ),
        ],
    })
}

fn mailbox_role_is(role: &str) -> SmartMailboxRuleNode {
    condition(
        SmartMailboxField::MailboxRole,
        SmartMailboxOperator::Equals,
        SmartMailboxValue::String(role.to_string()),
    )
}

fn rule(
    id: &str,
    nodes: Vec<SmartMailboxRuleNode>,
    actions: Vec<AutomationAction>,
) -> AutomationRule {
    AutomationRule {
        id: id.to_string(),
        name: id.to_string(),
        enabled: true,
        triggers: vec![AutomationTrigger::MessageArrived],
        condition: SmartMailboxRule {
            root: SmartMailboxGroup {
                operator: SmartMailboxGroupOperator::All,
                negated: false,
                nodes,
            },
        },
        actions,
        backfill: true,
    }
}

struct RuleHarness {
    service: Arc<MailService>,
}

impl RuleHarness {
    fn new() -> Self {
        let root = temp_root();
        let config_root = root.join("config");
        let state_root = root.join("state");
        let config_repo =
            TomlConfigRepository::open(&config_root).expect("config repository should open");
        config_repo
            .initialize_defaults()
            .expect("config defaults should initialize");
        let store = Arc::new(
            DatabaseStore::open(state_root.join("mail.sqlite"), &state_root)
                .expect("database store should open"),
        );
        let config: Arc<dyn ConfigRepository> = Arc::new(config_repo);
        let service = Arc::new(MailService::new(store, config));
        Self { service }
    }

    fn save_account(&self, id: &str, name: &str) {
        self.service
            .save_source(&account(id, name))
            .expect("account should save");
    }

    fn save_rules(&self, rules: Vec<AutomationRule>) {
        self.service
            .put_app_settings(&AppSettings {
                default_account_id: None,
                automation_rules: rules,
            })
            .expect("settings should save");
    }

    async fn sync(&self, account_id: &str, gateway: &ScriptedGateway) {
        self.service
            .sync_account(&AccountId::from(account_id), SyncTrigger::Manual, gateway)
            .await
            .expect("sync should succeed");
    }

    async fn backfill(
        &self,
        account_id: &str,
        gateway: &ScriptedGateway,
        batch_size: usize,
    ) -> bool {
        let (_events, has_more) = self
            .service
            .backfill_automation_rules_batch(&AccountId::from(account_id), gateway, batch_size)
            .await
            .expect("backfill should succeed");
        has_more
    }

    async fn process_backfill_job(
        &self,
        account_id: &str,
        gateway: &ScriptedGateway,
        batch_size: usize,
    ) -> (bool, bool) {
        let outcome = self
            .service
            .process_automation_backfill_job_batch(
                &AccountId::from(account_id),
                gateway,
                batch_size,
            )
            .await
            .expect("backfill job should process");
        (outcome.ran, outcome.has_more)
    }

    fn current_backfill_status(&self, account_id: &str) -> Option<AutomationBackfillJobStatus> {
        self.service
            .automation_backfill_job_for_current_rules(&AccountId::from(account_id))
            .expect("backfill job should load")
            .map(|job| job.status)
    }

    fn message_keywords(&self, account_id: &str, message_id: &str) -> Vec<String> {
        self.service
            .list_messages(&AccountId::from(account_id), None)
            .expect("messages should list")
            .into_iter()
            .find(|message| message.id.as_str() == message_id)
            .expect("message should exist")
            .keywords
    }

    fn message_mailboxes(&self, account_id: &str, message_id: &str) -> Vec<String> {
        self.service
            .list_messages(&AccountId::from(account_id), None)
            .expect("messages should list")
            .into_iter()
            .find(|message| message.id.as_str() == message_id)
            .expect("message should exist")
            .mailbox_ids
            .into_iter()
            .map(|mailbox_id| mailbox_id.to_string())
            .collect()
    }

    fn message_is_read(&self, account_id: &str, message_id: &str) -> bool {
        self.service
            .list_messages(&AccountId::from(account_id), None)
            .expect("messages should list")
            .into_iter()
            .find(|message| message.id.as_str() == message_id)
            .expect("message should exist")
            .is_read
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum RecordedMutation {
    SetKeywords {
        account_id: String,
        message_id: String,
        add: Vec<String>,
        remove: Vec<String>,
    },
    ReplaceMailboxes {
        account_id: String,
        message_id: String,
        mailbox_ids: Vec<String>,
    },
}

struct ScriptedGateway {
    state: Mutex<GatewayState>,
}

struct GatewayState {
    revision: u64,
    mailboxes: Vec<MailboxRecord>,
    messages: BTreeMap<String, MessageRecord>,
    mutations: Vec<RecordedMutation>,
}

impl ScriptedGateway {
    fn new(mailboxes: Vec<MailboxRecord>, messages: Vec<MessageRecord>) -> Self {
        Self {
            state: Mutex::new(GatewayState {
                revision: 1,
                mailboxes,
                messages: messages
                    .into_iter()
                    .map(|message| (message.id.to_string(), message))
                    .collect(),
                mutations: Vec::new(),
            }),
        }
    }

    fn mutations(&self) -> Vec<RecordedMutation> {
        self.state
            .lock()
            .expect("gateway state lock should not be poisoned")
            .mutations
            .clone()
    }
}

fn mutation_outcome(state: &mut GatewayState, object_type: SyncObject) -> MutationOutcome {
    state.revision += 1;
    MutationOutcome {
        cursor: Some(SyncCursor {
            object_type,
            state: format!("{}-{}", object_type.as_str(), state.revision),
            updated_at: RFC3339_EPOCH.to_string(),
        }),
    }
}

#[async_trait]
impl MailGateway for ScriptedGateway {
    async fn sync(
        &self,
        _account_id: &AccountId,
        _cursors: &[SyncCursor],
    ) -> Result<SyncBatch, GatewayError> {
        let state = self
            .state
            .lock()
            .map_err(|_| GatewayError::Rejected("gateway state poisoned".to_string()))?;
        Ok(SyncBatch {
            mailboxes: state.mailboxes.clone(),
            messages: state.messages.values().cloned().collect(),
            deleted_mailbox_ids: Vec::new(),
            deleted_message_ids: Vec::new(),
            replace_all_mailboxes: true,
            replace_all_messages: true,
            cursors: vec![
                SyncCursor {
                    object_type: SyncObject::Mailbox,
                    state: format!("mailbox-{}", state.revision),
                    updated_at: RFC3339_EPOCH.to_string(),
                },
                SyncCursor {
                    object_type: SyncObject::Message,
                    state: format!("message-{}", state.revision),
                    updated_at: RFC3339_EPOCH.to_string(),
                },
            ],
        })
    }

    async fn fetch_message_body(
        &self,
        _account_id: &AccountId,
        _message_id: &MessageId,
    ) -> Result<FetchedBody, GatewayError> {
        Err(GatewayError::Rejected(
            "unused in automation tests".to_string(),
        ))
    }

    async fn download_blob(
        &self,
        _account_id: &AccountId,
        _blob_id: &BlobId,
    ) -> Result<Vec<u8>, GatewayError> {
        Err(GatewayError::Rejected(
            "unused in automation tests".to_string(),
        ))
    }

    async fn set_keywords(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
        _expected_state: Option<&str>,
        command: &SetKeywordsCommand,
    ) -> Result<MutationOutcome, GatewayError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| GatewayError::Rejected("gateway state poisoned".to_string()))?;
        let message = state
            .messages
            .get_mut(message_id.as_str())
            .ok_or_else(|| GatewayError::Rejected("unknown message".to_string()))?;
        for keyword in &command.remove {
            message.keywords.retain(|candidate| candidate != keyword);
        }
        for keyword in &command.add {
            if !message
                .keywords
                .iter()
                .any(|candidate| candidate == keyword)
            {
                message.keywords.push(keyword.clone());
            }
        }
        state.mutations.push(RecordedMutation::SetKeywords {
            account_id: account_id.to_string(),
            message_id: message_id.to_string(),
            add: command.add.clone(),
            remove: command.remove.clone(),
        });
        Ok(mutation_outcome(&mut state, SyncObject::Message))
    }

    async fn replace_mailboxes(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
        _expected_state: Option<&str>,
        mailbox_ids: &[MailboxId],
    ) -> Result<MutationOutcome, GatewayError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| GatewayError::Rejected("gateway state poisoned".to_string()))?;
        let message = state
            .messages
            .get_mut(message_id.as_str())
            .ok_or_else(|| GatewayError::Rejected("unknown message".to_string()))?;
        message.mailbox_ids = mailbox_ids.to_vec();
        state.mutations.push(RecordedMutation::ReplaceMailboxes {
            account_id: account_id.to_string(),
            message_id: message_id.to_string(),
            mailbox_ids: mailbox_ids.iter().map(ToString::to_string).collect(),
        });
        Ok(mutation_outcome(&mut state, SyncObject::Message))
    }

    async fn destroy_message(
        &self,
        _account_id: &AccountId,
        _message_id: &MessageId,
        _expected_state: Option<&str>,
    ) -> Result<MutationOutcome, GatewayError> {
        Err(GatewayError::Rejected(
            "unused in automation tests".to_string(),
        ))
    }

    async fn set_mailbox_role(
        &self,
        _account_id: &AccountId,
        _mailbox_id: &MailboxId,
        _expected_state: Option<&str>,
        _role: Option<&str>,
        _clear_role_from: Option<&MailboxId>,
    ) -> Result<MutationOutcome, GatewayError> {
        Err(GatewayError::Rejected(
            "unused in automation tests".to_string(),
        ))
    }

    async fn fetch_identity(&self, _account_id: &AccountId) -> Result<Identity, GatewayError> {
        Err(GatewayError::Rejected(
            "unused in automation tests".to_string(),
        ))
    }

    async fn fetch_reply_context(
        &self,
        _account_id: &AccountId,
        _message_id: &MessageId,
    ) -> Result<ReplyContext, GatewayError> {
        Err(GatewayError::Rejected(
            "unused in automation tests".to_string(),
        ))
    }

    async fn send_message(
        &self,
        _account_id: &AccountId,
        _request: &SendMessageRequest,
    ) -> Result<(), GatewayError> {
        Err(GatewayError::Rejected(
            "unused in automation tests".to_string(),
        ))
    }

    fn push_transports(&self) -> Vec<Box<dyn PushTransport>> {
        Vec::new()
    }
}

#[tokio::test]
async fn global_rule_applies_only_to_matching_account() {
    let harness = RuleHarness::new();
    harness.save_account("primary", "Primary");
    harness.save_account("secondary", "Secondary");
    harness.save_rules(vec![rule(
        "tag-posthaste-primary",
        vec![source_is("primary"), from_contains("Posthaste")],
        vec![AutomationAction::ApplyTag {
            tag: "newsletter".to_string(),
        }],
    )]);
    let primary_gateway = ScriptedGateway::new(
        vec![mailbox("inbox", "Inbox", Some("inbox"))],
        vec![
            message(
                "primary-match",
                &["inbox"],
                "Posthaste",
                "news@posthaste.test",
                &[],
            ),
            message(
                "primary-other",
                &["inbox"],
                "Other",
                "other@example.test",
                &[],
            ),
        ],
    );
    let secondary_gateway = ScriptedGateway::new(
        vec![mailbox("inbox", "Inbox", Some("inbox"))],
        vec![message(
            "secondary-match",
            &["inbox"],
            "Posthaste",
            "news@posthaste.test",
            &[],
        )],
    );

    harness.sync("primary", &primary_gateway).await;
    harness.sync("secondary", &secondary_gateway).await;

    assert_eq!(
        harness.message_keywords("primary", "primary-match"),
        vec!["newsletter".to_string()]
    );
    assert!(harness
        .message_keywords("primary", "primary-other")
        .is_empty());
    assert!(harness
        .message_keywords("secondary", "secondary-match")
        .is_empty());
    assert_eq!(primary_gateway.mutations().len(), 1);
    assert!(secondary_gateway.mutations().is_empty());
}

#[tokio::test]
async fn mailbox_role_condition_marks_only_matching_messages_read() {
    let harness = RuleHarness::new();
    harness.save_account("primary", "Primary");
    harness.save_rules(vec![rule(
        "read-inbox-posthaste",
        vec![
            source_is("primary"),
            mailbox_role_is("inbox"),
            from_contains("Posthaste"),
        ],
        vec![AutomationAction::MarkRead],
    )]);
    let gateway = ScriptedGateway::new(
        vec![
            mailbox("inbox", "Inbox", Some("inbox")),
            mailbox("archive", "Archive", Some("archive")),
        ],
        vec![
            message(
                "inbox-match",
                &["inbox"],
                "Posthaste",
                "news@posthaste.test",
                &[],
            ),
            message(
                "archive-match",
                &["archive"],
                "Posthaste",
                "news@posthaste.test",
                &[],
            ),
        ],
    );

    harness.sync("primary", &gateway).await;

    assert!(harness.message_is_read("primary", "inbox-match"));
    assert!(!harness.message_is_read("primary", "archive-match"));
    assert_eq!(
        gateway.mutations(),
        vec![RecordedMutation::SetKeywords {
            account_id: "primary".to_string(),
            message_id: "inbox-match".to_string(),
            add: vec!["$seen".to_string()],
            remove: Vec::new(),
        }]
    );
}

#[tokio::test]
async fn automation_actions_are_idempotent_across_repeated_syncs() {
    let harness = RuleHarness::new();
    harness.save_account("primary", "Primary");
    harness.save_rules(vec![rule(
        "read-posthaste",
        vec![source_is("primary"), from_contains("Posthaste")],
        vec![AutomationAction::MarkRead],
    )]);
    let gateway = ScriptedGateway::new(
        vec![mailbox("inbox", "Inbox", Some("inbox"))],
        vec![message(
            "message-1",
            &["inbox"],
            "Posthaste",
            "news@posthaste.test",
            &[],
        )],
    );

    harness.sync("primary", &gateway).await;
    harness.sync("primary", &gateway).await;

    assert!(harness.message_is_read("primary", "message-1"));
    assert_eq!(gateway.mutations().len(), 1);
}

#[tokio::test]
async fn keyword_state_actions_apply_expected_keyword_deltas() {
    let cases = [
        (
            "apply-tag",
            AutomationAction::ApplyTag {
                tag: "newsletter".to_string(),
            },
            Vec::<&str>::new(),
            vec!["newsletter".to_string()],
            vec!["newsletter".to_string()],
            Vec::new(),
        ),
        (
            "remove-tag",
            AutomationAction::RemoveTag {
                tag: "newsletter".to_string(),
            },
            vec!["newsletter"],
            Vec::new(),
            Vec::new(),
            vec!["newsletter".to_string()],
        ),
        (
            "mark-unread",
            AutomationAction::MarkUnread,
            vec!["$seen"],
            Vec::new(),
            Vec::new(),
            vec!["$seen".to_string()],
        ),
        (
            "flag",
            AutomationAction::Flag,
            Vec::<&str>::new(),
            vec!["$flagged".to_string()],
            vec!["$flagged".to_string()],
            Vec::new(),
        ),
        (
            "unflag",
            AutomationAction::Unflag,
            vec!["$flagged"],
            Vec::new(),
            Vec::new(),
            vec!["$flagged".to_string()],
        ),
    ];

    for (case, action, initial_keywords, expected_keywords, expected_add, expected_remove) in cases
    {
        let harness = RuleHarness::new();
        harness.save_account("primary", "Primary");
        harness.save_rules(vec![rule(
            case,
            vec![source_is("primary"), from_contains("Posthaste")],
            vec![action],
        )]);
        let gateway = ScriptedGateway::new(
            vec![mailbox("inbox", "Inbox", Some("inbox"))],
            vec![message(
                "message-1",
                &["inbox"],
                "Posthaste",
                "news@posthaste.test",
                &initial_keywords,
            )],
        );

        harness.sync("primary", &gateway).await;

        assert_eq!(
            harness.message_keywords("primary", "message-1"),
            expected_keywords,
            "{case} should leave the expected local keywords"
        );
        assert_eq!(
            gateway.mutations(),
            vec![RecordedMutation::SetKeywords {
                account_id: "primary".to_string(),
                message_id: "message-1".to_string(),
                add: expected_add,
                remove: expected_remove,
            }],
            "{case} should send the expected gateway keyword mutation"
        );
    }
}

#[tokio::test]
async fn move_to_mailbox_action_replaces_mailbox_membership() {
    let harness = RuleHarness::new();
    harness.save_account("primary", "Primary");
    harness.save_rules(vec![rule(
        "archive-posthaste",
        vec![source_is("primary"), from_contains("Posthaste")],
        vec![AutomationAction::MoveToMailbox {
            mailbox_id: MailboxId::from("archive"),
        }],
    )]);
    let gateway = ScriptedGateway::new(
        vec![
            mailbox("inbox", "Inbox", Some("inbox")),
            mailbox("archive", "Archive", Some("archive")),
        ],
        vec![message(
            "message-1",
            &["inbox"],
            "Posthaste",
            "news@posthaste.test",
            &[],
        )],
    );

    harness.sync("primary", &gateway).await;

    assert_eq!(
        harness.message_mailboxes("primary", "message-1"),
        vec!["archive".to_string()]
    );
    assert_eq!(
        gateway.mutations(),
        vec![RecordedMutation::ReplaceMailboxes {
            account_id: "primary".to_string(),
            message_id: "message-1".to_string(),
            mailbox_ids: vec!["archive".to_string()],
        }]
    );
}

#[tokio::test]
async fn backfill_processes_existing_matches_in_bounded_batches() {
    let harness = RuleHarness::new();
    harness.save_account("primary", "Primary");
    let gateway = ScriptedGateway::new(
        vec![mailbox("inbox", "Inbox", Some("inbox"))],
        vec![
            message(
                "message-1",
                &["inbox"],
                "Posthaste",
                "one@posthaste.test",
                &[],
            ),
            message(
                "message-2",
                &["inbox"],
                "Posthaste",
                "two@posthaste.test",
                &[],
            ),
        ],
    );
    harness.sync("primary", &gateway).await;
    harness.save_rules(vec![rule(
        "tag-existing-posthaste",
        vec![source_is("primary"), from_contains("Posthaste")],
        vec![AutomationAction::ApplyTag {
            tag: "newsletter".to_string(),
        }],
    )]);

    let has_more_after_first_batch = harness.backfill("primary", &gateway, 1).await;
    let has_more_after_second_batch = harness.backfill("primary", &gateway, 1).await;
    let has_more_after_third_batch = harness.backfill("primary", &gateway, 1).await;

    assert!(has_more_after_first_batch);
    assert!(has_more_after_second_batch);
    assert!(!has_more_after_third_batch);
    assert_eq!(
        vec![
            harness.message_keywords("primary", "message-1"),
            harness.message_keywords("primary", "message-2"),
        ],
        vec![
            vec!["newsletter".to_string()],
            vec!["newsletter".to_string()]
        ]
    );
    assert_eq!(gateway.mutations().len(), 2);
}

#[tokio::test]
async fn durable_backfill_job_completes_current_rules_and_reruns_changed_rules() {
    let harness = RuleHarness::new();
    harness.save_account("primary", "Primary");
    let gateway = ScriptedGateway::new(
        vec![mailbox("inbox", "Inbox", Some("inbox"))],
        vec![message(
            "message-1",
            &["inbox"],
            "Posthaste",
            "one@posthaste.test",
            &[],
        )],
    );
    harness.sync("primary", &gateway).await;
    harness.save_rules(vec![rule(
        "tag-existing-posthaste",
        vec![source_is("primary"), from_contains("Posthaste")],
        vec![AutomationAction::ApplyTag {
            tag: "newsletter".to_string(),
        }],
    )]);

    let first_outcome = harness.process_backfill_job("primary", &gateway, 10).await;
    let second_outcome = harness.process_backfill_job("primary", &gateway, 10).await;

    assert_eq!(first_outcome, (true, false));
    assert_eq!(second_outcome, (false, false));
    assert_eq!(
        harness.current_backfill_status("primary"),
        Some(AutomationBackfillJobStatus::Completed)
    );
    assert_eq!(
        harness.message_keywords("primary", "message-1"),
        vec!["newsletter".to_string()]
    );
    assert_eq!(gateway.mutations().len(), 1);

    harness.save_rules(vec![rule(
        "tag-existing-posthaste-again",
        vec![source_is("primary"), from_contains("Posthaste")],
        vec![AutomationAction::ApplyTag {
            tag: "followup".to_string(),
        }],
    )]);

    let changed_rules_outcome = harness.process_backfill_job("primary", &gateway, 10).await;

    assert_eq!(changed_rules_outcome, (true, false));
    assert_eq!(
        harness.current_backfill_status("primary"),
        Some(AutomationBackfillJobStatus::Completed)
    );
    let mut keywords = harness.message_keywords("primary", "message-1");
    keywords.sort();
    assert_eq!(
        keywords,
        vec!["followup".to_string(), "newsletter".to_string()]
    );
    assert_eq!(gateway.mutations().len(), 2);
}
