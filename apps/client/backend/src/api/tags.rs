//! The tags family: keyword-derived tags with counts — one account's tags,
//! or the merged set across all accounts (same-named tags merged, counts
//! summed) when the query carries no account scope.

use posthaste_client_models::{TagsQuery, TagsResult};

use super::{scoped_accounts, ApiFailure};
use crate::AppState;

pub(crate) fn evaluate_tags(app: &AppState, query: TagsQuery) -> Result<TagsResult, ApiFailure> {
    let rows = match &query.account_id {
        Some(account_id) => {
            if app.service.get_source(account_id)?.is_none() {
                return Err(ApiFailure::unknown_id(format!(
                    "account {}",
                    account_id.as_str()
                )));
            }
            app.service.list_tags(account_id)?
        }
        None => {
            let account_ids = scoped_accounts(app, None)?;
            app.service.list_merged_tags(&account_ids)?
        }
    };
    Ok(TagsResult { rows })
}
