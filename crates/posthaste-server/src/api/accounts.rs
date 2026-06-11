use super::*;

pub(crate) mod crud;
pub(crate) mod lifecycle;
pub(crate) mod logos;
pub(crate) mod oauth;
mod oauth_support;
mod support;
mod types;

pub use crud::{create_account, get_account, list_accounts, patch_account, verify_account};
pub use lifecycle::{disable_account, enable_account, reload_config};
pub use logos::{delete_account, get_account_logo, upload_account_logo};
pub use oauth::{complete_account_oauth, start_account_oauth, start_provider_oauth};
pub use types::{
    AccountTransportRequest, CreateAccountRequest, OAuthCallbackQuery, PatchAccountRequest,
    SecretWriteMode, SecretWriteRequest, StartOAuthRequest, StartOAuthResponse,
    StartProviderOAuthRequest,
};

use logos::{account_appearance_image_id, delete_account_logo_file};
use support::{allocate_unique_account_id, persist_new_account};

#[cfg(test)]
pub(super) use oauth_support::{oauth_account_settings, oauth_provider_mail_transport};
