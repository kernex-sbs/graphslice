# How-To Guide: GraphSlice

This guide provides practical examples and workflows for using GraphSlice to extract minimal code context for LLMs.

## Table of Contents
1. [Prerequisites](#prerequisites)
2. [Scenario 1: Standard Slicing (Valid Code)](#scenario-1-standard-slicing-valid-code)
3. [Scenario 2: Fuzzy Slicing (Broken Code)](#scenario-2-fuzzy-slicing-broken-code)
4. [Scenario 3: Verification & Pruning](#scenario-3-verification--pruning)
5. [Using the Output with LLMs](#using-the-output-with-llms)

---

## Prerequisites

Before you begin, ensure you have the following installed:
- **Rust Toolchain**: `cargo` (via rustup)
- **Rust Analyzer**: `rust-analyzer` binary in your PATH
- **Z3 Library**: GraphSlice bundles Z3, but having `libz3-dev` (Ubuntu) or `z3` (macOS) helps if compilation fails.

Build the release binary:
```bash
cd graphslice/graphslice
cargo build --release
# Binary location: target/release/graphslice
```

---

## Scenario 1: Standard Slicing (Valid Code)

**Goal**: Extract context for a specific function in a healthy Rust project.

1. **Identify the Target**:
   Navigate to your target project. Let's say you want to refactor `process_data` in `src/main.rs` at line 50.

2. **Run GraphSlice**:
   ```bash
   # Usage: graphslice <workspace_root> <file_path> <line>:<column>
   ./target/release/graphslice . src/main.rs 50:10
   ```

3. **Check the Output**:
   The tool generates `graphslice_context.txt`. Open it to see:
   - **Target Code**: Full source of `process_data`.
   - **Dependencies**: Full source of functions called by `process_data`.
   - **Type Definitions**: Structs used in the function signature.
   - **Compression**: Distant dependencies (depth > 2) are replaced with function signatures (`fn foo(...);`) to save tokens.

---

## Scenario 2: Fuzzy Slicing (Broken Code)

**Goal**: Get context for a function that currently has syntax errors (e.g., during a refactor).

**Note**: This requires an LLM API key.

1. **Configure Environment**:
   ```bash
   export LLM_API_KEY="sk-..."
   export LLM_MODEL="gpt-4o" # or "claude-3-5-sonnet-20241022"
   ```

2. **The "Broken" State**:
   Suppose `src/main.rs` has a missing semicolon or invalid type:
   ```rust
   fn broken_func() {
       let x = unknown_function(  // <-- Syntax error
   }
   ```

3. **Run GraphSlice**:
   Run the same command as normal:
   ```bash
   ./target/release/graphslice . src/main.rs 60:5
   ```

4. **Observe Behavior**:
   - GraphSlice detects the compilation error via LSP diagnostics.
   - It prints: `⚠️ File has errors. Switching to Fuzzy (LLM) Slicer.`
   - It uses Tree-sitter to parse the partial AST.
   - It asks the LLM to infer likely calls (e.g., "It looks like you're calling `unknown_function`").
   - It generates a best-effort context slice.

---

## Scenario 3: Verification & Pruning

**Goal**: Eliminate unreachable code from the context to save tokens.

1. **Code with Dead Paths**:
   Imagine this code:
   ```rust
   fn complex_logic(x: i32) {
       let mode = 1;
       if mode > 5 {
           heavy_dependency(); // <-- This is unreachable
       }
   }
   ```

2. **Run GraphSlice**:
   ```bash
   ./target/release/graphslice . src/lib.rs 10:5
   ```

3. **Verification in Action**:
   - The **Verifier** module analyzes integer constraints.
   - It sees `mode = 1` and `if mode > 5`.
   - It proves `1 > 5` is `UNSAT` (unsatisfiable).
   - It prints: `✂️ Pruned unreachable code...`
   - The resulting `graphslice_context.txt` will **NOT** include the source code for `heavy_dependency`, saving valuable context window space.

---

## Using the Output with LLMs

Once you have `graphslice_context.txt`, use it to prompt your LLM efficiently.

**Example Prompt**:

> I am refactoring the function `process_data`.
> I have attached the dependency slice in `graphslice_context.txt`.
> This file contains the target function and all its relevant dependencies (callers, callees, and types).
> Transitive dependencies are summarized as interfaces.
>
> Please refactor `process_data` to handle async streams.

**Why this works better than RAG**:
- The LLM can see the **exact** structs definition.
- It knows **who calls** the function (so it doesn't break API compatibility).
- It doesn't get distracted by unrelated files in the repository.
