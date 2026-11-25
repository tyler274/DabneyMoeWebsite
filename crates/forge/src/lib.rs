//! git.dabney.moe (COD-103): a pure-Rust Git forge and pull-request
//! coordinator. Git objects, packs, refs, diffs and merges all go through
//! gitoxide; no code path spawns the `git` executable.

pub mod api;
pub mod app;
pub mod auth;
pub mod db;
pub mod diff;
pub mod error;
pub mod git;
pub mod merge;
pub mod mirror;
pub mod pr;
pub mod repo;
pub mod ui;
pub mod webhooks;

pub use app::{router, spawn_workers, App, Forge, ForgeConfig, WebhookTarget};
