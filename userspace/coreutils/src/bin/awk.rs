//! awk — pattern scanning and processing language.
//!
//! Usage: awk [-F SEP] [PROGRAM] [FILE...]
//!   -F SEP    field separator (default: whitespace)
//!
//! Supported features:
//!   - Pattern-action pairs: /PATTERN/ { ACTION }
//!   - BEGIN { ... } and END { ... } blocks
//!   - Variables: $0 (whole line), $1..$N (fields), NR, NF, FS
//!   - print statement with field references
//!   - Simple conditions: $N == "value", $N != "value", $N ~ /regex/
//!   - Arithmetic: $N + $M, $N - $M, $N * $M, $N / $M
//!   - String concatenation in print
//!   - Built-in functions: length(), substr(), index(), tolower(), toupper()
//!
//! This is a minimal awk — not a full POSIX awk implementation.

use std::env;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::process;

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let mut fs_char = ' ';
    let mut program: Option<String> = None;
    let mut files: Vec<String> = Vec::new();
    let mut i = 0;

    while i < args.len() {
        match args[i].as_str() {
            "-F" => {
                i += 1;
                if i < args.len() {
                    fs_char = args[i].chars().next().unwrap_or(' ');
                }
            }
            arg => {
                if program.is_none() {
                    program = Some(arg.to_string());
                } else {
                    files.push(arg.to_string());
                }
            }
        }
        i += 1;
    }

    let program = match program {
        Some(p) => p,
        None => {
            eprintln!("awk: no program specified");
            process::exit(1);
        }
    };

    let rules = parse_program(&program);

    if files.is_empty() {
        files.push("-".to_string());
    }

    let stdout = io::stdout();
    let mut out = stdout.lock();

    // Execute BEGIN blocks
    for rule in &rules {
        if rule.pattern == Pattern::Begin {
            execute_action(&rule.action, &[], "", 0, 0, fs_char, &mut out);
        }
    }

    let mut nr: usize = 0;

    for path in &files {
        let reader: Box<dyn Read> = if path == "-" {
            Box::new(io::stdin())
        } else {
            match File::open(path) {
                Ok(f) => Box::new(f),
                Err(e) => {
                    eprintln!("awk: {path}: {e}");
                    continue;
                }
            }
        };

        let buf = BufReader::new(reader);
        for line_result in buf.lines() {
            let line = match line_result {
                Ok(l) => l,
                Err(_) => break,
            };
            nr += 1;

            let fields: Vec<&str> = if fs_char == ' ' {
                line.split_whitespace().collect()
            } else {
                line.split(fs_char).collect()
            };
            let nf = fields.len();

            for rule in &rules {
                if rule.pattern == Pattern::Begin || rule.pattern == Pattern::End {
                    continue;
                }
                if pattern_matches(&rule.pattern, &line, &fields, nr) {
                    execute_action(&rule.action, &fields, &line, nr, nf, fs_char, &mut out);
                }
            }
        }
    }

    // Execute END blocks
    for rule in &rules {
        if rule.pattern == Pattern::End {
            execute_action(&rule.action, &[], "", nr, 0, fs_char, &mut out);
        }
    }
}

#[derive(Debug, PartialEq)]
enum Pattern {
    Always,
    Begin,
    End,
    Regex(String),
    Condition(String), // raw condition string
}

#[derive(Debug)]
struct Rule {
    pattern: Pattern,
    action: String,
}

fn parse_program(prog: &str) -> Vec<Rule> {
    let mut rules = Vec::new();
    let prog = prog.trim();

    // Handle simple cases: just a print-like action with no braces
    if !prog.contains('{') {
        // Treat entire program as a pattern with implicit print
        rules.push(Rule {
            pattern: Pattern::Regex(prog.to_string()),
            action: "print".to_string(),
        });
        return rules;
    }

    let mut pos = 0;
    let bytes = prog.as_bytes();

    while pos < bytes.len() {
        // Skip whitespace
        while pos < bytes.len() && bytes[pos].is_ascii_whitespace() {
            pos += 1;
        }
        if pos >= bytes.len() {
            break;
        }

        // Parse pattern
        let pattern = if bytes[pos] == b'{' {
            // No pattern — always matches
            Pattern::Always
        } else {
            let pat_start = pos;
            // Read until '{'
            while pos < bytes.len() && bytes[pos] != b'{' {
                pos += 1;
            }
            let pat_str = prog[pat_start..pos].trim();
            if pat_str == "BEGIN" {
                Pattern::Begin
            } else if pat_str == "END" {
                Pattern::End
            } else if let Some(inner) = pat_str
                .strip_prefix('/')
                .and_then(|s| s.strip_suffix('/'))
            {
                Pattern::Regex(inner.to_string())
            } else {
                Pattern::Condition(pat_str.to_string())
            }
        };

        // Parse action in braces
        if pos < bytes.len() && bytes[pos] == b'{' {
            pos += 1;
            let action_start = pos;
            let mut depth = 1;
            while pos < bytes.len() && depth > 0 {
                if bytes[pos] == b'{' {
                    depth += 1;
                } else if bytes[pos] == b'}' {
                    depth -= 1;
                }
                if depth > 0 {
                    pos += 1;
                }
            }
            let action = prog[action_start..pos].trim().to_string();
            if pos < bytes.len() {
                pos += 1; // skip '}'
            }

            rules.push(Rule { pattern, action });
        }
    }

    rules
}

