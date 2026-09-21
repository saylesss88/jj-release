//! Library error types and exit codes.

use thiserror::Error;

/// The core error type for `jj-release` library operations.
#[derive(Error, Debug)]
pub enum ReleaseError {
    #[error("no release.toml found\nhint: run `jj-release init` to get started")]
    MissingConfig,

    #[error("{0} CLI not found on PATH\nhint: run `jj-release init` to check your setup")]
    MissingTool(String),

    /// A generic message error for uncategorized failures.
    #[error("{0}")]
    Message(String),

    /// Used when a wrapped underlying IO/Parse error occurs.
    #[error(transparent)]
    Io(#[from] std::io::Error),
    // Add other transparent wrappers here as needed (e.g., toml::de::Error)
}

/// A wrapper for exit states that aren't strictly "failures" (like `NoTrigger`),
/// or for passing explicit exit codes back to the CLI runner.
#[derive(Debug)]
pub enum ExitState {
    /// Exit silently with a specific code, error was already handled upstream.
    Silent(i32),
    /// Nothing to release: not an error, exit 0.
    NoTrigger,
    /// An actual failure occurred.
    Error { error: ReleaseError, code: i32 },
}

impl ExitState {
    #[must_use]
    pub const fn silent(code: i32) -> Self {
        Self::Silent(code)
    }

    #[must_use]
    pub const fn no_trigger() -> Self {
        Self::NoTrigger
    }

    pub fn missing_config() -> Self {
        Self::Error {
            error: ReleaseError::MissingConfig,
            code: 2,
        }
    }

    pub fn missing_tool(name: &str) -> Self {
        Self::Error {
            error: ReleaseError::MissingTool(name.to_string()),
            code: 3,
        }
    }

    pub fn message(msg: impl Into<String>) -> Self {
        Self::Error {
            error: ReleaseError::Message(msg.into()),
            code: 101,
        }
    }
}

/// Report the result and return the exit code for the CLI `main()` function.
#[must_use]
pub fn report(result: Result<(), ExitState>) -> i32 {
    match result {
        Ok(()) | Err(ExitState::NoTrigger) => 0,
        // Err(ExitState::NoTrigger) => 0,
        Err(ExitState::Silent(code)) => code,
        Err(ExitState::Error { error, code }) => {
            eprintln!("error: {error}");
            code
        }
    }
}

/// A convenient alias for `Result` that defaults to `ReleaseError`.
pub type Result<T, E = ReleaseError> = std::result::Result<T, E>;
