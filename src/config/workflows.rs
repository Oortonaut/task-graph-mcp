//! Workflow configuration for states, phases, and transition prompts.
//!
//! This module defines the unified workflow configuration that combines:
//! - State definitions (exits, timed)
//! - Phase definitions
//! - Transition prompts (enter/exit for states, phases, and combos)

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use super::types::{
    GateDefinition, PhasesConfig, StateDefinition, StatesConfig, UnknownKeyBehavior,
};

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
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
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

/// Definition of a role in a workflow (e.g., "lead", "worker").
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RoleDefinition {
    /// Human-readable description of this role.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Tags that identify agents in this role.
    #[serde(default)]
    pub tags: Vec<String>,

    /// Maximum number of tasks this role can claim simultaneously.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_claims: Option<u32>,

    /// Whether this role can assign tasks to other agents.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub can_assign: Option<bool>,

    /// Whether this role can create subtasks.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub can_create_subtasks: Option<bool>,
}

/// Unified workflow configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowsConfig {
    /// Short identifier for the workflow (e.g., "swarm", "relay", "solo").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Human-readable description of the workflow's coordination model.
    /// Should explain when to choose this workflow and how agents coordinate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Path to the source file this workflow was loaded from.
    /// Not deserialized from YAML - populated by the loader.
    #[serde(skip)]
    pub source_file: Option<std::path::PathBuf>,

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

    /// Gate definitions for status and phase exits.
    /// Keys are "status:<name>" or "phase:<name>", values are lists of gate definitions.
    #[serde(default)]
    pub gates: HashMap<String, Vec<GateDefinition>>,

    /// Role definitions (e.g., "lead", "worker") with tags, permissions, and constraints.
    #[serde(default)]
    pub roles: HashMap<String, RoleDefinition>,

    /// Role-specific prompts. Outer key is role name, inner key is prompt name
    /// (e.g., "claiming", "completing"), value is the prompt content.
    #[serde(default)]
    pub role_prompts: HashMap<String, HashMap<String, String>>,

    /// Cache of named workflow configs (e.g., "swarm" -> workflow-swarm.yaml).
    /// Populated at server startup, not serialized.
    #[serde(skip)]
    pub named_workflows: HashMap<String, Arc<WorkflowsConfig>>,

    /// Key to look up the default workflow in named_workflows cache.
    /// If set, workers without a workflow use this instead of the base config.
    #[serde(skip)]
    pub default_workflow_key: Option<String>,
}

impl Default for WorkflowsConfig {
    fn default() -> Self {
        Self {
            name: None,
            description: None,
            source_file: None,
            settings: WorkflowSettings::default(),
            states: default_state_workflows(),
            phases: default_phase_workflows(),
            combos: HashMap::new(),
            gates: HashMap::new(),
            roles: HashMap::new(),
            role_prompts: HashMap::new(),
            named_workflows: HashMap::new(),
            default_workflow_key: None,
        }
    }
}

impl WorkflowsConfig {
    /// Get a named workflow config, or None if not found.
    pub fn get_named_workflow(&self, name: &str) -> Option<&Arc<WorkflowsConfig>> {
        self.named_workflows.get(name)
    }

    /// Get the default workflow config from the cache, if one is configured.
    pub fn get_default_workflow(&self) -> Option<&Arc<WorkflowsConfig>> {
        self.default_workflow_key
            .as_ref()
            .and_then(|key| self.named_workflows.get(key))
    }

    /// Match worker tags to a role defined in this workflow.
    /// Returns the role name if any role's tags overlap with the worker's tags.
    /// If multiple roles match, returns the first match (by sorted key order for determinism).
    pub fn match_role(&self, worker_tags: &[String]) -> Option<String> {
        let mut role_names: Vec<&String> = self.roles.keys().collect();
        role_names.sort();
        for role_name in role_names {
            if let Some(role) = self.roles.get(role_name) {
                if role.tags.iter().any(|t| worker_tags.contains(t)) {
                    return Some(role_name.clone());
                }
            }
        }
        None
    }

    /// Get all prompts for a matched role.
    /// Returns an empty HashMap if the role has no prompts defined.
    pub fn get_role_prompts(&self, role_name: &str) -> HashMap<String, String> {
        self.role_prompts
            .get(role_name)
            .cloned()
            .unwrap_or_default()
    }

    /// Get a specific role prompt by role name and prompt key.
    pub fn get_role_prompt(&self, role_name: &str, prompt_key: &str) -> Option<&str> {
        self.role_prompts
            .get(role_name)
            .and_then(|prompts| prompts.get(prompt_key))
            .map(|s| s.as_str())
    }

    /// Get the role definition for a matched role.
    pub fn get_role(&self, role_name: &str) -> Option<&RoleDefinition> {
        self.roles.get(role_name)
    }

