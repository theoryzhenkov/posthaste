use jmap_core::{Email, Mailbox, Thread};

pub fn generate_mailboxes() -> Vec<Mailbox> {
    vec![
        Mailbox {
            id: "mb-inbox".into(),
            name: "Inbox".into(),
            parent_id: None,
            role: Some("inbox".into()),
            sort_order: 1,
            total_emails: 15,
            unread_emails: 3,
        },
        Mailbox {
            id: "mb-sent".into(),
            name: "Sent".into(),
            parent_id: None,
            role: Some("sent".into()),
            sort_order: 2,
            total_emails: 8,
            unread_emails: 0,
        },
        Mailbox {
            id: "mb-drafts".into(),
            name: "Drafts".into(),
            parent_id: None,
            role: Some("drafts".into()),
            sort_order: 3,
            total_emails: 1,
            unread_emails: 1,
        },
        Mailbox {
            id: "mb-trash".into(),
            name: "Trash".into(),
            parent_id: None,
            role: Some("trash".into()),
            sort_order: 4,
            total_emails: 2,
            unread_emails: 0,
        },
    ]
}

pub fn generate_emails() -> Vec<Email> {
    vec![
        // --- Thread: Q2 Planning (3 messages) ---
        Email {
            id: "em-001".into(),
            thread_id: "th-q2plan".into(),
            blob_id: "blob-001".into(),
            subject: Some("Q2 Planning — priorities and timeline".into()),
            from_name: Some("Alice Chen".into()),
            from_email: Some("alice.chen@acmecorp.com".into()),
            preview: Some(
                "Hi team, I've drafted the Q2 roadmap. Key areas: \
                 infrastructure migration, SDK v2 launch, and the new onboarding flow..."
                    .into(),
            ),
            received_at: 1743177600, // 2026-03-28 12:00 UTC
            has_attachment: true,
            size: 48_210,
            mailbox_ids: vec!["mb-inbox".into()],
            keywords: vec!["$seen".into(), "$flagged".into()],
        },
        Email {
            id: "em-002".into(),
            thread_id: "th-q2plan".into(),
            blob_id: "blob-002".into(),
            subject: Some("Re: Q2 Planning — priorities and timeline".into()),
            from_name: Some("Marcus Johnson".into()),
            from_email: Some("marcus.j@acmecorp.com".into()),
            preview: Some(
                "Looks great, Alice. One concern — can we realistically \
                 ship the SDK and onboarding in the same quarter?"
                    .into(),
            ),
            received_at: 1743184800, // 2026-03-28 14:00 UTC
            has_attachment: false,
            size: 3_840,
            mailbox_ids: vec!["mb-inbox".into()],
            keywords: vec!["$seen".into()],
        },
        Email {
            id: "em-003".into(),
            thread_id: "th-q2plan".into(),
            blob_id: "blob-003".into(),
            subject: Some("Re: Q2 Planning — priorities and timeline".into()),
            from_name: Some("Alice Chen".into()),
            from_email: Some("alice.chen@acmecorp.com".into()),
            preview: Some(
                "Good point. Let me pull up the capacity estimates — \
                 I think we can stagger the launches if we bring in one more engineer."
                    .into(),
            ),
            received_at: 1743192000, // 2026-03-28 16:00 UTC
            has_attachment: false,
            size: 4_120,
            mailbox_ids: vec!["mb-inbox".into()],
            keywords: vec![], // unread
        },
        // --- Thread: Code Review (2 messages) ---
        Email {
            id: "em-004".into(),
            thread_id: "th-codereview".into(),
            blob_id: "blob-004".into(),
            subject: Some("Code review: auth token rotation PR #347".into()),
            from_name: Some("Priya Sharma".into()),
            from_email: Some("priya@devstream.io".into()),
            preview: Some(
                "Hey, could you take a look at the token rotation changes? \
                 Main concern is the race condition in the refresh path."
                    .into(),
            ),
            received_at: 1743105600, // 2026-03-27 16:00 UTC
            has_attachment: false,
            size: 6_730,
            mailbox_ids: vec!["mb-inbox".into()],
            keywords: vec!["$seen".into()],
        },
        Email {
            id: "em-005".into(),
            thread_id: "th-codereview".into(),
            blob_id: "blob-005".into(),
            subject: Some("Re: Code review: auth token rotation PR #347".into()),
            from_name: Some("Priya Sharma".into()),
            from_email: Some("priya@devstream.io".into()),
            preview: Some(
                "Updated the PR — added a mutex around the refresh call \
                 and wrote a regression test. Let me know if that addresses your comments."
                    .into(),
            ),
            received_at: 1743156000, // 2026-03-28 06:00 UTC
            has_attachment: false,
            size: 5_210,
            mailbox_ids: vec!["mb-inbox".into()],
            keywords: vec![], // unread
        },
        // --- Single-message threads ---
        Email {
            id: "em-006".into(),
            thread_id: "th-006".into(),
            blob_id: "blob-006".into(),
            subject: Some("Invoice #2026-0312 from Cloudflare".into()),
            from_name: Some("Cloudflare Billing".into()),
            from_email: Some("billing@cloudflare.com".into()),
            preview: Some(
                "Your invoice for March 2026 is ready. \
                 Amount due: $127.40. Payment is due by April 15, 2026."
                    .into(),
            ),
            received_at: 1743004800, // 2026-03-26 12:00 UTC
            has_attachment: true,
            size: 52_480,
            mailbox_ids: vec!["mb-inbox".into()],
            keywords: vec!["$seen".into()],
        },
        Email {
            id: "em-007".into(),
            thread_id: "th-007".into(),
            blob_id: "blob-007".into(),
            subject: Some("Lunch Tuesday?".into()),
            from_name: Some("Elena Rossi".into()),
            from_email: Some("elena.rossi@gmail.com".into()),
            preview: Some(
                "Are you free for lunch on Tuesday? There's a new ramen \
                 place on Valencia I've been wanting to try."
                    .into(),
            ),
            received_at: 1742918400, // 2026-03-25 12:00 UTC
            has_attachment: false,
            size: 1_820,
            mailbox_ids: vec!["mb-inbox".into()],
            keywords: vec!["$seen".into()],
        },
        Email {
            id: "em-008".into(),
            thread_id: "th-008".into(),
            blob_id: "blob-008".into(),
            subject: Some("Your GitHub Actions usage this month".into()),
            from_name: Some("GitHub".into()),
            from_email: Some("noreply@github.com".into()),
            preview: Some(
                "You've used 1,842 of 3,000 included minutes this billing \
                 cycle. Consider upgrading to Team for unlimited minutes."
                    .into(),
            ),
            received_at: 1742832000, // 2026-03-24 12:00 UTC
            has_attachment: false,
            size: 8_120,
            mailbox_ids: vec!["mb-inbox".into()],
            keywords: vec!["$seen".into()],
        },
        Email {
            id: "em-009".into(),
            thread_id: "th-009".into(),
            blob_id: "blob-009".into(),
            subject: Some("Conference talk accepted — RustConf 2026".into()),
            from_name: Some("RustConf Program Committee".into()),
            from_email: Some("program@rustconf.com".into()),
            preview: Some(
                "Congratulations! Your talk \"Bridging Rust and Swift with UniFFI\" \
                 has been accepted for RustConf 2026. Please confirm by April 5."
                    .into(),
            ),
            received_at: 1742745600, // 2026-03-23 12:00 UTC
            has_attachment: false,
            size: 12_640,
            mailbox_ids: vec!["mb-inbox".into()],
            keywords: vec!["$seen".into(), "$flagged".into()],
        },
        Email {
            id: "em-010".into(),
            thread_id: "th-010".into(),
            blob_id: "blob-010".into(),
            subject: Some("Security alert: new sign-in from Safari on macOS".into()),
            from_name: Some("Google".into()),
            from_email: Some("no-reply@accounts.google.com".into()),
            preview: Some(
                "We noticed a new sign-in to your Google Account on a Mac. \
                 If this was you, you don't need to do anything."
                    .into(),
            ),
            received_at: 1742659200, // 2026-03-22 12:00 UTC
            has_attachment: false,
            size: 9_340,
            mailbox_ids: vec!["mb-inbox".into()],
            keywords: vec!["$seen".into()],
        },
        Email {
            id: "em-011".into(),
            thread_id: "th-011".into(),
            blob_id: "blob-011".into(),
            subject: Some("Weekly digest: Hacker News top stories".into()),
            from_name: Some("HN Digest".into()),
            from_email: Some("digest@hndigest.com".into()),
            preview: Some(
                "This week: SQLite as a document database, \
                 why Rust compile times are improving, and a deep dive into io_uring."
                    .into(),
            ),
            received_at: 1742572800, // 2026-03-21 12:00 UTC
            has_attachment: false,
            size: 22_150,
            mailbox_ids: vec!["mb-inbox".into()],
            keywords: vec!["$seen".into()],
        },
        Email {
            id: "em-012".into(),
            thread_id: "th-012".into(),
            blob_id: "blob-012".into(),
            subject: Some("Design mockups for mail client".into()),
            from_name: Some("Tomoko Nakamura".into()),
            from_email: Some("tomoko@designstudio.co".into()),
            preview: Some(
                "Attached are the latest Figma exports for the sidebar \
                 and message list views. Let me know what you think of the spacing."
                    .into(),
            ),
            received_at: 1742486400, // 2026-03-20 12:00 UTC
            has_attachment: true,
            size: 156_800,
            mailbox_ids: vec!["mb-inbox".into()],
            keywords: vec!["$seen".into(), "$flagged".into()],
        },
        Email {
            id: "em-013".into(),
            thread_id: "th-013".into(),
            blob_id: "blob-013".into(),
            subject: Some("Reminder: dentist appointment March 31".into()),
            from_name: Some("Dr. Park's Office".into()),
            from_email: Some("appointments@parkdental.com".into()),
            preview: Some(
                "This is a reminder that you have an appointment \
                 on March 31, 2026 at 2:00 PM. Reply CONFIRM to confirm."
                    .into(),
            ),
            received_at: 1742400000, // 2026-03-19 12:00 UTC
            has_attachment: false,
            size: 2_640,
            mailbox_ids: vec!["mb-inbox".into()],
            keywords: vec![], // unread
        },
        Email {
            id: "em-014".into(),
            thread_id: "th-014".into(),
            blob_id: "blob-014".into(),
            subject: Some("Package shipped — order #AMZ-9847231".into()),
            from_name: Some("Amazon".into()),
            from_email: Some("ship-confirm@amazon.com".into()),
            preview: Some(
                "Your package with Keychron K2 keyboard is on its way. \
                 Estimated delivery: March 22, 2026."
                    .into(),
            ),
            received_at: 1742313600, // 2026-03-18 12:00 UTC
            has_attachment: false,
            size: 14_200,
            mailbox_ids: vec!["mb-inbox".into()],
            keywords: vec!["$seen".into()],
        },
        Email {
            id: "em-015".into(),
            thread_id: "th-015".into(),
            blob_id: "blob-015".into(),
            subject: Some("Quick question about the JMAP spec".into()),
            from_name: Some("David Kim".into()),
            from_email: Some("dkim@fastmail.com".into()),
            preview: Some(
                "Hey, section 5.5 of RFC 8621 mentions server-side sorting \
                 but the Comparator object isn't super clear. Have you run into this?"
                    .into(),
            ),
            received_at: 1742227200, // 2026-03-17 12:00 UTC
            has_attachment: false,
            size: 3_450,
            mailbox_ids: vec!["mb-inbox".into()],
            keywords: vec!["$seen".into()],
        },
    ]
}

