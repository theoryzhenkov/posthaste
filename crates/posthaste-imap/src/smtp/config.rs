use lettre::Address;
use posthaste_domain_service::{
    AccountSettings, AccountTransportSettings, ProviderAuthKind, ProviderHint, ProviderProfile,
    SmtpSentCopyPolicy, TransportSecurity,
};

use crate::ImapAdapterError;

/// Concrete connection details for one SMTP submission endpoint.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SmtpConnectionConfig {
    pub host: String,
    pub port: u16,
    pub security: TransportSecurity,
    pub sender_name: Option<String>,
    pub sender_email: String,
    pub username: String,
    pub secret: String,
    pub auth: ProviderAuthKind,
    pub provider: ProviderHint,
}

impl SmtpConnectionConfig {
    pub fn from_account_settings(
        account: &AccountSettings,
        secret: String,
    ) -> Result<Self, ImapAdapterError> {
        Self::from_parts(
            &account.transport,
            account.full_name.as_deref(),
            concrete_sender_email(&account.email_patterns),
            secret,
        )
    }

    fn from_parts(
        transport: &AccountTransportSettings,
        sender_name: Option<&str>,
        sender_email: Option<String>,
        secret: String,
    ) -> Result<Self, ImapAdapterError> {
        let smtp = transport
            .smtp
            .as_ref()
            .ok_or(ImapAdapterError::MissingSmtpTransport)?;
        let username = transport
            .username
            .as_deref()
            .map(str::trim)
            .filter(|username| !username.is_empty())
            .ok_or(ImapAdapterError::MissingUsername)?;
        if secret.trim().is_empty() {
            return Err(ImapAdapterError::MissingSecret);
        }
        let sender_email = sender_email.ok_or(ImapAdapterError::MissingSmtpSenderEmail)?;
        let sender_name = sender_name.and_then(|name| {
            let name = name.trim();
            (!name.is_empty()).then(|| name.to_string())
        });

        Ok(Self {
            host: smtp.host.clone(),
            port: smtp.port,
            security: smtp.security.clone(),
            sender_name,
            sender_email,
            username: username.to_string(),
            secret,
            auth: transport.auth.clone(),
            provider: transport.provider.clone(),
        })
    }
}

fn concrete_sender_email<'a>(emails: impl IntoIterator<Item = &'a String>) -> Option<String> {
    emails.into_iter().find_map(|email| {
        let email = email.trim();
        if email.is_empty() || email.contains('*') {
            return None;
        }
        email.parse::<Address>().is_ok().then(|| email.to_string())
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SmtpSentCopyStrategy {
    ProviderManaged,
    AppendToSentMailbox,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SubmittedSmtpMessage {
    pub raw_message: Vec<u8>,
}

pub fn smtp_sent_copy_strategy(provider: &ProviderHint) -> SmtpSentCopyStrategy {
    match ProviderProfile::from_hint(provider).smtp().sent_copy() {
        SmtpSentCopyPolicy::ProviderManaged => SmtpSentCopyStrategy::ProviderManaged,
        SmtpSentCopyPolicy::AppendToSentMailbox => SmtpSentCopyStrategy::AppendToSentMailbox,
    }
}
