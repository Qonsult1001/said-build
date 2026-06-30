//! Identity & authentication primitives shared across capabilities.
//!
//! `AuthContext` is the authorization input every service's `authorize` step
//! consumes. Fields are private so an `AuthContext` can only be constructed
//! through `authenticated`/`anonymous`, preserving the invariant that an
//! authenticated context always carries a `UserId`.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Opaque, globally-unique user identifier.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct UserId(Uuid);

impl UserId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    pub fn from_uuid(id: Uuid) -> Self {
        Self(id)
    }

    pub fn as_uuid(&self) -> &Uuid {
        &self.0
    }
}

impl Default for UserId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for UserId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A bearer token minted at login and presented on every subsequent call.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Token(String);

impl Token {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn value(&self) -> &str {
        &self.0
    }
}

/// The authorization context threaded into every service call.
///
/// Invariant: an authenticated context always carries a `UserId`. Enforced by
/// keeping the field private and only minting via [`AuthContext::authenticated`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthContext {
    user_id: Option<UserId>,
}

impl AuthContext {
    /// Build an authenticated context bound to `user_id`.
    pub fn authenticated(user_id: UserId) -> Self {
        Self {
            user_id: Some(user_id),
        }
    }

    /// Build an anonymous (unauthenticated) context.
    pub fn anonymous() -> Self {
        Self { user_id: None }
    }

    pub fn is_authenticated(&self) -> bool {
        self.user_id.is_some()
    }

    /// The authenticated user, or `None` when anonymous.
    pub fn user_id(&self) -> Option<&UserId> {
        self.user_id.as_ref()
    }

    /// Require an authenticated user, returning a reference or an
    /// authorization error suitable for a service `authorize` step.
    pub fn require_user(&self) -> Result<&UserId, crate::CoreError> {
        self.user_id
            .as_ref()
            .ok_or_else(|| crate::CoreError::Unauthorized("authentication required".into()))
    }
}
