use uuid::Uuid;

use crate::{AccountId, SmartMailboxId};

/// App-wide generated identifier.
///
/// The serialized form is a UUID without separators so generated IDs use one
/// stable format across resource types.
pub struct Id {
    uuid: Uuid,
}

impl Id {
    pub fn generate() -> Self {
        Self {
            uuid: Uuid::new_v4(),
        }
    }
}

impl std::fmt::Display for Id {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}", self.uuid.simple())
    }
}

impl From<Id> for String {
    fn from(id: Id) -> Self {
        id.to_string()
    }
}

impl From<Id> for AccountId {
    fn from(id: Id) -> Self {
        Self::from(id.to_string())
    }
}

impl From<Id> for SmartMailboxId {
    fn from(id: Id) -> Self {
        Self::from(id.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_id_uses_simple_uuid_format() {
        let id = Id::generate().to_string();

        assert_eq!(id.len(), 32);
        assert!(id.chars().all(|ch| ch.is_ascii_hexdigit()));
    }

    #[test]
    fn generated_id_converts_to_domain_ids_without_changing_format() {
        let account_id = AccountId::from(Id::generate());
        let smart_mailbox_id = SmartMailboxId::from(Id::generate());

        assert_eq!(account_id.as_str().len(), 32);
        assert_eq!(smart_mailbox_id.as_str().len(), 32);
        assert!(account_id.as_str().chars().all(|ch| ch.is_ascii_hexdigit()));
        assert!(smart_mailbox_id
            .as_str()
            .chars()
            .all(|ch| ch.is_ascii_hexdigit()));
    }
}