    /// Collect all unique role tags across this workflow and all named workflows.
    /// Returns a deduplicated list of tag names used in role definitions.
    pub fn all_role_tags(&self) -> Vec<String> {
        let mut tags = std::collections::HashSet::new();
        // Collect from this workflow's roles
        for role in self.roles.values() {
            for tag in &role.tags {
                tags.insert(tag.clone());
            }
        }
        // Collect from all named workflows
        for workflow in self.named_workflows.values() {
            for role in workflow.roles.values() {
                for tag in &role.tags {
                    tags.insert(tag.clone());
                }
            }
        }
        tags.into_iter().collect()
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

### Heartbeat & Coordination
- Call `thinking(agent=your_id, thought="...")` regularly to maintain heartbeat
- Call `mark_updates(agent=your_id)` every 30-60s during long operations to detect file conflicts
- Stale workers (no heartbeat for 5+ min) get evicted automatically
- The lead monitors worker heartbeats -- stay visible to avoid reassignment

## Valid Next States

From `working` you can transition to:
{{valid_exits}}

Use `update(status="completed")` when done, `update(status="failed")` if blocked, or `update(status="pending")` to release without completing.

## Phase

Current phase: {{current_phase}}

Valid phases: {{valid_phases}}

Set a phase with `update(phase="implement")` to categorize the type of work you're doing.
"#
                        .to_string(),
                ),
                exit: Some(
                    "Before completing:\n- [ ] Unmark files\n- [ ] Attach results or notes\n- [ ] `log_metrics()`".to_string(),
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
                    "Task failed. Document: what was attempted, what blocked, suggested next steps."
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
                enter: Some("Review: tests pass, no new warnings, docs updated.".to_string()),
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
                    "Security: input validation, auth/authz, no secrets in code.".to_string(),
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

    /// Get exit gates for a status transition.
    /// Returns gates defined under "status:<name>" key.
    pub fn get_status_exit_gates(&self, status: &str) -> Vec<&GateDefinition> {
        self.gates
            .get(&format!("status:{}", status))
            .map(|v| v.iter().collect())
            .unwrap_or_default()
    }

    /// Get exit gates for a phase transition.
    /// Returns gates defined under "phase:<name>" key.
    pub fn get_phase_exit_gates(&self, phase: &str) -> Vec<&GateDefinition> {
        self.gates
            .get(&format!("phase:{}", phase))
            .map(|v| v.iter().collect())
            .unwrap_or_default()
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
            unknown_phase: workflows.settings.unknown_phase,
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

    #[test]
    fn test_all_role_tags_from_base_config() {
        let mut workflows = WorkflowsConfig::default();
        workflows.roles.insert(
            "worker".to_string(),
            RoleDefinition {
                tags: vec!["worker".to_string(), "backend".to_string()],
                ..Default::default()
            },
        );
        workflows.roles.insert(
            "lead".to_string(),
            RoleDefinition {
                tags: vec!["lead".to_string(), "coordinator".to_string()],
                ..Default::default()
            },
        );

        let tags = workflows.all_role_tags();
        assert_eq!(tags.len(), 4);
        assert!(tags.contains(&"worker".to_string()));
        assert!(tags.contains(&"backend".to_string()));
        assert!(tags.contains(&"lead".to_string()));
        assert!(tags.contains(&"coordinator".to_string()));
    }

    #[test]
    fn test_all_role_tags_includes_named_workflows() {
        let mut workflows = WorkflowsConfig::default();

        // Add a named workflow with its own roles
        let mut named = WorkflowsConfig::default();
        named.roles.insert(
            "reviewer".to_string(),
            RoleDefinition {
                tags: vec!["reviewer".to_string()],
                ..Default::default()
            },
        );
        workflows
            .named_workflows
            .insert("review".to_string(), Arc::new(named));

        // Base has no roles, but named workflow does
        let tags = workflows.all_role_tags();
        assert_eq!(tags.len(), 1);
        assert!(tags.contains(&"reviewer".to_string()));
    }

    #[test]
    fn test_all_role_tags_deduplicates() {
        let mut workflows = WorkflowsConfig::default();
        workflows.roles.insert(
            "worker".to_string(),
            RoleDefinition {
                tags: vec!["shared-tag".to_string()],
                ..Default::default()
            },
        );

        let mut named = WorkflowsConfig::default();
        named.roles.insert(
            "builder".to_string(),
            RoleDefinition {
                tags: vec!["shared-tag".to_string()],
                ..Default::default()
            },
        );
        workflows
            .named_workflows
            .insert("build".to_string(), Arc::new(named));

        let tags = workflows.all_role_tags();
        assert_eq!(tags.len(), 1);
        assert!(tags.contains(&"shared-tag".to_string()));
    }
}
