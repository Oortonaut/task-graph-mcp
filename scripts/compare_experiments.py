#!/usr/bin/env python3
"""
compare_experiments.py - Compare metrics across task-graph experiment runs.

Reads one or more task-graph SQLite databases, extracts key metrics, and
produces side-by-side comparison tables (Markdown) and optional charts.

Usage:
    python compare_experiments.py db1.db db2.db [db3.db ...]
    python compare_experiments.py db1.db db2.db --labels "baseline,optimized"
    python compare_experiments.py db1.db db2.db --charts --output report
    python compare_experiments.py db1.db --json

Metrics computed:
    - Wall-clock time (total duration, avg task time)
    - Token usage (metric_0..metric_7 mapped to in/out/cached/thinking/image/audio)
    - Cost (total, per-task, per-agent)
    - Task distribution (completion rate, failure rate, rework rate)
    - Throughput (tasks/hour, points/hour)
    - Coordination overhead (blocked vs working time)
    - Agent performance (per-worker breakdown)

Requirements:
    - Python 3.8+
    - sqlite3 (stdlib)
    - matplotlib (optional, for --charts)
"""

import argparse
import json
import os
import sqlite3
import sys
from dataclasses import dataclass, field, asdict
from pathlib import Path
from typing import Dict, List, Optional, Tuple


# ---------------------------------------------------------------------------
# Metric name mapping: metric_0..7 -> human-readable labels
# ---------------------------------------------------------------------------
METRIC_LABELS = [
    "tokens_in",
    "tokens_out",
    "tokens_cached",
    "tokens_thinking",
    "tokens_image",
    "tokens_audio",
    "metric_6",
    "metric_7",
]


# ---------------------------------------------------------------------------
# Data classes for experiment results
# ---------------------------------------------------------------------------
@dataclass
class TimeMetrics:
    total_duration_ms: int = 0
    avg_task_time_ms: float = 0.0
    median_task_time_ms: float = 0.0
    min_task_time_ms: int = 0
    max_task_time_ms: int = 0
    total_working_ms: int = 0
    total_blocked_ms: int = 0
    blocking_ratio_pct: float = 0.0
    avg_queue_wait_ms: float = 0.0


@dataclass
class TokenMetrics:
    tokens_in: int = 0
    tokens_out: int = 0
    tokens_cached: int = 0
    tokens_thinking: int = 0
    tokens_image: int = 0
    tokens_audio: int = 0
    metric_6: int = 0
    metric_7: int = 0
    total_billable: int = 0
    cache_hit_rate_pct: float = 0.0
    output_ratio: float = 0.0
    thinking_overhead: float = 0.0


@dataclass
class CostMetrics:
    total_cost_usd: float = 0.0
    avg_cost_per_task: float = 0.0
    cost_per_completed_task: float = 0.0
    cost_per_point: float = 0.0


@dataclass
class TaskDistribution:
    total_tasks: int = 0
    completed: int = 0
    failed: int = 0
    cancelled: int = 0
    pending: int = 0
    working: int = 0
    completion_rate_pct: float = 0.0
    failure_rate_pct: float = 0.0
    total_points: int = 0
    completed_points: int = 0


@dataclass
class QualityMetrics:
    rework_count: int = 0
    rework_rate_pct: float = 0.0
    avg_rework_cycles: float = 0.0
    first_pass_success_pct: float = 0.0


@dataclass
class ThroughputMetrics:
    tasks_per_hour: float = 0.0
    points_per_hour: float = 0.0
    avg_tasks_per_agent_hour: float = 0.0


@dataclass
class AgentStats:
    worker_id: str = ""
    tasks_completed: int = 0
    tasks_failed: int = 0
    total_cost_usd: float = 0.0
    total_time_ms: int = 0
    tokens_in: int = 0
    tokens_out: int = 0


@dataclass
class ExperimentResults:
    label: str = ""
    db_path: str = ""
    agent_count: int = 0
    time: TimeMetrics = field(default_factory=TimeMetrics)
    tokens: TokenMetrics = field(default_factory=TokenMetrics)
    cost: CostMetrics = field(default_factory=CostMetrics)
    tasks: TaskDistribution = field(default_factory=TaskDistribution)
    quality: QualityMetrics = field(default_factory=QualityMetrics)
    throughput: ThroughputMetrics = field(default_factory=ThroughputMetrics)
    agents: List[AgentStats] = field(default_factory=list)


# ---------------------------------------------------------------------------
# Database querying
# ---------------------------------------------------------------------------
def _safe_div(numerator, denominator, default=0.0):
    """Safe division avoiding ZeroDivisionError."""
    return numerator / denominator if denominator else default


