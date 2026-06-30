//! Newtype identifiers shared across capabilities.
//!
//! A `String` id everywhere is a stringly-typed trap: nothing stops a
//! `ProductId` being passed where an `AccountId` is wanted. These zero-cost
//! newtypes restore type safety while staying `Clone + Hash + Eq` so they key a
//! `HashMap` directly.

use std::fmt;

macro_rules! id_newtype {
    ($name:ident, $label:literal) => {
        #[derive(Debug, Clone, PartialEq, Eq, Hash)]
        pub struct $name(String);

        impl $name {
            pub fn new(raw: impl Into<String>) -> Self {
                Self(raw.into())
            }
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}({})", $label, self.0)
            }
        }

        impl From<String> for $name {
            fn from(s: String) -> Self {
                Self(s)
            }
        }
    };
}

id_newtype!(AccountId, "account");
id_newtype!(CoverageId, "coverage");
id_newtype!(LeadId, "lead");
id_newtype!(ProductId, "product");
