//! Guards the mirror module against drift from the domain model.
//!
//! Every reused domain type is serialized fully populated and strictly
//! decoded into its `deny_unknown_fields` mirror twin. A field the domain
//! adds shows up as an unknown key (and the exhaustive struct literals below
//! stop compiling); a field the mirror carries but the domain stopped
//! serializing fails the strict decode.

use posthaste_client_models::mirror;
use posthaste_domain_model as domain;
use serde::de::DeserializeOwned;
use serde_json::json;

fn assert_mirrors<M: DeserializeOwned + std::fmt::Debug>(
    name: &str,
    value: &impl serde::Serialize,
) {
    let json = serde_json::to_value(value).unwrap_or_else(|error| panic!("{name}: {error}"));
    if let Err(error) = serde_json::from_value::<M>(json.clone()) {
        panic!("mirror::{name} drifted from the domain type: {error}\nserialized: {json}");
    }
}

fn recipient() -> domain::Recipient {
    domain::Recipient {
        name: Some("Ada".into()),
        email: "ada@example.com".into(),
    }
}

fn message_summary() -> domain::MessageSummary {
    domain::MessageSummary {
        id: "m1".into(),
        source_id: "a1".into(),
        source_name: "Work".into(),
        source_thread_id: "t1".into(),
        conversation_id: "c1".into(),
        subject: Some("Subject".into()),
        from_name: Some("Ada".into()),
        from_email: Some("ada@example.com".into()),
        to: vec![recipient()],
        preview: Some("preview".into()),
        received_at: "2026-01-01T00:00:00Z".into(),
        has_attachment: true,
        is_read: false,
        is_flagged: true,
        mailbox_ids: vec!["inbox".into()],
        keywords: vec!["$flagged".into()],
        version: Some(7),
        rfc_message_id: Some("<x@example.com>".into()),
        in_reply_to: Some("<y@example.com>".into()),
        draft_id: Some("d1".into()),
    }
}

#[test]
fn structs_decode_strictly_from_domain_serialization() {
    assert_mirrors::<mirror::Recipient>("Recipient", &recipient());
    assert_mirrors::<mirror::MessageSummary>("MessageSummary", &message_summary());
    assert_mirrors::<mirror::MailboxSummary>(
        "MailboxSummary",
        &domain::MailboxSummary {
            id: "inbox".into(),
            name: "Inbox".into(),
            role: Some("inbox".into()),
            unread_emails: 3,
            total_emails: 10,
        },
    );
    assert_mirrors::<mirror::MessageAttachment>(
        "MessageAttachment",
        &domain::MessageAttachment {
            id: "att1".into(),
            blob_id: "b1".into(),
            part_id: Some("2".into()),
            filename: Some("file.pdf".into()),
            mime_type: "application/pdf".into(),
            size: 1024,
            disposition: Some("attachment".into()),
            cid: Some("cid1".into()),
            is_inline: false,
        },
    );
    assert_mirrors::<mirror::ListUnsubscribe>(
        "ListUnsubscribe",
        &domain::ListUnsubscribe {
            https: Some("https://example.com/u".into()),
            mailto: Some("mailto:u@example.com".into()),
            one_click: true,
        },
    );
    assert_mirrors::<mirror::ThreadView>(
        "ThreadView",
        &domain::ThreadView {
            id: "t1".into(),
            messages: vec![message_summary()],
        },
    );
    assert_mirrors::<mirror::SetKeywordsCommand>(
        "SetKeywordsCommand",
        &domain::SetKeywordsCommand {
            add: vec!["$seen".into()],
            remove: vec!["$flagged".into()],
        },
    );
    assert_mirrors::<mirror::ReplaceMailboxesCommand>(
        "ReplaceMailboxesCommand",
        &domain::ReplaceMailboxesCommand {
            mailbox_ids: vec!["archive".into()],
        },
    );
    assert_mirrors::<mirror::SendMessageRequest>(
        "SendMessageRequest",
        &domain::SendMessageRequest {
            from: Some(recipient()),
            to: vec![recipient()],
            cc: vec![recipient()],
            bcc: vec![recipient()],
            subject: "Subject".into(),
            body: "Body".into(),
            in_reply_to: Some("<x@example.com>".into()),
            references: Some("<x@example.com>".into()),
            attachments: vec![domain::SendMessageAttachment {
                filename: "file.txt".into(),
                mime_type: "text/plain".into(),
                content_base64: "aGk=".into(),
            }],
            draft_id: Some("d1".into()),
            send_at: Some("2026-01-01T00:00:00Z".into()),
            undo_window_seconds: Some(30),
        },
    );
    assert_mirrors::<mirror::MessageUpdatedPayload>(
        "MessageUpdatedPayload",
        &domain::MessageUpdatedPayload {
            message_id: "m1".into(),
            source_thread_id: "t1".into(),
            conversation_id: "c1".into(),
            created: true,
            changes: domain::MessageChangeFlags {
                keywords: true,
                mailboxes: true,
                arrived: true,
            },
            keywords: vec!["$seen".into()],
            mailbox_ids: vec!["inbox".into()],
            arrived_mailbox_ids: vec!["inbox".into()],
            projection: Some(message_summary()),
        },
    );
    assert_mirrors::<mirror::OperationSettlement>(
        "OperationSettlement",
        &domain::OperationSettlement {
            id: "op1".into(),
            outcome: domain::OperationOutcome::Applied,
            assigned_entity_id: Some("m2".into()),
            error: Some("boom".into()),
            send_filing: Some(domain::SendFiling::PendingFiling),
        },
    );
    assert_mirrors::<mirror::SyncCompletedPayload>(
        "SyncCompletedPayload",
        &domain::SyncCompletedPayload {
            mailbox_count: 2,
            message_count: 10,
            deleted_imap_location_count: 1,
            deleted_message_count: 1,
            automation_event_count: 3,
            trigger: domain::SyncTrigger::Poll,
            mode: domain::SyncMode::Incremental,
            resources: vec![domain::SyncResourceRef {
                kind: "sync".into(),
                operation: "completed".into(),
                account_id: "a1".into(),
                mode: Some(domain::SyncMode::Incremental),
            }],
            post_commit_errors: vec!["automation_flush_failed".into()],
        },
    );
}

