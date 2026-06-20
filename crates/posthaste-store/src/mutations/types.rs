use super::*;

#[derive(Default)]
pub(crate) struct ProjectionInputs {
    pub(crate) threads: BTreeSet<ThreadId>,
    pub(crate) conversations: BTreeSet<ConversationId>,
}

pub(crate) struct MessageBeforeApply {
    pub(crate) mailboxes: Vec<MailboxId>,
    pub(crate) keywords: Vec<String>,
    pub(crate) conversation_id: Option<ConversationId>,
    pub(crate) existed: bool,
}
