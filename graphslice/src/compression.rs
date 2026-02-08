use crate::graph::{DependencyGraph, NodeId};
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub enum InclusionLevel {
    FullSource,
    InterfaceSummary,
    Reference,
}

pub struct HierarchicalContext {
    pub sections: HashMap<NodeId, (String, InclusionLevel)>,
}

impl Default for HierarchicalContext {
    fn default() -> Self {
        Self::new()
    }
}

impl HierarchicalContext {
    pub fn new() -> Self {
        Self {
            sections: HashMap::new(),
        }
    }

    /// Build hierarchical context with token budget
    pub fn build(
        graph: &DependencyGraph,
        root: &NodeId,
        max_tokens: usize,
    ) -> Self {
        let mut context = Self::new();
        let mut current_tokens = 0;

        for (node_id, depth) in graph.bfs_from(root) {
            if current_tokens >= max_tokens {
                break;
            }

            let node = graph.nodes.get(&node_id).unwrap();

            let (content, level) = match depth {
                0 => {
                    // Target: always full source
                    (node.code.clone(), InclusionLevel::FullSource)
                }
                1 => {
                    // Direct dependencies: full source if budget allows
                    let tokens = estimate_tokens(&node.code);
                    if current_tokens + tokens <= max_tokens {
                        current_tokens += tokens;
                        (node.code.clone(), InclusionLevel::FullSource)
                    } else {
                        // Compress to interface
                        let summary = extract_interface(&node.code);
                        current_tokens += estimate_tokens(&summary);
                        (summary, InclusionLevel::InterfaceSummary)
                    }
                }
                2.. => {
                    // Transitive: interface summary only
                    let summary = extract_interface(&node.code);
                    let tokens = estimate_tokens(&summary);
                    
                    if current_tokens + tokens <= max_tokens {
                        current_tokens += tokens;
                        (summary, InclusionLevel::InterfaceSummary)
                    } else {
                        // Just reference
                        let reference = format!(
                            "// See: {}:{}",
                            node_id.file.display(),
                            node_id.line
                        );
                        (reference, InclusionLevel::Reference)
                    }
                }
            };

            context.sections.insert(node_id, (content, level));
        }

        context
    }

    /// Render to string
    pub fn render(&self) -> String {
        let mut output = String::new();

        for (node_id, (content, level)) in &self.sections {
            let marker = match level {
                InclusionLevel::FullSource => "FULL",
                InclusionLevel::InterfaceSummary => "INTERFACE",
                InclusionLevel::Reference => "REF",
            };

            output.push_str(&format!(
                "\n// [{}] {}:{}:{}\n",
                marker, node_id.file.display(), node_id.line, node_id.column
            ));
            output.push_str(content);
            output.push('\n');
        }

        output
    }
}

/// Extract function signature from implementation
fn extract_interface(code: &str) -> String {
    // Simple heuristic: keep lines with fn/struct/impl/pub
    let lines: Vec<&str> = code
        .lines()
        .filter(|line| {
            let trimmed = line.trim();
            trimmed.starts_with("pub fn")
                || trimmed.starts_with("fn")
                || trimmed.starts_with("pub struct")
                || trimmed.starts_with("struct")
                || trimmed.starts_with("impl")
                || trimmed.contains("///")
        })
        .collect();

    if lines.is_empty() {
        // Fallback: first line
        code.lines().next().unwrap_or("").to_string()
    } else {
        lines.join("\n")
    }
}

/// Estimate tokens (rough: 1 token ≈ 4 chars)
fn estimate_tokens(text: &str) -> usize {
    text.len() / 4
}