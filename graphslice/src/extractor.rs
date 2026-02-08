use anyhow::Result;
use tree_sitter::{Parser, Point, Node};
use tree_sitter_rust;

pub struct SymbolInfo {
    pub name: String,
    pub kind: String,
    pub code: String,
    pub line: usize,
}

pub struct Extractor {
    parser: Parser,
}

impl Extractor {
    pub fn new() -> Result<Self> {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_rust::LANGUAGE.into())
            .map_err(|e| anyhow::anyhow!("Failed to set language: {}", e))?;
        Ok(Self { parser })
    }

    /// Extract the full code block surrounding a given position.
    /// Walks up the AST to find relevant containers (function, struct, impl, etc.).
    pub fn extract_block(&mut self, source_code: &str, line: usize, column: usize) -> Option<String> {
        let tree = self.parser.parse(source_code, None)?;
        let root = tree.root_node();

        // tree-sitter uses 0-indexed lines and columns
        let target_point = Point::new(line, column);

        // Find the smallest named node containing the point
        let mut node = root.descendant_for_point_range(target_point, target_point)?;

        // Walk up to find a significant node
        while let Some(parent) = node.parent() {
            let kind = node.kind();

            // List of nodes we consider "blocks" worth extracting entirely
            if matches!(kind,
                "function_item" |
                "struct_item" |
                "enum_item" |
                "impl_item" |
                "trait_item" |
                "mod_item" |
                "macro_definition"
            ) {
                return Some(self.get_node_text(source_code, &node));
            }

            // If we hit the root without finding a specific item, maybe it's a top-level statement?
            // For now, keep bubbling up.
            node = parent;
        }

        // If we reached root, check if the node itself is interesting (e.g. if we started at a function item)
        // descendant_for_point_range might have returned the function_item directly if we pointed exactly at it?
        // Actually descendant usually returns leaf nodes (identifiers).
        // If we didn't find a parent, maybe we are at the top level.
        // Let's fallback to returning the line if we can't find a block, or maybe the node itself?
        // But the previous loop should catch it if we started inside.

        // As a fallback, if we are at the top level, return the node text if it spans multiple lines?
        // Or just return None to let caller handle (or maybe the whole file?)
        // For MVP, if we fail to find a block, let's try to return the statement.

        // Let's try to find statement level
        // Reset node
        if let Some(n) = root.descendant_for_point_range(target_point, target_point) {
             let mut curr = n;
             while let Some(parent) = curr.parent() {
                 if parent.kind() == "source_file" {
                     return Some(self.get_node_text(source_code, &curr));
                 }
                 curr = parent;
             }
        }

        None
    }

    /// Scan source code for top-level definitions
    pub fn get_defined_symbols(&mut self, source_code: &str) -> Vec<SymbolInfo> {
        let mut symbols = Vec::new();
        let tree = match self.parser.parse(source_code, None) {
            Some(t) => t,
            None => return symbols,
        };

        let root = tree.root_node();
        let mut cursor = root.walk();

        for child in root.children(&mut cursor) {
            let kind = child.kind();
            if matches!(kind,
                "function_item" |
                "struct_item" |
                "enum_item" |
                "trait_item" |
                "mod_item" |
                "macro_definition"
            ) {
                // Extract name
                // Usually the name is in a child node of type "identifier" or "name"
                // Or "type_identifier" for structs
                let name = child.child_by_field_name("name")
                    .map(|n| self.get_node_text(source_code, &n))
                    .unwrap_or_else(|| "unknown".to_string());

                symbols.push(SymbolInfo {
                    name,
                    kind: kind.to_string(),
                    code: self.get_node_text(source_code, &child),
                    line: child.start_position().row,
                });
            }
        }

        symbols
    }

    fn get_node_text(&self, source: &str, node: &Node) -> String {
        let start_byte = node.start_byte();
        let end_byte = node.end_byte();
        if start_byte < source.len() && end_byte <= source.len() {
            source[start_byte..end_byte].to_string()
        } else {
            String::new()
        }
    }

    /// Extract constraints for a specific location in the code
    /// Returns (assignments, conditions)
    /// assignments: variables known to have constant integer values before this point
    /// conditions: conditions that must be true to reach this point (from surrounding if statements)
    pub fn extract_constraints(&mut self, source_code: &str, line: usize, column: usize) -> (Vec<Constraint>, Vec<Constraint>) {
        let mut assignments = Vec::new();
        let mut conditions = Vec::new();

        let tree = match self.parser.parse(source_code, None) {
            Some(t) => t,
            None => return (assignments, conditions),
        };

        let root = tree.root_node();
        let target_point = Point::new(line, column);
        let target_node = match root.descendant_for_point_range(target_point, target_point) {
            Some(n) => n,
            None => return (assignments, conditions),
        };

        // 1. Find assignments in the same scope before the target
        // This is a naive heuristic: scan all `let x = int` in the function/block
        // ideally we should respect scope, but for MVP we just scan the whole root/block text?
        // Better: walk up scopes and scan siblings before the target.

        let mut curr = target_node;
        while let Some(parent) = curr.parent() {
            // If parent is a block, scan previous siblings
            if parent.kind() == "block" {
                let mut cursor = parent.walk();
                for child in parent.children(&mut cursor) {
                    if child.end_byte() <= curr.start_byte() {
                        // This child comes before our path
                        if child.kind() == "let_declaration"
                            && let Some(constraint) = self.parse_let_assignment(source_code, &child) {
                                assignments.push(constraint);
                            }
                    }
                }
            }

            // 2. Check if we are inside an IF block
            if parent.kind() == "if_expression" {
                // Check if we are in the consequence block
                if let Some(consequence) = parent.child_by_field_name("consequence") {
                    // Check if 'curr' is inside 'consequence'
                    // Note: 'curr' might be the block inside consequence, or deeper
                    if consequence.start_byte() <= curr.start_byte() && curr.end_byte() <= consequence.end_byte() {
                        // We are in the THEN block
                        if let Some(condition) = parent.child_by_field_name("condition")
                            && let Some(constraint) = self.parse_condition(source_code, &condition) {
                                conditions.push(constraint);
                            }
                    }
                }
            }

            curr = parent;
        }

        (assignments, conditions)
    }

    fn parse_let_assignment(&self, source: &str, node: &Node) -> Option<Constraint> {
        // let pattern = value;
        let pattern = node.child_by_field_name("pattern")?;
        let value = node.child_by_field_name("value")?;

        if pattern.kind() == "identifier" && value.kind() == "integer_literal" {
            let name = self.get_node_text(source, &pattern);
            let val_str = self.get_node_text(source, &value);
            if let Ok(val) = val_str.parse::<i64>() {
                return Some(Constraint {
                    var: name,
                    op: "==".to_string(),
                    val,
                });
            }
        }
        None
    }

    fn parse_condition(&self, source: &str, node: &Node) -> Option<Constraint> {
        // Simple binary expression: left op right
        // heuristic: strip parenthesis if present
        // tree-sitter often wraps condition in nothing special, but binary_expression is key

        // If condition is just a binary expression
        if node.kind() == "binary_expression" {
             return self.parse_binary_expression(source, node);
        }

        // Use recursive search for binary expression if it's wrapped?
        // e.g. `x < 5` inside `(x < 5)`?
        // For MVP, just direct binary expression check
        None
    }

    fn parse_binary_expression(&self, source: &str, node: &Node) -> Option<Constraint> {
        let left = node.child_by_field_name("left")?;
        let right = node.child_by_field_name("right")?;
        let op_node = node.child_by_field_name("operator")?;
        let op = self.get_node_text(source, &op_node);

        // Case 1: x < 10
        if left.kind() == "identifier" && right.kind() == "integer_literal" {
            let name = self.get_node_text(source, &left);
            let val = self.get_node_text(source, &right).parse::<i64>().ok()?;
            return Some(Constraint { var: name, op, val });
        }

        // Case 2: 10 > x  (flip to x < 10)
        if left.kind() == "integer_literal" && right.kind() == "identifier" {
            let name = self.get_node_text(source, &right);
            let val = self.get_node_text(source, &left).parse::<i64>().ok()?;
            let new_op = match op.as_str() {
                ">" => "<",
                "<" => ">",
                ">=" => "<=",
                "<=" => ">=",
                "==" => "==",
                "!=" => "!=",
                _ => return None,
            };
            return Some(Constraint { var: name, op: new_op.to_string(), val });
        }

        None
    }
}

#[derive(Debug, Clone)]
pub struct Constraint {
    pub var: String,
    pub op: String,
    pub val: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_constraint_extraction() {
        let code = r#"
            fn test() {
                let x = 10;
                let y = 20;
                if x > 5 {
                    // Target location inside here
                    let z = 30;
                }
            }
        "#;

        let mut extractor = Extractor::new().unwrap();

        // Line 6 is inside the if block: "let z = 30;"
        // 0-indexed: line 6
        // Column 20 (arbitrary inside the block)
        let (assignments, conditions) = extractor.extract_constraints(code, 6, 20);

        println!("Assignments: {:?}", assignments);
        println!("Conditions: {:?}", conditions);

        // Expect x=10, y=20
        assert!(assignments.iter().any(|c| c.var == "x" && c.val == 10));
        assert!(assignments.iter().any(|c| c.var == "y" && c.val == 20));

        // Expect x > 5
        assert!(conditions.iter().any(|c| c.var == "x" && c.op == ">" && c.val == 5));
    }
}
