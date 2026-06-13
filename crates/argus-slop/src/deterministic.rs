//! Deterministic AST-based pre-flight analyzer. [Refs: 5.1]
//!
//! Hybrid architecture: cheap regex rules BEFORE the LLM analyzers
//! to catch mechanical slop in < 100ms with zero API calls.
//!
//! Public entry point: [`run_deterministic_rules`].

/// SLOP-001 threshold. Exposed for tests and downstream config.
pub const OVERSIZED_FN_LOC: usize = 80;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Severity {
    Info,
    Warning,
    Error,
}

impl Severity {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Warning => "warning",
            Self::Error => "error",
        }
    }
}

#[derive(Debug, Clone)]
pub struct SlopSignal {
    pub rule_id: String,
    pub severity: Severity,
    pub line: usize,
    pub message: String,
}

impl SlopSignal {
    pub fn info(rule_id: &str, line: usize, msg: impl Into<String>) -> Self {
        Self {
            rule_id: rule_id.to_string(),
            severity: Severity::Info,
            line,
            message: msg.into(),
        }
    }
    pub fn warning(rule_id: &str, line: usize, msg: impl Into<String>) -> Self {
        Self {
            rule_id: rule_id.to_string(),
            severity: Severity::Warning,
            line,
            message: msg.into(),
        }
    }
    pub fn error(rule_id: &str, line: usize, msg: impl Into<String>) -> Self {
        Self {
            rule_id: rule_id.to_string(),
            severity: Severity::Error,
            line,
            message: msg.into(),
        }
    }
}

/// Run all 5 rules. Pure regex — never panics, even on non-Rust input.
pub fn run_deterministic_rules(src: &str) -> Vec<SlopSignal> {
    let mut signals = Vec::new();
    slop_001_oversized(src, &mut signals);
    slop_002_swallowed(src, &mut signals);
    slop_003_todo(src, &mut signals);
    slop_004_unwrap(src, &mut signals);
    slop_005_unused_pub(src, &mut signals);
    signals.sort_by_key(|s| s.line);
    signals
}

// SLOP-001: function body > 80 LOC
// Heuristic: count consecutive non-empty lines inside `{` ... `}` pairs.
fn slop_001_oversized(src: &str, signals: &mut Vec<SlopSignal>) {
    let mut depth: i32 = 0;
    let mut body_start: Option<usize> = None;
    let mut body_lines: usize = 0;
    let mut fn_name: Option<String> = None;
    let mut fn_start_line: usize = 0;

    for (i, line) in src.lines().enumerate() {
        let line_num = i + 1;
        let trimmed = line.trim();

        // Track fn declarations at depth 0
        if depth == 0 {
            if let Some(name) = trimmed
                .strip_prefix("fn ")
                .and_then(|s| s.split('(').next())
            {
                fn_name = Some(name.trim().to_string());
                fn_start_line = line_num;
                body_start = Some(line_num);
                body_lines = 0;
            }
            if let Some(name) = trimmed
                .strip_prefix("pub fn ")
                .and_then(|s| s.split('(').next())
            {
                fn_name = Some(name.trim().to_string());
                fn_start_line = line_num;
                body_start = Some(line_num);
                body_lines = 0;
            }
        }

        for ch in line.chars() {
            match ch {
                '{' => {
                    if depth == 0 && body_start.is_none() {
                        body_start = Some(line_num);
                        body_lines = 0;
                    }
                    depth += 1;
                }
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        if let (Some(_start), Some(name)) = (body_start, fn_name.take()) {
                            if body_lines > OVERSIZED_FN_LOC {
                                signals.push(SlopSignal::warning(
                                    "SLOP-001",
                                    fn_start_line,
                                    format!(
                                        "Function '{}' has {} LOC (> {})",
                                        name, body_lines, OVERSIZED_FN_LOC
                                    ),
                                ));
                            }
                        }
                        body_start = None;
                    }
                }
                _ => {}
            }
        }
        if depth > 0 && !trimmed.is_empty() && !trimmed.starts_with("//") {
            body_lines += 1;
        }
    }
}

// SLOP-002: `Err(_) => {}` or `Err(_) => ();` — swallowed error arm
fn slop_002_swallowed(src: &str, signals: &mut Vec<SlopSignal>) {
    for (i, line) in src.lines().enumerate() {
        let trimmed = line.trim();
        // Match: Err(...) => {} | Err(...) => (); | Err(...) => ;
        if !trimmed.contains("Err") {
            continue;
        }
        if trimmed.contains("=> {}")
            || trimmed.contains("=>()")
            || trimmed.contains("=> {},")
            || trimmed.contains("=> {};")
            || trimmed.ends_with("=> ;")
            || trimmed.ends_with("=> {}")
        {
            signals.push(SlopSignal::error(
                "SLOP-002",
                i + 1,
                "Error arm discards error silently",
            ));
        }
    }
}

// SLOP-003: `// TODO` stub in non-test code
fn slop_003_todo(src: &str, signals: &mut Vec<SlopSignal>) {
    for (i, line) in src.lines().enumerate() {
        if line.trim_start().starts_with("// TODO") {
            signals.push(SlopSignal::info(
                "SLOP-003",
                i + 1,
                "TODO stub in non-test code",
            ));
        }
    }
}

