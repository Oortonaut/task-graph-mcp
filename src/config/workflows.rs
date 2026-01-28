//! Workflow configuration for states, phases, and transition prompts.
//!
//! This module defines the unified workflow configuration that combines:
//! - State definitions (exits, timed)
//! - Phase definitions
//! - Transition prompts (enter/exit for states, phases, and combos)

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

use super::types::{PhasesConfig, StateDefinition, StatesConfig, UnknownKeyBehavior};

/// Settings for workflow behavior.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowSettings {
    /// Default state for new tasks.
    #[serde(default = "default_initial_state")]
    pub initial_state: String,

    /// State for tasks when agent disconnects (must be untimed).
    #[serde(default = "default_disconnect_state")]
    pub disconnect_state: String,

    /// States that block dependent tasks (tasks in these states count as "not done").
    #[serde(default = "default_blocking_states")]
    pub blocking_states: Vec<String>,

    /// Behavior for unknown phase values (allow, warn, reject).
    #[serde(default)]
    pub unknown_phase: UnknownKeyBehavior,
}

fn default_initial_state() -> String {
    "pending".to_string()
}

fn default_disconnect_state() -> String {
    "pending".to_string()
}

fn default_blocking_states() -> Vec<String> {
    vec![
        "pending".to_string(),
        "assigned".to_string(),
        "working".to_string(),
    ]
}

impl Default for WorkflowSettings {
    fn default() -> Self {
        Self {
            initial_state: default_initial_state(),
            disconnect_state: default_disconnect_state(),
            blocking_states: default_blocking_states(),
            unknown_phase: UnknownKeyBehavior::default(),
        }
    }
}

/// Prompts for state/phase transitions.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TransitionPrompts {
    /// Prompt shown when entering this state/phase.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enter: Option<String>,

    /// Prompt shown when exiting this state/phase.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit: Option<String>,
}

/// Definition of a single state in the workflow.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateWorkflow {
    /// Allowed states to transition to from this state.
    #[serde(default)]
    pub exits: Vec<String>,

    /// Whether time spent in this state should be tracked.
    #[serde(default)]
    pub timed: bool,

    /// Prompts for entering/exiting this state.
    #[serde(default)]
    pub prompts: TransitionPrompts,
}

impl Default for StateWorkflow {
    fn default() -> Self {
        Self {
            exits: Vec::new(),
            timed: false,
            prompts: TransitionPrompts::default(),
        }
    }
}

/// Definition of a phase in the workflow.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PhaseWorkflow {
    /// Prompts for entering/exiting this phase.
    #[serde(default)]
    pub prompts: TransitionPrompts,
}

/// Prompts for state+phase combinations.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ComboPrompts {
    /// Prompt shown when entering this state+phase combination.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enter: Option<String>,

    /// Prompt shown when exiting this state+phase combination.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit: Option<String>,
}

/// Unified workflow configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowsConfig {
    /// Global workflow settings.
    #[serde(default)]
    pub settings: WorkflowSettings,

    /// State definitions with transitions, timing, and prompts.
    #[serde(default)]
    pub states: HashMap<String, StateWorkflow>,

    /// Phase definitions with prompts.
    #[serde(default)]
    pub phases: HashMap<String, PhaseWorkflow>,

    /// State+phase combination prompts (key format: "state+phase").
    #[serde(default)]
    pub combos: HashMap<String, ComboPrompts>,
}

impl Default for WorkflowsConfig {
    fn default() -> Self {
        Self {
            settings: WorkflowSettings::default(),
            states: default_state_workflows(),
            phases: default_phase_workflows(),
            combos: HashMap::new(),
        }
    }
}

