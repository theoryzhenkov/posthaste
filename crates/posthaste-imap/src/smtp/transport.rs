use std::num::NonZeroU32;

use imap_client::imap_types::flag::Flag;
use lettre::transport::smtp::authentication::{Credentials, Mechanism};
use lettre::{AsyncSmtpTransport, AsyncTransport, Tokio1Executor};
use posthaste_domain::{ProviderAuthKind, SendMessageRequest, TransportSecurity};

use crate::discovery::connect_authenticated_client;
use crate::smtp::{build_smtp_message, SmtpConnectionConfig, SubmittedSmtpMessage};
use crate::{ImapAdapterError, ImapConnectionConfig};

/// Send one message through the configured SMTP endpoint.
///
/// @spec docs/L0-providers#imap-smtp-sync-strategy
pub async fn send_smtp_message(
    config: &SmtpConnectionConfig,
    request: &SendMessageRequest,
) -> Result<(), ImapAdapterError> {
    submit_smtp_message(config, request).await?;
    Ok(())
}

/// Send many messages through a single pooled SMTP transport (one connection
/// reused across the batch), avoiding a connection storm on the server.
pub async fn send_smtp_messages(
    config: &SmtpConnectionConfig,
    requests: &[SendMessageRequest],
) -> Result<(), ImapAdapterError> {
    let transport = smtp_transport(config)?;
    for request in requests {
        let message = build_smtp_message(config, request)?;
        transport.send(message).await?;
    }
    Ok(())
}

/// Submit one message and return the exact RFC 5322 bytes accepted by SMTP.
pub async fn submit_smtp_message(
    config: &SmtpConnectionConfig,
    request: &SendMessageRequest,
) -> Result<SubmittedSmtpMessage, ImapAdapterError> {
    let message = build_smtp_message(config, request)?;
    let raw_message = message.formatted();
    smtp_transport(config)?.send(message).await?;

    Ok(SubmittedSmtpMessage { raw_message })
}

/// Append the accepted outbound message to an IMAP Sent mailbox.
///
/// This is only used when provider policy says SMTP submission does not create
/// a server-side Sent copy. The message is appended with `\Seen`.
///
/// @spec docs/L0-providers#imap-smtp-sync-strategy
pub async fn append_smtp_sent_copy(
    config: &ImapConnectionConfig,
    sent_mailbox_name: &str,
    raw_message: &[u8],
) -> Result<Option<NonZeroU32>, ImapAdapterError> {
    let mut client = connect_authenticated_client(config).await?;
    client.refresh_capabilities().await?;
    client
        .appenduid_or_fallback(sent_mailbox_name, [Flag::Seen], raw_message)
        .await
        .map_err(ImapAdapterError::from)
}

fn smtp_transport(
    config: &SmtpConnectionConfig,
) -> Result<AsyncSmtpTransport<Tokio1Executor>, ImapAdapterError> {
    let credentials = Credentials::new(config.username.clone(), config.secret.clone());
    let builder = match config.security {
        TransportSecurity::Tls => AsyncSmtpTransport::<Tokio1Executor>::relay(&config.host)?,
        TransportSecurity::StartTls => {
            AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&config.host)?
        }
        TransportSecurity::Plain => {
            AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(config.host.clone())
        }
    };
    let mechanisms = match config.auth {
        ProviderAuthKind::Password | ProviderAuthKind::AppPassword => {
            vec![Mechanism::Plain, Mechanism::Login]
        }
        ProviderAuthKind::OAuth2 => vec![Mechanism::Xoauth2],
    };
    Ok(builder
        .port(config.port)
        .credentials(credentials)
        .authentication(mechanisms)
        .build())
}
