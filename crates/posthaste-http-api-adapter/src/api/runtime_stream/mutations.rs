use super::*;

use posthaste_contract_core::MailOperation;

use crate::authz::{required_actions, Action, CaveatContext, OperationActions};

#[utoipa::path(
    post,
    path = "/v1/runtime/sessions/{session_id}/mutations",
    tag = "runtime",
    summary = "Run a runtime mutation",
    description = "Submits a named mutation to a runtime link (message read/flag/tags/move/destroy) and emits mutationSettlement RuntimeFrame values on the link stream.",
    params(
        ("session_id" = String, Path, description = "Runtime link id"),
        RuntimeLinkQuery
    ),
    request_body = MutationRequest,
    responses(
        (status = 200, description = "Mutation receipt", body = MutationReceipt),
        (status = 400, description = "Invalid mutation", body = ApiErrorBody),
        (status = 401, description = "Unauthorized", body = ApiErrorBody),
        (status = 403, description = "Forbidden", body = ApiErrorBody),
        (status = 404, description = "Unknown runtime link", body = ApiErrorBody),
        (status = 500, description = "Internal error", body = ApiErrorBody)
    )
)]
pub async fn run_runtime_link_mutation(
    State(state): State<Arc<AppState>>,
    Path(link_id): Path<String>,
    Query(query): Query<RuntimeLinkQuery>,
    presented: Option<Extension<crate::auth::PresentedToken>>,
    Json(mut request): Json<MutationRequest>,
) -> Result<Json<MutationReceipt>, ApiError> {
    let path_link_id = RuntimeLinkId::new(link_id);
    if request
        .link_id
        .as_ref()
        .is_some_and(|body_link_id| body_link_id != &path_link_id)
    {
        return Err(ApiError::from_runtime_error(
            RuntimeError::invalid_mutation("request link id does not match path link id"),
        ));
    }
    request.link_id = Some(path_link_id);
    // Per-operation authorization (deny-by-default): this one route carries
    // every mutation op, so the required action is derived from the PARSED
    // operation — not from a static route verb — and enforced BEFORE dispatch.
    // The perimeter middleware has already enforced the resource/expiry
    // caveats (the route is `RouteAction::HandlerDerived`).
    require_actions_for_link_mutation(
        state.as_ref(),
        presented.as_ref().map(|Extension(token)| token),
        &request.operation,
    )?;
    state
        .runtime
        .forward_mutation(runtime_caller(query.source_id.as_deref()), request)
        .await
        .map(Json)
        .map_err(ApiError::from_runtime_error)
}

#[utoipa::path(
    get,
    path = "/v1/runtime/sessions/{session_id}/mutations/{client_mutation_id}",
    tag = "runtime",
    summary = "Read a runtime mutation's settlement",
    description = "Returns the settlement receipt the runtime holds for a client mutation id, or a null receipt when it has no record (unknown link, never accepted, or already cleared). The client near-end's reconciler queries this for sent-but-unsettled records after a link-continuity loss: a terminal receipt settles locally; a null receipt re-forwards.",
    params(
        ("session_id" = String, Path, description = "Runtime link id the mutation was dispatched under"),
        ("client_mutation_id" = String, Path, description = "Client mutation id"),
        RuntimeLinkQuery
    ),
    responses(
        (status = 200, description = "The runtime's settlement record (receipt is null when unknown)", body = RuntimeMutationSettlement),
        (status = 401, description = "Unauthorized", body = ApiErrorBody),
        (status = 403, description = "Forbidden", body = ApiErrorBody),
        (status = 500, description = "Internal error", body = ApiErrorBody)
    )
)]
pub async fn runtime_link_mutation_settlement(
    State(state): State<Arc<AppState>>,
    Path((link_id, client_mutation_id)): Path<(String, String)>,
    Query(query): Query<RuntimeLinkQuery>,
    presented: Option<Extension<crate::auth::PresentedToken>>,
) -> Result<Json<RuntimeMutationSettlement>, ApiError> {
    require_read_for_link_mutation(
        state.as_ref(),
        query.source_id.as_deref(),
        presented.as_ref().map(|Extension(token)| token),
    )?;
    state
        .runtime
        .mutation_settlement(
            runtime_caller(query.source_id.as_deref()),
            RuntimeLinkId::new(link_id),
            posthaste_contract_core::ClientMutationId::new(client_mutation_id),
        )
        .await
        .map(|receipt| Json(RuntimeMutationSettlement { receipt }))
        .map_err(ApiError::from_runtime_error)
}