/// Default state workflow definitions.
fn default_state_workflows() -> HashMap<String, StateWorkflow> {
    let mut states = HashMap::new();

    states.insert(
        "pending".to_string(),
        StateWorkflow {
            exits: vec![
                "assigned".to_string(),
                "working".to_string(),
                "cancelled".to_string(),
            ],
            timed: false,
            prompts: TransitionPrompts::default(),
        },
    );

    states.insert(
        "assigned".to_string(),
        StateWorkflow {
            exits: vec![
                "working".to_string(),
                "pending".to_string(),
                "cancelled".to_string(),
            ],
            timed: false,
            prompts: TransitionPrompts {
                enter: Some(
                    "A task has been assigned to you. Review and claim when ready.".to_string(),
                ),
                exit: None,
            },
        },
    );

    states.insert(
        "working".to_string(),
        StateWorkflow {
            exits: vec![
                "completed".to_string(),
                "failed".to_string(),
                "pending".to_string(),
            ],
            timed: true,
            prompts: TransitionPrompts {
                enter: Some(
                    r#"You are now actively working on this task. Keep your thinking updated regularly using the `thinking` tool to show progress and allow coordination with other agents.

## Valid Next States

From `{{current_status}}` you can transition to:
{{valid_exits}}

Use `update(status="completed")` when done, `update(status="failed")` if blocked, or `update(status="pending")` to release without completing.

## Phase

Current phase: {{current_phase}}

Valid phases: {{valid_phases}}

Set a phase with `update(phase="implement")` to categorize the type of work you're doing."#
                        .to_string(),
                ),
                exit: Some(
                    r#"Before leaving working state:
- [ ] Unmark any files you marked
- [ ] Attach results or notes
- [ ] Log costs with `log_metrics()`"#
                        .to_string(),
                ),
            },
        },
    );

    states.insert(
        "completed".to_string(),
        StateWorkflow {
            exits: vec!["pending".to_string()],
            timed: false,
            prompts: TransitionPrompts {
                enter: Some("Task completed. Results should be attached.".to_string()),
                exit: None,
            },
        },
    );

    states.insert(
        "failed".to_string(),
        StateWorkflow {
            exits: vec!["pending".to_string()],
            timed: false,
            prompts: TransitionPrompts {
                enter: Some(
                    r#"Task failed. Please document:
- What was attempted
- What blocked progress
- Suggested next steps"#
                        .to_string(),
                ),
                exit: None,
            },
        },
    );

    states.insert(
        "cancelled".to_string(),
        StateWorkflow {
            exits: Vec::new(),
            timed: false,
            prompts: TransitionPrompts::default(),
        },
    );

    states
}

/// Default phase workflow definitions.
fn default_phase_workflows() -> HashMap<String, PhaseWorkflow> {
    let mut phases = HashMap::new();

    // Phases with prompts
    phases.insert(
        "explore".to_string(),
        PhaseWorkflow {
            prompts: TransitionPrompts {
                enter: None,
                exit: Some(
                    "Capture exploration findings before moving on.\nAttach discoveries to parent task for sibling agents.".to_string(),
                ),
            },
        },
    );

    phases.insert(
        "implement".to_string(),
        PhaseWorkflow {
            prompts: TransitionPrompts {
                enter: Some("Implementation phase. Mark files before editing.".to_string()),
                exit: None,
            },
        },
    );

    phases.insert(
        "review".to_string(),
        PhaseWorkflow {
            prompts: TransitionPrompts {
                enter: Some(
                    r#"## Code Review Checklist
- [ ] Tests pass
- [ ] No new warnings
- [ ] Documentation updated"#
                        .to_string(),
                ),
                exit: None,
            },
        },
    );

    phases.insert(
        "test".to_string(),
        PhaseWorkflow {
            prompts: TransitionPrompts {
                enter: Some(
                    "Testing phase. Verify the implementation works correctly.".to_string(),
                ),
                exit: None,
            },
        },
    );

    phases.insert(
        "security".to_string(),
        PhaseWorkflow {
            prompts: TransitionPrompts {
                enter: Some(
                    r#"## Security Review
- [ ] Input validation
- [ ] Auth/authz checks
- [ ] No secrets in code"#
                        .to_string(),
                ),
                exit: None,
            },
        },
    );

    // Phases without prompts
    for phase in &[
        "deliver",
        "triage",
        "diagnose",
        "design",
        "plan",
        "doc",
        "integrate",
        "deploy",
        "monitor",
        "optimize",
    ] {
        phases.insert(phase.to_string(), PhaseWorkflow::default());
    }

    phases
}

