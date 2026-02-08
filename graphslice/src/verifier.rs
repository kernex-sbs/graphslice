use anyhow::{Result, anyhow};
use z3::{Solver, SatResult};
use z3::ast::Int;

pub struct Verifier;

impl Verifier {
    pub fn new() -> Result<Self> {
        // z3 0.19.7 uses a thread-local context by default.
        // We don't need to manually create or store it.
        Ok(Self)
    }

    /// Prove that a condition is always false (Unsatisfiable)
    /// Used for dead code elimination.
    ///
    /// Example: if we know "x > 10", and we check "x < 5", it should return true (unreachable).
    pub fn is_unreachable(&self) -> bool {
        let solver = Solver::new();

        // Proof of concept:
        // Assert: x > 10
        // Check: x < 5

        let x = Int::new_const("x");
        let ten = Int::from_i64(10);
        let five = Int::from_i64(5);

        // Constraint: x > 10
        solver.assert(x.gt(&ten));

        // We want to check if (x < 5) is consistent with (x > 10).
        // If SAT, then it is reachable.
        // If UNSAT, then it is unreachable.

        let condition = x.lt(&five);

        // Push a new scope to check the condition without permanently adding it
        solver.push();
        solver.assert(&condition);
        let result = solver.check();
        solver.pop(1);

        result == SatResult::Unsat
    }

    /// Verify constraint satisfaction for a set of integer constraints
    /// constraints: List of (var_name, op, value) tuples, e.g. ("x", ">", 10)
    /// target: (var_name, op, value) to check reachability for
    pub fn verify_integer_reachability(
        &self,
        constraints: &[(&str, &str, i64)],
        target: (&str, &str, i64)
    ) -> Result<bool> {
        let solver = Solver::new();

        for (name, op, val) in constraints {
            let var = Int::new_const(*name);
            let val_ast = Int::from_i64(*val);

            let constraint = match *op {
                ">" => var.gt(&val_ast),
                "<" => var.lt(&val_ast),
                ">=" => var.ge(&val_ast),
                "<=" => var.le(&val_ast),
                "==" => var.eq(&val_ast),
                "!=" => var.eq(&val_ast).not(),
                _ => return Err(anyhow!("Unsupported operator: {}", op)),
            };

            solver.assert(&constraint);
        }

        // Check target
        let (name, op, val) = target;
        let var = Int::new_const(name);
        let val_ast = Int::from_i64(val);

        let target_constraint = match op {
            ">" => var.gt(&val_ast),
            "<" => var.lt(&val_ast),
            ">=" => var.ge(&val_ast),
            "<=" => var.le(&val_ast),
            "==" => var.eq(&val_ast),
            "!=" => var.eq(&val_ast).not(),
            _ => return Err(anyhow!("Unsupported operator: {}", op)),
        };

        solver.assert(&target_constraint);

        // If satisfiable, the path is reachable
        Ok(solver.check() == SatResult::Sat)
    }

    /// Check if a set of constraints is logically consistent (Satisfiable)
    pub fn check_consistency(&self, constraints: &[(&str, &str, i64)]) -> bool {
        let solver = Solver::new();

        for (name, op, val) in constraints {
            let var = Int::new_const(*name);
            let val_ast = Int::from_i64(*val);

            let constraint = match *op {
                ">" => var.gt(&val_ast),
                "<" => var.lt(&val_ast),
                ">=" => var.ge(&val_ast),
                "<=" => var.le(&val_ast),
                "==" => var.eq(&val_ast),
                "!=" => var.eq(&val_ast).not(),
                _ => continue, // Skip unsupported ops
            };

            solver.assert(&constraint);
        }

        solver.check() == SatResult::Sat
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_unreachability() {
        let verifier = Verifier::new().expect("Failed to create verifier");

        // x > 10 AND x < 5 should be unreachable (UNSAT)
        // verify_integer_reachability returns TRUE if reachable (SAT)
        // so we expect FALSE
        let constraints = vec![("x", ">", 10)];
        let target = ("x", "<", 5);

        let reachable = verifier.verify_integer_reachability(&constraints, target).unwrap();
        assert!(!reachable, "x < 5 should be unreachable given x > 10");
    }

    #[test]
    fn test_reachable() {
        let verifier = Verifier::new().expect("Failed to create verifier");

        // x > 10 AND x > 5 should be reachable (SAT)
        let constraints = vec![("x", ">", 10)];
        let target = ("x", ">", 5);

        let reachable = verifier.verify_integer_reachability(&constraints, target).unwrap();
        assert!(reachable, "x > 5 should be reachable given x > 10");
    }

    #[test]
    fn test_equality() {
        let verifier = Verifier::new().expect("Failed to create verifier");

        // x == 10 AND x != 10 should be unreachable
        let constraints = vec![("x", "==", 10)];
        let target = ("x", "!=", 10);

        let reachable = verifier.verify_integer_reachability(&constraints, target).unwrap();
        assert!(!reachable, "x != 10 should be unreachable given x == 10");
    }
}