fn require_read_for_link_mutation(
    state: &AppState,
    source_id: Option<&str>,
    presented: Option<&crate::auth::PresentedToken>,
) -> Result<(), ApiError> {
    if !state.require_auth {
        return Ok(());
    }
    // Delegate the token→status + deny→status mapping to the one shared helper
    // the middleware also owns, so a handler-side check can never drift from the
    // perimeter's (D72). The deny reason is logged inside the helper.
    let ctx = CaveatContext {
        action: Some(Action::Read),
        account: source_id.map(str::to_owned),
        mailbox: None,
        message: None,
        now: time::OffsetDateTime::now_utc(),
    };
    crate::auth::authorize_presented_caveats(
        presented,
        &state.macaroon_root_key,
        &ctx,
        "runtime link mutation",
    )
}

/// Enforce the PER-OPERATION action(s) for a named mutation (the handler half
/// of the route's `RouteAction::HandlerDerived` contract). The requirement is
/// derived from the parsed operation by [`required_actions`] — an exhaustive,
/// wildcard-free mapping, so every operation kind is explicitly gated and a
/// new one cannot ship unmapped (deny-by-default).
///
/// The caveat context is built from the operation BODY: `account` is the
/// account the operation actually targets (stricter than the query filter the
/// perimeter checked) and `message` is its target message. `mailbox` is
/// deliberately left `None`: a move's semantics span source AND destination
/// mailboxes, so a mailbox-caveated token stays unsatisfiable (fail-closed) on
/// this route, exactly as before.
fn require_actions_for_link_mutation(
    state: &AppState,
    presented: Option<&crate::auth::PresentedToken>,
    operation: &MailOperation,
) -> Result<(), ApiError> {
    if !state.require_auth {
        return Ok(());
    }
    authorize_operation_caveats(
        presented,
        &state.macaroon_root_key,
        operation,
        time::OffsetDateTime::now_utc(),
    )
}

/// Root-key-parameterized core of [`require_actions_for_link_mutation`],
/// factored out so the per-operation authorization matrix is unit-testable
/// without an `AppState`. Delegates each decision to the shared middleware
/// helpers (D72) so the status mapping can never drift from the perimeter's.
fn authorize_operation_caveats(
    presented: Option<&crate::auth::PresentedToken>,
    macaroon_root_key: &crate::token::RootKey,
    operation: &MailOperation,
    now: time::OffsetDateTime,
) -> Result<(), ApiError> {
    const ROUTE: &str = "runtime link mutation";
    let ctx = |action: Action| CaveatContext {
        action: Some(action),
        account: Some(operation.account_id().to_owned()),
        mailbox: None,
        message: operation.message_id().map(str::to_owned),
        now,
    };
    match required_actions(operation) {
        OperationActions::AllOf(actions) => actions.into_iter().try_for_each(|action| {
            crate::auth::authorize_presented_caveats(
                presented,
                macaroon_root_key,
                &ctx(action),
                ROUTE,
            )
        }),
        OperationActions::AnyOf(actions) => {
            let ctxs: Vec<CaveatContext> = actions.into_iter().map(ctx).collect();
            crate::auth::authorize_presented_caveats_any(presented, macaroon_root_key, &ctxs, ROUTE)
        }
    }
}

#[cfg(test)]
mod tests {
    use axum::http::StatusCode;
    use axum::response::IntoResponse;
    use serde_json::json;

