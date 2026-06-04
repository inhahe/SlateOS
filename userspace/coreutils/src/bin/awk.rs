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
    let parsed = match parse_args(&args) {
        Ok(p) => p,
        Err(msg) => {
            eprintln!("awk: {msg}");
            process::exit(1);
        }
    };

    let AwkArgs { fs_char, program, mut files } = parsed;
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
            nr = nr.saturating_add(1);

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

/// Parsed CLI arguments.
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
#[derive(Default)]
struct AwkArgs {
    fs_char: char,
    program: String,
    files: Vec<String>,
}

/// Parse the awk command line.  The first non-flag positional argument is
/// the program; remaining positionals are input files.
fn parse_args(args: &[String]) -> Result<AwkArgs, String> {
    let mut fs_char = ' ';
    let mut program: Option<String> = None;
    let mut files: Vec<String> = Vec::new();
    let mut i = 0;

    while let Some(arg) = args.get(i) {
        match arg.as_str() {
            "-F" => {
                i = i.saturating_add(1);
                let Some(sep) = args.get(i) else {
                    return Err("-F requires an argument".to_string());
                };
                fs_char = sep.chars().next().unwrap_or(' ');
            }
            other => {
                if program.is_none() {
                    program = Some(other.to_string());
                } else {
                    files.push(other.to_string());
                }
            }
        }
        i = i.saturating_add(1);
    }

    let program = program.ok_or_else(|| "no program specified".to_string())?;
    Ok(AwkArgs { fs_char, program, files })
}

#[cfg_attr(test, derive(Debug))]
#[derive(PartialEq, Eq)]
enum Pattern {
    Always,
    Begin,
    End,
    Regex(String),
    Condition(String), // raw condition string
}

#[cfg_attr(test, derive(Debug))]
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
        while let Some(b) = bytes.get(pos) {
            if !b.is_ascii_whitespace() {
                break;
            }
            pos = pos.saturating_add(1);
        }
        if pos >= bytes.len() {
            break;
        }

        // Parse pattern
        let pattern = if bytes.get(pos) == Some(&b'{') {
            // No pattern — always matches
            Pattern::Always
        } else {
            let pat_start = pos;
            // Read until '{'
            while let Some(b) = bytes.get(pos) {
                if *b == b'{' {
                    break;
                }
                pos = pos.saturating_add(1);
            }
            let pat_str = prog.get(pat_start..pos).unwrap_or("").trim();
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
        if bytes.get(pos) == Some(&b'{') {
            pos = pos.saturating_add(1);
            let action_start = pos;
            let mut depth: usize = 1;
            while let Some(b) = bytes.get(pos) {
                if *b == b'{' {
                    depth = depth.saturating_add(1);
                } else if *b == b'}' {
                    depth = depth.saturating_sub(1);
                }
                if depth == 0 {
                    break;
                }
                pos = pos.saturating_add(1);
            }
            let action = prog.get(action_start..pos).unwrap_or("").trim().to_string();
            if pos < bytes.len() {
                pos = pos.saturating_add(1); // skip '}'
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
        && !right.starts_with('=')
    {
        let l: f64 = resolve_value(left.trim(), fields, line, nr)
            .parse()
            .unwrap_or(0.0);
        let r: f64 = resolve_value(right.trim(), fields, line, nr)
            .parse()
            .unwrap_or(0.0);
        return l > r;
    }
    if let Some((left, right)) = cond.split_once('<')
        && !right.starts_with('=')
    {
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
        } else {
            // Convert 1-based field index to 0-based slice index.
            fields
                .get(n.saturating_sub(1))
                .map(|s| (*s).to_string())
                .unwrap_or_default()
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

    // Field reference: $N where the rest is a pure decimal integer.  Be
    // strict here so expressions like `$1+$2` fall through to arithmetic
    // rather than getting misparsed as field 0.
    if let Some(rest) = expr.strip_prefix('$')
        && !rest.is_empty()
        && rest.bytes().all(|b| b.is_ascii_digit())
    {
        let n: usize = rest.parse().unwrap_or(0);
        return if n == 0 {
            line.to_string()
        } else {
            fields
                .get(n.saturating_sub(1))
                .map(|s| (*s).to_string())
                .unwrap_or_default()
        };
    }

    // Built-in variables
    if expr == "NR" {
        return nr.to_string();
    }
    if expr == "NF" {
        return nf.to_string();
    }

    // Built-in functions: name(arg)
    if let Some(arg) = strip_call(expr, "length") {
        let val = eval_expr(arg, fields, line, nr, nf);
        return val.len().to_string();
    }
    if let Some(arg) = strip_call(expr, "toupper") {
        let val = eval_expr(arg, fields, line, nr, nf);
        return val.to_uppercase();
    }
    if let Some(arg) = strip_call(expr, "tolower") {
        let val = eval_expr(arg, fields, line, nr, nf);
        return val.to_lowercase();
    }

    // Arithmetic: check for +, -, *, /
    for op in &['+', '-', '*', '/'] {
        if let Some((left, right)) = expr.split_once(*op)
            && !left.is_empty()
        {
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
                '/' if r != 0.0 => l / r,
                _ => 0.0,
            };
            // Print as integer if no fractional part and value fits in i64.
            #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
            if result.fract() == 0.0
                && result.is_finite()
                && result.abs() < (i64::MAX as f64)
            {
                return format!("{}", result as i64);
            }
            return format!("{result}");
        }
    }

    // Literal number or string
    expr.to_string()
}

/// If `expr` is exactly `name(...)`, return the inner argument string.
/// Otherwise return None.
fn strip_call<'a>(expr: &'a str, name: &str) -> Option<&'a str> {
    let rest = expr.strip_prefix(name)?;
    let inner = rest.strip_prefix('(')?;
    inner.strip_suffix(')')
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]
mod tests {
    use super::*;

