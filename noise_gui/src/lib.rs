//! The GUI as a library, so its state transforms and its parallel job runner
//! can be tested without a window. `main.rs` is a thin wrapper around this.

pub mod app;
pub mod export;
pub mod jobs;
pub mod state;
pub mod ui;
