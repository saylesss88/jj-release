//! CLI error types with exit codes.

use std::fmt::{Display, Formatter};

use anyhow::{Error, anyhow};

#[derive(Debug)]
pub struct CliError {
    pub error: Option<Error>,
    pub code: i32,
}

impl CliError {
    /// Exit silently with a specific code, error was already handled upstream.
    #[must_use]
    pub const fn silent(code: i32) -> Self {
        Self { error: None, code }
    }

    /// Exit with code 101 and a message.
    pub fn message(e: impl Into<Error>) -> Self {
        Self {
            error: Some(e.into()),
            code: 101,
        }
    }

    /// Nothing to release: not an error, exit 0.
    #[must_use]
    pub const fn no_trigger() -> Self {
        Self {
            error: None,
            code: 0,
        }
    }

    /// Missing or invalid release.toml: suggest jj-release init.
    #[must_use]
    pub fn missing_config() -> Self {
        Self {
            error: Some(anyhow!(
                "no release.toml found\nhint: run `jj-release init` to get started"
            )),
            code: 2,
        }
    }

    /// Missing CLI tool: suggest installing it.
    #[must_use]
    pub fn missing_tool(name: &str) -> Self {
        Self {
            error: Some(anyhow!(
                "{name} CLI not found on PATH\nhint: run `jj-release init` to check your setup"
            )),
            code: 3,
        }
    }
}

impl Display for CliError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        self.error.as_ref().map_or(Ok(()), |error| error.fmt(f))
    }
}

impl From<Error> for CliError {
    fn from(error: Error) -> Self {
        Self::message(error)
    }
}

/// Report the result and return the exit code.
#[must_use]
pub fn report(result: Result<(), CliError>) -> i32 {
    match result {
        Ok(()) => 0,
        Err(err) => {
            if let Some(error) = err.error {
                eprintln!("error: {error:#}");
            }
            err.code
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn silent_error_has_no_message() {
        let e = CliError::silent(2);
        assert_eq!(e.code, 2);
        assert!(e.error.is_none());
    }

    #[test]
    fn message_error_has_code_101() {
        let e = CliError::message(anyhow!("something went wrong"));
        assert_eq!(e.code, 101);
        assert!(e.error.is_some());
    }

    #[test]
    fn no_trigger_has_code_0() {
        let e = CliError::no_trigger();
        assert_eq!(e.code, 0);
        assert!(e.error.is_none());
    }

    #[test]
    fn missing_config_has_code_2() {
        let e = CliError::missing_config();
        assert_eq!(e.code, 2);
        assert!(e.error.is_some());
    }

    #[test]
    fn missing_tool_has_code_3() {
        let e = CliError::missing_tool("gh");
        assert_eq!(e.code, 3);
        assert!(e.error.is_some());
    }
}
