use jmap_client::identity;
use posthaste_domain::{GatewayError, Identity};

use crate::live::{map_gateway_error, required_method_response, LiveJmapGateway};

/// Fetch the primary sender identity for an account via `Identity/get`.
///
/// @spec docs/L1-jmap#methods-used
/// @spec docs/L1-compose#composesession-interface
pub(crate) async fn fetch_identity(gateway: &LiveJmapGateway) -> Result<Identity, GatewayError> {
    let mut identities = fetch_identities(gateway).await?;
    identities
        .pop()
        .ok_or_else(|| GatewayError::Rejected("no identity available".to_string()))
}

pub(crate) async fn fetch_identities(
    gateway: &LiveJmapGateway,
) -> Result<Vec<Identity>, GatewayError> {
    let mut request = gateway.client().build();
    request.get_identity().properties([
        identity::Property::Id,
        identity::Property::Name,
        identity::Property::Email,
    ]);
    let mut response = gateway.send_request(request).await?;
    let identities = required_method_response(response.pop_method_response(), "Identity/get")?
        .unwrap_get_identity()
        .map_err(map_gateway_error)?
        .take_list();
    Ok(identities
        .into_iter()
        .map(|identity| Identity {
            id: identity.id().unwrap_or_default().to_string(),
            name: identity.name().unwrap_or_default().to_string(),
            email: identity.email().unwrap_or_default().to_string(),
        })
        .collect())
}

/// Resolve the sender identity for an account before composing or sending.
///
/// @spec docs/L1-jmap#methods-used
/// @spec docs/L1-compose#composesession-interface
pub(crate) async fn fetch_send_identity(
    gateway: &LiveJmapGateway,
    requested_from: Option<&posthaste_domain::Recipient>,
) -> Result<Identity, GatewayError> {
    resolve_send_identity(fetch_identities(gateway).await?, requested_from)
}

/// Resolve the `from` header for a *draft* save (name + email only).
///
/// A draft create via `Email/set` carries no `identityId` — only the `from`
/// address — so unlike [`fetch_send_identity`], a provider with an empty
/// `Identity/get` (e.g. Stalwart without a configured identity) must **not**
/// block saving a draft. When the caller supplies a `from` address we use it
/// directly, consulting the provider identities only to fill a missing display
/// name; we fall back to a provider identity solely when no `from` was given.
///
/// @spec docs/L1-jmap#methods-used
/// @spec docs/L1-compose#composesession-interface
pub(crate) async fn fetch_draft_sender(
    gateway: &LiveJmapGateway,
    requested_from: Option<&posthaste_domain::Recipient>,
) -> Result<Identity, GatewayError> {
    resolve_draft_sender(fetch_identities(gateway).await?, requested_from)
}

pub(crate) fn resolve_draft_sender(
    identities: Vec<Identity>,
    requested_from: Option<&posthaste_domain::Recipient>,
) -> Result<Identity, GatewayError> {
    let Some(requested_from) = requested_from else {
        // No requested sender: fall back to the provider's default identity,
        // which is the only source of an address in this case.
        return identities
            .into_iter()
            .next_back()
            .ok_or_else(|| GatewayError::Rejected("no identity available".to_string()));
    };
    let requested_email = requested_from.email.trim();
    if requested_email.is_empty() {
        return Err(GatewayError::Rejected(
            "sender email address cannot be empty".to_string(),
        ));
    }
    // The draft has no identityId, so the id is irrelevant; only name + email
    // reach the wire. Prefer the caller's display name, then a matching
    // provider identity's name, then the default identity's name.
    let name = requested_from
        .name
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_string)
        .or_else(|| {
            identities
                .iter()
                .find(|identity| identity.email.eq_ignore_ascii_case(requested_email))
                .or_else(|| identities.last())
                .map(|identity| identity.name.clone())
        })
        .unwrap_or_default();
    Ok(Identity {
        id: String::new(),
        name,
        email: requested_email.to_string(),
    })
}

pub(crate) fn resolve_send_identity(
    mut identities: Vec<Identity>,
    requested_from: Option<&posthaste_domain::Recipient>,
) -> Result<Identity, GatewayError> {
    let default_identity = identities
        .pop()
        .ok_or_else(|| GatewayError::Rejected("no identity available".to_string()))?;

    let Some(requested_from) = requested_from else {
        return Ok(default_identity);
    };
    let requested_email = requested_from.email.trim();
    if requested_email.is_empty() {
        return Err(GatewayError::Rejected(
            "sender email address cannot be empty".to_string(),
        ));
    }

    let identity_id = identities
        .iter()
        .chain(std::iter::once(&default_identity))
        .find(|identity| identity.email.eq_ignore_ascii_case(requested_email))
        .map(|identity| identity.id.clone())
        .unwrap_or_else(|| default_identity.id.clone());
    Ok(Identity {
        id: identity_id,
        name: requested_from
            .name
            .as_deref()
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .unwrap_or(default_identity.name.as_str())
            .to_string(),
        email: requested_email.to_string(),
    })
}
