//! JSON shapes of the public API. Every struct here is the wire contract the
//! web app is generated from, so field names are camelCase, enums are
//! lowercase without their proto prefix and timestamps are RFC 3339.

use chrono::{DateTime, Utc};
use prost_types::Timestamp;

pub mod code;
pub mod common;
pub mod delivery;
pub mod diagrams;
pub mod execution;
pub mod github;
pub mod identity;
pub mod intelligence;
pub mod knowledge;
pub mod reviews;
pub mod settings;
pub mod system;

/// Declares a public enum mirroring a proto enum: same variants, snake_case
/// on the wire, with lossless conversions both ways so handlers can map
/// query filters and request bodies back to the proto value.
macro_rules! proto_enum {
    (
        $(#[$meta:meta])*
        $name:ident => $proto:path { $($variant:ident),+ $(,)? }
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, utoipa::ToSchema)]
        #[serde(rename_all = "snake_case")]
        pub enum $name {
            Unspecified,
            $($variant),+
        }

        impl From<i32> for $name {
            fn from(raw: i32) -> Self {
                match <$proto>::try_from(raw).unwrap_or_default() {
                    $(<$proto>::$variant => Self::$variant,)+
                    _ => Self::Unspecified,
                }
            }
        }

        impl From<$name> for $proto {
            fn from(value: $name) -> Self {
                match value {
                    $name::Unspecified => Self::Unspecified,
                    $($name::$variant => Self::$variant,)+
                }
            }
        }

        impl From<$name> for i32 {
            fn from(value: $name) -> Self {
                <$proto>::from(value) as i32
            }
        }
    };
}

pub(crate) use proto_enum;

/// Required timestamp. The epoch marks a value the service never set, which
/// is visible in the UI instead of silently becoming "now".
pub(crate) fn timestamp(at: Option<&Timestamp>) -> DateTime<Utc> {
    sdi_common::time::opt_from_proto(at).unwrap_or(DateTime::UNIX_EPOCH)
}

/// Optional timestamp: unset in the proto stays `null` on the wire.
pub(crate) fn opt_timestamp(at: Option<&Timestamp>) -> Option<DateTime<Utc>> {
    sdi_common::time::opt_from_proto(at)
}

/// Empty proto strings mean "not set"; the API says so explicitly with `null`.
pub(crate) fn opt_string(value: String) -> Option<String> {
    (!value.is_empty()).then_some(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn required_timestamp_falls_back_to_epoch() {
        assert_eq!(timestamp(None), DateTime::UNIX_EPOCH);
        let at = Timestamp {
            seconds: 1_700_000_000,
            nanos: 0,
        };
        assert_eq!(timestamp(Some(&at)).timestamp(), 1_700_000_000);
    }

    #[test]
    fn empty_strings_become_null() {
        assert_eq!(opt_string(String::new()), None);
        assert_eq!(opt_string("x".into()), Some("x".into()));
    }
}
