/// TOML-backed configuration persistence for accounts and smart mailboxes.
///
/// @spec docs/L1-accounts#config-directory-layout

mod atomic;
mod defaults;
mod repository;
mod schema;

pub use defaults::default_smart_mailboxes;
pub use repository::TomlConfigRepository;
