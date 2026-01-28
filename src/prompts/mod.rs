//! Transition prompts system.
//!
//! Loads and delivers prompts when tasks transition between states/phases.
//! Prompts are defined in `workflows.yaml` with the following structure:
//!
//! - State prompts: `states.<state>.prompts.enter` / `states.<state>.prompts.exit`
//! - Phase prompts: `phases.<phase>.prompts.enter` / `phases.<phase>.prompts.exit`
//! - Combo prompts: `combos.<state>+<phase>.enter` / `combos.<state>+<phase>.exit`
//!
//! Trigger naming convention:
//! - `enter~{status}` - entering a status (any phase)
//! - `exit~{status}` - exiting a status (any phase)
//! - `enter%{phase}` - entering a phase (any status)
//! - `exit%{phase}` - exiting a phase (any status)
//! - `enter~{status}%{phase}` - entering specific status+phase combo
//! - `exit~{status}%{phase}` - exiting specific status+phase combo
//!
//! Template variables are expanded in prompts:
//! - `{{valid_exits}}` - valid states to transition to from current state
//! - `{{current_phase}}` - current phase if set
//! - `{{valid_phases}}` - list of valid phases that can be set
//! - `{{current_status}}` - current status name

use crate::config::workflows::WorkflowsConfig;
use crate::config::{PhasesConfig, StatesConfig};

/// Context for expanding template variables in prompts.
#[derive(Debug, Clone)]
pub struct PromptContext<'a> {
    /// Current status of the task
    pub status: &'a str,
    /// Current phase of the task (if any)
    pub phase: Option<&'a str>,
    /// States configuration for looking up valid transitions
    pub states_config: &'a StatesConfig,
    /// Phases configuration for listing valid phases
    pub phases_config: &'a PhasesConfig,
}

impl<'a> PromptContext<'a> {
    /// Create a new prompt context.
    pub fn new(
        status: &'a str,
        phase: Option<&'a str>,
        states_config: &'a StatesConfig,
        phases_config: &'a PhasesConfig,
    ) -> Self {
        Self {
            status,
            phase,
            states_config,
            phases_config,
        }
    }
}

/// Load a prompt by trigger name from WorkflowsConfig.
///
/// Returns None if no prompt exists for this trigger.
pub fn load_prompt(trigger: &str, workflows: &WorkflowsConfig) -> Option<String> {
    workflows.get_prompt(trigger).map(|s| s.to_string())
}

/// Expand template variables in a prompt string.
///
/// Supported variables:
/// - `{{valid_exits}}` - markdown list of valid exit states
/// - `{{current_phase}}` - current phase or "(none)" if not set
/// - `{{valid_phases}}` - comma-separated list of valid phases
/// - `{{current_status}}` - current status name
pub fn expand_prompt(content: &str, ctx: &PromptContext) -> String {
    let mut result = content.to_string();

    // Expand {{current_status}}
    result = result.replace("{{current_status}}", ctx.status);

    // Expand {{valid_exits}}
    if result.contains("{{valid_exits}}") {
        let exits = ctx.states_config.get_exits(ctx.status);
        let exits_md = if exits.is_empty() {
            "- _(no transitions available - terminal state)_".to_string()
        } else {
            exits
                .iter()
                .map(|s| format!("- `{}`", s))
                .collect::<Vec<_>>()
                .join("\n")
        };
        result = result.replace("{{valid_exits}}", &exits_md);
    }

    // Expand {{current_phase}}
    if result.contains("{{current_phase}}") {
        let phase_str = ctx
            .phase
            .map(|p| format!("`{}`", p))
            .unwrap_or_else(|| "_(none)_".to_string());
        result = result.replace("{{current_phase}}", &phase_str);
    }

    // Expand {{valid_phases}}
    if result.contains("{{valid_phases}}") {
        let mut phases: Vec<&str> = ctx.phases_config.phase_names();
        phases.sort();
        let phases_str = phases.join(", ");
        result = result.replace("{{valid_phases}}", &phases_str);
    }

    result
}

