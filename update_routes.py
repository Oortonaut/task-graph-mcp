#!/usr/bin/env python3
"""Script to add bulk operation handler functions to routes.rs."""

def main():
    with open('src/dashboard/routes.rs', 'r', encoding='utf-8') as f:
        content = f.read()

    # Check if handlers already exist
    if 'async fn api_tasks_bulk_delete' in content:
        print('Bulk handlers already exist')
        return

    # Use appropriate line endings
    nl = '\r\n' if '\r\n' in content else '\n'

    # Find where to insert (before get_activity_html function)
    marker = '/// Generate HTML for activity feed.'
    marker_pos = content.find(marker)
    if marker_pos == -1:
        print('Could not find get_activity_html marker')
        return

    handlers = f'''{nl}/// API endpoint for bulk delete of tasks (POST from htmx).
async fn api_tasks_bulk_delete(
    State(state): State<AppState>,
    Form(params): Form<BulkDeleteParams>,
) -> impl IntoResponse {{
    let task_ids: Vec<&str> = params.task_ids.split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();

    if task_ids.is_empty() {{
        return Html("<div class=\\"alert alert-error\\">No tasks selected</div>".to_string()).into_response();
    }}

    let mut deleted_count = 0;
    let mut errors: Vec<String> = Vec::new();

    for task_id in &task_ids {{
        match state.db.delete_task(
            task_id,
            "dashboard",
            false,
            Some("Bulk deleted via dashboard".to_string()),
            false,
            true,
        ) {{
            Ok(_) => deleted_count += 1,
            Err(e) => errors.push(format!("{{}}: {{}}", task_id, e)),
        }}
    }}

    let mut response = String::new();
    if deleted_count > 0 {{
        response.push_str(&format!("<div class=\\"alert alert-success\\">Successfully deleted {{}} task(s)</div>", deleted_count));
    }}
    if !errors.is_empty() {{
        response.push_str(&format!("<div class=\\"alert alert-error\\">Errors: {{}}</div>", errors.join(", ")));
    }}

    match get_tasks_html(&state.db) {{
        Ok(html) => Html(format!("{{}}{{}}", response, html)).into_response(),
        Err(e) => Html(format!("{{}}<tr><td colspan=\\"8\\" class=\\"empty\\">Error loading tasks: {{}}</td></tr>", response, e)).into_response(),
    }}
}}

/// API endpoint for bulk status change of tasks (POST from htmx).
async fn api_tasks_bulk_status(
    State(state): State<AppState>,
    Form(params): Form<BulkStatusParams>,
) -> impl IntoResponse {{
    let task_ids: Vec<&str> = params.task_ids.split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();

    if task_ids.is_empty() {{
        return Html("<div class=\\"alert alert-error\\">No tasks selected</div>".to_string()).into_response();
    }}

    let new_status = params.status.trim();
    if new_status.is_empty() {{
        return Html("<div class=\\"alert alert-error\\">No status selected</div>".to_string()).into_response();
    }}

    let mut updated_count = 0;
    let mut errors: Vec<String> = Vec::new();

    let states_config = &state.states_config;
    let deps_config = &state.deps_config;
    let auto_advance = crate::config::AutoAdvanceConfig::default();

    for task_id in &task_ids {{
        match state.db.update_task_unified(
            task_id,
            "dashboard",
            None,
            None,
            None,
            Some(new_status.to_string()),
            None,
            None,
            None,
            None,
            None,
            None,
            Some(format!("Bulk status change to {{}} via dashboard", new_status)),
            true,
            states_config,
            deps_config,
            &auto_advance,
        ) {{
            Ok(_) => updated_count += 1,
            Err(e) => errors.push(format!("{{}}: {{}}", task_id, e)),
        }}
    }}

    let mut response = String::new();
    if updated_count > 0 {{
        response.push_str(&format!("<div class=\\"alert alert-success\\">Successfully updated {{}} task(s) to '{{}}'</div>", updated_count, escape_html(new_status)));
    }}
    if !errors.is_empty() {{
        response.push_str(&format!("<div class=\\"alert alert-error\\">Errors: {{}}</div>", errors.join(", ")));
    }}

    match get_tasks_html(&state.db) {{
        Ok(html) => Html(format!("{{}}{{}}", response, html)).into_response(),
        Err(e) => Html(format!("{{}}<tr><td colspan=\\"8\\" class=\\"empty\\">Error loading tasks: {{}}</td></tr>", response, e)).into_response(),
    }}
}}

/// API endpoint for bulk release of task claims (POST from htmx).
async fn api_tasks_bulk_release(
    State(state): State<AppState>,
    Form(params): Form<BulkReleaseParams>,
) -> impl IntoResponse {{
    let task_ids: Vec<&str> = params.task_ids.split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();

    if task_ids.is_empty() {{
        return Html("<div class=\\"alert alert-error\\">No tasks selected</div>".to_string()).into_response();
    }}

    let mut released_count = 0;
    let mut errors: Vec<String> = Vec::new();

    let states_config = &state.states_config;
    let deps_config = &state.deps_config;
    let auto_advance = crate::config::AutoAdvanceConfig::default();

    for task_id in &task_ids {{
        match state.db.update_task_unified(
            task_id,
            "dashboard",
            None,
            None,
            None,
            Some("pending".to_string()),
            None,
            None,
            None,
            None,
            None,
            None,
            Some("Force released via dashboard bulk action".to_string()),
            true,
            states_config,
            deps_config,
            &auto_advance,
        ) {{
            Ok(_) => released_count += 1,
            Err(e) => errors.push(format!("{{}}: {{}}", task_id, e)),
        }}
    }}

    let mut response = String::new();
    if released_count > 0 {{
        response.push_str(&format!("<div class=\\"alert alert-success\\">Successfully released {{}} task(s)</div>", released_count));
    }}
    if !errors.is_empty() {{
        response.push_str(&format!("<div class=\\"alert alert-error\\">Errors: {{}}</div>", errors.join(", ")));
    }}

    match get_tasks_html(&state.db) {{
        Ok(html) => Html(format!("{{}}{{}}", response, html)).into_response(),
        Err(e) => Html(format!("{{}}<tr><td colspan=\\"8\\" class=\\"empty\\">Error loading tasks: {{}}</td></tr>", response, e)).into_response(),
    }}
}}

'''

    content = content[:marker_pos] + handlers + content[marker_pos:]

    with open('src/dashboard/routes.rs', 'w', encoding='utf-8') as f:
        f.write(content)

    print('Added bulk operation handler functions')

if __name__ == '__main__':
    main()