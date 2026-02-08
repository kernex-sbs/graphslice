use crate::graph::{CodeNode, DependencyGraph, Edge, EdgeType, NodeId};
use crate::lsp_client::LspClient;
use crate::extractor::Extractor;
use crate::fuzzy_slicer::FuzzySlicer;
use crate::verifier::Verifier;
use anyhow::{Result, anyhow};
use std::fs;
use std::path::PathBuf;
use url::Url;
use lsp_types::DiagnosticSeverity;

pub struct Slicer {
    lsp: LspClient,
    extractor: Extractor,
    fuzzy: FuzzySlicer,
    verifier: Verifier,
    _workspace_root: PathBuf,
}

impl Slicer {
    pub async fn new(workspace_root: PathBuf) -> Result<Self> {
        let lsp = LspClient::new(workspace_root.clone()).await?;
        let extractor = Extractor::new()?;
        let fuzzy = FuzzySlicer::new()?;
        let verifier = Verifier::new()?;
        Ok(Self {
            lsp,
            extractor,
            fuzzy,
            verifier,
            _workspace_root: workspace_root,
        })
    }

    /// Check if a location is reachable based on static constraints
    fn is_reachable(&mut self, file: &PathBuf, line: u32, col: u32) -> bool {
        // Read file content (inefficient to re-read, but simple for MVP)
        // In production we should cache this
        let content = match fs::read_to_string(file) {
            Ok(c) => c,
            Err(_) => return true, // Assume reachable if we can't read
        };

        let (assignments, conditions) = self.extractor.extract_constraints(&content, line as usize, col as usize);

        if assignments.is_empty() && conditions.is_empty() {
            return true;
        }

        // Convert to verifier format
        let mut constraints = Vec::new();
        for c in &assignments {
            constraints.push((c.var.as_str(), c.op.as_str(), c.val));
        }
        for c in &conditions {
            constraints.push((c.var.as_str(), c.op.as_str(), c.val));
        }

        let consistent = self.verifier.check_consistency(&constraints);
        if !consistent {
            eprintln!("✂️ Pruned unreachable code at {}:{}:{} (Constraints: {:?} + {:?})",
                file.display(), line, col, assignments, conditions);
        }
        consistent
    }

