// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Robson Trasel

/// Extended search query parser and logical query support.
///
/// Supports operators:
///   `=term`  — exact match
///   `^term`  — prefix match
///   `term$`  — suffix match
///   `!term`  — inverse (must NOT contain)
///   `!^term` — must NOT start with
///   `!term$` — must NOT end with
///   `term`   — fuzzy match (default)

#[derive(Debug, Clone, PartialEq)]
pub enum QueryOp {
    Fuzzy(String),
    Exact(String),
    Prefix(String),
    Suffix(String),
    Not(String),
    NotPrefix(String),
    NotSuffix(String),
}

/// Parse a query string into operators. Space-separated terms are ANDed.
pub fn parse(input: &str) -> Vec<QueryOp> {
    input.split_whitespace()
        .map(|token| parse_token(token))
        .collect()
}

fn parse_token(token: &str) -> QueryOp {
    if token.starts_with("!^") {
        QueryOp::NotPrefix(token[2..].to_string())
    } else if token.starts_with('!') && token.ends_with('$') && token.len() > 2 {
        QueryOp::NotSuffix(token[1..token.len() - 1].to_string())
    } else if token.starts_with('!') {
        QueryOp::Not(token[1..].to_string())
    } else if token.starts_with('=') {
        QueryOp::Exact(token[1..].to_string())
    } else if token.starts_with('^') {
        QueryOp::Prefix(token[1..].to_string())
    } else if token.ends_with('$') {
        QueryOp::Suffix(token[..token.len() - 1].to_string())
    } else {
        QueryOp::Fuzzy(token.to_string())
    }
}

/// Check if a term matches a single operator.
pub fn matches_op(term: &str, op: &QueryOp) -> bool {
    let t = term.to_lowercase();
    match op {
        QueryOp::Exact(v) => t == v.to_lowercase(),
        QueryOp::Prefix(v) => t.starts_with(&v.to_lowercase()),
        QueryOp::Suffix(v) => t.ends_with(&v.to_lowercase()),
        QueryOp::Not(v) => !t.contains(&v.to_lowercase()),
        QueryOp::NotPrefix(v) => !t.starts_with(&v.to_lowercase()),
        QueryOp::NotSuffix(v) => !t.ends_with(&v.to_lowercase()),
        QueryOp::Fuzzy(v) => t.contains(&v.to_lowercase()), // substring check for logical eval
    }
}

/// Logical query expression — AND/OR combinations of operators.
#[derive(Debug, Clone)]
pub enum LogicalExpr {
    And(Vec<LogicalExpr>),
    Or(Vec<LogicalExpr>),
    Op(QueryOp),
}

impl LogicalExpr {
    /// Evaluate the expression against a term.
    pub fn matches(&self, term: &str) -> bool {
        match self {
            LogicalExpr::Op(op) => matches_op(term, op),
            LogicalExpr::And(exprs) => exprs.iter().all(|e| e.matches(term)),
            LogicalExpr::Or(exprs) => exprs.iter().any(|e| e.matches(term)),
        }
    }
}

/// Parse a query with | (OR) and space (AND) operators into a logical expression.
/// "java | python" → Or([Fuzzy("java"), Fuzzy("python")])
/// "^java script$" → And([Prefix("java"), Suffix("script")])
/// "^java | python !ruby" → Or([Prefix("java"), And([Fuzzy("python"), Not("ruby")])])
pub fn parse_logical(input: &str) -> LogicalExpr {
    let or_parts: Vec<&str> = input.split('|').collect();
    if or_parts.len() == 1 {
        // no OR — parse as AND
        let ops = parse(input);
        if ops.len() == 1 {
            LogicalExpr::Op(ops.into_iter().next().unwrap())
        } else {
            LogicalExpr::And(ops.into_iter().map(LogicalExpr::Op).collect())
        }
    } else {
        LogicalExpr::Or(
            or_parts.iter()
                .map(|part| {
                    let ops = parse(part.trim());
                    if ops.len() == 1 {
                        LogicalExpr::Op(ops.into_iter().next().unwrap())
                    } else {
                        LogicalExpr::And(ops.into_iter().map(LogicalExpr::Op).collect())
                    }
                })
                .collect()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_fuzzy() {
        assert_eq!(parse("hello"), vec![QueryOp::Fuzzy("hello".into())]);
    }

    #[test]
    fn parse_exact() {
        assert_eq!(parse("=hello"), vec![QueryOp::Exact("hello".into())]);
    }

    #[test]
    fn parse_prefix() {
        assert_eq!(parse("^java"), vec![QueryOp::Prefix("java".into())]);
    }

    #[test]
    fn parse_suffix() {
        assert_eq!(parse("script$"), vec![QueryOp::Suffix("script".into())]);
    }

    #[test]
    fn parse_not() {
        assert_eq!(parse("!python"), vec![QueryOp::Not("python".into())]);
    }

    #[test]
    fn parse_not_prefix() {
        assert_eq!(parse("!^java"), vec![QueryOp::NotPrefix("java".into())]);
    }

    #[test]
    fn parse_not_suffix() {
        assert_eq!(parse("!script$"), vec![QueryOp::NotSuffix("script".into())]);
    }

    #[test]
    fn parse_multi() {
        let ops = parse("^java !python");
        assert_eq!(ops.len(), 2);
        assert_eq!(ops[0], QueryOp::Prefix("java".into()));
        assert_eq!(ops[1], QueryOp::Not("python".into()));
    }

    #[test]
    fn matches_exact() {
        assert!(matches_op("hello", &QueryOp::Exact("hello".into())));
        assert!(!matches_op("hello", &QueryOp::Exact("world".into())));
    }

    #[test]
    fn matches_prefix() {
        assert!(matches_op("javascript", &QueryOp::Prefix("java".into())));
        assert!(!matches_op("typescript", &QueryOp::Prefix("java".into())));
    }

    #[test]
    fn matches_suffix() {
        assert!(matches_op("javascript", &QueryOp::Suffix("script".into())));
        assert!(!matches_op("python", &QueryOp::Suffix("script".into())));
    }

    #[test]
    fn matches_not() {
        assert!(matches_op("rust", &QueryOp::Not("python".into())));
        assert!(!matches_op("python", &QueryOp::Not("python".into())));
    }

    #[test]
    fn logical_or() {
        let expr = parse_logical("java | python");
        assert!(expr.matches("javascript"));
        assert!(expr.matches("python"));
        assert!(!expr.matches("rust"));
    }

    #[test]
    fn logical_and() {
        let expr = parse_logical("^java script$");
        assert!(expr.matches("javascript"));
        assert!(!expr.matches("javafx"));
        assert!(!expr.matches("typescript"));
    }

    #[test]
    fn logical_or_with_and() {
        let expr = parse_logical("^java | ^type");
        assert!(expr.matches("javascript"));
        assert!(expr.matches("typescript"));
        assert!(!expr.matches("python"));
    }
}
