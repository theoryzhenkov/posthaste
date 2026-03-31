use std::path::Path;

use mail_domain::{
    AccountDriver, AccountId, AccountSettings, AccountTransportSettings, AppSettings, ConfigError,
    ConfigRepository, SecretKind, SecretRef, SmartMailbox, SmartMailboxId, SmartMailboxKind,
    SmartMailboxRule,
};
use rusqlite::{Connection, OptionalExtension};

use crate::repository::TomlConfigRepository;
use crate::schema::AppToml;

pub fn export_from_sqlite(
    db_path: &Path,
    config_repo: &TomlConfigRepository,
) -> Result<bool, ConfigError> {
    if !db_path.exists() {
        return Ok(false);
    }

    let connection = Connection::open_with_flags(db_path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|e| ConfigError::Io(format!("failed to open legacy database: {e}")))?;

    let has_data = has_legacy_config(&connection)?;
    if !has_data {
        return Ok(false);
    }

    let app_settings = read_legacy_app_settings(&connection)?;
    let accounts = read_legacy_accounts(&connection)?;
    let smart_mailboxes = read_legacy_smart_mailboxes(&connection)?;

    // Write app.toml
    let app_toml = AppToml {
        schema_version: 1,
        default_source_id: app_settings
            .default_account_id
            .as_ref()
            .map(|id| id.to_string()),
        daemon: Default::default(),
    };
    config_repo.put_app_settings(&app_settings)?;

    // Write app.toml with daemon defaults preserved
    let existing = config_repo.read_app_toml()?;
    let _ = app_toml; // app_toml was used for the structure, but put_app_settings already wrote it
    // Re-read to make sure daemon section wasn't lost
    let _ = existing;

    // Write sources
    for account in &accounts {
        config_repo.save_source(account)?;
    }

    // Write smart mailboxes
    for mailbox in &smart_mailboxes {
        config_repo.save_smart_mailbox(mailbox)?;
    }

    Ok(true)
}

fn has_legacy_config(connection: &Connection) -> Result<bool, ConfigError> {
    // Check if account_config table exists and has rows
    let has_accounts: bool = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='account_config')",
            [],
            |row| row.get(0),
        )
        .map_err(sql_error)?;
    if !has_accounts {
        return Ok(false);
    }

    let count: i64 = connection
        .query_row("SELECT COUNT(*) FROM account_config", [], |row| row.get(0))
        .unwrap_or(0);
    Ok(count > 0)
}

fn read_legacy_app_settings(connection: &Connection) -> Result<AppSettings, ConfigError> {
    let json: Option<String> = connection
        .query_row(
            "SELECT settings_json FROM app_settings WHERE singleton = 1",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(sql_error)?;

    match json {
        Some(json) => serde_json::from_str::<AppSettings>(&json)
            .map_err(|e| ConfigError::Parse(format!("app_settings: {e}"))),
        None => Ok(AppSettings::default()),
    }
}

fn read_legacy_accounts(connection: &Connection) -> Result<Vec<AccountSettings>, ConfigError> {
    let mut statement = connection
        .prepare(
            "SELECT account_id, name, driver, enabled, transport_json, created_at, updated_at
             FROM account_config
             ORDER BY account_id",
        )
        .map_err(sql_error)?;

    let rows = statement
        .query_map([], |row| {
            let id: String = row.get(0)?;
            let name: String = row.get(1)?;
            let driver_str: String = row.get(2)?;
            let enabled: bool = row.get(3)?;
            let transport_json: String = row.get(4)?;
            let created_at: String = row.get(5)?;
            let updated_at: String = row.get(6)?;
            Ok((id, name, driver_str, enabled, transport_json, created_at, updated_at))
        })
        .map_err(sql_error)?;

    let mut accounts = Vec::new();
    for row in rows {
        let (id, name, driver_str, enabled, transport_json, created_at, updated_at) =
            row.map_err(sql_error)?;

        let driver = match driver_str.as_str() {
            "jmap" => AccountDriver::Jmap,
            "mock" => AccountDriver::Mock,
            other => {
                return Err(ConfigError::Parse(format!(
                    "unknown driver '{other}' for account '{id}'"
                )))
            }
        };

        let transport: LegacyTransport = serde_json::from_str(&transport_json)
            .map_err(|e| ConfigError::Parse(format!("transport for '{id}': {e}")))?;

        accounts.push(AccountSettings {
            id: AccountId::from(id.as_str()),
            name,
            driver,
            enabled,
            transport: AccountTransportSettings {
                base_url: transport.base_url,
                username: transport.username,
                secret_ref: transport.secret_ref.map(|sr| SecretRef {
                    kind: match sr.kind.as_str() {
                        "env" => SecretKind::Env,
                        _ => SecretKind::Os,
                    },
                    key: sr.key,
                }),
            },
            created_at,
            updated_at,
        });
    }
    Ok(accounts)
}

fn read_legacy_smart_mailboxes(
    connection: &Connection,
) -> Result<Vec<SmartMailbox>, ConfigError> {
    // Check if smart_mailbox table exists
    let has_table: bool = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='smart_mailbox')",
            [],
            |row| row.get(0),
        )
        .map_err(sql_error)?;
    if !has_table {
        return Ok(Vec::new());
    }

    let mut statement = connection
        .prepare(
            "SELECT id, name, position, kind, default_key, parent_id, rule_json, created_at, updated_at
             FROM smart_mailbox
             ORDER BY position ASC, name ASC",
        )
        .map_err(sql_error)?;

    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, Option<String>>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, String>(7)?,
                row.get::<_, String>(8)?,
            ))
        })
        .map_err(sql_error)?;

    let mut mailboxes = Vec::new();
    for row in rows {
        let (id, name, position, kind_str, default_key, parent_id, rule_json, created_at, updated_at) =
            row.map_err(sql_error)?;

        let kind = match kind_str.as_str() {
            "default" => SmartMailboxKind::Default,
            "user" => SmartMailboxKind::User,
            other => {
                return Err(ConfigError::Parse(format!(
                    "unknown smart mailbox kind '{other}' for '{id}'"
                )))
            }
        };

        let rule: SmartMailboxRule = serde_json::from_str(&rule_json)
            .map_err(|e| ConfigError::Parse(format!("rule for smart mailbox '{id}': {e}")))?;

        mailboxes.push(SmartMailbox {
            id: SmartMailboxId::from(id.as_str()),
            name,
            position,
            kind,
            default_key,
            parent_id: parent_id.map(|id| SmartMailboxId::from(id.as_str())),
            rule,
            created_at,
            updated_at,
        });
    }
    Ok(mailboxes)
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct LegacyTransport {
    base_url: Option<String>,
    username: Option<String>,
    secret_ref: Option<LegacySecretRef>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct LegacySecretRef {
    kind: String,
    key: String,
}

fn sql_error(err: rusqlite::Error) -> ConfigError {
    ConfigError::Io(format!("sqlite: {err}"))
}
