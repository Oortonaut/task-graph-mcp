#!/usr/bin/env python3
"""
Experiment Runner for task-graph-mcp

Automates the full lifecycle of a task-graph experiment:
  1. Reset the database (delete and recreate tasks.db)
  2. Import a task template from a snapshot file
  3. Generate agent launch commands for N agents with a chosen workflow
  4. Export metrics and results when the experiment is done

This script does NOT spawn agents directly (since agents are interactive
MCP clients like Claude Code that require their own terminal sessions).
Instead, it produces ready-to-run launch commands and provides a
`--wait` mode that polls the database until all tasks reach a terminal
state, then auto-exports metrics.

Requirements:
  - Python 3.9+
  - The `task-graph-mcp` binary must be on PATH or specified via --binary
  - SQLite3 (bundled with Python)

Usage:
  # Full experiment lifecycle
  python scripts/run_experiment.py \\
      --template experiments/my-tasks.json \\
      --workflow hierarchical \\
      --agents 4 \\
      --output results/exp-001

  # Reset only (clear database for a fresh start)
  python scripts/run_experiment.py --reset-only

  # Import only (load a template without resetting)
  python scripts/run_experiment.py --template experiments/my-tasks.json --import-only

  # Export only (collect metrics from a completed experiment)
  python scripts/run_experiment.py --export-only --output results/exp-001

  # Wait for completion then export
  python scripts/run_experiment.py --wait --output results/exp-001
"""

import argparse
import json
import os
import shutil
import sqlite3
import subprocess
import sys
import time
from datetime import datetime, timezone
from pathlib import Path
from typing import Optional


# ---------------------------------------------------------------------------
# Configuration defaults
# ---------------------------------------------------------------------------

DEFAULT_DB_PATH = Path("task-graph") / "tasks.db"
DEFAULT_CONFIG_DIR = Path("task-graph")
DEFAULT_BINARY = "task-graph-mcp"
AVAILABLE_WORKFLOWS = ["hierarchical", "swarm", "relay", "solo", "push"]
POLL_INTERVAL_SECONDS = 10


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

def log(msg: str, level: str = "INFO") -> None:
    """Print a timestamped log message."""
    ts = datetime.now().strftime("%H:%M:%S")
    print(f"[{ts}] [{level}] {msg}", file=sys.stderr)


def find_binary(binary: str) -> str:
    """Locate the task-graph-mcp binary."""
    # Try the provided path first
    if os.path.isfile(binary):
        return binary

    # Try common locations
    candidates = [
        binary,
        os.path.join("target", "release", binary),
        os.path.join("target", "debug", binary),
    ]
    # On Windows, also check with .exe extension
    if sys.platform == "win32":
        candidates += [c + ".exe" for c in candidates]

    for candidate in candidates:
        if os.path.isfile(candidate):
            return candidate

    # Fall back to PATH lookup
    found = shutil.which(binary)
    if found:
        return found

    # On Windows, also try with .exe
    if sys.platform == "win32":
        found = shutil.which(binary + ".exe")
        if found:
            return found

    return binary  # Return as-is, let subprocess handle the error


def run_cli(binary: str, args: list, db_path: Optional[str] = None,
            capture: bool = True) -> subprocess.CompletedProcess:
    """Run a task-graph-mcp CLI command."""
    cmd = [binary]
    if db_path:
        cmd.extend(["--database", str(db_path)])
    cmd.extend(args)

    log(f"Running: {' '.join(cmd)}")
    result = subprocess.run(
        cmd,
        capture_output=capture,
        text=True,
        timeout=120,
    )

    if result.returncode != 0:
        stderr = result.stderr.strip() if result.stderr else ""
        stdout = result.stdout.strip() if result.stdout else ""
        msg = stderr or stdout or f"Exit code {result.returncode}"
        log(f"Command failed: {msg}", "ERROR")

    return result


# ---------------------------------------------------------------------------
# Core operations
# ---------------------------------------------------------------------------

def reset_database(db_path: Path) -> bool:
    """Delete the existing database to start fresh.

    The task-graph-mcp server will auto-create and migrate a new database
    on first connection, so we only need to remove the old files.
    """
    files_to_remove = [
        db_path,
        Path(str(db_path) + "-shm"),
        Path(str(db_path) + "-wal"),
        Path(str(db_path) + "-journal"),
    ]

    removed = []
    for f in files_to_remove:
        if f.exists():
            try:
                f.unlink()
                removed.append(f.name)
            except OSError as e:
                log(f"Failed to remove {f}: {e}", "ERROR")
                return False

    if removed:
        log(f"Removed database files: {', '.join(removed)}")
    else:
        log("No existing database files to remove")

    return True


