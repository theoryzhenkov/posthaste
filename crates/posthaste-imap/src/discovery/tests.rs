use super::*;

#[test]
fn normalizes_capability_tokens_case_insensitively() {
    let capabilities = normalize_imap_capabilities(["imap4rev1", "idle", "x-gm-ext-1", "uidplus"]);

    assert!(capabilities.supports_idle());
    assert!(capabilities.supports_uidplus());
    assert!(capabilities.supports_gmail_extensions());
}

#[test]
fn maps_special_use_mailbox_roles() {
    let mailbox = map_imap_mailbox("Sent Items", ["\\HasNoChildren", "\\Sent"]);

    assert_eq!(
        mailbox.id,
        MailboxId::from("imap:mailbox:53656e74204974656d73")
    );
    assert_eq!(mailbox.role, Some(MailboxRole::Sent.as_str()));
    assert!(mailbox.selectable);
}

#[test]
fn maps_noselect_mailboxes_without_role_loss() {
    let mailbox = map_imap_mailbox("[Gmail]", ["\\Noselect"]);

    assert_eq!(mailbox.role, None);
    assert!(!mailbox.selectable);
}

#[test]
fn maps_gmail_role_aliases_only_with_gmail_provider_policy() {
    let generic = map_imap_mailbox("[Gmail]/All Mail", ["\\All", "\\HasNoChildren"]);
    let gmail = map_imap_mailbox_with_provider(
        ProviderProfile::from_kind(posthaste_domain::ProviderKind::Gmail),
        "[Gmail]/All Mail",
        ["\\All", "\\HasNoChildren"],
    );

    assert_eq!(generic.role, None);
    assert_eq!(gmail.role, Some(MailboxRole::Archive.as_str()));
}