/// A mail-query rule exercising groups, conditions, and every value shape.
fn mail_query_rule() -> domain::MailQueryRule {
    domain::MailQueryRule {
        root: domain::MailQueryGroup {
            operator: domain::MailQueryGroupOperator::All,
            negated: false,
            nodes: vec![
                domain::MailQueryRuleNode::Condition(domain::MailQueryCondition {
                    field: domain::MailQueryField::Subject,
                    operator: domain::MailQueryOperator::Contains,
                    negated: false,
                    value: domain::MailQueryValue::String("invoice".into()),
                }),
                domain::MailQueryRuleNode::Condition(domain::MailQueryCondition {
                    field: domain::MailQueryField::Keyword,
                    operator: domain::MailQueryOperator::In,
                    negated: true,
                    value: domain::MailQueryValue::Strings(vec!["work".into(), "urgent".into()]),
                }),
                domain::MailQueryRuleNode::Group(domain::MailQueryGroup {
                    operator: domain::MailQueryGroupOperator::Any,
                    negated: false,
                    nodes: vec![
                        domain::MailQueryRuleNode::Condition(domain::MailQueryCondition {
                            field: domain::MailQueryField::IsRead,
                            operator: domain::MailQueryOperator::Equals,
                            negated: false,
                            value: domain::MailQueryValue::Bool(false),
                        }),
                        domain::MailQueryRuleNode::Condition(domain::MailQueryCondition {
                            field: domain::MailQueryField::ReceivedAt,
                            operator: domain::MailQueryOperator::Ge,
                            negated: false,
                            value: domain::MailQueryValue::Date(domain::DateValue::Relative {
                                amount: 7,
                                unit: domain::DateUnit::Days,
                            }),
                        }),
                        domain::MailQueryRuleNode::Condition(domain::MailQueryCondition {
                            field: domain::MailQueryField::ReceivedAt,
                            operator: domain::MailQueryOperator::Lt,
                            negated: false,
                            value: domain::MailQueryValue::Date(domain::DateValue::Absolute {
                                value: "2026-01-01T00:00:00Z".into(),
                            }),
                        }),
                    ],
                }),
            ],
        },
    }
}

