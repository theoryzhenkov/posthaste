use super::*;

pub(crate) struct ImapSyncProgressUpdate {
    pub(crate) stage: SyncProgressStage,
    pub(crate) detail: &'static str,
    pub(crate) mailbox_name: Option<String>,
    pub(crate) mailbox_index: Option<usize>,
    pub(crate) mailbox_count: Option<usize>,
    pub(crate) message_count: Option<usize>,
    pub(crate) total_count: Option<usize>,
}

impl ImapSyncProgressUpdate {
    pub(crate) fn new(stage: SyncProgressStage, detail: &'static str) -> Self {
        Self {
            stage,
            detail,
            mailbox_name: None,
            mailbox_index: None,
            mailbox_count: None,
            message_count: None,
            total_count: None,
        }
    }

    pub(crate) fn with_mailbox_count(mut self, mailbox_count: usize) -> Self {
        self.mailbox_count = Some(mailbox_count);
        self
    }

    pub(crate) fn with_mailbox(
        mut self,
        mailbox_name: String,
        mailbox_index: usize,
        mailbox_count: usize,
    ) -> Self {
        self.mailbox_name = Some(mailbox_name);
        self.mailbox_index = Some(mailbox_index);
        self.mailbox_count = Some(mailbox_count);
        self
    }

    pub(crate) fn with_message_count(mut self, message_count: usize) -> Self {
        self.message_count = Some(message_count);
        self
    }
}

pub(crate) fn report_sync_progress(
    reporter: &Option<SyncProgressReporter>,
    update: ImapSyncProgressUpdate,
) {
    if let Some(reporter) = reporter {
        reporter.report(SyncProgress {
            sync_id: String::new(),
            trigger: SyncTrigger::Manual,
            started_at: String::new(),
            stage: update.stage,
            detail: update.detail.to_string(),
            mailbox_name: update.mailbox_name,
            mailbox_index: update.mailbox_index,
            mailbox_count: update.mailbox_count,
            message_count: update.message_count,
            total_count: update.total_count,
        });
    }
}
