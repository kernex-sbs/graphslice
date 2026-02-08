use crate::graph::{CodeNode, DependencyGraph, Edge, EdgeType, NodeId};
use crate::extractor::{Extractor, SymbolInfo};
use crate::llm_client::LlmClient;
use anyhow::{Result, anyhow};
use std::path::{Path, PathBuf};
use std::fs;
use std::collections::HashMap;
use walkdir::WalkDir;
use serde::Deserialize;

#[derive(Deserialize, Debug)]
struct LlmAnalysis {
    calls: Vec<String>,
    #[serde(default)]
    types: Vec<String>,
}

pub struct LocatedSymbol {
    pub info: SymbolInfo,
    pub file: PathBuf,
}

pub struct FuzzySlicer {
    extractor: Extractor,
    llm: LlmClient,
    symbol_cache: HashMap<String, Vec<LocatedSymbol>>, // Name -> [Locations]
    workspace_scanned: bool,
}

impl FuzzySlicer {
    pub fn new() -> Result<Self> {
        Ok(Self {
            extractor: Extractor::new()?,
            llm: LlmClient::new()?,
            symbol_cache: HashMap::new(),
            workspace_scanned: false,
        })
    }

    pub async fn slice(
        &mut self,
        target_file: PathBuf,
        target_line: u32,
        target_col: u32,
    ) -> Result<DependencyGraph> {
        let mut graph = DependencyGraph::new();

        // 1. Read and extract target
        let content = fs::read_to_string(&target_file)?;
        let target_code = if let Some(code) = self.extractor.extract_block(&content, target_line as usize, 0) {
            code
        } else {
            // Fallback to line if block extraction fails
             let lines: Vec<&str> = content.lines().collect();
             if (target_line as usize) < lines.len() {
                 lines[target_line as usize].to_string()
             } else {
                 return Err(anyhow!("Failed to extract target block at line {}", target_line));
             }
        };

        let target_id = NodeId {
            file: target_file.clone(),
            line: target_line,
            column: target_col,
        };

        graph.add_node(CodeNode {
            id: target_id.clone(),
            code: target_code.clone(),
            node_type: "target".to_string(),
        });

        // 2. Scan workspace if needed
        if !self.workspace_scanned {
            let root = self.find_workspace_root(&target_file).unwrap_or_else(|| PathBuf::from("."));
            eprintln!("FuzzySlicer: Scanning workspace at {}", root.display());
            self.scan_workspace(&root)?;
            self.workspace_scanned = true;
        }

        // 3. Ask LLM for dependencies
        let analysis = self.analyze_dependencies(&target_code).await?;
        eprintln!("FuzzySlicer: LLM identified dependencies: {:?}", analysis);

        // 4. Resolve dependencies
        for call_name in analysis.calls {
            self.add_dependency(&mut graph, &target_id, &call_name, EdgeType::Calls)?;
        }

        for type_name in analysis.types {
            self.add_dependency(&mut graph, &target_id, &type_name, EdgeType::References)?;
        }

        Ok(graph)
    }

    fn find_workspace_root(&self, start: &Path) -> Option<PathBuf> {
        let mut current = start.to_path_buf();
        if current.is_file() {
            current.pop();
        }

        loop {
            if current.join("Cargo.toml").exists() {
                return Some(current);
            }
            if !current.pop() {
                break;
            }
        }
        None
    }

    fn scan_workspace(&mut self, root: &Path) -> Result<()> {
        for entry in WalkDir::new(root).into_iter().filter_map(|e| e.ok()) {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("rs")
                && let Ok(content) = fs::read_to_string(path) {
                    let symbols = self.extractor.get_defined_symbols(&content);
                    for sym in symbols {
                        let located = LocatedSymbol {
                            info: sym,
                            file: path.to_path_buf(),
                        };

                        self.symbol_cache.entry(located.info.name.clone())
                            .or_default()
                            .push(located);
                    }
                }
        }
        Ok(())
    }

    async fn analyze_dependencies(&self, code: &str) -> Result<LlmAnalysis> {
        let prompt = format!(
            "Analyze the following Rust code and identify external function calls and type references that are crucial for understanding this code's behavior. \
            Ignore standard library calls (std::*). Return a JSON object with 'calls' (list of function names) and 'types' (list of struct/enum names).\n\n\
            Code:\n```rust\n{}\n```\n\nJSON:",
            code
        );

        let response = self.llm.completion(&prompt).await?;

        // Clean up response if it contains markdown blocks
        let json_str = response.trim();
        let json_str = if json_str.starts_with("```json") {
             json_str.strip_prefix("```json").unwrap_or(json_str)
                .strip_suffix("```").unwrap_or(json_str)
                .trim()
        } else if json_str.starts_with("```") {
             json_str.strip_prefix("```").unwrap_or(json_str)
                .strip_suffix("```").unwrap_or(json_str)
                .trim()
        } else {
            json_str
        };

        let analysis: LlmAnalysis = serde_json::from_str(json_str)
            .map_err(|e| anyhow!("Failed to parse LLM response: {}. Response: {}", e, response))?;

        Ok(analysis)
    }

    fn add_dependency(
        &mut self,
        graph: &mut DependencyGraph,
        target_id: &NodeId,
        name: &str,
        edge_type: EdgeType
    ) -> Result<()> {
        // Look up name in cache
        if let Some(definitions) = self.symbol_cache.get(name) {
            // Heuristic: take the first match. Ideally we'd disambiguate based on imports/context.
            // But this is "Fuzzy" slicing.
            if let Some(def) = definitions.first() {
                let def_id = NodeId {
                    file: def.file.clone(),
                    line: def.info.line as u32,
                    column: 0, // We don't have column in SymbolInfo yet, default to 0
                };

                // Add node if not exists
                if !graph.nodes.contains_key(&def_id) {
                    graph.add_node(CodeNode {
                        id: def_id.clone(),
                        code: def.info.code.clone(),
                        node_type: def.info.kind.clone(),
                    });
                }

                // Add edge
                let edge = match edge_type {
                    EdgeType::Calls => Edge {
                        from: target_id.clone(),
                        to: def_id,
                        edge_type: EdgeType::Calls,
                    },
                    _ => Edge {
                        from: target_id.clone(),
                        to: def_id,
                        edge_type: EdgeType::Defines, // Or References
                    }
                };

                graph.add_edge(edge);
            }
        }

        Ok(())
    }
}