impl WorkflowsConfig {
    /// Get the enter prompt for a state.
    pub fn get_state_enter_prompt(&self, state: &str) -> Option<&str> {
        self.states
            .get(state)
            .and_then(|s| s.prompts.enter.as_deref())
    }

    /// Get the exit prompt for a state.
    pub fn get_state_exit_prompt(&self, state: &str) -> Option<&str> {
        self.states
            .get(state)
            .and_then(|s| s.prompts.exit.as_deref())
    }

    /// Get the enter prompt for a phase.
    pub fn get_phase_enter_prompt(&self, phase: &str) -> Option<&str> {
        self.phases
            .get(phase)
            .and_then(|p| p.prompts.enter.as_deref())
    }

    /// Get the exit prompt for a phase.
    pub fn get_phase_exit_prompt(&self, phase: &str) -> Option<&str> {
        self.phases
            .get(phase)
            .and_then(|p| p.prompts.exit.as_deref())
    }

    /// Get the enter prompt for a state+phase combo.
    pub fn get_combo_enter_prompt(&self, state: &str, phase: &str) -> Option<&str> {
        let key = format!("{}+{}", state, phase);
        self.combos.get(&key).and_then(|c| c.enter.as_deref())
    }

    /// Get the exit prompt for a state+phase combo.
    pub fn get_combo_exit_prompt(&self, state: &str, phase: &str) -> Option<&str> {
        let key = format!("{}+{}", state, phase);
        self.combos.get(&key).and_then(|c| c.exit.as_deref())
    }

    /// Get a prompt by trigger name.
    ///
    /// Trigger format:
    /// - `enter~{state}` - entering a state
    /// - `exit~{state}` - exiting a state
    /// - `enter%{phase}` - entering a phase
    /// - `exit%{phase}` - exiting a phase
    /// - `enter~{state}%{phase}` - entering a state+phase combo
    /// - `exit~{state}%{phase}` - exiting a state+phase combo
    pub fn get_prompt(&self, trigger: &str) -> Option<&str> {
        if let Some(rest) = trigger.strip_prefix("enter~") {
            if let Some(idx) = rest.find('%') {
                // Combo: enter~state%phase
                let state = &rest[..idx];
                let phase = &rest[idx + 1..];
                self.get_combo_enter_prompt(state, phase)
            } else {
                // State: enter~state
                self.get_state_enter_prompt(rest)
            }
        } else if let Some(rest) = trigger.strip_prefix("exit~") {
            if let Some(idx) = rest.find('%') {
                // Combo: exit~state%phase
                let state = &rest[..idx];
                let phase = &rest[idx + 1..];
                self.get_combo_exit_prompt(state, phase)
            } else {
                // State: exit~state
                self.get_state_exit_prompt(rest)
            }
        } else if let Some(phase) = trigger.strip_prefix("enter%") {
            self.get_phase_enter_prompt(phase)
        } else if let Some(phase) = trigger.strip_prefix("exit%") {
            self.get_phase_exit_prompt(phase)
        } else {
            None
        }
    }

    /// List all available prompt triggers.
    pub fn list_prompt_triggers(&self) -> Vec<String> {
        let mut triggers = Vec::new();

        // State prompts
        for (state, workflow) in &self.states {
            if workflow.prompts.enter.is_some() {
                triggers.push(format!("enter~{}", state));
            }
            if workflow.prompts.exit.is_some() {
                triggers.push(format!("exit~{}", state));
            }
        }

        // Phase prompts
        for (phase, workflow) in &self.phases {
            if workflow.prompts.enter.is_some() {
                triggers.push(format!("enter%{}", phase));
            }
            if workflow.prompts.exit.is_some() {
                triggers.push(format!("exit%{}", phase));
            }
        }

        // Combo prompts
        for (combo, prompts) in &self.combos {
            if prompts.enter.is_some() {
                triggers.push(format!("enter~{}", combo.replace('+', "%")));
            }
            if prompts.exit.is_some() {
                triggers.push(format!("exit~{}", combo.replace('+', "%")));
            }
        }

        triggers.sort();
        triggers
    }
}

