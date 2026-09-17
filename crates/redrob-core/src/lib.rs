//! Database and AI runtime for Redrob Query.

pub mod ai;
pub mod connection;
pub mod error;
pub mod models;
pub(crate) mod profile_store;
pub mod query;
pub mod secret;

pub use connection::{DataService, RemoveProfileOutcome, SaveProfileOutcome};
pub use error::{DataError, Result};
pub use models::*;
