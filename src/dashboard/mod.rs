//! Web dashboard HTTP server module.
//!
//! This module provides an HTTP server for the web dashboard UI.
//! It is enabled when the `--ui=web` CLI option is passed.

mod server;
pub mod templates;

pub use server::{
    DashboardHandle, DashboardServer, DashboardStatus, start_server, start_server_with_retry,
};
