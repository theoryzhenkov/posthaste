//! 1:1 TOML <-> domain enum conversions generated from a single variant list.
//!
//! Each `*Toml` enum below is variant-name-identical to a domain enum and was
//! previously mapped by two hand-written `match` tables (one per direction).
//! `bimap_enum!` emits both directions from one list. The methods are inherent
//! on the local `*Toml` type so we stay clear of the orphan rule (the domain
//! enum is foreign to this crate).

use super::*;

macro_rules! bimap_enum {
    ($toml:ident => $domain:ident { $($variant:ident),+ $(,)? }) => {
        impl $toml {
            pub(crate) fn to_domain(&self) -> $domain {
                match self {
                    $($toml::$variant => $domain::$variant,)+
                }
            }

            pub(crate) fn from_domain(value: &$domain) -> $toml {
                match value {
                    $($domain::$variant => $toml::$variant,)+
                }
            }
        }
    };
}

bimap_enum!(SmartMailboxKindToml => SmartMailboxKind { Default, User });
bimap_enum!(GroupOperatorToml => SmartMailboxGroupOperator { All, Any });
bimap_enum!(FieldToml => SmartMailboxField {
    SourceId, SourceName, MessageId, ThreadId, ConversationId, MailboxId, MailboxName,
    MailboxRole, IsRead, IsFlagged, HasAttachment, Keyword, FromName, FromEmail, Subject,
    Preview, ReceivedAt,
});
bimap_enum!(ConditionOperatorToml => SmartMailboxOperator {
    Equals, In, Contains, Before, After, OnOrBefore, OnOrAfter,
});
bimap_enum!(DriverToml => AccountDriver { Jmap, ImapSmtp, Mock });
bimap_enum!(SecretKindToml => SecretKind { Env, Os });
bimap_enum!(ProviderHintToml => ProviderHint { Generic, Gmail, Outlook, Icloud });
bimap_enum!(ProviderAuthKindToml => ProviderAuthKind { Password, AppPassword, OAuth2 });
bimap_enum!(TransportSecurityToml => TransportSecurity { Tls, StartTls, Plain });
