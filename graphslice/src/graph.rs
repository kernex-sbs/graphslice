use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;

#[derive(Debug, Clone, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub struct NodeId {
    pub file: PathBuf,
    pub line: u32,
    pub column: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum EdgeType {
    Defines,     // A defines B
    Calls,       // A calls B
    Reads,       // A reads B
    Writes,      // A writes to B
    References,  // Generic reference
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeNode {
    pub id: NodeId,
    pub code: String,
    pub node_type: String, // "function", "struct", "variable", etc.
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Edge {
    pub from: NodeId,
    pub to: NodeId,
    pub edge_type: EdgeType,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct DependencyGraph {
    pub nodes: HashMap<NodeId, CodeNode>,
    pub edges: Vec<Edge>,
}

impl DependencyGraph {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_node(&mut self, node: CodeNode) {
        self.nodes.insert(node.id.clone(), node);
    }

    pub fn add_edge(&mut self, edge: Edge) {
        self.edges.push(edge);
    }

    /// Get all nodes reachable from root via BFS
    /// Returns (node, distance) pairs
    pub fn bfs_from(&self, root: &NodeId) -> Vec<(NodeId, usize)> {
        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();
        let mut result = Vec::new();

        queue.push_back((root.clone(), 0));
        visited.insert(root.clone());

        while let Some((node_id, distance)) = queue.pop_front() {
            result.push((node_id.clone(), distance));

            // Find all edges from this node
            for edge in &self.edges {
                if edge.from == node_id && !visited.contains(&edge.to) {
                    visited.insert(edge.to.clone());
                    queue.push_back((edge.to.clone(), distance + 1));
                }
            }
        }

        result
    }

    /// Get direct dependencies of a node
    pub fn get_dependencies(&self, node: &NodeId) -> Vec<&CodeNode> {
        self.edges
            .iter()
            .filter(|e| &e.from == node)
            .filter_map(|e| self.nodes.get(&e.to))
            .collect()
    }
}