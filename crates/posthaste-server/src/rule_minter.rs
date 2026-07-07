//! The macaroon-backed [`CapabilityMinter`] for the in-process rule engine.
//!
//! The rule engine lives in `posthaste-authority-server`, which must not depend
//! on the HTTP adapter's macaroon machinery (a layering inversion). It declares
//! the [`CapabilityMinter`] port; this composition-root type supplies the
//! concrete implementation, signing per-invocation tokens with the same macaroon
//! root key the `/v1` auth perimeter verifies against. So a hook token minted
//! here is accepted by `apply`/`send` — and, being a fresh macaroon carrying only
//! the granted `action`/`account`/`message`/`expires` caveats, can never exceed
//! the grant (it holds no `mint`/`manage`, so e.g. `config:reload` is refused).

use posthaste_authority_server::{CapabilityMinter, RuleTokenGrant};
use posthaste_http_api_adapter::token::{mint_with_caveats, RootKey};

/// Mints per-invocation capability tokens from the process macaroon root key.
pub struct MacaroonMinter {
    root: RootKey,
}

impl MacaroonMinter {
    pub fn new(root: RootKey) -> Self {
        Self { root }
    }
}

impl CapabilityMinter for MacaroonMinter {
    fn mint(&self, grant: &RuleTokenGrant) -> Result<String, String> {
        // F1 (security review, 2026-07-03): an empty action set would mint a
        // token with NO `action` caveat — and an absent caveat is unrestricted,
        // so a grant-less hook could read/tag/move/DELETE the triggering
        // message instead of doing nothing. Reject it (mirrors the REST mint
        // guard in auth_tokens.rs), so the hook dead-letters rather than
        // handing an over-broad credential to a possibly-hijacked agent.
        if grant.actions.is_empty() {
            return Err("rule token grant must name at least one action; \
                        a webhook/exec rule with empty grants is rejected"
                .to_string());
        }
        // The caveat predicate format mirrors the mint route's
        // `build_token_caveats` (a single comma-joined `action` caveat, plus the
        // resource axes) so a hook token is indistinguishable from one minted via
        // `POST /v1/auth/tokens`.
        let mut predicates: Vec<String> = Vec::new();
        if !grant.actions.is_empty() {
            predicates.push(format!("action = {}", grant.actions.join(",")));
        }
        if let Some(account) = &grant.account {
            predicates.push(format!("account = {account}"));
        }
        if let Some(message) = &grant.message {
            predicates.push(format!("message = {message}"));
        }
        if let Some(expires) = &grant.expiry_rfc3339 {
            predicates.push(format!("expires = {expires}"));
        }
        let refs: Vec<&str> = predicates.iter().map(String::as_str).collect();
        Ok(mint_with_caveats(&self.root, &refs))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use posthaste_http_api_adapter::authz::{evaluate, Action, CaveatContext, Decision};
    use posthaste_http_api_adapter::token::verify_authenticity;
    use time::format_description::well_known::Rfc3339;
    use time::OffsetDateTime;

    fn ctx(action: Action, message: &str, now: OffsetDateTime) -> CaveatContext {
        CaveatContext {
            action: Some(action),
            account: Some("acct-1".to_string()),
            mailbox: None,
            message: Some(message.to_string()),
            now,
        }
    }

    /// A hook token carries EXACTLY the granted actions, is confined to the one
    /// matched message, and expires — verified against the real `authz`
    /// enforcement the `/v1` perimeter runs.
    #[test]
    fn hook_token_is_scoped_confined_and_expiring() {
        let root = RootKey::from_test_bytes([7u8; 32]);
        let minter = MacaroonMinter::new(root.clone());
        let now = OffsetDateTime::now_utc();
        let expiry = (now + time::Duration::hours(1))
            .format(&Rfc3339)
            .expect("format expiry");
        let token = minter
            .mint(&RuleTokenGrant {
                actions: vec!["read".to_string(), "tag".to_string()],
                account: Some("acct-1".to_string()),
                message: Some("msg-1".to_string()),
                expiry_rfc3339: Some(expiry),
            })
            .expect("mint");
        let caveats = verify_authenticity(&token, &root).expect("token authentic under root");

        // In scope: the granted verbs on the granted message, before expiry.
        assert_eq!(
            evaluate(&caveats, &ctx(Action::Tag, "msg-1", now)),
            Decision::Allow
        );
        assert_eq!(
            evaluate(&caveats, &ctx(Action::Read, "msg-1", now)),
            Decision::Allow
        );
        // Scope wall: an ungranted verb (mint — the escalation path) is denied.
        assert!(matches!(
            evaluate(&caveats, &ctx(Action::Mint, "msg-1", now)),
            Decision::Deny(_)
        ));
        // Least privilege: a DIFFERENT message is denied even for a granted verb.
        assert!(matches!(
            evaluate(&caveats, &ctx(Action::Tag, "msg-2", now)),
            Decision::Deny(_)
        ));
        // Expiry: a context after the token's expiry is denied.
        let later = now + time::Duration::hours(2);
        assert!(matches!(
            evaluate(&caveats, &ctx(Action::Tag, "msg-1", later)),
            Decision::Deny(_)
        ));
    }
}