    use super::*;
    use crate::auth::PresentedToken;
    use crate::token::{mint_full_scope_token, mint_with_caveats, RootKey};

    fn root() -> RootKey {
        RootKey::from_test_bytes([42u8; 32])
    }

    fn now() -> time::OffsetDateTime {
        time::OffsetDateTime::from_unix_timestamp(1_700_000_000).unwrap()
    }

    fn op(value: serde_json::Value) -> MailOperation {
        serde_json::from_value(value).expect("operation fixture parses")
    }

    fn set_keywords() -> MailOperation {
        op(json!({
            "name": "message.setKeywords",
            "args": { "sourceId": "acct-a", "messageId": "m1",
                      "command": { "add": ["$seen"], "remove": [] } }
        }))
    }

    fn replace_mailboxes() -> MailOperation {
        op(json!({
            "name": "message.replaceMailboxes",
            "args": { "sourceId": "acct-a", "messageId": "m1", "mailboxIds": ["mbx-archive"] }
        }))
    }

    fn destroy() -> MailOperation {
        op(json!({
            "name": "message.destroy",
            "args": { "sourceId": "acct-a", "messageId": "m1" }
        }))
    }

    fn send() -> MailOperation {
        op(json!({
            "name": "message.send",
            "args": { "sourceId": "acct-a", "messageId": "d1",
                      "request": { "from": null, "to": [], "cc": [], "bcc": [],
                                   "subject": "s", "body": "b",
                                   "inReplyTo": null, "references": null, "draftId": "d1" } }
        }))
    }

    fn apply_diff_keywords() -> MailOperation {
        op(json!({
            "name": "message.applyDiff",
            "args": { "sourceId": "acct-a", "messageId": "m1",
                      "diff": { "keywords": { "added": ["$seen"], "removed": [] } } }
        }))
    }

    fn apply_diff_both() -> MailOperation {
        op(json!({
            "name": "message.applyDiff",
            "args": { "sourceId": "acct-a", "messageId": "m1",
                      "diff": { "keywords": { "added": ["$seen"], "removed": [] },
                                "mailboxes": { "added": ["mbx"], "removed": [] } } }
        }))
    }

    fn rev_cursor() -> MailOperation {
        op(json!({
            "name": "revCursor",
            "args": { "accountId": "acct-a", "cursorStepId": null, "redoTail": [] }
        }))
    }

    fn authorize(caveats: &[&str], operation: &MailOperation) -> Result<(), StatusCode> {
        let token = if caveats.is_empty() {
            mint_full_scope_token(&root())
        } else {
            mint_with_caveats(&root(), caveats)
        };
        let presented = PresentedToken(token);
        authorize_operation_caveats(Some(&presented), &root(), operation, now())
            .map_err(|error| error.into_response().status())
    }

    /// The attenuated-token matrix (the security half of the fix): a token
    /// scoped to one write verb can perform exactly that verb's operations
    /// through the mutation funnel and is 403-denied on every other verb —
    /// a tag-scoped token can no longer destroy or send.
    #[test]
    fn per_operation_actions_are_enforced_by_scope() {
        let tag = ["action = tag", "account = acct-a"];
        assert_eq!(authorize(&tag, &set_keywords()), Ok(()));
        assert_eq!(authorize(&tag, &apply_diff_keywords()), Ok(()));
        assert_eq!(
            authorize(&tag, &destroy()),
            Err(StatusCode::FORBIDDEN),
            "a tag-scoped token must not destroy"
        );
        assert_eq!(
            authorize(&tag, &send()),
            Err(StatusCode::FORBIDDEN),
            "a tag-scoped token must not send"
        );
        assert_eq!(
            authorize(&tag, &replace_mailboxes()),
            Err(StatusCode::FORBIDDEN)
        );
        assert_eq!(
            authorize(&tag, &apply_diff_both()),
            Err(StatusCode::FORBIDDEN),
            "a diff touching mailboxes needs move too"
        );

        let mv = ["action = move", "account = acct-a"];
        assert_eq!(authorize(&mv, &replace_mailboxes()), Ok(()));
        assert_eq!(authorize(&mv, &set_keywords()), Err(StatusCode::FORBIDDEN));
        assert_eq!(authorize(&mv, &destroy()), Err(StatusCode::FORBIDDEN));

        let delete = ["action = delete", "account = acct-a"];
        assert_eq!(authorize(&delete, &destroy()), Ok(()));
        assert_eq!(authorize(&delete, &send()), Err(StatusCode::FORBIDDEN));

        let send_scope = ["action = send", "account = acct-a"];
        assert_eq!(authorize(&send_scope, &send()), Ok(()));
        assert_eq!(
            authorize(&send_scope, &destroy()),
            Err(StatusCode::FORBIDDEN)
        );

        // A tag+move token satisfies the two-facet diff's FULL requirement.
        let tag_move = ["action = tag,move", "account = acct-a"];
        assert_eq!(authorize(&tag_move, &apply_diff_both()), Ok(()));
    }

