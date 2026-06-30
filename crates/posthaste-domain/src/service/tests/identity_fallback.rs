use super::*;

#[tokio::test]
async fn fetch_identity_falls_back_to_configured_sender() {
    // The gateway exposes no identity (Stalwart's Identity/get is empty); the
    // service falls back to the account's configured address + display name so
    // compose still gets a default `from`.
    let account_id = AccountId::from("primary");
    let mut account = sample_source();
    account.full_name = Some("Casey Jones".to_string());
    account.email_patterns = vec!["casey@stalwart.example".to_string()];
    let config = Arc::new(TestConfig {
        sources: vec![account],
        ..Default::default()
    });
    let service = MailService::new(Arc::new(TestStore::default()), config);
    // MutationGateway::fetch_identity always errors, standing in for an empty
    // provider identity list.
    let gateway = MutationGateway::with_revision(1);

    let identity = service
        .fetch_identity(&account_id, &gateway)
        .await
        .expect("config fallback identity");
    assert_eq!(identity.email, "casey@stalwart.example");
    assert_eq!(identity.name, "Casey Jones");
}

#[tokio::test]
async fn fetch_identity_overrides_provider_name_with_configured_full_name() {
    // The provider identity succeeds but reports the bare username as the
    // name; the account's `full_name` overrides it. The server `id` and
    // `email` are preserved so submission still references a valid provider
    // identity and delivery uses the real address.
    let account_id = AccountId::from("primary");
    let mut account = sample_source();
    account.full_name = Some("Theo Ryzhenkov".to_string());
    let config = Arc::new(TestConfig {
        sources: vec![account],
        ..Default::default()
    });
    let service = MailService::new(Arc::new(TestStore::default()), config);
    let gateway = MutationGateway::with_identity(Identity {
        id: "theor-identity".to_string(),
        name: "theor".to_string(),
        email: "theor@example.com".to_string(),
    });

    let identity = service
        .fetch_identity(&account_id, &gateway)
        .await
        .expect("overridden identity");
    assert_eq!(identity.name, "Theo Ryzhenkov");
    assert_eq!(identity.id, "theor-identity");
    assert_eq!(identity.email, "theor@example.com");
}

#[tokio::test]
async fn fetch_identity_keeps_provider_name_when_full_name_unset() {
    // No `full_name` configured: the provider identity wins unchanged.
    let account_id = AccountId::from("primary");
    let config = Arc::new(TestConfig {
        sources: vec![sample_source()],
        ..Default::default()
    });
    let service = MailService::new(Arc::new(TestStore::default()), config);
    let gateway = MutationGateway::with_identity(Identity {
        id: "theor-identity".to_string(),
        name: "theor".to_string(),
        email: "theor@example.com".to_string(),
    });

    let identity = service
        .fetch_identity(&account_id, &gateway)
        .await
        .expect("provider identity");
    assert_eq!(identity.name, "theor");
}

#[tokio::test]
async fn fetch_identity_propagates_error_without_a_configured_address() {
    // No configured address to fall back to: surface the gateway error.
    let account_id = AccountId::from("primary");
    let config = Arc::new(TestConfig {
        sources: vec![sample_source()],
        ..Default::default()
    });
    let service = MailService::new(Arc::new(TestStore::default()), config);
    let gateway = MutationGateway::with_revision(1);

    assert!(service.fetch_identity(&account_id, &gateway).await.is_err());
}
