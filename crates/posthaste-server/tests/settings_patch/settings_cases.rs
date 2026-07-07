use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use posthaste_domain_model::{
    AccountId, AppSettings, Appearance, AutomationAction, AutomationBackfillJobStatus, CachePolicy,
    GlassBloom, GlassTheme, MailboxColor, MailboxId, TagAppearance, ThemeColors, ThemeMode,
    UiDensity,
};
use posthaste_http_api_adapter::api::{patch_settings, PatchSettingsRequest};

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
                compose: None,
                force_backfill: false,
                mailbox_groups: None,
                smart_mailbox_order: None,
                account_order: None,
                default_account_id: None,
                cache_policy: None,
                automation_rules: Some(vec![source_rule("primary")]),
                automation_drafts: None,
                appearance: None,
                notifications: None,
                mailbox_colors: None,
                tags: None,
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
                compose: None,
                force_backfill: false,
                mailbox_groups: None,
                smart_mailbox_order: None,
                account_order: None,
                default_account_id: Some(None),
                cache_policy: None,
                automation_rules: None,
                automation_drafts: None,
                appearance: None,
                notifications: None,
                mailbox_colors: None,
                tags: None,
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
                compose: None,
                force_backfill: false,
                mailbox_groups: None,
                smart_mailbox_order: None,
                account_order: None,
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
                appearance: None,
                notifications: None,
                mailbox_colors: None,
                tags: None,
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
                compose: None,
                force_backfill: false,
                mailbox_groups: None,
                smart_mailbox_order: None,
                account_order: None,
                default_account_id: None,
                cache_policy: None,
                automation_rules: None,
                automation_drafts: Some(vec![draft]),
                appearance: None,
                notifications: None,
                mailbox_colors: None,
                tags: None,
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
            compose: None,
            force_backfill: false,
            mailbox_groups: None,
            smart_mailbox_order: None,
            account_order: None,
            default_account_id: Some(Some("missing".to_string())),
            cache_policy: None,
            automation_rules: None,
            automation_drafts: None,
            appearance: None,
            notifications: None,
            mailbox_colors: None,
            tags: None,
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
            compose: None,
            force_backfill: false,
            mailbox_groups: None,
            smart_mailbox_order: None,
            account_order: None,
            default_account_id: None,
            cache_policy: None,
            automation_rules: Some(vec![invalid_rule]),
            automation_drafts: None,
            appearance: None,
            notifications: None,
            mailbox_colors: None,
            tags: None,
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

#[tokio::test]
async fn patch_settings_persists_appearance_to_app_toml() {
    let harness = SettingsHarness::new();

    let Json(settings) = expect_settings_ok(
        patch_settings(
            State(harness.state.clone()),
            Json(PatchSettingsRequest {
                compose: None,
                force_backfill: false,
                mailbox_groups: None,
                smart_mailbox_order: None,
                account_order: None,
                default_account_id: None,
                cache_policy: None,
                automation_rules: None,
                automation_drafts: None,
                notifications: None,
                mailbox_colors: None,
                tags: None,
                appearance: Some(Appearance {
                    mode: Some(ThemeMode::Dark),
                    theme: Some("glass".to_string()),
                    density: Some(UiDensity::Compact),
                    light: Some(ThemeColors {
                        accent_hue: Some(210),
                        surface_hue: Some(40),
                        ..ThemeColors::default()
                    }),
                    dark: Some(ThemeColors {
                        accent_hue: Some(250),
                        surface_hue: Some(260),
                        ..ThemeColors::default()
                    }),
                    glass_theme: Some(GlassTheme {
                        blooms: vec![GlassBloom {
                            id: "bloom-1".to_string(),
                            hue: 285,
                            x: 20.0,
                            y: 10.0,
                            opacity: 0.35,
                            radius: 45.0,
                        }],
                    }),
                }),
            }),
        )
        .await,
    );

    let appearance = settings.appearance.expect("appearance should be set");
    assert_eq!(appearance.mode, Some(ThemeMode::Dark));
    assert_eq!(appearance.theme.as_deref(), Some("glass"));
    assert_eq!(
        appearance.dark.as_ref().and_then(|c| c.accent_hue),
        Some(250)
    );
    assert_eq!(
        appearance.light.as_ref().and_then(|c| c.surface_hue),
        Some(40)
    );
    let glass = appearance.glass_theme.expect("glass theme should be set");
    assert_eq!(glass.blooms.len(), 1);
    assert_eq!(glass.blooms[0].id, "bloom-1");

    // The TOML file is the source of truth: the appearance round-trips through it
    // with snake_case keys and per-mode color tables.
    let app_toml = harness.app_toml();
    assert_eq!(app_toml["appearance"]["mode"].as_str(), Some("dark"));
    assert_eq!(app_toml["appearance"]["theme"].as_str(), Some("glass"));
    assert_eq!(
        app_toml["appearance"]["dark"]["accent_hue"].as_integer(),
        Some(250),
    );
    assert_eq!(
        app_toml["appearance"]["glass_theme"]["blooms"][0]["id"].as_str(),
        Some("bloom-1"),
    );
}

