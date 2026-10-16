//! Safe, structured errors exposed by the backend.

use std::time::Duration;

use thiserror::Error;

/// Result type used throughout the core crate.
pub type Result<T> = std::result::Result<T, DataError>;

/// Backend failures. Messages are deliberately sanitized before crossing IPC.
#[derive(Debug, Error)]
pub enum DataError {
    #[error("connection profile was not found")]
    ProfileNotFound,
    #[error("invalid connection profile: {0}")]
    InvalidProfile(String),
    #[error("this operation is not supported: {0}")]
    Unsupported(String),
    #[error("a required secret is unavailable")]
    SecretUnavailable,
    #[error("secure credential storage is unavailable")]
    KeyringUnavailable,
    #[error("connection profile storage failed: {0}")]
    ProfileStorage(String),
    #[error("connection settings could not be updated safely")]
    PersistenceConsistency,
    #[error("connection failed: {0}")]
    Connection(String),
    #[error("query was rejected by the safety policy: {0}")]
    ReadOnlyViolation(String),
    #[error("query is invalid: {0}")]
    InvalidQuery(String),
    #[error("query timed out after {0:?}")]
    Timeout(Duration),
    #[error("query execution failed: {0}")]
    Query(String),
    #[error("mutation approval is required")]
    ApprovalRequired,
    #[error("mutation approval is invalid or expired")]
    InvalidApproval,
    #[error("AI authentication is not configured")]
    AiAuthentication,
    #[error("AI service is temporarily unavailable")]
    AiUnavailable,
    #[error("AI request was rejected: {0}")]
    AiRejected(String),
    #[error("AI response was invalid")]
    AiInvalidResponse,
    #[error("internal data conversion failed: {0}")]
    Conversion(String),
}

impl DataError {
    /// Converts untrusted database errors to a safe message without URLs or credentials.
    pub(crate) fn database(context: &'static str, error: &impl std::fmt::Display) -> Self {
        let message = sanitize_error(&error.to_string());
        Self::Query(format!("{context}: {message}"))
    }

    /// Converts untrusted connection errors to a safe message.
    pub(crate) fn connection(error: &impl std::fmt::Display) -> Self {
        Self::Connection(sanitize_error(&error.to_string()))
    }
}

fn sanitize_error(input: &str) -> String {
    let mut output = input.to_owned();
    for marker in [
        "postgres://",
        "postgresql://",
        "mysql://",
        "mongodb://",
        "mongodb+srv://",
    ] {
        while let Some(start) = output.find(marker) {
            let tail = &output[start..];
            let end = tail.find(char::is_whitespace).unwrap_or(tail.len());
            output.replace_range(start..start + end, "[redacted database URL]");
        }
    }
    if output.len() > 400 {
        output.truncate(400);
        output.push('…');
    }
    output
}

#[cfg(test)]
mod tests {
    use super::sanitize_error;

    #[test]
    fn sanitizes_database_urls() {
        for input in [
            "failed mongodb://alice:secret@example.test/db while connecting",
            "failed mongodb+srv://alice:secret@example.test/db while connecting",
        ] {
            let safe = sanitize_error(input);
            assert!(!safe.contains("secret"));
            assert!(!safe.contains("alice"));
            assert!(safe.contains("[redacted database URL]"));
        }
    }
}
