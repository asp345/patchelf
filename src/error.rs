//! A single error type: a message, optionally prefixed with context.

use std::fmt;

#[derive(Debug)]
pub struct Error(String);

impl Error {
    pub fn new(msg: impl Into<String>) -> Self {
        Error(msg.into())
    }

    /// Prefix the message with `ctx: `.
    pub fn context(self, ctx: impl fmt::Display) -> Self {
        Error(format!("{ctx}: {}", self.0))
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error(e.to_string())
    }
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Return early with a formatted [`Error`].
#[macro_export]
macro_rules! bail {
    ($($arg:tt)*) => { return Err($crate::error::Error::new(format!($($arg)*))) };
}

/// Return early with a formatted [`Error`] unless `cond` holds.
#[macro_export]
macro_rules! ensure {
    ($cond:expr, $($arg:tt)*) => { if !$cond { $crate::bail!($($arg)*) } };
}
