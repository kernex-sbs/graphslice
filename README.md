# GraphSlice

**Compiler-driven dependency slicing for LLM code editing**

Stop feeding LLMs entire files. Extract minimal, executable code slices based on actual program dependencies.

---

## The Problem

Current AI coding tools use text similarity to select context:
```
❌ User query → embeddings → top-k files → LLM
```
This retrieves *similar text*, not *program dependencies*. Result:
- Missing transitive callees → broken edits
- 80% irrelevant context → token waste
- No caller awareness → hidden regressions

## The Solution

GraphSlice uses compiler infrastructure (LSP) to compute minimal dependency closures:
```
✅ Target symbol → call graph → bounded closure → LLM
```

---

## Quick Start

### Installation

```bash
git clone https://github.com/kernex-sbs/graphslice
cd graphslice/graphslice
cargo build --release
```

### Usage

The current MVP supports analyzing Rust codebases using `rust-analyzer`.

```bash
# Syntax
target/release/graphslice <workspace_root> <relative_path_to_file> <line>:<column> [--max-tokens N]

# Example
# Analyze the 'Slicer::new' function in this repo
target/release/graphslice . src/slicer.rs 13:17
```

**Note:** Line and column numbers are 0-indexed.

### Output

GraphSlice produces a compressed context file `graphslice_context.txt` containing:
1. **Full Source** for the target function/struct.
2. **Full Source** for direct dependencies (callees, types used).
3. **Interface Summaries** (signatures only) for transitive dependencies to save tokens.
4. **References** for deep dependencies.

---

## Architecture

GraphSlice operates in phases:
1. **LSP Initialization**: Spawns `rust-analyzer` in the workspace.
2. **Graph Construction**:
   - Finds the definition of the target symbol.
   - Finds incoming references (callers).
   - Finds outgoing calls (callees) using Call Hierarchy.
   - Finds type definitions.
3. **Context Compression**:
   - Traverses the dependency graph (BFS).
   - Includes full code for immediate neighbors.
   - "Compresses" distant nodes into interface summaries (signatures).
4. **Output Generation**: Renders the context to a file.

---

## Status: MVP v0.2

- [x] **Core Rust Slicer**: Functional with `rust-analyzer`.
- [x] **Fuzzy Slicer**: Fallback to LLM-based slicing for code with errors.
- [x] **Dependency Graph**: Handles definitions, references, and outgoing calls.
- [x] **Hierarchical Compression**: Reduces context size by ~15x.
- [x] **Cross-File Resolution**: Follows dependencies across files in the workspace.
- [x] **Verification**: Z3/SMT solver integration for dead code elimination.
- [ ] **Multi-Language**: Python and C++ support planned.

---

## Configuration

GraphSlice automatically switches to **Fuzzy Slicer** if the target file has compilation errors. This requires an LLM provider (OpenAI-compatible).

Set the following environment variables:

```bash
export LLM_API_KEY="sk-..."
export LLM_BASE_URL="https://api.openai.com/v1"  # Optional, defaults to OpenAI
export LLM_MODEL="gpt-4o"                        # Optional, defaults to gpt-4o
```

## License

MIT
