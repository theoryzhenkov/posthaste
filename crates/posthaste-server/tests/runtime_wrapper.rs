//! API runtime-wrapper migration tests.
//!
//! spec: docs/eph/PLAN-L3-api-runtime-wrapper-migration#wrapper-fitness-tests

mod support;

use std::fs;
use std::path::{Path, PathBuf};

use axum::http::StatusCode;
use posthaste_runtime_contract::RuntimeLifecycle;
use support::Harness;

#[tokio::test]
async fn api_harness_state_exposes_runtime_handle_status() {
    let harness = Harness::new();

    let status = harness.runtime_status().await;

    assert_eq!(status.lifecycle, RuntimeLifecycle::Ready);
    assert!(status.store.config_loaded);
    assert!(status.store.state_store_open);
    assert!(
        !status.store.cache_root_ready,
        "manual API harnesses attach a legacy graph and do not build cache roots"
    );
}

// spec: docs/eph/PLAN-L3-api-runtime-wrapper-migration#wrapper-fitness-tests
// spec: docs/backend/L3#first-api-read-runtime-backed
#[tokio::test]
async fn get_settings_matches_runtime_read_projection() {
    let harness = Harness::new();
    let token = harness.full_scope();

    let runtime_settings = harness.runtime_app_settings().await;
    let (status, body) = harness.get_json(&token, "/v1/settings").await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body,
        serde_json::to_value(runtime_settings).expect("settings should serialize")
    );
}

// spec: docs/eph/PLAN-L3-api-runtime-wrapper-migration#no-new-route-service-graphs
// spec: docs/eph/PLAN-L3-api-runtime-wrapper-migration#wrapper-fitness-tests
// spec: docs/backend/L3#account-list-runtime-backed
#[tokio::test]
async fn list_accounts_and_read_account_list_match_runtime_projection() {
    let harness = Harness::new();
    harness.save_account("acct-a", "Account A", true);
    harness.save_account("acct-b", "Account B", false);
    let token = harness.full_scope();

    let runtime_accounts = harness.runtime_accounts().await;

    let (list_status, list_body) = harness.get_json(&token, "/v1/accounts").await;
    assert_eq!(list_status, StatusCode::OK);
    assert_eq!(
        list_body,
        serde_json::to_value(&runtime_accounts.items).expect("account items should serialize")
    );

    let (read_status, read_body) = harness
        .post_json(
            &token,
            "/v1/read",
            serde_json::json!({
                "calls": [{"id": "accounts", "op": "Account/list", "args": {}}]
            }),
        )
        .await;
    assert_eq!(read_status, StatusCode::OK);
    assert_eq!(
        read_body["results"]["accounts"]["value"],
        serde_json::to_value(&runtime_accounts).expect("runtime account list should serialize")
    );
}

// spec: docs/eph/PLAN-L3-api-runtime-wrapper-migration#wrapper-fitness-tests
// spec: docs/backend/L3#account-get-runtime-backed
#[tokio::test]
async fn get_account_matches_runtime_account_projection() {
    let harness = Harness::new();
    harness.save_account("acct-a", "Account A", true);
    let token = harness.full_scope();

    let runtime_account = harness
        .runtime_accounts()
        .await
        .items
        .into_iter()
        .find(|account| account.id.as_str() == "acct-a")
        .expect("runtime account should exist");
    let (status, body) = harness.get_json(&token, "/v1/accounts/acct-a").await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body,
        serde_json::to_value(runtime_account).expect("account should serialize")
    );
}

// spec: docs/eph/PLAN-L3-api-runtime-wrapper-migration#wrapper-fitness-tests
#[tokio::test]
async fn read_account_list_enabled_ids_reference_still_drives_followup_reads() {
    let harness = Harness::new();
    harness.save_account("acct-a", "Account A", true);
    harness.save_account("acct-b", "Account B", false);
    let token = harness.full_scope();

    let (status, body) = harness
        .post_json(
            &token,
            "/v1/read",
            serde_json::json!({
                "calls": [
                    {"id": "accounts", "op": "Account/list", "args": {}},
                    {
                        "id": "mailboxes",
                        "op": "Mailbox/list",
                        "args": {"accountIds": "#accounts.enabledIds"}
                    }
                ]
            }),
        )
        .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body["results"]["accounts"]["value"]["enabledIds"],
        serde_json::json!(["acct-a"])
    );
    assert!(body["results"]["mailboxes"]["value"]["byAccountId"]["acct-a"].is_array());
    assert!(body["results"]["mailboxes"]["value"]["byAccountId"]
        .get("acct-b")
        .is_none());
}

