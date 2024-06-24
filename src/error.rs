//! Error type.

use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

/// Error type.
#[derive(Error, Debug)]
pub enum Error {
    /// Raised when attempting to access a field of a model that has not been set.
    #[error("field {0} is absent")]
    FieldAbsent(&'static str),

    /// Raised when attempting to access the primary key of a model that has no primary key defined.
    #[error("no primary key")]
    NoPrimaryKey,

    /// Raised when executing a delete or update query without a WHERE clause.
    #[error("no where clause")]
    NoWhereClause,

    /// Errors from sqlx.
    #[error(transparent)]
    Sqlx(#[from] sqlx::Error),
}
