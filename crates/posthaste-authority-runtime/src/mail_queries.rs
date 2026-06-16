mod rules;
mod visibility;

use std::sync::Arc;

use posthaste_domain::{
    ConversationCursor, ConversationPage, ConversationSortField, MailService, MessageCursor,
    MessagePage, MessageSortField, SortDirection,
};
use posthaste_runtime_contract::{
    MailPresentationRequest, MailQueryPage, MailQueryRequest, RuntimeError,
};

use crate::account_mutations::service_error_to_runtime_error;
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
            return self
                .service
                .query_message_page_by_rule(&rule, limit, cursor, sort_field, sort_direction)
                .map_err(service_error_to_runtime_error);
        }
        self.service
            .query_messages_by_rule_sorted(&rule, sort_field, sort_direction)
            .map(|items| MessagePage {
                items,
                next_cursor: None,
            })
            .map_err(service_error_to_runtime_error)
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
        self.service
            .query_conversations_by_rule(&rule, limit, cursor, sort_field, sort_direction)
            .map_err(service_error_to_runtime_error)
    }
}