def _table_exists(conn: sqlite3.Connection, name: str) -> bool:
    """Check whether a table exists in the database."""
    cur = conn.execute(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?", (name,)
    )
    return cur.fetchone()[0] > 0


def _get_sequence_table(conn: sqlite3.Connection) -> Optional[str]:
    """Return the name of the state-sequence table if it exists."""
    for candidate in ("task_sequence", "task_state_sequence"):
        if _table_exists(conn, candidate):
            return candidate
    return None


def extract_metrics(db_path: str, label: str) -> ExperimentResults:
    """Extract all metrics from a single task-graph database."""
    if not os.path.exists(db_path):
        print(f"Error: database file not found: {db_path}", file=sys.stderr)
        sys.exit(1)

    conn = sqlite3.connect(f"file:{db_path}?mode=ro", uri=True)
    conn.row_factory = sqlite3.Row

    result = ExperimentResults(label=label, db_path=db_path)

    # Determine the sequence table name
    seq_table = _get_sequence_table(conn)

    # ------------------------------------------------------------------
    # 1. Task distribution
    # ------------------------------------------------------------------
    row = conn.execute(
        """
        SELECT
            COUNT(*) as total,
            SUM(CASE WHEN status = 'completed' THEN 1 ELSE 0 END) as completed,
            SUM(CASE WHEN status = 'failed' THEN 1 ELSE 0 END) as failed,
            SUM(CASE WHEN status = 'cancelled' THEN 1 ELSE 0 END) as cancelled,
            SUM(CASE WHEN status = 'pending' THEN 1 ELSE 0 END) as pending,
            SUM(CASE WHEN status = 'working' THEN 1 ELSE 0 END) as working,
            COALESCE(SUM(points), 0) as total_points,
            COALESCE(SUM(CASE WHEN status = 'completed' THEN points ELSE 0 END), 0) as completed_points
        FROM tasks
        WHERE deleted_at IS NULL
        """
    ).fetchone()

    td = result.tasks
    td.total_tasks = row["total"]
    td.completed = row["completed"]
    td.failed = row["failed"]
    td.cancelled = row["cancelled"]
    td.pending = row["pending"]
    td.working = row["working"]
    td.total_points = row["total_points"]
    td.completed_points = row["completed_points"]

    claimed_total = td.completed + td.failed + td.working
    td.completion_rate_pct = round(_safe_div(td.completed * 100, claimed_total), 1)
    td.failure_rate_pct = round(_safe_div(td.failed * 100, claimed_total), 1)

    # ------------------------------------------------------------------
    # 2. Time metrics
    # ------------------------------------------------------------------
    row = conn.execute(
        """
        SELECT
            MIN(created_at)   AS first_created,
            MAX(completed_at) AS last_completed,
            AVG(time_actual_ms)  AS avg_time,
            MIN(CASE WHEN time_actual_ms > 0 THEN time_actual_ms END) AS min_time,
            MAX(time_actual_ms)  AS max_time
        FROM tasks
        WHERE deleted_at IS NULL
        """
    ).fetchone()

    tm = result.time
    first = row["first_created"]
    last = row["last_completed"]
    if first and last:
        tm.total_duration_ms = last - first
    tm.avg_task_time_ms = round(row["avg_time"] or 0, 1)
    tm.min_task_time_ms = row["min_time"] or 0
    tm.max_task_time_ms = row["max_time"] or 0

    # Median task time
    actual_times = [
        r[0]
        for r in conn.execute(
            """
            SELECT time_actual_ms FROM tasks
            WHERE deleted_at IS NULL AND time_actual_ms > 0
            ORDER BY time_actual_ms
            """
        ).fetchall()
    ]
    if actual_times:
        mid = len(actual_times) // 2
        if len(actual_times) % 2 == 0:
            tm.median_task_time_ms = (actual_times[mid - 1] + actual_times[mid]) / 2
        else:
            tm.median_task_time_ms = actual_times[mid]

    # Time blocked vs working (from sequence table)
    if seq_table:
        # Determine the column name for status in the sequence table
        # task_sequence uses 'status', task_state_sequence uses 'event'
        cols_info = conn.execute(f"PRAGMA table_info({seq_table})").fetchall()
        col_names = [c["name"] for c in cols_info]
        status_col = "status" if "status" in col_names else "event"

        working_row = conn.execute(
            f"""
            SELECT COALESCE(SUM(
                COALESCE(end_timestamp, CAST(strftime('%s','now') AS INTEGER)*1000) - timestamp
            ), 0)
            FROM {seq_table}
            WHERE {status_col} = 'working'
            """
        ).fetchone()
        tm.total_working_ms = working_row[0] if working_row else 0

        blocked_row = conn.execute(
            f"""
            SELECT COALESCE(SUM(
                COALESCE(end_timestamp, CAST(strftime('%s','now') AS INTEGER)*1000) - timestamp
            ), 0)
            FROM {seq_table}
            WHERE {status_col} IN ('pending', 'assigned')
            """
        ).fetchone()
        tm.total_blocked_ms = blocked_row[0] if blocked_row else 0

        total_tracked = tm.total_working_ms + tm.total_blocked_ms
        tm.blocking_ratio_pct = round(
            _safe_div(tm.total_blocked_ms * 100, total_tracked), 1
        )

        # Average queue wait: time from created_at to first claim (started_at)
        wait_row = conn.execute(
            """
            SELECT AVG(started_at - created_at)
            FROM tasks
            WHERE deleted_at IS NULL AND started_at IS NOT NULL AND created_at IS NOT NULL
            """
        ).fetchone()
        tm.avg_queue_wait_ms = round(wait_row[0] or 0, 1)

    # ------------------------------------------------------------------
    # 3. Token metrics
    # ------------------------------------------------------------------
    row = conn.execute(
        """
        SELECT
            COALESCE(SUM(metric_0), 0) as m0,
            COALESCE(SUM(metric_1), 0) as m1,
            COALESCE(SUM(metric_2), 0) as m2,
            COALESCE(SUM(metric_3), 0) as m3,
            COALESCE(SUM(metric_4), 0) as m4,
            COALESCE(SUM(metric_5), 0) as m5,
            COALESCE(SUM(metric_6), 0) as m6,
            COALESCE(SUM(metric_7), 0) as m7
        FROM tasks
        WHERE deleted_at IS NULL
        """
    ).fetchone()

    tk = result.tokens
    tk.tokens_in = row["m0"]
    tk.tokens_out = row["m1"]
    tk.tokens_cached = row["m2"]
    tk.tokens_thinking = row["m3"]
    tk.tokens_image = row["m4"]
    tk.tokens_audio = row["m5"]
    tk.metric_6 = row["m6"]
    tk.metric_7 = row["m7"]
    tk.total_billable = tk.tokens_in + tk.tokens_out + tk.tokens_thinking
    tk.cache_hit_rate_pct = round(
        _safe_div(tk.tokens_cached * 100, tk.tokens_in + tk.tokens_cached), 1
    )
    tk.output_ratio = round(_safe_div(tk.tokens_out, tk.tokens_in), 3)
    tk.thinking_overhead = round(_safe_div(tk.tokens_thinking, tk.tokens_out), 3)

    # ------------------------------------------------------------------
    # 4. Cost metrics
    # ------------------------------------------------------------------
    row = conn.execute(
        """
        SELECT
            COALESCE(SUM(cost_usd), 0.0) as total_cost,
            COUNT(*) as total_tasks,
            SUM(CASE WHEN status = 'completed' THEN 1 ELSE 0 END) as completed,
            COALESCE(SUM(CASE WHEN status = 'completed' THEN points ELSE 0 END), 0) as pts
        FROM tasks
        WHERE deleted_at IS NULL
        """
    ).fetchone()

    cm = result.cost
    cm.total_cost_usd = round(row["total_cost"], 4)
    cm.avg_cost_per_task = round(_safe_div(row["total_cost"], row["total_tasks"]), 4)
    cm.cost_per_completed_task = round(
        _safe_div(row["total_cost"], row["completed"]), 4
    )
    cm.cost_per_point = round(_safe_div(row["total_cost"], row["pts"]), 4)

    # ------------------------------------------------------------------
    # 5. Quality / Rework metrics
    # ------------------------------------------------------------------
    if seq_table:
        cols_info = conn.execute(f"PRAGMA table_info({seq_table})").fetchall()
        col_names = [c["name"] for c in cols_info]
        status_col = "status" if "status" in col_names else "event"

        rework_rows = conn.execute(
            f"""
            SELECT task_id, COUNT(*) as working_periods
            FROM {seq_table}
            WHERE {status_col} = 'working'
            GROUP BY task_id
            """
        ).fetchall()

        total_with_work = len(rework_rows)
        reworked = sum(1 for r in rework_rows if r["working_periods"] > 1)
        rework_cycles = [
            r["working_periods"] for r in rework_rows if r["working_periods"] > 1
        ]

        qm = result.quality
        qm.rework_count = reworked
        qm.rework_rate_pct = round(_safe_div(reworked * 100, total_with_work), 1)
        qm.avg_rework_cycles = round(
            _safe_div(sum(rework_cycles), len(rework_cycles)), 1
        )
        qm.first_pass_success_pct = round(
            _safe_div((total_with_work - reworked) * 100, total_with_work), 1
        )

    # ------------------------------------------------------------------
    # 6. Throughput metrics
    # ------------------------------------------------------------------
    tp = result.throughput
    duration_hours = _safe_div(tm.total_duration_ms, 3_600_000)
    tp.tasks_per_hour = round(_safe_div(td.completed, duration_hours), 2)
    tp.points_per_hour = round(_safe_div(td.completed_points, duration_hours), 2)

    # ------------------------------------------------------------------
    # 7. Agent / worker stats
    # ------------------------------------------------------------------
    agents = conn.execute(
        """
        SELECT
            worker_id,
            SUM(CASE WHEN status = 'completed' THEN 1 ELSE 0 END) as completed,
            SUM(CASE WHEN status = 'failed' THEN 1 ELSE 0 END) as failed,
            COALESCE(SUM(cost_usd), 0.0) as cost,
            COALESCE(SUM(time_actual_ms), 0) as time_ms,
            COALESCE(SUM(metric_0), 0) as tok_in,
            COALESCE(SUM(metric_1), 0) as tok_out
        FROM tasks
        WHERE deleted_at IS NULL AND worker_id IS NOT NULL
        GROUP BY worker_id
        ORDER BY cost DESC
        """
    ).fetchall()

    for a in agents:
        result.agents.append(
            AgentStats(
                worker_id=a["worker_id"],
                tasks_completed=a["completed"],
                tasks_failed=a["failed"],
                total_cost_usd=round(a["cost"], 4),
                total_time_ms=a["time_ms"],
                tokens_in=a["tok_in"],
                tokens_out=a["tok_out"],
            )
        )

    result.agent_count = len(result.agents)

    # Per-agent throughput
    if result.agent_count > 0 and duration_hours > 0:
        tp.avg_tasks_per_agent_hour = round(
            _safe_div(td.completed, duration_hours * result.agent_count), 2
        )

    conn.close()
    return result