def import_template(binary: str, db_path: Path, template: Path,
                    force: bool = True) -> bool:
    """Import a task template snapshot into the database."""
    if not template.exists():
        log(f"Template file not found: {template}", "ERROR")
        return False

    args = ["import", str(template)]
    if force:
        args.append("--force")

    result = run_cli(binary, args, db_path=str(db_path))
    if result.returncode != 0:
        return False

    log(f"Imported template: {template}")
    if result.stdout:
        # Print import summary
        for line in result.stdout.strip().split("\n"):
            log(f"  {line}")

    return True


def export_snapshot(binary: str, db_path: Path, output_path: Path) -> bool:
    """Export the full database snapshot to a JSON file."""
    output_path.parent.mkdir(parents=True, exist_ok=True)

    args = ["export", "--output", str(output_path)]
    result = run_cli(binary, args, db_path=str(db_path))
    if result.returncode != 0:
        return False

    log(f"Exported snapshot to: {output_path}")
    return True


def export_metrics(db_path: Path, output_dir: Path) -> bool:
    """Export experiment metrics by querying the database directly.

    Produces several files:
      - summary.json: High-level experiment summary
      - tasks.json: All tasks with status, timing, and cost data
      - agents.json: Agent activity summary
      - transitions.json: Full state transition history
      - timeline.csv: Time-series of task completions
    """
    if not db_path.exists():
        log(f"Database not found: {db_path}", "ERROR")
        return False

    output_dir.mkdir(parents=True, exist_ok=True)
    conn = sqlite3.connect(str(db_path))
    conn.row_factory = sqlite3.Row

    try:
        # --- Task summary ---
        tasks = conn.execute("""
            SELECT
                id, title, status, priority,
                worker_id, phase,
                points, time_estimate_ms, time_actual_ms,
                started_at, completed_at, claimed_at,
                cost_usd,
                metric_0, metric_1, metric_2, metric_3,
                metric_4, metric_5, metric_6, metric_7,
                current_thought
            FROM tasks
            WHERE deleted_at IS NULL
            ORDER BY created_at
        """).fetchall()
        tasks_data = [dict(row) for row in tasks]

        with open(output_dir / "tasks.json", "w", encoding="utf-8") as f:
            json.dump(tasks_data, f, indent=2)
        log(f"Exported {len(tasks_data)} tasks to tasks.json")

        # --- Status counts ---
        status_counts = {}
        for task in tasks_data:
            s = task.get("status", "unknown")
            status_counts[s] = status_counts.get(s, 0) + 1

        # --- Timing statistics ---
        completed_tasks = [t for t in tasks_data if t.get("status") == "completed"]
        total_actual_ms = sum(t.get("time_actual_ms") or 0 for t in completed_tasks)
        total_cost = sum(t.get("cost_usd") or 0.0 for t in tasks_data)

        # Wall-clock time: earliest start to latest completion
        start_times = [t["started_at"] for t in tasks_data
                       if t.get("started_at")]
        end_times = [t["completed_at"] for t in tasks_data
                     if t.get("completed_at")]
        wall_clock_ms = 0
        if start_times and end_times:
            wall_clock_ms = max(end_times) - min(start_times)

        summary = {
            "exported_at": datetime.now(timezone.utc).isoformat(),
            "db_path": str(db_path),
            "total_tasks": len(tasks_data),
            "status_counts": status_counts,
            "completed_tasks": len(completed_tasks),
            "total_actual_ms": total_actual_ms,
            "wall_clock_ms": wall_clock_ms,
            "total_cost_usd": round(total_cost, 6),
            "avg_task_time_ms": (
                round(total_actual_ms / len(completed_tasks))
                if completed_tasks else 0
            ),
        }

        with open(output_dir / "summary.json", "w", encoding="utf-8") as f:
            json.dump(summary, f, indent=2)
        log(f"Exported summary to summary.json")

        # --- Agent activity ---
        try:
            agents = conn.execute("""
                SELECT
                    worker_id,
                    COUNT(*) as tasks_worked,
                    SUM(CASE WHEN status = 'completed' THEN 1 ELSE 0 END) as tasks_completed,
                    SUM(CASE WHEN status = 'failed' THEN 1 ELSE 0 END) as tasks_failed,
                    SUM(COALESCE(time_actual_ms, 0)) as total_time_ms,
                    SUM(COALESCE(cost_usd, 0.0)) as total_cost_usd
                FROM tasks
                WHERE worker_id IS NOT NULL
                  AND deleted_at IS NULL
                GROUP BY worker_id
                ORDER BY tasks_completed DESC
            """).fetchall()
            agents_data = [dict(row) for row in agents]
        except sqlite3.OperationalError:
            agents_data = []

        with open(output_dir / "agents.json", "w", encoding="utf-8") as f:
            json.dump(agents_data, f, indent=2)
        log(f"Exported {len(agents_data)} agent records to agents.json")

        # --- State transitions ---
        try:
            transitions = conn.execute("""
                SELECT
                    task_id, status, worker_id,
                    timestamp, end_timestamp,
                    COALESCE(end_timestamp - timestamp, 0) as duration_ms
                FROM task_sequence
                ORDER BY timestamp
            """).fetchall()
            transitions_data = [dict(row) for row in transitions]
        except sqlite3.OperationalError:
            transitions_data = []

        with open(output_dir / "transitions.json", "w", encoding="utf-8") as f:
            json.dump(transitions_data, f, indent=2)
        log(f"Exported {len(transitions_data)} transitions to transitions.json")

        # --- Timeline CSV ---
        try:
            timeline = conn.execute("""
                SELECT
                    completed_at as timestamp_ms,
                    id as task_id,
                    title,
                    worker_id,
                    time_actual_ms,
                    cost_usd
                FROM tasks
                WHERE status = 'completed'
                  AND completed_at IS NOT NULL
                  AND deleted_at IS NULL
                ORDER BY completed_at
            """).fetchall()

            with open(output_dir / "timeline.csv", "w", encoding="utf-8") as f:
                f.write("timestamp_ms,task_id,title,worker_id,time_actual_ms,cost_usd\n")
                for row in timeline:
                    r = dict(row)
                    title = (r.get("title") or "").replace('"', '""')
                    if ',' in title or '"' in title or '\n' in title:
                        title = f'"{title}"'
                    f.write(
                        f"{r.get('timestamp_ms', '')},{r.get('task_id', '')},"
                        f"{title},{r.get('worker_id', '')},"
                        f"{r.get('time_actual_ms', '')},{r.get('cost_usd', '')}\n"
                    )
            log(f"Exported {len(timeline)} completions to timeline.csv")
        except sqlite3.OperationalError as e:
            log(f"Could not export timeline: {e}", "WARN")

        # --- Dependencies ---
        try:
            deps = conn.execute("""
                SELECT
                    from_task_id, to_task_id, dep_type
                FROM dependencies
            """).fetchall()
            deps_data = [dict(row) for row in deps]

            with open(output_dir / "dependencies.json", "w", encoding="utf-8") as f:
                json.dump(deps_data, f, indent=2)
            log(f"Exported {len(deps_data)} dependencies to dependencies.json")
        except sqlite3.OperationalError:
            pass

        return True

    except Exception as e:
        log(f"Metrics export failed: {e}", "ERROR")
        return False
    finally:
        conn.close()


