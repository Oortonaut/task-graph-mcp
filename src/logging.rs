//! Unified MCP-compatible logging system.
//!
//! This module provides a unified logger that outputs to multiple backends:
//! - CLI/stderr/file via tracing
//! - MCP client via `notify_logging_message`
//!
//! Uses MCP LoggingLevel as the canonical level type.

use rmcp::{
    RoleServer,
    model::{LoggingLevel, LoggingMessageNotificationParam},
    service::Peer,
};
use serde_json::{Value, json};
use std::sync::{
    Arc,
    atomic::{AtomicU8, Ordering},
};
use tracing::Level;

/// Atomic level filter that can be adjusted via `logging/setLevel`.
///
/// The level is stored as a u8 corresponding to LoggingLevel variants:
/// 0=Debug, 1=Info, 2=Notice, 3=Warning, 4=Error, 5=Critical, 6=Alert, 7=Emergency
pub struct LogLevelFilter(AtomicU8);

impl LogLevelFilter {
    /// Create a new filter with the given minimum level.
    pub fn new(level: LoggingLevel) -> Self {
        Self(AtomicU8::new(level_to_u8(level)))
    }

    /// Get the current minimum level.
    pub fn get(&self) -> LoggingLevel {
        u8_to_level(self.0.load(Ordering::Relaxed))
    }

    /// Set the minimum level.
    pub fn set(&self, level: LoggingLevel) {
        self.0.store(level_to_u8(level), Ordering::Relaxed);
    }

    /// Check if a message at the given level should be logged.
    pub fn should_log(&self, level: LoggingLevel) -> bool {
        level_to_u8(level) >= self.0.load(Ordering::Relaxed)
    }
}

impl Default for LogLevelFilter {
    fn default() -> Self {
        Self::new(LoggingLevel::Debug)
    }
}

/// Convert LoggingLevel to u8 for atomic storage.
fn level_to_u8(level: LoggingLevel) -> u8 {
    match level {
        LoggingLevel::Debug => 0,
        LoggingLevel::Info => 1,
        LoggingLevel::Notice => 2,
        LoggingLevel::Warning => 3,
        LoggingLevel::Error => 4,
        LoggingLevel::Critical => 5,
        LoggingLevel::Alert => 6,
        LoggingLevel::Emergency => 7,
    }
}

/// Convert u8 back to LoggingLevel.
fn u8_to_level(val: u8) -> LoggingLevel {
    match val {
        0 => LoggingLevel::Debug,
        1 => LoggingLevel::Info,
        2 => LoggingLevel::Notice,
        3 => LoggingLevel::Warning,
        4 => LoggingLevel::Error,
        5 => LoggingLevel::Critical,
        6 => LoggingLevel::Alert,
        7 => LoggingLevel::Emergency,
        _ => LoggingLevel::Debug,
    }
}

/// Convert MCP LoggingLevel to tracing Level.
pub fn logging_level_to_tracing(level: LoggingLevel) -> Level {
    match level {
        LoggingLevel::Debug => Level::DEBUG,
        LoggingLevel::Info | LoggingLevel::Notice => Level::INFO,
        LoggingLevel::Warning => Level::WARN,
        LoggingLevel::Error
        | LoggingLevel::Critical
        | LoggingLevel::Alert
        | LoggingLevel::Emergency => Level::ERROR,
    }
}

/// Unified logger that outputs to multiple backends.
///
/// Outputs to:
/// 1. tracing (stderr/file) - always
/// 2. MCP client (via peer.notify_logging_message) - if peer is set
#[derive(Clone)]
pub struct Logger {
    /// MCP peer for client notifications (optional).
    peer: Option<Peer<RoleServer>>,
    /// Minimum level to log.
    level_filter: Arc<LogLevelFilter>,
    /// Logger name/category.
    name: Option<String>,
}

impl Logger {
    /// Create a new logger with default settings.
    pub fn new() -> Self {
        Self {
            peer: None,
            level_filter: Arc::new(LogLevelFilter::default()),
            name: None,
        }
    }

    /// Set the MCP peer for client notifications.
    pub fn with_peer(mut self, peer: Peer<RoleServer>) -> Self {
        self.peer = Some(peer);
        self
    }

    /// Set the level filter.
    pub fn with_level_filter(mut self, filter: Arc<LogLevelFilter>) -> Self {
        self.level_filter = filter;
        self
    }

