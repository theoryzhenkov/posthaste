use super::*;

#[test]
fn profile_from_hint_groups_protocol_policies_under_provider_kind() {
    let profile = ProviderProfile::from_hint(&ProviderHint::Gmail);

    assert_eq!(profile.kind(), ProviderKind::Gmail);
    assert_eq!(
        profile.jmap().remote_observation().idle_scope(),
        RemoteIdleScope::Account
    );
    assert_eq!(
        profile.imap().required_full_sync_reason(),
        Some(ImapFullSyncReason::ProviderCanonicalizationRequired)
    );
    assert_eq!(
        profile.smtp().sent_copy(),
        SmtpSentCopyPolicy::ProviderManaged
    );
    assert!(profile
        .imap()
        .remote_observation()
        .treats_hints_as_incomplete());
}

#[test]
fn profile_from_imap_capabilities_detects_gmail_policy() {
    let profile = ProviderProfile::from_imap_capabilities(&ImapCapabilities::from_tokens([
        "IMAP4rev1",
        "X-GM-EXT-1",
    ]));

    assert_eq!(profile.kind(), ProviderKind::Gmail);
    assert_eq!(
        profile.imap().features().message_identity,
        ImapMessageIdentitySource::GmailMessageId
    );
}

#[test]
fn generic_profile_uses_conservative_default_policies() {
    let profile = ProviderProfile::from_hint(&ProviderHint::Generic);

    assert_eq!(profile.kind(), ProviderKind::Generic);
    assert!(profile.imap().allows_status_skip());
    assert!(!profile.imap().canonicalizes_by_rfc5322_message_id());
    assert_eq!(
        profile.imap().remote_observation().idle_scope(),
        RemoteIdleScope::SelectedMailbox
    );
    assert!(profile.imap().remote_observation().observes_empty_hints());
    assert!(!profile
        .imap()
        .remote_observation()
        .treats_hints_as_incomplete());
    assert_eq!(
        profile.smtp().sent_copy(),
        SmtpSentCopyPolicy::AppendToSentMailbox
    );
}

#[test]
fn remote_observation_policy_keeps_jmap_and_gmail_push_semantics_distinct() {
    let jmap = ProviderProfile::from_hint(&ProviderHint::Generic)
        .jmap()
        .remote_observation();
    let gmail_imap = ProviderProfile::from_hint(&ProviderHint::Gmail)
        .imap()
        .remote_observation();

    assert_eq!(jmap.idle_scope(), RemoteIdleScope::Account);
    assert!(!jmap.observes_empty_hints());
    assert!(!jmap.treats_hints_as_incomplete());
    assert_eq!(gmail_imap.idle_scope(), RemoteIdleScope::SelectedMailbox);
    assert!(gmail_imap.observes_empty_hints());
    assert!(gmail_imap.treats_hints_as_incomplete());
}

#[test]
fn imap_mailbox_role_aliases_are_provider_policy() {
    let generic = ProviderProfile::from_hint(&ProviderHint::Generic).imap();
    let gmail = ProviderProfile::from_hint(&ProviderHint::Gmail).imap();

    assert_eq!(
        generic.mailbox_role("[Gmail]/All Mail", ["\\All", "\\HasNoChildren"]),
        None
    );
    assert_eq!(
        generic.mailbox_role("[Gmail]/Spam", ["\\Spam", "\\HasNoChildren"]),
        None
    );
    assert_eq!(
        gmail.mailbox_role("[Gmail]/All Mail", ["\\All", "\\HasNoChildren"]),
        Some(MailboxRole::Archive.as_str())
    );
    assert_eq!(
        gmail.mailbox_role("[Gmail]/Spam", ["\\Spam", "\\HasNoChildren"]),
        Some(MailboxRole::Junk.as_str())
    );
    assert_eq!(
        gmail.mailbox_role("Sent Items", ["\\Sent"]),
        Some(MailboxRole::Sent.as_str())
    );
}

#[test]
fn imap_policy_exposes_gmail_canonicalization_without_vendor_match() {
    let profile = ProviderProfile::from_imap_capabilities(&ImapCapabilities::from_tokens([
        "IMAP4rev1",
        "X-GM-EXT-1",
    ]));

    assert!(profile.imap().canonicalizes_by_gmail_message_id());
}

#[test]
fn account_transport_exposes_provider_profile_boundary() {
    let transport = AccountTransportSettings {
        provider: ProviderHint::Outlook,
        ..AccountTransportSettings::default()
    };

    assert_eq!(transport.provider_profile().kind(), ProviderKind::Outlook);
    assert_eq!(
        transport.provider_profile().smtp().sent_copy(),
        SmtpSentCopyPolicy::ProviderManaged
    );
}

#[test]
fn oauth_policy_is_available_only_for_supported_provider_profiles() {
    let cases = [
        (ProviderHint::Gmail, ProviderKind::Gmail, true),
        (ProviderHint::Outlook, ProviderKind::Outlook, true),
        (ProviderHint::Generic, ProviderKind::Generic, false),
        (ProviderHint::Icloud, ProviderKind::Icloud, false),
    ];

    for (hint, kind, supported) in cases {
        let profile = ProviderProfile::from_hint(&hint);

        assert_eq!(profile.kind(), kind);
        assert_eq!(profile.oauth().is_supported(), supported);
        assert_eq!(
            profile.oauth().default_mail_transport().is_some(),
            supported
        );
    }
}

#[test]
fn oauth_policy_matches_provider_issuer_rules() {
    let gmail = ProviderProfile::from_kind(ProviderKind::Gmail).oauth();
    let outlook = ProviderProfile::from_kind(ProviderKind::Outlook).oauth();
    let generic = ProviderProfile::from_kind(ProviderKind::Generic).oauth();

    assert!(gmail.openid_issuer_matches("https://accounts.google.com"));
    assert!(gmail.openid_issuer_matches("accounts.google.com"));
    assert!(!gmail.openid_issuer_matches("https://login.microsoftonline.com/tenant/v2.0"));
    assert!(outlook.openid_issuer_matches("https://login.microsoftonline.com/tenant/v2.0"));
    assert!(!outlook.openid_issuer_matches("https://accounts.google.com"));
    assert!(!generic.openid_issuer_matches("https://accounts.google.com"));
}

#[test]
fn oauth_policy_provides_default_mail_endpoints() {
    let gmail = ProviderProfile::from_kind(ProviderKind::Gmail)
        .oauth()
        .default_mail_transport()
        .expect("Gmail OAuth mail transport");
    let outlook = ProviderProfile::from_kind(ProviderKind::Outlook)
        .oauth()
        .default_mail_transport()
        .expect("Outlook OAuth mail transport");

    assert_eq!(gmail.0.host, "imap.gmail.com");
    assert_eq!(gmail.0.security, TransportSecurity::Tls);
    assert_eq!(gmail.1.host, "smtp.gmail.com");
    assert_eq!(gmail.1.security, TransportSecurity::StartTls);
    assert_eq!(outlook.0.host, "outlook.office365.com");
    assert_eq!(outlook.1.host, "smtp.office365.com");
}
