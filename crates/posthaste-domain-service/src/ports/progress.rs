use super::*;

#[derive(Clone)]
pub struct SyncProgressReporter {
    sync_id: String,
    trigger: SyncTrigger,
    started_at: String,
    callback: Arc<dyn Fn(SyncProgress) + Send + Sync>,
}

impl SyncProgressReporter {
    pub fn new(
        sync_id: impl Into<String>,
        trigger: SyncTrigger,
        started_at: impl Into<String>,
        callback: impl Fn(SyncProgress) + Send + Sync + 'static,
    ) -> Self {
        Self {
            sync_id: sync_id.into(),
            trigger,
            started_at: started_at.into(),
            callback: Arc::new(callback),
        }
    }

    pub fn report(&self, mut progress: SyncProgress) {
        progress.sync_id.clone_from(&self.sync_id);
        progress.trigger = self.trigger.clone();
        progress.started_at.clone_from(&self.started_at);
        (self.callback)(progress);
    }
}
