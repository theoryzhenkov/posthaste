use super::*;

fn json_array_strings(value: &serde_json::Value, key: &str) -> Vec<String> {
    value
        .get(key)
        .and_then(serde_json::Value::as_array)
        .unwrap_or_else(|| panic!("{key} should be an array"))
        .iter()
        .map(|item| {
            item.as_str()
                .unwrap_or_else(|| panic!("{key} entries should be strings"))
                .to_string()
        })
        .collect()
}

fn parse_json_fixture(raw: &str) -> serde_json::Value {
    serde_json::from_str(raw).expect("desktop fixture should parse as JSON")
}

#[test]
fn default_capability_is_limited_to_posthaste_windows_and_minimal_plugins() {
    let capability = parse_json_fixture(include_str!("../capabilities/default.json"));

    assert_eq!(
        capability
            .get("identifier")
            .and_then(|value| value.as_str()),
        Some("default")
    );
    assert_eq!(
        json_array_strings(&capability, "windows"),
        vec!["main", "settings", "message-*", "attachment-*", "compose-*"]
    );
    assert_eq!(
        json_array_strings(&capability, "permissions"),
        vec!["core:default", "opener:allow-open-url"]
    );
    assert!(
        capability.get("remote").is_none(),
        "default capability must not grant permissions to remote origins"
    );

    let permissions = json_array_strings(&capability, "permissions");
    let forbidden_prefixes = [
        "fs:",
        "shell:",
        "process:",
        "http:",
        "clipboard-manager:",
        "global-shortcut:",
        "notification:",
    ];
    for permission in permissions {
        assert!(
            !forbidden_prefixes
                .iter()
                .any(|prefix| permission.starts_with(prefix)),
            "default capability must not expose generic desktop permission {permission}"
        );
    }
}

#[test]
fn e2e_playwright_capability_is_local_and_main_window_only() {
    let capability = parse_json_fixture(include_str!("../capabilities/e2e-playwright.json"));

    assert_eq!(
        capability.get("local").and_then(|value| value.as_bool()),
        Some(true)
    );
    assert_eq!(json_array_strings(&capability, "windows"), vec!["main"]);
    assert_eq!(
        json_array_strings(&capability, "permissions"),
        vec!["playwright:default"]
    );
    let remote_urls = capability
        .get("remote")
        .and_then(|remote| remote.get("urls"))
        .and_then(serde_json::Value::as_array)
        .expect("e2e capability should list remote dev URLs")
        .iter()
        .map(|value| value.as_str().expect("remote URL should be a string"))
        .collect::<Vec<_>>();
    assert_eq!(
        remote_urls,
        vec!["http://127.0.0.1:5173/*", "http://localhost:5173/*"]
    );
}

#[test]
fn production_tauri_config_loads_bundled_renderer_assets() {
    let config = parse_json_fixture(include_str!("../tauri.conf.json"));

    assert_eq!(
        config
            .get("build")
            .and_then(|build| build.get("frontendDist"))
            .and_then(serde_json::Value::as_str),
        Some("../web/dist")
    );
    assert_eq!(
        config
            .get("build")
            .and_then(|build| build.get("devUrl"))
            .and_then(serde_json::Value::as_str),
        Some("http://127.0.0.1:5173")
    );
    assert_eq!(
        config
            .get("app")
            .and_then(|app| app.get("windows"))
            .and_then(serde_json::Value::as_array)
            .map(Vec::len),
        Some(0)
    );
}

#[test]
fn backend_init_script_injects_loopback_runtime_mode() {
    let script = backend_init_script(
        &BackendInjection {
            port: 4815,
            auth_token: "token-1".to_string(),
        },
        "main",
    );

    assert!(script.contains("__POSTHASTE_RUNTIME_MODE__"));
    assert!(script.contains("'loopback'"));
    assert!(script.contains("__POSTHASTE_PORT__"));
    assert!(script.contains("__POSTHASTE_TOKEN__"));
    assert!(script.contains("writable: false"));
}