def generate_agent_commands(
    num_agents: int,
    workflow: str,
    project_dir: str = ".",
    binary: str = DEFAULT_BINARY,
) -> list:
    """Generate shell commands to launch N agents.

    Returns a list of command strings that can be run in separate terminals.
    Each agent connects to the same task-graph MCP server with the specified
    workflow. The first agent in a hierarchical workflow is designated as lead.
    """
    commands = []

    for i in range(1, num_agents + 1):
        if workflow == "hierarchical" and i == 1:
            worker_id = "lead"
            tags = "lead,coordinator"
            role_note = "# Lead agent - decomposes and assigns tasks"
        elif workflow == "hierarchical":
            worker_id = f"worker-{i}"
            tags = "worker,implementer,code"
            role_note = f"# Worker agent {i}"
        elif workflow == "push" and i == 1:
            worker_id = "coordinator"
            tags = "coordinator,lead"
            role_note = "# Coordinator - assigns ALL tasks via update(assignee=)"
        elif workflow == "push":
            worker_id = f"worker-{i - 1}"
            tags = "worker,implementer,code"
            role_note = f"# Worker {i - 1} (waits for push-assignment, no self-select)"
        elif workflow == "swarm":
            worker_id = f"swarm-{i}"
            tags = "worker,implementer,code"
            role_note = f"# Swarm agent {i}"
        elif workflow == "relay":
            worker_id = f"relay-{i}"
            tags = "worker,implementer,code"
            role_note = f"# Relay agent {i}"
        else:
            worker_id = f"agent-{i}"
            tags = "worker,implementer,code"
            role_note = f"# Agent {i}"

        # Claude Code launch command with task-graph MCP
        if workflow == "push" and i == 1:
            # Coordinator gets push-specific instructions
            cmd = (
                f'{role_note}\n'
                f'claude --task "Connect to the task-graph as {worker_id} '
                f'with workflow={workflow} and tags=[{tags}]. '
                f'You are the coordinator in a pure-push experiment. '
                f'Assign ALL tasks to workers via update(assignee=worker-id). '
                f'Workers do not self-select. Monitor and reassign on failure."'
            )
        elif workflow == "push":
            # Workers get passive instructions
            cmd = (
                f'{role_note}\n'
                f'claude --task "Connect to the task-graph as {worker_id} '
                f'with workflow={workflow} and tags=[{tags}]. '
                f'Wait for the coordinator to assign tasks to you. '
                f'Do NOT browse for tasks. When assigned, claim and complete, then wait."'
            )
        else:
            cmd = (
                f'{role_note}\n'
                f'claude --task "Connect to the task-graph as {worker_id} '
                f'with workflow={workflow} and tags=[{tags}]. '
                f'Then find ready tasks and work through them until all are complete."'
            )
        commands.append({
            "agent_num": i,
            "worker_id": worker_id,
            "workflow": workflow,
            "tags": tags.split(","),
            "command": cmd,
        })

    return commands