fn automation_rule() -> domain::AutomationRule {
    domain::AutomationRule {
        id: "rule-1".into(),
        name: "File invoices".into(),
        enabled: true,
        triggers: vec![
            domain::AutomationTrigger::MessageArrived,
            domain::AutomationTrigger::Manual,
        ],
        condition: mail_query_rule(),
        actions: vec![
            domain::AutomationAction::ApplyTag { tag: "work".into() },
            domain::AutomationAction::MoveToMailbox {
                mailbox_id: "archive".into(),
            },
        ],
        backfill: true,
    }
}

#[test]
fn query_ast_and_settings_mirrors_decode_domain_values() {
    assert_mirrors::<mirror::MailQueryRule>("MailQueryRule", &mail_query_rule());
    assert_mirrors::<mirror::AutomationRule>("AutomationRule", &automation_rule());
    assert_mirrors::<mirror::TagSummary>(
        "TagSummary",
        &domain::TagSummary {
            name: "work".into(),
            unread_messages: 2,
            total_messages: 9,
        },
    );
    assert_mirrors::<mirror::AppSettings>(
        "AppSettings",
        &domain::AppSettings {
            default_account_id: Some("a1".into()),
            cache_policy: domain::CachePolicy::default(),
            automation_rules: vec![automation_rule()],
            automation_drafts: vec![automation_rule()],
            appearance: Some(domain::Appearance {
                mode: Some(domain::ThemeMode::Dark),
                theme: Some("glass".into()),
                density: Some(domain::UiDensity::Cozy),
                light: Some(domain::ThemeColors {
                    accent_hue: Some(200),
                    surface_hue: Some(220),
                    tokens: [("--radius".to_string(), "4px".to_string())]
                        .into_iter()
                        .collect(),
                }),
                dark: Some(domain::ThemeColors::default()),
                glass_theme: Some(domain::GlassTheme {
                    blooms: vec![domain::GlassBloom {
                        id: "b1".into(),
                        hue: 12,
                        x: 0.2,
                        y: 0.4,
                        opacity: 0.5,
                        radius: 0.8,
                    }],
                }),
            }),
            notifications: Some(domain::Notifications {
                new_mail: Some(true),
                sound: Some(false),
            }),
            mailbox_colors: vec![domain::MailboxColor {
                source_id: "a1".into(),
                mailbox_id: "inbox".into(),
                hue: 128,
            }],
            tags: vec![domain::TagAppearance {
                name: "work".into(),
                fg: Some("#1f2937".into()),
                bg: Some("#dbeafe".into()),
                icon: Some("briefcase".into()),
            }],
            smart_mailbox_order: vec!["sm-1".into()],
            account_order: vec!["a1".into()],
            mailbox_groups: vec![domain::MailboxGroup {
                id: "g1".into(),
                name: "Projects".into(),
                mailbox_ids: vec!["mb1".into()],
                order: 1,
            }],
            compose: Some(domain::ComposeSettings {
                undo_send_delay_seconds: Some(10),
            }),
        },
    );
}

#[test]
fn account_config_mirrors_decode_domain_values() {
    assert_mirrors::<mirror::ImapTransportSettings>(
        "ImapTransportSettings",
        &domain::ImapTransportSettings {
            host: "imap.example.com".into(),
            port: 993,
            security: domain::TransportSecurity::Tls,
        },
    );
    assert_mirrors::<mirror::SmtpTransportSettings>(
        "SmtpTransportSettings",
        &domain::SmtpTransportSettings {
            host: "smtp.example.com".into(),
            port: 465,
            security: domain::TransportSecurity::StartTls,
        },
    );
    assert_mirrors::<mirror::SecretStatus>(
        "SecretStatus",
        &domain::SecretStatus {
            storage: domain::SecretKind::Os,
            configured: true,
            label: Some("IMAP password".into()),
        },
    );
    assert_mirrors::<mirror::AccountAppearance>(
        "AccountAppearance",
        &domain::AccountAppearance::Initials {
            initials: "AB".into(),
            color_hue: 210,
        },
    );
    assert_mirrors::<mirror::AccountAppearance>(
        "AccountAppearance",
        &domain::AccountAppearance::Image {
            image_id: "img-1".into(),
            initials: "AB".into(),
            color_hue: 210,
        },
    );
}