#[test]
fn backend_init_script_keeps_loopback_token_out_of_urls() {
    let script = backend_init_script(
        &BackendInjection {
            port: 4815,
            auth_token: "secret-token".to_string(),
        },
        "main",
    );

    let runtime_mode_segment = script
        .split("__POSTHASTE_RUNTIME_MODE__")
        .nth(1)
        .and_then(|tail| tail.split("__POSTHASTE_PORT__").next())
        .expect("runtime mode should be defined before loopback connection fields");
    assert!(!runtime_mode_segment.contains("secret-token"));
    assert!(!runtime_mode_segment.contains("4815"));
    assert!(!script.contains("http://127.0.0.1:4815"));
    assert!(!script.contains("http://localhost:4815"));
    assert!(!script.contains("access_token="));
    assert!(!script.contains("Bearer secret-token"));
}

#[test]
fn external_web_urls_are_only_non_loopback_http() {
    let external = |s: &str| is_external_web_url(&url::Url::parse(s).unwrap());
    // Outbound email links open in the browser.
    assert!(external("https://example.com/path?q=1"));
    assert!(external("http://example.com/"));
    // The app's own loopback / tauri origins navigate in-app.
    assert!(!external("http://127.0.0.1:5173/index.html"));
    assert!(!external("http://localhost:1420/"));
    assert!(!external("https://tauri.localhost/index.html"));
    assert!(!external("tauri://localhost/index.html"));
    // Non-web schemes are never intercepted as web links.
    assert!(!external("mailto:hello@example.com"));
}

#[test]
fn message_surface_route_uses_hash_route_and_encoded_params() {
    let surface = SurfaceDescriptor::Message {
        disposition: SurfaceDisposition::Focused,
        params: MessageSurfaceParams {
            conversation_id: "conversation/1".to_string(),
            source_id: "source:primary".to_string(),
            message_id: "message 1".to_string(),
        },
    };

    assert_eq!(
        surface_route(&surface),
        "/surface/message?conversationId=conversation%2F1&sourceId=source%3Aprimary&messageId=message%201"
    );
}

#[test]
fn attachment_surface_route_uses_hash_route_and_encoded_params() {
    let surface = SurfaceDescriptor::Attachment {
        disposition: SurfaceDisposition::Focused,
        params: AttachmentSurfaceParams {
            source_id: "source:primary".to_string(),
            message_id: "message 1".to_string(),
            attachment_id: "part/2".to_string(),
        },
    };

    assert_eq!(
        surface_route(&surface),
        "/surface/attachment?sourceId=source%3Aprimary&messageId=message%201&attachmentId=part%2F2"
    );
}

#[test]
fn closeable_window_labels_distinguish_main_and_surface_windows() {
    assert!(is_main_window_label("main"));
    assert!(!is_main_window_label("settings"));
    assert!(!is_closeable_surface_window_label("main"));
    assert!(is_closeable_surface_window_label("settings"));
    assert!(is_closeable_surface_window_label(
        "message-0123456789abcdef"
    ));
    assert!(is_closeable_surface_window_label(
        "attachment-0123456789abcdef"
    ));
    assert!(is_closeable_surface_window_label(
        "compose-0123456789abcdef"
    ));
}

#[test]
fn compose_surface_descriptor_deserializes_frontend_camel_case_params() {
    let surface: SurfaceDescriptor = serde_json::from_value(serde_json::json!({
        "kind": "compose",
        "disposition": "focused",
        "params": {
            "kind": "reply",
            "sourceId": "source:primary",
            "messageId": "message 1"
        }
    }))
    .unwrap();

    assert_eq!(
        surface_route(&surface),
        "/surface/compose?composeKind=reply&sourceId=source%3Aprimary&messageId=message%201"
    );
}

#[test]
fn compose_surface_route_uses_hash_route_and_encoded_params() {
    let surface = SurfaceDescriptor::Compose {
        disposition: SurfaceDisposition::Focused,
        params: ComposeSurfaceParams::Reply {
            source_id: "source:primary".to_string(),
            message_id: "message 1".to_string(),
        },
    };

    assert_eq!(
        surface_route(&surface),
        "/surface/compose?composeKind=reply&sourceId=source%3Aprimary&messageId=message%201"
    );
    assert!(surface_window_label(&surface).starts_with("compose-"));
    assert_eq!(surface_title(&surface), "Posthaste Compose");
    assert_eq!(surface_window_size(&surface), (780.0, 640.0));
}

