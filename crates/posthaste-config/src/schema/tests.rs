use super::*;
use crate::defaults::default_smart_mailboxes;

#[test]
fn source_toml_round_trips() {
    let settings = AccountSettings {
        id: AccountId::from("primary"),
        name: "My Fastmail".to_string(),
        full_name: Some("Example User".to_string()),
        signature: None,
        email_patterns: vec!["user@example.com".to_string(), "*@example.net".to_string()],
        driver: AccountDriver::Jmap,
        enabled: true,
        appearance: Some(AccountAppearance::Initials {
            initials: "MF".to_string(),
            color_hue: 245,
        }),
        transport: AccountTransportSettings {
            base_url: Some("https://api.fastmail.com".to_string()),
            username: Some("user@example.com".to_string()),
            secret_ref: Some(SecretRef {
                kind: SecretKind::Os,
                key: "account:primary".to_string(),
            }),
            ..Default::default()
        },
        created_at: "2026-03-31T00:00:00Z".to_string(),
        updated_at: "2026-03-31T00:00:00Z".to_string(),
    };

    let toml_struct = SourceToml::from_account_settings(&settings);
    let toml_string = toml::to_string_pretty(&toml_struct).unwrap();
    let parsed: SourceToml = toml::from_str(&toml_string).unwrap();
    let round_tripped = parsed.to_account_settings().unwrap();

    assert_eq!(round_tripped, settings);
}

#[test]
fn imap_smtp_source_toml_round_trips_provider_transport() {
    let settings = AccountSettings {
        id: AccountId::from("icloud"),
        name: "iCloud".to_string(),
        full_name: None,
        signature: None,
        email_patterns: vec!["user@icloud.com".to_string()],
        driver: AccountDriver::ImapSmtp,
        enabled: true,
        appearance: None,
        transport: AccountTransportSettings {
            provider: ProviderHint::Icloud,
            auth: ProviderAuthKind::AppPassword,
            username: Some("user@icloud.com".to_string()),
            secret_ref: Some(SecretRef {
                kind: SecretKind::Os,
                key: "account:icloud".to_string(),
            }),
            imap: Some(ImapTransportSettings {
                host: "imap.mail.me.com".to_string(),
                port: 993,
                security: TransportSecurity::Tls,
            }),
            smtp: Some(SmtpTransportSettings {
                host: "smtp.mail.me.com".to_string(),
                port: 587,
                security: TransportSecurity::StartTls,
            }),
            ..Default::default()
        },
        created_at: "2026-04-25T00:00:00Z".to_string(),
        updated_at: "2026-04-25T00:00:00Z".to_string(),
    };

    let toml_struct = SourceToml::from_account_settings(&settings);
    let toml_string = toml::to_string_pretty(&toml_struct).unwrap();
    assert!(toml_string.contains("driver = \"imap_smtp\""));
    assert!(toml_string.contains("provider = \"icloud\""));

    let parsed: SourceToml = toml::from_str(&toml_string).unwrap();
    let round_tripped = parsed.to_account_settings().unwrap();

    assert_eq!(round_tripped, settings);
}

#[test]
fn imap_smtp_source_toml_requires_runtime_transport_fields() {
    let parsed: SourceToml = toml::from_str(
        r#"
id = "icloud"
name = "iCloud"
email_patterns = ["*@icloud.com"]
driver = "imap_smtp"

[transport]
provider = "icloud"
auth = "app_password"
"#,
    )
    .unwrap();

    let error = parsed
        .to_account_settings()
        .expect_err("missing transport fields should be rejected");

    assert!(error.contains("transport.username"));
}

