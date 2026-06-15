use super::*;
mod appearance;
mod events;
mod normalization;
mod overview;
mod secrets;
mod transport;
mod validation;

pub(super) use appearance::{
    default_account_appearance, normalize_account_appearance, validate_account_appearance,
    validate_logo_image_id,
};
pub(super) use events::{append_and_publish_account_event, internal_error, store_error_to_api};
#[cfg(test)]
pub(super) use normalization::apply_account_patch;
pub(super) use normalization::normalize_optional;
pub(super) use overview::account_overview;
pub(super) use secrets::delete_managed_secret;
#[cfg(test)]
pub(super) use secrets::{
    account_secret_ref, apply_secret_instruction, decide_secret_instruction,
    AccountSecretRefUpdate, SecretInstructionDecision, SecretStoreInstruction,
};

#[cfg(test)]
pub(super) use overview::secret_status;
#[cfg(test)]
pub(super) use secrets::validate_secret_request;
pub(super) use validation::validate_account_settings;

#[cfg(test)]
mod tests;