    /// Build dependency graph from a target location
    pub async fn build_graph(
        &mut self,
        target_file: PathBuf,
        target_line: u32,
        target_col: u32,
    ) -> Result<DependencyGraph> {
        // Notify LSP that we opened the file (to ensure we get diagnostics)
        if let Ok(full_text) = fs::read_to_string(&target_file) {
            let _ = self.lsp.did_open(&target_file, full_text).await;
        }

        // Give LSP a moment to process diagnostics
        tokio::time::sleep(std::time::Duration::from_millis(2000)).await;

        // Check diagnostics to decide on slicing strategy
        let diagnostics = self.lsp.get_diagnostics(&target_file).unwrap_or_default();
        let error_count = diagnostics
            .iter()
            .filter(|d| d.severity == Some(DiagnosticSeverity::ERROR))
            .count();

        if error_count > 0 {
            eprintln!("⚠️  File has {} errors. Switching to Fuzzy (LLM) Slicer.", error_count);
            return self.fuzzy.slice(target_file, target_line, target_col).await;
        }

        eprintln!("✅ File is healthy. Using Strict LSP Slicer.");

        // Strict LSP Slicer Logic
        let mut graph = DependencyGraph::new();

        let target_id = NodeId {
            file: target_file.clone(),
            line: target_line,
            column: target_col,
        };

        // Add target node
        let code = self.read_location(&target_file, target_line)?;

        graph.add_node(CodeNode {
            id: target_id.clone(),
            code,
            node_type: "target".to_string(),
        });

        // Get all references to this location
        let refs = self
            .lsp
            .get_references(&target_file, target_line, target_col)
            .await?;

        for location in refs {
            let uri_str = location.uri.as_str();
            let url = Url::parse(uri_str).map_err(|e| anyhow!("Failed to parse URI: {}", e))?;
            let ref_path = url.to_file_path().map_err(|_| anyhow!("URI is not a file path: {}", uri_str))?;

            let ref_line = location.range.start.line;
            let ref_col = location.range.start.character;

            let ref_id = NodeId {
                file: ref_path.clone(),
                line: ref_line,
                column: ref_col,
            };

            // Add reference node
            let ref_code = self.read_location(&ref_path, ref_line)?;
            graph.add_node(CodeNode {
                id: ref_id.clone(),
                code: ref_code,
                node_type: "reference".to_string(),
            });

            // Add edge: reference -> target
            graph.add_edge(Edge {
                from: ref_id,
                to: target_id.clone(),
                edge_type: EdgeType::References,
            });
        }

        // Get definition
        let defs = self
            .lsp
            .get_definition(&target_file, target_line, target_col)
            .await?;

        for location in defs {
            let uri_str = location.uri.as_str();
            let url = Url::parse(uri_str).map_err(|e| anyhow!("Failed to parse URI: {}", e))?;
            let def_path = url.to_file_path().map_err(|_| anyhow!("URI is not a file path: {}", uri_str))?;

            let def_line = location.range.start.line;
            let def_col = location.range.start.character;

            let def_id = NodeId {
                file: def_path.clone(),
                line: def_line,
                column: def_col,
            };

            // Add definition node
            let def_code = self.read_implementation(&def_path, def_line)?;
            graph.add_node(CodeNode {
                id: def_id.clone(),
                code: def_code,
                node_type: "definition".to_string(),
            });

            // Add edge: target -> definition
            graph.add_edge(Edge {
                from: target_id.clone(),
                to: def_id.clone(),
                edge_type: EdgeType::Defines,
            });

            // Expand outgoing calls from definition
            let hierarchy_items = self.lsp.prepare_call_hierarchy(&def_path, def_line, def_col).await?;
            for item in hierarchy_items {
                let outgoing = self.lsp.get_outgoing_calls(item).await?;
                for call in outgoing {
                    let call_item = call.to;
                    let uri_str = call_item.uri.as_str();
                    // Skip if uri parsing fails or not a file
                    if let Ok(url) = Url::parse(uri_str)
                        && let Ok(call_path) = url.to_file_path() {
                            let call_line = call_item.range.start.line;
                            let call_col = call_item.range.start.character;

                            let call_id = NodeId {
                                file: call_path.clone(),
                                line: call_line,
                                column: call_col,
                            };

                            // Avoid cycles or duplicates if already added
                            if !graph.nodes.contains_key(&call_id) {
                                // Phase 3: Prune unreachable calls
                                // Check all call sites in the caller function
                                let mut any_site_reachable = false;
                                for range in &call.from_ranges {
                                    if self.is_reachable(&def_path, range.start.line, range.start.character) {
                                        any_site_reachable = true;
                                        break;
                                    }
                                }

                                if !any_site_reachable {
                                    eprintln!("✂️ Pruned call to {} (all sites unreachable)", call_item.name);
                                    continue;
                                }

                                let call_code = self.read_implementation(&call_path, call_line)?;
                                graph.add_node(CodeNode {
                                    id: call_id.clone(),
                                    code: call_code,
                                    node_type: "call".to_string(),
                                });
                            }

                            graph.add_edge(Edge {
                                from: def_id.clone(),
                                to: call_id,
                                edge_type: EdgeType::Calls,
                            });
                        }
                }
            }
        }

        Ok(graph)
    }

    /// Read a single line from file
    fn read_location(&self, file: &PathBuf, line: u32) -> Result<String> {
        let content = fs::read_to_string(file)?;
        let lines: Vec<&str> = content.lines().collect();

        if (line as usize) < lines.len() {
            Ok(lines[line as usize].to_string())
        } else {
            Ok(String::new())
        }
    }

    /// Read implementation block using Tree-sitter
    fn read_implementation(&mut self, file: &PathBuf, start_line: u32) -> Result<String> {
        let content = fs::read_to_string(file)?;

        // Try to extract the block using tree-sitter
        if let Some(block) = self.extractor.extract_block(&content, start_line as usize, 0) {
            return Ok(block);
        }

        // Fallback: read single line if extraction fails
        // This can happen for non-block items or if the position is not inside a supported node
        let lines: Vec<&str> = content.lines().collect();
        if (start_line as usize) < lines.len() {
            Ok(lines[start_line as usize].to_string())
        } else {
            Ok(String::new())
        }
    }

    /// Extract minimal context from graph
    pub fn extract_context(&self, graph: &DependencyGraph, max_depth: usize) -> String {
        let mut context = String::new();

        // Safety check for empty graph
        let root = match graph.nodes.keys().next() {
            Some(id) => id,
            None => return String::from("// No context found (graph is empty)"),
        };

        for (node_id, depth) in graph.bfs_from(root) {
            if depth > max_depth {
                break;
            }

            if let Some(node) = graph.nodes.get(&node_id) {
                context.push_str(&format!(
                    "// {}:{}:{} (depth {})\n{}\n\n",
                    node_id.file.display(),
                    node_id.line,
                    node_id.column,
                    depth,
                    node.code
                ));
            }
        }

        context
    }
}