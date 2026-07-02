use imap_client::client::tokio::Client as ImapClient;
use imap_client::imap_types::flag::FlagNameAttribute;
use imap_client::imap_types::mailbox::Mailbox;
use posthaste_domain_model::{AccountTransportSettings, ImapCapabilities, MailboxRole, ProviderAuthKind, ProviderProfile, TransportSecurity};
use posthaste_domain_model::MailboxId;

use crate::ImapAdapterError;

/// Concrete connection details for one IMAP account.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImapConnectionConfig {
    pub host: String,
    pub port: u16,
    pub security: TransportSecurity,
    pub username: String,
    pub secret: String,
    pub auth: ProviderAuthKind,
}

impl ImapConnectionConfig {
    pub fn from_account_transport(
        transport: &AccountTransportSettings,
        secret: String,
    ) -> Result<Self, ImapAdapterError> {
        let imap = transport
            .imap
            .as_ref()
            .ok_or(ImapAdapterError::MissingTransport)?;
        let username = transport
            .username
            .as_deref()
            .map(str::trim)
            .filter(|username| !username.is_empty())
            .ok_or(ImapAdapterError::MissingUsername)?;
        if secret.trim().is_empty() {
            return Err(ImapAdapterError::MissingSecret);
        }

        Ok(Self {
            host: imap.host.clone(),
            port: imap.port,
            security: imap.security.clone(),
            username: username.to_string(),
            secret,
            auth: transport.auth.clone(),
        })
    }
}

/// IMAP account information discovered after connection and authentication.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiscoveredImapAccount {
    pub capabilities: ImapCapabilities,
    pub mailboxes: Vec<DiscoveredImapMailbox>,
}

impl DiscoveredImapAccount {
    pub fn provider_profile(&self) -> ProviderProfile {
        ProviderProfile::from_imap_capabilities(&self.capabilities)
    }
}

/// Mailbox metadata from IMAP LIST, normalized for Posthaste.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiscoveredImapMailbox {
    pub id: MailboxId,
    pub name: String,
    pub role: Option<&'static str>,
    pub selectable: bool,
    pub attributes: Vec<String>,
}

/// Connect, authenticate, refresh capabilities, and list mailboxes.
///
/// This is intentionally discovery-only. Sync code selects mailboxes later and
/// records UIDVALIDITY/MODSEQ through the domain planner.
///
/// @spec docs/L0-providers#imap-smtp-sync-strategy
pub async fn discover_imap_account(
    config: &ImapConnectionConfig,
) -> Result<DiscoveredImapAccount, ImapAdapterError> {
    let mut client = connect_authenticated_client(config).await?;
    discover_authenticated_client(&mut client).await
}

pub(crate) async fn connect_authenticated_client(
    config: &ImapConnectionConfig,
) -> Result<ImapClient, ImapAdapterError> {
    let mut client = connect(config).await?;
    authenticate(&mut client, config).await?;
    Ok(client)
}

pub(crate) async fn discover_authenticated_client(
    client: &mut ImapClient,
) -> Result<DiscoveredImapAccount, ImapAdapterError> {
    client.refresh_capabilities().await?;

    let capabilities = normalize_imap_capabilities(
        client
            .state
            .capabilities_iter()
            .map(std::string::ToString::to_string),
    );
    let provider = ProviderProfile::from_imap_capabilities(&capabilities);
    let mailboxes = client
        .list("", "*")
        .await?
        .into_iter()
        .map(|(mailbox, _delimiter, attributes)| {
            map_client_mailbox(&provider, &mailbox, &attributes)
        })
        .collect::<Result<Vec<_>, _>>()?;

    Ok(DiscoveredImapAccount {
        capabilities,
        mailboxes,
    })
}

async fn connect(config: &ImapConnectionConfig) -> Result<ImapClient, ImapAdapterError> {
    match config.security {
        TransportSecurity::Tls => {
            Ok(ImapClient::rustls(&config.host, config.port, false, None).await?)
        }
        TransportSecurity::StartTls => {
            Ok(ImapClient::rustls(&config.host, config.port, true, None).await?)
        }
        TransportSecurity::Plain => Ok(ImapClient::insecure(&config.host, config.port).await?),
    }
}

async fn authenticate(
    client: &mut ImapClient,
    config: &ImapConnectionConfig,
) -> Result<(), ImapAdapterError> {
    match config.auth {
        ProviderAuthKind::Password | ProviderAuthKind::AppPassword => {
            client
                .authenticate_plain(&config.username, &config.secret)
                .await?;
        }
        ProviderAuthKind::OAuth2 => {
            client
                .authenticate_xoauth2(&config.username, &config.secret)
                .await?;
        }
    }
    Ok(())
}

/// Normalize raw IMAP capability tokens into the domain capability set.
pub fn normalize_imap_capabilities(
    tokens: impl IntoIterator<Item = impl AsRef<str>>,
) -> ImapCapabilities {
    ImapCapabilities::from_tokens(tokens)
}

/// Map a mailbox name and LIST attributes into Posthaste discovery metadata.
pub fn map_imap_mailbox(
    name: impl Into<String>,
    attributes: impl IntoIterator<Item = impl AsRef<str>>,
) -> DiscoveredImapMailbox {
    map_imap_mailbox_with_provider(
        ProviderProfile::from_kind(posthaste_domain_model::ProviderKind::Generic),
        name,
        attributes,
    )
}

/// Map a mailbox name and LIST attributes with provider-specific role policy.
pub fn map_imap_mailbox_with_provider(
    provider: ProviderProfile,
    name: impl Into<String>,
    attributes: impl IntoIterator<Item = impl AsRef<str>>,
) -> DiscoveredImapMailbox {
    let name = name.into();
    let attributes = attributes
        .into_iter()
        .map(|attribute| attribute.as_ref().to_string())
        .collect::<Vec<_>>();
    let selectable = !attributes
        .iter()
        .any(|attribute| attribute.eq_ignore_ascii_case("\\Noselect"));
    let role = provider
        .imap()
        .mailbox_role(&name, attributes.iter().map(String::as_str))
        .map(|role| {
            MailboxRole::parse(role)
                .map(MailboxRole::as_str)
                .unwrap_or(role)
        });

    DiscoveredImapMailbox {
        id: imap_mailbox_id(&name),
        name,
        role,
        selectable,
        attributes,
    }
}

/// Build an opaque stable mailbox ID from an IMAP mailbox name.
pub fn imap_mailbox_id(name: &str) -> MailboxId {
    MailboxId(format!("imap:mailbox:{}", hex::encode(name.as_bytes())))
}

fn map_client_mailbox(
    provider: &ProviderProfile,
    mailbox: &Mailbox<'_>,
    attributes: &[FlagNameAttribute<'_>],
) -> Result<DiscoveredImapMailbox, ImapAdapterError> {
    let name = match mailbox {
        Mailbox::Inbox => "INBOX".to_string(),
        Mailbox::Other(other) => String::from_utf8(other.as_ref().to_vec())
            .map_err(|_| ImapAdapterError::InvalidMailboxName(format!("{other:?}")))?,
    };
    Ok(map_imap_mailbox_with_provider(
        *provider,
        name,
        attributes.iter().map(std::string::ToString::to_string),
    ))
}

#[cfg(test)]
mod tests;