def check_completion(db_path: Path) -> dict:
    """Check how many tasks are in terminal vs non-terminal states."""
    if not db_path.exists():
        return {"complete": False, "error": "Database not found"}

    conn = sqlite3.connect(str(db_path))
    try:
        cursor = conn.execute("""
            SELECT status, COUNT(*) as cnt
            FROM tasks
            WHERE deleted_at IS NULL
            GROUP BY status
        """)
        status_counts = {row[0]: row[1] for row in cursor.fetchall()}

        total = sum(status_counts.values())
        terminal = (
            status_counts.get("completed", 0) +
            status_counts.get("cancelled", 0) +
            status_counts.get("failed", 0)
        )
        non_terminal = total - terminal

        return {
            "complete": non_terminal == 0 and total > 0,
            "total": total,
            "terminal": terminal,
            "non_terminal": non_terminal,
            "status_counts": status_counts,
        }
    except Exception as e:
        return {"complete": False, "error": str(e)}
    finally:
        conn.close()


def wait_for_completion(db_path: Path, poll_interval: int = POLL_INTERVAL_SECONDS,
                        timeout: int = 0) -> bool:
    """Poll the database until all tasks are in terminal states.

    Args:
        db_path: Path to the SQLite database.
        poll_interval: Seconds between polls.
        timeout: Maximum seconds to wait (0 = no timeout).

    Returns:
        True if all tasks completed, False on timeout.
    """
    start = time.time()
    iteration = 0

    log("Waiting for all tasks to reach terminal state...")
    log(f"  Poll interval: {poll_interval}s, Timeout: {'none' if timeout == 0 else f'{timeout}s'}")

    while True:
        status = check_completion(db_path)

        if "error" in status:
            log(f"Error checking status: {status['error']}", "WARN")
            time.sleep(poll_interval)
            continue

        elapsed = int(time.time() - start)
        counts_str = ", ".join(
            f"{k}={v}" for k, v in sorted(status.get("status_counts", {}).items())
        )
        log(
            f"Progress [{elapsed}s]: {status['terminal']}/{status['total']} "
            f"terminal ({counts_str})"
        )

        if status["complete"]:
            log("All tasks have reached terminal state!")
            return True

        if timeout > 0 and elapsed >= timeout:
            log(f"Timeout after {timeout}s with {status['non_terminal']} tasks remaining", "WARN")
            return False

        iteration += 1
        time.sleep(poll_interval)


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------

