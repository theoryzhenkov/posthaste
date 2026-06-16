use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use posthaste_domain::*;
use posthaste_server::api::{patch_settings, PatchSettingsRequest};

use crate::support::{expect_settings_ok, source_rule, SettingsHarness};

#[tokio::test]
async fn patch_settings_automation_rules_preserves_default_account_and_writes_app_toml() {
    let harness = SettingsHarness::new();
    harness.save_account("primary", "Primary");
    harness
        .service
        .put_app_settings(&AppSettings {
            default_account_id: Some(AccountId::from("primary")),
            automation_rules: Vec::new(),
            automation_drafts: Vec::new(),
            ..Default::default()
        })
        .expect("settings should save");

    let Json(settings) = expect_settings_ok(
        patch_settings(
            State(harness.state.clone()),
            Json(PatchSettingsRequest {
                default_account_id: None,
                cache_policy: None,
                automation_rules: Some(vec![source_rule("primary")]),
                automation_drafts: None,
            }),
        )
        .await,
    );

    assert_eq!(
        settings.default_account_id,
        Some(AccountId::from("primary"))
    );
    assert_eq!(settings.automation_rules.len(), 1);
    let backfill_job = harness
        .service
        .automation_backfill_job_for_current_rules(&AccountId::from("primary"))
        .expect("backfill job should load")
        .expect("backfill job should be queued");
    assert_eq!(backfill_job.status, AutomationBackfillJobStatus::Pending);
    let app_toml = harness.app_toml();
    assert_eq!(app_toml["default_source_id"].as_str(), Some("primary"));
    assert_eq!(
        app_toml["automations"][0]["id"].as_str(),
        Some("rule-newsletters")
    );
}

#[tokio::test]
async fn patch_settings_can_clear_default_account_without_replacing_rules() {
    let harness = SettingsHarness::new();
    harness.save_account("primary", "Primary");
    harness
        .service
        .put_app_settings(&AppSettings {
            default_account_id: Some(AccountId::from("primary")),
            automation_rules: vec![source_rule("primary")],
            automation_drafts: Vec::new(),
            ..Default::default()
        })
        .expect("settings should save");

    let Json(settings) = expect_settings_ok(
        patch_settings(
            State(harness.state.clone()),
            Json(PatchSettingsRequest {
                default_account_id: Some(None),
                cache_policy: None,
                automation_rules: None,
                automation_drafts: None,
            }),
        )
        .await,
    );

    assert_eq!(settings.default_account_id, None);
    assert_eq!(settings.automation_rules.len(), 1);
    let app_toml = harness.app_toml();
    assert!(app_toml.get("default_source_id").is_none());
    assert_eq!(
        app_toml["automations"][0]["id"].as_str(),
        Some("rule-newsletters")
    );
}

#[tokio::test]
async fn patch_settings_can_update_cache_policy() {
    let harness = SettingsHarness::new();

    let Json(settings) = expect_settings_ok(
        patch_settings(
            State(harness.state.clone()),
            Json(PatchSettingsRequest {
                default_account_id: None,
                cache_policy: Some(CachePolicy {
                    soft_cap_bytes: 64 * 1024 * 1024,
                    hard_cap_bytes: 32 * 1024 * 1024,
                    cache_bodies: true,
                    cache_raw_messages: false,
                    cache_attachments: false,
                }),
                automation_rules: None,
                automation_drafts: None,
            }),
        )
        .await,
    );

    assert_eq!(settings.cache_policy.soft_cap_bytes, 64 * 1024 * 1024);
    assert_eq!(settings.cache_policy.hard_cap_bytes, 64 * 1024 * 1024);
    let app_toml = harness.app_toml();
    assert_eq!(
        app_toml["cache"]["soft_cap_bytes"].as_integer(),
        Some(64 * 1024 * 1024)
    );
    assert_eq!(
        app_toml["cache"]["hard_cap_bytes"].as_integer(),
        Some(64 * 1024 * 1024)
    );
}

#[tokio::test]
async fn patch_settings_persists_incomplete_automation_drafts_without_enqueuing_backfill() {
    let harness = SettingsHarness::new();
    harness.save_account("primary", "Primary");
    harness
        .service
        .put_app_settings(&AppSettings {
            default_account_id: Some(AccountId::from("primary")),
            automation_rules: Vec::new(),
            automation_drafts: Vec::new(),
            ..Default::default()
        })
        .expect("settings should save");
    let mut draft = source_rule("primary");
    draft.id = "draft-newsletters".to_string();
    draft.name = String::new();
    draft.actions = vec![AutomationAction::ApplyTag { tag: String::new() }];

    let Json(settings) = expect_settings_ok(
        patch_settings(
            State(harness.state.clone()),
            Json(PatchSettingsRequest {
                default_account_id: None,
                cache_policy: None,
                automation_rules: None,
                automation_drafts: Some(vec![draft]),
            }),
        )
        .await,
    );

    assert_eq!(settings.automation_rules.len(), 0);
    assert_eq!(settings.automation_drafts.len(), 1);
    assert!(harness
        .service
        .automation_backfill_job_for_current_rules(&AccountId::from("primary"))
        .expect("backfill job should load")
        .is_none());
    let app_toml = harness.app_toml();
    assert_eq!(
        app_toml["draft_automations"][0]["id"].as_str(),
        Some("draft-newsletters")
    );
}

#[tokio::test]
async fn patch_settings_rejects_default_account_that_does_not_exist() {
    let harness = SettingsHarness::new();

    let error = patch_settings(
        State(harness.state.clone()),
        Json(PatchSettingsRequest {
            default_account_id: Some(Some("missing".to_string())),
            cache_policy: None,
            automation_rules: None,
            automation_drafts: None,
        }),
    )
    .await
    .expect_err("settings patch should reject missing default account");

    assert_eq!(error.into_response().status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        harness
            .service
            .get_app_settings()
            .expect("settings should load"),
        AppSettings::default()
    );
}

#[tokio::test]
async fn patch_settings_rejects_invalid_automation_rules_without_persisting() {
    let harness = SettingsHarness::new();
    harness.save_account("primary", "Primary");
    harness
        .service
        .put_app_settings(&AppSettings {
            default_account_id: Some(AccountId::from("primary")),
            automation_rules: Vec::new(),
            automation_drafts: Vec::new(),
            ..Default::default()
        })
        .expect("settings should save");
    let mut invalid_rule = source_rule("primary");
    invalid_rule.actions = Vec::new();

    let error = patch_settings(
        State(harness.state.clone()),
        Json(PatchSettingsRequest {
            default_account_id: None,
            cache_policy: None,
            automation_rules: Some(vec![invalid_rule]),
            automation_drafts: None,
        }),
    )
    .await
    .expect_err("settings patch should reject invalid automations");

    assert_eq!(error.into_response().status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        harness
            .service
            .get_app_settings()
            .expect("settings should load"),
        AppSettings {
            default_account_id: Some(AccountId::from("primary")),
            automation_rules: Vec::new(),
            automation_drafts: Vec::new(),
            ..Default::default()
        }
    );
    let app_toml = harness.app_toml();
    assert_eq!(
        app_toml["automations"]
            .as_array()
            .expect("automations should serialize as an array")
            .len(),
        0
    );
}