#[test]
fn rev_log_mirrors_decode_domain_values() {
    assert_mirrors::<mirror::RevLogStep>(
        "RevLogStep",
        &domain::RevLogStep {
            step_id: "01J".into(),
            seq: 3,
            message_id: "m1".into(),
            source_id: "a1".into(),
            diff: json!({ "keywords": { "added": ["$seen"], "removed": [] } }),
            created_at: "2026-01-01T00:00:00Z".into(),
        },
    );
    assert_mirrors::<mirror::RevCursor>(
        "RevCursor",
        &domain::RevCursor {
            cursor_step_id: Some("01J".into()),
            redo_tail: vec!["01K".into()],
        },
    );
}

#[test]
fn ids_decode_as_plain_strings() {
    assert_mirrors::<mirror::AccountId>("AccountId", &domain::AccountId::from("a1"));
    assert_mirrors::<mirror::MailboxId>("MailboxId", &domain::MailboxId::from("mb1"));
    assert_mirrors::<mirror::MessageId>("MessageId", &domain::MessageId::from("m1"));
    assert_mirrors::<mirror::ThreadId>("ThreadId", &domain::ThreadId::from("t1"));
    assert_mirrors::<mirror::ConversationId>("ConversationId", &domain::ConversationId::from("c1"));
    assert_mirrors::<mirror::BlobId>("BlobId", &domain::BlobId::from("b1"));
    assert_mirrors::<mirror::OperationId>("OperationId", &domain::OperationId::from("op1"));
    assert_mirrors::<mirror::SmartMailboxId>(
        "SmartMailboxId",
        &domain::SmartMailboxId::from("sm1"),
    );
}

#[test]
fn enums_decode_every_domain_variant() {
    // Exhaustive matches: a new domain variant fails compilation here, which
    // is the cue to extend both the mirror and these lists.
    fn covered_sort(value: domain::MessageSortField) {
        use domain::MessageSortField as F;
        match value {
            F::Date | F::From | F::Subject | F::Source | F::Flagged | F::Attachment => {}
        }
    }
    fn covered_kind(value: domain::OperationKind) {
        use domain::OperationKind as K;
        match value {
            K::SetKeywords
            | K::ReplaceMailboxes
            | K::Destroy
            | K::DraftCreate
            | K::DraftUpdate
            | K::DraftDelete
            | K::Send => {}
        }
    }
    fn covered_state(value: domain::OperationState) {
        use domain::OperationState as S;
        match value {
            S::Pending | S::Inflight | S::Applied | S::Failed | S::DispatchUncertain => {}
        }
    }
    fn covered_entity(value: domain::OperationEntityKind) {
        use domain::OperationEntityKind as E;
        match value {
            E::Message | E::Draft => {}
        }
    }
    fn covered_status(value: domain::AccountStatus) {
        use domain::AccountStatus as A;
        match value {
            A::Ready | A::Syncing | A::Degraded | A::AuthError | A::Offline | A::Disabled => {}
        }
    }
    fn covered_push(value: domain::PushStatus) {
        use domain::PushStatus as P;
        match value {
            P::Connected | P::Reconnecting | P::Unsupported | P::Disabled => {}
        }
    }
    covered_sort(domain::MessageSortField::Date);
    covered_kind(domain::OperationKind::Send);
    covered_state(domain::OperationState::Pending);
    covered_entity(domain::OperationEntityKind::Message);
    covered_status(domain::AccountStatus::Ready);
    covered_push(domain::PushStatus::Connected);

    for field in [
        domain::MessageSortField::Date,
        domain::MessageSortField::From,
        domain::MessageSortField::Subject,
        domain::MessageSortField::Source,
        domain::MessageSortField::Flagged,
        domain::MessageSortField::Attachment,
    ] {
        assert_mirrors::<mirror::MessageSortField>("MessageSortField", &field);
    }
    for kind in [
        domain::OperationKind::SetKeywords,
        domain::OperationKind::ReplaceMailboxes,
        domain::OperationKind::Destroy,
        domain::OperationKind::DraftCreate,
        domain::OperationKind::DraftUpdate,
        domain::OperationKind::DraftDelete,
        domain::OperationKind::Send,
    ] {
        assert_mirrors::<mirror::OperationKind>("OperationKind", &kind);
    }
    for state in [
        domain::OperationState::Pending,
        domain::OperationState::Inflight,
        domain::OperationState::Applied,
        domain::OperationState::Failed,
        domain::OperationState::DispatchUncertain,
    ] {
        assert_mirrors::<mirror::OperationState>("OperationState", &state);
    }
    for entity in [
        domain::OperationEntityKind::Message,
        domain::OperationEntityKind::Draft,
    ] {
        assert_mirrors::<mirror::OperationEntityKind>("OperationEntityKind", &entity);
    }
    for status in [
        domain::AccountStatus::Ready,
        domain::AccountStatus::Syncing,
        domain::AccountStatus::Degraded,
        domain::AccountStatus::AuthError,
        domain::AccountStatus::Offline,
        domain::AccountStatus::Disabled,
    ] {
        assert_mirrors::<mirror::AccountStatus>("AccountStatus", &status);
    }
    for push in [
        domain::PushStatus::Connected,
        domain::PushStatus::Reconnecting,
        domain::PushStatus::Unsupported,
        domain::PushStatus::Disabled,
    ] {
        assert_mirrors::<mirror::PushStatus>("PushStatus", &push);
    }
}