/// Get the list of triggers that should fire for a state transition.
///
/// Order: exits (specific → general), then enters (general → specific)
pub fn get_transition_triggers(
    old_status: &str,
    old_phase: Option<&str>,
    new_status: &str,
    new_phase: Option<&str>,
) -> Vec<String> {
    let mut triggers = Vec::new();

    let status_changed = old_status != new_status;
    let phase_changed = old_phase != new_phase;

    // === EXITS (specific → general) ===

    // Exit combo (if either changed and had a phase)
    if (status_changed || phase_changed) && old_phase.is_some() {
        if let Some(op) = old_phase {
            triggers.push(format!("exit~{}%{}", old_status, op));
        }
    }

    // Exit phase (if phase changed)
    if phase_changed {
        if let Some(op) = old_phase {
            triggers.push(format!("exit%{}", op));
        }
    }

    // Exit status (if status changed)
    if status_changed {
        triggers.push(format!("exit~{}", old_status));
    }

    // === ENTERS (general → specific) ===

    // Enter status (if status changed)
    if status_changed {
        triggers.push(format!("enter~{}", new_status));
    }

    // Enter phase (if phase changed)
    if phase_changed {
        if let Some(np) = new_phase {
            triggers.push(format!("enter%{}", np));
        }
    }

    // Enter combo (if either changed and has a phase)
    if (status_changed || phase_changed) && new_phase.is_some() {
        if let Some(np) = new_phase {
            triggers.push(format!("enter~{}%{}", new_status, np));
        }
    }

    triggers
}

/// Get all prompts that should be delivered for a state transition.
///
/// Returns a vector of prompt strings (caller concatenates as needed).
/// This version does NOT expand template variables - use `get_transition_prompts_with_context` for that.
pub fn get_transition_prompts(
    old_status: &str,
    old_phase: Option<&str>,
    new_status: &str,
    new_phase: Option<&str>,
    workflows: &WorkflowsConfig,
) -> Vec<String> {
    get_transition_triggers(old_status, old_phase, new_status, new_phase)
        .iter()
        .filter_map(|trigger| load_prompt(trigger, workflows))
        .collect()
}

/// Get all prompts that should be delivered for a state transition, with template expansion.
///
/// Returns a vector of prompt strings with template variables expanded.
pub fn get_transition_prompts_with_context(
    old_status: &str,
    old_phase: Option<&str>,
    new_status: &str,
    new_phase: Option<&str>,
    workflows: &WorkflowsConfig,
    ctx: &PromptContext,
) -> Vec<String> {
    get_transition_triggers(old_status, old_phase, new_status, new_phase)
        .iter()
        .filter_map(|trigger| load_prompt(trigger, workflows))
        .map(|content| expand_prompt(&content, ctx))
        .collect()
}

