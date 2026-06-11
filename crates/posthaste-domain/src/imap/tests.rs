use super::*;

#[test]
fn imap_message_id_includes_uid_validity_to_avoid_uid_reuse_aliasing() {
    let mailbox_id = MailboxId::from("Inbox");

    let first = imap_message_id(&mailbox_id, ImapUidValidity(1), ImapUid(42));
    let after_reset = imap_message_id(&mailbox_id, ImapUidValidity(2), ImapUid(42));

    assert_ne!(first, after_reset);
}

#[test]
fn imap_message_id_encodes_mailbox_id_without_delimiter_ambiguity() {
    let inbox = imap_message_id(&MailboxId::from("A:B"), ImapUidValidity(1), ImapUid(42));
    let other = imap_message_id(&MailboxId::from("A"), ImapUidValidity(1), ImapUid(42));

    assert_eq!(inbox.as_str(), "imap:1:42:413a42");
    assert_ne!(inbox, other);
}

#[test]
fn mailbox_sync_state_tracks_high_watermarks_monotonically() {
    let mut state = ImapMailboxSyncState::new(
        MailboxId::from("Inbox"),
        "Inbox".to_string(),
        ImapUidValidity(7),
        "2026-04-25T00:00:00Z".to_string(),
    );

    state.record_seen_uid(ImapUid(20));
    state.record_seen_uid(ImapUid(10));
    state.record_highest_modseq(ImapModSeq(300));
    state.record_highest_modseq(ImapModSeq(200));

    assert_eq!(state.highest_uid, Some(ImapUid(20)));
    assert_eq!(state.highest_modseq, Some(ImapModSeq(300)));
    assert!(state.is_valid_for(ImapUidValidity(7)));
    assert!(!state.is_valid_for(ImapUidValidity(8)));
}

fn selected_mailbox(uid_validity: ImapUidValidity) -> ImapSelectedMailbox {
    ImapSelectedMailbox {
        mailbox_id: MailboxId::from("Inbox"),
        mailbox_name: "INBOX".to_string(),
        uid_validity,
        uid_next: Some(ImapUid(43)),
        highest_modseq: Some(ImapModSeq(400)),
    }
}

fn stored_state() -> ImapMailboxSyncState {
    let mut state = ImapMailboxSyncState::new(
        MailboxId::from("Inbox"),
        "INBOX".to_string(),
        ImapUidValidity(7),
        "2026-04-25T00:00:00Z".to_string(),
    );
    state.record_seen_uid(ImapUid(42));
    state.record_highest_modseq(ImapModSeq(300));
    state
}

#[test]
fn planner_uses_qresync_when_server_and_state_support_it() {
    let capabilities = ImapCapabilities::from_tokens(["IMAP4rev1", "ENABLE", "QRESYNC"]);
    let stored = stored_state();
    let provider = ProviderProfile::from_imap_capabilities(&capabilities);

    let plan = plan_imap_mailbox_sync(
        &capabilities,
        &provider,
        Some(&stored),
        &selected_mailbox(ImapUidValidity(7)),
    );

    assert_eq!(
        plan,
        ImapMailboxSyncPlan::QresyncDelta {
            uid_validity: ImapUidValidity(7),
            since_modseq: ImapModSeq(300),
            after_uid: Some(ImapUid(42)),
        }
    );
}

#[test]
fn planner_falls_back_to_full_snapshot_after_uidvalidity_change() {
    let capabilities = ImapCapabilities::from_tokens(["ENABLE", "QRESYNC"]);
    let stored = stored_state();
    let provider = ProviderProfile::from_imap_capabilities(&capabilities);

    let plan = plan_imap_mailbox_sync(
        &capabilities,
        &provider,
        Some(&stored),
        &selected_mailbox(ImapUidValidity(8)),
    );

    assert_eq!(
        plan,
        ImapMailboxSyncPlan::FullSnapshot {
            reason: ImapFullSyncReason::UidValidityChanged,
        }
    );
}

#[test]
fn planner_uses_full_snapshot_when_flag_delta_is_unavailable() {
    let capabilities = ImapCapabilities::from_tokens(["IMAP4rev1"]);
    let stored = stored_state();
    let provider = ProviderProfile::from_imap_capabilities(&capabilities);

    let plan = plan_imap_mailbox_sync(
        &capabilities,
        &provider,
        Some(&stored),
        &selected_mailbox(ImapUidValidity(7)),
    );

    assert_eq!(
        plan,
        ImapMailboxSyncPlan::FullSnapshot {
            reason: ImapFullSyncReason::FlagDeltaUnavailable,
        }
    );
}

