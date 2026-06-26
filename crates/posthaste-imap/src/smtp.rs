mod config;
mod message;
mod transport;

pub use config::{
    smtp_sent_copy_strategy, SmtpConnectionConfig, SmtpSentCopyStrategy, SubmittedSmtpMessage,
};
pub use message::{build_smtp_message, render_smtp_markdown, smtp_mailbox_for_recipient};
pub use transport::{
    append_smtp_sent_copy, send_smtp_message, send_smtp_messages, submit_smtp_message,
};

#[cfg(test)]
mod tests;