#[test]
fn message_window_label_is_stable_and_safe() {
    let surface = SurfaceDescriptor::Message {
        disposition: SurfaceDisposition::Focused,
        params: MessageSurfaceParams {
            conversation_id: "conversation/1".to_string(),
            source_id: "source:primary".to_string(),
            message_id: "message 1".to_string(),
        },
    };

    assert!(surface_window_label(&surface).starts_with("message-"));
    assert_eq!(
        surface_window_label(&surface),
        surface_window_label(&surface)
    );
}

#[test]
fn settings_window_navigation_script_replaces_hash_route() {
    let script = surface_window_navigation_script("/surface/settings?category=accounts");

    assert!(script.contains("window.history.replaceState"));
    assert!(script.contains("window.dispatchEvent(new HashChangeEvent('hashchange'))"));
    assert!(script.contains("\"/surface/settings?category=accounts\""));
}

#[test]
fn settings_surface_descriptor_deserializes_frontend_camel_case_target() {
    let surface = serde_json::from_value::<SurfaceDescriptor>(serde_json::json!({
        "kind": "settings",
        "disposition": "focused",
        "params": {
            "category": "mailboxes",
            "target": {
                "kind": "sourceMailbox",
                "sourceAccountId": "primary",
                "sourceMailboxId": "inbox"
            }
        }
    }))
    .expect("frontend settings surface descriptors should deserialize");

    assert_eq!(
        surface_route(&surface),
        "/surface/settings?category=mailboxes&targetKind=sourceMailbox&sourceAccountId=primary&sourceMailboxId=inbox"
    );
    assert!(validate_surface_descriptor(&surface).is_ok());
}

#[test]
fn settings_surface_rejects_unknown_frontend_category() {
    let result = serde_json::from_value::<SurfaceDescriptor>(serde_json::json!({
        "kind": "settings",
        "disposition": "focused",
        "params": {
            "category": "advanced"
        }
    }));

    assert!(result.is_err());
}

#[test]
fn surface_descriptors_reject_unknown_frontend_fields() {
    let result = serde_json::from_value::<SurfaceDescriptor>(serde_json::json!({
        "kind": "compose",
        "disposition": "focused",
        "params": {
            "kind": "new",
            "sourceId": "primary",
            "draftId": "unexpected"
        }
    }));

    assert!(result.is_err());
}

#[test]
fn settings_surface_category_must_match_target_kind() {
    let surface = SurfaceDescriptor::Settings {
        disposition: SurfaceDisposition::Focused,
        params: SettingsSurfaceParams {
            category: Some(SettingsSurfaceCategory::Mailboxes),
            target: Some(SettingsSurfaceTarget::Account {
                account_id: "primary".to_string(),
            }),
        },
    };

    assert_eq!(
        validate_surface_descriptor(&surface).unwrap_err(),
        "settings surface category does not match target kind"
    );
}

#[test]
fn settings_window_reuses_stable_label() {
    let surface = SurfaceDescriptor::Settings {
        disposition: SurfaceDisposition::Focused,
        params: SettingsSurfaceParams {
            category: Some(SettingsSurfaceCategory::Accounts),
            target: Some(SettingsSurfaceTarget::Account {
                account_id: "primary".to_string(),
            }),
        },
    };

    assert_eq!(surface_window_label(&surface), "settings");
    assert_eq!(
        surface_route(&surface),
        "/surface/settings?category=accounts&targetKind=account&accountId=primary"
    );
}

#[test]
fn external_url_validation_accepts_http_urls() {
    assert!(validate_external_url("https://accounts.example.test/oauth").is_ok());
    assert!(validate_external_url("http://127.0.0.1/callback").is_ok());
}

#[test]
fn external_url_validation_rejects_non_web_urls() {
    assert!(validate_external_url("file:///tmp/secret").is_err());
    assert!(validate_external_url("not a url").is_err());
}

#[test]
fn log_token_accepts_short_ascii_metadata() {
    assert_eq!(
        log_token(Some(" mail.search:preview_1 ".to_string())),
        "mail.search:preview_1"
    );
}

#[test]
fn log_token_rejects_unsafe_metadata() {
    assert_eq!(log_token(Some("mail search".to_string())), "");
    assert_eq!(log_token(Some("mail/search".to_string())), "");
    assert_eq!(log_token(Some("x".repeat(129))), "");
}
