//! HTTP server implementation for the web dashboard.
//!
//! This module provides the axum-based HTTP server that serves the dashboard UI
//! and exposes REST API endpoints.

use axum::{
    Router,
    extract::{Form, Path, Query, State},
    response::{Html, IntoResponse, Json, Redirect},
    routing::{get, post},
};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{oneshot, watch};
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;
use tracing::info;

use super::templates;
use crate::config::{StatesConfig, UiConfig};
use crate::db::Database;
use crate::db::dashboard::{ActivityListQuery, TaskListQuery};
use crate::db::now_ms;
use tracing::warn;

/// Dashboard server state shared across handlers.
#[derive(Clone)]
pub struct DashboardServer {
    /// Reference to the task database.
    db: Arc<Database>,
    /// Port the server is listening on.
    port: u16,
    /// States configuration for determining timed/untimed states.
    states_config: Arc<StatesConfig>,
}

impl DashboardServer {
    /// Create a new dashboard server instance.
    pub fn new(db: Arc<Database>, port: u16, states_config: Arc<StatesConfig>) -> Self {
        Self {
            db,
            port,
            states_config,
        }
    }

    /// Get the database reference.
    pub fn db(&self) -> &Arc<Database> {
        &self.db
    }

    /// Get the configured port.
    pub fn port(&self) -> u16 {
        self.port
    }

    /// Get the states configuration.
    pub fn states_config(&self) -> &StatesConfig {
        &self.states_config
    }
}

/// Health check response.
#[derive(serde::Serialize)]
struct HealthResponse {
    status: &'static str,
    version: &'static str,
}

/// Root endpoint - serves the dashboard index page with htmx.
async fn root() -> Html<&'static str> {
    Html(templates::INDEX_TEMPLATE)
}

/// Workers page - serves the workers list page.
async fn workers_page() -> Html<&'static str> {
    Html(templates::WORKERS_TEMPLATE)
}

/// Stats API endpoint for htmx - returns HTML fragment.
async fn api_stats(State(state): State<DashboardServer>) -> Html<String> {
    // Query task counts from database
    let (total, working, completed): (i64, i64, i64) =
        state.db().get_task_stats().unwrap_or_default();

    // Query worker count from database
    let worker_count: i64 = state.db().get_active_worker_count().unwrap_or_default();

    Html(format!(
        r#"
        <div class="grid grid-stats">
            <div class="card stat">
                <div class="stat-value">{}</div>
                <div class="stat-label">Total Tasks</div>
            </div>
            <div class="card stat">
                <div class="stat-value">{}</div>
                <div class="stat-label">Active Workers</div>
            </div>
            <div class="card stat">
                <div class="stat-value">{}</div>
                <div class="stat-label">In Progress</div>
            </div>
            <div class="card stat">
                <div class="stat-value">{}</div>
                <div class="stat-label">Completed</div>
            </div>
        </div>
    "#,
        total, worker_count, working, completed
    ))
}

