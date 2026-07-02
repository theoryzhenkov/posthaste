use super::*;

#[test]
fn gmail_profile_uses_imap_smtp_mail_scope_and_offline_access() {
    let profile = OAuthProviderProfile::for_provider(&ProviderHint::Gmail).expect("profile");

    assert_eq!(
        profile.scopes,
        &["openid", "email", "https://mail.google.com/"]
    );
    assert!(profile
        .extra_authorization_params
        .contains(&("access_type", "offline")));
    let (imap, smtp) = profile
        .default_mail_transport()
        .expect("Gmail profile should include mail endpoints");
    assert_eq!(imap.host, "imap.gmail.com");
    assert_eq!(imap.security, TransportSecurity::Tls);
    assert_eq!(smtp.host, "smtp.gmail.com");
    assert_eq!(smtp.security, TransportSecurity::StartTls);
}

#[test]
fn outlook_profile_uses_imap_smtp_and_refresh_scopes() {
    let profile = OAuthProviderProfile::for_provider(&ProviderHint::Outlook).expect("profile");

    assert!(profile.scopes.contains(&"offline_access"));
    assert!(profile
        .scopes
        .contains(&"https://outlook.office.com/IMAP.AccessAsUser.All"));
    assert!(profile
        .scopes
        .contains(&"https://outlook.office.com/SMTP.Send"));
    let (imap, smtp) = profile
        .default_mail_transport()
        .expect("Outlook profile should include mail endpoints");
    assert_eq!(imap.host, "outlook.office365.com");
    assert_eq!(smtp.host, "smtp.office365.com");
}

#[test]
fn provider_profile_matches_openid_issuer() {
    let gmail = OAuthProviderProfile::for_provider(&ProviderHint::Gmail).expect("profile");
    let outlook = OAuthProviderProfile::for_provider(&ProviderHint::Outlook).expect("profile");

    assert!(gmail.openid_issuer_matches("https://accounts.google.com"));
    assert!(gmail.openid_issuer_matches("accounts.google.com"));
    assert!(outlook.openid_issuer_matches("https://login.microsoftonline.com/tenant-id/v2.0"));
    assert!(!outlook.openid_issuer_matches("https://accounts.google.com"));
}

#[test]
fn oauth_profile_availability_follows_provider_profile_policy() {
    for provider in [
        ProviderHint::Gmail,
        ProviderHint::Outlook,
        ProviderHint::Generic,
        ProviderHint::Icloud,
    ] {
        let domain_policy = ProviderProfile::from_hint(&provider).oauth();

        assert_eq!(
            OAuthProviderProfile::for_provider(&provider).is_some(),
            domain_policy.is_supported()
        );
    }
}

#[test]
fn oauth_profile_mail_transport_matches_provider_profile_policy() {
    for provider in [ProviderHint::Gmail, ProviderHint::Outlook] {
        let oauth_profile = OAuthProviderProfile::for_provider(&provider).expect("profile");
        let domain_transport = ProviderProfile::from_hint(&provider)
            .oauth()
            .default_mail_transport();

        assert_eq!(oauth_profile.default_mail_transport(), domain_transport);
    }
}

#[test]
fn oauth_profile_issuer_matching_matches_provider_profile_policy() {
    let issuers = [
        "https://accounts.google.com",
        "accounts.google.com",
        "https://login.microsoftonline.com/tenant-id/v2.0",
        "https://example.com",
    ];

    for provider in [ProviderHint::Gmail, ProviderHint::Outlook] {
        let oauth_profile = OAuthProviderProfile::for_provider(&provider).expect("profile");
        let domain_policy = ProviderProfile::from_hint(&provider).oauth();

        for issuer in issuers {
            assert_eq!(
                oauth_profile.openid_issuer_matches(issuer),
                domain_policy.openid_issuer_matches(issuer)
            );
        }
    }
}
