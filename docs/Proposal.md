# **GraphSlice: Compiler-Driven Context Extraction for LLM Code Editing**

**A Static Analysis Framework for Minimal, Executable Code Slices**

---

## Abstract

Current LLM coding tools treat source code as unstructured text, using semantic embeddings to retrieve context. This approach is fundamentally misaligned with how programs are structured: it retrieves *textually similar* code rather than *programmatically dependent* code. The result is token waste, incomplete context, and edit-induced regressions.

We propose **GraphSlice**, a compiler-aware dependency slicer that extracts minimal executable closures around target symbols. By leveraging language server protocols (LSPs) and compiler infrastructure, GraphSlice computes bounded dependency graphs—upstream callers, downstream callees, and transitive type dependencies—and feeds only this structured slice to an LLM.

This inverts the current paradigm: instead of asking LLMs to guess what's relevant, we use program analysis to compute relevance, then delegate editing to the LLM within that bounded context.

Our design is grounded in CodeWiki's empirical finding that **dependency-driven decomposition with hierarchical synthesis outperforms whole-repository prompting**, achieving 4.73% improvement over commercial baselines. GraphSlice adapts this architecture from documentation generation to interactive code modification.

**Key insight:** LLMs don't need bigger context windows. They need better dependency graphs.

[paper for dependency graph](https://arxiv.org/abs/2510.24428)
---

## 1. Motivation: The Context Selection Problem

### 1.1 Current State: Retrieval-Based Context

Modern AI coding assistants follow this pipeline:

```
User query → embedding similarity → top-k files → LLM edit
```

**Problems:**
- **Semantic drift:** "Similar text" ≠ "program dependencies"
- **Incomplete closures:** Missing transitive callees breaks edits
- **Token inefficiency:** 80% of retrieved context is irrelevant
- **No structural awareness:** Cannot distinguish caller/callee direction

**Example failure case:**
```rust
// User asks: "optimize authenticate_user for speed"
// Retrieval returns: auth.rs, user.rs, session.rs
// But misses: crypto.rs (called by auth), metrics.rs (caller)
// LLM suggests changes that break crypto integration
```

### 1.2 Why This Fails: Program Structure ≠ Text Similarity

Source code has **two representations**:
1. **Surface text** (what embeddings see)
2. **Execution graph** (what compilers see)

Embeddings capture (1), but program correctness depends on (2).

**The core problem:** Retrieving contextually similar functions doesn't guarantee you've retrieved *programmatically necessary* functions.

---

## 2. Core Insight: Compilers Already Solve This

Language servers and compilers maintain:
- **Symbol tables** (what exists)
- **Call graphs** (who calls whom)
- **Type dependency graphs** (what depends on what)
- **Module boundaries** (architectural structure)

**GraphSlice's thesis:** Use this existing infrastructure to extract context, not heuristics.

---

## 3. Technical Approach

### 3.1 Definition: Program Slice

A **program slice** for symbol `s` is the minimal subset of code required to understand or modify `s` without breaking correctness.

**Formal definition:**
```
Slice(s) = {s} ∪ Callers(s) ∪ Callees(s) ∪ TypeDeps(s)
```

Where:
- `Callers(s)`: Functions that invoke `s` (upstream)
- `Callees(s)`: Functions `s` invokes (downstream)
- `TypeDeps(s)`: Structs/traits/interfaces `s` uses or implements

### 3.2 Bounded Dependency Closure

Naïve transitive closure can explode to entire codebase. We compute a **bounded closure** with limits:

**Parameters:**
- `max_depth`: Traversal depth (default: 2)
- `max_fanout`: Max callees per function (default: 10)
- `boundary_policy`: Stop at module/crate boundaries

**Algorithm:**
```
1. Start: target symbol s
2. BFS/DFS traversal on dependency graph
3. Collect nodes within bounds
4. Extract source code for collected symbols only
5. Emit as structured slice (JSON/AST)
```

### 3.3 Rust Implementation Architecture

**Phase 1 focuses on Rust** for three reasons:
1. `rust-analyzer` provides production-grade LSP
2. Static analysis is reliable (no dynamic dispatch ambiguity)
3. Strong type system makes dependency extraction precise

**Tooling stack:**
- **LSP client:** Communicate with `rust-analyzer`
- **Graph engine:** Build and traverse dependency DAG
- **Slice extractor:** Emit minimal code + metadata
- **LLM adapter:** Format slice for model consumption

**Data structures:**
```rust
struct DependencyGraph {
    nodes: HashMap<SymbolId, Symbol>,
    edges: Vec<(SymbolId, SymbolId, EdgeType)>,
}

enum EdgeType {
    Calls,        // Function call
    Uses,         // Type usage
    Implements,   // Trait implementation
    Imports,      // Module dependency
}

struct Slice {
    target: Symbol,
    dependencies: Vec<Symbol>,
    metadata: SliceMetadata,
}
```

---

## 4. GraphSlice Architecture

### 4.1 High-Level Pipeline

```
┌─────────────┐
│ User Intent │ "Fix login_user performance"
└──────┬──────┘
       │
       v
┌─────────────────┐
│ Symbol Resolver │ → crate::auth::login_user
└────────┬────────┘
         │
         v
┌──────────────────┐
│ Dependency Graph │ → Build call graph + type graph
└────────┬─────────┘
         │
         v
┌─────────────────┐
│ Bounded Closure │ → Apply depth/fanout limits
└────────┬────────┘
         │
         v
┌────────────────┐
│ Slice Emitter  │ → Extract source code
└────────┬───────┘
         │
         v
┌────────────────┐
│ LLM Invocation │ → Send slice + prompt
└────────┬───────┘
         │
         v
┌────────────────┐
│ Patch Output   │ → Unified diff
└────────────────┘
```

### 4.2 CLI Interface

**Basic usage:**
```bash
# Extract and display slice
graphslice show crate::auth::login_user

# Generate LLM-assisted fix
graphslice fix crate::auth::login_user \
  --prompt "Optimize for throughput" \
  --model claude-sonnet-4

# Export slice for external tools
graphslice export crate::auth::login_user \
  --format json \
  --output slice.json
```

**Advanced options:**
```bash
graphslice fix target_fn \
  --max-depth 3 \
  --max-fanout 15 \
  --boundary crate \
  --include-tests
```

### 4.3 Output Format

**Slice JSON schema:**
```json
{
  "target": {
    "name": "login_user",
    "file": "src/auth.rs",
    "line": 42,
    "kind": "function"
  },
  "dependencies": [
    {
      "name": "hash_password",
      "file": "src/crypto.rs",
      "relation": "callee"
    },
    {
      "name": "validate_session",
      "file": "src/session.rs",
      "relation": "caller"
    }
  ],
  "source_code": {
    "auth.rs": "...",
    "crypto.rs": "..."
  },
  "metadata": {
    "depth_reached": 2,
    "nodes_collected": 8,
    "boundary": "crate"
  }
}
```

---

## 5. Theoretical Foundation: Why This Works

### 5.1 Alignment with CodeWiki Findings

The CodeWiki paper demonstrates that:

**Finding 1:** AST-derived dependency graphs preserve architectural coherence
- **GraphSlice application:** We use LSP (which consumes ASTs) to extract the same dependency relations

**Finding 2:** Hierarchical decomposition scales to large repositories
- **GraphSlice application:** Bounded closure with depth limits provides the same hierarchical structure

**Finding 3:** Bottom-up synthesis outperforms whole-repo prompting
- **GraphSlice application:** We extract only the *necessary* bottom-up closure, not the entire repository

**Finding 4:** Dependency-aware processing improves coverage (68.79% vs 64.06%)
- **GraphSlice application:** Our slices are *guaranteed* complete by construction (all dependencies included)

**Key difference:** CodeWiki applies this to documentation (one-time generation), GraphSlice applies it to editing (interactive modification).

### 5.2 Comparison to Existing Tools

| Approach | Context Method | Completeness | Token Efficiency |
|----------|---------------|--------------|------------------|
| Cursor | Embeddings | Heuristic | Low (80% waste) |
| Copilot | File-level | Incomplete | Medium |
| **GraphSlice** | **Dependency graph** | **Guaranteed** | **High (minimal)** |

---

## 6. Evaluation Strategy

### 6.1 Metrics

**Correctness:**
- Does slice compilation succeed?
- Are all dependencies present?
- Are type constraints satisfied?

**Efficiency:**
- Token count reduction vs. file-level context
- Slice size vs. repository size

**Edit Quality:**
- LLM success rate with slice vs. full files
- Regression rate (tests pass before/after)

### 6.2 Benchmark Tasks

1. **Optimization:** "Make this function faster"
2. **Refactoring:** "Extract this into a helper"
3. **Bug fixing:** "Fix this null pointer dereference"
4. **Feature addition:** "Add logging to this module"

### 6.3 Baselines

- **Full-file context:** Entire file containing target function
- **Embedding retrieval:** Top-5 semantically similar files
- **Manual selection:** Human expert selects relevant code

---

## 7. Roadmap

### Milestone 1: Core Rust Slicer (Weeks 1-4)
- LSP integration with `rust-analyzer`
- Dependency graph construction
- Bounded closure algorithm
- CLI for slice display

**Deliverable:** `graphslice show` command works on real Rust codebases

### Milestone 2: LLM Integration (Weeks 5-8)
- LLM adapter (Claude, GPT-4, Gemini)
- Prompt engineering for slice-based editing
- Diff generation and validation
- Basic evaluation on benchmark tasks

**Deliverable:** `graphslice fix` generates valid patches

### Milestone 3: Python Support (Weeks 9-12)
- Integrate `pyright` / `jedi`
- Handle dynamic typing challenges
- Adapt graph construction for Python semantics

**Deliverable:** GraphSlice works on Python codebases

### Milestone 4: C/C++ Support (Weeks 13-16)
- Integrate `clangd` / `libclang`
- Handle preprocessor and header complexity
- Translation-unit aware analysis

**Deliverable:** GraphSlice works on systems programming languages

### Milestone 5: API & Ecosystem (Weeks 17-20)
- JSON API for external tool integration
- Language server protocol extension
- VSCode/Neovim plugin wrappers

**Deliverable:** Other tools can consume GraphSlice outputs

---

## 8. Expected Impact

### 8.1 For Developers
- **Faster onboarding:** Understand unfamiliar code via precise slices
- **Safer refactoring:** See all affected callers before editing
- **Better prompts:** Feed LLMs minimal, relevant context

### 8.2 For Research
- **Benchmark:** Standardized way to evaluate context selection
- **Comparison:** Quantify retrieval vs. analysis approaches
- **Foundation:** Enable dependency-aware agentic systems

### 8.3 For Industry
- **Cost reduction:** 50-80% fewer tokens per edit
- **Quality improvement:** Fewer regressions from incomplete context
- **Composability:** Drop-in replacement for embedding retrieval

---

## 9. Why This Will Succeed

### 9.1 Technical Advantages
1. **Correctness by construction:** Compiler guarantees completeness
2. **Language-agnostic design:** LSP standardization enables multi-language support
3. **Composable architecture:** Works with any LLM backend

### 9.2 Adoption Strategy

**Target users:**
- Systems programmers (Rust, C, C++)
- Backend engineers (Python, Java)
- Infrastructure teams (large codebases)

**Positioning:**
- Not "another AI coding assistant"
- A **compiler tool that happens to use LLMs**

**Distribution:**
- Standalone CLI (primary)
- Editor plugins (secondary)
- API for agent integration (tertiary)

### 9.3 Why Now?

1. **LSP maturity:** rust-analyzer, pyright, clangd are production-ready
2. **LLM capability:** Models can finally handle precise instructions
3. **Context limits:** 200K tokens still insufficient for large repos
4. **Cost pressure:** Token efficiency matters at scale

---

## 10. Limitations and Future Work

### 10.1 Known Limitations
- **Dynamic languages:** Python/JS have ambiguous call graphs
- **Macros:** Rust macros complicate dependency extraction
- **Cross-language:** Polyglot repos need multiple analyzers

### 10.2 Future Extensions
- **Incremental slicing:** Update slices as code changes
- **Probabilistic edges:** Handle dynamic dispatch with confidence scores
- **Multi-target slicing:** Optimize multiple functions simultaneously
- **Version-aware:** Track dependency changes across commits

---

## 11. Success Criteria

**Minimum Viable Product:**
- ✅ Rust slice extraction with <5% false negatives
- ✅ 50% token reduction vs. file-level context
- ✅ LLM edit success rate > 80% on benchmark

**Adoption Target (6 months):**
- 1,000 GitHub stars
- 10 external contributors
- Integration in 1 popular editor/agent

**Research Impact:**
- 1 workshop/conference paper
- Cited by 5 follow-up projects
- Used in 2 academic benchmarks

---

## 12. Related Work Differentiation

| System | Approach | GraphSlice Advantage |
|--------|----------|---------------------|
| CodeWiki | Documentation via dependency graphs | We apply same principle to **editing** |
| RepoAgent | Function-level aggregation | We do **hierarchical synthesis** |
| DocAgent | Multi-agent with topological order | We add **bounded closure** |
| Cursor | Embedding retrieval | We use **compiler analysis** |

---

## 13. Conclusion

GraphSlice brings **program analysis rigor** to LLM-assisted coding.

Instead of asking LLMs to retrieve context from text similarity, we compute context from program structure. Instead of expanding context windows, we shrink context intelligently.

This is not a novel idea—it's **program slicing**, a 40-year-old compiler technique. The innovation is applying it to LLM workflows.

**The hypothesis:**
> Compiler-extracted slices will outperform embedding-retrieved context for code editing tasks, especially in large codebases.

**The validation:**
> Build it, benchmark it, and measure token efficiency + edit correctness.

**The vision:**
> Every coding agent should have access to a dependency slicer. GraphSlice will be that tool.

---

## Appendix A: Detailed Technical Design

### A.1 Dependency Graph Schema

```rust
pub struct DepGraph {
    symbols: HashMap<SymbolId, SymbolNode>,
    edges: Vec<Edge>,
}

pub struct SymbolNode {
    id: SymbolId,
    name: String,
    location: Location,
    kind: SymbolKind,
    visibility: Visibility,
}

pub enum SymbolKind {
    Function,
    Struct,
    Trait,
    Enum,
    Module,
    Const,
}

pub struct Edge {
    from: SymbolId,
    to: SymbolId,
    kind: EdgeKind,
    metadata: EdgeMetadata,
}

pub enum EdgeKind {
    Calls,
    Uses,
    Implements,
    Imports,
    Derives,
}
```

### A.2 Slice Extraction Algorithm

```python
def extract_slice(target: Symbol, graph: DepGraph, config: Config) -> Slice:
    visited = set()
    queue = [(target, 0)]  # (symbol, depth)
    slice_nodes = []
    
    while queue:
        symbol, depth = queue.pop(0)
        
        if symbol in visited or depth > config.max_depth:
            continue
            
        visited.add(symbol)
        slice_nodes.append(symbol)
        
        # Traverse dependencies
        for edge in graph.edges_from(symbol):
            if should_follow(edge, config):
                queue.append((edge.to, depth + 1))
                
        # Traverse dependents (callers)
        if config.include_callers:
            for edge in graph.edges_to(symbol):
                if depth < config.caller_depth:
                    queue.append((edge.from, depth + 1))
    
    return construct_slice(slice_nodes, graph)
```

---

## Appendix B: Example Use Cases

### B.1 Refactoring Assistant

**Scenario:** Extract helper function

```bash
$ graphslice refactor src/parser.rs:parse_expression \
  --extract "validate_tokens" \
  --lines 45-67
```

**GraphSlice workflow:**
1. Extract slice for `parse_expression`
2. Identify token validation code
3. Compute new slice for extracted function
4. Verify no new dependencies introduced
5. Generate refactoring patch

### B.2 Security Audit

**Scenario:** Find all code paths to sensitive function

```bash
$ graphslice audit src/crypto.rs:decrypt_key \
  --direction upstream \
  --max-depth 5
```

**Output:** All functions that transitively call `decrypt_key`

### B.3 Performance Optimization

**Scenario:** Optimize hot path

```bash
$ graphslice optimize src/server.rs:handle_request \
  --profile "perf.data" \
  --focus "allocations"
```

**GraphSlice workflow:**
1. Extract slice for `handle_request`
2. Include only functions in profiler hotpath
3. Send slice + profiling data to LLM
4. Generate optimization suggestions

---

**Repository:** github.com/kernex-sbs/graphslice  
**License:** MIT  
**Contact:** [utkarsh@kernex.sbs]