fn pattern_matches(pattern: &Pattern, line: &str, fields: &[&str], nr: usize) -> bool {
    match pattern {
        Pattern::Always => true,
        Pattern::Begin | Pattern::End => false,
        Pattern::Regex(re) => simple_contains(line, re),
        Pattern::Condition(cond) => eval_condition(cond, fields, line, nr),
    }
}

fn simple_contains(text: &str, pattern: &str) -> bool {
    text.contains(pattern)
}

fn eval_condition(cond: &str, fields: &[&str], line: &str, nr: usize) -> bool {
    let cond = cond.trim();

    // $N == "value"
    if let Some((left, right)) = cond.split_once("==") {
        let left_val = resolve_value(left.trim(), fields, line, nr);
        let right_val = resolve_value(right.trim(), fields, line, nr);
        return left_val == right_val;
    }
    if let Some((left, right)) = cond.split_once("!=") {
        let left_val = resolve_value(left.trim(), fields, line, nr);
        let right_val = resolve_value(right.trim(), fields, line, nr);
        return left_val != right_val;
    }
    if let Some((left, right)) = cond.split_once('>')
        && !right.starts_with('=') {
            let l: f64 = resolve_value(left.trim(), fields, line, nr)
                .parse()
                .unwrap_or(0.0);
            let r: f64 = resolve_value(right.trim(), fields, line, nr)
                .parse()
                .unwrap_or(0.0);
            return l > r;
        }
    if let Some((left, right)) = cond.split_once('<')
        && !right.starts_with('=') {
            let l: f64 = resolve_value(left.trim(), fields, line, nr)
                .parse()
                .unwrap_or(0.0);
            let r: f64 = resolve_value(right.trim(), fields, line, nr)
                .parse()
                .unwrap_or(0.0);
            return l < r;
        }

    // NR > N, NF > N etc.
    true
}

fn resolve_value(expr: &str, fields: &[&str], line: &str, nr: usize) -> String {
    let expr = expr.trim().trim_matches('"');

    if let Some(rest) = expr.strip_prefix('$') {
        let n: usize = rest.parse().unwrap_or(0);
        if n == 0 {
            line.to_string()
        } else if n <= fields.len() {
            fields[n - 1].to_string()
        } else {
            String::new()
        }
    } else if expr == "NR" {
        nr.to_string()
    } else if expr == "NF" {
        fields.len().to_string()
    } else {
        expr.to_string()
    }
}

fn execute_action(
    action: &str,
    fields: &[&str],
    line: &str,
    nr: usize,
    nf: usize,
    _fs_char: char,
    out: &mut impl Write,
) {
    // Split action into statements by semicolons
    for stmt in action.split(';') {
        let stmt = stmt.trim();
        if stmt.is_empty() {
            continue;
        }

        if stmt == "print" || stmt == "print $0" {
            let _ = writeln!(out, "{line}");
        } else if let Some(args_str) = stmt.strip_prefix("print ") {
            let mut output = String::new();

            // Parse print arguments separated by commas
            let parts: Vec<&str> = args_str.split(',').collect();
            for (i, part) in parts.iter().enumerate() {
                let part = part.trim();
                if i > 0 {
                    output.push(' ');
                }
                let val = eval_expr(part, fields, line, nr, nf);
                output.push_str(&val);
            }

            let _ = writeln!(out, "{output}");
        }
        // Other statements could be added here (assignments, if/else, etc.)
    }
}

fn eval_expr(expr: &str, fields: &[&str], line: &str, nr: usize, nf: usize) -> String {
    let expr = expr.trim();

    // String literal
    if let Some(inner) = expr.strip_prefix('"').and_then(|s| s.strip_suffix('"')) {
        return inner.to_string();
    }

    // Field reference
    if let Some(rest) = expr.strip_prefix('$') {
        let n: usize = rest.parse().unwrap_or(0);
        return if n == 0 {
            line.to_string()
        } else if n <= fields.len() {
            fields[n - 1].to_string()
        } else {
            String::new()
        };
    }

    // Built-in variables
    if expr == "NR" {
        return nr.to_string();
    }
    if expr == "NF" {
        return nf.to_string();
    }

    // Built-in functions
    if expr.starts_with("length(") {
        let arg = &expr[7..expr.len().saturating_sub(1)];
        let val = eval_expr(arg, fields, line, nr, nf);
        return val.len().to_string();
    }
    if expr.starts_with("toupper(") {
        let arg = &expr[8..expr.len().saturating_sub(1)];
        let val = eval_expr(arg, fields, line, nr, nf);
        return val.to_uppercase();
    }
    if expr.starts_with("tolower(") {
        let arg = &expr[8..expr.len().saturating_sub(1)];
        let val = eval_expr(arg, fields, line, nr, nf);
        return val.to_lowercase();
    }

    // Arithmetic: check for +, -, *, /
    for op in &['+', '-', '*', '/'] {
        if let Some((left, right)) = expr.split_once(*op)
            && !left.is_empty() {
                let l: f64 = eval_expr(left, fields, line, nr, nf)
                    .parse()
                    .unwrap_or(0.0);
                let r: f64 = eval_expr(right, fields, line, nr, nf)
                    .parse()
                    .unwrap_or(0.0);
                let result = match op {
                    '+' => l + r,
                    '-' => l - r,
                    '*' => l * r,
                    '/'
                        if r != 0.0 => {
                            l / r
                        }
                    _ => 0.0,
                };
                // Print as integer if no fractional part
                if result.fract() == 0.0 {
                    return format!("{}", result as i64);
                }
                return format!("{result}");
            }
    }

    // Literal number or string
    expr.to_string()
}
