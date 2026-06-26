use super::*;

pub(crate) async fn send_message_via_smtp(
    gateway: &LiveImapSmtpGateway,
    imap_config: &ImapConnectionConfig,
    smtp_config: &SmtpConnectionConfig,
    request: &SendMessageRequest,
) -> Result<(), GatewayError> {
    let submitted = submit_smtp_message(smtp_config, request)
        .await
        .map_err(imap_error_to_gateway)?;

    if smtp_sent_copy_strategy(&smtp_config.provider) == SmtpSentCopyStrategy::AppendToSentMailbox {
        if let Some(sent_mailbox) = gateway
            .discovery
            .mailboxes
            .iter()
            .find(|mailbox| mailbox.selectable && mailbox.role == Some("sent"))
        {
            if let Err(error) =
                append_smtp_sent_copy(imap_config, &sent_mailbox.name, &submitted.raw_message).await
            {
                ph_warn!(
                    events::IMAP_SMTP_SENT_APPEND_FAILED,
                    mailbox = sent_mailbox.name,
                    error = %error,
                    "SMTP send accepted but IMAP Sent copy append failed"
                );
            }
        } else {
            ph_warn!(
                events::IMAP_SMTP_SENT_MAILBOX_MISSING,
                "SMTP send accepted but no selectable IMAP Sent mailbox was discovered"
            );
        }
    }

    Ok(())
}
