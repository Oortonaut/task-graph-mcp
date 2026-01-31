//! Integration tests for the feedback tools (give_feedback and list_feedback).
//!
//! These tools write/read a human-readable markdown file in a directory. Tests use
//! a temporary directory so they are fully isolated and leave no artefacts.

use serde_json::json;
use task_graph_mcp::tools::feedback;
use tempfile::TempDir;

/// Create a fresh temporary directory for each test.
fn setup_dir() -> TempDir {
    TempDir::new().expect("Failed to create temp directory")
}

// ---------------------------------------------------------------------------
// give_feedback – happy path
// ---------------------------------------------------------------------------

mod give_feedback_tests {
    use super::*;

    #[test]
    fn happy_path_creates_file_and_returns_recorded() {
        let dir = setup_dir();

        let result = feedback::give_feedback(
            dir.path(),
            json!({
                "message": "The search tool is great!"
            }),
        )
        .expect("give_feedback should succeed");

        // Response shape
        assert_eq!(result["status"], "recorded");
        let file_path = result["file"]
            .as_str()
            .expect("file field should be a string");
        assert!(file_path.contains("feedback.md"));

        // File should exist on disk
        let content = std::fs::read_to_string(file_path).expect("feedback file should exist");
        assert!(content.contains("# Agent Feedback"), "should have header");
        assert!(
            content.contains("The search tool is great!"),
            "should contain the message"
        );
        // Defaults
        assert!(
            content.contains("general"),
            "default category should be general"
        );
        assert!(
            content.contains("neutral"),
            "default sentiment should be neutral"
        );
    }

    #[test]
    fn with_all_optional_fields() {
        let dir = setup_dir();

        let result = feedback::give_feedback(
            dir.path(),
            json!({
                "message": "Workflow needs improvement",
                "category": "workflow",
                "sentiment": "negative",
                "agent_id": "agent-42",
                "tool_name": "update",
                "task_id": "task-99"
            }),
        )
        .expect("give_feedback should succeed");

        assert_eq!(result["status"], "recorded");

        let content =
            std::fs::read_to_string(result["file"].as_str().unwrap()).expect("read feedback file");

        assert!(content.contains("workflow"));
        assert!(content.contains("negative"));
        assert!(content.contains("**Agent:** agent-42"));
        assert!(content.contains("**Tool:** update"));
        assert!(content.contains("**Task:** task-99"));
        assert!(content.contains("Workflow needs improvement"));
    }

    #[test]
    fn with_explicit_category_and_sentiment() {
        let dir = setup_dir();

        let result = feedback::give_feedback(
            dir.path(),
            json!({
                "message": "Config is easy to understand",
                "category": "config",
                "sentiment": "positive"
            }),
        )
        .expect("give_feedback should succeed");

        let content =
            std::fs::read_to_string(result["file"].as_str().unwrap()).expect("read feedback file");

        assert!(content.contains("config"));
        assert!(content.contains("positive"));
    }

    #[test]
    fn suggestion_sentiment_accepted() {
        let dir = setup_dir();

        let result = feedback::give_feedback(
            dir.path(),
            json!({
                "message": "It would be nice to have auto-complete",
                "sentiment": "suggestion"
            }),
        )
        .expect("give_feedback should succeed");

        let content =
            std::fs::read_to_string(result["file"].as_str().unwrap()).expect("read feedback file");

        assert!(content.contains("suggestion"));
    }

    // -----------------------------------------------------------------------
    // Validation errors
    // -----------------------------------------------------------------------

    #[test]
    fn missing_message_returns_error() {
        let dir = setup_dir();

        let result = feedback::give_feedback(dir.path(), json!({}));

        assert!(result.is_err(), "missing message should fail");
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("message"),
            "error should mention 'message': {}",
            err_msg
        );
    }

    #[test]
    fn empty_message_returns_error() {
        let dir = setup_dir();

        let result = feedback::give_feedback(
            dir.path(),
            json!({
                "message": ""
            }),
        );

        assert!(result.is_err(), "empty message should fail");
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.to_lowercase().contains("empty"),
            "error should mention empty: {}",
            err_msg
        );
    }

    #[test]
    fn whitespace_only_message_returns_error() {
        let dir = setup_dir();

        let result = feedback::give_feedback(
            dir.path(),
            json!({
                "message": "   \t\n  "
            }),
        );

        assert!(result.is_err(), "whitespace-only message should fail");
    }

    #[test]
    fn invalid_category_returns_error() {
        let dir = setup_dir();

        let result = feedback::give_feedback(
            dir.path(),
            json!({
                "message": "some feedback",
                "category": "nonexistent"
            }),
        );

        assert!(result.is_err(), "invalid category should fail");
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("nonexistent"),
            "error should mention the bad category: {}",
            err_msg
        );
    }

    #[test]
    fn invalid_sentiment_returns_error() {
        let dir = setup_dir();

        let result = feedback::give_feedback(
            dir.path(),
            json!({
                "message": "some feedback",
                "sentiment": "angry"
            }),
        );

        assert!(result.is_err(), "invalid sentiment should fail");
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("angry"),
            "error should mention the bad sentiment: {}",
            err_msg
        );
    }

    #[test]
    fn all_valid_categories_accepted() {
        let dir = setup_dir();
        let categories = ["tool", "workflow", "config", "ux", "general"];

        for cat in &categories {
            let result = feedback::give_feedback(
                dir.path(),
                json!({
                    "message": format!("testing {}", cat),
                    "category": cat
                }),
            );
            assert!(
                result.is_ok(),
                "category '{}' should be accepted but got error: {:?}",
                cat,
                result.err()
            );
        }
    }

    #[test]
    fn all_valid_sentiments_accepted() {
        let dir = setup_dir();
        let sentiments = ["positive", "negative", "neutral", "suggestion"];

        for s in &sentiments {
            let result = feedback::give_feedback(
                dir.path(),
                json!({
                    "message": format!("testing {}", s),
                    "sentiment": s
                }),
            );
            assert!(
                result.is_ok(),
                "sentiment '{}' should be accepted but got error: {:?}",
                s,
                result.err()
            );
        }
    }
}

