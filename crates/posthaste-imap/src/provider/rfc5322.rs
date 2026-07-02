use std::collections::BTreeMap;

use posthaste_domain_model::MessageRecord;
use posthaste_domain_model::MessageId;

use crate::ImapMappedHeader;

use super::group::CanonicalMessageGroup;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct Rfc5322CanonicalMessageProfile;

impl Rfc5322CanonicalMessageProfile {
    pub(super) fn project_headers(&self, headers: Vec<ImapMappedHeader>) -> Vec<ImapMappedHeader> {
        self.project_headers_with_canonicalizer(headers, |message| {
            self.canonical_message_id(message)
        })
    }

    fn project_headers_with_canonicalizer(
        &self,
        headers: Vec<ImapMappedHeader>,
        canonical_message_id: impl Fn(&MessageRecord) -> MessageId,
    ) -> Vec<ImapMappedHeader> {
        let mut groups = BTreeMap::<MessageId, CanonicalMessageGroup>::new();

        for mut header in headers {
            let canonical_id = canonical_message_id(&header.message);
            header.message.id = canonical_id.clone();
            header.location.message_id = canonical_id.clone();

            groups
                .entry(canonical_id)
                .or_insert_with(|| CanonicalMessageGroup::new(header.message.clone()))
                .push(header);
        }

        groups
            .into_values()
            .flat_map(CanonicalMessageGroup::into_headers)
            .collect()
    }

    pub(super) fn canonical_message_id(&self, message: &MessageRecord) -> MessageId {
        message
            .rfc_message_id
            .as_deref()
            .filter(|message_id| !message_id.is_empty())
            .map(|message_id| {
                MessageId(format!(
                    "imap:gmail:rfc822msgid:{}",
                    hex::encode(message_id.as_bytes())
                ))
            })
            .unwrap_or_else(|| message.id.clone())
    }
}
