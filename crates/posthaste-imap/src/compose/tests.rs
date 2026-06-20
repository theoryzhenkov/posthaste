use super::*;

#[test]
fn builds_reply_context_from_raw_mime() {
    let raw = concat!(
        "From: Alice <alice@example.test>\r\n",
        "Cc: Bob <bob@example.test>, carol@example.test\r\n",
        "Subject: Hello\r\n",
        "Message-ID: <m1@example.test>\r\n",
        "References: <root@example.test> <parent@example.test>\r\n",
        "Content-Type: text/plain; charset=utf-8\r\n",
        "\r\n",
        "Line one\r\n",
        "Line two\r\n",
    )
    .as_bytes()
    .to_vec();

    let context = imap_reply_context_from_raw_mime(raw).expect("context");

    assert_eq!(context.to.len(), 1);
    assert_eq!(context.to[0].name.as_deref(), Some("Alice"));
    assert_eq!(context.to[0].email, "alice@example.test");
    assert_eq!(context.cc.len(), 2);
    assert_eq!(context.cc[0].email, "bob@example.test");
    assert_eq!(context.cc[1].email, "carol@example.test");
    assert_eq!(context.reply_subject, "Re: Hello");
    assert_eq!(context.forward_subject, "Fwd: Hello");
    assert_eq!(
        context.quoted_body.as_deref(),
        Some("> Line one\n> Line two")
    );
    assert_eq!(context.in_reply_to.as_deref(), Some("m1@example.test"));
    assert_eq!(
        context.references.as_deref(),
        Some("root@example.test parent@example.test")
    );
    let forwarded = context.forwarded_body.expect("forwarded body");
    assert!(forwarded.starts_with("---------- Forwarded message ----------\n"));
    assert!(forwarded.contains("From: Alice <alice@example.test>"));
    assert!(forwarded.contains("Subject: Hello"));
    // Original body is included unquoted in the forwarded block.
    assert!(forwarded.contains("Line one"));
    assert!(forwarded.contains("Line two"));
    assert!(!forwarded.contains("> Line one"));
}

#[test]
fn does_not_duplicate_reply_or_forward_subject_prefixes() {
    let raw = concat!(
        "From: Alice <alice@example.test>\r\n",
        "Subject: Re: Already replied\r\n",
        "Content-Type: text/plain; charset=utf-8\r\n",
        "\r\n",
        "Body\r\n",
    )
    .as_bytes()
    .to_vec();

    let context = imap_reply_context_from_raw_mime(raw).expect("context");

    assert_eq!(context.reply_subject, "Re: Already replied");
    assert_eq!(context.forward_subject, "Fwd: Re: Already replied");
}