#[test]
fn planner_uses_full_snapshot_for_gmail_label_canonicalization() {
    let capabilities =
        ImapCapabilities::from_tokens(["IMAP4rev1", "ENABLE", "QRESYNC", "X-GM-EXT-1"]);
    let stored = stored_state();
    let provider = ProviderProfile::from_imap_capabilities(&capabilities);

    let plan = plan_imap_mailbox_sync(
        &capabilities,
        &provider,
        Some(&stored),
        &selected_mailbox(ImapUidValidity(7)),
    );

    assert_eq!(
        plan,
        ImapMailboxSyncPlan::FullSnapshot {
            reason: ImapFullSyncReason::ProviderCanonicalizationRequired,
        }
    );
}

#[test]
fn special_use_mapping_prefers_standard_attributes() {
    assert_eq!(
        imap_special_use_role("Sent Items", ["\\Sent"]),
        Some("sent")
    );
    assert_eq!(
        imap_special_use_role("INBOX", [] as [&str; 0]),
        Some("inbox")
    );
    assert_eq!(imap_special_use_role("Projects", ["\\HasNoChildren"]), None);
}

#[test]
fn special_use_mapping_keeps_provider_aliases_out_of_standard_mapping() {
    assert_eq!(
        imap_special_use_role("[Gmail]/All Mail", ["\\All", "\\HasNoChildren"]),
        None
    );
    assert_eq!(
        imap_special_use_role("[Gmail]/Spam", ["\\Spam", "\\HasNoChildren"]),
        None
    );
}

#[test]
fn move_planner_uses_uidplus_when_available() {
    assert_eq!(
        plan_imap_move(&ImapCapabilities::from_tokens(["MOVE", "UIDPLUS"])),
        ImapMoveStrategy::UidMoveWithCopyUid
    );
    assert_eq!(
        plan_imap_move(&ImapCapabilities::from_tokens(["MOVE"])),
        ImapMoveStrategy::UidMoveThenResync
    );
    assert_eq!(
        plan_imap_move(&ImapCapabilities::from_tokens(["IMAP4rev1"])),
        ImapMoveStrategy::CopyDeleteThenResync
    );
    assert_eq!(
        plan_imap_move(&ImapCapabilities::from_tokens(["IMAP4rev2"])),
        ImapMoveStrategy::UidMoveWithCopyUid
    );
}

#[test]
fn provider_features_use_gmail_extension_for_deduplication_and_threads() {
    let capabilities = ImapCapabilities::from_tokens(["IMAP4rev1", "X-GM-EXT-1"]);
    let profile = ProviderProfile::from_imap_capabilities(&capabilities);

    let features = ImapProviderFeatures::from_capabilities(&capabilities);

    assert_eq!(profile.kind(), ProviderKind::Gmail);
    assert_eq!(
        profile.imap().required_full_sync_reason(),
        Some(ImapFullSyncReason::ProviderCanonicalizationRequired)
    );
    assert!(!profile.imap().allows_status_skip());
    assert_eq!(
        features,
        ImapProviderFeatures {
            message_identity: ImapMessageIdentitySource::GmailMessageId,
            thread_identity: ImapThreadIdentitySource::GmailThreadId,
            label_source: ImapLabelSource::GmailLabels,
        }
    );
    assert_eq!(
        gmail_message_id(GmailMessageId(1278455344230334865)).as_str(),
        "imap:gmail:msgid:1278455344230334865"
    );
    assert_eq!(
        gmail_thread_id(GmailThreadId(1266894439832287888)).as_str(),
        "imap:gmail:thrid:1266894439832287888"
    );
}

#[test]
fn provider_profile_uses_generic_policy_without_gmail_extension() {
    let capabilities = ImapCapabilities::from_tokens(["IMAP4rev1", "ENABLE", "QRESYNC"]);
    let profile = ProviderProfile::from_imap_capabilities(&capabilities);

    assert_eq!(profile.kind(), ProviderKind::Generic);
    assert_eq!(profile.imap().required_full_sync_reason(), None);
    assert!(profile.imap().allows_status_skip());
    assert_eq!(
        profile.imap().features(),
        ImapProviderFeatures {
            message_identity: ImapMessageIdentitySource::UidValidityUid,
            thread_identity: ImapThreadIdentitySource::Rfc5322Headers,
            label_source: ImapLabelSource::MailboxMembership,
        }
    );
}
