use thiserror::Error;

use crate::ValidationError;

/// Errors from configuration persistence operations.
///
/// @spec docs/L1-accounts#configrepository-trait
#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("not found: {0}")]
    NotFound(String),
    #[error("conflict: {0}")]
    Conflict(String),
    #[error("validation error: {0}")]
    Validation(String),
    #[error("io error: {0}")]
    Io(String),
    #[error("parse error: {0}")]
    Parse(String),
}

impl From<ValidationError> for ConfigError {
    fn from(error: ValidationError) -> Self {
        Self::Validation(error.to_string())
    }
}

impl From<Vec<ValidationError>> for ConfigError {
    fn from(errors: Vec<ValidationError>) -> Self {
        Self::Validation(format_validation_errors(&errors))
    }
}

fn format_validation_errors(errors: &[ValidationError]) -> String {
    if errors.is_empty() {
        return "config validation failed".to_string();
    }
    errors
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("; ")
}
