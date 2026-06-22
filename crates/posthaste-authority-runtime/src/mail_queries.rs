mod rules;
mod visibility;

use std::sync::Arc;

use posthaste_domain::{
    AccountId, ConversationCursor, ConversationPage, ConversationSortField, MailService,
    MessageCursor, MessageDetail, MessageId, MessagePage, MessageSortField, SortDirection,
};
use posthaste_runtime_contract::{
    MailPresentationRequest, MailQueryPage, MailQueryRequest, RuntimeError,
};

use crate::supervisor::AccountSupervisor;

pub(crate) struct MailQueryService {
    service: Arc<MailService>,
    supervisor: Arc<AccountSupervisor>,
}

impl MailQueryService {
    pub(crate) fn new(service: Arc<MailService>, supervisor: Arc<AccountSupervisor>) -> Self {
        Self {
            service,
            supervisor,
        }
    }

    /// The overlay-folded local message detail for a runtime `messageDetail`
    /// view. No gateway is passed, so this is a pure local read (the projection
    /// with pending assertions folded), never a provider fetch.
    ///
    /// @spec docs/replication/L1#retire-on-confirmation
    pub(crate) async fn message_detail(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
    ) -> Result<Option<MessageDetail>, RuntimeError> {
        let result = self
            .service
            .get_message_detail(account_id, message_id, None)
            .await?;
        Ok(result.detail)
    }

    pub(crate) async fn query_mail_page(
        &self,
        request: MailQueryRequest,
    ) -> Result<MailQueryPage, RuntimeError> {
        match request.presentation {
            MailPresentationRequest::Messages {
                limit,
                cursor,
                sort_field,
                sort_direction,
            } => {
                let page = self.query_messages(
                    &request.query,
                    limit,
                    cursor.as_ref(),
                    sort_field,
                    sort_direction,
                )?;
                if let Some(visibility) = request.visibility {
                    visibility::record(
                        &self.service,
                        &self.supervisor,
                        &request.query,
                        &page,
                        visibility,
                    )
                    .await;
                }
                Ok(MailQueryPage::Messages(page))
            }
            MailPresentationRequest::CollapsedByConversation {
                limit,
                cursor,
                sort_field,
                sort_direction,
            } => self
                .query_conversations(
                    &request.query,
                    limit,
                    cursor.as_ref(),
                    sort_field,
                    sort_direction,
                )
                .map(MailQueryPage::CollapsedByConversation),
        }
    }

    fn query_messages(
        &self,
        query: &str,
        limit: Option<usize>,
        cursor: Option<&MessageCursor>,
        sort_field: MessageSortField,
        sort_direction: SortDirection,
    ) -> Result<MessagePage, RuntimeError> {
        let rule = rules::compile(&self.service, query)?;
        if let Some(limit) = limit {
            return Ok(self.service.query_message_page_by_rule(
                &rule,
                limit,
                cursor,
                sort_field,
                sort_direction,
            )?);
        }
        let items =
            self.service
                .query_messages_by_rule_sorted(&rule, sort_field, sort_direction)?;
        Ok(MessagePage {
            items,
            next_cursor: None,
        })
    }

    fn query_conversations(
        &self,
        query: &str,
        limit: usize,
        cursor: Option<&ConversationCursor>,
        sort_field: ConversationSortField,
        sort_direction: SortDirection,
    ) -> Result<ConversationPage, RuntimeError> {
        let rule = rules::compile(&self.service, query)?;
        Ok(self.service.query_conversations_by_rule(
            &rule,
            limit,
            cursor,
            sort_field,
            sort_direction,
        )?)
    }
}
