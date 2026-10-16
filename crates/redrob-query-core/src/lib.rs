//! Redrob Query engine: database connectivity, metadata, bounded querying, reviewed edits and the
//! console AI transport. The desktop shell (`apps/redrob-query`) drives it through [`Workspace`].

pub mod account;
pub mod ai;
pub mod cell_edits;
pub mod connection;
pub mod ddl;
pub mod error;
pub mod keys;
pub mod models;
pub(crate) mod profile_store;
pub mod query;
pub mod secret;
pub mod workspace;

pub use connection::{DataService, RemoveProfileOutcome, SaveProfileOutcome};
pub use error::{DataError, Result};
pub use models::*;
pub use workspace::{Pending, Workspace};
