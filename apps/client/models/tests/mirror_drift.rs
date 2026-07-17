//! Guards the mirror module against drift from the domain model.
//!
//! Every reused domain type is serialized fully populated and strictly
//! decoded into its `deny_unknown_fields` mirror twin. A field the domain
//! adds shows up as an unknown key (and the exhaustive struct literals below
//! stop compiling); a field the mirror carries but the domain stopped
//! serializing fails the strict decode.

use posthaste_client_models::mirror;
use posthaste_domain_model as domain;
use serde::de::DeserializeOwned;
use serde_json::json;

fn assert_mirrors<M: DeserializeOwned + std::fmt::Debug>(
    name: &str,
    value: &impl serde::Serialize,
) {
    let json = serde_json::to_value(value).unwrap_or_else(|error| panic!("{name}: {error}"));
    if let Err(error) = serde_json::from_value::<M>(json.clone()) {
        panic!("mirror::{name} drifted from the domain type: {error}\nserialized: {json}");
    }
}

fn recipient() -> domain::Recipient {
    domain::Recipient {
        name: Some("Ada".into()),
        email: "ada@example.com".into(),
    }
}

fn message_summary() -> domain::MessageSummary {
    domain::MessageSummary {
        id: "m1".into(),
        source_id: "a1".into(),
        source_name: "Work".into(),
        source_thread_id: "t1".into(),
        conversation_id: "c1".into(),
        subject: Some("Subject".into()),
        from_name: Some("Ada".into()),
        from_email: Some("ada@example.com".into()),
        to: vec![recipient()],
        preview: Some("preview".into()),
        received_at: "2026-01-01T00:00:00Z".into(),
        has_attachment: true,
        is_read: false,
        is_flagged: true,
        mailbox_ids: vec!["inbox".into()],
        keywords: vec!["$flagged".into()],
        version: Some(7),
        rfc_message_id: Some("<x@example.com>".into()),
        in_reply_to: Some("<y@example.com>".into()),
        draft_id: Some("d1".into()),
    }
}

#[test]
fn structs_decode_strictly_from_domain_serialization() {
    assert_mirrors::<mirror::Recipient>("Recipient", &recipient());
    assert_mirrors::<mirror::MessageSummary>("MessageSummary", &message_summary());
    assert_mirrors::<mirror::MailboxSummary>(
        "MailboxSummary",
        &domain::MailboxSummary {
            id: "inbox".into(),
            name: "Inbox".into(),
            role: Some("inbox".into()),
            unread_emails: 3,
            total_emails: 10,
        },
    );
    assert_mirrors::<mirror::MessageAttachment>(
        "MessageAttachment",
        &domain::MessageAttachment {
            id: "att1".into(),
            blob_id: "b1".into(),
            part_id: Some("2".into()),
            filename: Some("file.pdf".into()),
            mime_type: "application/pdf".into(),
            size: 1024,
            disposition: Some("attachment".into()),
            cid: Some("cid1".into()),
            is_inline: false,
        },
    );
    assert_mirrors::<mirror::ListUnsubscribe>(
        "ListUnsubscribe",
        &domain::ListUnsubscribe {
            https: Some("https://example.com/u".into()),
            mailto: Some("mailto:u@example.com".into()),
            one_click: true,
        },
    );
    assert_mirrors::<mirror::ThreadView>(
        "ThreadView",
        &domain::ThreadView {
            id: "t1".into(),
            messages: vec![message_summary()],
        },
    );
    assert_mirrors::<mirror::SetKeywordsCommand>(
        "SetKeywordsCommand",
        &domain::SetKeywordsCommand {
            add: vec!["$seen".into()],
            remove: vec!["$flagged".into()],
        },
    );
    assert_mirrors::<mirror::ReplaceMailboxesCommand>(
        "ReplaceMailboxesCommand",
        &domain::ReplaceMailboxesCommand {
            mailbox_ids: vec!["archive".into()],
        },
    );
    assert_mirrors::<mirror::SendMessageRequest>(
        "SendMessageRequest",
        &domain::SendMessageRequest {
            from: Some(recipient()),
            to: vec![recipient()],
            cc: vec![recipient()],
            bcc: vec![recipient()],
            subject: "Subject".into(),
            body: "Body".into(),
            in_reply_to: Some("<x@example.com>".into()),
            references: Some("<x@example.com>".into()),
            attachments: vec![domain::SendMessageAttachment {
                filename: "file.txt".into(),
                mime_type: "text/plain".into(),
                content_base64: "aGk=".into(),
            }],
            draft_id: Some("d1".into()),
            send_at: Some("2026-01-01T00:00:00Z".into()),
            undo_window_seconds: Some(30),
        },
    );
}

#[test]
fn ids_decode_as_plain_strings() {
    assert_mirrors::<mirror::AccountId>("AccountId", &domain::AccountId::from("a1"));
    assert_mirrors::<mirror::MailboxId>("MailboxId", &domain::MailboxId::from("mb1"));
    assert_mirrors::<mirror::MessageId>("MessageId", &domain::MessageId::from("m1"));
    assert_mirrors::<mirror::ThreadId>("ThreadId", &domain::ThreadId::from("t1"));
    assert_mirrors::<mirror::ConversationId>("ConversationId", &domain::ConversationId::from("c1"));
    assert_mirrors::<mirror::BlobId>("BlobId", &domain::BlobId::from("b1"));
    assert_mirrors::<mirror::OperationId>("OperationId", &domain::OperationId::from("op1"));
}

