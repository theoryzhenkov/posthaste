use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::Json;
use posthaste_api::api::{
    health, list_mailboxes, list_source_messages, read, AccountIdSelector, ReadCall, ReadCallArgs,
    ReadOperation, ReadRequest,
};

use crate::support::{
    api_error_json, default_source_messages_query, message, response_json, ApiHarness,
};

#[tokio::test]
async fn health_returns_only_product_readiness_status() {
    let (status, body) = response_json(health().await.into_response()).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, serde_json::json!({ "status": "ok" }));
}

// spec: docs/L1-api#read-calls
#[tokio::test]
async fn read_calls_support_account_references_for_navigation_data() {
    let harness = ApiHarness::new();
    harness.save_account("primary");
    harness.seed_messages(
        "primary",
        vec![message("message-1", "2026-04-02T10:00:00Z")],
    );

    let Json(response) = match read(
        State(harness.state.clone()),
        Json(ReadRequest {
            calls: vec![
                ReadCall {
                    id: "accounts".to_string(),
                    op: ReadOperation::AccountList,
                    args: ReadCallArgs::default(),
                },
                ReadCall {
                    id: "mailboxes".to_string(),
                    op: ReadOperation::MailboxList,
                    args: ReadCallArgs {
                        account_ids: Some(AccountIdSelector::Reference(
                            "#accounts.enabledIds".to_string(),
                        )),
                    },
                },
                ReadCall {
                    id: "smart".to_string(),
                    op: ReadOperation::SmartMailboxList,
                    args: ReadCallArgs::default(),
                },
                ReadCall {
                    id: "tags".to_string(),
                    op: ReadOperation::TagList,
                    args: ReadCallArgs {
                        account_ids: Some(AccountIdSelector::Reference(
                            "#accounts.enabledIds".to_string(),
                        )),
                    },
                },
            ],
        }),
    )
    .await
    {
        Ok(response) => response,
        Err(error) => panic!(
            "read calls should succeed, got {}",
            error.into_response().status()
        ),
    };

    let json = serde_json::to_value(response).expect("response should serialize");
    assert_eq!(json["results"]["accounts"]["op"], "Account/list");
    assert_eq!(
        json["results"]["accounts"]["value"]["enabledIds"],
        serde_json::json!(["primary"])
    );
    assert_eq!(json["results"]["mailboxes"]["op"], "Mailbox/list");
    assert_eq!(
        json["results"]["mailboxes"]["value"]["byAccountId"]["primary"][0]["name"],
        "Inbox"
    );
    assert_eq!(json["results"]["smart"]["op"], "SmartMailbox/list");
    assert_eq!(json["results"]["tags"]["op"], "Tag/list");
}

// spec: docs/L1-api#read-calls
#[tokio::test]
async fn read_calls_reject_unknown_result_references() {
    let harness = ApiHarness::new();

    let error = read(
        State(harness.state.clone()),
        Json(ReadRequest {
            calls: vec![ReadCall {
                id: "mailboxes".to_string(),
                op: ReadOperation::MailboxList,
                args: ReadCallArgs {
                    account_ids: Some(AccountIdSelector::Reference("#missing.ids".to_string())),
                },
            }],
        }),
    )
    .await
    .expect_err("missing reference should fail");

    let (status, body) = api_error_json(error).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], "invalid_query");
}

// spec: docs/L0-testing#api-boundary-contracts
#[tokio::test]
async fn source_message_page_returns_structured_not_found_for_unknown_source() {
    let harness = ApiHarness::new();

    let error = list_source_messages(
        State(harness.state.clone()),
        Path("missing".to_string()),
        HeaderMap::new(),
        Query(default_source_messages_query()),
    )
    .await
    .expect_err("unknown source should fail");

    let (status, body) = api_error_json(error).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["code"], "not_found");
    assert_eq!(body["message"], "account not found");
    assert_eq!(body["details"], serde_json::json!({}));
}

// spec: docs/L0-testing#api-boundary-contracts
#[tokio::test]
async fn source_mailboxes_return_structured_not_found_for_unknown_source() {
    let harness = ApiHarness::new();

    let error = list_mailboxes(State(harness.state.clone()), Path("missing".to_string()))
        .await
        .expect_err("unknown source should fail");

    let (status, body) = api_error_json(error).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["code"], "not_found");
    assert_eq!(body["message"], "account not found");
    assert_eq!(body["details"], serde_json::json!({}));
}

// spec: docs/L0-testing#api-boundary-contracts
#[tokio::test]
async fn source_message_page_rejects_cursor_issued_for_another_source() {
    let harness = ApiHarness::new();
    harness.save_account("primary");
    harness.save_account("secondary");
    harness.seed_messages(
        "primary",
        vec![
            message("primary-new", "2026-04-02T10:00:00Z"),
            message("primary-old", "2026-04-01T10:00:00Z"),
        ],
    );
    harness.seed_messages(
        "secondary",
        vec![
            message("secondary-new", "2026-04-02T11:00:00Z"),
            message("secondary-old", "2026-04-01T11:00:00Z"),
        ],
    );
    let mut first_page_query = default_source_messages_query();
    first_page_query.limit = Some(1);
    let Json(first_page) = match list_source_messages(
        State(harness.state.clone()),
        Path("secondary".to_string()),
        HeaderMap::new(),
        Query(first_page_query),
    )
    .await
    {
        Ok(page) => page,
        Err(error) => panic!(
            "secondary page should load, got {}",
            error.into_response().status()
        ),
    };
    let cursor = first_page
        .next_cursor
        .expect("first secondary page should include a cursor");

    let mut cross_source_query = default_source_messages_query();
    cross_source_query.cursor = Some(cursor);
    let error = list_source_messages(
        State(harness.state.clone()),
        Path("primary".to_string()),
        HeaderMap::new(),
        Query(cross_source_query),
    )
    .await
    .expect_err("cursor from another source should fail");

    let (status, body) = api_error_json(error).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], "invalid_cursor");
    assert_eq!(body["details"], serde_json::json!({}));
}
