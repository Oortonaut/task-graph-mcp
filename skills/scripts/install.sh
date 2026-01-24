#!/bin/bash
# Task Graph MCP Skills Installer (Shell wrapper)
#
# Usage:
#   ./install.sh                    # Install to ~/.claude/skills/
#   ./install.sh /path/to/target    # Install to custom location
#   ./install.sh --help             # Show Python script help

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# If first arg is a path (not starting with -), use as target
if [[ -n "$1" && ! "$1" =~ ^- ]]; then
    python3 "$SCRIPT_DIR/install.py" --target "$1" "${@:2}"
else
    python3 "$SCRIPT_DIR/install.py" "$@"
fi