#[tokio::test]
async fn patch_settings_persists_mailbox_colors_to_app_toml() {
    let harness = SettingsHarness::new();

    let Json(settings) = expect_settings_ok(
        patch_settings(
            State(harness.state.clone()),
            Json(PatchSettingsRequest {
                compose: None,
                force_backfill: false,
                mailbox_groups: None,
                smart_mailbox_order: None,
                account_order: None,
                default_account_id: None,
                cache_policy: None,
                automation_rules: None,
                automation_drafts: None,
                appearance: None,
                notifications: None,
                mailbox_colors: Some(vec![MailboxColor {
                    source_id: AccountId::from("primary"),
                    mailbox_id: MailboxId::from("INBOX"),
                    hue: 200,
                }]),
                tags: None,
            }),
        )
        .await,
    );

    assert_eq!(settings.mailbox_colors.len(), 1);
    assert_eq!(settings.mailbox_colors[0].mailbox_id.as_str(), "INBOX");
    assert_eq!(settings.mailbox_colors[0].hue, 200);

    // The TOML file is the source of truth: a `[[mailbox_colors]]` array table.
    let app_toml = harness.app_toml();
    assert_eq!(
        app_toml["mailbox_colors"][0]["mailbox_id"].as_str(),
        Some("INBOX"),
    );
    assert_eq!(app_toml["mailbox_colors"][0]["hue"].as_integer(), Some(200),);
}

#[tokio::test]
async fn patch_settings_persists_tag_appearance_to_app_toml() {
    let harness = SettingsHarness::new();

    let Json(settings) = expect_settings_ok(
        patch_settings(
            State(harness.state.clone()),
            Json(PatchSettingsRequest {
                compose: None,
                force_backfill: false,
                mailbox_groups: None,
                smart_mailbox_order: None,
                account_order: None,
                default_account_id: None,
                cache_policy: None,
                automation_rules: None,
                automation_drafts: None,
                appearance: None,
                notifications: None,
                mailbox_colors: None,
                tags: Some(vec![TagAppearance {
                    name: "work".to_string(),
                    fg: Some("#1f2937".to_string()),
                    bg: Some("#dbeafe".to_string()),
                    icon: Some("briefcase".to_string()),
                }]),
            }),
        )
        .await,
    );

    assert_eq!(settings.tags.len(), 1);
    assert_eq!(settings.tags[0].name, "work");
    assert_eq!(settings.tags[0].icon.as_deref(), Some("briefcase"));

    // The TOML file is the source of truth: a `[[tags]]` array table. Absent
    // optional fields are omitted (skip_serializing_if).
    let app_toml = harness.app_toml();
    assert_eq!(app_toml["tags"][0]["name"].as_str(), Some("work"));
    assert_eq!(app_toml["tags"][0]["bg"].as_str(), Some("#dbeafe"));
    assert_eq!(app_toml["tags"][0]["icon"].as_str(), Some("briefcase"));
}
