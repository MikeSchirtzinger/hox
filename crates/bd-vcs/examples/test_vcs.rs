//! Example demonstrating VCS operations.
//!
//! Run with: cargo run --package bd-vcs --example test_vcs -- <repo_path>

use bd_vcs::Vcs;
use std::env;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== VCS Example ===\n");

    // Get repo path from command line or use current directory
    let repo_path = env::args().nth(1).unwrap_or_else(|| ".".to_string());
    println!("Testing with repo at: {}", repo_path);

    match Vcs::open(&repo_path) {
        Ok(vcs) => {
            println!("✓ Opened repository");
            println!("  VCS Type: {:?}", vcs.vcs_type());
            println!("  Repo Root: {}", vcs.repo_root().display());

            // Get current commit
            match vcs.current_commit() {
                Ok(commit) => println!("  Current commit: {}", commit),
                Err(e) => println!("  Error getting current commit: {}", e),
            }

            // Test find_files with a pattern
            println!("\nTesting file discovery...");
            match vcs.find_files("*.md") {
                Ok(files) => {
                    println!("✓ Found {} Markdown files", files.len());
                    if !files.is_empty() {
                        println!("  First 5 files:");
                        for file in files.iter().take(5) {
                            println!("    - {}", file.display());
                        }
                    }
                }
                Err(e) => println!("✗ Error finding files: {}", e),
            }

            // Test is_tracked
            println!("\nTesting file tracking...");
            let test_files = vec!["README.md", "Cargo.toml", "nonexistent.txt"];
            for test_file in test_files {
                match vcs.is_tracked(test_file) {
                    Ok(tracked) => {
                        let status = if tracked { "✓ tracked" } else { "✗ not tracked" };
                        println!("  {}: {}", test_file, status);
                    }
                    Err(e) => println!("  {}: error - {}", test_file, e),
                }
            }

            println!("\n✓ All VCS operations completed successfully!");
        }
        Err(e) => {
            println!("✗ Failed to open repository: {}", e);
            println!("\nTip: Run with a path to a Git repository:");
            println!("  cargo run --package bd-vcs --example test_vcs -- /path/to/git/repo");
            return Err(e.into());
        }
    }

    Ok(())
}
