mod config;
mod message;
pub(crate) mod transport;

pub use config::{
    smtp_sent_copy_strategy, SmtpConnectionConfig, SmtpSentCopyStrategy, SubmittedSmtpMessage,
};
pub use message::{
    build_smtp_message, render_smtp_markdown, smtp_mailbox_for_recipient, smtp_stable_message_id,
};
pub use transport::{send_smtp_message, send_smtp_messages, submit_smtp_message};

#[cfg(test)]
mod tests;
