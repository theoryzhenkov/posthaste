---
scope: L1
summary: "Config directory layout, ConfigRepository contract, TOML schema, reload behavior, smart mailbox defaults"
modified: 2026-04-24
reviewed: 2026-04-24
depends:
  - path: docs/L0-accounts
  - path: docs/L0-search
dependents: []
---

# Accounts domain -- L1

## Config directory layout

All persistent configuration lives in a single directory (`config_root`), organized as:

```
<config_root>/
  app.toml                        # Global app settings
  sources/
    <account_id>.toml             # One file per account
  smart-mailboxes/
    <smart_mailbox_id>.toml       # One file per smart mailbox
```

Each TOML file's filename stem must match the `id` field inside it. Mismatches are rejected at load time. Directories are created on first open if they don't exist.

## ConfigRepository trait

The `ConfigRepository` trait defines the config persistence boundary. Implementations must be `Send + Sync` and support concurrent readers.

```
load_snapshot() → ConfigSnapshot       // Full in-memory snapshot
reload() → ConfigDiff                  // Re-read disk, diff against cached snapshot

get_app_settings() → AppSettings       // Global settings
put_app_settings(AppSettings)          // Persist global settings

list_sources() → Vec<AccountSettings>  // All accounts
get_source(id) → Option<AccountSettings>
save_source(AccountSettings)           // Create or update
delete_source(id)                      // Remove account

list_smart_mailboxes() → Vec<SmartMailbox>
get_smart_mailbox(id) → Option<SmartMailbox>
save_smart_mailbox(SmartMailbox)        // Create or update
delete_smart_mailbox(id)
reset_default_smart_mailboxes() → Vec<SmartMailbox>
```

All methods return `Result<_, ConfigError>`. Error variants: `NotFound`, `Conflict`, `Validation`, `Io`, `Parse`.

## ConfigSnapshot

A `ConfigSnapshot` holds the full in-memory state: `app_settings`, `sources`, and `smart_mailboxes`. The `TomlConfigRepository` caches this snapshot in an `RwLock` and updates it on every write operation, so reads never hit disk after initialization.

## ConfigDiff

`reload()` re-reads all files from disk, compares against the cached snapshot, and returns a `ConfigDiff` listing `added_sources`, `changed_sources`, and `removed_sources`. The caller (posthaste-server) uses this diff to start/stop supervisor runtimes for changed accounts.

## TOML schema

### app.toml

```toml
schema_version = 1
default_source_id = "primary"   # optional

[[automations]]
id = "rule-newsletters"
name = "Posthaste newsletters"
enabled = true
triggers = ["message_arrived"]
backfill = true

[automations.condition]
operator = "all"
negated = false

[[automations.condition.nodes]]
type = "condition"
field = "source_id"
operator = "equals"
negated = false
value = "primary"

[[automations.condition.nodes]]
type = "condition"
field = "from_name"
operator = "contains"
negated = false
value = "Posthaste"

[[automations.actions]]
kind = "apply_tag"
tag = "newsletter"

[daemon]
bind = "127.0.0.1:2525"         # optional, daemon bind address
cors_origin = "http://localhost:5173"  # optional, CORS origin
poll_interval_seconds = 300     # optional, sync poll interval
```

`AppToml` converts bidirectionally to `AppSettings`. `automations` are global backend rules with explicit triggers, smart-mailbox-style conditions, actions, and backfill behavior. Account or mailbox restrictions are ordinary conditions such as `source_id`, `source_name`, `mailbox_id`, `mailbox_name`, or `mailbox_role`. Actions mutate JMAP state through the backend command path, so the server remains authoritative. The `daemon` section is only read at startup and not exposed through the API.

### sources/{id}.toml

```toml
id = "primary"
name = "My Fastmail"
full_name = "Example User"      # optional, sender/display name
email_patterns = ["user@example.com", "*@example.net"]
driver = "jmap"                 # internal; UI-created accounts use JMAP
enabled = true                  # default: true

[appearance]
kind = "initials"               # "initials" or "image"
initials = "MF"
color_hue = 245                 # 0-360 hue used for the account mark
# image_id = "..."              # present for image-backed marks

[transport]
base_url = "https://api.fastmail.com/jmap/session"
username = "user@example.com"  # optional; omit for bearer-token auth

[transport.secret_ref]
kind = "os"                     # "os" (keyring) or "env" (environment variable)
key = "account:primary"
```

