//! Gate checking tools.
//!
//! Tools for pre-flight checking gate requirements before status/phase transitions.

use super::{get_string, make_tool_with_prompts};
use crate::config::Prompts;
use crate::config::workflows::WorkflowsConfig;
use crate::db::Database;
use crate::error::ToolError;
use crate::gates::{GateResult, evaluate_gates};
use anyhow::Result;
use rmcp::model::Tool;
use serde_json::{Value, json};

/// Get the gate-related tools.
pub fn get_tools(prompts: &Prompts) -> Vec<Tool> {
    vec![make_tool_with_prompts(
        "check_gates",
        "Check gate requirements for a task before attempting a status/phase transition. Returns unsatisfied gates with overall status (pass/warn/fail).",
        json!({
            "task": {
                "type": "string",
                "description": "Task ID to check gates for"
            }
        }),
        vec!["task"],
        prompts,
    )]
}

/// Check gates for a task.
///
/// This tool allows agents to pre-flight check gate requirements before attempting
/// a status/phase transition. It evaluates all exit gates for the task's current
/// status and phase, returning only unsatisfied gates.
///
/// # Response format
/// ```json
/// {
///   "status": "pass" | "warn" | "fail",
///   "gates": [
///     {
///       "type": "gate/tests",
///       "enforcement": "reject",
///       "description": "Attach test results",
///       "satisfied": false
///     }
///   ]
/// }
/// ```
///
/// - "pass" = all gates satisfied OR only allow gates missing
/// - "warn" = some warn gates missing (would need force=true)
/// - "fail" = some reject gates missing (cannot proceed)
pub fn check_gates(db: &Database, workflows: &WorkflowsConfig, args: Value) -> Result<Value> {
    let task_id = get_string(&args, "task").ok_or_else(|| ToolError::missing_field("task"))?;

    // Get the task to find current status and phase
    let task = db
        .get_task(&task_id)?
        .ok_or_else(|| ToolError::new(crate::error::ErrorCode::TaskNotFound, "Task not found"))?;

    // Collect all applicable gates (status + phase exit gates)
    let mut all_gates = Vec::new();

    // Get exit gates for current status
    let status_gates = workflows.get_status_exit_gates(&task.status);
    all_gates.extend(status_gates.into_iter().cloned());

    // Get exit gates for current phase (if set)
    if let Some(ref phase) = task.phase {
        let phase_gates = workflows.get_phase_exit_gates(phase);
        all_gates.extend(phase_gates.into_iter().cloned());
    }

    // Evaluate gates
    let result = evaluate_gates(db, &task_id, &all_gates)?;

    // Build response in the required format
    let gates: Vec<Value> = result
        .unsatisfied_gates
        .iter()
        .map(gate_result_to_json)
        .collect();

    Ok(json!({
        "status": result.status,
        "gates": gates
    }))
}

/// Convert a GateResult to the response JSON format.
fn gate_result_to_json(gate: &GateResult) -> Value {
    json!({
        "type": gate.gate_type,
        "enforcement": gate.enforcement,
        "description": gate.description,
        "satisfied": gate.satisfied
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gate_result_to_json() {
        use crate::config::GateEnforcement;

        let gate = GateResult {
            gate_type: "gate/tests".to_string(),
            enforcement: GateEnforcement::Reject,
            description: "Attach test results".to_string(),
            satisfied: false,
        };

        let json = gate_result_to_json(&gate);
        assert_eq!(json["type"], "gate/tests");
        assert_eq!(json["enforcement"], "reject");
        assert_eq!(json["description"], "Attach test results");
        assert_eq!(json["satisfied"], false);
    }
}
