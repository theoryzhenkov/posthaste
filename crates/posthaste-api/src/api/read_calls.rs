use super::*;

/// Request body for the typed read-call endpoint.
///
/// @spec docs/L1-api#read-calls
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ReadRequest {
    pub calls: Vec<ReadCall>,
}

/// A single domain read operation requested as part of `POST /v1/read`.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ReadCall {
    pub id: String,
    pub op: ReadOperation,
    #[serde(default)]
    pub args: ReadCallArgs,
}

/// Supported read operation names.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, ToSchema)]
pub enum ReadOperation {
    #[serde(rename = "Account/list")]
    AccountList,
    #[serde(rename = "Mailbox/list")]
    MailboxList,
    #[serde(rename = "SmartMailbox/list")]
    SmartMailboxList,
    #[serde(rename = "Tag/list")]
    TagList,
}

/// Optional read-call arguments. Only `Mailbox/list` currently uses `accountIds`.
#[derive(Debug, Default, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ReadCallArgs {
    pub account_ids: Option<AccountIdSelector>,
}

/// Account id selector for read calls. A string beginning with `#` is a result
/// reference such as `#accounts.ids`; an array is an explicit account-id list.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(untagged)]
pub enum AccountIdSelector {
    Explicit(Vec<String>),
    Reference(String),
}

/// Response body for the typed read-call endpoint.
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ReadResponse {
    pub results: BTreeMap<String, ReadResult>,
}

/// A successful read-call result, discriminated by operation name.
#[derive(Debug, Serialize, ToSchema)]
#[serde(tag = "op", content = "value")]
pub enum ReadResult {
    #[serde(rename = "Account/list")]
    AccountList(AccountListReadResult),
    #[serde(rename = "Mailbox/list")]
    MailboxList(MailboxListReadResult),
    #[serde(rename = "SmartMailbox/list")]
    SmartMailboxList(SmartMailboxListReadResult),
    #[serde(rename = "Tag/list")]
    TagList(TagListReadResult),
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AccountListReadResult {
    pub ids: Vec<AccountId>,
    pub enabled_ids: Vec<AccountId>,
    pub items: Vec<AccountOverview>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MailboxListReadResult {
    pub by_account_id: BTreeMap<AccountId, Vec<MailboxSummary>>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SmartMailboxListReadResult {
    pub items: Vec<SmartMailboxSummary>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TagListReadResult {
    pub items: Vec<TagSummary>,
}

const MAX_READ_CALLS: usize = 16;

/// POST /v1/read
///
/// Executes typed, read-only domain operations in order. Later calls can refer
/// to earlier account-list results with references such as `#accounts.enabledIds`.
///
/// @spec docs/L1-api#read-calls
#[utoipa::path(
    post,
    path = "/v1/read",
    tag = "read",
    summary = "Execute typed read calls",
    description = "Executes a JMAP-style batch of typed, read-only domain operations.",
    request_body = ReadRequest,
    responses(
        (status = 200, description = "Read-call results keyed by call id", body = ReadResponse),
        (status = 400, description = "Invalid read call or result reference", body = ApiErrorBody),
        (status = 500, description = "Internal error", body = ApiErrorBody)
    )
)]
pub async fn read(
    State(state): State<Arc<AppState>>,
    Json(request): Json<ReadRequest>,
) -> Result<Json<ReadResponse>, ApiError> {
    if request.calls.len() > MAX_READ_CALLS {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            ApiErrorCode::InvalidQuery,
            format!("read request exceeds {MAX_READ_CALLS} calls"),
        ));
    }
    let mut results = BTreeMap::new();
    for call in request.calls {
        if call.id.trim().is_empty() {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                ApiErrorCode::InvalidQuery,
                "read call id must not be empty",
            ));
        }
        if results.contains_key(&call.id) {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                ApiErrorCode::InvalidQuery,
                "read call ids must be unique",
            ));
        }
        let id = call.id;
        let result = execute_read_call(&state, &results, call.op, call.args).await?;
        results.insert(id, result);
    }
    Ok(Json(ReadResponse { results }))
}