/// Recent tasks API endpoint for htmx - returns HTML fragment.
async fn api_recent_tasks(State(state): State<DashboardServer>) -> Html<String> {
    let tasks = state.db().get_recent_tasks(5).unwrap_or_default();

    if tasks.is_empty() {
        return Html(r#"<div class="empty-state">No tasks found</div>"#.to_string());
    }

    let mut html = String::from(
        "<table><thead><tr><th>Task</th><th>Status</th><th>Priority</th></tr></thead><tbody>",
    );

    for task in tasks {
        let badge_class = match task.status.as_str() {
            "completed" => "badge-success",
            "working" => "badge-info",
            "failed" => "badge-error",
            "pending" => "badge-pending",
            _ => "badge-warning",
        };

        let title = task
            .title
            .as_deref()
            .filter(|t| !t.is_empty())
            .unwrap_or(&task.id);

        html.push_str(&format!(
            r#"<tr><td>{}</td><td><span class="badge {}">{}</span></td><td>{}</td></tr>"#,
            html_escape(title),
            badge_class,
            task.status,
            task.priority
        ));
    }

    html.push_str("</tbody></table>");
    Html(html)
}

/// Active workers API endpoint for htmx - returns HTML fragment.
async fn api_active_workers(State(state): State<DashboardServer>) -> Html<String> {
    let workers = state.db().get_active_workers().unwrap_or_default();

    if workers.is_empty() {
        return Html(r#"<div class="empty-state">No active workers</div>"#.to_string());
    }

    let mut html = String::from(
        "<table><thead><tr><th>Worker</th><th>Status</th><th>Claims</th></tr></thead><tbody>",
    );

    for worker in workers {
        html.push_str(&format!(
            r#"<tr><td><div class="worker-status"><span class="status-dot online"></span>{}</div></td><td>{}</td><td>{}</td></tr>"#,
            html_escape(&worker.id),
            worker.current_thought.as_deref().unwrap_or("idle"),
            worker.claim_count
        ));
    }

    html.push_str("</tbody></table>");
    Html(html)
}

/// Format milliseconds as human-readable time ago string.
fn format_time_ago(ms_ago: i64) -> (String, &'static str) {
    let seconds = ms_ago / 1000;
    let (text, class) = if seconds < 60 {
        (format!("{}s ago", seconds), "recent")
    } else if seconds < 3600 {
        (format!("{}m ago", seconds / 60), "recent")
    } else if seconds < 86400 {
        let hours = seconds / 3600;
        (
            format!("{}h ago", hours),
            if hours < 2 { "recent" } else { "stale" },
        )
    } else {
        (format!("{}d ago", seconds / 86400), "old")
    };
    (text, class)
}

/// Workers list API endpoint for htmx - returns HTML fragment with full worker details.
async fn api_workers_list(State(state): State<DashboardServer>) -> Html<String> {
    let workers = state.db().list_workers_info().unwrap_or_default();

    if workers.is_empty() {
        return Html(r#"<div class="empty-state">No workers registered</div>"#.to_string());
    }

    let now = now_ms();
    let mut html = String::from(
        r#"<table>
        <thead>
            <tr>
                <th></th>
                <th>Worker ID</th>
                <th>Tags</th>
                <th>Registered</th>
                <th>Last Heartbeat</th>
                <th>Claims</th>
            </tr>
        </thead>
        <tbody>"#,
    );

    for worker in &workers {
        // Determine worker status based on heartbeat
        let heartbeat_age = now - worker.last_heartbeat;
        let status_class = if heartbeat_age < 60_000 {
            "online"
        } else if heartbeat_age < 300_000 {
            "stale"
        } else {
            "offline"
        };

        // Format heartbeat time
        let (heartbeat_text, heartbeat_class) = format_time_ago(heartbeat_age);

        // Format registered time
        let registered_age = now - worker.registered_at;
        let (registered_text, _) = format_time_ago(registered_age);

        // Format tags
        let tags_html: String = worker
            .tags
            .iter()
            .map(|t| format!(r#"<span class="tag">{}</span>"#, html_escape(t)))
            .collect();

        // Escape worker ID for use in HTML attribute
        let worker_id_escaped = html_escape(&worker.id);
        let worker_id_attr = worker.id.replace('"', "&quot;").replace('\'', "&#39;");

        html.push_str(&format!(
            r#"<tr class="expandable-row" onclick="toggleWorkerDetail('{worker_id_attr}')">
                <td><span id="expand-icon-{worker_id_attr}" class="expand-icon">&#9654;</span></td>
                <td><div class="worker-status"><span class="status-dot {status_class}"></span>{worker_id_escaped}</div></td>
                <td>{tags_html}</td>
                <td><span class="time-ago">{registered_text}</span></td>
                <td><span class="time-ago {heartbeat_class}">{heartbeat_text}</span></td>
                <td>{claim_count}</td>
            </tr>
            <tr id="worker-detail-{worker_id_attr}" class="detail-row">
                <td colspan="6">
                    <div class="detail-content"
                         hx-get="/api/workers/{worker_id_attr}/details"
                         hx-trigger="load-details"
                         hx-swap="innerHTML">
                        <div class="empty-state">Loading details...</div>
                    </div>
                </td>
            </tr>"#,
            worker_id_attr = worker_id_attr,
            worker_id_escaped = worker_id_escaped,
            status_class = status_class,
            tags_html = if tags_html.is_empty() { "<span class=\"tag\">none</span>".to_string() } else { tags_html },
            registered_text = registered_text,
            heartbeat_text = heartbeat_text,
            heartbeat_class = heartbeat_class,
            claim_count = worker.claim_count,
        ));
    }

    html.push_str("</tbody></table>");
    Html(html)
}

/// Worker details API endpoint for htmx - returns HTML fragment with claims and file marks.
async fn api_worker_details(
    State(state): State<DashboardServer>,
    axum::extract::Path(worker_id): axum::extract::Path<String>,
) -> Html<String> {
    let mut html = String::new();

    // Get claimed tasks
    let tasks = state
        .db()
        .get_worker_claimed_tasks(&worker_id)
        .unwrap_or_default();

    html.push_str(r#"<div class="detail-section"><h3>Claimed Tasks</h3>"#);
    if tasks.is_empty() {
        html.push_str(r#"<div class="empty-state">No claimed tasks</div>"#);
    } else {
        html.push_str(r#"<ul class="detail-list">"#);
        for task in tasks {
            let title = task
                .title
                .as_deref()
                .filter(|t| !t.is_empty())
                .unwrap_or(&task.id);
            let thought = task
                .current_thought
                .as_deref()
                .map(|t| format!(r#" - <em>{}</em>"#, html_escape(t)))
                .unwrap_or_default();
            html.push_str(&format!(
                r#"<li><strong>{}</strong>{}</li>"#,
                html_escape(title),
                thought
            ));
        }
        html.push_str("</ul>");
    }
    html.push_str("</div>");

    // Get file marks
    let file_locks = state
        .db()
        .get_file_locks(None, Some(&worker_id), None)
        .unwrap_or_default();

    html.push_str(r#"<div class="detail-section"><h3>File Marks</h3>"#);
    if file_locks.is_empty() {
        html.push_str(r#"<div class="empty-state">No file marks</div>"#);
    } else {
        html.push_str(r#"<ul class="detail-list">"#);
        for (file_path, lock) in file_locks {
            let reason = lock
                .reason
                .as_deref()
                .map(|r| format!(r#" - {}"#, html_escape(r)))
                .unwrap_or_default();
            html.push_str(&format!(
                r#"<li><span class="file-path">{}</span>{}</li>"#,
                html_escape(&file_path),
                reason
            ));
        }
        html.push_str("</ul>");
    }
    html.push_str("</div>");

    // Add disconnect button
    let worker_id_attr = worker_id.replace('"', "&quot;").replace('\'', "&#39;");
    html.push_str(&format!(
        "<div class=\"detail-actions\">\
            <form hx-post=\"/api/workers/{worker_id}/disconnect\"\
                  hx-target=\"#workers-list\"\
                  hx-swap=\"innerHTML\"\
                  hx-confirm=\"Are you sure you want to disconnect this worker? Their claimed tasks will be released.\">\
                <label>Release tasks as:\
                    <select name=\"final_status\" class=\"select\">\
                        <option value=\"pending\" selected>Pending</option>\
                        <option value=\"failed\">Failed</option>\
                        <option value=\"cancelled\">Cancelled</option>\
                    </select>\
                </label>\
                <button type=\"submit\" class=\"btn btn-danger btn-sm\">Disconnect Worker</button>\
            </form>\
        </div>",
        worker_id = worker_id_attr,
    ));

    Html(html)
}

/// Form data for disconnect endpoint.
#[derive(Debug, serde::Deserialize)]
struct DisconnectForm {
    final_status: Option<String>,
}

/// Disconnect a worker - releases all claims and removes worker.
async fn api_worker_disconnect(
    State(state): State<DashboardServer>,
    Path(worker_id): Path<String>,
    Form(form): Form<DisconnectForm>,
) -> Html<String> {
    let final_status = form.final_status.as_deref().unwrap_or("pending");

    match state.db().unregister_worker(&worker_id, final_status) {
        Ok(summary) => {
            // Return updated workers list
            let workers = state.db().list_workers_info().unwrap_or_default();
            if workers.is_empty() {
                return Html(format!(
                    r#"<div class="empty-state">Worker '{}' disconnected. {} tasks released as {}. No workers remaining.</div>"#,
                    html_escape(&worker_id),
                    summary.tasks_released,
                    final_status
                ));
            }

            // Re-render the workers list (simplified - redirect to refresh)
            api_workers_list(State(state)).await
        }
        Err(e) => Html(format!(
            r#"<div class="empty-state" style="color: var(--accent);">Error disconnecting worker: {}</div>"#,
            html_escape(&e.to_string())
        )),
    }
}

/// Cleanup stale workers endpoint.
async fn api_workers_cleanup(State(state): State<DashboardServer>) -> Html<String> {
    // Default timeout: 5 minutes (300 seconds)
    let timeout_seconds = 300;
    let final_status = "pending";

    match state
        .db()
        .cleanup_stale_workers(timeout_seconds, final_status)
    {
        Ok(summary) => {
            if summary.workers_evicted == 0 {
                Html(r#"<span class="badge badge-info">No stale workers</span>"#.to_string())
            } else {
                Html(format!(
                    r#"<span class="badge badge-success">{} evicted, {} tasks released</span>"#,
                    summary.workers_evicted, summary.tasks_released
                ))
            }
        }
        Err(e) => Html(format!(
            r#"<span class="badge badge-error">Error: {}</span>"#,
            html_escape(&e.to_string())
        )),
    }
}

/// Tasks page - serves the tasks list page.
async fn tasks_page() -> Html<&'static str> {
    Html(templates::TASKS_TEMPLATE)
}

/// Activity page - serves the activity feed page.
async fn activity_page() -> Html<&'static str> {
    Html(templates::ACTIVITY_TEMPLATE)
}

/// Query parameters for activity list API.
#[derive(Debug, serde::Deserialize)]
struct ActivityListParams {
    event_type: Option<String>,
    status: Option<String>,
    worker: Option<String>,
    task: Option<String>,
    page: Option<i32>,
    limit: Option<i32>,
}

/// Activity stats API endpoint for htmx - returns HTML fragment.
async fn api_activity_stats(State(state): State<DashboardServer>) -> Html<String> {
    let stats =
        state
            .db()
            .get_activity_stats()
            .unwrap_or_else(|_| crate::db::dashboard::ActivityStats {
                total_events_24h: 0,
                transitions_24h: 0,
                file_events_24h: 0,
                active_workers: 0,
                events_by_status: std::collections::HashMap::new(),
            });

    Html(format!(
        r#"
        <div class="stats-row">
            <div class="stat-card">
                <div class="stat-value">{}</div>
                <div class="stat-label">Events (24h)</div>
            </div>
            <div class="stat-card">
                <div class="stat-value">{}</div>
                <div class="stat-label">Task Transitions</div>
            </div>
            <div class="stat-card">
                <div class="stat-value">{}</div>
                <div class="stat-label">File Events</div>
            </div>
            <div class="stat-card">
                <div class="stat-value">{}</div>
                <div class="stat-label">Active Workers</div>
            </div>
        </div>
    "#,
        stats.total_events_24h, stats.transitions_24h, stats.file_events_24h, stats.active_workers
    ))
}

/// Activity list API endpoint for htmx - returns HTML fragment.
async fn api_activity_list(
    State(state): State<DashboardServer>,
    Query(params): Query<ActivityListParams>,
) -> Html<String> {
    let query = ActivityListQuery {
        event_type: params.event_type.filter(|s| !s.is_empty()),
        status: params.status.filter(|s| !s.is_empty()),
        worker: params.worker.filter(|s| !s.is_empty()),
        task: params.task.filter(|s| !s.is_empty()),
        page: params.page.unwrap_or(1).max(1),
        limit: params.limit.unwrap_or(50).clamp(10, 100),
    };

    let result = match state.db().query_activity(&query) {
        Ok(r) => r,
        Err(_) => {
            return Html(r#"<div class="empty-state">Error loading activity</div>"#.to_string());
        }
    };

    if result.events.is_empty() {
        return Html(
            r#"<div class="empty-state">No activity matches the current filters</div>"#.to_string(),
        );
    }

    let now = now_ms();
    let mut html = String::from(r#"<div class="activity-feed">"#);

    for event in &result.events {
        let (event_icon, event_class, event_label) = match event.event_type {
            crate::db::dashboard::ActivityEventType::TaskTransition => {
                let status = event.to_status.as_deref().unwrap_or("unknown");
                let icon = match status {
                    "completed" => "&#10003;",
                    "working" => "&#9654;",
                    "pending" => "&#9679;",
                    "failed" => "&#10007;",
                    "cancelled" => "&#10008;",
                    "assigned" => "&#10148;",
                    _ => "&#8594;",
                };
                (icon, "event-type-transition", status)
            }
            crate::db::dashboard::ActivityEventType::FileClaim => {
                ("&#128274;", "event-type-claim", "claimed")
            }
            crate::db::dashboard::ActivityEventType::FileRelease => {
                ("&#128275;", "event-type-release", "released")
            }
        };

        // Build description
        let description = match event.event_type {
            crate::db::dashboard::ActivityEventType::TaskTransition => {
                let task_id = event.task_id.as_deref().unwrap_or("unknown");
                let task_short = if task_id.len() > 30 {
                    format!("{}...", &task_id[..27])
                } else {
                    task_id.to_string()
                };
                let status = event.to_status.as_deref().unwrap_or("unknown");
                format!(
                    r#"Task <a href="/tasks/{}" class="task-link">{}</a> transitioned to <span class="badge badge-{}">{}</span>"#,
                    html_escape(task_id),
                    html_escape(&task_short),
                    match status {
                        "completed" => "success",
                        "working" => "info",
                        "pending" => "pending",
                        "failed" => "error",
                        "cancelled" => "warning",
                        "assigned" => "assigned",
                        _ => "warning",
                    },
                    status
                )
            }
            crate::db::dashboard::ActivityEventType::FileClaim => {
                let file_path = event.file_path.as_deref().unwrap_or("unknown");
                let file_short = if file_path.len() > 40 {
                    format!("...{}", &file_path[file_path.len() - 37..])
                } else {
                    file_path.to_string()
                };
                format!(
                    r#"File <span class="file-path">{}</span> was marked"#,
                    html_escape(&file_short)
                )
            }
            crate::db::dashboard::ActivityEventType::FileRelease => {
                let file_path = event.file_path.as_deref().unwrap_or("unknown");
                let file_short = if file_path.len() > 40 {
                    format!("...{}", &file_path[file_path.len() - 37..])
                } else {
                    file_path.to_string()
                };
                format!(
                    r#"File <span class="file-path">{}</span> was unmarked"#,
                    html_escape(&file_short)
                )
            }
        };

        // Worker info
        let worker_html = match &event.worker_id {
            Some(worker) => format!(
                r#"by <a href="/workers" class="worker-link">{}</a>"#,
                html_escape(worker)
            ),
            None => String::new(),
        };

        // Reason if available
        let reason_html = match &event.reason {
            Some(reason) if !reason.is_empty() => format!(
                r#" <span class="activity-details">- {}</span>"#,
                html_escape(reason)
            ),
            _ => String::new(),
        };

        // Time ago
        let time_ago = format_time_ago(now - event.timestamp);

        html.push_str(&format!(
            r#"<div class="activity-item">
                <span class="event-type {event_class}">
                    <span class="event-icon">{event_icon}</span>
                    {event_label}
                </span>
                <div class="activity-meta">
                    <span class="activity-description">{description} {worker_html}</span>
                    {reason_html}
                </div>
                <span class="activity-time" data-timestamp="{timestamp}">{time_text}</span>
            </div>"#,
            event_class = event_class,
            event_icon = event_icon,
            event_label = event_label,
            description = description,
            worker_html = worker_html,
            reason_html = reason_html,
            timestamp = event.timestamp,
            time_text = time_ago.0,
        ));
    }

    html.push_str("</div>");

    // Pagination
    if result.total_pages > 1 {
        let start = ((result.page - 1) * result.limit + 1) as i64;
        let end = (start - 1 + result.events.len() as i64).min(result.total);

        html.push_str(&format!(
            r#"<div class="pagination">
                <div class="pagination-info">
                    Showing {start} - {end} of {total} events
                </div>
                <div class="pagination-controls">
                    <button onclick="goToPage(1)" {first_disabled}>First</button>
                    <button onclick="goToPage({prev_page})" {prev_disabled}>Prev</button>
                    <span class="page-number">{page}</span>
                    <button onclick="goToPage({next_page})" {next_disabled}>Next</button>
                    <button onclick="goToPage({total_pages})" {last_disabled}>Last</button>
                </div>
            </div>"#,
            start = start,
            end = end,
            total = result.total,
            page = result.page,
            prev_page = (result.page - 1).max(1),
            next_page = (result.page + 1).min(result.total_pages),
            total_pages = result.total_pages,
            first_disabled = if result.page <= 1 { "disabled" } else { "" },
            prev_disabled = if result.page <= 1 { "disabled" } else { "" },
            next_disabled = if result.page >= result.total_pages {
                "disabled"
            } else {
                ""
            },
            last_disabled = if result.page >= result.total_pages {
                "disabled"
            } else {
                ""
            },
        ));
    }

    Html(html)
}

/// Format a timestamp in milliseconds to a human-readable date string.
fn format_timestamp(ms: Option<i64>) -> String {
    match ms {
        Some(ts) => {
            use std::time::{Duration, UNIX_EPOCH};
            let datetime = UNIX_EPOCH + Duration::from_millis(ts as u64);
            // Format as ISO 8601
            let secs = datetime.duration_since(UNIX_EPOCH).unwrap().as_secs();
            let days = secs / 86400;
            let remaining = secs % 86400;
            let hours = remaining / 3600;
            let minutes = (remaining % 3600) / 60;
            let seconds = remaining % 60;
            // Approximate date from epoch days (not accounting for leap years precisely)
            let year = 1970 + (days / 365);
            let day_of_year = days % 365;
            let month = (day_of_year / 30).min(11) + 1;
            let day = (day_of_year % 30) + 1;
            format!(
                "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
                year, month, day, hours, minutes, seconds
            )
        }
        None => "-".to_string(),
    }
}

/// Task detail page - serves the full task view with edit form.
async fn task_detail_page(
    State(state): State<DashboardServer>,
    Path(task_id): Path<String>,
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    // Get task from database
    let task = match state.db().get_task(&task_id) {
        Ok(Some(t)) => t,
        Ok(None) => {
            return Html(format!(
                r#"<!DOCTYPE html><html><head><title>Task Not Found</title></head>
                <body style="background:#1a1a2e;color:#eaeaea;font-family:system-ui;padding:2rem;">
                <h1>Task Not Found</h1><p>Task with ID '{}' does not exist.</p>
                <a href="/tasks" style="color:#e94560;">Back to Tasks</a></body></html>"#,
                html_escape(&task_id)
            ));
        }
        Err(_) => {
            return Html(
                r#"<!DOCTYPE html><html><head><title>Error</title></head>
                <body style="background:#1a1a2e;color:#eaeaea;font-family:system-ui;padding:2rem;">
                <h1>Error</h1><p>Failed to load task.</p>
                <a href="/tasks" style="color:#e94560;">Back to Tasks</a></body></html>"#
                    .to_string(),
            );
        }
    };

    // Get parent task
    let parent_html = match state.db().get_parent(&task_id) {
        Ok(Some(parent_id)) => format!(
            r#"<a href="/tasks/{}">{}</a>"#,
            html_escape(&parent_id),
            html_escape(&parent_id)
        ),
        _ => "-".to_string(),
    };

    // Get blocked by (tasks that block this one)
    let blocked_by = state.db().get_blockers(&task_id).unwrap_or_default();
    let blocked_by_html = if blocked_by.is_empty() {
        r#"<li class="empty-state">No blocking dependencies</li>"#.to_string()
    } else {
        blocked_by
            .iter()
            .map(|id| {
                format!(
                    r#"<li><a href="/tasks/{}">{}</a></li>"#,
                    html_escape(id),
                    html_escape(id)
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };

    // Get blocks (tasks this one blocks)
    let blocks = state.db().get_blocking(&task_id).unwrap_or_default();
    let blocks_html = if blocks.is_empty() {
        r#"<li class="empty-state">No tasks blocked</li>"#.to_string()
    } else {
        blocks
            .iter()
            .map(|id| {
                format!(
                    r#"<li><a href="/tasks/{}">{}</a></li>"#,
                    html_escape(id),
                    html_escape(id)
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };

    // Status badge class
    let status_badge = match task.status.as_str() {
        "completed" => "badge-success",
        "working" => "badge-info",
        "failed" => "badge-error",
        "pending" => "badge-pending",
        "assigned" => "badge-info",
        "cancelled" => "badge-warning",
        _ => "badge-warning",
    };

    // Tags display
    let tags_html = if task.tags.is_empty() {
        "-".to_string()
    } else {
        task.tags
            .iter()
            .map(|t| format!(r#"<span class="tag">{}</span>"#, html_escape(t)))
            .collect::<Vec<_>>()
            .join(" ")
    };

    let tags_raw = task.tags.join(", ");

    // Owner display
    let owner_html = task
        .worker_id
        .as_deref()
        .map(html_escape)
        .unwrap_or_else(|| "-".to_string());

    // Title display
    let title = task.title.as_str();
    let title_display = if title.is_empty() { &task.id } else { title };

    // Description
    let description = task.description.as_deref().unwrap_or("");
    let description_escaped = html_escape(description);

    // Status select options
    let status_pending = if task.status == "pending" {
        "selected"
    } else {
        ""
    };
    let status_assigned = if task.status == "assigned" {
        "selected"
    } else {
        ""
    };
    let status_working = if task.status == "working" {
        "selected"
    } else {
        ""
    };
    let status_completed = if task.status == "completed" {
        "selected"
    } else {
        ""
    };
    let status_failed = if task.status == "failed" {
        "selected"
    } else {
        ""
    };
    let status_cancelled = if task.status == "cancelled" {
        "selected"
    } else {
        ""
    };

    // Check for message from form submission
    let message = params
        .get("msg")
        .map(|m| {
            let (class, text) = if let Some(stripped) = m.strip_prefix("success:") {
                ("message-success", stripped)
            } else if let Some(stripped) = m.strip_prefix("error:") {
                ("message-error", stripped)
            } else {
                ("message-success", m.as_str())
            };
            format!(
                r#"<div class="message {}">{}</div>"#,
                class,
                html_escape(text)
            )
        })
        .unwrap_or_default();

    // Load and render template
    let template = templates::TASK_DETAIL_TEMPLATE;
    let html = template
        .replace("{{task_id}}", &html_escape(&task.id))
        .replace("{{task_title}}", &html_escape(title_display))
        .replace("{{task_status}}", &task.status)
        .replace("{{status_badge}}", status_badge)
        .replace("{{task_priority}}", &task.priority.to_string())
        .replace("{{task_owner}}", &owner_html)
        .replace("{{task_parent}}", &parent_html)
        .replace("{{task_tags}}", &tags_html)
        .replace("{{task_tags_raw}}", &html_escape(&tags_raw))
        .replace("{{task_description}}", &description_escaped)
        .replace("{{task_description_raw}}", &html_escape(description))
        .replace("{{created_at}}", &format_timestamp(Some(task.created_at)))
        .replace("{{updated_at}}", &format_timestamp(Some(task.updated_at)))
        .replace("{{started_at}}", &format_timestamp(task.started_at))
        .replace("{{claimed_at}}", &format_timestamp(task.claimed_at))
        .replace("{{completed_at}}", &format_timestamp(task.completed_at))
        .replace("{{blocked_by}}", &blocked_by_html)
        .replace("{{blocks}}", &blocks_html)
        .replace("{{status_pending}}", status_pending)
        .replace("{{status_assigned}}", status_assigned)
        .replace("{{status_working}}", status_working)
        .replace("{{status_completed}}", status_completed)
        .replace("{{status_failed}}", status_failed)
        .replace("{{status_cancelled}}", status_cancelled)
        .replace("{{message}}", &message);

    Html(html)
}

/// Form data for task updates.
#[derive(Debug, serde::Deserialize)]
struct TaskUpdateForm {
    status: Option<String>,
    priority: Option<i32>,
    tags: Option<String>,
    description: Option<String>,
}

/// Handle task update form submission.
async fn task_update_handler(
    State(state): State<DashboardServer>,
    Path(task_id): Path<String>,
    Form(form): Form<TaskUpdateForm>,
) -> impl IntoResponse {
    // Parse tags from comma-separated string
    let new_tags: Option<Vec<String>> = form.tags.as_ref().map(|t| {
        t.split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect()
    });

    // Use dashboard-specific update method
    match state.db().dashboard_update_task(
        &task_id,
        form.status.as_deref(),
        form.priority,
        form.description.as_deref(),
        new_tags,
    ) {
        Ok(()) => Html(
            r#"<div class="message message-success">Task updated successfully</div>"#.to_string(),
        ),
        Err(e) => Html(format!(
            r#"<div class="message message-error">Failed to update task: {}</div>"#,
            html_escape(&e.to_string())
        )),
    }
}

/// Handle task deletion.
async fn task_delete_handler(
    State(state): State<DashboardServer>,
    Path(task_id): Path<String>,
) -> impl IntoResponse {
    match state.db().dashboard_delete_task(&task_id) {
        Ok(()) => {
            // Redirect to tasks list with success message
            Redirect::to("/tasks?deleted=1")
        }
        Err(_) => {
            // Redirect back to task with error message
            Redirect::to(&format!("/tasks/{}?msg=error:Delete+failed", task_id))
        }
    }
}

/// Request body for bulk operations.
#[derive(Debug, serde::Deserialize)]
struct BulkOperationRequest {
    action: String,
    task_ids: Vec<String>,
    status: Option<String>,
}

/// Response for bulk operations.
#[derive(Debug, serde::Serialize)]
struct BulkOperationResponse {
    success: bool,
    affected: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

/// Query parameters for task list.
#[derive(Debug, serde::Deserialize)]
struct TaskListParams {
    status: Option<String>,
    phase: Option<String>,
    tags: Option<String>,
    parent: Option<String>,
    owner: Option<String>,
    /// If "true", show only timed states. If "false", show only untimed. If absent/empty, show all.
    show_untimed: Option<String>,
    sort: Option<String>,
    page: Option<i32>,
    limit: Option<i32>,
}

/// Task list API endpoint for htmx - returns HTML fragment with table and pagination.
async fn api_tasks_list(
    State(state): State<DashboardServer>,
    Query(params): Query<TaskListParams>,
) -> Html<String> {
    // Parse sort parameter (format: "field_direction", e.g., "priority_desc")
    let (sort_by, sort_order) = params
        .sort
        .as_deref()
        .and_then(|s| s.rsplit_once('_'))
        .map(|(field, order)| (field.to_string(), order.to_string()))
        .unwrap_or_else(|| ("priority".to_string(), "desc".to_string()));

    // Determine timed filter based on show_untimed parameter
    // Default behavior: show only timed states (active work)
    // When show_untimed=true, show all states (no filter)
    let show_untimed = params
        .show_untimed
        .as_ref()
        .map(|s| s == "true" || s == "1")
        .unwrap_or(false);

    let (timed_filter, timed_states) = if show_untimed {
        // Show all tasks (no filter)
        (None, Vec::new())
    } else {
        // Show only timed states (default - focus on active work)
        let timed: Vec<String> = state
            .states_config()
            .state_names()
            .into_iter()
            .filter(|s| state.states_config().is_timed_state(s))
            .map(|s| s.to_string())
            .collect();
        (Some(true), timed)
    };

    let query = TaskListQuery {
        status: params.status.filter(|s| !s.is_empty()),
        phase: params.phase.filter(|s| !s.is_empty()),
        tags: params.tags.filter(|s| !s.is_empty()),
        parent: params.parent.filter(|s| !s.is_empty()),
        owner: params.owner.filter(|s| !s.is_empty()),
        timed_filter,
        timed_states,
        sort_by,
        sort_order,
        page: params.page.unwrap_or(1).max(1),
        limit: params.limit.unwrap_or(25).clamp(10, 100),
    };

    let result = match state.db().query_tasks(&query) {
        Ok(r) => r,
        Err(_) => return Html(r#"<div class="empty-state">Error loading tasks</div>"#.to_string()),
    };

    if result.tasks.is_empty() {
        return Html(
            r#"<div class="empty-state">No tasks match the current filters</div>"#.to_string(),
        );
    }

    let mut html = String::from(
        r#"<table>
        <thead>
            <tr>
                <th class="checkbox-col"><input type="checkbox" id="select-all-checkbox" class="task-checkbox" onchange="onSelectAllChange(this)"></th>
                <th class="sortable">ID</th>
                <th class="sortable">Title</th>
                <th class="sortable">Status</th>
                <th class="sortable">Priority</th>
                <th>Tags</th>
                <th>Owner</th>
            </tr>
        </thead>
        <tbody>"#,
    );

    for task in &result.tasks {
        let badge_class = match task.status.as_str() {
            "completed" => "badge-success",
            "working" => "badge-info",
            "failed" => "badge-error",
            "pending" => "badge-pending",
            "assigned" => "badge-assigned",
            "cancelled" => "badge-warning",
            _ => "badge-warning",
        };

        let priority_class = if task.priority >= 8 {
            "priority-high"
        } else if task.priority >= 4 {
            "priority-normal"
        } else {
            "priority-low"
        };

        let title_display = task
            .title
            .as_deref()
            .filter(|t| !t.is_empty())
            .map(|t| {
                if t.len() > 50 {
                    format!("{}...", &t[..47])
                } else {
                    t.to_string()
                }
            })
            .unwrap_or_else(|| "-".to_string());

        // Parse tags (stored as JSON array string)
        let tags_html = if task.tags.is_empty() || task.tags == "[]" {
            String::new()
        } else {
            // Try to parse as JSON array, fall back to displaying as-is
            match serde_json::from_str::<Vec<String>>(&task.tags) {
                Ok(tags) => tags
                    .iter()
                    .take(3) // Limit to 3 visible tags
                    .map(|t| format!(r#"<span class="tag">{}</span>"#, html_escape(t)))
                    .collect::<Vec<_>>()
                    .join(""),
                Err(_) => task.tags.clone(),
            }
        };

        let owner_display = task
            .worker_id
            .as_deref()
            .map(html_escape)
            .unwrap_or_else(|| "-".to_string());

        html.push_str(&format!(
            r#"<tr>
                <td class="checkbox-col"><input type="checkbox" class="task-checkbox" data-task-id="{id}" onchange="onTaskCheckboxChange(this, '{id}')"></td>
                <td class="task-id"><a href="/tasks/{id}">{id_short}</a></td>
                <td class="task-title" title="{title_full}">{title}</td>
                <td><span class="badge {badge_class}">{status}</span></td>
                <td class="{priority_class}">{priority}</td>
                <td class="task-tags">{tags}</td>
                <td>{owner}</td>
            </tr>"#,
            id = html_escape(&task.id),
            id_short = if task.id.len() > 20 { format!("{}...", &task.id[..17]) } else { task.id.clone() },
            title = html_escape(&title_display),
            title_full = html_escape(task.title.as_deref().unwrap_or("")),
            badge_class = badge_class,
            status = task.status,
            priority_class = priority_class,
            priority = task.priority,
            tags = tags_html,
            owner = owner_display,
        ));
    }

    html.push_str("</tbody></table>");

    // Pagination
    let start = ((result.page - 1) * result.limit + 1) as i64;
    let end = (start - 1 + result.tasks.len() as i64).min(result.total);

    html.push_str(&format!(
        r#"<div class="pagination">
            <div class="pagination-info">
                Showing {start} - {end} of {total} tasks
            </div>
            <div class="pagination-controls">
                <button onclick="goToPage(1)" {first_disabled}>First</button>
                <button onclick="goToPage({prev_page})" {prev_disabled}>Prev</button>
                <span class="page-number">{page}</span>
                <button onclick="goToPage({next_page})" {next_disabled}>Next</button>
                <button onclick="goToPage({total_pages})" {last_disabled}>Last</button>
            </div>
        </div>"#,
        start = start,
        end = end,
        total = result.total,
        page = result.page,
        prev_page = (result.page - 1).max(1),
        next_page = (result.page + 1).min(result.total_pages),
        total_pages = result.total_pages,
        first_disabled = if result.page <= 1 { "disabled" } else { "" },
        prev_disabled = if result.page <= 1 { "disabled" } else { "" },
        next_disabled = if result.page >= result.total_pages {
            "disabled"
        } else {
            ""
        },
        last_disabled = if result.page >= result.total_pages {
            "disabled"
        } else {
            ""
        },
    ));

    Html(html)
}

/// Query parameters for task search.
#[derive(Debug, serde::Deserialize)]
struct TaskSearchParams {
    q: Option<String>,
    status: Option<String>,
    limit: Option<i32>,
}

/// Task search API endpoint for htmx - returns HTML fragment with search results.
/// When query is empty, returns all tasks (filtered by status if provided).
async fn api_tasks_search(
    State(state): State<DashboardServer>,
    Query(params): Query<TaskSearchParams>,
) -> Html<String> {
    let query = params.q.filter(|s| !s.is_empty());

    // If no query text, fall back to listing all tasks with optional status filter
    if query.is_none() {
        let list_query = TaskListQuery {
            status: params.status.clone().filter(|s| !s.is_empty()),
            phase: None,
            tags: None,
            parent: None,
            owner: None,
            timed_filter: None, // Search shows all tasks
            timed_states: Vec::new(),
            sort_by: "priority".to_string(),
            sort_order: "desc".to_string(),
            page: 1,
            limit: params.limit.unwrap_or(50).clamp(10, 100),
        };

        let result = match state.db().query_tasks(&list_query) {
            Ok(r) => r,
            Err(_) => {
                return Html(r#"<div class="empty-state">Error loading tasks</div>"#.to_string());
            }
        };

        if result.tasks.is_empty() {
            return Html(r#"<div class="empty-state">No tasks found</div>"#.to_string());
        }

        // Return simple table without search-specific columns (score)
        let mut html = format!(
            r#"<div style="margin-bottom: 1rem; color: var(--text-secondary);">
                Showing {} tasks{}
            </div>
            <table>
            <thead>
                <tr>
                    <th>ID</th>
                    <th>Title</th>
                    <th>Status</th>
                    <th>Priority</th>
                </tr>
            </thead>
            <tbody>"#,
            result.tasks.len(),
            params
                .status
                .as_ref()
                .map(|s| format!(" (status: {})", s))
                .unwrap_or_default()
        );

        for task in &result.tasks {
            let badge_class = match task.status.as_str() {
                "completed" => "badge-success",
                "working" => "badge-info",
                "failed" => "badge-error",
                "pending" => "badge-pending",
                "assigned" => "badge-assigned",
                "cancelled" => "badge-warning",
                _ => "badge-warning",
            };

            let title_display = task
                .title
                .as_deref()
                .filter(|t| !t.is_empty())
                .map(|t| {
                    if t.len() > 60 {
                        format!("{}...", &t[..57])
                    } else {
                        t.to_string()
                    }
                })
                .unwrap_or_else(|| "-".to_string());

            html.push_str(&format!(
                r#"<tr>
                    <td class="task-id"><a href="/tasks/{id}">{id_short}</a></td>
                    <td class="task-title">{title}</td>
                    <td><span class="badge {badge_class}">{status}</span></td>
                    <td>{priority}</td>
                </tr>"#,
                id = html_escape(&task.id),
                id_short = if task.id.len() > 20 {
                    format!("{}...", &task.id[..17])
                } else {
                    task.id.clone()
                },
                title = html_escape(&title_display),
                badge_class = badge_class,
                status = task.status,
                priority = task.priority,
            ));
        }

        html.push_str("</tbody></table>");
        return Html(html);
    }

    let query = query.unwrap();

    let status_filter = params.status.as_deref().filter(|s| !s.is_empty());
    let limit = params.limit.unwrap_or(50).clamp(10, 100);

    let results = match state
        .db()
        .search_tasks(&query, Some(limit), 0, false, status_filter)
    {
        Ok(r) => r,
        Err(e) => {
            // Handle FTS5 query syntax errors gracefully
            let error_msg = e.to_string();
            if error_msg.contains("fts5")
                || error_msg.contains("syntax")
                || error_msg.contains("MATCH")
            {
                return Html(format!(
                    r#"<div class="empty-state">Invalid search syntax: {}<br><br>
                    <small>Try: simple words, "exact phrase", prefix*, title:word, AND/OR/NOT</small></div>"#,
                    html_escape(&error_msg)
                ));
            }
            return Html(format!(
                r#"<div class="empty-state">Search error: {}</div>"#,
                html_escape(&error_msg)
            ));
        }
    };

    if results.is_empty() {
        return Html(format!(
            r#"<div class="empty-state">No tasks found matching "{}"</div>"#,
            html_escape(&query)
        ));
    }

    let mut html = format!(
        r#"<div style="margin-bottom: 1rem; color: var(--text-secondary);">
            Found {} results for "{}"
        </div>
        <table>
        <thead>
            <tr>
                <th>ID</th>
                <th>Title/Snippet</th>
                <th>Status</th>
                <th>Score</th>
            </tr>
        </thead>
        <tbody>"#,
        results.len(),
        html_escape(&query)
    );

    for result in &results {
        let badge_class = match result.status.as_str() {
            "completed" => "badge-success",
            "working" => "badge-info",
            "failed" => "badge-error",
            "pending" => "badge-pending",
            "assigned" => "badge-assigned",
            "cancelled" => "badge-warning",
            _ => "badge-warning",
        };

        // Use title_snippet which has <mark> tags for highlighting
        let title_display = if result.title_snippet.is_empty() {
            html_escape(&result.title)
        } else {
            // title_snippet already has HTML <mark> tags, so don't escape the marks
            result
                .title_snippet
                .replace('<', "&lt;")
                .replace('>', "&gt;")
                .replace("&lt;mark&gt;", "<mark>")
                .replace("&lt;/mark&gt;", "</mark>")
        };

        // Format score (lower is better in BM25)
        let score_display = format!("{:.2}", -result.score);

        html.push_str(&format!(
            r#"<tr class="search-result">
                <td class="task-id"><a href="/tasks/{id}">{id_short}</a></td>
                <td class="task-title">{title}</td>
                <td><span class="badge {badge_class}">{status}</span></td>
                <td class="search-score">{score}</td>
            </tr>"#,
            id = html_escape(&result.task_id),
            id_short = if result.task_id.len() > 20 {
                format!("{}...", &result.task_id[..17])
            } else {
                result.task_id.clone()
            },
            title = title_display,
            badge_class = badge_class,
            status = result.status,
            score = score_display,
        ));
    }

    html.push_str("</tbody></table>");

    Html(html)
}

/// API endpoint to get available phases (distinct phases from existing tasks).
async fn api_tasks_phases(State(state): State<DashboardServer>) -> Json<Vec<String>> {
    match state.db().get_available_phases() {
        Ok(phases) => Json(phases),
        Err(_) => Json(Vec::new()),
    }
}

/// Response for states configuration endpoint.
#[derive(serde::Serialize)]
struct StatesConfigResponse {
    /// All valid state names.
    states: Vec<String>,
    /// States that are timed (time spent is tracked).
    timed_states: Vec<String>,
    /// States that are untimed.
    untimed_states: Vec<String>,
}

/// API endpoint to get states configuration (for timed/untimed filtering).
async fn api_states_config(State(state): State<DashboardServer>) -> Json<StatesConfigResponse> {
    let config = state.states_config();
    let states: Vec<String> = config
        .state_names()
        .into_iter()
        .map(|s| s.to_string())
        .collect();
    let timed_states: Vec<String> = states
        .iter()
        .filter(|s| config.is_timed_state(s))
        .cloned()
        .collect();
    let untimed_states: Vec<String> = states
        .iter()
        .filter(|s| !config.is_timed_state(s))
        .cloned()
        .collect();

    Json(StatesConfigResponse {
        states,
        timed_states,
        untimed_states,
    })
}

/// Bulk operations API endpoint - handles delete, status change, and force-release.
async fn api_tasks_bulk(
    State(state): State<DashboardServer>,
    Json(request): Json<BulkOperationRequest>,
) -> Json<BulkOperationResponse> {
    if request.task_ids.is_empty() {
        return Json(BulkOperationResponse {
            success: false,
            affected: 0,
            error: Some("No task IDs provided".to_string()),
        });
    }

    let mut affected = 0;
    let mut last_error: Option<String> = None;

    match request.action.as_str() {
        "delete" => {
            for task_id in &request.task_ids {
                match state.db().dashboard_delete_task(task_id) {
                    Ok(()) => affected += 1,
                    Err(e) => last_error = Some(e.to_string()),
                }
            }
        }
        "change_status" => {
            let status = match &request.status {
                Some(s) => s.as_str(),
                None => {
                    return Json(BulkOperationResponse {
                        success: false,
                        affected: 0,
                        error: Some("No status provided for change_status action".to_string()),
                    });
                }
            };
            for task_id in &request.task_ids {
                match state
                    .db()
                    .dashboard_update_task(task_id, Some(status), None, None, None)
                {
                    Ok(()) => affected += 1,
                    Err(e) => last_error = Some(e.to_string()),
                }
            }
        }
        "force_release" => {
            // Force release claims by setting status to pending and clearing owner
            for task_id in &request.task_ids {
                match state.db().dashboard_force_release_task(task_id) {
                    Ok(()) => affected += 1,
                    Err(e) => last_error = Some(e.to_string()),
                }
            }
        }
        _ => {
            return Json(BulkOperationResponse {
                success: false,
                affected: 0,
                error: Some(format!("Unknown action: {}", request.action)),
            });
        }
    }

    Json(BulkOperationResponse {
        success: affected > 0,
        affected,
        error: if affected < request.task_ids.len() {
            last_error
        } else {
            None
        },
    })
}

/// Escape HTML special characters.
fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

/// Health check endpoint.
async fn health() -> impl IntoResponse {
    Json(HealthResponse {
        status: "healthy",
        version: env!("CARGO_PKG_VERSION"),
    })
}

/// API root - returns available endpoints.
async fn api_root() -> impl IntoResponse {
    Json(serde_json::json!({
        "version": env!("CARGO_PKG_VERSION"),
        "endpoints": {
            "health": "/api/health",
            "tasks": "/api/tasks (coming soon)",
            "agents": "/api/agents (coming soon)",
        }
    }))
}

/// File marks page - serves the file marks coordination page.
async fn file_marks_page() -> Html<&'static str> {
    Html(templates::FILE_MARKS_TEMPLATE)
}

/// Metrics page - serves the metrics dashboard page.
async fn metrics_page() -> Html<&'static str> {
    Html(templates::METRICS_TEMPLATE)
}

/// File marks stats API endpoint for htmx - returns HTML fragment with stats.
async fn api_file_marks_stats(State(state): State<DashboardServer>) -> Html<String> {
    let stats = state.db().get_file_marks_stats().unwrap_or({
        crate::db::dashboard::FileMarksStats {
            total_marks: 0,
            unique_agents: 0,
            with_tasks: 0,
            stale_marks: 0,
        }
    });

    Html(format!(
        r#"
        <div class="stats-row">
            <div class="stat-item">
                <div class="stat-value">{}</div>
                <div class="stat-label">Total Marks</div>
            </div>
            <div class="stat-item">
                <div class="stat-value">{}</div>
                <div class="stat-label">Unique Agents</div>
            </div>
            <div class="stat-item">
                <div class="stat-value">{}</div>
                <div class="stat-label">With Tasks</div>
            </div>
            <div class="stat-item">
                <div class="stat-value" style="color: {}">{}</div>
                <div class="stat-label">Stale (&gt;1h)</div>
            </div>
        </div>
    "#,
        stats.total_marks,
        stats.unique_agents,
        stats.with_tasks,
        if stats.stale_marks > 0 {
            "var(--warning)"
        } else {
            "var(--text-primary)"
        },
        stats.stale_marks
    ))
}

/// File marks list API endpoint for htmx - returns HTML fragment with table.
async fn api_file_marks_list(State(state): State<DashboardServer>) -> Html<String> {
    let marks = state.db().get_all_file_marks().unwrap_or_default();

    if marks.is_empty() {
        return Html(
            r#"<div class="empty-state">No file marks currently active</div>"#.to_string(),
        );
    }

    let now = now_ms();
    let mut html = String::from(
        r#"<table>
        <thead>
            <tr>
                <th>File Path</th>
                <th>Agent</th>
                <th>Task</th>
                <th>Reason</th>
                <th>Age</th>
                <th>Actions</th>
            </tr>
        </thead>
        <tbody>"#,
    );

    for mark in &marks {
        let age = now - mark.locked_at;
        let (age_text, age_class) = format_time_ago(age);

        // Determine if mark is stale (older than 1 hour)
        let is_stale = age > 60 * 60 * 1000;
        let row_class = if is_stale { "stale-mark" } else { "" };

        let task_html = mark
            .task_id
            .as_ref()
            .map(|t| {
                format!(
                    r#"<a href="/tasks/{}" class="task-link">{}</a>"#,
                    html_escape(t),
                    if t.len() > 20 {
                        format!("{}...", &t[..17])
                    } else {
                        t.clone()
                    }
                )
            })
            .unwrap_or_else(|| "-".to_string());

        let reason_html = mark
            .reason
            .as_ref()
            .map(|r| format!(r#"<span class="reason-text">{}</span>"#, html_escape(r)))
            .unwrap_or_else(|| "-".to_string());

        html.push_str(&format!(
            r##"<tr class="{row_class}">
                <td><span class="file-path">{file_path}</span></td>
                <td><span class="agent-link">{agent}</span></td>
                <td>{task}</td>
                <td>{reason}</td>
                <td><span class="time-ago {age_class}">{age_text}</span></td>
                <td>
                    <form hx-post="/api/file-marks/force-unmark"
                          hx-target="#file-marks-list"
                          hx-swap="innerHTML"
                          hx-confirm="Force-unmark this file? The agent will lose coordination.">
                        <input type="hidden" name="file_path" value="{file_path_raw}">
                        <button type="submit" class="btn btn-danger btn-sm">Force Unmark</button>
                    </form>
                </td>
            </tr>"##,
            row_class = row_class,
            file_path = html_escape(&mark.file_path),
            file_path_raw = html_escape(&mark.file_path),
            agent = html_escape(&mark.worker_id),
            task = task_html,
            reason = reason_html,
            age_class = age_class,
            age_text = age_text,
        ));
    }

    html.push_str("</tbody></table>");
    Html(html)
}

/// Form data for force-unmark endpoint.
#[derive(Debug, serde::Deserialize)]
struct ForceUnmarkForm {
    file_path: String,
}

/// Force-unmark a file - admin operation to remove stale marks.
async fn api_file_marks_force_unmark(
    State(state): State<DashboardServer>,
    Form(form): Form<ForceUnmarkForm>,
) -> Html<String> {
    match state.db().force_unmark_file(&form.file_path) {
        Ok(true) => {
            // Return updated file marks list
            api_file_marks_list(State(state)).await
        }
        Ok(false) => Html(format!(
            r#"<div class="empty-state" style="color: var(--warning);">File mark not found: {}</div>"#,
            html_escape(&form.file_path)
        )),
        Err(e) => Html(format!(
            r#"<div class="empty-state" style="color: var(--accent);">Error removing mark: {}</div>"#,
            html_escape(&e.to_string())
        )),
    }
}

// ========== METRICS API HANDLERS ==========

/// Format milliseconds as human-readable duration.
fn format_duration(ms: i64) -> String {
    if ms < 1000 {
        format!("{}ms", ms)
    } else if ms < 60_000 {
        format!("{}s", ms / 1000)
    } else if ms < 3_600_000 {
        let mins = ms / 60_000;
        let secs = (ms % 60_000) / 1000;
        if secs > 0 {
            format!("{}m {}s", mins, secs)
        } else {
            format!("{}m", mins)
        }
    } else if ms < 86_400_000 {
        let hours = ms / 3_600_000;
        let mins = (ms % 3_600_000) / 60_000;
        if mins > 0 {
            format!("{}h {}m", hours, mins)
        } else {
            format!("{}h", hours)
        }
    } else {
        let days = ms / 86_400_000;
        let hours = (ms % 86_400_000) / 3_600_000;
        if hours > 0 {
            format!("{}d {}h", days, hours)
        } else {
            format!("{}d", days)
        }
    }
}

/// Metrics overview API endpoint for htmx - returns HTML fragment with key stats.
async fn api_metrics_overview(State(state): State<DashboardServer>) -> Html<String> {
    let overview = state.db().get_metrics_overview().unwrap_or({
        crate::db::dashboard::MetricsOverview {
            total_tasks: 0,
            completed_tasks: 0,
            total_cost_usd: 0.0,
            total_time_ms: 0,
            total_points: 0,
            completed_points: 0,
        }
    });

    let time_str = format_duration(overview.total_time_ms);
    let cost_str = if overview.total_cost_usd > 0.0 {
        format!("${:.2}", overview.total_cost_usd)
    } else {
        "$0.00".to_string()
    };

    Html(format!(
        r#"
        <div class="grid grid-stats">
            <div class="card stat">
                <div class="stat-value">{}</div>
                <div class="stat-label">Total Tasks</div>
            </div>
            <div class="card stat">
                <div class="stat-value money">{}</div>
                <div class="stat-label">Total Cost</div>
            </div>
            <div class="card stat">
                <div class="stat-value time">{}</div>
                <div class="stat-label">Total Time</div>
            </div>
            <div class="card stat">
                <div class="stat-value">{}</div>
                <div class="stat-label">Completed</div>
            </div>
        </div>
    "#,
        overview.total_tasks, cost_str, time_str, overview.completed_tasks
    ))
}

/// Metrics distribution API endpoint for htmx - returns status distribution chart.
async fn api_metrics_distribution(State(state): State<DashboardServer>) -> Html<String> {
    let distribution = state.db().get_status_distribution().unwrap_or_default();

    if distribution.is_empty() {
        return Html(r#"<div class="empty-state">No tasks to display</div>"#.to_string());
    }

    let total: i64 = distribution.values().sum();
    if total == 0 {
        return Html(r#"<div class="empty-state">No tasks to display</div>"#.to_string());
    }

    // Build status bar
    let mut bar_html = String::from(r#"<div class="status-bar">"#);

    // Order statuses for consistent display
    let statuses = [
        "pending",
        "assigned",
        "working",
        "completed",
        "failed",
        "cancelled",
    ];

    for status in &statuses {
        if let Some(&count) = distribution.get(*status)
            && count > 0
        {
            bar_html.push_str(&format!(
                r#"<div class="status-segment {}" style="flex-grow: {};" title="{}: {}">{}</div>"#,
                status, count, status, count, count
            ));
        }
    }

    bar_html.push_str("</div>");

    // Build legend
    let mut legend_html = String::from(r#"<div class="status-legend">"#);

    for status in &statuses {
        if let Some(&count) = distribution.get(*status)
            && count > 0
        {
            let percentage = (count as f64 / total as f64) * 100.0;
            legend_html.push_str(&format!(
                    r#"<div class="legend-item"><span class="legend-dot {}"></span>{}: {} ({:.1}%)</div>"#,
                    status,
                    status,
                    count,
                    percentage
                ));
        }
    }

    legend_html.push_str("</div>");

    Html(format!("{}{}", bar_html, legend_html))
}

/// Query parameters for velocity endpoint.
#[derive(Debug, serde::Deserialize)]
struct VelocityParams {
    period: Option<String>,
}

/// Metrics velocity API endpoint for htmx - returns velocity chart.
async fn api_metrics_velocity(
    State(state): State<DashboardServer>,
    Query(params): Query<VelocityParams>,
) -> Html<String> {
    let period = params.period.as_deref().unwrap_or("day");
    let num_periods = if period == "week" { 6 } else { 7 };

    let velocity = state
        .db()
        .get_velocity(period, num_periods)
        .unwrap_or_default();

    if velocity.is_empty() {
        return Html(r#"<div class="empty-state">No velocity data available</div>"#.to_string());
    }

    // Find max for scaling
    let max_count = velocity
        .iter()
        .map(|v| v.completed_count)
        .max()
        .unwrap_or(1)
        .max(1);

    let mut html = String::from(r#"<div class="velocity-bars">"#);

    for point in &velocity {
        let width_percent = (point.completed_count as f64 / max_count as f64) * 100.0;

        html.push_str(&format!(
            r#"<div class="velocity-row">
                <span class="velocity-label">{}</span>
                <div class="velocity-bar-container">
                    <div class="velocity-bar" style="width: {}%;">{}</div>
                </div>
                <span class="velocity-value">{}</span>
            </div>"#,
            html_escape(&point.period_label),
            width_percent,
            if point.completed_count > 0 {
                point.completed_count.to_string()
            } else {
                String::new()
            },
            point.completed_count
        ));
    }

    html.push_str("</div>");

    // Add summary stats
    let total_completed: i64 = velocity.iter().map(|v| v.completed_count).sum();
    let total_points: i64 = velocity.iter().map(|v| v.total_points).sum();
    let avg = total_completed as f64 / num_periods as f64;

    html.push_str(&format!(
        r#"<div class="status-legend" style="margin-top: 1rem;">
            <div class="legend-item">Total: {} tasks</div>
            <div class="legend-item">Points: {}</div>
            <div class="legend-item">Avg: {:.1} per {}</div>
        </div>"#,
        total_completed, total_points, avg, period
    ));

    Html(html)
}

/// Metrics time-in-status API endpoint for htmx - returns time stats table.
async fn api_metrics_time_in_status(State(state): State<DashboardServer>) -> Html<String> {
    let time_stats = state.db().get_time_in_status().unwrap_or_default();

    if time_stats.is_empty() {
        return Html(
            r#"<div class="empty-state">No time tracking data available</div>"#.to_string(),
        );
    }

    let mut html = String::from(
        r#"<table>
        <thead>
            <tr>
                <th>Status</th>
                <th>Avg Duration</th>
                <th>Total Duration</th>
                <th>Transitions</th>
            </tr>
        </thead>
        <tbody>"#,
    );

    for stat in &time_stats {
        html.push_str(&format!(
            r#"<tr>
                <td><span class="badge badge-{}">{}</span></td>
                <td class="time">{}</td>
                <td class="time">{}</td>
                <td class="number">{}</td>
            </tr>"#,
            match stat.status.as_str() {
                "completed" => "success",
                "working" => "info",
                "pending" => "pending",
                "failed" => "error",
                "cancelled" => "warning",
                "assigned" => "assigned",
                _ => "warning",
            },
            html_escape(&stat.status),
            format_duration(stat.avg_duration_ms),
            format_duration(stat.total_duration_ms),
            stat.transition_count
        ));
    }

    html.push_str("</tbody></table>");
    Html(html)
}

/// Metrics cost-by-agent API endpoint for htmx - returns agent cost table.
async fn api_metrics_cost_by_agent(State(state): State<DashboardServer>) -> Html<String> {
    let agent_stats = state.db().get_cost_by_agent().unwrap_or_default();

    if agent_stats.is_empty() {
        return Html(
            r#"<div class="empty-state">No cost data by agent available</div>"#.to_string(),
        );
    }

    let mut html = String::from(
        r#"<table>
        <thead>
            <tr>
                <th>Agent</th>
                <th>Cost</th>
                <th>Tasks</th>
                <th>Completed</th>
                <th>Time</th>
            </tr>
        </thead>
        <tbody>"#,
    );

    for stat in &agent_stats {
        let cost_str = if stat.total_cost_usd > 0.0 {
            format!("${:.4}", stat.total_cost_usd)
        } else {
            "$0.00".to_string()
        };

        html.push_str(&format!(
            r#"<tr>
                <td>{}</td>
                <td class="cost">{}</td>
                <td class="number">{}</td>
                <td class="number">{}</td>
                <td class="time">{}</td>
            </tr>"#,
            html_escape(&stat.worker_id),
            cost_str,
            stat.task_count,
            stat.completed_count,
            format_duration(stat.total_time_ms)
        ));
    }

    html.push_str("</tbody></table>");
    Html(html)
}

/// Metrics custom metrics API endpoint for htmx - returns custom metrics display.
async fn api_metrics_custom(State(state): State<DashboardServer>) -> Html<String> {
    let custom = state
        .db()
        .get_custom_metrics()
        .unwrap_or(crate::db::dashboard::CustomMetricsAggregate { metrics: [0; 8] });

    // Check if all metrics are zero
    let has_data = custom.metrics.iter().any(|&m| m != 0);

    if !has_data {
        return Html(r#"<div class="empty-state">No custom metrics recorded. Use log_metrics() to track custom values.</div>"#.to_string());
    }

    let mut html = String::from(r#"<div class="metrics-row">"#);

    for (i, &value) in custom.metrics.iter().enumerate() {
        html.push_str(&format!(
            r#"<div class="metric-box">
                <div class="value">{}</div>
                <div class="label">Metric {}</div>
            </div>"#,
            value, i
        ));
    }

    html.push_str("</div>");
    Html(html)
}

// ========== DEPENDENCY GRAPH HANDLERS ==========

/// Dependency graph page - serves the graph visualization page.
async fn graph_page() -> Html<&'static str> {
    Html(templates::DEP_GRAPH_TEMPLATE)
}

/// Query parameters for graph mermaid endpoint.
#[derive(Debug, serde::Deserialize)]
struct GraphMermaidParams {
    dep_type: Option<String>,
    focus: Option<String>,
    depth: Option<i32>,
    direction: Option<String>,
}

/// Mermaid diagram response.
#[derive(Debug, serde::Serialize)]
struct MermaidResponse {
    diagram: String,
    node_count: usize,
    edge_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

/// Generate Mermaid diagram syntax from dependency graph.
fn generate_mermaid_diagram(
    graph: &crate::db::dashboard::DependencyGraph,
    direction: &str,
) -> String {
    if graph.nodes.is_empty() {
        return String::new();
    }

    let mut diagram = format!("flowchart {}\n", direction);

    // Define node styles based on status
    diagram.push_str("    %% Node styles\n");
    diagram.push_str("    classDef pending fill:#a0a0a0,stroke:#666,color:#000\n");
    diagram.push_str("    classDef assigned fill:#60a5fa,stroke:#3b82f6,color:#000\n");
    diagram.push_str("    classDef working fill:#60a5fa,stroke:#3b82f6,color:#000\n");
    diagram.push_str("    classDef completed fill:#4ade80,stroke:#22c55e,color:#000\n");
    diagram.push_str("    classDef failed fill:#e94560,stroke:#dc2626,color:#fff\n");
    diagram.push_str("    classDef cancelled fill:#fbbf24,stroke:#f59e0b,color:#000\n");

    // Add nodes with escaped labels
    diagram.push_str("    %% Nodes\n");
    for node in &graph.nodes {
        // Sanitize node ID for mermaid (replace special chars)
        let safe_id = sanitize_mermaid_id(&node.id);

        // Create a display label (truncate if too long)
        let display_label = if node.title.is_empty() {
            truncate_string(&node.id, 30)
        } else {
            truncate_string(&node.title, 30)
        };

        // Escape quotes in label
        let escaped_label = display_label
            .replace('"', "'")
            .replace('<', "&lt;")
            .replace('>', "&gt;");

        diagram.push_str(&format!("    {}[\"{}\"]\n", safe_id, escaped_label));
    }

    // Add edges with different styles based on dependency type
    diagram.push_str("    %% Edges\n");
    for edge in &graph.edges {
        let from_safe = sanitize_mermaid_id(&edge.from_id);
        let to_safe = sanitize_mermaid_id(&edge.to_id);

        let edge_style = match edge.dep_type.as_str() {
            "blocks" => "-->|blocks|",
            "follows" => "-.->|follows|",
            "contains" => "==>|contains|",
            _ => "-->",
        };

        diagram.push_str(&format!("    {} {} {}\n", from_safe, edge_style, to_safe));
    }

    // Apply classes based on status
    diagram.push_str("    %% Apply status classes\n");
    for node in &graph.nodes {
        let safe_id = sanitize_mermaid_id(&node.id);
        let class = match node.status.as_str() {
            "pending" => "pending",
            "assigned" => "assigned",
            "working" => "working",
            "completed" => "completed",
            "failed" => "failed",
            "cancelled" => "cancelled",
            _ => "pending",
        };
        diagram.push_str(&format!("    class {} {}\n", safe_id, class));
    }

    diagram
}

/// Sanitize a task ID for use as a mermaid node ID.
fn sanitize_mermaid_id(id: &str) -> String {
    id.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Truncate a string to a maximum length, adding ellipsis if needed.
fn truncate_string(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len.saturating_sub(3)])
    }
}

/// Graph mermaid API endpoint - returns mermaid diagram syntax.
async fn api_graph_mermaid(
    State(state): State<DashboardServer>,
    Query(params): Query<GraphMermaidParams>,
) -> Json<MermaidResponse> {
    let dep_type = params.dep_type.as_deref();
    let focus = params.focus.as_deref().filter(|s| !s.is_empty());
    let depth = params.depth.unwrap_or(2);
    let direction = params.direction.as_deref().unwrap_or("TB");

    match state.db().get_dependency_graph(dep_type, focus, depth) {
        Ok(graph) => {
            let diagram = generate_mermaid_diagram(&graph, direction);
            Json(MermaidResponse {
                diagram,
                node_count: graph.nodes.len(),
                edge_count: graph.edges.len(),
                error: None,
            })
        }
        Err(e) => Json(MermaidResponse {
            diagram: String::new(),
            node_count: 0,
            edge_count: 0,
            error: Some(e.to_string()),
        }),
    }
}

/// Graph stats API endpoint - returns HTML fragment with graph statistics.
async fn api_graph_stats(State(state): State<DashboardServer>) -> Html<String> {
    let stats = state.db().get_dependency_graph_stats().unwrap_or({
        crate::db::dashboard::DependencyGraphStats {
            total_tasks: 0,
            total_deps: 0,
            blocks_count: 0,
            follows_count: 0,
            contains_count: 0,
        }
    });

    Html(format!(
        r#"
        <div class="stat-item">
            <div class="stat-value">{}</div>
            <div class="stat-label">Tasks</div>
        </div>
        <div class="stat-item">
            <div class="stat-value">{}</div>
            <div class="stat-label">Dependencies</div>
        </div>
        <div class="stat-item">
            <div class="stat-value">{}</div>
            <div class="stat-label">Blocks</div>
        </div>
        <div class="stat-item">
            <div class="stat-value">{}</div>
            <div class="stat-label">Follows</div>
        </div>
        <div class="stat-item">
            <div class="stat-value">{}</div>
            <div class="stat-label">Contains</div>
        </div>
    "#,
        stats.total_tasks,
        stats.total_deps,
        stats.blocks_count,
        stats.follows_count,
        stats.contains_count
    ))
}

/// SQL query page - serves the SQL query interface for power users.
async fn sql_query_page() -> Html<&'static str> {
    Html(templates::SQL_QUERY_TEMPLATE)
}

/// SQL query form data.
#[derive(Debug, serde::Deserialize)]
struct SqlQueryForm {
    sql: String,
    limit: Option<i32>,
}

/// SQL query execute API endpoint - returns HTML fragment with results.
async fn api_sql_execute(
    State(state): State<DashboardServer>,
    Form(form): Form<SqlQueryForm>,
) -> Html<String> {
    use std::time::Duration;

    // Validate the query is read-only
    let sql = form.sql.trim();
    if sql.is_empty() {
        return Html(r#"<div class="error-message">Please enter a SQL query.</div>"#.to_string());
    }

    // Normalize and validate
    let normalized = sql.to_uppercase();
    let first_word = normalized.split_whitespace().next().unwrap_or("");

    if first_word != "SELECT" && first_word != "WITH" {
        return Html(format!(
            r#"<div class="error-message">Only SELECT queries are allowed. Got: {}</div>"#,
            html_escape(first_word)
        ));
    }

    // Check for forbidden statements
    let forbidden = [
        "INSERT", "UPDATE", "DELETE", "DROP", "CREATE", "ALTER", "TRUNCATE", "REPLACE", "UPSERT",
        "MERGE", "GRANT", "REVOKE", "ATTACH", "DETACH", "VACUUM", "REINDEX", "ANALYZE", "PRAGMA",
    ];

    for keyword in &forbidden {
        let pattern = format!(r"\b{}\s+", keyword);
        if let Ok(re) = regex_lite::Regex::new(&pattern)
            && re.is_match(&normalized)
        {
            return Html(format!(
                r#"<div class="error-message">{} statements are not allowed.</div>"#,
                keyword
            ));
        }
    }

    // Check for multiple statements
    if sql.matches(';').count() > 1 {
        return Html(
            r#"<div class="error-message">Multiple SQL statements are not allowed.</div>"#
                .to_string(),
        );
    }

    let limit = form.limit.map(|l| l.clamp(1, 1000)).unwrap_or(100);

    // Execute the query
    let result = state.db().with_conn(|conn| {
        conn.busy_timeout(Duration::from_secs(5))?;

        let mut stmt = conn.prepare(sql)?;
        let column_count = stmt.column_count();
        let columns: Vec<String> = (0..column_count)
            .map(|i| stmt.column_name(i).unwrap_or("?").to_string())
            .collect();

        let mut rows: Vec<Vec<String>> = Vec::new();
        let mut row_iter = stmt.query([])?;
        let mut count = 0;

        while let Some(row) = row_iter.next()? {
            if count >= limit {
                break;
            }

            let mut row_values: Vec<String> = Vec::with_capacity(column_count);
            for i in 0..column_count {
                let value = match row.get_ref(i)? {
                    rusqlite::types::ValueRef::Null => "NULL".to_string(),
                    rusqlite::types::ValueRef::Integer(i) => i.to_string(),
                    rusqlite::types::ValueRef::Real(f) => f.to_string(),
                    rusqlite::types::ValueRef::Text(s) => String::from_utf8_lossy(s).to_string(),
                    rusqlite::types::ValueRef::Blob(b) => {
                        format!("[BLOB {} bytes]", b.len())
                    }
                };
                row_values.push(value);
            }
            rows.push(row_values);
            count += 1;
        }

        let has_more = row_iter.next()?.is_some();

        Ok((columns, rows, count, has_more))
    });

    match result {
        Ok((columns, rows, row_count, truncated)) => {
            let mut html = String::new();

            // Result stats
            html.push_str(r#"<div class="result-stats">"#);
            html.push_str(&format!(
                r#"<div class="result-stat"><span class="result-stat-label">Rows:</span> <span class="result-stat-value{}">{}{}</span></div>"#,
                if truncated { " truncated" } else { "" },
                row_count,
                if truncated { "+" } else { "" }
            ));
            html.push_str(&format!(
                r#"<div class="result-stat"><span class="result-stat-label">Columns:</span> <span class="result-stat-value">{}</span></div>"#,
                columns.len()
            ));
            if truncated {
                html.push_str(&format!(
                    r#"<div class="result-stat"><span class="result-stat-label">Limit:</span> <span class="result-stat-value">{}</span></div>"#,
                    limit
                ));
            }
            html.push_str("</div>");

            // Results table
            html.push_str(r#"<div class="results-container"><table>"#);

            // Header
            html.push_str("<thead><tr>");
            for col in &columns {
                html.push_str(&format!("<th>{}</th>", html_escape(col)));
            }
            html.push_str("</tr></thead>");

            // Body
            html.push_str("<tbody>");
            if rows.is_empty() {
                html.push_str(&format!(
                    r#"<tr><td colspan="{}" class="empty-state">No rows returned</td></tr>"#,
                    columns.len().max(1)
                ));
            } else {
                for row in &rows {
                    html.push_str("<tr>");
                    for value in row {
                        if value == "NULL" {
                            html.push_str(r#"<td class="null-value">NULL</td>"#);
                        } else {
                            html.push_str(&format!("<td>{}</td>", html_escape(value)));
                        }
                    }
                    html.push_str("</tr>");
                }
            }
            html.push_str("</tbody></table></div>");

            Html(html)
        }
        Err(e) => Html(format!(
            r#"<div class="error-message">Query Error: {}</div>"#,
            html_escape(&e.to_string())
        )),
    }
}

/// SQL schema API endpoint - returns HTML fragment with schema reference.
async fn api_sql_schema(State(state): State<DashboardServer>) -> Html<String> {
    let schema_result = state.db().get_schema(false);

    match schema_result {
        Ok(schema) => {
            let mut html = String::new();

            for table in &schema.tables {
                // Table name header
                html.push_str(&format!(
                    r#"<div class="schema-table">
                        <div class="schema-table-name" onclick="toggleSchemaTable(this)">
                            <span class="toggle-icon">&#9660;</span> {}
                        </div>
                        <div class="schema-columns">"#,
                    html_escape(&table.name)
                ));

                // Columns
                for col in &table.columns {
                    let pk_indicator = if col.primary_key {
                        r#"<span class="schema-column-pk">PK</span>"#
                    } else {
                        ""
                    };
                    let nullable = if col.nullable { "" } else { " NOT NULL" };
                    html.push_str(&format!(
                        r#"<div class="schema-column">
                            <span class="schema-column-name">{}</span>
                            <span class="schema-column-type">{}{}</span>
                            {}
                        </div>"#,
                        html_escape(&col.name),
                        html_escape(&col.data_type),
                        nullable,
                        pk_indicator
                    ));
                }

                html.push_str("</div></div>");
            }

            Html(html)
        }
        Err(e) => Html(format!(
            r#"<div class="error-message">Failed to load schema: {}</div>"#,
            html_escape(&e.to_string())
        )),
    }
}

/// Build the router with all routes.
fn build_router(state: DashboardServer) -> Router {
    // Configure CORS for development
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    Router::new()
        // Page routes
        .route("/", get(root))
        .route("/workers", get(workers_page))
        .route("/tasks", get(tasks_page))
        .route(
            "/tasks/{task_id}",
            get(task_detail_page)
                .post(task_update_handler)
                .delete(task_delete_handler),
        )
        .route("/activity", get(activity_page))
        .route("/file-marks", get(file_marks_page))
        .route("/metrics", get(metrics_page))
        .route("/graph", get(graph_page))
        .route("/sql", get(sql_query_page))
        // htmx fragment routes (for periodic refresh)
        .route("/api/stats", get(api_stats))
        .route("/api/tasks/recent", get(api_recent_tasks))
        .route("/api/tasks/list", get(api_tasks_list))
        .route("/api/tasks/search", get(api_tasks_search))
        .route("/api/tasks/phases", get(api_tasks_phases))
        .route("/api/states/config", get(api_states_config))
        .route("/api/tasks/bulk", post(api_tasks_bulk))
        .route("/api/workers/active", get(api_active_workers))
        .route("/api/workers/list", get(api_workers_list))
        .route("/api/workers/{worker_id}/details", get(api_worker_details))
        .route(
            "/api/workers/{worker_id}/disconnect",
            post(api_worker_disconnect),
        )
        .route("/api/workers/cleanup", post(api_workers_cleanup))
        .route("/api/activity/stats", get(api_activity_stats))
        .route("/api/activity/list", get(api_activity_list))
        .route("/api/file-marks/stats", get(api_file_marks_stats))
        .route("/api/file-marks/list", get(api_file_marks_list))
        .route(
            "/api/file-marks/force-unmark",
            post(api_file_marks_force_unmark),
        )
        // Metrics routes
        .route("/api/metrics/overview", get(api_metrics_overview))
        .route("/api/metrics/distribution", get(api_metrics_distribution))
        .route("/api/metrics/velocity", get(api_metrics_velocity))
        .route(
            "/api/metrics/time-in-status",
            get(api_metrics_time_in_status),
        )
        .route("/api/metrics/cost-by-agent", get(api_metrics_cost_by_agent))
        .route("/api/metrics/custom", get(api_metrics_custom))
        // Graph routes
        .route("/api/graph/mermaid", get(api_graph_mermaid))
        .route("/api/graph/stats", get(api_graph_stats))
        // SQL query routes
        .route("/api/sql/execute", post(api_sql_execute))
        .route("/api/sql/schema", get(api_sql_schema))
        // API routes
        .route("/api", get(api_root))
        .route("/api/health", get(health))
        // Add middleware
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

/// Status of the dashboard server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DashboardStatus {
    /// Dashboard is running and serving requests.
    Running,
    /// Dashboard failed to start, retrying in background.
    Retrying,
    /// Dashboard has been shut down.
    Stopped,
}

/// Handle for managing the dashboard server lifecycle.
pub struct DashboardHandle {
    /// Channel to signal shutdown.
    shutdown_tx: Option<oneshot::Sender<()>>,
    /// Receiver for status updates.
    status_rx: watch::Receiver<DashboardStatus>,
}

impl DashboardHandle {
    /// Get the current status of the dashboard.
    pub fn status(&self) -> DashboardStatus {
        *self.status_rx.borrow()
    }

    /// Trigger shutdown of the dashboard server.
    pub fn shutdown(mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
    }
}

/// Start the HTTP server on the specified port.
///
/// Returns a oneshot sender that can be used to signal shutdown,
/// and the actual address the server is bound to.
pub async fn start_server(
    db: Arc<Database>,
    port: u16,
    states_config: Arc<StatesConfig>,
) -> anyhow::Result<(oneshot::Sender<()>, SocketAddr)> {
    let state = DashboardServer::new(db, port, states_config);
    let app = build_router(state);

    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let listener = tokio::net::TcpListener::bind(addr).await?;
    let bound_addr = listener.local_addr()?;

    info!("Dashboard server listening on http://{}", bound_addr);

    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();

    tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = shutdown_rx.await;
                info!("Dashboard server shutting down");
            })
            .await
        {
            // Log error but don't crash - the main MCP server continues
            tracing::error!("Dashboard server error: {}", e);
        }
    });

    Ok((shutdown_tx, bound_addr))
}

/// Compute jittered delay for retry.
/// Uses system time nanoseconds for simple jitter without requiring rand crate.
fn compute_jittered_delay(base_ms: u64, jitter_ms: u64) -> Duration {
    use std::time::SystemTime;

    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);

    // Map nanos to range [-jitter_ms, +jitter_ms]
    let jitter_range = (jitter_ms * 2) as i64;
    let jitter = if jitter_range > 0 {
        (nanos as i64 % jitter_range) - (jitter_ms as i64)
    } else {
        0
    };

    let delay_ms = (base_ms as i64 + jitter).max(1000) as u64; // At least 1 second
    Duration::from_millis(delay_ms)
}

/// Start the HTTP server with automatic retry on failure.
///
/// This function never fails - if the port is in use, it will retry in the background
/// with exponential backoff. Returns a handle to monitor and control the dashboard.
///
/// # Arguments
/// * `db` - Database handle
/// * `ui_config` - UI configuration including port and retry settings
/// * `states_config` - States configuration for the dashboard
pub fn start_server_with_retry(
    db: Arc<Database>,
    ui_config: &UiConfig,
    states_config: Arc<StatesConfig>,
) -> DashboardHandle {
    let port = ui_config.port;
    let retry_initial_ms = ui_config.retry_initial_ms;
    let retry_jitter_ms = ui_config.retry_jitter_ms;
    let retry_max_ms = ui_config.retry_max_ms;
    let retry_multiplier = ui_config.retry_multiplier;

    let (status_tx, status_rx) = watch::channel(DashboardStatus::Retrying);
    let (handle_shutdown_tx, mut handle_shutdown_rx) = oneshot::channel::<()>();

    let db_clone = Arc::clone(&db);
    let states_config_clone = Arc::clone(&states_config);

    tokio::spawn(async move {
        let mut current_delay_ms = retry_initial_ms;
        let mut server_shutdown_tx: Option<oneshot::Sender<()>> = None;

        loop {
            // Check if we've been asked to shut down
            match handle_shutdown_rx.try_recv() {
                Ok(()) | Err(oneshot::error::TryRecvError::Closed) => {
                    info!("Dashboard retry loop shutting down");
                    if let Some(tx) = server_shutdown_tx.take() {
                        let _ = tx.send(());
                    }
                    let _ = status_tx.send(DashboardStatus::Stopped);
                    break;
                }
                Err(oneshot::error::TryRecvError::Empty) => {}
            }

            // Try to start the server
            match start_server(
                Arc::clone(&db_clone),
                port,
                Arc::clone(&states_config_clone),
            )
            .await
            {
                Ok((shutdown_tx, bound_addr)) => {
                    info!("Dashboard available at http://{}", bound_addr);
                    let _ = status_tx.send(DashboardStatus::Running);
                    server_shutdown_tx = Some(shutdown_tx);

                    // Wait for shutdown signal
                    let _ = handle_shutdown_rx.await;
                    info!("Dashboard handle shutdown received");
                    if let Some(tx) = server_shutdown_tx.take() {
                        let _ = tx.send(());
                    }
                    let _ = status_tx.send(DashboardStatus::Stopped);
                    break;
                }
                Err(e) => {
                    warn!(
                        "Failed to start dashboard on port {}: {}. Retrying in {:.1}s...",
                        port,
                        e,
                        current_delay_ms as f64 / 1000.0
                    );
                    let _ = status_tx.send(DashboardStatus::Retrying);

                    // Wait with jitter
                    let delay = compute_jittered_delay(current_delay_ms, retry_jitter_ms);
                    tokio::time::sleep(delay).await;

                    // Exponential backoff, capped at max
                    current_delay_ms =
                        ((current_delay_ms as f64 * retry_multiplier) as u64).min(retry_max_ms);
                }
            }
        }
    });

    DashboardHandle {
        shutdown_tx: Some(handle_shutdown_tx),
        status_rx,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_health_response_serialization() {
        let response = HealthResponse {
            status: "healthy",
            version: "0.1.0",
        };
        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("healthy"));
        assert!(json.contains("0.1.0"));
    }
}