#[test]
fn vocabulary_enums_decode_every_domain_variant() {
    // Exhaustive matches: a new domain variant fails compilation here, which
    // is the cue to extend both the mirror and these lists.
    fn covered_field(value: domain::MailQueryField) {
        use domain::MailQueryField as F;
        match value {
            F::SourceId
            | F::SourceName
            | F::MessageId
            | F::ThreadId
            | F::ConversationId
            | F::MailboxId
            | F::MailboxName
            | F::MailboxRole
            | F::IsRead
            | F::IsFlagged
            | F::HasAttachment
            | F::Keyword
            | F::FromName
            | F::FromEmail
            | F::To
            | F::Subject
            | F::Preview
            | F::Body
            | F::ReceivedAt
            | F::Size => {}
        }
    }
    fn covered_operator(value: domain::MailQueryOperator) {
        use domain::MailQueryOperator as O;
        match value {
            O::Equals
            | O::In
            | O::Contains
            | O::BeginsWith
            | O::EndsWith
            | O::Regex
            | O::Lt
            | O::Gt
            | O::Le
            | O::Ge => {}
        }
    }
    fn covered_sync(value: domain::SyncMode) {
        use domain::SyncMode as S;
        match value {
            S::Incremental | S::FullMetadata => {}
        }
    }
    covered_field(domain::MailQueryField::Subject);
    covered_operator(domain::MailQueryOperator::Equals);
    covered_sync(domain::SyncMode::Incremental);

    for field in [
        domain::MailQueryField::SourceId,
        domain::MailQueryField::SourceName,
        domain::MailQueryField::MessageId,
        domain::MailQueryField::ThreadId,
        domain::MailQueryField::ConversationId,
        domain::MailQueryField::MailboxId,
        domain::MailQueryField::MailboxName,
        domain::MailQueryField::MailboxRole,
        domain::MailQueryField::IsRead,
        domain::MailQueryField::IsFlagged,
        domain::MailQueryField::HasAttachment,
        domain::MailQueryField::Keyword,
        domain::MailQueryField::FromName,
        domain::MailQueryField::FromEmail,
        domain::MailQueryField::To,
        domain::MailQueryField::Subject,
        domain::MailQueryField::Preview,
        domain::MailQueryField::Body,
        domain::MailQueryField::ReceivedAt,
        domain::MailQueryField::Size,
    ] {
        assert_mirrors::<mirror::MailQueryField>("MailQueryField", &field);
    }
    for operator in [
        domain::MailQueryOperator::Equals,
        domain::MailQueryOperator::In,
        domain::MailQueryOperator::Contains,
        domain::MailQueryOperator::BeginsWith,
        domain::MailQueryOperator::EndsWith,
        domain::MailQueryOperator::Regex,
        domain::MailQueryOperator::Lt,
        domain::MailQueryOperator::Gt,
        domain::MailQueryOperator::Le,
        domain::MailQueryOperator::Ge,
    ] {
        assert_mirrors::<mirror::MailQueryOperator>("MailQueryOperator", &operator);
    }
    for group_operator in [
        domain::MailQueryGroupOperator::All,
        domain::MailQueryGroupOperator::Any,
    ] {
        assert_mirrors::<mirror::MailQueryGroupOperator>("MailQueryGroupOperator", &group_operator);
    }
    for unit in [
        domain::DateUnit::Minutes,
        domain::DateUnit::Hours,
        domain::DateUnit::Days,
        domain::DateUnit::Weeks,
        domain::DateUnit::Months,
    ] {
        assert_mirrors::<mirror::DateUnit>("DateUnit", &unit);
    }
    for kind in [
        domain::SmartMailboxKind::Default,
        domain::SmartMailboxKind::User,
    ] {
        assert_mirrors::<mirror::SmartMailboxKind>("SmartMailboxKind", &kind);
    }
    for trigger in [
        domain::AutomationTrigger::MessageArrived,
        domain::AutomationTrigger::MessageChanged,
        domain::AutomationTrigger::Manual,
    ] {
        assert_mirrors::<mirror::AutomationTrigger>("AutomationTrigger", &trigger);
    }
    for driver in [
        domain::AccountDriver::Jmap,
        domain::AccountDriver::ImapSmtp,
        domain::AccountDriver::Mock,
    ] {
        assert_mirrors::<mirror::AccountDriver>("AccountDriver", &driver);
    }
    for provider in [
        domain::ProviderHint::Generic,
        domain::ProviderHint::Gmail,
        domain::ProviderHint::Outlook,
        domain::ProviderHint::Icloud,
    ] {
        assert_mirrors::<mirror::ProviderHint>("ProviderHint", &provider);
    }
    for auth in [
        domain::ProviderAuthKind::Password,
        domain::ProviderAuthKind::AppPassword,
        domain::ProviderAuthKind::OAuth2,
    ] {
        assert_mirrors::<mirror::ProviderAuthKind>("ProviderAuthKind", &auth);
    }
    for security in [
        domain::TransportSecurity::Tls,
        domain::TransportSecurity::StartTls,
        domain::TransportSecurity::Plain,
    ] {
        assert_mirrors::<mirror::TransportSecurity>("TransportSecurity", &security);
    }
    for secret_kind in [domain::SecretKind::Env, domain::SecretKind::Os] {
        assert_mirrors::<mirror::SecretKind>("SecretKind", &secret_kind);
    }
    for mode in [
        domain::ThemeMode::Light,
        domain::ThemeMode::Dark,
        domain::ThemeMode::System,
    ] {
        assert_mirrors::<mirror::ThemeMode>("ThemeMode", &mode);
    }
    for density in [
        domain::UiDensity::Compact,
        domain::UiDensity::Cozy,
        domain::UiDensity::Comfortable,
    ] {
        assert_mirrors::<mirror::UiDensity>("UiDensity", &density);
    }
    for sync_mode in [
        domain::SyncMode::Incremental,
        domain::SyncMode::FullMetadata,
    ] {
        assert_mirrors::<mirror::SyncMode>("SyncMode", &sync_mode);
    }
}