async fn execute_read_call(
    state: &Arc<AppState>,
    prior_results: &BTreeMap<String, ReadResult>,
    op: ReadOperation,
    args: ReadCallArgs,
) -> Result<ReadResult, ApiError> {
    match op {
        ReadOperation::AccountList => read_accounts(state).await.map(ReadResult::AccountList),
        ReadOperation::MailboxList => read_mailboxes(state, prior_results, args)
            .await
            .map(ReadResult::MailboxList),
        ReadOperation::SmartMailboxList => state
            .runtime
            .list_smart_mailboxes(RuntimeCaller::api())
            .await
            .map(|items| ReadResult::SmartMailboxList(SmartMailboxListReadResult { items }))
            .map_err(ApiError::from_runtime_error),
        ReadOperation::TagList => read_tags(state, prior_results, args)
            .await
            .map(ReadResult::TagList),
    }
}

async fn read_accounts(state: &Arc<AppState>) -> Result<AccountListReadResult, ApiError> {
    state
        .runtime
        .list_accounts(RuntimeCaller::api())
        .await
        .map(account_list_read_result)
        .map_err(ApiError::from_runtime_error)
}

fn account_list_read_result(accounts: RuntimeAccountList) -> AccountListReadResult {
    AccountListReadResult {
        ids: accounts.ids,
        enabled_ids: accounts.enabled_ids,
        items: accounts.items,
    }
}

async fn read_mailboxes(
    state: &Arc<AppState>,
    prior_results: &BTreeMap<String, ReadResult>,
    args: ReadCallArgs,
) -> Result<MailboxListReadResult, ApiError> {
    let account_ids = resolve_read_account_ids(state, prior_results, args.account_ids).await?;
    state
        .runtime
        .list_mailboxes(
            RuntimeCaller::api(),
            AccountScopeRequest::Explicit { account_ids },
        )
        .await
        .map(|by_account_id| MailboxListReadResult { by_account_id })
        .map_err(ApiError::from_runtime_error)
}

async fn read_tags(
    state: &Arc<AppState>,
    prior_results: &BTreeMap<String, ReadResult>,
    args: ReadCallArgs,
) -> Result<TagListReadResult, ApiError> {
    let account_ids = resolve_read_account_ids(state, prior_results, args.account_ids).await?;
    state
        .runtime
        .list_tags(
            RuntimeCaller::api(),
            AccountScopeRequest::Explicit { account_ids },
        )
        .await
        .map(|items| TagListReadResult { items })
        .map_err(ApiError::from_runtime_error)
}

async fn resolve_read_account_ids(
    state: &Arc<AppState>,
    prior_results: &BTreeMap<String, ReadResult>,
    selector: Option<AccountIdSelector>,
) -> Result<Vec<AccountId>, ApiError> {
    match selector {
        Some(AccountIdSelector::Explicit(ids)) => Ok(ids.into_iter().map(AccountId).collect()),
        Some(AccountIdSelector::Reference(reference)) => {
            resolve_account_id_reference(prior_results, &reference)
        }
        None => state
            .runtime
            .resolve_account_scope(RuntimeCaller::api(), AccountScopeRequest::EnabledAccounts)
            .await
            .map_err(ApiError::from_runtime_error),
    }
}

fn resolve_account_id_reference(
    prior_results: &BTreeMap<String, ReadResult>,
    reference: &str,
) -> Result<Vec<AccountId>, ApiError> {
    let Some(reference) = reference.strip_prefix('#') else {
        return Err(invalid_read_reference(reference));
    };
    let Some((call_id, field)) = reference.split_once('.') else {
        return Err(invalid_read_reference(reference));
    };
    let Some(ReadResult::AccountList(accounts)) = prior_results.get(call_id) else {
        return Err(invalid_read_reference(reference));
    };
    match field {
        "ids" => Ok(accounts.ids.clone()),
        "enabledIds" => Ok(accounts.enabled_ids.clone()),
        _ => Err(invalid_read_reference(reference)),
    }
}

fn invalid_read_reference(reference: &str) -> ApiError {
    ApiError::new(
        StatusCode::BAD_REQUEST,
        ApiErrorCode::InvalidQuery,
        format!("invalid read result reference: {reference}"),
    )
}