// spec: docs/eph/PLAN-L3-api-runtime-wrapper-migration#wrapper-fitness-tests
// spec: docs/backend/L3#typed-read-catalog-runtime-backed
#[tokio::test]
async fn typed_read_catalog_reads_work_after_runtime_migration() {
    let harness = Harness::new();
    harness.save_account("acct-a", "Account A", true);
    let token = harness.full_scope();

    let (status, body) = harness
        .post_json(
            &token,
            "/v1/read",
            serde_json::json!({
                "calls": [
                    {"id": "accounts", "op": "Account/list", "args": {}},
                    {"id": "mailboxes", "op": "Mailbox/list", "args": {"accountIds": "#accounts.enabledIds"}},
                    {"id": "smart", "op": "SmartMailbox/list", "args": {}},
                    {"id": "tags", "op": "Tag/list", "args": {"accountIds": "#accounts.enabledIds"}}
                ]
            }),
        )
        .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["results"]["accounts"]["op"], "Account/list");
    assert_eq!(body["results"]["mailboxes"]["op"], "Mailbox/list");
    assert_eq!(body["results"]["smart"]["op"], "SmartMailbox/list");
    assert_eq!(body["results"]["tags"]["op"], "Tag/list");
}

// spec: docs/eph/PLAN-L3-api-runtime-wrapper-migration#wrapper-fitness-tests
// spec: docs/backend/L3#collection-list-routes-runtime-backed
#[tokio::test]
async fn list_mailboxes_and_smart_mailboxes_routes_use_runtime_reads() {
    let harness = Harness::new();
    harness.save_account("acct-a", "Account A", true);
    let token = harness.full_scope();

    let (mailbox_status, mailbox_body) = harness
        .get_json(&token, "/v1/sources/acct-a/mailboxes")
        .await;
    assert_eq!(mailbox_status, StatusCode::OK);
    assert!(mailbox_body.is_array());

    let (smart_status, smart_body) = harness.get_json(&token, "/v1/smart-mailboxes").await;
    assert_eq!(smart_status, StatusCode::OK);
    assert!(smart_body.is_array());
}

// spec: docs/eph/PLAN-L3-api-runtime-wrapper-migration#wrapper-fitness-tests
// spec: docs/backend/L3#collection-list-routes-runtime-backed
#[tokio::test]
async fn get_smart_mailbox_route_uses_runtime_read() {
    let harness = Harness::new();
    let token = harness.full_scope();

    let (status, body) = harness
        .get_json(&token, "/v1/smart-mailboxes/default-inbox")
        .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["id"], "default-inbox");
    assert_eq!(body["name"], "Inbox");
}

// spec: docs/eph/PLAN-L3-api-runtime-wrapper-migration#wrapper-fitness-tests
// spec: docs/backend/L3#provider-backed-compose-reads-runtime-backed
#[tokio::test]
async fn identity_route_uses_runtime_provider_backed_read() {
    let harness = Harness::new();
    harness.save_account("acct-a", "Account A", true);
    harness.start_account_runtime("acct-a").await;
    let token = harness.full_scope();

    let (status, body) = harness
        .get_json(&token, "/v1/sources/acct-a/identity")
        .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["id"], "mock-identity");
    assert_eq!(body["email"], "mock@example.com");
}

// spec: docs/eph/PLAN-L3-api-runtime-wrapper-migration#wrapper-fitness-tests
// spec: docs/backend/L3#sync-command-runtime-backed
#[tokio::test]
async fn sync_command_route_uses_runtime_provider() {
    let harness = Harness::new();
    harness.save_account("acct-a", "Account A", true);
    harness.start_account_runtime("acct-a").await;
    let token = harness.full_scope();

    let (status, body) = harness
        .post_json(
            &token,
            "/v1/sources/acct-a/commands/sync",
            serde_json::json!({"mode": "incremental"}),
        )
        .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["ok"], true);
    assert_eq!(body["mode"], "incremental");
}