// SLOP-004: `.unwrap()` or `.expect(...)` outside test functions
fn slop_004_unwrap(src: &str, signals: &mut Vec<SlopSignal>) {
    let mut in_test = false;
    for (i, line) in src.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with("#[cfg(test)]") || trimmed.starts_with("#[test]") {
            in_test = true;
            continue;
        }
        // Update in_test state if this line declares a fn
        if trimmed.starts_with("fn ") || trimmed.starts_with("pub fn ") {
            let name = trimmed
                .strip_prefix("pub fn ")
                .or_else(|| trimmed.strip_prefix("fn "))
                .unwrap_or("");
            in_test = name.starts_with("test_");
        }
        // Check for unwrap on this line BEFORE continuing
        if !in_test && (line.contains(".unwrap()") || line.contains(".expect(")) {
            signals.push(SlopSignal::warning(
                "SLOP-004",
                i + 1,
                ".unwrap() / .expect() in non-test code",
            ));
        }
    }
}

// SLOP-005: `pub fn` with no callers in the same file
// Heuristic: count occurrences of the fn name (excluding the declaration).
fn slop_005_unused_pub(src: &str, signals: &mut Vec<SlopSignal>) {
    let mut pub_fns: Vec<(String, usize)> = Vec::new();
    for (i, line) in src.lines().enumerate() {
        let trimmed = line.trim();
        let name = trimmed
            .strip_prefix("pub fn ")
            .and_then(|s| s.split('(').next())
            .map(|s| s.trim().to_string());
        if let Some(n) = name {
            if !n.is_empty() {
                pub_fns.push((n, i + 1));
            }
        }
    }
    for (name, decl_line) in &pub_fns {
        let mut count = 0;
        for (i, line) in src.lines().enumerate() {
            if i + 1 == *decl_line {
                continue;
            }
            if line.contains(name) {
                count += 1;
            }
        }
        if count == 0 {
            signals.push(SlopSignal::info(
                "SLOP-005",
                *decl_line,
                format!("Public function '{}' has no callers in this file", name),
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn severity_as_str() {
        assert_eq!(Severity::Info.as_str(), "info");
        assert_eq!(Severity::Warning.as_str(), "warning");
        assert_eq!(Severity::Error.as_str(), "error");
    }

    #[test]
    fn signal_constructors() {
        assert_eq!(SlopSignal::info("X", 1, "m").severity, Severity::Info);
        assert_eq!(SlopSignal::warning("X", 1, "m").severity, Severity::Warning);
        assert_eq!(SlopSignal::error("X", 1, "m").severity, Severity::Error);
    }

    #[test]
    fn oversized_function() {
        let body = (0..100)
            .map(|i| format!("    let _x{i} = {i};"))
            .collect::<Vec<_>>()
            .join("\n");
        let src = format!("fn big() {{\n{body}\n}}\n");
        let signals = run_deterministic_rules(&src);
        assert!(
            signals.iter().any(|s| s.rule_id == "SLOP-001"),
            "got: {:?}",
            signals
        );
    }

    #[test]
    fn swallowed_error() {
        let src = "fn f(r: Result<u8, ()>) {\n    match r {\n        Err(_) => {},\n        Ok(_) => (),\n    }\n}\n";
        let signals = run_deterministic_rules(src);
        assert!(
            signals.iter().any(|s| s.rule_id == "SLOP-002"),
            "got: {:?}",
            signals
        );
    }

    #[test]
    fn non_rust_no_panic() {
        let signals = run_deterministic_rules("def f():\n    print('hi')");
        assert!(signals.is_empty());
    }

    #[test]
    fn unwrap_in_test_ok_outside_warns() {
        let ok = run_deterministic_rules("#[test]\nfn test_x() { let _ = Some(1).unwrap(); }\n");
        let bad = run_deterministic_rules("fn prod() { let _ = Some(1).unwrap(); }\n");
        assert!(
            !ok.iter().any(|s| s.rule_id == "SLOP-004"),
            "test should not warn: {:?}",
            ok
        );
        assert!(
            bad.iter().any(|s| s.rule_id == "SLOP-004"),
            "prod should warn: {:?}",
            bad
        );
    }

    #[test]
    fn todo_stub() {
        let src = "fn f() {\n    // TODO: implement\n    let _ = 1;\n}\n";
        let signals = run_deterministic_rules(src);
        assert!(
            signals.iter().any(|s| s.rule_id == "SLOP-003"),
            "got: {:?}",
            signals
        );
    }

    #[test]
    fn performance_10k_loc_under_500ms() {
        let body = (0..10_000)
            .map(|i| format!("    let _x{i} = {i};"))
            .collect::<Vec<_>>()
            .join("\n");
        let src = format!("fn big() {{\n{body}\n}}\n");
        let start = std::time::Instant::now();
        let _ = run_deterministic_rules(&src);
        let elapsed = start.elapsed();
        assert!(elapsed.as_millis() < 500, "took {}ms", elapsed.as_millis());
    }
}