# ---------------------------------------------------------------------------
# Formatting helpers
# ---------------------------------------------------------------------------
def _fmt_ms(ms) -> str:
    """Format milliseconds to human-readable string."""
    if ms is None or ms == 0:
        return "-"
    ms = int(ms)
    if ms < 1000:
        return f"{ms}ms"
    if ms < 60_000:
        return f"{ms / 1000:.1f}s"
    if ms < 3_600_000:
        m, s = divmod(ms, 60_000)
        return f"{m}m {s // 1000}s"
    h, rem = divmod(ms, 3_600_000)
    m = rem // 60_000
    return f"{h}h {m}m"


def _fmt_num(n) -> str:
    """Format a number with comma separators."""
    if n is None:
        return "-"
    if isinstance(n, float):
        if n == int(n):
            return f"{int(n):,}"
        return f"{n:,.2f}"
    return f"{n:,}"


def _fmt_usd(v) -> str:
    """Format USD value."""
    if v is None or v == 0:
        return "$0.00"
    return f"${v:,.4f}"


def _fmt_pct(v) -> str:
    """Format percentage."""
    if v is None:
        return "-"
    return f"{v:.1f}%"


# ---------------------------------------------------------------------------
# Markdown table generation
# ---------------------------------------------------------------------------
def _make_comparison_table(
    title: str,
    headers: List[str],
    rows: List[Tuple[str, ...]],
) -> str:
    """Build a Markdown table with the given title, headers, and rows."""
    lines = [f"### {title}", ""]

    # Header row
    lines.append("| " + " | ".join(headers) + " |")
    # Alignment row: left-align metric name, right-align values
    aligns = [":---"] + ["---:"] * (len(headers) - 1)
    lines.append("| " + " | ".join(aligns) + " |")

    for row in rows:
        lines.append("| " + " | ".join(str(c) for c in row) + " |")

    lines.append("")
    return "\n".join(lines)


