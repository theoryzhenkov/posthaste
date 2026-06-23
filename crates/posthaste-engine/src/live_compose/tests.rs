use posthaste_domain::Identity;

use super::*;

#[test]
fn send_identity_uses_requested_from_with_matching_identity_id() {
    let identity = resolve_send_identity(
        vec![
            Identity {
                id: "primary".to_string(),
                name: "Primary".to_string(),
                email: "primary@example.com".to_string(),
            },
            Identity {
                id: "alias".to_string(),
                name: "Alias".to_string(),
                email: "alias@example.com".to_string(),
            },
        ],
        Some(&posthaste_domain::Recipient {
            name: Some("Alias Sender".to_string()),
            email: "ALIAS@example.com".to_string(),
        }),
    )
    .expect("identity should resolve");

    assert_eq!(identity.id, "alias");
    assert_eq!(identity.name, "Alias Sender");
    assert_eq!(identity.email, "ALIAS@example.com");
}

#[test]
fn send_identity_uses_default_identity_id_for_freeform_sender() {
    let identity = resolve_send_identity(
        vec![Identity {
            id: "primary".to_string(),
            name: "Primary".to_string(),
            email: "primary@example.com".to_string(),
        }],
        Some(&posthaste_domain::Recipient {
            name: None,
            email: "catchall@example.com".to_string(),
        }),
    )
    .expect("identity should resolve");

    assert_eq!(identity.id, "primary");
    assert_eq!(identity.name, "Primary");
    assert_eq!(identity.email, "catchall@example.com");
}

#[test]
fn draft_sender_uses_requested_from_when_identity_get_is_empty() {
    // The Stalwart case: an empty `Identity/get` must not block saving a draft,
    // because a draft create carries no identityId — only the `from` address.
    let identity = resolve_draft_sender(
        Vec::new(),
        Some(&posthaste_domain::Recipient {
            name: Some("Casey".to_string()),
            email: "casey@example.com".to_string(),
        }),
    )
    .expect("draft sender should resolve from the requested address");

    assert_eq!(identity.email, "casey@example.com");
    assert_eq!(identity.name, "Casey");
    assert!(identity.id.is_empty());
}

#[test]
fn draft_sender_borrows_display_name_from_a_matching_identity() {
    // No display name supplied: fill it from the matching provider identity
    // while still using the requested address.
    let identity = resolve_draft_sender(
        vec![Identity {
            id: "primary".to_string(),
            name: "Primary Name".to_string(),
            email: "primary@example.com".to_string(),
        }],
        Some(&posthaste_domain::Recipient {
            name: None,
            email: "PRIMARY@example.com".to_string(),
        }),
    )
    .expect("draft sender should resolve");

    assert_eq!(identity.email, "PRIMARY@example.com");
    assert_eq!(identity.name, "Primary Name");
}

#[test]
fn draft_sender_requires_an_address_when_nothing_is_available() {
    // No requested `from` and no provider identity: there is no address to use.
    let result = resolve_draft_sender(Vec::new(), None);
    assert!(result.is_err());
}
