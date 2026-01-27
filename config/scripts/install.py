#!/usr/bin/env python3
"""
Task Graph MCP Skills Installer

Installs task-graph skills to a target directory (default: ~/.claude/skills/).
Supports custom targets for different environments.

Usage:
    python install.py                      # Install to ~/.claude/skills/
    python install.py --target /path/to/   # Install to custom location
    python install.py --list               # List available skills
    python install.py --skills worker,coordinator  # Install specific skills
    python install.py --dry-run            # Show what would be installed
    python install.py --uninstall          # Remove installed skills
"""

import argparse
import shutil
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import List, Optional


@dataclass
class Result:
    """Standard result type for script operations."""
    success: bool
    message: str
    data: Optional[dict] = None


# Skills in this suite
SKILLS = [
    "task-graph-basics",
    "task-graph-coordinator",
    "task-graph-worker",
    "task-graph-reporting",
    "task-graph-migration",
    "task-graph-repair",
]

# Default installation target
DEFAULT_TARGET = Path.home() / ".claude" / "skills"


def get_script_dir() -> Path:
    """Get the directory containing this script."""
    return Path(__file__).parent.resolve()


def get_skills_dir() -> Path:
    """Get the skills source directory (parent of scripts/)."""
    return get_script_dir().parent


def list_skills() -> Result:
    """List all available skills in the suite."""
    skills_dir = get_skills_dir()
    available = []

    for skill in SKILLS:
        skill_path = skills_dir / skill / "SKILL.md"
        if skill_path.exists():
            available.append(skill)

    return Result(
        success=True,
        message=f"Found {len(available)} skills",
        data={"skills": available}
    )


def validate_skill(skill_path: Path) -> Result:
    """Validate a skill directory has required files."""
    skill_md = skill_path / "SKILL.md"

    if not skill_path.exists():
        return Result(False, f"Skill directory not found: {skill_path}")

    if not skill_md.exists():
        return Result(False, f"SKILL.md not found in {skill_path}")

    # Check frontmatter
    content = skill_md.read_text(encoding="utf-8")
    if not content.startswith("---"):
        return Result(False, f"Missing frontmatter in {skill_md}")

    return Result(True, "Valid skill")


def install_skill(
    skill_name: str,
    target_dir: Path,
    dry_run: bool = False
) -> Result:
    """Install a single skill to the target directory."""
    skills_dir = get_skills_dir()
    source = skills_dir / skill_name
    dest = target_dir / skill_name

    # Validate source
    validation = validate_skill(source)
    if not validation.success:
        return validation

    if dry_run:
        return Result(
            True,
            f"Would install: {source} -> {dest}",
            data={"source": str(source), "dest": str(dest)}
        )

    # Create target directory if needed
    target_dir.mkdir(parents=True, exist_ok=True)

    # Remove existing if present
    if dest.exists():
        shutil.rmtree(dest)

    # Copy skill directory
    shutil.copytree(source, dest)

    return Result(
        True,
        f"Installed: {skill_name} -> {dest}",
        data={"source": str(source), "dest": str(dest)}
    )


def uninstall_skill(skill_name: str, target_dir: Path, dry_run: bool = False) -> Result:
    """Uninstall a single skill from the target directory."""
    dest = target_dir / skill_name

    if not dest.exists():
        return Result(True, f"Not installed: {skill_name}")

    if dry_run:
        return Result(True, f"Would remove: {dest}")

    shutil.rmtree(dest)
    return Result(True, f"Removed: {dest}")


def install_all(
    target_dir: Path,
    skills: Optional[List[str]] = None,
    dry_run: bool = False
) -> Result:
    """Install all (or selected) skills to target directory."""
    to_install = skills if skills else SKILLS
    results = []
    failed = []

    for skill in to_install:
        if skill not in SKILLS:
            results.append(f"Unknown skill: {skill}")
            failed.append(skill)
            continue

        result = install_skill(skill, target_dir, dry_run)
        results.append(result.message)
        if not result.success:
            failed.append(skill)

    success = len(failed) == 0
    summary = f"Installed {len(to_install) - len(failed)}/{len(to_install)} skills"
    if dry_run:
        summary = f"[DRY RUN] {summary}"

    return Result(
        success=success,
        message=summary,
        data={"results": results, "failed": failed}
    )


def uninstall_all(
    target_dir: Path,
    skills: Optional[List[str]] = None,
    dry_run: bool = False
) -> Result:
    """Uninstall all (or selected) skills from target directory."""
    to_uninstall = skills if skills else SKILLS
    results = []

    for skill in to_uninstall:
        result = uninstall_skill(skill, target_dir, dry_run)
        results.append(result.message)

    summary = f"Uninstalled {len(to_uninstall)} skills"
    if dry_run:
        summary = f"[DRY RUN] {summary}"

    return Result(
        success=True,
        message=summary,
        data={"results": results}
    )


def main():
    parser = argparse.ArgumentParser(
        description="Install task-graph-mcp skills",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog="""
Examples:
  %(prog)s                              Install all to ~/.claude/skills/
  %(prog)s --target ~/my-skills/        Install to custom directory
  %(prog)s --skills worker,coordinator  Install specific skills only
  %(prog)s --list                       Show available skills
  %(prog)s --dry-run                    Preview installation
  %(prog)s --uninstall                  Remove installed skills
        """
    )

    parser.add_argument(
        "--target", "-t",
        type=Path,
        default=DEFAULT_TARGET,
        help=f"Installation directory (default: {DEFAULT_TARGET})"
    )

    parser.add_argument(
        "--skills", "-s",
        type=str,
        help="Comma-separated list of skills to install"
    )

    parser.add_argument(
        "--list", "-l",
        action="store_true",
        help="List available skills"
    )

    parser.add_argument(
        "--dry-run", "-n",
        action="store_true",
        help="Show what would be done without doing it"
    )

    parser.add_argument(
        "--uninstall", "-u",
        action="store_true",
        help="Uninstall skills instead of installing"
    )

    parser.add_argument(
        "--quiet", "-q",
        action="store_true",
        help="Minimal output"
    )

    args = parser.parse_args()

    # List mode
    if args.list:
        result = list_skills()
        if not args.quiet:
            print("Available skills:")
            skills_list = (result.data or {}).get("skills", [])
            for skill in skills_list:
                print(f"  - {skill}")
        return 0

    # Parse skill selection
    skills = None
    if args.skills:
        skills = [s.strip() for s in args.skills.split(",")]

    # Uninstall mode
    if args.uninstall:
        result = uninstall_all(args.target, skills, args.dry_run)
    else:
        result = install_all(args.target, skills, args.dry_run)

    # Output
    if not args.quiet:
        print(result.message)
        data = result.data or {}
        if "results" in data:
            for r in data["results"]:
                print(f"  {r}")
        if data.get("failed"):
            print(f"\nFailed: {', '.join(data['failed'])}")

    return 0 if result.success else 1


if __name__ == "__main__":
    sys.exit(main())