def generate_markdown(results: List[ExperimentResults]) -> str:
    """Generate a full Markdown comparison report."""
    labels = [r.label for r in results]
    headers_base = ["Metric"] + labels

    sections = []
    sections.append("# Experiment Comparison Report\n")

    # Summary
    summary_rows = [
        ("Database", *[r.db_path for r in results]),
        ("Agents", *[str(r.agent_count) for r in results]),
        ("Total Tasks", *[_fmt_num(r.tasks.total_tasks) for r in results]),
        ("Completed", *[_fmt_num(r.tasks.completed) for r in results]),
        ("Failed", *[_fmt_num(r.tasks.failed) for r in results]),
        ("Total Cost", *[_fmt_usd(r.cost.total_cost_usd) for r in results]),
        ("Total Duration", *[_fmt_ms(r.time.total_duration_ms) for r in results]),
    ]
    sections.append("## Summary\n")
    sections.append(
        _make_comparison_table("Overview", headers_base, summary_rows)
    )

    # Time Metrics
    time_rows = [
        ("Total Duration", *[_fmt_ms(r.time.total_duration_ms) for r in results]),
        ("Avg Task Time", *[_fmt_ms(r.time.avg_task_time_ms) for r in results]),
        ("Median Task Time", *[_fmt_ms(r.time.median_task_time_ms) for r in results]),
        ("Min Task Time", *[_fmt_ms(r.time.min_task_time_ms) for r in results]),
        ("Max Task Time", *[_fmt_ms(r.time.max_task_time_ms) for r in results]),
        ("Total Working Time", *[_fmt_ms(r.time.total_working_ms) for r in results]),
        ("Total Blocked Time", *[_fmt_ms(r.time.total_blocked_ms) for r in results]),
        ("Blocking Ratio", *[_fmt_pct(r.time.blocking_ratio_pct) for r in results]),
        ("Avg Queue Wait", *[_fmt_ms(r.time.avg_queue_wait_ms) for r in results]),
    ]
    sections.append("## Time Metrics\n")
    sections.append(_make_comparison_table("Time", headers_base, time_rows))

    # Token Metrics
    token_rows = [
        ("Input Tokens", *[_fmt_num(r.tokens.tokens_in) for r in results]),
        ("Output Tokens", *[_fmt_num(r.tokens.tokens_out) for r in results]),
        ("Cached Tokens", *[_fmt_num(r.tokens.tokens_cached) for r in results]),
        ("Thinking Tokens", *[_fmt_num(r.tokens.tokens_thinking) for r in results]),
        ("Image Tokens", *[_fmt_num(r.tokens.tokens_image) for r in results]),
        ("Audio Tokens", *[_fmt_num(r.tokens.tokens_audio) for r in results]),
        ("Total Billable", *[_fmt_num(r.tokens.total_billable) for r in results]),
        ("Cache Hit Rate", *[_fmt_pct(r.tokens.cache_hit_rate_pct) for r in results]),
        ("Output Ratio", *[f"{r.tokens.output_ratio:.3f}" for r in results]),
        (
            "Thinking Overhead",
            *[f"{r.tokens.thinking_overhead:.3f}" for r in results],
        ),
    ]
    sections.append("## Token Metrics\n")
    sections.append(_make_comparison_table("Tokens", headers_base, token_rows))

    # Cost Metrics
    cost_rows = [
        ("Total Cost", *[_fmt_usd(r.cost.total_cost_usd) for r in results]),
        ("Avg Cost / Task", *[_fmt_usd(r.cost.avg_cost_per_task) for r in results]),
        (
            "Cost / Completed Task",
            *[_fmt_usd(r.cost.cost_per_completed_task) for r in results],
        ),
        ("Cost / Point", *[_fmt_usd(r.cost.cost_per_point) for r in results]),
    ]
    sections.append("## Cost Metrics\n")
    sections.append(_make_comparison_table("Cost", headers_base, cost_rows))

    # Task Distribution
    dist_rows = [
        ("Total Tasks", *[_fmt_num(r.tasks.total_tasks) for r in results]),
        ("Completed", *[_fmt_num(r.tasks.completed) for r in results]),
        ("Failed", *[_fmt_num(r.tasks.failed) for r in results]),
        ("Cancelled", *[_fmt_num(r.tasks.cancelled) for r in results]),
        ("Pending", *[_fmt_num(r.tasks.pending) for r in results]),
        ("Working", *[_fmt_num(r.tasks.working) for r in results]),
        (
            "Completion Rate",
            *[_fmt_pct(r.tasks.completion_rate_pct) for r in results],
        ),
        ("Failure Rate", *[_fmt_pct(r.tasks.failure_rate_pct) for r in results]),
        ("Total Points", *[_fmt_num(r.tasks.total_points) for r in results]),
        ("Completed Points", *[_fmt_num(r.tasks.completed_points) for r in results]),
    ]
    sections.append("## Task Distribution\n")
    sections.append(
        _make_comparison_table("Distribution", headers_base, dist_rows)
    )

    # Quality Metrics
    quality_rows = [
        ("Reworked Tasks", *[_fmt_num(r.quality.rework_count) for r in results]),
        ("Rework Rate", *[_fmt_pct(r.quality.rework_rate_pct) for r in results]),
        (
            "Avg Rework Cycles",
            *[f"{r.quality.avg_rework_cycles:.1f}" for r in results],
        ),
        (
            "First-Pass Success",
            *[_fmt_pct(r.quality.first_pass_success_pct) for r in results],
        ),
    ]
    sections.append("## Quality Metrics\n")
    sections.append(
        _make_comparison_table("Quality", headers_base, quality_rows)
    )

    # Throughput Metrics
    throughput_rows = [
        ("Tasks / Hour", *[f"{r.throughput.tasks_per_hour:.2f}" for r in results]),
        ("Points / Hour", *[f"{r.throughput.points_per_hour:.2f}" for r in results]),
        (
            "Tasks / Agent-Hour",
            *[f"{r.throughput.avg_tasks_per_agent_hour:.2f}" for r in results],
        ),
    ]
    sections.append("## Throughput\n")
    sections.append(
        _make_comparison_table("Throughput", headers_base, throughput_rows)
    )

    # Per-Agent Breakdown (for each experiment)
    sections.append("## Per-Agent Breakdown\n")
    for r in results:
        if not r.agents:
            sections.append(f"### {r.label}\n\nNo agent data.\n")
            continue

        agent_headers = [
            "Agent",
            "Completed",
            "Failed",
            "Cost (USD)",
            "Time",
            "Tokens In",
            "Tokens Out",
        ]
        agent_rows = []
        for a in r.agents:
            agent_rows.append(
                (
                    a.worker_id,
                    str(a.tasks_completed),
                    str(a.tasks_failed),
                    _fmt_usd(a.total_cost_usd),
                    _fmt_ms(a.total_time_ms),
                    _fmt_num(a.tokens_in),
                    _fmt_num(a.tokens_out),
                )
            )
        sections.append(
            _make_comparison_table(f"Agents - {r.label}", agent_headers, agent_rows)
        )

    # Delta analysis (when exactly 2 experiments)
    if len(results) == 2:
        sections.append("## Delta Analysis (B vs A)\n")
        a, b = results[0], results[1]

        def _delta(va, vb, fmt_fn=_fmt_num, lower_better=True):
            diff = vb - va
            pct = _safe_div(diff * 100, abs(va)) if va else 0
            direction = "lower" if diff < 0 else "higher"
            indicator = ""
            if diff != 0:
                if lower_better:
                    indicator = " (+)" if diff < 0 else " (-)"
                else:
                    indicator = " (+)" if diff > 0 else " (-)"
            return f"{fmt_fn(diff)} ({pct:+.1f}%){indicator}"

        delta_rows = [
            (
                "Total Duration",
                _delta(
                    a.time.total_duration_ms,
                    b.time.total_duration_ms,
                    _fmt_ms,
                    lower_better=True,
                ),
            ),
            (
                "Total Cost",
                _delta(
                    a.cost.total_cost_usd,
                    b.cost.total_cost_usd,
                    _fmt_usd,
                    lower_better=True,
                ),
            ),
            (
                "Tasks Completed",
                _delta(
                    a.tasks.completed,
                    b.tasks.completed,
                    _fmt_num,
                    lower_better=False,
                ),
            ),
            (
                "Completion Rate",
                _delta(
                    a.tasks.completion_rate_pct,
                    b.tasks.completion_rate_pct,
                    _fmt_pct,
                    lower_better=False,
                ),
            ),
            (
                "Tasks/Hour",
                _delta(
                    a.throughput.tasks_per_hour,
                    b.throughput.tasks_per_hour,
                    _fmt_num,
                    lower_better=False,
                ),
            ),
            (
                "Rework Rate",
                _delta(
                    a.quality.rework_rate_pct,
                    b.quality.rework_rate_pct,
                    _fmt_pct,
                    lower_better=True,
                ),
            ),
            (
                "Total Billable Tokens",
                _delta(
                    a.tokens.total_billable,
                    b.tokens.total_billable,
                    _fmt_num,
                    lower_better=True,
                ),
            ),
            (
                "Blocking Ratio",
                _delta(
                    a.time.blocking_ratio_pct,
                    b.time.blocking_ratio_pct,
                    _fmt_pct,
                    lower_better=True,
                ),
            ),
        ]

        delta_headers = ["Metric", f"Delta ({b.label} vs {a.label})"]
        sections.append(
            _make_comparison_table("Deltas", delta_headers, delta_rows)
        )
        sections.append(
            "_(+) = improvement, (-) = regression relative to target direction_\n"
        )

    sections.append(
        "\n---\n_Generated by `compare_experiments.py` from task-graph-mcp_\n"
    )

    return "\n".join(sections)