    /// Set the logger name.
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Log a message to all configured endpoints.
    pub fn log(&self, level: LoggingLevel, message: &str, data: Option<Value>) {
        if !self.level_filter.should_log(level) {
            return;
        }

        // 1. Output to tracing (stderr/file)
        let tracing_level = logging_level_to_tracing(level);
        match tracing_level {
            Level::ERROR => {
                if let Some(ref name) = self.name {
                    tracing::error!(logger = %name, "{}", message);
                } else {
                    tracing::error!("{}", message);
                }
            }
            Level::WARN => {
                if let Some(ref name) = self.name {
                    tracing::warn!(logger = %name, "{}", message);
                } else {
                    tracing::warn!("{}", message);
                }
            }
            Level::INFO => {
                if let Some(ref name) = self.name {
                    tracing::info!(logger = %name, "{}", message);
                } else {
                    tracing::info!("{}", message);
                }
            }
            Level::DEBUG => {
                if let Some(ref name) = self.name {
                    tracing::debug!(logger = %name, "{}", message);
                } else {
                    tracing::debug!("{}", message);
                }
            }
            Level::TRACE => {
                if let Some(ref name) = self.name {
                    tracing::trace!(logger = %name, "{}", message);
                } else {
                    tracing::trace!("{}", message);
                }
            }
        }

        // 2. Output to MCP client (if connected)
        if let Some(ref peer) = self.peer {
            let param = LoggingMessageNotificationParam {
                level,
                logger: self.name.clone(),
                data: data.unwrap_or_else(|| json!({ "message": message })),
            };
            let peer = peer.clone();
            tokio::spawn(async move {
                let _ = peer.notify_logging_message(param).await;
            });
        }
    }

    /// Log a message with structured data.
    pub fn log_with_data(&self, level: LoggingLevel, message: &str, data: Value) {
        self.log(level, message, Some(data));
    }

    // Convenience methods using MCP levels

    /// Log a debug message.
    pub fn debug(&self, msg: &str) {
        self.log(LoggingLevel::Debug, msg, None);
    }

    /// Log an info message.
    pub fn info(&self, msg: &str) {
        self.log(LoggingLevel::Info, msg, None);
    }

    /// Log a notice message.
    pub fn notice(&self, msg: &str) {
        self.log(LoggingLevel::Notice, msg, None);
    }

    /// Log a warning message.
    pub fn warning(&self, msg: &str) {
        self.log(LoggingLevel::Warning, msg, None);
    }

    /// Log an error message.
    pub fn error(&self, msg: &str) {
        self.log(LoggingLevel::Error, msg, None);
    }

    /// Log a critical message.
    pub fn critical(&self, msg: &str) {
        self.log(LoggingLevel::Critical, msg, None);
    }

    /// Log an alert message.
    pub fn alert(&self, msg: &str) {
        self.log(LoggingLevel::Alert, msg, None);
    }

    /// Log an emergency message.
    pub fn emergency(&self, msg: &str) {
        self.log(LoggingLevel::Emergency, msg, None);
    }
}

impl Default for Logger {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_level_filter() {
        let filter = LogLevelFilter::new(LoggingLevel::Warning);

        // Should not log debug/info/notice
        assert!(!filter.should_log(LoggingLevel::Debug));
        assert!(!filter.should_log(LoggingLevel::Info));
        assert!(!filter.should_log(LoggingLevel::Notice));

        // Should log warning and above
        assert!(filter.should_log(LoggingLevel::Warning));
        assert!(filter.should_log(LoggingLevel::Error));
        assert!(filter.should_log(LoggingLevel::Critical));
        assert!(filter.should_log(LoggingLevel::Alert));
        assert!(filter.should_log(LoggingLevel::Emergency));
    }

    #[test]
    fn test_level_filter_update() {
        let filter = LogLevelFilter::new(LoggingLevel::Debug);
        assert!(filter.should_log(LoggingLevel::Debug));

        filter.set(LoggingLevel::Error);
        assert!(!filter.should_log(LoggingLevel::Debug));
        assert!(!filter.should_log(LoggingLevel::Warning));
        assert!(filter.should_log(LoggingLevel::Error));
    }

    #[test]
    fn test_logging_level_to_tracing() {
        assert_eq!(logging_level_to_tracing(LoggingLevel::Debug), Level::DEBUG);
        assert_eq!(logging_level_to_tracing(LoggingLevel::Info), Level::INFO);
        assert_eq!(logging_level_to_tracing(LoggingLevel::Notice), Level::INFO);
        assert_eq!(logging_level_to_tracing(LoggingLevel::Warning), Level::WARN);
        assert_eq!(logging_level_to_tracing(LoggingLevel::Error), Level::ERROR);
        assert_eq!(
            logging_level_to_tracing(LoggingLevel::Critical),
            Level::ERROR
        );
        assert_eq!(logging_level_to_tracing(LoggingLevel::Alert), Level::ERROR);
        assert_eq!(
            logging_level_to_tracing(LoggingLevel::Emergency),
            Level::ERROR
        );
    }

    #[test]
    fn test_level_roundtrip() {
        for level in [
            LoggingLevel::Debug,
            LoggingLevel::Info,
            LoggingLevel::Notice,
            LoggingLevel::Warning,
            LoggingLevel::Error,
            LoggingLevel::Critical,
            LoggingLevel::Alert,
            LoggingLevel::Emergency,
        ] {
            let filter = LogLevelFilter::new(level);
            assert_eq!(filter.get(), level);
        }
    }
}