// spec: docs/eph/PLAN-L3-api-runtime-wrapper-migration#wrapper-fitness-tests
// spec: docs/backend/L3#provider-backed-compose-reads-runtime-backed
#[tokio::test]
async fn sender_address_route_uses_runtime_read() {
    let harness = Harness::new();
    harness.remember_sender_address("acct-a", Some("Alice"), "alice@example.com");
    let token = harness.full_scope();

    let (status, body) = harness.get_json(&token, "/v1/sender-addresses").await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body[0]["sourceId"], "acct-a");
    assert_eq!(body[0]["name"], "Alice");
    assert_eq!(body[0]["email"], "alice@example.com");
}

// spec: docs/eph/PLAN-L3-api-runtime-wrapper-migration#wrapper-fitness-tests
// spec: docs/backend/L3#account-mutations-runtime-backed
#[tokio::test]
async fn account_crud_and_lifecycle_routes_match_runtime_projection() {
    let harness = Harness::new();
    let token = harness.full_scope();

    let (create_status, created) = harness
        .post_json(
            &token,
            "/v1/accounts",
            serde_json::json!({
                "id": "acct-runtime",
                "name": "Runtime Account",
                "driver": "mock",
                "enabled": true
            }),
        )
        .await;
    assert_eq!(create_status, StatusCode::OK);
    assert_eq!(created["id"], "acct-runtime");

    let runtime_account = harness
        .runtime_accounts()
        .await
        .items
        .into_iter()
        .find(|account| account.id.as_str() == "acct-runtime")
        .expect("created account should be visible through runtime");
    assert_eq!(created, serde_json::to_value(runtime_account).unwrap());

    let (patch_status, patched) = harness
        .patch_json(
            &token,
            "/v1/accounts/acct-runtime",
            serde_json::json!({"name": "Runtime Account Renamed"}),
        )
        .await;
    assert_eq!(patch_status, StatusCode::OK);
    assert_eq!(patched["name"], "Runtime Account Renamed");

    let (disable_status, disable_body) = harness
        .post_json(
            &token,
            "/v1/accounts/acct-runtime/disable",
            serde_json::json!({}),
        )
        .await;
    assert_eq!(disable_status, StatusCode::OK);
    assert_eq!(disable_body["ok"], true);

    let (enable_status, enable_body) = harness
        .post_json(
            &token,
            "/v1/accounts/acct-runtime/enable",
            serde_json::json!({}),
        )
        .await;
    assert_eq!(enable_status, StatusCode::OK);
    assert_eq!(enable_body["ok"], true);

    let (verify_status, verify_body) = harness
        .post_json(
            &token,
            "/v1/accounts/acct-runtime/verify",
            serde_json::json!({}),
        )
        .await;
    assert_eq!(verify_status, StatusCode::OK);
    assert_eq!(verify_body["ok"], true);

    let (reload_status, reload_body) = harness
        .post_json(&token, "/v1/config:reload", serde_json::json!({}))
        .await;
    assert_eq!(reload_status, StatusCode::OK);
    assert_eq!(reload_body["ok"], true);
}

// spec: docs/eph/PLAN-L3-api-runtime-wrapper-migration#wrapper-fitness-tests
// spec: docs/backend/L3#account-logo-metadata-runtime-backed
#[test]
fn account_asset_routes_keep_metadata_and_delete_linkage_behind_runtime() {
    let server_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/api");
    let source = ["accounts/logos.rs", "accounts/crud.rs"]
        .into_iter()
        .map(|relative| {
            fs::read_to_string(server_dir.join(relative))
                .expect("account route source should be readable")
        })
        .collect::<Vec<_>>()
        .join("\n");
    for forbidden in [
        "state.service.save_source",
        "state.service.delete_source",
        "state.supervisor",
        "delete_managed_secret(state.as_ref()",
        "append_and_publish_account_event",
    ] {
        assert!(
            !source.contains(forbidden),
            "account asset routes should use AuthorityRuntimeHandle instead of {forbidden}"
        );
    }
}

// spec: docs/eph/PLAN-L3-api-runtime-wrapper-migration#wrapper-fitness-tests
#[test]
fn app_state_does_not_expose_legacy_runtime_parts_to_routes() {
    let server_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
    let source = fs::read_to_string(server_dir.join("app_state.rs"))
        .expect("app state source should be readable");
    for forbidden in [
        "pub service:",
        "pub store:",
        "pub secret_store:",
        "pub supervisor:",
        "pub event_sender:",
        "fn publish_events",
    ] {
        assert!(
            !source.contains(forbidden),
            "AppState should expose the runtime handle and HTTP adapter state instead of {forbidden}"
        );
    }
}