#[test]
fn default_smart_mailboxes_round_trip_through_toml() {
    for mailbox in default_smart_mailboxes() {
        let toml_struct = SmartMailboxToml::from_smart_mailbox(&mailbox);
        let toml_string = toml::to_string_pretty(&toml_struct).unwrap();
        let parsed: SmartMailboxToml = toml::from_str(&toml_string).unwrap();
        let round_tripped = parsed.to_smart_mailbox().unwrap();

        assert_eq!(round_tripped.id, mailbox.id);
        assert_eq!(round_tripped.name, mailbox.name);
        assert_eq!(round_tripped.kind, mailbox.kind);
        assert_eq!(round_tripped.default_key, mailbox.default_key);
        assert_eq!(round_tripped.role, mailbox.role);
        assert_eq!(round_tripped.rule, mailbox.rule);
    }
}

#[test]
fn default_smart_mailboxes_stamp_role_for_contextual_actions() {
    // Built-in role smart mailboxes carry their role (drives contextual
    // actions like Delete permanently in Trash); All Mail has none.
    let by_key: std::collections::HashMap<String, Option<String>> = default_smart_mailboxes()
        .into_iter()
        .filter_map(|mailbox| mailbox.default_key.map(|key| (key, mailbox.role)))
        .collect();
    assert_eq!(by_key.get("inbox"), Some(&Some("inbox".to_string())));
    assert_eq!(by_key.get("trash"), Some(&Some("trash".to_string())));
    assert_eq!(by_key.get("all-mail"), Some(&None));
}

#[test]
fn app_toml_round_trips() {
    let settings = AppSettings {
        default_account_id: Some(AccountId::from("primary")),
        automation_rules: vec![AutomationRule {
            id: "rule-newsletters".to_string(),
            name: "Newsletters".to_string(),
            enabled: true,
            triggers: vec![AutomationTrigger::MessageArrived],
            condition: SmartMailboxRule {
                root: SmartMailboxGroup {
                    operator: SmartMailboxGroupOperator::Any,
                    negated: false,
                    nodes: vec![
                        SmartMailboxRuleNode::Condition(SmartMailboxCondition {
                            field: SmartMailboxField::FromName,
                            operator: SmartMailboxOperator::Contains,
                            negated: false,
                            value: SmartMailboxValue::String("Posthaste".to_string()),
                        }),
                        SmartMailboxRuleNode::Condition(SmartMailboxCondition {
                            field: SmartMailboxField::FromEmail,
                            operator: SmartMailboxOperator::Contains,
                            negated: false,
                            value: SmartMailboxValue::String("Posthaste".to_string()),
                        }),
                    ],
                },
            },
            actions: vec![AutomationAction::ApplyTag {
                tag: "newsletter".to_string(),
            }],
            backfill: true,
        }],
        automation_drafts: vec![AutomationRule {
            id: "draft-newsletters".to_string(),
            name: "Draft newsletters".to_string(),
            enabled: true,
            triggers: vec![AutomationTrigger::MessageArrived],
            condition: SmartMailboxRule {
                root: SmartMailboxGroup {
                    operator: SmartMailboxGroupOperator::Any,
                    negated: false,
                    nodes: Vec::new(),
                },
            },
            actions: vec![AutomationAction::ApplyTag { tag: String::new() }],
            backfill: true,
        }],
        ..Default::default()
    };
    let existing = AppToml {
        schema_version: 1,
        default_source_id: None,
        automations: Vec::new(),
        draft_automations: Vec::new(),
        daemon: DaemonToml::default(),
        logging: LoggingToml::default(),
        cache: CachePolicyToml::default(),
        appearance: Default::default(),
        notifications: Default::default(),
        mailbox_colors: Vec::new(),
        tags: Vec::new(),
        smart_mailbox_order: Vec::new(),
        account_order: Vec::new(),
        link: LinkToml::default(),
        tls: None,
    };
    let toml_struct = AppToml::from_app_settings(&settings, &existing);
    let toml_string = toml::to_string_pretty(&toml_struct).unwrap();
    let parsed: AppToml = toml::from_str(&toml_string).unwrap();
    let round_tripped = parsed.to_app_settings().unwrap();

    assert_eq!(round_tripped, settings);
}
