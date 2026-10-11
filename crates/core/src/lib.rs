//! Roll's core: the data model, database, scanning, sorting rules, the
//! organize planner, the journal and background jobs.

pub mod actions;
pub mod categories;
pub mod classify;
pub mod datefix;
pub mod dates;
pub mod db;
pub mod discover;
pub mod dupes;
pub mod edits;
pub mod embeddings;
pub mod fsops;
pub mod groups;
pub mod jobs;
pub mod journal;
pub mod library;
pub mod model;
pub mod packs;
pub mod paths;
pub mod places;
pub mod planner;
pub mod privacy;
pub mod query;
pub mod review;
pub mod rules;
pub mod scan;
pub mod schema;
pub mod search;
pub mod secrets;
pub mod settings;
pub mod sidecars;
pub mod tags;
pub mod template;
pub mod thumbs;
pub mod trash;
pub mod watch;
pub mod why;

pub use library::{LibEvent, Library};

#[cfg(test)]
pub(crate) mod testutil;
