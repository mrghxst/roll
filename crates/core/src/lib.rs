//! Roll's core: the data model, database, scanning, sorting rules, the
//! organize planner, the journal and background jobs.

pub mod actions;
pub mod bestshot;
pub mod categories;
pub mod classify;
pub mod datefix;
pub mod dates;
pub mod db;
pub mod decide;
pub mod discover;
pub mod dupes;
pub mod edits;
pub mod embeddings;
pub mod events;
pub mod fsops;
pub mod groups;
pub mod immich;
pub mod import;
pub mod jobs;
pub mod journal;
pub mod library;
pub mod localai;
pub mod model;
pub mod offline;
pub mod packs;
pub mod paths;
pub mod people;
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
pub mod teach;
pub mod template;
pub mod thumbs;
pub mod trash;
pub mod vision;
pub mod watch;
pub mod why;

pub use library::{LibEvent, Library};

#[cfg(test)]
pub(crate) mod testutil;
