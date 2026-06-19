use super::*;

impl AccountMutationService {
    pub fn create_smart_mailbox(
        &self,
        request: CreateSmartMailboxMutation,
    ) -> Result<SmartMailbox, RuntimeError> {
        let timestamp = domain_now_iso8601()
            .map_err(|error| RuntimeError::new(RuntimeErrorCode::Internal, error))?;
        let smart_mailbox = SmartMailbox {
            id: Id::generate().into(),
            name: request.name,
            position: request.position.unwrap_or(0),
            kind: SmartMailboxKind::User,
            default_key: None,
            parent_id: None,
            rule: request.rule,
            created_at: timestamp.clone(),
            updated_at: timestamp,
        };
        self.service.save_smart_mailbox(&smart_mailbox)?;
        self.append_and_publish_event(
            &AccountId::from(GLOBAL_EVENT_ACCOUNT_ID),
            EVENT_TOPIC_SMART_MAILBOX_CREATED,
            config_event_payload(
                vec![ResourceChange::smart_mailbox(
                    ResourceOperation::Created,
                    &smart_mailbox.id,
                )],
                json!({ "smartMailboxId": smart_mailbox.id.as_str() }),
            ),
        )?;
        Ok(smart_mailbox)
    }

    pub fn patch_smart_mailbox(
        &self,
        smart_mailbox_id: SmartMailboxId,
        request: PatchSmartMailboxMutation,
    ) -> Result<SmartMailbox, RuntimeError> {
        let mut smart_mailbox = self.service.get_smart_mailbox(&smart_mailbox_id)?;
        if let Some(name) = request.name {
            smart_mailbox.name = name;
        }
        if let Some(position) = request.position {
            smart_mailbox.position = position;
        }
        if let Some(rule) = request.rule {
            smart_mailbox.rule = rule;
        }
        smart_mailbox.updated_at = domain_now_iso8601()
            .map_err(|error| RuntimeError::new(RuntimeErrorCode::Internal, error))?;
        self.service.save_smart_mailbox(&smart_mailbox)?;
        self.append_and_publish_event(
            &AccountId::from(GLOBAL_EVENT_ACCOUNT_ID),
            EVENT_TOPIC_SMART_MAILBOX_UPDATED,
            config_event_payload(
                vec![ResourceChange::smart_mailbox(
                    ResourceOperation::Updated,
                    &smart_mailbox.id,
                )],
                json!({ "smartMailboxId": smart_mailbox.id.as_str() }),
            ),
        )?;
        Ok(smart_mailbox)
    }

    pub fn delete_smart_mailbox(
        &self,
        smart_mailbox_id: SmartMailboxId,
    ) -> Result<(), RuntimeError> {
        self.service.delete_smart_mailbox(&smart_mailbox_id)?;
        self.append_and_publish_event(
            &AccountId::from(GLOBAL_EVENT_ACCOUNT_ID),
            EVENT_TOPIC_SMART_MAILBOX_DELETED,
            config_event_payload(
                vec![ResourceChange::smart_mailbox(
                    ResourceOperation::Deleted,
                    &smart_mailbox_id,
                )],
                json!({ "smartMailboxId": smart_mailbox_id.as_str() }),
            ),
        )
    }

    pub fn reset_default_smart_mailboxes(
        &self,
    ) -> Result<Vec<posthaste_domain::SmartMailboxSummary>, RuntimeError> {
        self.service.reset_default_smart_mailboxes()?;
        self.append_and_publish_event(
            &AccountId::from(GLOBAL_EVENT_ACCOUNT_ID),
            EVENT_TOPIC_SMART_MAILBOX_RESET,
            config_event_payload(
                vec![ResourceChange::smart_mailbox_reset()],
                json!({ "scope": "smartMailboxes" }),
            ),
        )?;
        Ok(self.reads.list_smart_mailboxes()?)
    }
}
