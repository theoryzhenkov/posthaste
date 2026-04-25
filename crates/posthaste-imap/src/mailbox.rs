use imap_client::tasks::tasks::select::SelectDataUnvalidated;
use posthaste_domain::{ImapSelectedMailbox, ImapUid, ImapUidValidity};

use crate::discovery::connect_authenticated_client;
use crate::{imap_mailbox_id, ImapAdapterError, ImapConnectionConfig};

/// EXAMINE one IMAP mailbox and return the server state needed by the sync planner.
///
/// @spec docs/L0-providers#imap-smtp-sync-strategy
pub async fn examine_imap_mailbox(
    config: &ImapConnectionConfig,
    mailbox_name: &str,
) -> Result<ImapSelectedMailbox, ImapAdapterError> {
    let mut client = connect_authenticated_client(config).await?;
    let data = client.examine(mailbox_name).await?;
    selected_mailbox_from_examine(mailbox_name, data)
}

/// Convert an IMAP EXAMINE/SELECT response into Posthaste's selected-mailbox state.
pub fn selected_mailbox_from_examine(
    mailbox_name: &str,
    data: SelectDataUnvalidated,
) -> Result<ImapSelectedMailbox, ImapAdapterError> {
    let uid_validity = data
        .uid_validity
        .ok_or(ImapAdapterError::MissingSelectData("UIDVALIDITY"))?;
    Ok(ImapSelectedMailbox {
        mailbox_id: imap_mailbox_id(mailbox_name),
        mailbox_name: mailbox_name.to_string(),
        uid_validity: ImapUidValidity(uid_validity.get()),
        uid_next: data.uid_next.map(|uid| ImapUid(uid.get())),
        highest_modseq: None,
    })
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;

    use posthaste_domain::MailboxId;

    use super::*;

    #[test]
    fn selected_mailbox_requires_uidvalidity() {
        let error = selected_mailbox_from_examine("INBOX", SelectDataUnvalidated::default())
            .expect_err("UIDVALIDITY is required");

        assert!(matches!(
            error,
            ImapAdapterError::MissingSelectData("UIDVALIDITY")
        ));
    }

    #[test]
    fn selected_mailbox_maps_uidvalidity_and_uidnext() {
        let selected = selected_mailbox_from_examine(
            "INBOX",
            SelectDataUnvalidated {
                uid_validity: Some(NonZeroU32::new(42).expect("nonzero")),
                uid_next: Some(NonZeroU32::new(100).expect("nonzero")),
                ..Default::default()
            },
        )
        .expect("selected mailbox");

        assert_eq!(
            selected.mailbox_id,
            MailboxId::from("imap:mailbox:494e424f58")
        );
        assert_eq!(selected.uid_validity, ImapUidValidity(42));
        assert_eq!(selected.uid_next, Some(ImapUid(100)));
        assert_eq!(selected.highest_modseq, None);
    }
}