/// List all available prompt triggers from the workflows config.
pub fn list_available_prompts(workflows: &WorkflowsConfig) -> Vec<String> {
    workflows.list_prompt_triggers()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_triggers_status_change_only() {
        let triggers = get_transition_triggers("pending", None, "working", None);
        assert_eq!(triggers, vec!["exit~pending", "enter~working"]);
    }

    #[test]
    fn test_triggers_phase_change_only() {
        let triggers =
            get_transition_triggers("working", Some("diagnose"), "working", Some("review"));
        assert_eq!(
            triggers,
            vec![
                "exit~working%diagnose",
                "exit%diagnose",
                "enter%review",
                "enter~working%review"
            ]
        );
    }

    #[test]
    fn test_triggers_both_change() {
        let triggers =
            get_transition_triggers("working", Some("diagnose"), "finished", Some("review"));
        assert_eq!(
            triggers,
            vec![
                "exit~working%diagnose",
                "exit%diagnose",
                "exit~working",
                "enter~finished",
                "enter%review",
                "enter~finished%review"
            ]
        );
    }

    #[test]
    fn test_triggers_enter_phase_from_none() {
        let triggers = get_transition_triggers("working", None, "working", Some("diagnose"));
        assert_eq!(triggers, vec!["enter%diagnose", "enter~working%diagnose"]);
    }

    #[test]
    fn test_triggers_exit_phase_to_none() {
        let triggers = get_transition_triggers("working", Some("diagnose"), "working", None);
        assert_eq!(triggers, vec!["exit~working%diagnose", "exit%diagnose"]);
    }

    #[test]
    fn test_no_triggers_when_unchanged() {
        let triggers =
            get_transition_triggers("working", Some("diagnose"), "working", Some("diagnose"));
        assert!(triggers.is_empty());
    }

    #[test]
    fn test_expand_prompt_valid_exits() {
        let states_config = StatesConfig::default();
        let phases_config = PhasesConfig::default();
        let ctx = PromptContext::new("working", None, &states_config, &phases_config);

        let template = "From {{current_status}} you can go to:\n{{valid_exits}}";
        let result = expand_prompt(template, &ctx);

        assert!(result.contains("From working you can go to:"));
        assert!(result.contains("`completed`"));
        assert!(result.contains("`failed`"));
        assert!(result.contains("`pending`"));
    }

    #[test]
    fn test_expand_prompt_current_phase() {
        let states_config = StatesConfig::default();
        let phases_config = PhasesConfig::default();

        // With a phase
        let ctx = PromptContext::new("working", Some("implement"), &states_config, &phases_config);
        let template = "Phase: {{current_phase}}";
        let result = expand_prompt(template, &ctx);
        assert_eq!(result, "Phase: `implement`");

        // Without a phase
        let ctx = PromptContext::new("working", None, &states_config, &phases_config);
        let result = expand_prompt(template, &ctx);
        assert_eq!(result, "Phase: _(none)_");
    }

    #[test]
    fn test_expand_prompt_valid_phases() {
        let states_config = StatesConfig::default();
        let phases_config = PhasesConfig::default();
        let ctx = PromptContext::new("working", None, &states_config, &phases_config);

        let template = "Phases: {{valid_phases}}";
        let result = expand_prompt(template, &ctx);

        // Should contain various default phases
        assert!(result.contains("implement"));
        assert!(result.contains("test"));
        assert!(result.contains("review"));
    }

    #[test]
    fn test_expand_prompt_terminal_state() {
        let states_config = StatesConfig::default();
        let phases_config = PhasesConfig::default();
        let ctx = PromptContext::new("cancelled", None, &states_config, &phases_config);

        let template = "Exits: {{valid_exits}}";
        let result = expand_prompt(template, &ctx);

        // Cancelled is a terminal state (no exits)
        assert!(result.contains("no transitions available"));
    }

    #[test]
    fn test_load_prompt_from_workflows() {
        let workflows = WorkflowsConfig::default();

        // Should find enter~working
        let prompt = load_prompt("enter~working", &workflows);
        assert!(prompt.is_some());
        assert!(prompt.unwrap().contains("actively working"));

        // Should find enter%implement
        let prompt = load_prompt("enter%implement", &workflows);
        assert!(prompt.is_some());
        assert!(prompt.unwrap().contains("Implementation"));
    }

    #[test]
    fn test_get_transition_prompts() {
        let workflows = WorkflowsConfig::default();

        let prompts = get_transition_prompts("pending", None, "working", None, &workflows);

        // Should have at least the enter~working prompt
        assert!(!prompts.is_empty());
        assert!(prompts.iter().any(|p| p.contains("actively working")));
    }

    #[test]
    fn test_list_available_prompts() {
        let workflows = WorkflowsConfig::default();
        let prompts = list_available_prompts(&workflows);

        assert!(prompts.contains(&"enter~working".to_string()));
        assert!(prompts.contains(&"exit~working".to_string()));
        assert!(prompts.contains(&"enter%implement".to_string()));
    }
}