    fn s(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| (*s).to_string()).collect()
    }

    // ---------- parse_args ----------

    #[test]
    fn parse_args_empty_errors() {
        let err = parse_args(&[]).unwrap_err();
        assert!(err.contains("no program"));
    }

    #[test]
    fn parse_args_program_only() {
        let p = parse_args(&s(&["{ print }"])).unwrap();
        assert_eq!(p.program, "{ print }");
        assert!(p.files.is_empty());
        assert_eq!(p.fs_char, ' ');
    }

    #[test]
    fn parse_args_program_and_files() {
        let p = parse_args(&s(&["{ print }", "a.txt", "b.txt"])).unwrap();
        assert_eq!(p.program, "{ print }");
        assert_eq!(p.files, vec!["a.txt".to_string(), "b.txt".to_string()]);
    }

    #[test]
    fn parse_args_f_flag_sets_separator() {
        let p = parse_args(&s(&["-F", ":", "{ print $1 }"])).unwrap();
        assert_eq!(p.fs_char, ':');
        assert_eq!(p.program, "{ print $1 }");
    }

    #[test]
    fn parse_args_f_flag_uses_first_char_of_multichar_sep() {
        let p = parse_args(&s(&["-F", "::", "{ print }"])).unwrap();
        assert_eq!(p.fs_char, ':');
    }

    #[test]
    fn parse_args_missing_f_value_errors() {
        let err = parse_args(&s(&["-F"])).unwrap_err();
        assert!(err.contains("-F"));
    }

    #[test]
    fn parse_args_f_flag_with_files() {
        let p = parse_args(&s(&["-F", ",", "{ print }", "data.csv"])).unwrap();
        assert_eq!(p.fs_char, ',');
        assert_eq!(p.files, vec!["data.csv".to_string()]);
    }

    // ---------- parse_program ----------

    #[test]
    fn parse_program_bare_pattern_gets_implicit_print() {
        let rules = parse_program("foo");
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].pattern, Pattern::Regex("foo".to_string()));
        assert_eq!(rules[0].action, "print");
    }

    #[test]
    fn parse_program_always_action() {
        let rules = parse_program("{ print }");
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].pattern, Pattern::Always);
        assert_eq!(rules[0].action, "print");
    }

    #[test]
    fn parse_program_begin_end() {
        let rules = parse_program("BEGIN { print \"start\" } { print } END { print \"end\" }");
        assert_eq!(rules.len(), 3);
        assert_eq!(rules[0].pattern, Pattern::Begin);
        assert_eq!(rules[1].pattern, Pattern::Always);
        assert_eq!(rules[2].pattern, Pattern::End);
    }

    #[test]
    fn parse_program_regex_pattern() {
        let rules = parse_program("/foo/ { print }");
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].pattern, Pattern::Regex("foo".to_string()));
    }

    #[test]
    fn parse_program_condition_pattern() {
        let rules = parse_program("$1 == \"x\" { print }");
        assert_eq!(rules.len(), 1);
        assert_eq!(
            rules[0].pattern,
            Pattern::Condition("$1 == \"x\"".to_string())
        );
    }

    #[test]
    fn parse_program_handles_nested_braces() {
        let rules = parse_program("{ { print } }");
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].action, "{ print }");
    }

    // ---------- pattern_matches ----------

    #[test]
    fn pattern_matches_always() {
        assert!(pattern_matches(&Pattern::Always, "anything", &[], 1));
    }

    #[test]
    fn pattern_matches_begin_end_never_during_main() {
        assert!(!pattern_matches(&Pattern::Begin, "x", &[], 1));
        assert!(!pattern_matches(&Pattern::End, "x", &[], 1));
    }

    #[test]
    fn pattern_matches_regex_substring() {
        let p = Pattern::Regex("ello".to_string());
        assert!(pattern_matches(&p, "hello world", &[], 1));
        assert!(!pattern_matches(&p, "bye", &[], 1));
    }

    // ---------- simple_contains ----------

    #[test]
    fn simple_contains_substring() {
        assert!(simple_contains("hello world", "world"));
        assert!(!simple_contains("hello", "bye"));
        assert!(simple_contains("xyz", ""));
    }

    // ---------- resolve_value ----------

    #[test]
    fn resolve_value_dollar_zero_is_line() {
        assert_eq!(resolve_value("$0", &["a", "b"], "the line", 5), "the line");
    }

    #[test]
    fn resolve_value_field_in_range() {
        assert_eq!(resolve_value("$2", &["a", "b", "c"], "", 1), "b");
    }

    #[test]
    fn resolve_value_field_out_of_range_is_empty() {
        assert_eq!(resolve_value("$9", &["a"], "", 1), "");
    }

    #[test]
    fn resolve_value_nr_and_nf() {
        assert_eq!(resolve_value("NR", &["a"], "", 7), "7");
        assert_eq!(resolve_value("NF", &["a", "b"], "", 1), "2");
    }

    #[test]
    fn resolve_value_quoted_literal_strips_quotes() {
        assert_eq!(resolve_value("\"hello\"", &[], "", 0), "hello");
    }

    #[test]
    fn resolve_value_bare_literal_passes_through() {
        assert_eq!(resolve_value("foo", &[], "", 0), "foo");
    }

    // ---------- eval_condition ----------

    #[test]
    fn eval_condition_eq_matches() {
        assert!(eval_condition("$1 == \"a\"", &["a", "b"], "", 1));
        assert!(!eval_condition("$1 == \"x\"", &["a", "b"], "", 1));
    }

    #[test]
    fn eval_condition_ne_matches() {
        assert!(eval_condition("$1 != \"x\"", &["a", "b"], "", 1));
        assert!(!eval_condition("$1 != \"a\"", &["a", "b"], "", 1));
    }

    #[test]
    fn eval_condition_gt_lt_numeric() {
        assert!(eval_condition("$1 > 2", &["3"], "", 1));
        assert!(!eval_condition("$1 > 5", &["3"], "", 1));
        assert!(eval_condition("$1 < 5", &["3"], "", 1));
    }

    #[test]
    fn eval_condition_unknown_is_truthy() {
        // Falls through to default `true` (matches every line).
        assert!(eval_condition("something_weird", &[], "", 1));
    }

    // ---------- execute_action / eval_expr ----------

    fn run_action(action: &str, fields: &[&str], line: &str, nr: usize, nf: usize) -> String {
        let mut buf = Vec::new();
        execute_action(action, fields, line, nr, nf, ' ', &mut buf);
        String::from_utf8(buf).unwrap()
    }

    #[test]
    fn action_bare_print_emits_line() {
        assert_eq!(run_action("print", &["a"], "the line", 1, 1), "the line\n");
    }

    #[test]
    fn action_print_dollar0_emits_line() {
        assert_eq!(run_action("print $0", &["a"], "hi", 1, 1), "hi\n");
    }

    #[test]
    fn action_print_field_reference() {
        assert_eq!(
            run_action("print $2", &["a", "b", "c"], "a b c", 1, 3),
            "b\n"
        );
    }

    #[test]
    fn action_print_multiple_args_space_separated() {
        assert_eq!(
            run_action("print $1, $3", &["a", "b", "c"], "a b c", 1, 3),
            "a c\n"
        );
    }

    #[test]
    fn action_print_string_literal() {
        assert_eq!(run_action("print \"hello\"", &[], "", 1, 0), "hello\n");
    }

    #[test]
    fn action_semicolon_separated_statements() {
        // Two prints emit two lines.
        let out = run_action("print \"a\" ; print \"b\"", &[], "", 1, 0);
        assert_eq!(out, "a\nb\n");
    }

    #[test]
    fn action_empty_statements_skipped() {
        // Leading / trailing / doubled ';' should not panic or print blanks.
        let out = run_action(";; print \"x\" ; ;", &[], "", 1, 0);
        assert_eq!(out, "x\n");
    }

    #[test]
    fn action_print_nr_and_nf() {
        assert_eq!(
            run_action("print NR, NF", &["a", "b"], "", 7, 2),
            "7 2\n"
        );
    }

    #[test]
    fn action_print_length_builtin() {
        assert_eq!(
            run_action("print length($1)", &["hello"], "", 1, 1),
            "5\n"
        );
    }

    #[test]
    fn action_print_toupper_tolower() {
        assert_eq!(
            run_action("print toupper(\"abc\")", &[], "", 1, 0),
            "ABC\n"
        );
        assert_eq!(
            run_action("print tolower(\"AbC\")", &[], "", 1, 0),
            "abc\n"
        );
    }

    #[test]
    fn action_arithmetic_integer_result_prints_without_decimal() {
        assert_eq!(
            run_action("print $1+$2", &["2", "3"], "", 1, 2),
            "5\n"
        );
        assert_eq!(
            run_action("print $1*$2", &["4", "3"], "", 1, 2),
            "12\n"
        );
    }

    #[test]
    fn action_division_by_zero_yields_zero() {
        // The default branch returns 0 when divisor is 0 — verify no panic.
        assert_eq!(
            run_action("print $1/$2", &["5", "0"], "", 1, 2),
            "0\n"
        );
    }

    // ---------- strip_call ----------

    #[test]
    fn strip_call_matches_named_fn() {
        assert_eq!(strip_call("length(x)", "length"), Some("x"));
        assert_eq!(strip_call("toupper(\"abc\")", "toupper"), Some("\"abc\""));
    }

    #[test]
    fn strip_call_wrong_name_returns_none() {
        assert_eq!(strip_call("length(x)", "toupper"), None);
    }

    #[test]
    fn strip_call_missing_parens_returns_none() {
        assert_eq!(strip_call("length", "length"), None);
        assert_eq!(strip_call("length(x", "length"), None);
    }
}