    /// Read-only (and the dev bootstrap's `{mint, read}`) tokens hold no write
    /// verb, so EVERY mutation through the funnel is denied — including the
    /// revCursor control op. The e2e scripts must therefore mint a
    /// write-capable token from the bootstrap; the bootstrap itself never
    /// mutates.
    #[test]
    fn tokens_without_a_write_verb_are_denied_every_operation() {
        for caveats in [&["action = read"] as &[&str], &["action = mint,read"]] {
            for operation in [
                set_keywords(),
                replace_mailboxes(),
                destroy(),
                send(),
                apply_diff_keywords(),
                rev_cursor(),
            ] {
                assert_eq!(
                    authorize(caveats, &operation),
                    Err(StatusCode::FORBIDDEN),
                    "{caveats:?} must not run {}",
                    operation.name()
                );
            }
        }
    }

    /// revCursor (undo/redo bookkeeping) is usable by ANY message-writer: it
    /// accompanies undo/redo of tag AND move diffs, so any one write verb
    /// suffices — but read-only never (covered above).
    #[test]
    fn rev_cursor_accepts_any_message_write_verb() {
        for verb in ["tag", "move", "delete", "send"] {
            let caveats = [format!("action = {verb}"), "account = acct-a".to_string()];
            let refs: Vec<&str> = caveats.iter().map(String::as_str).collect();
            let token = mint_with_caveats(&root(), &refs);
            let presented = PresentedToken(token);
            assert!(
                authorize_operation_caveats(Some(&presented), &root(), &rev_cursor(), now())
                    .is_ok(),
                "a {verb}-scoped token can move the undo cursor"
            );
        }
    }

    /// Full-scope (the embedded webview token) runs everything; the account
    /// caveat is enforced against the account the operation actually targets
    /// (the BODY, not just the query filter); a missing token is 401 not 403.
    #[test]
    fn full_scope_account_walls_and_missing_token() {
        for operation in [
            set_keywords(),
            replace_mailboxes(),
            destroy(),
            send(),
            rev_cursor(),
        ] {
            assert_eq!(authorize(&[], &operation), Ok(()));
        }
        assert_eq!(
            authorize(&["action = tag", "account = acct-OTHER"], &set_keywords()),
            Err(StatusCode::FORBIDDEN),
            "the body-targeted account is enforced"
        );
        let result = authorize_operation_caveats(None, &root(), &set_keywords(), now());
        assert_eq!(
            result.map_err(|error| error.into_response().status()),
            Err(StatusCode::UNAUTHORIZED)
        );
    }

    /// An expired token is denied regardless of its verbs (the handler-side
    /// evaluation carries the full caveat set, expiry included).
    #[test]
    fn expired_token_is_denied() {
        assert_eq!(
            authorize(
                &["action = tag", "expires = 2020-01-01T00:00:00Z"],
                &set_keywords()
            ),
            Err(StatusCode::FORBIDDEN)
        );
    }
}
