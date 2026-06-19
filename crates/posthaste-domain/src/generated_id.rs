use std::marker::PhantomData;

use crate::{AccountId, SmartMailboxId};

pub trait GeneratedIdKind {
    const PREFIX: &'static str;
}

pub struct AccountGeneratedId;
impl GeneratedIdKind for AccountGeneratedId {
    const PREFIX: &'static str = "acct";
}

pub struct SmartMailboxGeneratedId;
impl GeneratedIdKind for SmartMailboxGeneratedId {
    const PREFIX: &'static str = "sm";
}

pub struct GeneratedId<K: GeneratedIdKind> {
    uuid: uuid::Uuid,
    _kind: PhantomData<K>,
}

impl<K: GeneratedIdKind> GeneratedId<K> {
    pub fn generate() -> Self {
        Self {
            uuid: uuid::Uuid::new_v4(),
            _kind: PhantomData,
        }
    }
}

impl<K: GeneratedIdKind> std::fmt::Display for GeneratedId<K> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}_{}", K::PREFIX, self.uuid.simple())
    }
}

pub fn generate_account_id() -> AccountId {
    AccountId::from(GeneratedId::<AccountGeneratedId>::generate().to_string())
}

pub fn generate_smart_mailbox_id() -> SmartMailboxId {
    SmartMailboxId::from(GeneratedId::<SmartMailboxGeneratedId>::generate().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestGeneratedId;
    impl GeneratedIdKind for TestGeneratedId {
        const PREFIX: &'static str = "tst";
    }

    #[test]
    fn generated_id_uses_kind_prefix_and_simple_uuid() {
        let id = GeneratedId::<TestGeneratedId>::generate().to_string();

        let uuid = id.strip_prefix("tst_").expect("prefix should be present");
        assert_eq!(uuid.len(), 32);
        assert!(uuid.chars().all(|ch| ch.is_ascii_hexdigit()));
    }

    #[test]
    fn generated_domain_ids_use_distinct_namespaces() {
        let account_id = generate_account_id();
        let smart_mailbox_id = generate_smart_mailbox_id();

        assert!(account_id.as_str().starts_with("acct_"));
        assert!(smart_mailbox_id.as_str().starts_with("sm_"));
    }
}
