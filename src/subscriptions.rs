//! MCP resource subscription manager.
//!
//! Tracks which resource URIs have been subscribed to by the connected MCP
//! client. When a tool call mutates state that corresponds to a subscribed
//! resource, the server sends a `notifications/resources/updated` notification
//! to the client so it can re-fetch the resource.
//!
//! Because MCP stdio transport is single-client, we only need to track one
//! peer's subscriptions. The manager stores the set of subscribed URIs and
//! provides a method to determine which URIs should be notified after a
//! particular category of mutation.

use std::collections::HashSet;
use std::sync::Mutex;

/// Categories of mutations that affect resources.
/// When a tool call completes, it reports which categories of data changed,
/// and the SubscriptionManager maps those to affected resource URIs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MutationKind {
    /// A task was created, updated, deleted, or had its status changed.
    TaskChanged,
    /// A dependency link was created, removed, or changed.
    DependencyChanged,
    /// A file mark was added or removed.
    FileMarkChanged,
    /// An agent connected, disconnected, or was cleaned up.
    AgentChanged,
    /// An attachment was added or removed.
    AttachmentChanged,
}

impl MutationKind {
    /// Return the set of resource URIs that are potentially affected by this
    /// kind of mutation.
    pub fn affected_uris(&self) -> &'static [&'static str] {
        match self {
            MutationKind::TaskChanged => &[
                "query://tasks/all",
                "query://tasks/ready",
                "query://tasks/blocked",
                "query://tasks/claimed",
                "query://stats/summary",
            ],
            MutationKind::DependencyChanged => &[
                "query://tasks/all",
                "query://tasks/ready",
                "query://tasks/blocked",
                "query://stats/summary",
            ],
            MutationKind::FileMarkChanged => &["query://files/marks"],
            MutationKind::AgentChanged => &[
                "query://agents/all",
                "query://tasks/claimed",
                "query://stats/summary",
            ],
            MutationKind::AttachmentChanged => &["query://tasks/all", "query://stats/summary"],
        }
    }
}

/// Manages resource subscriptions for the connected MCP client.
///
/// Thread-safe: uses an internal `Mutex` so it can be shared across async
/// tasks without requiring `&mut self`.
pub struct SubscriptionManager {
    /// Set of resource URIs the client has subscribed to.
    subscribed: Mutex<HashSet<String>>,
}

impl SubscriptionManager {
    /// Create a new empty subscription manager.
    pub fn new() -> Self {
        Self {
            subscribed: Mutex::new(HashSet::new()),
        }
    }

    /// Subscribe to a resource URI. Returns `true` if newly added.
    pub fn subscribe(&self, uri: &str) -> bool {
        let mut set = self.subscribed.lock().unwrap();
        set.insert(uri.to_string())
    }

    /// Unsubscribe from a resource URI. Returns `true` if was present.
    pub fn unsubscribe(&self, uri: &str) -> bool {
        let mut set = self.subscribed.lock().unwrap();
        set.remove(uri)
    }

    /// Check if any subscriptions are registered.
    pub fn has_subscriptions(&self) -> bool {
        let set = self.subscribed.lock().unwrap();
        !set.is_empty()
    }

    /// Given a set of mutation kinds, return the subscribed URIs that need
    /// notification. Only returns URIs that the client has actually subscribed to.
    pub fn affected_subscriptions(&self, mutations: &[MutationKind]) -> Vec<String> {
        let set = self.subscribed.lock().unwrap();
        if set.is_empty() {
            return Vec::new();
        }

        let mut result = HashSet::new();
        for kind in mutations {
            for uri in kind.affected_uris() {
                if set.contains(*uri) {
                    result.insert((*uri).to_string());
                }
            }
        }
        result.into_iter().collect()
    }
}

impl Default for SubscriptionManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_subscribe_unsubscribe() {
        let mgr = SubscriptionManager::new();
        assert!(!mgr.has_subscriptions());

        // Subscribe
        assert!(mgr.subscribe("query://tasks/all"));
        assert!(mgr.has_subscriptions());

        // Duplicate subscribe returns false
        assert!(!mgr.subscribe("query://tasks/all"));

        // Unsubscribe
        assert!(mgr.unsubscribe("query://tasks/all"));
        assert!(!mgr.has_subscriptions());

        // Unsubscribe missing returns false
        assert!(!mgr.unsubscribe("query://tasks/all"));
    }

    #[test]
    fn test_affected_subscriptions() {
        let mgr = SubscriptionManager::new();
        mgr.subscribe("query://tasks/all");
        mgr.subscribe("query://files/marks");

        // TaskChanged should include query://tasks/all but not query://files/marks
        let affected = mgr.affected_subscriptions(&[MutationKind::TaskChanged]);
        assert!(affected.contains(&"query://tasks/all".to_string()));
        assert!(!affected.contains(&"query://files/marks".to_string()));

        // FileMarkChanged should include query://files/marks
        let affected = mgr.affected_subscriptions(&[MutationKind::FileMarkChanged]);
        assert!(affected.contains(&"query://files/marks".to_string()));
        assert!(!affected.contains(&"query://tasks/all".to_string()));

        // Combined mutations
        let affected =
            mgr.affected_subscriptions(&[MutationKind::TaskChanged, MutationKind::FileMarkChanged]);
        assert!(affected.contains(&"query://tasks/all".to_string()));
        assert!(affected.contains(&"query://files/marks".to_string()));
    }

    #[test]
    fn test_no_subscriptions_returns_empty() {
        let mgr = SubscriptionManager::new();
        let affected = mgr.affected_subscriptions(&[MutationKind::TaskChanged]);
        assert!(affected.is_empty());
    }

    #[test]
    fn test_unsubscribed_uri_not_notified() {
        let mgr = SubscriptionManager::new();
        // Subscribe only to query://files/marks, not query://tasks/all
        mgr.subscribe("query://files/marks");

        let affected = mgr.affected_subscriptions(&[MutationKind::TaskChanged]);
        assert!(affected.is_empty()); // query://tasks/all is not subscribed
    }
}