#[test]
fn wire_envelopes_serialize_the_documented_shapes() {
    use posthaste_client_models as models;

    let query = models::Query::MailList(models::MailListQuery {
        mailbox_id: Some("inbox".into()),
        ..Default::default()
    });
    assert_eq!(
        serde_json::to_value(&query).unwrap(),
        json!({
            "mailList": {
                "accountId": null,
                "mailboxId": "inbox",
                "smartMailboxId": null,
                "freeText": null,
                "isRead": null,
                "isFlagged": null,
                "hasAttachment": null,
                "sort": null,
                "limit": null,
                "cursor": null,
            }
        })
    );

    // Defaulted fields may be omitted entirely by a TS caller.
    let decoded: models::Query = serde_json::from_value(json!({ "mailList": {} })).unwrap();
    match decoded {
        models::Query::MailList(list) => assert!(list.mailbox_id.is_none()),
        other => panic!("wrong family: {other:?}"),
    }

    let command = models::CommandEnvelope {
        id: "01J".into(),
        command: models::Command::Destroy(models::DestroyMessageIntent {
            account_id: "a1".into(),
            message_id: "m1".into(),
        }),
    };
    assert_eq!(
        serde_json::to_value(&command).unwrap(),
        json!({
            "id": "01J",
            "command": { "destroy": { "accountId": "a1", "messageId": "m1" } }
        })
    );

    let heartbeat = models::EventMessage {
        generation: 4184,
        run_id: None,
        event: None,
    };
    assert_eq!(
        serde_json::to_value(&heartbeat).unwrap(),
        json!({ "generation": 4184 })
    );

    let handshake = models::EventMessage {
        generation: 4184,
        run_id: Some("run-1".into()),
        event: None,
    };
    assert_eq!(
        serde_json::to_value(&handshake).unwrap(),
        json!({ "generation": 4184, "runId": "run-1" })
    );
}

