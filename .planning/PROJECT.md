# GraphSlice

## What This Is
GraphSlice is a compiler-driven context extraction tool for LLM code editing. It uses LSP (specifically `rust-analyzer`) to generate minimal, executable code slices based on program dependencies rather than text similarity, ensuring LLMs receive complete and correct context for editing tasks.

## Core Value
**Correctness by Construction**: The extracted slice must be a valid, compilable closure containing all necessary dependencies (callers, callees, types) for the target symbol, eliminating hallucinated or missing context.

## Requirements

### Validated

(None yet — ship to validate)

### Active

- [ ] **LSP Integration**: Communicate with `rust-analyzer` to retrieve symbol information and references.
- [ ] **Dependency Graph**: Build a directed graph of program dependencies (calls, uses, implements).
- [ ] **Bounded Closure**: Implement traversal algorithm with configurable depth and fanout limits.
- [ ] **Slice Extraction**: Extract and format source code for the computed slice.
- [ ] **CLI Interface**: `graphslice show` for inspecting slices.
- [ ] **LLM Integration**: `graphslice fix` to send slices to LLMs for editing (Milestone 2).

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
| **LSP Client Architecture** | Simpler to maintain and upgrade than linking against unstable `rust-analyzer` internal crates. | — Pending |
| **Rust-First** | Strong type system and mature LSP make it the ideal testbed for dependency slicing. | — Pending |
| **CLI Interface** | Easiest for composability and testing before building complex IDE plugins. | — Pending |

---
*Last updated: 2026-02-08 after project initialization*
