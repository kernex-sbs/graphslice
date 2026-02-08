use graphslice::Slicer;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

#[tokio::test]
async fn test_basic_slicing() {
    // Create test project
    let test_dir = create_test_project();
    println!("Created test project at: {}", test_dir.display());

    let mut slicer = Slicer::new(test_dir.clone())
        .await
        .expect("Failed to create slicer");

    let target_file = test_dir.join("src/main.rs").canonicalize().unwrap();
    println!("Targeting file: {}", target_file.display());

    // Target the definition of 'helper' on line 2 (0-indexed line 1), column 3
    // content: "fn helper(x: i32) -> i32 {"
    let graph = slicer
        .build_graph(target_file.clone(), 1, 3)
        .await
        .expect("Failed to build graph");

    println!("Graph nodes: {:?}", graph.nodes.keys());
    println!("Graph edges: {:?}", graph.edges);

    assert!(!graph.nodes.is_empty(), "Should find nodes");
    assert!(!graph.edges.is_empty(), "Should find edges");

    // Verify we found the call site in main
    let found_main_call = graph.nodes.values().any(|node|
        node.node_type == "reference" && node.code.contains("helper(5)")
    );
    assert!(found_main_call, "Should find usage of helper in main");

    // Clean up
    std::fs::remove_dir_all(test_dir).ok();
}

#[tokio::test]
async fn test_fuzzy_slicing_on_error() {
    // Set test mode for LLM client
    // SAFETY: This is a test, and we are setting the env var before running the Slicer.
    // In a real scenario, we should avoid setting env vars in multi-threaded contexts if possible,
    // but for this integration test it is necessary to trigger the mock mode.
    unsafe {
        std::env::set_var("GRAPHSLICE_TEST_MODE", "1");
    }

    // Create broken project
    let test_dir = create_broken_project();
    println!("Created broken project at: {}", test_dir.display());

    let mut slicer = Slicer::new(test_dir.clone())
        .await
        .expect("Failed to create slicer");

    let target_file = test_dir.join("src/main.rs").canonicalize().unwrap();

    // Target 'main' function
    // content: "fn main() {\n    helper(10);\n    invalid_syntax!!!!\n}"
    // Line 0 is fn main
    let graph = slicer
        .build_graph(target_file.clone(), 0, 3)
        .await
        .expect("Failed to build graph");

    // We expect fuzzy slicer to find 'helper' call from the mock LLM response
    let found_helper_call = graph.edges.iter().any(|edge| {
        edge.edge_type == graphslice::EdgeType::Calls &&
        graph.nodes.get(&edge.to).map(|n| n.code.contains("fn helper")).unwrap_or(false)
    });

    // Note: The mock LLM returns "helper", and our fuzzy slicer looks up "helper" in the workspace.
    // The broken project DOES define "helper", so it should find it.

    assert!(found_helper_call, "Fuzzy slicer should find call to helper despite syntax errors");

    std::fs::remove_dir_all(test_dir).ok();
}

#[tokio::test]
async fn test_dead_code_elimination() {
    // Create project with dead code
    let test_dir = create_dce_project();
    println!("Created DCE project at: {}", test_dir.display());

    let mut slicer = Slicer::new(test_dir.clone())
        .await
        .expect("Failed to create slicer");

    let target_file = test_dir.join("src/main.rs").canonicalize().unwrap();

    // Target 'main' function at line 9 (fn main)
    let graph = slicer
        .build_graph(target_file.clone(), 9, 3)
        .await
        .expect("Failed to build graph");

    // Check edges from main
    // We expect a call to 'reachable_fn'
    // We expect NO call to 'unreachable_fn'

    let found_reachable = graph.edges.iter().any(|edge| {
        edge.edge_type == graphslice::EdgeType::Calls &&
        graph.nodes.get(&edge.to).map(|n| n.code.contains("fn reachable_fn")).unwrap_or(false)
    });

    let found_unreachable = graph.edges.iter().any(|edge| {
        edge.edge_type == graphslice::EdgeType::Calls &&
        graph.nodes.get(&edge.to).map(|n| n.code.contains("fn unreachable_fn")).unwrap_or(false)
    });

    println!("Found reachable: {}", found_reachable);
    println!("Found unreachable: {}", found_unreachable);

    assert!(found_reachable, "Should find reachable function");
    assert!(!found_unreachable, "Should prune unreachable function");

    std::fs::remove_dir_all(test_dir).ok();
}

fn create_dce_project() -> PathBuf {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis();
    let dir = std::env::temp_dir().join(format!("graphslice_dce_test_{}", timestamp));
    std::fs::create_dir_all(dir.join("src")).unwrap();

    // Create Cargo.toml
    std::fs::write(
        dir.join("Cargo.toml"),
        r#"
[package]
name = "dce_project"
version = "0.1.0"
edition = "2021"

[dependencies]
        "#,
    ).unwrap();

    // Create main.rs
    std::fs::write(
        dir.join("src/main.rs"),
        r#"
fn reachable_fn() {
    println!("I am reachable");
}

fn unreachable_fn() {
    println!("I am NOT reachable");
}

fn main() {
    let x = 10;

    if x > 5 {
        reachable_fn();
    }

    if x < 5 {
        unreachable_fn();
    }
}
        "#,
    ).unwrap();

    // Run cargo check
    std::process::Command::new("cargo")
        .arg("check")
        .current_dir(&dir)
        .output()
        .expect("Failed to run cargo check");

    dir
}

fn create_broken_project() -> PathBuf {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis();
    let dir = std::env::temp_dir().join(format!("graphslice_fuzzy_test_{}", timestamp));
    std::fs::create_dir_all(dir.join("src")).unwrap();

    // Create Cargo.toml
    std::fs::write(
        dir.join("Cargo.toml"),
        r#"
[package]
name = "broken_project"
version = "0.1.0"
edition = "2021"

[dependencies]
        "#,
    ).unwrap();

    // Create main.rs with syntax error
    std::fs::write(
        dir.join("src/main.rs"),
        r#"
fn helper(x: i32) {
    println!("{}", x);
}

fn main() {
    helper(10);
    let x = ; // Syntax error here
}
        "#,
    ).unwrap();

    // We don't run cargo check because it would fail
    // But we need the file to exist.

    dir
}

fn create_test_project() -> PathBuf {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis();
    let dir = std::env::temp_dir().join(format!("graphslice_test_{}", timestamp));
    std::fs::create_dir_all(dir.join("src")).unwrap();

    // Create Cargo.toml
    std::fs::write(
        dir.join("Cargo.toml"),
        r#"
[package]
name = "test_project"
version = "0.1.0"
edition = "2021"

[dependencies]
        "#,
    )
    .unwrap();

    // Create main.rs
    std::fs::write(
        dir.join("src/main.rs"),
        r#"
fn helper(x: i32) -> i32 {
    x + 1
}

fn main() {
    let value = helper(5);
    println!("{}", value);
}
        "#,
    )
    .unwrap();

    // Run cargo check to generate lockfile and metadata
    std::process::Command::new("cargo")
        .arg("check")
        .current_dir(&dir)
        .output()
        .expect("Failed to run cargo check");

    dir
}