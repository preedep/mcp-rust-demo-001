use thiserror::Error;

/// Failures the domain can express, independent of transport or framework.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum DomainError {
    #[error("unknown tool '{0}'")]
    UnknownTool(String),

    #[error("invalid argument: {0}")]
    InvalidArgument(String),

    #[error("unknown session")]
    UnknownSession,
}
