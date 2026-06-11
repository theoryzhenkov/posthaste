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
