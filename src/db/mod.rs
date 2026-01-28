//! Database layer for the Task Graph MCP Server.
//!
//! # Crash Safety
//!
//! This module uses mutex recovery to prevent cascading failures. If a thread
//! panics while holding the database mutex, subsequent operations will recover
//! the connection rather than panicking on poison errors.
//!
//! ## Known TODO items for improved robustness:
//!
//! - `src/db/tasks.rs:612,659` - `current_owner.unwrap()` could panic on inconsistent state
//! - `src/tools/tasks.rs:650` - `serde_json::to_value().unwrap()` could panic
//! - `src/tools/tracking.rs:34,215,338` - various unwraps on date/option handling
//! - `src/tools/attachments.rs:247,250` - `content.unwrap()` assumes content is Some
//! - `src/db/migrations.rs:323,556` - `expect()` on migration path validation

pub mod agents;
pub mod attachments;
pub mod dashboard;
pub mod deps;
pub mod export;
pub mod import;
pub mod locks;
pub mod schema;
pub mod search;
pub mod state_transitions;
pub mod stats;
pub mod tasks;

pub use deps::AddDependencyResult;
pub use search::{AttachmentMatch, SearchResult};

use anyhow::Result;
use rusqlite::Connection;
use std::path::Path;
use std::sync::{Arc, Mutex};

mod embedded {
    use refinery::embed_migrations;
    embed_migrations!("migrations");
}

/// Database handle wrapping a SQLite connection.
#[derive(Clone)]
pub struct Database {
    conn: Arc<Mutex<Connection>>,
}

impl Database {
    /// Open or create the database at the given path.
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let conn = Connection::open(path)?;

        // Enable WAL mode for concurrent access
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA foreign_keys=ON;
             PRAGMA busy_timeout=5000;",
        )?;

        let db = Self {
            conn: Arc::new(Mutex::new(conn)),
        };

        db.run_migrations()?;

        Ok(db)
    }

    /// Open an in-memory database (for testing).
    #[allow(dead_code)]
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;

        conn.execute_batch("PRAGMA foreign_keys=ON;")?;

        let db = Self {
            conn: Arc::new(Mutex::new(conn)),
        };

        db.run_migrations()?;

        Ok(db)
    }

    /// Run database migrations.
    fn run_migrations(&self) -> Result<()> {
        // Recover from poisoned mutex to prevent cascading failures
        let mut conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        embedded::migrations::runner().run(&mut *conn)?;
        Ok(())
    }

    /// Execute a function with exclusive access to the connection.
    ///
    /// Recovers from poisoned mutex to prevent cascading failures if another
    /// thread panicked while holding the lock.
    pub fn with_conn<F, T>(&self, f: F) -> Result<T>
    where
        F: FnOnce(&Connection) -> Result<T>,
    {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        f(&conn)
    }

    /// Execute a function with mutable access to the connection (for transactions).
    ///
    /// Recovers from poisoned mutex to prevent cascading failures if another
    /// thread panicked while holding the lock.
    pub fn with_conn_mut<F, T>(&self, f: F) -> Result<T>
    where
        F: FnOnce(&mut Connection) -> Result<T>,
    {
        let mut conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        f(&mut conn)
    }
}

/// Get the current timestamp in milliseconds.
pub fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}
