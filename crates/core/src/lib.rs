//! Roll's core: the data model, database, scanning, sorting rules, the
//! organize planner, the journal and background jobs.

pub mod dates;
pub mod db;
pub mod groups;
pub mod jobs;
pub mod library;
pub mod model;
pub mod paths;
pub mod query;
pub mod scan;
pub mod schema;
pub mod settings;
pub mod sidecars;
pub mod tags;
pub mod thumbs;
pub mod watch;
pub mod why;

pub use library::{LibEvent, Library};

#[cfg(test)]
pub(crate) mod testutil;
