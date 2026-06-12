use super::*;

/// Map SPECIAL-USE attributes into Posthaste's mailbox role vocabulary.
///
/// @spec docs/L0-providers#imap-smtp-sync-strategy
pub fn imap_special_use_role(
    mailbox_name: &str,
    attributes: impl IntoIterator<Item = impl AsRef<str>>,
) -> Option<&'static str> {
    let normalized = attributes
        .into_iter()
        .map(|attribute| attribute.as_ref().to_ascii_uppercase())
        .collect::<BTreeSet<_>>();

    if normalized.contains("\\INBOX") || mailbox_name.eq_ignore_ascii_case("INBOX") {
        Some("inbox")
    } else if normalized.contains("\\SENT") {
        Some("sent")
    } else if normalized.contains("\\DRAFTS") {
        Some("drafts")
    } else if normalized.contains("\\TRASH") {
        Some("trash")
    } else if normalized.contains("\\JUNK") {
        Some("junk")
    } else if normalized.contains("\\ARCHIVE") {
        Some("archive")
    } else {
        None
    }
}