pub fn generate_threads() -> Vec<Thread> {
    vec![
        Thread {
            id: "th-q2plan".into(),
            email_ids: vec!["em-001".into(), "em-002".into(), "em-003".into()],
        },
        Thread {
            id: "th-codereview".into(),
            email_ids: vec!["em-004".into(), "em-005".into()],
        },
        // Single-message threads
        Thread {
            id: "th-006".into(),
            email_ids: vec!["em-006".into()],
        },
        Thread {
            id: "th-007".into(),
            email_ids: vec!["em-007".into()],
        },
        Thread {
            id: "th-008".into(),
            email_ids: vec!["em-008".into()],
        },
        Thread {
            id: "th-009".into(),
            email_ids: vec!["em-009".into()],
        },
        Thread {
            id: "th-010".into(),
            email_ids: vec!["em-010".into()],
        },
        Thread {
            id: "th-011".into(),
            email_ids: vec!["em-011".into()],
        },
        Thread {
            id: "th-012".into(),
            email_ids: vec!["em-012".into()],
        },
        Thread {
            id: "th-013".into(),
            email_ids: vec!["em-013".into()],
        },
        Thread {
            id: "th-014".into(),
            email_ids: vec!["em-014".into()],
        },
        Thread {
            id: "th-015".into(),
            email_ids: vec!["em-015".into()],
        },
    ]
}