// ---------------------------------------------------------------------------
// list_feedback
// ---------------------------------------------------------------------------

mod list_feedback_tests {
    use super::*;

    #[test]
    fn no_file_returns_empty_content() {
        let dir = setup_dir();

        let result = feedback::list_feedback(dir.path()).expect("list_feedback should succeed");

        assert_eq!(result["content"], "");
        assert_eq!(result["message"], "No feedback recorded yet.");
        let file_path = result["file"]
            .as_str()
            .expect("file field should be a string");
        assert!(file_path.contains("feedback.md"));
    }

    #[test]
    fn returns_content_after_giving_feedback() {
        let dir = setup_dir();

        // Give some feedback first
        feedback::give_feedback(
            dir.path(),
            json!({
                "message": "This is my feedback"
            }),
        )
        .expect("give_feedback should succeed");

        let result = feedback::list_feedback(dir.path()).expect("list_feedback should succeed");

        let content = result["content"]
            .as_str()
            .expect("content should be a string");
        assert!(!content.is_empty(), "content should not be empty");
        assert!(content.contains("# Agent Feedback"));
        assert!(content.contains("This is my feedback"));
        // Should not have the "no feedback" message
        assert!(result.get("message").is_none());
    }
}

// ---------------------------------------------------------------------------
// Multiple entries / append behaviour
// ---------------------------------------------------------------------------

mod append_tests {
    use super::*;

    #[test]
    fn multiple_entries_append_correctly() {
        let dir = setup_dir();

        // First entry
        feedback::give_feedback(
            dir.path(),
            json!({
                "message": "First feedback entry",
                "category": "tool",
                "sentiment": "positive"
            }),
        )
        .expect("first give_feedback should succeed");

        // Second entry
        feedback::give_feedback(
            dir.path(),
            json!({
                "message": "Second feedback entry",
                "category": "ux",
                "sentiment": "negative"
            }),
        )
        .expect("second give_feedback should succeed");

        // Third entry with optional metadata
        feedback::give_feedback(
            dir.path(),
            json!({
                "message": "Third entry with metadata",
                "agent_id": "worker-1",
                "tool_name": "search"
            }),
        )
        .expect("third give_feedback should succeed");

        // Read the file and verify all entries
        let result = feedback::list_feedback(dir.path()).expect("list_feedback should succeed");
        let content = result["content"]
            .as_str()
            .expect("content should be a string");

        // Header should appear only once
        assert_eq!(
            content.matches("# Agent Feedback").count(),
            1,
            "header should appear exactly once"
        );

        // Each entry is delimited by "---"
        // The implementation writes "---\n" before each entry, so we expect 3 separators.
        assert_eq!(
            content.matches("---").count(),
            3,
            "should have three separator lines for three entries"
        );

        // All three messages should be present
        assert!(content.contains("First feedback entry"));
        assert!(content.contains("Second feedback entry"));
        assert!(content.contains("Third entry with metadata"));

        // Category and sentiment from different entries
        assert!(content.contains("tool"));
        assert!(content.contains("positive"));
        assert!(content.contains("ux"));
        assert!(content.contains("negative"));

        // Metadata from third entry
        assert!(content.contains("**Agent:** worker-1"));
        assert!(content.contains("**Tool:** search"));
    }

    #[test]
    fn header_only_written_once_across_calls() {
        let dir = setup_dir();

        for i in 0..5 {
            feedback::give_feedback(
                dir.path(),
                json!({
                    "message": format!("entry {}", i)
                }),
            )
            .expect("give_feedback should succeed");
        }

        let result = feedback::list_feedback(dir.path()).expect("list_feedback should succeed");
        let content = result["content"].as_str().unwrap();

        assert_eq!(
            content.matches("# Agent Feedback").count(),
            1,
            "header should appear exactly once even after many writes"
        );

        // All five entries should be present
        for i in 0..5 {
            assert!(
                content.contains(&format!("entry {}", i)),
                "entry {} should be present",
                i
            );
        }
    }
}

// ---------------------------------------------------------------------------
// get_tools – tool definitions
// ---------------------------------------------------------------------------

mod tool_definition_tests {
    use super::*;

    #[test]
    fn get_tools_returns_two_tools() {
        let tools = feedback::get_tools();
        assert_eq!(tools.len(), 2, "should define exactly two feedback tools");

        let names: Vec<&str> = tools.iter().map(|t| t.name.as_ref()).collect();
        assert!(
            names.contains(&"give_feedback"),
            "should contain give_feedback"
        );
        assert!(
            names.contains(&"list_feedback"),
            "should contain list_feedback"
        );
    }
}
