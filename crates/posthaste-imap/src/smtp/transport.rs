use std::num::NonZeroU32;
use std::time::Duration;

use imap_client::client::tokio::Client as ImapClient;
use imap_client::imap_types::flag::Flag;
use lettre::transport::smtp::authentication::{Credentials, Mechanism};
use lettre::{AsyncSmtpTransport, AsyncTransport, Tokio1Executor};
use posthaste_domain_model::{ProviderAuthKind, SendMessageRequest, TransportSecurity};

use crate::smtp::{build_smtp_message, SmtpConnectionConfig, SubmittedSmtpMessage};
use crate::ImapAdapterError;

/// Socket-level timeout on every SMTP exchange (connect, EHLO, AUTH, DATA,
/// ...). Without it a stalled MTA hangs on lettre's internal defaults with no
/// app-level bound (audit C5). The send path layers the call-policy
/// `SEND_TOTAL` wall-clock deadline on top; this catches per-command stalls
/// inside that window.
pub(crate) const SMTP_COMMAND_TIMEOUT: Duration = Duration::from_secs(30);

/// Send one message through the configured SMTP endpoint.
///
/// @spec docs/L0-providers#imap-smtp-sync-strategy
pub async fn send_smtp_message(
    config: &SmtpConnectionConfig,
    request: &SendMessageRequest,
) -> Result<(), ImapAdapterError> {
    submit_smtp_message(config, request, None).await?;
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
        let message = build_smtp_message(config, request, None)?;
        transport.send(message).await?;
    }
    Ok(())
}

/// Submit one message and return the exact RFC 5322 bytes accepted by SMTP.
///
/// `message_id` carries the stable, retry-constant Message-ID (D85) when the
/// caller is the idempotent outbox send path; `None` lets lettre generate one.
pub async fn submit_smtp_message(
    config: &SmtpConnectionConfig,
    request: &SendMessageRequest,
    message_id: Option<&str>,
) -> Result<SubmittedSmtpMessage, ImapAdapterError> {
    let message = build_smtp_message(config, request, message_id)?;
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
pub(crate) async fn append_smtp_sent_copy(
    client: &mut ImapClient,
    sent_mailbox_name: &str,
    raw_message: &[u8],
) -> Result<Option<NonZeroU32>, ImapAdapterError> {
    crate::timeout::with_deadline(
        "append",
        client.appenduid_or_fallback(sent_mailbox_name, [Flag::Seen], raw_message),
    )
    .await
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
        // C5: bound every SMTP socket exchange; a stalled MTA must fail, not
        // wedge the send path.
        .timeout(Some(SMTP_COMMAND_TIMEOUT))
        .build())
}
