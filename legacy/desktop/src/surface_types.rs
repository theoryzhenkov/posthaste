use super::*;

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub(crate) enum SurfaceDescriptor {
    #[serde(rename = "attachment")]
    Attachment {
        disposition: SurfaceDisposition,
        params: AttachmentSurfaceParams,
    },
    #[serde(rename = "message")]
    Message {
        disposition: SurfaceDisposition,
        params: MessageSurfaceParams,
    },
    #[serde(rename = "settings")]
    Settings {
        disposition: SurfaceDisposition,
        params: SettingsSurfaceParams,
    },
    #[serde(rename = "compose")]
    Compose {
        disposition: SurfaceDisposition,
        params: ComposeSurfaceParams,
    },
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum SurfaceDisposition {
    #[serde(rename = "focused")]
    Focused,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct AttachmentSurfaceParams {
    pub(crate) source_id: String,
    pub(crate) message_id: String,
    pub(crate) attachment_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct MessageSurfaceParams {
    pub(crate) conversation_id: String,
    pub(crate) source_id: String,
    pub(crate) message_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SettingsSurfaceParams {
    pub(crate) category: Option<SettingsSurfaceCategory>,
    pub(crate) target: Option<SettingsSurfaceTarget>,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) enum SettingsSurfaceCategory {
    General,
    Appearance,
    Accounts,
    Mailboxes,
}

impl SettingsSurfaceCategory {
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            Self::General => "general",
            Self::Appearance => "appearance",
            Self::Accounts => "accounts",
            Self::Mailboxes => "mailboxes",
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub(crate) enum ComposeSurfaceParams {
    #[serde(rename = "new")]
    New {
        #[serde(rename = "sourceId")]
        source_id: String,
    },
    #[serde(rename = "reply")]
    Reply {
        #[serde(rename = "sourceId")]
        source_id: String,
        #[serde(rename = "messageId")]
        message_id: String,
    },
    #[serde(rename = "forward")]
    Forward {
        #[serde(rename = "sourceId")]
        source_id: String,
        #[serde(rename = "messageId")]
        message_id: String,
    },
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub(crate) enum SettingsSurfaceTarget {
    #[serde(rename = "account")]
    Account {
        #[serde(rename = "accountId")]
        account_id: String,
    },
    #[serde(rename = "newAccount")]
    NewAccount,
    #[serde(rename = "smartMailbox")]
    SmartMailbox {
        #[serde(rename = "smartMailboxId")]
        smart_mailbox_id: String,
    },
    #[serde(rename = "newSmartMailbox")]
    NewSmartMailbox,
    #[serde(rename = "sourceMailbox")]
    SourceMailbox {
        #[serde(rename = "sourceAccountId")]
        source_account_id: String,
        #[serde(rename = "sourceMailboxId")]
        source_mailbox_id: String,
    },
}