def main():
    parser = argparse.ArgumentParser(
        description="Experiment runner for task-graph-mcp",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog="""
Examples:
  # Set up and run a full experiment
  %(prog)s --template tasks.json --workflow hierarchical --agents 4 --output results/exp-001

  # Reset database only
  %(prog)s --reset-only

  # Import a template into a fresh database
  %(prog)s --template tasks.json --import-only

  # Export metrics from a completed experiment
  %(prog)s --export-only --output results/exp-001

  # Wait for experiment completion then auto-export
  %(prog)s --wait --output results/exp-001 --poll-interval 30

  # Dry run - show what would happen without doing it
  %(prog)s --template tasks.json --workflow swarm --agents 8 --dry-run
        """,
    )

    # --- Required-ish args ---
    parser.add_argument(
        "--template", "-t",
        type=Path,
        help="Path to a task template snapshot file (JSON or .json.gz)",
    )
    parser.add_argument(
        "--workflow", "-w",
        choices=AVAILABLE_WORKFLOWS,
        default="hierarchical",
        help="Workflow to use for agents (default: hierarchical)",
    )
    parser.add_argument(
        "--agents", "-n",
        type=int,
        default=3,
        help="Number of agents to generate commands for (default: 3)",
    )
    parser.add_argument(
        "--output", "-o",
        type=Path,
        help="Output directory for metrics and results",
    )

    # --- Database ---
    parser.add_argument(
        "--db",
        type=Path,
        default=DEFAULT_DB_PATH,
        help=f"Path to the task-graph database (default: {DEFAULT_DB_PATH})",
    )
    parser.add_argument(
        "--binary",
        type=str,
        default=DEFAULT_BINARY,
        help=f"Path to the task-graph-mcp binary (default: {DEFAULT_BINARY})",
    )

    # --- Mode flags ---
    parser.add_argument(
        "--reset-only",
        action="store_true",
        help="Only reset the database, then exit",
    )
    parser.add_argument(
        "--import-only",
        action="store_true",
        help="Only import the template (no reset, no agent commands)",
    )
    parser.add_argument(
        "--export-only",
        action="store_true",
        help="Only export metrics from the current database",
    )
    parser.add_argument(
        "--no-reset",
        action="store_true",
        help="Skip database reset (import into existing data with --force)",
    )
    parser.add_argument(
        "--no-export",
        action="store_true",
        help="Skip the final metrics export",
    )

    # --- Wait mode ---
    parser.add_argument(
        "--wait",
        action="store_true",
        help="Poll the database and wait for all tasks to complete",
    )
    parser.add_argument(
        "--poll-interval",
        type=int,
        default=POLL_INTERVAL_SECONDS,
        help=f"Seconds between completion polls (default: {POLL_INTERVAL_SECONDS})",
    )
    parser.add_argument(
        "--timeout",
        type=int,
        default=0,
        help="Max seconds to wait for completion (0 = no timeout, default: 0)",
    )

    # --- Other ---
    parser.add_argument(
        "--dry-run",
        action="store_true",
        help="Show what would be done without modifying anything",
    )
    parser.add_argument(
        "--snapshot",
        action="store_true",
        help="Also export a full database snapshot (via CLI export)",
    )
    parser.add_argument(
        "--commands-file",
        type=Path,
        help="Write agent launch commands to this file instead of stdout",
    )

    args = parser.parse_args()

    # --- Validate ---
    if args.export_only and not args.output:
        parser.error("--export-only requires --output")
    if args.import_only and not args.template:
        parser.error("--import-only requires --template")

    binary = find_binary(args.binary)

    # =====================================================================
    # Mode: Export only
    # =====================================================================
    if args.export_only:
        log("=== Export Only Mode ===")
        ok = export_metrics(args.db, args.output)
        if args.snapshot:
            snapshot_path = args.output / "snapshot.json"
            export_snapshot(binary, args.db, snapshot_path)
        return 0 if ok else 1

    # =====================================================================
    # Mode: Reset only
    # =====================================================================
    if args.reset_only:
        log("=== Reset Only Mode ===")
        if args.dry_run:
            log(f"[DRY RUN] Would delete: {args.db}")
            return 0
        ok = reset_database(args.db)
        return 0 if ok else 1

    # =====================================================================
    # Mode: Import only
    # =====================================================================
    if args.import_only:
        log("=== Import Only Mode ===")
        if args.dry_run:
            log(f"[DRY RUN] Would import: {args.template} into {args.db}")
            return 0
        ok = import_template(binary, args.db, args.template)
        return 0 if ok else 1

    # =====================================================================
    # Mode: Wait for completion
    # =====================================================================
    if args.wait:
        log("=== Wait Mode ===")
        completed = wait_for_completion(
            args.db,
            poll_interval=args.poll_interval,
            timeout=args.timeout,
        )
        if completed and args.output:
            export_metrics(args.db, args.output)
            if args.snapshot:
                export_snapshot(binary, args.db, args.output / "snapshot.json")
        return 0 if completed else 1

    # =====================================================================
    # Full experiment setup
    # =====================================================================
    log("=== Experiment Setup ===")
    log(f"  Template:  {args.template or '(none)'}")
    log(f"  Workflow:  {args.workflow}")
    log(f"  Agents:    {args.agents}")
    log(f"  Database:  {args.db}")
    log(f"  Output:    {args.output or '(none)'}")
    log(f"  Binary:    {binary}")

    # Step 1: Reset database
    if not args.no_reset and not args.dry_run:
        log("\n--- Step 1: Reset Database ---")
        if not reset_database(args.db):
            log("Database reset failed!", "ERROR")
            return 1
    elif args.dry_run:
        log(f"\n[DRY RUN] Would reset database at {args.db}")

    # Step 2: Import template
    if args.template:
        if args.dry_run:
            log(f"[DRY RUN] Would import {args.template}")
        else:
            log("\n--- Step 2: Import Template ---")
            if not import_template(binary, args.db, args.template):
                log("Template import failed!", "ERROR")
                return 1
    else:
        log("\n--- Step 2: Skipped (no template specified) ---")

    # Step 3: Generate agent commands
    log(f"\n--- Step 3: Agent Launch Commands ({args.agents} agents) ---")
    commands = generate_agent_commands(
        num_agents=args.agents,
        workflow=args.workflow,
        binary=binary,
    )

    # Write commands
    command_output = []
    command_output.append(f"# Experiment: {args.workflow} workflow with {args.agents} agents")
    command_output.append(f"# Generated: {datetime.now(timezone.utc).isoformat()}")
    command_output.append(f"# Template: {args.template or '(none)'}")
    command_output.append(f"# Database: {args.db}")
    command_output.append("")
    command_output.append("# Launch each agent in a separate terminal:")
    command_output.append("")

    for cmd_info in commands:
        command_output.append(f"# --- Agent {cmd_info['agent_num']}: {cmd_info['worker_id']} ---")
        command_output.append(cmd_info["command"])
        command_output.append("")

    command_text = "\n".join(command_output)

    if args.commands_file:
        args.commands_file.parent.mkdir(parents=True, exist_ok=True)
        with open(args.commands_file, "w", encoding="utf-8") as f:
            f.write(command_text)
        log(f"Agent commands written to: {args.commands_file}")
    else:
        print("\n" + command_text)

    # Also write commands as JSON for programmatic use
    if args.output and not args.dry_run:
        args.output.mkdir(parents=True, exist_ok=True)
        config_data = {
            "experiment": {
                "created_at": datetime.now(timezone.utc).isoformat(),
                "template": str(args.template) if args.template else None,
                "workflow": args.workflow,
                "num_agents": args.agents,
                "db_path": str(args.db),
                "binary": binary,
            },
            "agents": commands,
        }
        with open(args.output / "experiment-config.json", "w", encoding="utf-8") as f:
            json.dump(config_data, f, indent=2)
        log(f"Experiment config saved to: {args.output / 'experiment-config.json'}")

    # Step 4: Post-run instructions
    log("\n--- Next Steps ---")
    log("1. Launch each agent in a separate terminal using the commands above")
    log("2. Agents will connect to the task-graph and start working")
    log("3. Monitor progress via: task-graph-mcp --ui web")
    if args.output:
        log(f"4. When done, export metrics:")
        log(f"   python scripts/run_experiment.py --export-only --output {args.output}")
        log(f"   OR wait automatically:")
        log(f"   python scripts/run_experiment.py --wait --output {args.output}")
    else:
        log("4. When done, export metrics:")
        log("   python scripts/run_experiment.py --export-only --output results/my-experiment")

    return 0


if __name__ == "__main__":
    sys.exit(main())