`id` is an internal stable identifier used for config filenames, local data scoping, and keyring references. UI-created accounts derive it from the account name or first configured email pattern rather than asking the user to supply it.

`full_name` identifies the person behind the account. `email_patterns` lists owned sender addresses and may include catch-all patterns such as `*@example.net`.

`appearance` is optional account UI metadata. When absent, the API derives a stable initials mark from account name/full name and source ID. Image-backed marks keep the image bytes outside TOML under `account-assets/logos/`, with `image_id` pointing at the stored asset.

`base_url` is the configured JMAP Session URL or provider origin used for discovery. Fastmail accounts use the documented Session resource. Generic providers may use an origin that supports `/.well-known/jmap`.

The referenced secret is an opaque JMAP auth secret. For Fastmail this is an OAuth token set for distributed clients or an API token for personal/testing use, not a Fastmail app-specific password.

When `username` is absent or blank, the runtime sends the secret as a bearer token. When `username` is present, the runtime uses the provider's basic-auth path with the secret as the password/token component.

`SourceToml` converts bidirectionally to `AccountSettings`. Missing `created_at`/`updated_at` default to `RFC3339_EPOCH`.

### smart-mailboxes/{id}.toml

```toml
id = "default-inbox"
name = "Inbox"
position = 0
kind = "default"                # "default" or "user"
default_key = "inbox"           # optional, identifies built-in mailboxes

[rule]
operator = "all"                # "all" or "any"
negated = false

[[rule.nodes]]
type = "condition"
field = "mailbox_role"
operator = "equals"
negated = false
value = "inbox"
```

Smart mailbox rules are recursive: a `rule` contains `nodes` which are either `condition` (leaf) or `group` (nested group with its own `operator` and `nodes`). `SmartMailboxToml` converts bidirectionally to `SmartMailbox` via recursive conversion functions.

### Condition fields and operators

Fields: `source_id`, `source_name`, `message_id`, `thread_id`, `mailbox_id`, `mailbox_name`, `mailbox_role`, `is_read`, `is_flagged`, `has_attachment`, `keyword`, `from_name`, `from_email`, `subject`, `preview`, `received_at`.

Operators: `equals`, `in`, `contains`, `before`, `after`, `on_or_before`, `on_or_after`.

Values can be string, boolean, or string array (for `in` operator).

## Atomic writes

All file writes use `atomic_write`: write to a `.toml.tmp` sibling, `fsync`, then `rename`. This prevents partial writes from corrupting config files on crash.

## ID validation

IDs used as filenames are validated to reject empty strings, path separators (`/`, `\`), parent traversal (`..`), and null bytes. This prevents path injection attacks through the config API.

## Smart mailbox defaults

`default_smart_mailboxes()` returns the built-in set: Inbox, Archive, Drafts, Sent, Junk, Trash, and All Mail. Each is a `SmartMailbox` with `kind: Default`, a `default_key` identifying its role, and a rule filtering by `mailbox_role`. The All Mail mailbox uses an empty rule (matches everything).

`reset_default_smart_mailboxes()` restores these defaults by upserting them into the config directory and updating the snapshot. Existing user-created smart mailboxes are preserved.

## Initialization

On first open, if `app.toml` does not exist, the repository is considered empty. The caller can:
1. Import a bootstrap template (copies a preconfigured directory)
2. Call `initialize_defaults()` to create `app.toml` and the default smart mailboxes

## Assertions

| ID | Sev. | Assertion |
|----|------|-----------|
| filename-id-match | MUST | TOML filename stem must equal the `id` field inside the file |
| atomic-write | MUST | Config file writes use write-fsync-rename to prevent corruption |
| id-validation | MUST | IDs reject path separators, parent traversal, and null bytes |
| snapshot-cached | MUST | After initialization, all reads serve from the in-memory snapshot |
| reload-diff | MUST | `reload()` returns an accurate diff of added, changed, and removed sources |
| defaults-preserved | MUST | `reset_default_smart_mailboxes` does not delete user-created mailboxes |
| bidirectional-conversion | MUST | Domain↔TOML conversions round-trip without data loss |
