//! Demonstration of DepFile I/O operations
//!
//! This example shows how to use the dep_io module to manage dependency files.
//!
//! Run with: cargo run --example dep_io_demo

use bd_core::DepFile;
use bd_storage::{
    delete_dep_file, find_deps_for_task, read_all_dep_files, read_dep_file, write_dep_file,
};
use chrono::Utc;
use tempfile::TempDir;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create a temporary directory for the demo
    let temp_dir = TempDir::new()?;
    let deps_dir = temp_dir.path().join("deps");

    println!("Demo: DepFile I/O Operations");
    println!("=============================\n");

    // Create some dependency files
    println!("1. Creating dependency files...");

    let dep1 = DepFile {
        from: "task-001".to_string(),
        to: "task-002".to_string(),
        dep_type: "blocks".to_string(),
        created_at: Utc::now(),
    };

    let dep2 = DepFile {
        from: "task-002".to_string(),
        to: "task-003".to_string(),
        dep_type: "depends_on".to_string(),
        created_at: Utc::now(),
    };

    let dep3 = DepFile {
        from: "task-001".to_string(),
        to: "task-004".to_string(),
        dep_type: "related_to".to_string(),
        created_at: Utc::now(),
    };

    write_dep_file(&deps_dir, &dep1).await?;
    write_dep_file(&deps_dir, &dep2).await?;
    write_dep_file(&deps_dir, &dep3).await?;

    println!("   Created 3 dependency files:");
    println!("   - {}", dep1.to_filename());
    println!("   - {}", dep2.to_filename());
    println!("   - {}", dep3.to_filename());
    println!();

    // Read a single file
    println!("2. Reading a single dependency file...");
    let path = deps_dir.join(dep1.to_filename());
    let read_dep = read_dep_file(&path).await?;
    println!("   Read: {} --{}-> {}", read_dep.from, read_dep.dep_type, read_dep.to);
    println!();

    // Read all files
    println!("3. Reading all dependency files...");
    let all_deps = read_all_dep_files(&deps_dir).await?;
    println!("   Found {} dependencies:", all_deps.len());
    for dep in &all_deps {
        println!("   - {} --{}-> {}", dep.from, dep.dep_type, dep.to);
    }
    println!();

    // Find dependencies for a specific task
    println!("4. Finding dependencies for task-001...");
    let task_deps = find_deps_for_task(&deps_dir, "task-001").await?;
    println!("   Found {} dependencies involving task-001:", task_deps.len());
    for dep in &task_deps {
        println!("   - {} --{}-> {}", dep.from, dep.dep_type, dep.to);
    }
    println!();

    println!("5. Finding dependencies for task-002...");
    let task_deps = find_deps_for_task(&deps_dir, "task-002").await?;
    println!("   Found {} dependencies involving task-002:", task_deps.len());
    for dep in &task_deps {
        println!("   - {} --{}-> {}", dep.from, dep.dep_type, dep.to);
    }
    println!();

    // Delete a dependency
    println!("6. Deleting dependency: task-001 --blocks-> task-002");
    delete_dep_file(&deps_dir, "task-001", "blocks", "task-002").await?;
    println!("   Deleted successfully");
    println!();

    // Verify deletion
    println!("7. Reading all dependencies after deletion...");
    let all_deps = read_all_dep_files(&deps_dir).await?;
    println!("   Found {} dependencies:", all_deps.len());
    for dep in &all_deps {
        println!("   - {} --{}-> {}", dep.from, dep.dep_type, dep.to);
    }
    println!();

    // Test idempotent deletion
    println!("8. Testing idempotent deletion (deleting already-deleted file)...");
    delete_dep_file(&deps_dir, "task-001", "blocks", "task-002").await?;
    println!("   No error - deletion is idempotent");
    println!();

    println!("Demo completed successfully!");

    Ok(())
}
