//! Migration command for moving from deprecated `.task-graph/` to `task-graph/`.

use anyhow::{Context, Result};
use clap::Args;
use std::fs;
use std::path::Path;

/// Arguments for the migrate command.
#[derive(Args, Debug)]
pub struct MigrateArgs {
    /// Perform migration without prompting for confirmation.
    #[arg(short = 'y', long)]
    pub yes: bool,

    /// Show what would be migrated without making changes.
    #[arg(long)]
    pub dry_run: bool,

    /// Source directory (default: .task-graph)
    #[arg(long, default_value = ".task-graph")]
    pub from: String,

    /// Target directory (default: task-graph)
    #[arg(long, default_value = "task-graph")]
    pub to: String,
}

/// Run the migration command.
pub fn run_migrate(args: &MigrateArgs) -> Result<()> {
    let from = Path::new(&args.from);
    let to = Path::new(&args.to);

    // Check if source exists
    if !from.exists() {
        println!("No migration needed: '{}' does not exist.", args.from);
        return Ok(());
    }

    // Check if target already exists
    if to.exists() {
        println!(
            "Target directory '{}' already exists. Cannot migrate.",
            args.to
        );
        println!("Options:");
        println!("  1. Remove '{}' and run migrate again", args.to);
        println!("  2. Manually merge the directories");
        println!("  3. Use --to to specify a different target");
        return Ok(());
    }

    // Show what will be migrated
    println!("Migration plan:");
    println!("  From: {}", from.display());
    println!("  To:   {}", to.display());
    println!();

    // List contents
    let entries = list_directory_contents(from)?;
    if entries.is_empty() {
        println!("  (empty directory)");
    } else {
        for entry in &entries {
            println!("  {}", entry);
        }
    }
    println!();

    if args.dry_run {
        println!("Dry run: No changes made.");
        return Ok(());
    }

    // Confirm unless --yes
    if !args.yes {
        println!("This will move '{}' to '{}'.", args.from, args.to);
        print!("Continue? [y/N] ");
        use std::io::Write;
        std::io::stdout().flush()?;

        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;

        if !input.trim().eq_ignore_ascii_case("y") {
            println!("Migration cancelled.");
            return Ok(());
        }
    }

    // Perform the migration
    println!("Migrating...");
    fs::rename(from, to).context("Failed to rename directory")?;

    println!("Migration complete!");
    println!();
    println!("Your configuration has been moved to '{}'.", args.to);
    println!();
    println!("If you have any scripts or configurations that reference '.task-graph/',");
    println!("please update them to use 'task-graph/' instead.");

    Ok(())
}

/// List directory contents recursively (for display).
fn list_directory_contents(dir: &Path) -> Result<Vec<String>> {
    let mut entries = Vec::new();
    list_directory_recursive(dir, dir, &mut entries)?;
    entries.sort();
    Ok(entries)
}

fn list_directory_recursive(base: &Path, dir: &Path, entries: &mut Vec<String>) -> Result<()> {
    if !dir.is_dir() {
        return Ok(());
    }

    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let relative = path.strip_prefix(base).unwrap_or(&path);

        if path.is_dir() {
            entries.push(format!("{}/", relative.display()));
            list_directory_recursive(base, &path, entries)?;
        } else {
            entries.push(relative.display().to_string());
        }
    }

    Ok(())
}

/// Check if migration is recommended and print a warning.
pub fn check_and_warn_deprecated() {
    let deprecated = Path::new(".task-graph");
    let new_location = Path::new("task-graph");

    if deprecated.exists() && !new_location.exists() {
        eprintln!();
        eprintln!("Warning: Using deprecated directory '.task-graph/'.");
        eprintln!("Run 'task-graph migrate' to move to 'task-graph/'.");
        eprintln!();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use tempfile::TempDir;

    fn create_test_structure(temp: &TempDir) -> PathBuf {
        let base = temp.path().to_path_buf();
        let deprecated = base.join(".task-graph");
        
        // Create deprecated directory structure
        fs::create_dir_all(deprecated.join("skills")).unwrap();
        fs::create_dir_all(deprecated.join("media")).unwrap();
        fs::write(deprecated.join("config.yaml"), "server:\n  claim_limit: 10\n").unwrap();
        fs::write(deprecated.join("tasks.db"), "fake-db-content").unwrap();
        fs::write(deprecated.join("skills/custom.md"), "# Custom Skill").unwrap();
        
        base
    }

    #[test]
    fn test_migrate_dry_run_no_changes() {
        let temp = TempDir::new().unwrap();
        let base = create_test_structure(&temp);
        
        let args = MigrateArgs {
            yes: false,
            dry_run: true,
            from: base.join(".task-graph").to_string_lossy().to_string(),
            to: base.join("task-graph").to_string_lossy().to_string(),
        };
        
        run_migrate(&args).unwrap();
        
        // Source should still exist
        assert!(base.join(".task-graph").exists());
        // Target should not exist
        assert!(!base.join("task-graph").exists());
    }

    #[test]
    fn test_migrate_moves_directory() {
        let temp = TempDir::new().unwrap();
        let base = create_test_structure(&temp);
        
        let args = MigrateArgs {
            yes: true,  // Skip confirmation
            dry_run: false,
            from: base.join(".task-graph").to_string_lossy().to_string(),
            to: base.join("task-graph").to_string_lossy().to_string(),
        };
        
        run_migrate(&args).unwrap();
        
        // Source should no longer exist
        assert!(!base.join(".task-graph").exists());
        // Target should exist with all contents
        assert!(base.join("task-graph").exists());
        assert!(base.join("task-graph/config.yaml").exists());
        assert!(base.join("task-graph/tasks.db").exists());
        assert!(base.join("task-graph/skills/custom.md").exists());
        
        // Verify content
        let config = fs::read_to_string(base.join("task-graph/config.yaml")).unwrap();
        assert!(config.contains("claim_limit: 10"));
    }

    #[test]
    fn test_migrate_no_source() {
        let temp = TempDir::new().unwrap();
        let base = temp.path();
        
        let args = MigrateArgs {
            yes: true,
            dry_run: false,
            from: base.join(".task-graph").to_string_lossy().to_string(),
            to: base.join("task-graph").to_string_lossy().to_string(),
        };
        
        // Should succeed with "no migration needed" message
        run_migrate(&args).unwrap();
    }

    #[test]
    fn test_migrate_target_exists() {
        let temp = TempDir::new().unwrap();
        let base = create_test_structure(&temp);
        
        // Create target directory
        fs::create_dir_all(base.join("task-graph")).unwrap();
        
        let args = MigrateArgs {
            yes: true,
            dry_run: false,
            from: base.join(".task-graph").to_string_lossy().to_string(),
            to: base.join("task-graph").to_string_lossy().to_string(),
        };
        
        // Should succeed but not migrate (target exists)
        run_migrate(&args).unwrap();
        
        // Both should still exist
        assert!(base.join(".task-graph").exists());
        assert!(base.join("task-graph").exists());
    }
}
