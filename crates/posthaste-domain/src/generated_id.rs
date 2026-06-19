use std::marker::PhantomData;

use uuid::Uuid;

use crate::{AccountId, SmartMailboxId};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IdUuidFormat {
    Simple,
    Hyphenated,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IdGenerationConfig {
    pub separator: char,
    pub uuid_format: IdUuidFormat,
}

impl Default for IdGenerationConfig {
    fn default() -> Self {
        Self {
            separator: '_',
            uuid_format: IdUuidFormat::Simple,
        }
    }
}

pub trait IdKind {
    const PREFIX: &'static str;

    fn generation_config() -> IdGenerationConfig {
        IdGenerationConfig::default()
    }
}

pub struct AccountIdKind;
impl IdKind for AccountIdKind {
    const PREFIX: &'static str = "acct";
}

pub struct SmartMailboxIdKind;
impl IdKind for SmartMailboxIdKind {
    const PREFIX: &'static str = "sm";
}

pub struct Id<K: IdKind> {
    uuid: Uuid,
    _kind: PhantomData<K>,
}

impl<K: IdKind> Id<K> {
    pub fn generate() -> Self {
        Self {
            uuid: Uuid::new_v4(),
            _kind: PhantomData,
        }
    }
}

impl<K: IdKind> std::fmt::Display for Id<K> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let config = K::generation_config();
        match config.uuid_format {
            IdUuidFormat::Simple => write!(
                formatter,
                "{}{}{}",
                K::PREFIX,
                config.separator,
                self.uuid.simple()
            ),
            IdUuidFormat::Hyphenated => {
                write!(formatter, "{}{}{}", K::PREFIX, config.separator, self.uuid)
            }
        }
    }
}

impl From<Id<AccountIdKind>> for AccountId {
    fn from(id: Id<AccountIdKind>) -> Self {
        Self::from(id.to_string())
    }
}

impl From<Id<SmartMailboxIdKind>> for SmartMailboxId {
    fn from(id: Id<SmartMailboxIdKind>) -> Self {
        Self::from(id.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestGeneratedId;
    impl IdKind for TestGeneratedId {
        const PREFIX: &'static str = "tst";
    }

    struct HyphenatedGeneratedId;
    impl IdKind for HyphenatedGeneratedId {
        const PREFIX: &'static str = "hyp";

        fn generation_config() -> IdGenerationConfig {
            IdGenerationConfig {
                separator: '-',
                uuid_format: IdUuidFormat::Hyphenated,
            }
        }
    }

    #[test]
    fn generated_id_uses_kind_prefix_and_default_simple_uuid_config() {
        let id = Id::<TestGeneratedId>::generate().to_string();

        let uuid = id.strip_prefix("tst_").expect("prefix should be present");
        assert_eq!(uuid.len(), 32);
        assert!(uuid.chars().all(|ch| ch.is_ascii_hexdigit()));
    }

    #[test]
    fn generated_id_kind_can_override_default_config() {
        let id = Id::<HyphenatedGeneratedId>::generate().to_string();

        let uuid = id.strip_prefix("hyp-").expect("prefix should be present");
        assert_eq!(uuid.len(), 36);
        assert_eq!(uuid.chars().filter(|ch| *ch == '-').count(), 4);
    }

    #[test]
    fn generated_domain_ids_use_distinct_namespaces() {
        let account_id = AccountId::from(Id::<AccountIdKind>::generate());
        let smart_mailbox_id = SmartMailboxId::from(Id::<SmartMailboxIdKind>::generate());

        assert!(account_id.as_str().starts_with("acct_"));
        assert!(smart_mailbox_id.as_str().starts_with("sm_"));
    }
}