// spec: docs/eph/PLAN-L3-api-runtime-wrapper-migration#wrapper-fitness-tests
// spec: docs/backend/L3#message-mutations-runtime-backed
#[test]
fn migrated_runtime_routes_do_not_call_legacy_state_directly() {
    let server_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/api");
    for relative in [
        "message_commands.rs",
        "messages/compose.rs",
        "messages/detail.rs",
        "messages/listing.rs",
        "mailboxes.rs",
        "settings.rs",
        "smart_mailboxes/crud.rs",
        "smart_mailboxes/listings.rs",
    ] {
        let path = server_dir.join(relative);
        let source = fs::read_to_string(&path).expect("route source should be readable");
        for forbidden in [
            "live_gateway(state.as_ref()",
            "optional_live_gateway(state.as_ref()",
            "state.service",
            "state.store",
            "state.secret_store",
            "state.supervisor",
            "state.event_sender",
            "state.publish_events",
        ] {
            assert!(
                !source.contains(forbidden),
                "{} should use AuthorityRuntimeHandle instead of {forbidden}",
                path.display()
            );
        }
    }
}

#[test]
fn authority_runtime_core_does_not_use_api_bridge_as_dependency_bag() {
    let server_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let runtime_build =
        fs::read_to_string(server_dir.join("../posthaste-authority-runtime/src/build.rs"))
            .expect("authority runtime build source should be readable");
    let core_start = runtime_build
        .find("struct AuthorityRuntimeCore")
        .expect("authority runtime core struct should exist");
    let core_end = runtime_build[core_start..]
        .find("/// Cloneable authority runtime handle")
        .expect("runtime handle section should follow core struct")
        + core_start;
    let core_struct = &runtime_build[core_start..core_end];

    assert!(
        !core_struct.contains("api_bridge"),
        "AuthorityRuntimeCore should own explicit runtime dependencies, not api_bridge"
    );
    assert!(
        !runtime_build.contains("self.core.api_bridge"),
        "runtime methods should not use api_bridge as an internal dependency bag"
    );
}

#[test]
fn runtime_contract_exposes_single_mail_query_entry_point() {
    let server_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let contract = fs::read_to_string(server_dir.join("../posthaste-runtime-contract/src/lib.rs"))
        .expect("runtime contract source should be readable");
    assert!(
        contract.contains("async fn query_mail_page"),
        "mail-list/search reads should use the generalized query entry point"
    );
    for forbidden in [
        "list_smart_mailbox_message_page",
        "list_smart_mailbox_conversations",
        "query_message_page_by_rule",
        "query_conversations_by_rule",
        "record_search_cache_visibility",
        "async fn get_conversation",
    ] {
        assert!(
            !contract.contains(forbidden),
            "runtime contract should not expose route-shaped mail query method {forbidden}"
        );
    }
}

#[test]
fn api_route_modules_do_not_construct_new_mail_runtime_graphs() {
    let api_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/api");
    let mut violations = Vec::new();
    collect_forbidden_runtime_graph_constructors(&api_dir, &mut violations);

    assert!(
        violations.is_empty(),
        "route modules must not construct runtime service/store/supervisor graphs:\n{}",
        violations.join("\n")
    );
}

fn collect_forbidden_runtime_graph_constructors(path: &Path, violations: &mut Vec<String>) {
    let metadata = fs::metadata(path).expect("api path metadata should be readable");
    if metadata.is_dir() {
        for entry in fs::read_dir(path).expect("api directory should be readable") {
            let entry = entry.expect("api directory entry should be readable");
            collect_forbidden_runtime_graph_constructors(&entry.path(), violations);
        }
        return;
    }

    if path.extension().and_then(|extension| extension.to_str()) != Some("rs") {
        return;
    }
    if path.file_name().and_then(|name| name.to_str()) == Some("tests.rs") {
        return;
    }

    let source = fs::read_to_string(path).expect("api source file should be readable");
    for forbidden in [
        "MailService::new",
        "DatabaseStore::open",
        "AccountSupervisor::new",
    ] {
        if source.contains(forbidden) {
            violations.push(format!("{} contains {forbidden}", path.display()));
        }
    }
}