# ---------------------------------------------------------------------------
# Chart generation (optional matplotlib)
# ---------------------------------------------------------------------------
def generate_charts(results: List[ExperimentResults], output_prefix: str) -> List[str]:
    """Generate comparison charts. Returns list of saved file paths."""
    try:
        import matplotlib

        matplotlib.use("Agg")
        import matplotlib.pyplot as plt
        import matplotlib.ticker as ticker
    except ImportError:
        print(
            "Warning: matplotlib not available. Skipping chart generation.",
            file=sys.stderr,
        )
        return []

    saved = []
    labels = [r.label for r in results]
    n = len(results)
    x = range(n)
    bar_width = max(0.2, 0.6 / n)

    fig_size = (max(8, 3 * n), 5)
    colors = plt.cm.Set2.colors  # type: ignore[attr-defined]

    # --- Chart 1: Overview comparison (cost, duration, completion rate) ---
    fig, axes = plt.subplots(1, 3, figsize=(14, 5))

    # Cost
    ax = axes[0]
    costs = [r.cost.total_cost_usd for r in results]
    bars = ax.bar(labels, costs, color=colors[:n])
    ax.set_ylabel("USD")
    ax.set_title("Total Cost")
    ax.yaxis.set_major_formatter(ticker.FormatStrFormatter("$%.4f"))
    for bar, val in zip(bars, costs):
        ax.text(
            bar.get_x() + bar.get_width() / 2,
            bar.get_height(),
            f"${val:.4f}",
            ha="center",
            va="bottom",
            fontsize=9,
        )

    # Duration
    ax = axes[1]
    durations = [r.time.total_duration_ms / 1000 for r in results]
    bars = ax.bar(labels, durations, color=colors[:n])
    ax.set_ylabel("Seconds")
    ax.set_title("Total Duration")
    for bar, val in zip(bars, durations):
        ax.text(
            bar.get_x() + bar.get_width() / 2,
            bar.get_height(),
            _fmt_ms(int(val * 1000)),
            ha="center",
            va="bottom",
            fontsize=9,
        )

    # Completion Rate
    ax = axes[2]
    rates = [r.tasks.completion_rate_pct for r in results]
    bars = ax.bar(labels, rates, color=colors[:n])
    ax.set_ylabel("%")
    ax.set_title("Completion Rate")
    ax.set_ylim(0, 105)
    for bar, val in zip(bars, rates):
        ax.text(
            bar.get_x() + bar.get_width() / 2,
            bar.get_height(),
            f"{val:.1f}%",
            ha="center",
            va="bottom",
            fontsize=9,
        )

    plt.tight_layout()
    path = f"{output_prefix}_overview.png"
    plt.savefig(path, dpi=150)
    plt.close()
    saved.append(path)

    # --- Chart 2: Token breakdown (stacked bar) ---
    fig, ax = plt.subplots(figsize=fig_size)
    token_types = ["tokens_in", "tokens_out", "tokens_cached", "tokens_thinking"]
    token_labels = ["Input", "Output", "Cached", "Thinking"]
    bottom = [0] * n

    for i, (ttype, tlabel) in enumerate(zip(token_types, token_labels)):
        vals = [getattr(r.tokens, ttype) for r in results]
        ax.bar(labels, vals, bottom=bottom, label=tlabel, color=colors[i])
        bottom = [b + v for b, v in zip(bottom, vals)]

    ax.set_ylabel("Tokens")
    ax.set_title("Token Usage Breakdown")
    ax.legend()
    ax.yaxis.set_major_formatter(ticker.FuncFormatter(lambda x, _: f"{x / 1000:.0f}k" if x >= 1000 else f"{x:.0f}"))
    plt.tight_layout()
    path = f"{output_prefix}_tokens.png"
    plt.savefig(path, dpi=150)
    plt.close()
    saved.append(path)

    # --- Chart 3: Task distribution (stacked bar) ---
    fig, ax = plt.subplots(figsize=fig_size)
    statuses = ["completed", "failed", "pending", "working", "cancelled"]
    status_colors = ["#2ecc71", "#e74c3c", "#95a5a6", "#3498db", "#7f8c8d"]
    bottom = [0] * n

    for status, color in zip(statuses, status_colors):
        vals = [getattr(r.tasks, status) for r in results]
        ax.bar(labels, vals, bottom=bottom, label=status.capitalize(), color=color)
        bottom = [b + v for b, v in zip(bottom, vals)]

    ax.set_ylabel("Tasks")
    ax.set_title("Task Status Distribution")
    ax.legend()
    plt.tight_layout()
    path = f"{output_prefix}_tasks.png"
    plt.savefig(path, dpi=150)
    plt.close()
    saved.append(path)

    # --- Chart 4: Throughput comparison ---
    fig, axes = plt.subplots(1, 2, figsize=(10, 5))

    ax = axes[0]
    vals = [r.throughput.tasks_per_hour for r in results]
    bars = ax.bar(labels, vals, color=colors[:n])
    ax.set_ylabel("Tasks / Hour")
    ax.set_title("Throughput: Tasks per Hour")
    for bar, val in zip(bars, vals):
        ax.text(
            bar.get_x() + bar.get_width() / 2,
            bar.get_height(),
            f"{val:.1f}",
            ha="center",
            va="bottom",
            fontsize=9,
        )

    ax = axes[1]
    vals = [r.time.blocking_ratio_pct for r in results]
    bars = ax.bar(labels, vals, color=colors[:n])
    ax.set_ylabel("%")
    ax.set_title("Blocking Ratio")
    ax.set_ylim(0, 105)
    for bar, val in zip(bars, vals):
        ax.text(
            bar.get_x() + bar.get_width() / 2,
            bar.get_height(),
            f"{val:.1f}%",
            ha="center",
            va="bottom",
            fontsize=9,
        )

    plt.tight_layout()
    path = f"{output_prefix}_throughput.png"
    plt.savefig(path, dpi=150)
    plt.close()
    saved.append(path)

    # --- Chart 5: Per-agent cost (grouped bar, if agents exist) ---
    all_agent_ids = sorted(
        set(a.worker_id for r in results for a in r.agents)
    )
    if all_agent_ids:
        fig, ax = plt.subplots(figsize=(max(10, len(all_agent_ids) * 2), 5))
        import numpy as np

        x_pos = np.arange(len(all_agent_ids))
        width = 0.8 / n

        for i, r in enumerate(results):
            agent_costs = {}
            for a in r.agents:
                agent_costs[a.worker_id] = a.total_cost_usd
            vals = [agent_costs.get(aid, 0) for aid in all_agent_ids]
            offset = (i - n / 2 + 0.5) * width
            ax.bar(x_pos + offset, vals, width, label=r.label, color=colors[i % len(colors)])

        ax.set_xlabel("Agent")
        ax.set_ylabel("Cost (USD)")
        ax.set_title("Cost by Agent")
        ax.set_xticks(x_pos)
        ax.set_xticklabels(all_agent_ids, rotation=45, ha="right")
        ax.legend()
        ax.yaxis.set_major_formatter(ticker.FormatStrFormatter("$%.4f"))
        plt.tight_layout()
        path = f"{output_prefix}_agent_cost.png"
        plt.savefig(path, dpi=150)
        plt.close()
        saved.append(path)

    return saved


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------
def main():
    parser = argparse.ArgumentParser(
        description="Compare metrics across task-graph experiment runs.",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog="""
Examples:
  # Compare two experiment databases
  python compare_experiments.py run1/tasks.db run2/tasks.db

  # Custom labels
  python compare_experiments.py run1/tasks.db run2/tasks.db --labels "baseline,optimized"

  # Generate charts + write report
  python compare_experiments.py run1/tasks.db run2/tasks.db --charts --output report

  # Single experiment summary
  python compare_experiments.py tasks.db

  # JSON output
  python compare_experiments.py tasks.db --json
        """,
    )
    parser.add_argument(
        "databases",
        nargs="+",
        help="Paths to task-graph SQLite database files",
    )
    parser.add_argument(
        "--labels",
        type=str,
        default=None,
        help="Comma-separated labels for each database (default: filename stems)",
    )
    parser.add_argument(
        "--charts",
        action="store_true",
        help="Generate comparison charts (requires matplotlib)",
    )
    parser.add_argument(
        "--output",
        "-o",
        type=str,
        default=None,
        help="Output prefix for report file and charts (default: stdout for markdown)",
    )
    parser.add_argument(
        "--json",
        action="store_true",
        help="Output as JSON instead of Markdown",
    )

    args = parser.parse_args()

    # Resolve labels
    if args.labels:
        labels = [l.strip() for l in args.labels.split(",")]
        if len(labels) != len(args.databases):
            print(
                f"Error: {len(labels)} labels provided but {len(args.databases)} databases given.",
                file=sys.stderr,
            )
            sys.exit(1)
    else:
        labels = []
        used = set()
        for db in args.databases:
            stem = Path(db).stem
            # If duplicate stems, use parent dir + stem
            if stem in used:
                stem = f"{Path(db).parent.name}/{stem}"
            used.add(stem)
            labels.append(stem)

    # Extract metrics from each database
    results = []
    for db_path, label in zip(args.databases, labels):
        results.append(extract_metrics(db_path, label))

    # Output
    if args.json:
        output = json.dumps(
            [asdict(r) for r in results],
            indent=2,
            default=str,
        )
        if args.output:
            json_path = f"{args.output}.json"
            with open(json_path, "w") as f:
                f.write(output)
            print(f"JSON report written to: {json_path}")
        else:
            print(output)
    else:
        md = generate_markdown(results)
        if args.output:
            md_path = f"{args.output}.md"
            with open(md_path, "w") as f:
                f.write(md)
            print(f"Markdown report written to: {md_path}")
        else:
            print(md)

    # Charts
    if args.charts:
        prefix = args.output or "comparison"
        chart_files = generate_charts(results, prefix)
        if chart_files:
            print(f"\nCharts saved:")
            for cf in chart_files:
                print(f"  - {cf}")


if __name__ == "__main__":
    main()
