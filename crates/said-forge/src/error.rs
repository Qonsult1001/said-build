//! Unified error type for said-forge.
//!
//! Variants cover every recoverable class of failure. Non-recoverable
//! programming bugs use `panic!` directly.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum ForgeError {
    #[error("directive not found at {0}")]
    DirectiveNotFound(String),

    #[error("no source adapter detected for {0}")]
    NoAdapter(String),

    #[error("parse error in {path}: {message}")]
    Parse { path: String, message: String },

    #[error("brain error: {0}")]
    Brain(String),

    #[error("LLM provider error: {0}")]
    Llm(String),

    #[error("LLM response validation failed: {0}")]
    Validation(String),

    #[error("I/O error on {path}: {cause}")]
    Io {
        path: String,
        #[source]
        cause: std::io::Error,
    },

    #[error("config error: {0}")]
    Config(String),

    #[error("HTTP error fetching {url}: {message}")]
    Http { url: String, message: String },

    #[error("serde error: {0}")]
    Serde(#[from] serde_json::Error),

    #[error("circuit breaker halted batch after {consecutive} consecutive {class} failures")]
    CircuitBreaker { consecutive: u32, class: String },

    #[error("context window exceeded for story {slug}: {estimated_tokens} tokens")]
    ContextExceeded { slug: String, estimated_tokens: u32 },
}

pub type ForgeResult<T> = Result<T, ForgeError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn directive_not_found_displays_path() {
        let e = ForgeError::DirectiveNotFound("petstore.yaml".into());
        assert_eq!(format!("{}", e), "directive not found at petstore.yaml");
    }

    #[test]
    fn parse_error_formats_source_and_message() {
        let e = ForgeError::Parse {
            path: "petstore.yaml".into(),
            message: "missing paths object".into(),
        };
        assert_eq!(format!("{}", e), "parse error in petstore.yaml: missing paths object");
    }

    #[test]
    fn circuit_breaker_displays_counts() {
        let e = ForgeError::CircuitBreaker { consecutive: 5, class: "auth".into() };
        assert!(format!("{}", e).contains("5 consecutive auth"));
    }

    #[test]
    fn serde_error_converts_via_from() {
        // JSON parse failure should become a ForgeError::Serde
        let bad: Result<serde_json::Value, _> = serde_json::from_str("not json");
        let forge_err: ForgeError = bad.unwrap_err().into();
        assert!(matches!(forge_err, ForgeError::Serde(_)));
    }
}