/// Convert WorkflowsConfig to StatesConfig for backwards compatibility.
impl From<&WorkflowsConfig> for StatesConfig {
    fn from(workflows: &WorkflowsConfig) -> Self {
        let definitions = workflows
            .states
            .iter()
            .map(|(name, workflow)| {
                (
                    name.clone(),
                    StateDefinition {
                        exits: workflow.exits.clone(),
                        timed: workflow.timed,
                    },
                )
            })
            .collect();

        StatesConfig {
            initial: workflows.settings.initial_state.clone(),
            disconnect_state: workflows.settings.disconnect_state.clone(),
            blocking_states: workflows.settings.blocking_states.clone(),
            definitions,
        }
    }
}

/// Convert WorkflowsConfig to PhasesConfig for backwards compatibility.
impl From<&WorkflowsConfig> for PhasesConfig {
    fn from(workflows: &WorkflowsConfig) -> Self {
        let definitions: HashSet<String> = workflows.phases.keys().cloned().collect();

        PhasesConfig {
            unknown_phase: workflows.settings.unknown_phase.clone(),
            definitions,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_workflows() {
        let workflows = WorkflowsConfig::default();

        // Check settings
        assert_eq!(workflows.settings.initial_state, "pending");
        assert_eq!(workflows.settings.disconnect_state, "pending");
        assert!(
            workflows
                .settings
                .blocking_states
                .contains(&"working".to_string())
        );

        // Check states
        assert!(workflows.states.contains_key("pending"));
        assert!(workflows.states.contains_key("working"));
        assert!(workflows.states.contains_key("completed"));

        // Check working is timed
        assert!(workflows.states.get("working").unwrap().timed);

        // Check phases
        assert!(workflows.phases.contains_key("implement"));
        assert!(workflows.phases.contains_key("test"));
    }

    #[test]
    fn test_get_prompt() {
        let workflows = WorkflowsConfig::default();

        // State enter prompt
        let prompt = workflows.get_prompt("enter~working");
        assert!(prompt.is_some());
        assert!(prompt.unwrap().contains("actively working"));

        // State exit prompt
        let prompt = workflows.get_prompt("exit~working");
        assert!(prompt.is_some());
        assert!(prompt.unwrap().contains("Unmark"));

        // Phase enter prompt
        let prompt = workflows.get_prompt("enter%implement");
        assert!(prompt.is_some());
        assert!(prompt.unwrap().contains("Implementation"));

        // Phase exit prompt
        let prompt = workflows.get_prompt("exit%explore");
        assert!(prompt.is_some());
        assert!(prompt.unwrap().contains("findings"));
    }

    #[test]
    fn test_states_config_from_workflows() {
        let workflows = WorkflowsConfig::default();
        let states: StatesConfig = (&workflows).into();

        assert_eq!(states.initial, "pending");
        assert!(states.definitions.contains_key("working"));
        assert!(states.definitions.get("working").unwrap().timed);
    }

    #[test]
    fn test_phases_config_from_workflows() {
        let workflows = WorkflowsConfig::default();
        let phases: PhasesConfig = (&workflows).into();

        assert!(phases.definitions.contains("implement"));
        assert!(phases.definitions.contains("test"));
    }

    #[test]
    fn test_list_prompt_triggers() {
        let workflows = WorkflowsConfig::default();
        let triggers = workflows.list_prompt_triggers();

        assert!(triggers.contains(&"enter~working".to_string()));
        assert!(triggers.contains(&"exit~working".to_string()));
        assert!(triggers.contains(&"enter%implement".to_string()));
    }
}
