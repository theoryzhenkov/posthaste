use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::{
    AccountSettings, AccountTransportSettings, ImapCapabilities, ImapFullSyncReason,
    ImapLabelSource, ImapMessageIdentitySource, ImapProviderFeatures, ImapThreadIdentitySource,
    ImapTransportSettings, MailboxRole, ProviderHint, SmtpTransportSettings, TransportSecurity,
};

/// Provider family independent of the account driver/protocol.
///
/// `AccountDriver` selects the runtime protocol. `ProviderKind` selects the
/// vendor/family policy applied within that protocol.
///
/// @spec docs/L0-providers#driver-model
mod imap_policy;
mod jmap_policy;
mod kind_profile;
mod oauth_policy;
mod remote_observation;
mod smtp_policy;

pub use imap_policy::ImapProviderPolicy;
pub use jmap_policy::{JmapProviderPolicy, ProviderPolicy};
pub use kind_profile::{ProviderKind, ProviderProfile};
pub use oauth_policy::{OAuthDefaultMailTransport, OAuthOpenIdIssuerPolicy, OAuthProviderPolicy};
pub use remote_observation::{RemoteIdleScope, RemoteObservationPolicy};
pub use smtp_policy::{SmtpProviderPolicy, SmtpSentCopyPolicy};

#[cfg(test)]
mod tests;