#[test]
fn secret_material_has_exactly_one_wire_shape() {
    use posthaste_client_models as models;

    // The dedicated secret-bearing command: material appears only under an
    // explicit `replace`.
    let command = models::Command::SetAccountSecret(models::SetAccountSecretIntent {
        account_id: "a1".into(),
        change: models::AccountSecretChange::Replace {
            secret: "hunter2".into(),
        },
    });
    assert_eq!(
        serde_json::to_value(&command).unwrap(),
        json!({
            "setAccountSecret": {
                "accountId": "a1",
                "change": { "kind": "replace", "secret": "hunter2" }
            }
        })
    );

    // Keep/clear carry no material at all.
    let command = models::Command::SetAccountSecret(models::SetAccountSecretIntent {
        account_id: "a1".into(),
        change: models::AccountSecretChange::Clear,
    });
    assert_eq!(
        serde_json::to_value(&command).unwrap(),
        json!({
            "setAccountSecret": { "accountId": "a1", "change": { "kind": "clear" } }
        })
    );
}

#[test]
fn field_patches_serialize_keep_set_and_clear() {
    use posthaste_client_models as models;
    use posthaste_client_models::FieldPatch;

    // The three patch states have the documented tagged shapes.
    let intent = models::UpdateAccountIntent {
        account_id: "a1".into(),
        name: None,
        full_name: FieldPatch::Set {
            value: "Ada Lovelace".into(),
        },
        signature: FieldPatch::Clear,
        email_patterns: None,
        enabled: None,
        appearance: None,
    };
    let json = serde_json::to_value(&intent).unwrap();
    assert_eq!(
        json["fullName"],
        json!({ "kind": "set", "value": "Ada Lovelace" })
    );
    assert_eq!(json["signature"], json!({ "kind": "clear" }));

    // An absent field decodes as keep — a TS caller may omit it entirely.
    let decoded: models::UpdateAccountIntent =
        serde_json::from_value(json!({ "accountId": "a1" })).unwrap();
    assert_eq!(decoded.full_name, FieldPatch::Keep);
    assert_eq!(decoded.signature, FieldPatch::Keep);

    let decoded: models::UpdateAccountTransportIntent = serde_json::from_value(json!({
        "accountId": "a1",
        "baseUrl": { "kind": "clear" },
        "username": { "kind": "set", "value": "probe@example.com" },
    }))
    .unwrap();
    assert_eq!(decoded.base_url, FieldPatch::Clear);
    assert_eq!(
        decoded.username,
        FieldPatch::Set {
            value: "probe@example.com".into()
        }
    );
    assert_eq!(decoded.provider, None);

    // A bare null is not a patch: clearing must be said explicitly.
    assert!(serde_json::from_value::<models::UpdateAccountIntent>(
        json!({ "accountId": "a1", "fullName": null })
    )
    .is_err());

    let decoded: models::UpdateSmartMailboxIntent = serde_json::from_value(json!({
        "smartMailboxId": "sm1",
        "role": { "kind": "clear" },
    }))
    .unwrap();
    assert_eq!(decoded.role, FieldPatch::Clear);
    assert_eq!(decoded.name, None);
}
