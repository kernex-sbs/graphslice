use graphslice::Slicer;
use std::path::PathBuf;
use std::time::Instant;

#[tokio::main]
async fn main() {
    println!("GraphSlice vs RAG Benchmark\n");

    // Test on real Rust project (e.g., a small crate)
    let workspace = PathBuf::from("../test_projects/tokio");
    
    if !workspace.exists() {
        eprintln!("Clone tokio for testing: git clone https://github.com/tokio-rs/tokio test_projects/tokio");
        return;
    }

    let target_file = workspace.join("tokio/src/runtime/mod.rs");
    
    // GraphSlice timing
    let start = Instant::now();
    let mut slicer = Slicer::new(workspace.clone()).await.unwrap();
    let graph = slicer.build_graph(target_file.clone(), 10, 5).await.unwrap();
    let context = slicer.extract_context(&graph, 2);
    let graphslice_time = start.elapsed();
    let graphslice_tokens = context.len() / 4;

    // Simulate RAG (naive: include entire file + imports)
    let start = Instant::now();
    let rag_context = simulate_rag(&target_file);
    let rag_time = start.elapsed();
    let rag_tokens = rag_context.len() / 4;

    println!("Results:");
    println!("  GraphSlice: {} tokens in {:?}", graphslice_tokens, graphslice_time);
    println!("  RAG:        {} tokens in {:?}", rag_tokens, rag_time);
    println!();
    println!("  Token reduction: {:.1}x", rag_tokens as f32 / graphslice_tokens as f32);
    println!("  Speed: {:.1}x {}", 
        rag_time.as_millis() as f32 / graphslice_time.as_millis() as f32,
        if graphslice_time < rag_time { "faster" } else { "slower" }
    );
}

fn simulate_rag(file: &PathBuf) -> String {
    // RAG: read entire file + all files in directory
    let mut context = std::fs::read_to_string(file).unwrap();
    
    let dir = file.parent().unwrap();
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().map_or(false, |e| e == "rs") {
            if let Ok(content) = std::fs::read_to_string(&path) {
                context.push_str("\n\n");
                context.push_str(&content);
            }
        }
    }
    
    context
}