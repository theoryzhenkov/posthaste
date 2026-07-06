use axum::extract::{Path, State};
use axum::Json;
use posthaste_domain_model::{
    AccountAppearance, EVENT_TOPIC_ACCOUNT_UPDATED, EVENT_TOPIC_CONFIG_RELOADED,
    EVENT_TOPIC_SETTINGS_UPDATED, EVENT_TOPIC_SMART_MAILBOX_CREATED,
    EVENT_TOPIC_SMART_MAILBOX_DELETED, EVENT_TOPIC_SMART_MAILBOX_RESET,
    EVENT_TOPIC_SMART_MAILBOX_UPDATED,
};
use posthaste_http_api_adapter::api::{
    create_smart_mailbox, delete_smart_mailbox, patch_account, patch_settings, patch_smart_mailbox,
    reload_config, reset_default_smart_mailboxes, CreateSmartMailboxRequest, PatchAccountRequest,
    PatchSettingsRequest, PatchSmartMailboxRequest,
};

use crate::support::{
    expect_api_ok, expect_settings_ok, receive_event, smart_rule_for_source, SettingsHarness,
};

#[tokio::test]
async fn patch_settings_publishes_settings_updated_resource_event() {
    let harness = SettingsHarness::new();
    harness.save_account("primary", "Primary");
    let receiver = harness.subscribe_events();

    let _ = expect_settings_ok(
        patch_settings(
            State(harness.state.clone()),
            Json(PatchSettingsRequest {
                force_backfill: false,
                mailbox_groups: None,
                smart_mailbox_order: None,
                account_order: None,
                default_account_id: Some(Some("primary".to_string())),
                automation_rules: None,
                automation_drafts: None,
                cache_policy: None,
                appearance: None,
                notifications: None,
                mailbox_colors: None,
                tags: None,
            }),
        )
        .await,
    );

    let event = receive_event(receiver).await;
    assert_eq!(event.topic, EVENT_TOPIC_SETTINGS_UPDATED);
    assert_eq!(event.payload["scope"], "app");
    assert_eq!(
        event.payload["changed"],
        serde_json::json!(["defaultAccount"])
    );
    assert_eq!(event.payload["resources"][0]["kind"], "appSettings");
}

#[tokio::test]
async fn patch_account_appearance_publishes_account_updated_resource_event() {
    let harness = SettingsHarness::new();
    harness.save_account("primary", "Primary");
    let receiver = harness.subscribe_events();

    let _ = expect_api_ok(
        patch_account(
            State(harness.state.clone()),
            Path("primary".to_string()),
            Json(PatchAccountRequest {
                name: None,
                full_name: None,
                signature: None,
                email_patterns: None,
                driver: None,
                enabled: None,
                appearance: Some(AccountAppearance::Initials {
                    initials: "Z".to_string(),
                    color_hue: 240,
                }),
                secret: None,
                transport: None,
            }),
        )
        .await,
        "account patch should succeed",
    );

    let event = receive_event(receiver).await;
    assert_eq!(event.topic, EVENT_TOPIC_ACCOUNT_UPDATED);
    assert_eq!(event.account_id.as_str(), "primary");
    assert_eq!(event.payload["resources"][0]["kind"], "account");
    assert_eq!(event.payload["resources"][0]["operation"], "updated");
    assert_eq!(event.payload["resources"][0]["id"], "primary");
}

