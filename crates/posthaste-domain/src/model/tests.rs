use super::*;

fn configured_secret() -> SecretStatus {
    SecretStatus {
        storage: SecretStorage::Os,
        configured: true,
        label: None,
    }
}

#[test]
fn message_event_topics_preserve_serialized_strings() {
    assert_eq!(
        EVENT_TOPIC_MESSAGE_KEYWORDS_CHANGED,
        "message.keywords_changed"
    );
    assert_eq!(EVENT_TOPIC_MESSAGE_BODY_CACHED, "message.body_cached");
    assert_eq!(
        EVENT_TOPIC_MESSAGE_MAILBOXES_CHANGED,
        "message.mailboxes_changed"
    );
}

#[test]
fn account_driver_capabilities_match_cache_and_push_behavior() {
    assert_eq!(
        AccountDriver::Jmap.capabilities(),
        AccountDriverCapabilities {
            cache_fetch_unit: crate::CacheFetchUnit::BodyOnly,
            supports_push: true,
        }
    );
    assert_eq!(
        AccountDriver::ImapSmtp.capabilities(),
        AccountDriverCapabilities {
            cache_fetch_unit: crate::CacheFetchUnit::RawMessage,
            supports_push: false,
        }
    );
    assert_eq!(
        AccountDriver::Mock.capabilities(),
        AccountDriverCapabilities {
            cache_fetch_unit: crate::CacheFetchUnit::BodyOnly,
            supports_push: false,
        }
    );
}

#[test]
fn service_error_kind_preserves_existing_codes() {
    let cases = [
        (
            ServiceError::from(GatewayError::Auth),
            ServiceErrorKind::AuthError,
            "auth_error",
        ),
        (
            ServiceError::from(GatewayError::StateMismatch),
            ServiceErrorKind::StateMismatch,
            "state_mismatch",
        ),
        (
            ServiceError::from(GatewayError::Network("timeout".to_string())),
            ServiceErrorKind::NetworkError,
            "network_error",
        ),
        (
            ServiceError::from(SecretStoreError::Unsupported("os".to_string())),
            ServiceErrorKind::SecretUnsupported,
            "secret_unsupported",
        ),
        (
            ServiceError::from(StoreError::NotFound("message:1".to_string())),
            ServiceErrorKind::NotFound,
            "not_found",
        ),
        (
            ServiceError::from(StoreError::Failure("disk full".to_string())),
            ServiceErrorKind::StorageFailure,
            "storage_failure",
        ),
        (
            ServiceError::from(ConfigError::Validation("bad source".to_string())),
            ServiceErrorKind::ConfigValidation,
            "config_validation",
        ),
        (
            ServiceError::from(ConfigError::Io("denied".to_string())),
            ServiceErrorKind::ConfigIo,
            "config_io",
        ),
    ];

    for (error, kind, code) in cases {
        assert_eq!(error.kind(), kind);
        assert_eq!(error.code(), code);
        assert_eq!(error.kind().code(), code);
    }
}

#[test]
fn message_record_deserializes_without_recipients() {
    let record: MessageRecord = serde_json::from_value(serde_json::json!({
        "id": "message-1",
        "sourceThreadId": "thread-1",
        "remoteBlobId": null,
        "subject": "Legacy message",
        "fromName": null,
        "fromEmail": "sender@example.com",
        "preview": null,
        "receivedAt": "2026-05-24T00:00:00Z",
        "hasAttachment": false,
        "size": 0,
        "mailboxIds": [],
        "keywords": [],
        "bodyHtml": null,
        "bodyText": null,
        "rawMime": null,
        "rfcMessageId": null,
        "inReplyTo": null,
        "references": []
    }))
    .expect("legacy message record should deserialize");

    assert!(record.to.is_empty());
}

#[test]
fn manual_connection_overview_serializes_editable_credentials_variant() {
    let value = serde_json::to_value(AccountConnectionOverview::ManualCredentials {
        provider: ProviderHint::Generic,
        provider_kind: ProviderKind::Generic,
        auth: ProviderAuthKind::AppPassword,
        base_url: Some("https://mail.example.com/jmap".to_string()),
        username: Some("me@example.com".to_string()),
        imap: None,
        smtp: None,
        secret: configured_secret(),
    })
    .expect("serialize connection overview");

    assert_eq!(value["kind"], "manualCredentials");
    assert_eq!(value["provider"], "generic");
    assert_eq!(value["providerKind"], "generic");
    assert_eq!(value["auth"], "appPassword");
    assert_eq!(value["baseUrl"], "https://mail.example.com/jmap");
    assert_eq!(value["username"], "me@example.com");
}

#[test]
fn oauth_connection_overview_serializes_managed_variant_without_base_url() {
    let value = serde_json::to_value(AccountConnectionOverview::ManagedOAuth {
        provider: ProviderHint::Gmail,
        provider_kind: ProviderKind::Gmail,
        auth: ProviderAuthKind::OAuth2,
        username: Some("me@gmail.com".to_string()),
        imap: None,
        smtp: None,
        secret: configured_secret(),
    })
    .expect("serialize connection overview");

    assert_eq!(value["kind"], "managedOAuth");
    assert_eq!(value["provider"], "gmail");
    assert_eq!(value["providerKind"], "gmail");
    assert_eq!(value["auth"], "oauth2");
    assert!(value.get("baseUrl").is_none());
}

#[test]
fn connection_overview_deserializes_legacy_provider_without_provider_kind() {
    let value: AccountConnectionOverview = serde_json::from_value(serde_json::json!({
        "kind": "managedOAuth",
        "provider": "gmail",
        "auth": "oauth2",
        "username": "me@gmail.com",
        "imap": null,
        "smtp": null,
        "secret": {
            "storage": "os",
            "configured": true,
            "label": null
        }
    }))
    .expect("legacy connection overview should deserialize");

    match value {
        AccountConnectionOverview::ManagedOAuth { provider_kind, .. } => {
            assert_eq!(provider_kind, ProviderKind::Gmail);
        }
        AccountConnectionOverview::ManualCredentials { .. } => {
            panic!("expected managed OAuth variant");
        }
    }
}
