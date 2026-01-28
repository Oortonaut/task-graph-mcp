//! Gate evaluation logic for workflow transitions.
//!
//! Gates are checklist items that must be satisfied before transitioning out of
//! a status or phase. A gate is satisfied when the task has an attachment with
//! a matching type (e.g., "gate/tests", "gate/commit").

use crate::config::{GateDefinition, GateEnforcement};
use crate::db::Database;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// Result of evaluating a single gate.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GateResult {
    /// The attachment type that would satisfy this gate.
    pub gate_type: String,
    /// Enforcement level for this gate.
    pub enforcement: GateEnforcement,
    /// Human-readable description of what this gate requires.
    pub description: String,
    /// Whether the gate is satisfied (always false in unsatisfied_gates list).
    pub satisfied: bool,
}

/// Aggregated result of evaluating all gates for a transition.
#[derive(Debug, Serialize, Deserialize)]
pub struct GateCheckResult {
    /// Overall status: "pass", "warn", or "fail"
    pub status: String,
    /// Only unsatisfied gates are included
    pub unsatisfied_gates: Vec<GateResult>,
}

/// Evaluate gates for a task against its attachments.
/// Returns only unsatisfied gates.
///
/// # Arguments
/// * `db` - Database handle for fetching attachments
/// * `task_id` - The task ID to check gates for
/// * `gates` - List of gate definitions to evaluate
///
/// # Returns
/// A `GateCheckResult` with:
/// - `status`: "pass" if all gates satisfied, "warn" if only warn-level gates unsatisfied,
///   "fail" if any reject-level gates are unsatisfied
/// - `unsatisfied_gates`: List of gates that are not satisfied
pub fn evaluate_gates(
    db: &Database,
    task_id: &str,
    gates: &[GateDefinition],
) -> Result<GateCheckResult> {
    // Get all attachment types for this task
    let attachments = db.get_attachments(task_id)?;
    let attachment_types: HashSet<String> = attachments
        .iter()
        .map(|a| a.attachment_type.clone())
        .collect();

    let mut unsatisfied_gates = Vec::new();
    let mut has_reject = false;
    let mut has_warn = false;

    for gate in gates {
        let satisfied = attachment_types.contains(&gate.gate_type);

        if !satisfied {
            match gate.enforcement {
                GateEnforcement::Reject => has_reject = true,
                GateEnforcement::Warn => has_warn = true,
                GateEnforcement::Allow => {} // Still include in results but doesn't affect status
            }

            unsatisfied_gates.push(GateResult {
                gate_type: gate.gate_type.clone(),
                enforcement: gate.enforcement,
                description: gate.description.clone(),
                satisfied: false,
            });
        }
        // Satisfied gates are omitted from results per spec
    }

    let status = if has_reject {
        "fail".to_string()
    } else if has_warn {
        "warn".to_string()
    } else {
        "pass".to_string()
    };

    Ok(GateCheckResult {
        status,
        unsatisfied_gates,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gate_check_result_status_pass() {
        // Empty unsatisfied gates should result in "pass"
        let result = GateCheckResult {
            status: "pass".to_string(),
            unsatisfied_gates: vec![],
        };
        assert_eq!(result.status, "pass");
    }

    #[test]
    fn test_gate_result_serialization() {
        let gate = GateResult {
            gate_type: "gate/tests".to_string(),
            enforcement: GateEnforcement::Warn,
            description: "Tests must pass".to_string(),
            satisfied: false,
        };

        let json = serde_json::to_string(&gate).unwrap();
        assert!(json.contains("gate/tests"));
        assert!(json.contains("warn"));
    }
}
