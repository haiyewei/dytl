//! DYTL — multi-platform live stream toolbox.
//!
//! Layered layout:
//! - [`cli`]     command parsing and user-facing subcommands
//! - [`config`]  YAML configuration loading and validation
//! - [`core`]    shared error, logging, paths, time, signals, JSON helpers
//! - [`media`]   ffmpeg recording, live-stream workflow, rescue merge
//! - [`monitor`] unified polling monitor and recording process lifecycle
//! - [`platform`] platform API clients (via external `amagi`)

mod cli;
mod config;
mod core;
mod media;
mod monitor;
mod platform;

fn main() {
    if let Err(err) = cli::run(std::env::args().collect()) {
        core::logger::error(err.to_string());
        std::process::exit(1);
    }
}
