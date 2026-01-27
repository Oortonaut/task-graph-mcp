# Task Graph MCP Skills Installer (PowerShell wrapper)
#
# Usage:
#   .\install.ps1                    # Install to ~/.claude/skills/
#   .\install.ps1 -Target C:\path    # Install to custom location
#   .\install.ps1 -Help              # Show Python script help

param(
    [string]$Target,
    [switch]$List,
    [switch]$DryRun,
    [switch]$Uninstall,
    [string]$Skills,
    [switch]$Quiet,
    [switch]$Help
)

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path

$args = @()

if ($Help) {
    $args += "--help"
}
if ($Target) {
    $args += "--target", $Target
}
if ($List) {
    $args += "--list"
}
if ($DryRun) {
    $args += "--dry-run"
}
if ($Uninstall) {
    $args += "--uninstall"
}
if ($Skills) {
    $args += "--skills", $Skills
}
if ($Quiet) {
    $args += "--quiet"
}

python "$ScriptDir\install.py" @args