#[test]
fn enums_decode_every_domain_variant() {
    // Exhaustive matches: a new domain variant fails compilation here, which
    // is the cue to extend both the mirror and these lists.
    fn covered_sort(value: domain::MessageSortField) {
        use domain::MessageSortField as F;
        match value {
            F::Date | F::From | F::Subject | F::Source | F::Flagged | F::Attachment => {}
        }
    }
    fn covered_kind(value: domain::OperationKind) {
        use domain::OperationKind as K;
        match value {
            K::SetKeywords
            | K::ReplaceMailboxes
            | K::Destroy
            | K::DraftCreate
            | K::DraftUpdate
            | K::DraftDelete
            | K::Send => {}
        }
    }
    fn covered_state(value: domain::OperationState) {
        use domain::OperationState as S;
        match value {
            S::Pending | S::Inflight | S::Applied | S::Failed | S::DispatchUncertain => {}
        }
    }
    fn covered_entity(value: domain::OperationEntityKind) {
        use domain::OperationEntityKind as E;
        match value {
            E::Message | E::Draft => {}
        }
    }
    fn covered_status(value: domain::AccountStatus) {
        use domain::AccountStatus as A;
        match value {
            A::Ready | A::Syncing | A::Degraded | A::AuthError | A::Offline | A::Disabled => {}
        }
    }
    fn covered_push(value: domain::PushStatus) {
        use domain::PushStatus as P;
        match value {
            P::Connected | P::Reconnecting | P::Unsupported | P::Disabled => {}
        }
    }
    covered_sort(domain::MessageSortField::Date);
    covered_kind(domain::OperationKind::Send);
    covered_state(domain::OperationState::Pending);
    covered_entity(domain::OperationEntityKind::Message);
    covered_status(domain::AccountStatus::Ready);
    covered_push(domain::PushStatus::Connected);

    for field in [
        domain::MessageSortField::Date,
        domain::MessageSortField::From,
        domain::MessageSortField::Subject,
        domain::MessageSortField::Source,
        domain::MessageSortField::Flagged,
        domain::MessageSortField::Attachment,
    ] {
        assert_mirrors::<mirror::MessageSortField>("MessageSortField", &field);
    }
    for kind in [
        domain::OperationKind::SetKeywords,
        domain::OperationKind::ReplaceMailboxes,
        domain::OperationKind::Destroy,
        domain::OperationKind::DraftCreate,
        domain::OperationKind::DraftUpdate,
        domain::OperationKind::DraftDelete,
        domain::OperationKind::Send,
    ] {
        assert_mirrors::<mirror::OperationKind>("OperationKind", &kind);
    }
    for state in [
        domain::OperationState::Pending,
        domain::OperationState::Inflight,
        domain::OperationState::Applied,
        domain::OperationState::Failed,
        domain::OperationState::DispatchUncertain,
    ] {
        assert_mirrors::<mirror::OperationState>("OperationState", &state);
    }
    for entity in [
        domain::OperationEntityKind::Message,
        domain::OperationEntityKind::Draft,
    ] {
        assert_mirrors::<mirror::OperationEntityKind>("OperationEntityKind", &entity);
    }
    for status in [
        domain::AccountStatus::Ready,
        domain::AccountStatus::Syncing,
        domain::AccountStatus::Degraded,
        domain::AccountStatus::AuthError,
        domain::AccountStatus::Offline,
        domain::AccountStatus::Disabled,
    ] {
        assert_mirrors::<mirror::AccountStatus>("AccountStatus", &status);
    }
    for push in [
        domain::PushStatus::Connected,
        domain::PushStatus::Reconnecting,
        domain::PushStatus::Unsupported,
        domain::PushStatus::Disabled,
    ] {
        assert_mirrors::<mirror::PushStatus>("PushStatus", &push);
    }
}

#[test]
fn wire_envelopes_serialize_the_documented_shapes() {
    use posthaste_client_models as models;

    let query = models::Query::MailList(models::MailListQuery {
        mailbox_id: Some("inbox".into()),
        ..Default::default()
    });
    assert_eq!(
        serde_json::to_value(&query).unwrap(),
        json!({
            "mailList": {
                "accountId": null,
                "mailboxId": "inbox",
                "freeText": null,
                "isRead": null,
                "isFlagged": null,
                "hasAttachment": null,
                "sort": null,
                "limit": null,
                "cursor": null,
            }
        })
    );

    // Defaulted fields may be omitted entirely by a TS caller.
    let decoded: models::Query = serde_json::from_value(json!({ "mailList": {} })).unwrap();
    match decoded {
        models::Query::MailList(list) => assert!(list.mailbox_id.is_none()),
        other => panic!("wrong family: {other:?}"),
    }

    let command = models::CommandEnvelope {
        id: "01J".into(),
        command: models::Command::Destroy(models::DestroyMessageIntent {
            account_id: "a1".into(),
            message_id: "m1".into(),
        }),
    };
    assert_eq!(
        serde_json::to_value(&command).unwrap(),
        json!({
            "id": "01J",
            "command": { "destroy": { "accountId": "a1", "messageId": "m1" } }
        })
    );

    let heartbeat = models::EventMessage {
        generation: 4184,
        run_id: None,
        event: None,
    };
    assert_eq!(
        serde_json::to_value(&heartbeat).unwrap(),
        json!({ "generation": 4184 })
    );

    let handshake = models::EventMessage {
        generation: 4184,
        run_id: Some("run-1".into()),
        event: None,
    };
    assert_eq!(
        serde_json::to_value(&handshake).unwrap(),
        json!({ "generation": 4184, "runId": "run-1" })
    );
}
