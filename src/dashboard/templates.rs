//! HTML templates for the web dashboard.
//!
//! Templates are embedded at compile time using `include_str!`.
//! This module provides functions to serve the templates.

/// The base HTML template with navigation and layout.
pub const BASE_TEMPLATE: &str = include_str!("templates/base.html");

/// The index/home page template with dashboard overview.
pub const INDEX_TEMPLATE: &str = include_str!("templates/index.html");

/// The workers list page template.
pub const WORKERS_TEMPLATE: &str = include_str!("templates/workers.html");

/// The tasks list page template with filters and pagination.
pub const TASKS_TEMPLATE: &str = include_str!("templates/tasks.html");

/// The task detail page template with full task info and edit form.
pub const TASK_DETAIL_TEMPLATE: &str = include_str!("templates/task_detail.html");

/// The activity feed page template showing recent events.
pub const ACTIVITY_TEMPLATE: &str = include_str!("templates/activity.html");

/// The file marks coordination page template.
pub const FILE_MARKS_TEMPLATE: &str = include_str!("templates/file_marks.html");

/// The metrics dashboard page template with project health overview.
pub const METRICS_TEMPLATE: &str = include_str!("templates/metrics.html");

/// The dependency graph visualization page template.
pub const DEP_GRAPH_TEMPLATE: &str = include_str!("templates/dep_graph.html");

/// The SQL query interface page template for power users.
pub const SQL_QUERY_TEMPLATE: &str = include_str!("templates/sql_query.html");
