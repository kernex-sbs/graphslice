# GraphSlice

## What This Is
GraphSlice is a compiler-driven context extraction tool for LLM code editing. It uses LSP (specifically `rust-analyzer`) to generate minimal, executable code slices based on program dependencies rather than text similarity, ensuring LLMs receive complete and correct context for editing tasks.

## Core Value
**Correctness by Construction**: The extracted slice must be a valid, compilable closure containing all necessary dependencies (callers, callees, types) for the target symbol, eliminating hallucinated or missing context.

## Requirements

### Validated

- [x] **LSP Integration**: `lsp_client.rs` successfully communicates with `rust-analyzer` (requests, notifications, content modification retries).
- [x] **Dependency Graph**: `graph.rs` and `slicer.rs` build a graph with definitions, references, and outgoing calls (Call Hierarchy).
- [x] **Bounded Closure**: Hierarchical context implemented in `compression.rs`.
- [x] **CLI Interface**: Basic CLI functional and tested.
- [x] **Integration Test**: `tests/integration_test.rs` validates the full pipeline.
- [x] **Robust Code Extraction**: `extractor.rs` uses `tree-sitter` for accurate function/struct extraction.
- [x] **Phase 2: Fuzzy Slicer**: Implemented LLM-guided slicing for broken code (Engine B) using `tree-sitter` for definitions and LLM for call graph analysis.
- [x] **Phase 3: Verification**: Added Z3 integration (`verifier.rs`) to prune unreachable code paths based on static integer constraints.

### Active

### Out of Scope

- **Multi-language support**: Python, JS, C++ are explicitly deferred to later milestones.
- **Editor Plugins**: VS Code/Neovim integration is out of scope for MVP.
- **Repository-wide analysis**: We focus on bounded slices, not whole-program optimization.

## Context

Current LLM coding tools rely on RAG (Retrieval Augmented Generation) which uses semantic similarity. This often misses structural dependencies (e.g., a function that calls the target but uses different terminology). GraphSlice inverts this by using the compiler's knowledge of the code structure.

We are implementing this in **Rust**, targeting **Rust** codebases first, using **LSP** architecture.

## Constraints

- **Language**: Implemented in Rust.
- **Target**: Rust codebases only (v1).
- **Architecture**: Must interface with `rust-analyzer` as an external LSP process (not as a library).
- **Interface**: CLI-first design.

## Key Decisions

| Decision | Rationale | Outcome |
|----------|-----------|---------|
| **LSP Client Architecture** | Simpler to maintain and upgrade than linking against unstable `rust-analyzer` internal crates. | ✅ Done |
| **Rust-First** | Strong type system and mature LSP make it the ideal testbed for dependency slicing. | ✅ In Progress |
| **CLI Interface** | Easiest for composability and testing before building complex IDE plugins. | ✅ Done |

---
*Last updated: 2026-02-08 after project initialization*