#[tokio::test]
async fn smart_mailbox_mutations_publish_resource_events() {
    let harness = SettingsHarness::new();
    harness.save_account("primary", "Primary");

    let receiver = harness.subscribe_events();
    let created = expect_api_ok(
        create_smart_mailbox(
            State(harness.state.clone()),
            Json(CreateSmartMailboxRequest {
                name: "Work".to_string(),
                role: None,
                rule: smart_rule_for_source("primary"),
            }),
        )
        .await,
        "smart mailbox create should succeed",
    )
    .0;
    let event = receive_event(receiver).await;
    assert_eq!(event.topic, EVENT_TOPIC_SMART_MAILBOX_CREATED);
    assert_eq!(event.payload["smartMailboxId"], created.id.as_str());
    assert_eq!(event.payload["resources"][0]["operation"], "created");

    let receiver = harness.subscribe_events();
    let _ = expect_api_ok(
        patch_smart_mailbox(
            State(harness.state.clone()),
            Path(created.id.as_str().to_string()),
            Json(PatchSmartMailboxRequest {
                name: Some("Work updated".to_string()),
                role: None,
                rule: None,
            }),
        )
        .await,
        "smart mailbox patch should succeed",
    );
    let event = receive_event(receiver).await;
    assert_eq!(event.topic, EVENT_TOPIC_SMART_MAILBOX_UPDATED);
    assert_eq!(event.payload["resources"][0]["operation"], "updated");

    let receiver = harness.subscribe_events();
    let _ = expect_api_ok(
        delete_smart_mailbox(
            State(harness.state.clone()),
            Path(created.id.as_str().to_string()),
        )
        .await,
        "smart mailbox delete should succeed",
    );
    let event = receive_event(receiver).await;
    assert_eq!(event.topic, EVENT_TOPIC_SMART_MAILBOX_DELETED);
    assert_eq!(event.payload["resources"][0]["operation"], "deleted");
}

#[tokio::test]
async fn reload_config_publishes_declarative_resource_event() {
    let harness = SettingsHarness::new();
    std::fs::write(
        harness.config_root.join("sources/reloaded.toml"),
        r#"
id = "reloaded"
name = "Reloaded"
driver = "mock"
enabled = true
"#,
    )
    .expect("source config should write");
    let receiver = harness.subscribe_events();

    let _ = expect_api_ok(
        reload_config(State(harness.state.clone())).await,
        "config reload should succeed",
    );

    let event = receive_event(receiver).await;
    assert_eq!(event.topic, EVENT_TOPIC_CONFIG_RELOADED);
    assert_eq!(event.account_id.as_str(), "app");
    assert_eq!(event.payload["addedSourceCount"], 1);
    assert_eq!(event.payload["resources"][0]["kind"], "config");
    assert_eq!(event.payload["resources"][0]["operation"], "reloaded");
    assert_eq!(event.payload["resources"][1]["kind"], "account");
    assert_eq!(event.payload["resources"][1]["operation"], "created");
    assert_eq!(event.payload["resources"][1]["id"], "reloaded");
}

#[tokio::test]
async fn reset_default_smart_mailboxes_publishes_resource_event() {
    let harness = SettingsHarness::new();
    let receiver = harness.subscribe_events();

    let _ = expect_api_ok(
        reset_default_smart_mailboxes(State(harness.state.clone())).await,
        "reset defaults should succeed",
    );

    let event = receive_event(receiver).await;
    assert_eq!(event.topic, EVENT_TOPIC_SMART_MAILBOX_RESET);
    assert_eq!(event.payload["resources"][0]["kind"], "smartMailbox");
    assert_eq!(event.payload["resources"][0]["operation"], "reset");
}

#[tokio::test]
async fn smart_mailbox_role_is_assignable_and_validated() {
    let harness = SettingsHarness::new();
    harness.save_account("primary", "Primary");

    // Create a user smart mailbox tagged with a built-in view role.
    let created = expect_api_ok(
        create_smart_mailbox(
            State(harness.state.clone()),
            Json(CreateSmartMailboxRequest {
                name: "Work Archive".to_string(),
                role: Some("archive".to_string()),
                rule: smart_rule_for_source("primary"),
            }),
        )
        .await,
        "create with a role should succeed",
    )
    .0;
    assert_eq!(created.role.as_deref(), Some("archive"));

    // Patch clears the role (empty-string sentinel).
    let cleared = expect_api_ok(
        patch_smart_mailbox(
            State(harness.state.clone()),
            Path(created.id.as_str().to_string()),
            Json(PatchSmartMailboxRequest {
                name: None,
                role: Some(String::new()),
                rule: None,
            }),
        )
        .await,
        "patch clearing the role should succeed",
    )
    .0;
    assert_eq!(cleared.role, None);

    // An unknown role string is rejected.
    let rejected = create_smart_mailbox(
        State(harness.state.clone()),
        Json(CreateSmartMailboxRequest {
            name: "Bogus".to_string(),
            role: Some("nonsense".to_string()),
            rule: smart_rule_for_source("primary"),
        }),
    )
    .await;
    assert!(rejected.is_err(), "an unknown role must be rejected");
}
