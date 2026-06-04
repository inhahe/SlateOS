//! OurOS `make` — build automation tool
//!
//! A POSIX-compatible make(1) implementation that reads Makefiles, resolves
//! dependency DAGs, and executes recipes to bring targets up to date.  Supports
//! variable assignment (recursive, simple, conditional, append), pattern rules,
//! automatic variables, conditional directives, include directives, and the
//! standard command-line options.

use std::collections::{HashMap, HashSet};
use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process;
use std::time::SystemTime;

// ---------------------------------------------------------------------------
// Data types
// ---------------------------------------------------------------------------

/// How a variable was assigned — determines expansion behaviour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VarFlavour {
    /// `=` — expanded at use-time (recursive).
    Recursive,
    /// `:=` — expanded at assignment-time (simple).
    Simple,
}

/// Where a variable binding came from.  Command-line overrides always win.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum VarOrigin {
    Default = 0,
    File = 1,
    Environment = 2,
    CommandLine = 3,
}

/// A single variable binding.
#[derive(Debug, Clone)]
struct Variable {
    value: String,
    flavour: VarFlavour,
    origin: VarOrigin,
}

/// A concrete (non-pattern) rule.
#[derive(Debug, Clone)]
struct Rule {
    target: String,
    prerequisites: Vec<String>,
    recipe: Vec<String>,
    /// `true` for `.PHONY` targets.
    phony: bool,
}

/// A pattern rule such as `%.o: %.c`.
#[derive(Debug, Clone)]
struct PatternRule {
    /// Target pattern, e.g. `%.o`.
    target_pat: String,
    /// Prerequisite patterns, e.g. `%.c`.
    prereq_pats: Vec<String>,
    recipe: Vec<String>,
}

/// Prefix flags on a single recipe line.
#[derive(Debug, Clone, Copy, Default)]
struct RecipeFlags {
    /// `@` — suppress echoing the command.
    silent: bool,
    /// `-` — ignore errors from this command.
    ignore_error: bool,
    /// `+` — execute even in dry-run mode.
    force_exec: bool,
}

/// Command-line options.
#[derive(Debug, Clone)]
struct Options {
    makefile: Option<String>,
    dry_run: bool,
    keep_going: bool,
    always_make: bool,
    silent: bool,
    jobs: usize,
    directory: Option<String>,
    print_database: bool,
    question_mode: bool,
    targets: Vec<String>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            makefile: None,
            dry_run: false,
            keep_going: false,
            always_make: false,
            silent: false,
            jobs: 1,
            directory: None,
            print_database: false,
            question_mode: false,
            targets: Vec::new(),
        }
    }
}

/// The complete parsed makefile database.
#[derive(Debug, Clone)]
struct MakeDb {
    variables: HashMap<String, Variable>,
    rules: Vec<Rule>,
    pattern_rules: Vec<PatternRule>,
    phony_targets: HashSet<String>,
    /// Index of the first explicit (non-pattern, non-special) rule — its
    /// target is the default goal.
    default_target: Option<String>,
}

impl MakeDb {
    fn new() -> Self {
        Self {
            variables: HashMap::new(),
            rules: Vec::new(),
            pattern_rules: Vec::new(),
            phony_targets: HashSet::new(),
            default_target: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Argument parsing
// ---------------------------------------------------------------------------

/// Parse command-line arguments into `Options` plus any `VAR=value` overrides.
fn parse_args(args: &[String]) -> (Options, Vec<(String, String)>) {
    let mut opts = Options::default();
    let mut overrides: Vec<(String, String)> = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        if arg == "-f" {
            i += 1;
            if i < args.len() {
                opts.makefile = Some(args[i].clone());
            }
        } else if arg == "-n" || arg == "--just-print" {
            opts.dry_run = true;
        } else if arg == "-k" || arg == "--keep-going" {
            opts.keep_going = true;
        } else if arg == "-B" || arg == "--always-make" {
            opts.always_make = true;
        } else if arg == "-s" || arg == "--silent" {
            opts.silent = true;
        } else if arg == "-p" {
            opts.print_database = true;
        } else if arg == "-q" {
            opts.question_mode = true;
        } else if arg == "-j" {
            i += 1;
            if i < args.len()
                && let Ok(n) = args[i].parse::<usize>() {
                    opts.jobs = n;
                }
        } else if arg == "-C" {
            i += 1;
            if i < args.len() {
                opts.directory = Some(args[i].clone());
            }
        } else if let Some(eq_pos) = arg.find('=') {
            // VAR=value override — but only if the part before `=` looks
            // like an identifier (no dashes etc.)
            let name = &arg[..eq_pos];
            if !name.is_empty() && !name.starts_with('-') && is_var_name(name) {
                let value = &arg[eq_pos + 1..];
                overrides.push((name.to_string(), value.to_string()));
            } else {
                opts.targets.push(arg.clone());
            }
        } else if arg.starts_with('-') {
            // Ignore unknown flags gracefully.
        } else {
            opts.targets.push(arg.clone());
        }
        i += 1;
    }
    (opts, overrides)
}

/// Return `true` if `s` is a valid make variable name (letters, digits, `_`).
fn is_var_name(s: &str) -> bool {
    s.bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'.')
}

// ---------------------------------------------------------------------------
// Variable expansion
// ---------------------------------------------------------------------------

/// Expand `$(VAR)` / `${VAR}` references in `input`.
///
/// `auto_vars` supplies the automatic variables (`@`, `<`, `^`, `*`, `?`)
/// that are set per-rule.  They override anything in `db.variables`.
fn expand_vars(input: &str, db: &MakeDb, auto_vars: &HashMap<String, String>) -> String {
    expand_vars_depth(input, db, auto_vars, 0)
}

fn expand_vars_depth(
    input: &str,
    db: &MakeDb,
    auto_vars: &HashMap<String, String>,
    depth: usize,
) -> String {
    if depth > 64 {
        return input.to_string(); // prevent infinite recursion
    }
    let bytes = input.as_bytes();
    let mut out = String::with_capacity(input.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'$' && i + 1 < bytes.len() {
            let next = bytes[i + 1];
            if next == b'(' || next == b'{' {
                let close = if next == b'(' { b')' } else { b'}' };
                if let Some(end) = find_matching_close(bytes, i + 2, close) {
                    let var_expr =
                        std::str::from_utf8(&bytes[i + 2..end]).unwrap_or("");
                    let expanded_name =
                        expand_vars_depth(var_expr, db, auto_vars, depth + 1);
                    let val = lookup_var(&expanded_name, db, auto_vars);
                    let resolved = resolve_var_value(&val, db, auto_vars, depth);
                    out.push_str(&resolved);
                    i = end + 1;
                } else {
                    out.push('$');
                    i += 1;
                }
            } else if next == b'$' {
                out.push('$');
                i += 2;
            } else {
                // Single-char variable: $@, $<, $^, $*, $?
                let name = std::str::from_utf8(&bytes[i + 1..i + 2]).unwrap_or("");
                let val = lookup_var(name, db, auto_vars);
                let resolved = resolve_var_value(&val, db, auto_vars, depth);
                out.push_str(&resolved);
                i += 2;
            }
        } else {
            out.push(bytes[i] as char);
            i += 1;
        }
    }
    out
}

/// Look up a variable by name, checking auto vars first, then the database.
fn lookup_var(
    name: &str,
    db: &MakeDb,
    auto_vars: &HashMap<String, String>,
) -> Option<Variable> {
    if let Some(v) = auto_vars.get(name) {
        return Some(Variable {
            value: v.clone(),
            flavour: VarFlavour::Simple,
            origin: VarOrigin::Default,
        });
    }
    db.variables.get(name).cloned()
}

/// Resolve a variable's value, re-expanding for recursive flavour.
fn resolve_var_value(
    var: &Option<Variable>,
    db: &MakeDb,
    auto_vars: &HashMap<String, String>,
    depth: usize,
) -> String {
    match var {
        Some(v) if v.flavour == VarFlavour::Recursive => {
            expand_vars_depth(&v.value, db, auto_vars, depth + 1)
        }
        Some(v) => v.value.clone(),
        None => String::new(),
    }
}

/// Find the matching close delimiter, respecting nesting for `$(...)`/`${...}`.
fn find_matching_close(bytes: &[u8], start: usize, close: u8) -> Option<usize> {
    let open = if close == b')' { b'(' } else { b'{' };
    let mut depth: usize = 1;
    let mut i = start;
    while i < bytes.len() {
        if bytes[i] == b'$' && i + 1 < bytes.len() && bytes[i + 1] == open {
            depth += 1;
            i += 2;
        } else if bytes[i] == close {
            depth -= 1;
            if depth == 0 {
                return Some(i);
            }
            i += 1;
        } else {
            i += 1;
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Makefile parsing
// ---------------------------------------------------------------------------

/// Read and join continuation lines (`\` at end), strip comments, return the
/// logical lines together with their types.
fn read_logical_lines(contents: &str) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();
    for raw in contents.lines() {
        if let Some(head) = raw.strip_suffix('\\') {
            current.push_str(head);
            current.push(' ');
        } else {
            current.push_str(raw);
            lines.push(current.clone());
            current.clear();
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines
}

/// Strip a `# comment` from a line (respecting `\#` escapes).
fn strip_comment(line: &str) -> &str {
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\' {
            i += 2; // skip escaped character
            continue;
        }
        if bytes[i] == b'#' {
            return &line[..i];
        }
        i += 1;
    }
    line
}

/// Trim trailing whitespace.
fn rtrim(s: &str) -> &str {
    s.trim_end()
}

/// Parse a makefile from `path`, merging into `db`.  `included` prevents
/// infinite include loops.
fn parse_makefile(
    path: &Path,
    db: &mut MakeDb,
    included: &mut HashSet<PathBuf>,
    cmd_overrides: &[(String, String)],
) -> io::Result<()> {
    let canonical = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    if !included.insert(canonical.clone()) {
        return Ok(()); // already included
    }

    // Record in MAKEFILE_LIST
    let mfl_val = if let Some(v) = db.variables.get("MAKEFILE_LIST") {
        format!("{} {}", v.value, path.display())
    } else {
        path.display().to_string()
    };
    db.variables.insert(
        "MAKEFILE_LIST".into(),
        Variable {
            value: mfl_val,
            flavour: VarFlavour::Simple,
            origin: VarOrigin::File,
        },
    );

    let contents = fs::read_to_string(path)?;
    let logical = read_logical_lines(&contents);
    parse_lines(&logical, db, included, cmd_overrides, path)?;
    Ok(())
}

/// Parse already-split logical lines into the database.
fn parse_lines(
    lines: &[String],
    db: &mut MakeDb,
    included: &mut HashSet<PathBuf>,
    cmd_overrides: &[(String, String)],
    context_path: &Path,
) -> io::Result<()> {
    let mut idx = 0;
    // Track conditional nesting: each entry is `true` if we are in an active
    // (executing) branch, `false` if we are in an inactive branch.
    let mut cond_stack: Vec<bool> = Vec::new();

    while idx < lines.len() {
        let raw_line = &lines[idx];
        // Recipe lines start with a tab; handle them specially below.
        let is_recipe = raw_line.starts_with('\t');
        let stripped = if is_recipe {
            raw_line.as_str()
        } else {
            rtrim(strip_comment(raw_line))
        };
        let trimmed = stripped.trim();

        // --- conditional directives ---
        if let Some(rest) = try_strip_directive(trimmed, "ifeq") {
            let active = cond_stack.last().copied().unwrap_or(true);
            if active {
                let val = eval_ifeq(rest, db, &HashMap::new());
                cond_stack.push(val);
            } else {
                cond_stack.push(false);
            }
            idx += 1;
            continue;
        }
        if let Some(rest) = try_strip_directive(trimmed, "ifneq") {
            let active = cond_stack.last().copied().unwrap_or(true);
            if active {
                let val = !eval_ifeq(rest, db, &HashMap::new());
                cond_stack.push(val);
            } else {
                cond_stack.push(false);
            }
            idx += 1;
            continue;
        }
        if let Some(rest) = try_strip_directive(trimmed, "ifdef") {
            let active = cond_stack.last().copied().unwrap_or(true);
            if active {
                let var_name = rest.trim();
                cond_stack.push(db.variables.contains_key(var_name));
            } else {
                cond_stack.push(false);
            }
            idx += 1;
            continue;
        }
        if let Some(rest) = try_strip_directive(trimmed, "ifndef") {
            let active = cond_stack.last().copied().unwrap_or(true);
            if active {
                let var_name = rest.trim();
                cond_stack.push(!db.variables.contains_key(var_name));
            } else {
                cond_stack.push(false);
            }
            idx += 1;
            continue;
        }
        if trimmed == "else" {
            let len = cond_stack.len();
            if len > 0 {
                // Only flip if the enclosing scope is active.  Check the
                // *parent* — if the parent is inactive the else branch stays
                // inactive too.
                let parent_active = if len >= 2 {
                    cond_stack[len - 2]
                } else {
                    true
                };
                if parent_active {
                    let top = &mut cond_stack[len - 1];
                    *top = !*top;
                }
            }
            idx += 1;
            continue;
        }
        if trimmed == "endif" {
            cond_stack.pop();
            idx += 1;
            continue;
        }

        // Skip lines inside inactive conditional branches.
        if cond_stack.last().copied() == Some(false) {
            idx += 1;
            continue;
        }

        // --- include directive ---
        if let Some(rest) = trimmed.strip_prefix("include ") {
            let inc_path = expand_vars(rest.trim(), db, &HashMap::new());
            let base = context_path.parent().unwrap_or(Path::new("."));
            let full = base.join(&inc_path);
            parse_makefile(&full, db, included, cmd_overrides)?;
            idx += 1;
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("-include ") {
            let inc_path = expand_vars(rest.trim(), db, &HashMap::new());
            let base = context_path.parent().unwrap_or(Path::new("."));
            let full = base.join(&inc_path);
            let _ = parse_makefile(&full, db, included, cmd_overrides);
            idx += 1;
            continue;
        }

        // --- blank / comment-only lines ---
        if trimmed.is_empty() {
            idx += 1;
            continue;
        }

        // --- variable assignment ---
        if let Some((name, val, flav)) = try_parse_variable(stripped) {
            // Command-line overrides always win.
            let overridden = cmd_overrides.iter().any(|(n, _)| n == &name);
            if !overridden {
                apply_variable(db, &name, &val, flav, VarOrigin::File);
            }
            idx += 1;
            continue;
        }

        // --- rule line ---
        if !is_recipe
            && let Some(colon_pos) = find_rule_colon(stripped) {
                let target_part = stripped[..colon_pos].trim();
                let prereq_part = stripped[colon_pos + 1..].trim();

                // Collect recipe lines (tab-indented lines that follow).
                let mut recipe: Vec<String> = Vec::new();
                idx += 1;
                while idx < lines.len() && lines[idx].starts_with('\t') {
                    recipe.push(lines[idx][1..].to_string());
                    idx += 1;
                }

                let targets: Vec<&str> =
                    target_part.split_whitespace().collect();
                let prereqs: Vec<String> = prereq_part
                    .split_whitespace()
                    .map(|s| s.to_string())
                    .collect();

                // Check for .PHONY
                if targets.len() == 1 && targets[0] == ".PHONY" {
                    for p in &prereqs {
                        db.phony_targets.insert(p.clone());
                    }
                    // Mark any existing rules for these targets as phony.
                    for rule in &mut db.rules {
                        if db.phony_targets.contains(&rule.target) {
                            rule.phony = true;
                        }
                    }
                    continue;
                }

                // Pattern rule vs concrete rule.
                for tgt in &targets {
                    if tgt.contains('%') {
                        db.pattern_rules.push(PatternRule {
                            target_pat: tgt.to_string(),
                            prereq_pats: prereqs.clone(),
                            recipe: recipe.clone(),
                        });
                    } else {
                        let is_phony = db.phony_targets.contains(*tgt);
                        // Check if this rule merges with an existing one.
                        let existing = db
                            .rules
                            .iter_mut()
                            .find(|r| r.target == *tgt);
                        if let Some(existing_rule) = existing {
                            // Merge prerequisites.
                            for p in &prereqs {
                                if !existing_rule.prerequisites.contains(p) {
                                    existing_rule.prerequisites.push(p.clone());
                                }
                            }
                            // Replace recipe only if the new one is non-empty.
                            if !recipe.is_empty() {
                                existing_rule.recipe = recipe.clone();
                            }
                        } else {
                            db.rules.push(Rule {
                                target: tgt.to_string(),
                                prerequisites: prereqs.clone(),
                                recipe: recipe.clone(),
                                phony: is_phony,
                            });
                        }
                        if db.default_target.is_none()
                            && !tgt.starts_with('.')
                        {
                            db.default_target = Some(tgt.to_string());
                        }
                    }
                }
                continue;
            }

        idx += 1;
    }
    Ok(())
}

/// Try to parse a line as a variable assignment.  Returns
/// `(name, raw_value, flavour)` on success.
fn try_parse_variable(line: &str) -> Option<(String, String, VarFlavour)> {
    let trimmed = line.trim();
    // Try `:=` first (simple).
    if let Some(pos) = trimmed.find(":=") {
        let name = trimmed[..pos].trim();
        if !name.is_empty() && is_var_name(name) {
            let val = trimmed[pos + 2..].trim();
            return Some((name.to_string(), val.to_string(), VarFlavour::Simple));
        }
    }
    // `?=` — conditional.
    if let Some(pos) = trimmed.find("?=") {
        let name = trimmed[..pos].trim();
        if !name.is_empty() && is_var_name(name) {
            let val = trimmed[pos + 2..].trim();
            // Conditional: only set if not yet defined.  We signal this with
            // a special flavour that the caller handles.  For simplicity we
            // return Recursive and let `apply_variable` check for conditional.
            return Some((
                format!("?{}", name),
                val.to_string(),
                VarFlavour::Recursive,
            ));
        }
    }
    // `+=` — append.
    if let Some(pos) = trimmed.find("+=") {
        let name = trimmed[..pos].trim();
        if !name.is_empty() && is_var_name(name) {
            let val = trimmed[pos + 2..].trim();
            return Some((
                format!("+{}", name),
                val.to_string(),
                VarFlavour::Recursive,
            ));
        }
    }
    // Plain `=` — recursive.
    if let Some(pos) = trimmed.find('=') {
        // Ensure no `:`, `?`, `+` immediately before `=`.
        if pos > 0 {
            let before = trimmed.as_bytes()[pos - 1];
            if before == b':' || before == b'?' || before == b'+' {
                return None;
            }
        }
        let name = trimmed[..pos].trim();
        if !name.is_empty() && is_var_name(name) {
            let val = trimmed[pos + 1..].trim();
            return Some((name.to_string(), val.to_string(), VarFlavour::Recursive));
        }
    }
    None
}

/// Apply a variable assignment to the database.
fn apply_variable(
    db: &mut MakeDb,
    raw_name: &str,
    value: &str,
    flavour: VarFlavour,
    origin: VarOrigin,
) {
    // Handle conditional (`?` prefix).
    if let Some(name) = raw_name.strip_prefix('?') {
        if !db.variables.contains_key(name) {
            db.variables.insert(
                name.to_string(),
                Variable {
                    value: value.to_string(),
                    flavour,
                    origin,
                },
            );
        }
        return;
    }

    // Handle append (`+` prefix).
    if let Some(name) = raw_name.strip_prefix('+') {
        if let Some(existing) = db.variables.get_mut(name) {
            if existing.value.is_empty() {
                existing.value = value.to_string();
            } else {
                existing.value.push(' ');
                existing.value.push_str(value);
            }
        } else {
            db.variables.insert(
                name.to_string(),
                Variable {
                    value: value.to_string(),
                    flavour,
                    origin,
                },
            );
        }
        return;
    }

    // Don't override a higher-priority binding (e.g. command-line beats file).
    if let Some(existing) = db.variables.get(raw_name)
        && existing.origin > origin {
            return;
        }

    // For simple assignment, expand immediately.
    let final_val = if flavour == VarFlavour::Simple {
        expand_vars(value, db, &HashMap::new())
    } else {
        value.to_string()
    };

    db.variables.insert(
        raw_name.to_string(),
        Variable {
            value: final_val,
            flavour,
            origin,
        },
    );
}

/// Find the colon that separates target(s) from prerequisites in a rule line.
/// Must skip `:=` (variable assignment) and drive letters like `C:`.
fn find_rule_colon(line: &str) -> Option<usize> {
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b':' {
            // Skip `:=`
            if i + 1 < bytes.len() && bytes[i + 1] == b'=' {
                return None;
            }
            // Skip single-letter drive prefix (e.g. `C:`)
            if i == 1 && bytes[0].is_ascii_alphabetic() {
                i += 1;
                continue;
            }
            return Some(i);
        }
        // Skip `$(...)`
        if bytes[i] == b'$' && i + 1 < bytes.len() && bytes[i + 1] == b'('
            && let Some(close) = find_matching_close(bytes, i + 2, b')') {
                i = close + 1;
                continue;
            }
        i += 1;
    }
    None
}

/// Try to strip a conditional directive keyword from the beginning of a line.
fn try_strip_directive<'a>(line: &'a str, keyword: &str) -> Option<&'a str> {
    if let Some(rest) = line.strip_prefix(keyword)
        && (rest.is_empty()
            || rest.starts_with(' ')
            || rest.starts_with('\t')
            || rest.starts_with('('))
    {
        return Some(rest.trim_start());
    }
    None
}

/// Evaluate an `ifeq` / `ifneq` condition.  Supports both `(a,b)` and
/// `"a" "b"` syntax.
fn eval_ifeq(args: &str, db: &MakeDb, auto_vars: &HashMap<String, String>) -> bool {
    let args = args.trim();
    if args.starts_with('(') && args.ends_with(')') {
        let inner = &args[1..args.len() - 1];
        if let Some(comma) = inner.find(',') {
            let lhs = expand_vars(inner[..comma].trim(), db, auto_vars);
            let rhs = expand_vars(inner[comma + 1..].trim(), db, auto_vars);
            return lhs == rhs;
        }
    }
    // `"a" "b"` syntax
    let parts: Vec<&str> = args.splitn(2, '"').collect();
    if parts.len() >= 2 {
        // Crude: find two quoted strings.
        let mut strings: Vec<String> = Vec::new();
        let mut in_quote = false;
        let mut current = String::new();
        for ch in args.chars() {
            if ch == '"' || ch == '\'' {
                if in_quote {
                    strings.push(current.clone());
                    current.clear();
                    in_quote = false;
                } else {
                    in_quote = true;
                }
            } else if in_quote {
                current.push(ch);
            }
        }
        if strings.len() >= 2 {
            let lhs = expand_vars(&strings[0], db, auto_vars);
            let rhs = expand_vars(&strings[1], db, auto_vars);
            return lhs == rhs;
        }
    }
    false
}

// ---------------------------------------------------------------------------
// Pattern rule matching
// ---------------------------------------------------------------------------

/// If `pattern` is e.g. `%.o` and `target` is `foo.o`, return `Some("foo")`.
fn match_pattern(pattern: &str, target: &str) -> Option<String> {
    if let Some(pct) = pattern.find('%') {
        let prefix = &pattern[..pct];
        let suffix = &pattern[pct + 1..];
        if target.starts_with(prefix) && target.ends_with(suffix) {
            let stem_len = target.len() - prefix.len() - suffix.len();
            if stem_len > 0 || (prefix.len() + suffix.len() == target.len()) {
                let stem = &target[prefix.len()..target.len() - suffix.len()];
                return Some(stem.to_string());
            }
        }
    }
    None
}

/// Apply a stem to a pattern to produce a concrete name.
fn apply_stem(pattern: &str, stem: &str) -> String {
    pattern.replacen('%', stem, 1)
}

// ---------------------------------------------------------------------------
// Dependency resolution & DAG
// ---------------------------------------------------------------------------

/// Check whether `target` is up-to-date relative to its prerequisites.
fn is_up_to_date(target: &str, prereqs: &[String], phony: bool) -> bool {
    if phony {
        return false;
    }
    let target_mtime = match file_mtime(target) {
        Some(t) => t,
        None => return false, // target doesn't exist → must build
    };
    for p in prereqs {
        if let Some(p_mtime) = file_mtime(p)
            && p_mtime > target_mtime {
                return false;
            }
        // If the prereq doesn't exist as a file, we still consider it; the
        // build for it may create it.
    }
    true
}

/// Get a file's modification time.
fn file_mtime(path: &str) -> Option<SystemTime> {
    fs::metadata(path).ok().and_then(|m| m.modified().ok())
}

/// Find the rule (or synthesise one via pattern rules) that can build
/// `target`.  Returns `(prerequisites, recipe, is_phony)`.
fn find_rule_for(
    target: &str,
    db: &MakeDb,
) -> Option<(Vec<String>, Vec<String>, bool)> {
    // Concrete rules first.
    for rule in &db.rules {
        if rule.target == target {
            return Some((
                rule.prerequisites.clone(),
                rule.recipe.clone(),
                rule.phony || db.phony_targets.contains(target),
            ));
        }
    }
    // Pattern rules.
    for pr in &db.pattern_rules {
        if let Some(stem) = match_pattern(&pr.target_pat, target) {
            let prereqs: Vec<String> = pr
                .prereq_pats
                .iter()
                .map(|p| apply_stem(p, &stem))
                .collect();
            return Some((prereqs, pr.recipe.clone(), false));
        }
    }
    None
}

/// Build the full list of targets in dependency order (topological sort).
/// Returns the ordered list, or an error if there is a cycle.
fn topo_sort(
    goals: &[String],
    db: &MakeDb,
) -> Result<Vec<String>, String> {
    let mut order: Vec<String> = Vec::new();
    let mut visited: HashSet<String> = HashSet::new();
    let mut in_stack: HashSet<String> = HashSet::new();

    for goal in goals {
        topo_visit(goal, db, &mut order, &mut visited, &mut in_stack)?;
    }
    Ok(order)
}

fn topo_visit(
    target: &str,
    db: &MakeDb,
    order: &mut Vec<String>,
    visited: &mut HashSet<String>,
    in_stack: &mut HashSet<String>,
) -> Result<(), String> {
    if visited.contains(target) {
        return Ok(());
    }
    if in_stack.contains(target) {
        return Err(format!("circular dependency detected at `{}`", target));
    }
    in_stack.insert(target.to_string());

    if let Some((prereqs, _, _)) = find_rule_for(target, db) {
        for p in &prereqs {
            topo_visit(p, db, order, visited, in_stack)?;
        }
    }

    in_stack.remove(target);
    visited.insert(target.to_string());
    order.push(target.to_string());
    Ok(())
}

// ---------------------------------------------------------------------------
// Recipe execution
// ---------------------------------------------------------------------------

/// Parse prefix flags from the beginning of a recipe line.
fn parse_recipe_flags(line: &str) -> (RecipeFlags, &str) {
    let mut flags = RecipeFlags::default();
    let bytes = line.as_bytes();
    let mut start = 0;
    while start < bytes.len() {
        match bytes[start] {
            b'@' => flags.silent = true,
            b'-' => flags.ignore_error = true,
            b'+' => flags.force_exec = true,
            _ => break,
        }
        start += 1;
    }
    (flags, &line[start..])
}

/// Execute a single recipe line.  Returns `Ok(())` on success or error status.
fn exec_recipe_line(
    line: &str,
    opts: &Options,
    db: &MakeDb,
    auto_vars: &HashMap<String, String>,
) -> Result<(), i32> {
    let expanded = expand_vars(line, db, auto_vars);
    let (flags, cmd) = parse_recipe_flags(&expanded);

    let should_echo = !flags.silent && !opts.silent;
    let should_exec = !opts.dry_run || flags.force_exec;

    if should_echo {
        println!("{}", cmd);
    }

    if !should_exec {
        return Ok(());
    }

    let status = process::Command::new("/bin/sh")
        .arg("-c")
        .arg(cmd)
        .status();

    match status {
        Ok(s) if s.success() => Ok(()),
        Ok(s) => {
            let code = s.code().unwrap_or(1);
            if flags.ignore_error {
                eprintln!(
                    "make: [{}] Error {} (ignored)",
                    cmd, code
                );
                Ok(())
            } else {
                Err(code)
            }
        }
        Err(e) => {
            if flags.ignore_error {
                eprintln!("make: [{}] {}: (ignored)", cmd, e);
                Ok(())
            } else {
                eprintln!("make: [{}] {}", cmd, e);
                Err(1)
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Build driver
// ---------------------------------------------------------------------------

/// Run the build for the given goals.  Returns the exit code.
fn run_build(db: &MakeDb, opts: &Options) -> i32 {
    let goals = if opts.targets.is_empty() {
        match &db.default_target {
            Some(t) => vec![t.clone()],
            None => {
                eprintln!("make: *** No targets.  Stop.");
                return 2;
            }
        }
    } else {
        opts.targets.clone()
    };

    let build_order = match topo_sort(&goals, db) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("make: *** {}.  Stop.", e);
            return 2;
        }
    };

    let mut any_failure = false;

    for target in &build_order {
        let (prereqs, recipe, phony) = match find_rule_for(target, db) {
            Some(r) => r,
            None => {
                // No rule — if the file exists, it's a leaf.
                if Path::new(target).exists() {
                    continue;
                }
                eprintln!(
                    "make: *** No rule to make target `{}`.  Stop.",
                    target
                );
                if opts.keep_going {
                    any_failure = true;
                    continue;
                }
                return 2;
            }
        };

        // Expand prerequisites.
        let expanded_prereqs: Vec<String> = prereqs
            .iter()
            .map(|p| expand_vars(p, db, &HashMap::new()))
            .collect();

        let up_to_date = !opts.always_make
            && is_up_to_date(target, &expanded_prereqs, phony);

        if opts.question_mode {
            if !up_to_date {
                return 1;
            }
            continue;
        }

        if up_to_date {
            continue;
        }

        // Compute newer prerequisites for `$?`.
        let target_mtime = file_mtime(target);
        let newer: Vec<String> = expanded_prereqs
            .iter()
            .filter(|p| {
                if let (Some(tm), Some(pm)) = (target_mtime, file_mtime(p)) {
                    pm > tm
                } else {
                    true
                }
            })
            .cloned()
            .collect();

        // Build automatic variable map.
        let mut auto_vars = HashMap::new();
        auto_vars.insert("@".to_string(), target.clone());
        auto_vars.insert(
            "<".to_string(),
            expanded_prereqs.first().cloned().unwrap_or_default(),
        );
        auto_vars.insert("^".to_string(), expanded_prereqs.join(" "));
        auto_vars.insert("?".to_string(), newer.join(" "));
        // Stem — extract if this came from a pattern rule.
        let stem = db
            .pattern_rules
            .iter()
            .find_map(|pr| match_pattern(&pr.target_pat, target));
        auto_vars.insert("*".to_string(), stem.unwrap_or_default());

        for recipe_line in &recipe {
            if let Err(code) = exec_recipe_line(recipe_line, opts, db, &auto_vars) {
                eprintln!(
                    "make: *** [{}] Error {}",
                    target, code
                );
                if opts.keep_going {
                    any_failure = true;
                    break;
                }
                return code;
            }
        }
    }

    if any_failure {
        2
    } else {
        0
    }
}

// ---------------------------------------------------------------------------
// Print database (-p)
// ---------------------------------------------------------------------------

fn print_database(db: &MakeDb) {
    println!("# Variables");
    let mut names: Vec<&String> = db.variables.keys().collect();
    names.sort();
    for name in &names {
        if let Some(var) = db.variables.get(*name) {
            let flav = match var.flavour {
                VarFlavour::Recursive => "=",
                VarFlavour::Simple => ":=",
            };
            println!("{} {} {}", name, flav, var.value);
        }
    }
    println!();
    println!("# Rules");
    for rule in &db.rules {
        let phony_mark = if rule.phony { " (phony)" } else { "" };
        println!(
            "{}: {}{}",
            rule.target,
            rule.prerequisites.join(" "),
            phony_mark
        );
        for line in &rule.recipe {
            println!("\t{}", line);
        }
    }
    println!();
    println!("# Pattern Rules");
    for pr in &db.pattern_rules {
        println!("{}: {}", pr.target_pat, pr.prereq_pats.join(" "));
        for line in &pr.recipe {
            println!("\t{}", line);
        }
    }
}

// ---------------------------------------------------------------------------
// Built-in defaults
// ---------------------------------------------------------------------------

fn populate_defaults(db: &mut MakeDb) {
    let defaults: &[(&str, &str, VarFlavour)] = &[
        ("CC", "cc", VarFlavour::Recursive),
        ("CXX", "c++", VarFlavour::Recursive),
        ("CFLAGS", "", VarFlavour::Recursive),
        ("CXXFLAGS", "", VarFlavour::Recursive),
        ("MAKE", "make", VarFlavour::Recursive),
        ("SHELL", "/bin/sh", VarFlavour::Recursive),
    ];
    for &(name, val, flav) in defaults {
        if !db.variables.contains_key(name) {
            db.variables.insert(
                name.to_string(),
                Variable {
                    value: val.to_string(),
                    flavour: flav,
                    origin: VarOrigin::Default,
                },
            );
        }
    }

    // Import environment variables (lower priority than file assignments).
    for (key, val) in env::vars() {
        db.variables.entry(key).or_insert(Variable {
                    value: val,
                    flavour: VarFlavour::Recursive,
                    origin: VarOrigin::Environment,
                });
    }

    // Built-in pattern rules.
    db.pattern_rules.push(PatternRule {
        target_pat: "%.o".into(),
        prereq_pats: vec!["%.c".into()],
        recipe: vec!["$(CC) $(CFLAGS) -c -o $@ $<".into()],
    });
    db.pattern_rules.push(PatternRule {
        target_pat: "%.o".into(),
        prereq_pats: vec!["%.cpp".into()],
        recipe: vec!["$(CXX) $(CXXFLAGS) -c -o $@ $<".into()],
    });
}

// ---------------------------------------------------------------------------
// Locate makefile
// ---------------------------------------------------------------------------

fn find_makefile(opts: &Options) -> Option<PathBuf> {
    if let Some(ref f) = opts.makefile {
        return Some(PathBuf::from(f));
    }
    for name in &["Makefile", "makefile"] {
        let p = Path::new(name);
        if p.exists() {
            return Some(p.to_path_buf());
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

fn main() {
    let all_args: Vec<String> = env::args().collect();
    let (opts, overrides) = parse_args(&all_args[1..]);

    // Change directory if requested.
    if let Some(ref dir) = opts.directory
        && let Err(e) = env::set_current_dir(dir) {
            eprintln!("make: *** {}: {}.  Stop.", dir, e);
            process::exit(2);
        }

    let mf_path = match find_makefile(&opts) {
        Some(p) => p,
        None => {
            eprintln!(
                "make: *** No makefile found (Makefile, makefile).  Stop."
            );
            process::exit(2);
        }
    };

    let mut db = MakeDb::new();
    populate_defaults(&mut db);

    // Apply command-line variable overrides *before* parsing (highest priority).
    for (name, val) in &overrides {
        db.variables.insert(
            name.clone(),
            Variable {
                value: val.clone(),
                flavour: VarFlavour::Simple,
                origin: VarOrigin::CommandLine,
            },
        );
    }

    let mut included = HashSet::new();
    if let Err(e) = parse_makefile(&mf_path, &mut db, &mut included, &overrides) {
        eprintln!("make: {}: {}", mf_path.display(), e);
        process::exit(2);
    }

    if opts.print_database {
        print_database(&db);
        return;
    }

    let code = run_build(&db, &opts);
    process::exit(code);
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // Variable parsing
    // -----------------------------------------------------------------------

    #[test]
    fn parse_recursive_var() {
        let r = try_parse_variable("CC = gcc");
        assert!(r.is_some());
        let (name, val, flav) = r.unwrap();
        assert_eq!(name, "CC");
        assert_eq!(val, "gcc");
        assert_eq!(flav, VarFlavour::Recursive);
    }

    #[test]
    fn parse_simple_var() {
        let r = try_parse_variable("OBJS := foo.o bar.o");
        assert!(r.is_some());
        let (name, val, flav) = r.unwrap();
        assert_eq!(name, "OBJS");
        assert_eq!(val, "foo.o bar.o");
        assert_eq!(flav, VarFlavour::Simple);
    }

    #[test]
    fn parse_conditional_var() {
        let r = try_parse_variable("CC ?= gcc");
        assert!(r.is_some());
        let (name, val, _flav) = r.unwrap();
        assert_eq!(name, "?CC");
        assert_eq!(val, "gcc");
    }

    #[test]
    fn parse_append_var() {
        let r = try_parse_variable("CFLAGS += -Wall");
        assert!(r.is_some());
        let (name, val, _flav) = r.unwrap();
        assert_eq!(name, "+CFLAGS");
        assert_eq!(val, "-Wall");
    }

    #[test]
    fn parse_var_empty_value() {
        let r = try_parse_variable("EMPTY =");
        assert!(r.is_some());
        let (name, val, flav) = r.unwrap();
        assert_eq!(name, "EMPTY");
        assert_eq!(val, "");
        assert_eq!(flav, VarFlavour::Recursive);
    }

    #[test]
    fn parse_not_a_var() {
        assert!(try_parse_variable("all: main.o").is_none());
    }

    // -----------------------------------------------------------------------
    // Variable expansion
    // -----------------------------------------------------------------------

    #[test]
    fn expand_simple_var() {
        let mut db = MakeDb::new();
        db.variables.insert(
            "CC".into(),
            Variable {
                value: "gcc".into(),
                flavour: VarFlavour::Simple,
                origin: VarOrigin::File,
            },
        );
        let out = expand_vars("$(CC) -o foo", &db, &HashMap::new());
        assert_eq!(out, "gcc -o foo");
    }

    #[test]
    fn expand_curly_brace_var() {
        let mut db = MakeDb::new();
        db.variables.insert(
            "X".into(),
            Variable {
                value: "hello".into(),
                flavour: VarFlavour::Simple,
                origin: VarOrigin::File,
            },
        );
        let out = expand_vars("${X} world", &db, &HashMap::new());
        assert_eq!(out, "hello world");
    }

    #[test]
    fn expand_nested_var() {
        let mut db = MakeDb::new();
        db.variables.insert(
            "inner".into(),
            Variable {
                value: "CC".into(),
                flavour: VarFlavour::Simple,
                origin: VarOrigin::File,
            },
        );
        db.variables.insert(
            "CC".into(),
            Variable {
                value: "gcc".into(),
                flavour: VarFlavour::Simple,
                origin: VarOrigin::File,
            },
        );
        let out = expand_vars("$($(inner))", &db, &HashMap::new());
        assert_eq!(out, "gcc");
    }

    #[test]
    fn expand_recursive_var() {
        let mut db = MakeDb::new();
        db.variables.insert(
            "A".into(),
            Variable {
                value: "$(B)".into(),
                flavour: VarFlavour::Recursive,
                origin: VarOrigin::File,
            },
        );
        db.variables.insert(
            "B".into(),
            Variable {
                value: "hello".into(),
                flavour: VarFlavour::Simple,
                origin: VarOrigin::File,
            },
        );
        let out = expand_vars("$(A)", &db, &HashMap::new());
        assert_eq!(out, "hello");
    }

    #[test]
    fn expand_undefined_var_empty() {
        let db = MakeDb::new();
        let out = expand_vars("$(UNDEF)", &db, &HashMap::new());
        assert_eq!(out, "");
    }

    #[test]
    fn expand_dollar_dollar() {
        let db = MakeDb::new();
        let out = expand_vars("cost is $$5", &db, &HashMap::new());
        assert_eq!(out, "cost is $5");
    }

    #[test]
    fn expand_auto_var_at() {
        let db = MakeDb::new();
        let mut av = HashMap::new();
        av.insert("@".into(), "output.o".into());
        let out = expand_vars("building $@", &db, &av);
        assert_eq!(out, "building output.o");
    }

    #[test]
    fn expand_auto_var_less_than() {
        let db = MakeDb::new();
        let mut av = HashMap::new();
        av.insert("<".into(), "input.c".into());
        let out = expand_vars("$< is the source", &db, &av);
        assert_eq!(out, "input.c is the source");
    }

    #[test]
    fn expand_auto_var_caret() {
        let db = MakeDb::new();
        let mut av = HashMap::new();
        av.insert("^".into(), "a.o b.o c.o".into());
        let out = expand_vars("link $^", &db, &av);
        assert_eq!(out, "link a.o b.o c.o");
    }

    #[test]
    fn expand_auto_var_star() {
        let db = MakeDb::new();
        let mut av = HashMap::new();
        av.insert("*".into(), "main".into());
        let out = expand_vars("stem=$*", &db, &av);
        assert_eq!(out, "stem=main");
    }

    #[test]
    fn expand_auto_var_question() {
        let db = MakeDb::new();
        let mut av = HashMap::new();
        av.insert("?".into(), "new.o".into());
        let out = expand_vars("newer: $?", &db, &av);
        assert_eq!(out, "newer: new.o");
    }

    // -----------------------------------------------------------------------
    // Comment handling
    // -----------------------------------------------------------------------

    #[test]
    fn strip_comment_basic() {
        assert_eq!(strip_comment("hello # world"), "hello ");
    }

    #[test]
    fn strip_comment_no_comment() {
        assert_eq!(strip_comment("hello world"), "hello world");
    }

    #[test]
    fn strip_comment_escaped_hash() {
        assert_eq!(strip_comment("color = \\#red"), "color = \\#red");
    }

    // -----------------------------------------------------------------------
    // Line continuation
    // -----------------------------------------------------------------------

    #[test]
    fn line_continuation() {
        let input = "OBJS = foo.o \\\nbar.o \\\nbaz.o";
        let lines = read_logical_lines(input);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0], "OBJS = foo.o  bar.o  baz.o");
    }

    #[test]
    fn no_continuation() {
        let input = "line1\nline2\nline3";
        let lines = read_logical_lines(input);
        assert_eq!(lines.len(), 3);
    }

    // -----------------------------------------------------------------------
    // Rule parsing
    // -----------------------------------------------------------------------

    #[test]
    fn parse_single_target_rule() {
        let mut db = MakeDb::new();
        let lines = vec![
            "all: main.o utils.o".to_string(),
            "\tgcc -o all main.o utils.o".to_string(),
        ];
        let mut included = HashSet::new();
        parse_lines(&lines, &mut db, &mut included, &[], Path::new("Makefile"))
            .unwrap();
        assert_eq!(db.rules.len(), 1);
        assert_eq!(db.rules[0].target, "all");
        assert_eq!(
            db.rules[0].prerequisites,
            vec!["main.o".to_string(), "utils.o".to_string()]
        );
        assert_eq!(db.rules[0].recipe.len(), 1);
    }

    #[test]
    fn parse_rule_multiple_recipe_lines() {
        let mut db = MakeDb::new();
        let lines = vec![
            "build: src.c".to_string(),
            "\tgcc -c src.c".to_string(),
            "\tgcc -o build src.o".to_string(),
        ];
        let mut included = HashSet::new();
        parse_lines(&lines, &mut db, &mut included, &[], Path::new("Makefile"))
            .unwrap();
        assert_eq!(db.rules[0].recipe.len(), 2);
    }

    #[test]
    fn parse_rule_no_prereqs() {
        let mut db = MakeDb::new();
        let lines = vec![
            "clean:".to_string(),
            "\trm -f *.o".to_string(),
        ];
        let mut included = HashSet::new();
        parse_lines(&lines, &mut db, &mut included, &[], Path::new("Makefile"))
            .unwrap();
        assert_eq!(db.rules[0].target, "clean");
        assert!(db.rules[0].prerequisites.is_empty());
    }

    #[test]
    fn parse_multiple_targets() {
        let mut db = MakeDb::new();
        let lines = vec![
            "foo bar: baz".to_string(),
            "\techo done".to_string(),
        ];
        let mut included = HashSet::new();
        parse_lines(&lines, &mut db, &mut included, &[], Path::new("Makefile"))
            .unwrap();
        assert_eq!(db.rules.len(), 2);
        assert_eq!(db.rules[0].target, "foo");
        assert_eq!(db.rules[1].target, "bar");
    }

    // -----------------------------------------------------------------------
    // Pattern rules
    // -----------------------------------------------------------------------

    #[test]
    fn pattern_match_basic() {
        assert_eq!(
            match_pattern("%.o", "foo.o"),
            Some("foo".to_string())
        );
    }

    #[test]
    fn pattern_match_no_match() {
        assert_eq!(match_pattern("%.o", "foo.c"), None);
    }

    #[test]
    fn pattern_match_prefix() {
        assert_eq!(
            match_pattern("lib%.a", "libfoo.a"),
            Some("foo".to_string())
        );
    }

    #[test]
    fn pattern_apply_stem() {
        assert_eq!(apply_stem("%.o", "main"), "main.o");
        assert_eq!(apply_stem("lib%.a", "foo"), "libfoo.a");
    }

    #[test]
    fn parse_pattern_rule() {
        let mut db = MakeDb::new();
        let lines = vec![
            "%.o: %.c".to_string(),
            "\t$(CC) -c $< -o $@".to_string(),
        ];
        let mut included = HashSet::new();
        parse_lines(&lines, &mut db, &mut included, &[], Path::new("Makefile"))
            .unwrap();
        assert!(db.rules.is_empty());
        assert_eq!(db.pattern_rules.len(), 1);
        assert_eq!(db.pattern_rules[0].target_pat, "%.o");
        assert_eq!(db.pattern_rules[0].prereq_pats, vec!["%.c".to_string()]);
    }

    // -----------------------------------------------------------------------
    // Phony targets
    // -----------------------------------------------------------------------

    #[test]
    fn phony_target_registered() {
        let mut db = MakeDb::new();
        let lines = vec![
            ".PHONY: clean all".to_string(),
            "all:".to_string(),
            "\techo all".to_string(),
            "clean:".to_string(),
            "\trm -f *.o".to_string(),
        ];
        let mut included = HashSet::new();
        parse_lines(&lines, &mut db, &mut included, &[], Path::new("Makefile"))
            .unwrap();
        assert!(db.phony_targets.contains("clean"));
        assert!(db.phony_targets.contains("all"));
    }

    #[test]
    fn phony_always_out_of_date() {
        // Phony targets should never be considered up-to-date.
        assert!(!is_up_to_date("anything", &[], true));
    }

    // -----------------------------------------------------------------------
    // DAG / topological sort
    // -----------------------------------------------------------------------

    #[test]
    fn topo_sort_linear() {
        let mut db = MakeDb::new();
        db.rules.push(Rule {
            target: "a".into(),
            prerequisites: vec!["b".into()],
            recipe: vec![],
            phony: false,
        });
        db.rules.push(Rule {
            target: "b".into(),
            prerequisites: vec!["c".into()],
            recipe: vec![],
            phony: false,
        });
        db.rules.push(Rule {
            target: "c".into(),
            prerequisites: vec![],
            recipe: vec![],
            phony: false,
        });
        let order = topo_sort(&["a".into()], &db).unwrap();
        assert_eq!(order, vec!["c", "b", "a"]);
    }

    #[test]
    fn topo_sort_diamond() {
        let mut db = MakeDb::new();
        db.rules.push(Rule {
            target: "a".into(),
            prerequisites: vec!["b".into(), "c".into()],
            recipe: vec![],
            phony: false,
        });
        db.rules.push(Rule {
            target: "b".into(),
            prerequisites: vec!["d".into()],
            recipe: vec![],
            phony: false,
        });
        db.rules.push(Rule {
            target: "c".into(),
            prerequisites: vec!["d".into()],
            recipe: vec![],
            phony: false,
        });
        db.rules.push(Rule {
            target: "d".into(),
            prerequisites: vec![],
            recipe: vec![],
            phony: false,
        });
        let order = topo_sort(&["a".into()], &db).unwrap();
        // d must come before b and c; b and c before a.
        let pos = |t: &str| order.iter().position(|x| x == t).unwrap();
        assert!(pos("d") < pos("b"));
        assert!(pos("d") < pos("c"));
        assert!(pos("b") < pos("a"));
        assert!(pos("c") < pos("a"));
    }

    #[test]
    fn topo_sort_cycle_detected() {
        let mut db = MakeDb::new();
        db.rules.push(Rule {
            target: "a".into(),
            prerequisites: vec!["b".into()],
            recipe: vec![],
            phony: false,
        });
        db.rules.push(Rule {
            target: "b".into(),
            prerequisites: vec!["a".into()],
            recipe: vec![],
            phony: false,
        });
        let result = topo_sort(&["a".into()], &db);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .contains("circular dependency"));
    }

    #[test]
    fn topo_sort_self_cycle() {
        let mut db = MakeDb::new();
        db.rules.push(Rule {
            target: "a".into(),
            prerequisites: vec!["a".into()],
            recipe: vec![],
            phony: false,
        });
        let result = topo_sort(&["a".into()], &db);
        assert!(result.is_err());
    }

    // -----------------------------------------------------------------------
    // Up-to-date detection
    // -----------------------------------------------------------------------

    #[test]
    fn nonexistent_target_not_up_to_date() {
        assert!(!is_up_to_date(
            "/nonexistent/target/file",
            &[],
            false
        ));
    }

    #[test]
    fn target_exists_no_prereqs_is_up_to_date() {
        // Use the test binary itself as a file we know exists.
        let exe = env::current_exe().unwrap();
        let exe_str = exe.to_string_lossy().to_string();
        assert!(is_up_to_date(&exe_str, &[], false));
    }

    // -----------------------------------------------------------------------
    // Command prefix parsing
    // -----------------------------------------------------------------------

    #[test]
    fn prefix_silent() {
        let (flags, cmd) = parse_recipe_flags("@echo hello");
        assert!(flags.silent);
        assert!(!flags.ignore_error);
        assert!(!flags.force_exec);
        assert_eq!(cmd, "echo hello");
    }

    #[test]
    fn prefix_ignore_error() {
        let (flags, cmd) = parse_recipe_flags("-rm -f foo");
        assert!(flags.ignore_error);
        assert_eq!(cmd, "rm -f foo");
    }

    #[test]
    fn prefix_force_exec() {
        let (flags, cmd) = parse_recipe_flags("+$(MAKE) sub");
        assert!(flags.force_exec);
        assert_eq!(cmd, "$(MAKE) sub");
    }

    #[test]
    fn prefix_combined() {
        let (flags, cmd) = parse_recipe_flags("@-echo ignored");
        assert!(flags.silent);
        assert!(flags.ignore_error);
        assert_eq!(cmd, "echo ignored");
    }

    #[test]
    fn prefix_none() {
        let (flags, cmd) = parse_recipe_flags("gcc -o foo bar.c");
        assert!(!flags.silent);
        assert!(!flags.ignore_error);
        assert!(!flags.force_exec);
        assert_eq!(cmd, "gcc -o foo bar.c");
    }

    // -----------------------------------------------------------------------
    // Conditional directives
    // -----------------------------------------------------------------------

    #[test]
    fn ifeq_parens_equal() {
        let db = MakeDb::new();
        assert!(eval_ifeq("(foo,foo)", &db, &HashMap::new()));
    }

    #[test]
    fn ifeq_parens_not_equal() {
        let db = MakeDb::new();
        assert!(!eval_ifeq("(foo,bar)", &db, &HashMap::new()));
    }

    #[test]
    fn ifeq_quotes() {
        let db = MakeDb::new();
        assert!(eval_ifeq("\"abc\" \"abc\"", &db, &HashMap::new()));
    }

    #[test]
    fn ifeq_with_variable() {
        let mut db = MakeDb::new();
        db.variables.insert(
            "X".into(),
            Variable {
                value: "yes".into(),
                flavour: VarFlavour::Simple,
                origin: VarOrigin::File,
            },
        );
        assert!(eval_ifeq("($(X),yes)", &db, &HashMap::new()));
    }

    #[test]
    fn conditional_ifdef() {
        let mut db = MakeDb::new();
        db.variables.insert(
            "FOO".into(),
            Variable {
                value: "bar".into(),
                flavour: VarFlavour::Simple,
                origin: VarOrigin::File,
            },
        );
        let lines = vec![
            "ifdef FOO".to_string(),
            "RESULT = yes".to_string(),
            "else".to_string(),
            "RESULT = no".to_string(),
            "endif".to_string(),
        ];
        let mut included = HashSet::new();
        parse_lines(&lines, &mut db, &mut included, &[], Path::new("Makefile"))
            .unwrap();
        assert_eq!(db.variables.get("RESULT").unwrap().value, "yes");
    }

    #[test]
    fn conditional_ifndef() {
        let mut db = MakeDb::new();
        let lines = vec![
            "ifndef UNDEF".to_string(),
            "RESULT = yes".to_string(),
            "else".to_string(),
            "RESULT = no".to_string(),
            "endif".to_string(),
        ];
        let mut included = HashSet::new();
        parse_lines(&lines, &mut db, &mut included, &[], Path::new("Makefile"))
            .unwrap();
        assert_eq!(db.variables.get("RESULT").unwrap().value, "yes");
    }

    #[test]
    fn conditional_ifdef_false_branch() {
        let mut db = MakeDb::new();
        let lines = vec![
            "ifdef UNDEF".to_string(),
            "RESULT = yes".to_string(),
            "else".to_string(),
            "RESULT = no".to_string(),
            "endif".to_string(),
        ];
        let mut included = HashSet::new();
        parse_lines(&lines, &mut db, &mut included, &[], Path::new("Makefile"))
            .unwrap();
        assert_eq!(db.variables.get("RESULT").unwrap().value, "no");
    }

    #[test]
    fn conditional_ifeq_in_parse() {
        let mut db = MakeDb::new();
        db.variables.insert(
            "MODE".into(),
            Variable {
                value: "release".into(),
                flavour: VarFlavour::Simple,
                origin: VarOrigin::File,
            },
        );
        let lines = vec![
            "ifeq ($(MODE),release)".to_string(),
            "OPT = -O2".to_string(),
            "else".to_string(),
            "OPT = -O0".to_string(),
            "endif".to_string(),
        ];
        let mut included = HashSet::new();
        parse_lines(&lines, &mut db, &mut included, &[], Path::new("Makefile"))
            .unwrap();
        assert_eq!(db.variables.get("OPT").unwrap().value, "-O2");
    }

    #[test]
    fn conditional_ifneq_in_parse() {
        let mut db = MakeDb::new();
        db.variables.insert(
            "MODE".into(),
            Variable {
                value: "debug".into(),
                flavour: VarFlavour::Simple,
                origin: VarOrigin::File,
            },
        );
        let lines = vec![
            "ifneq ($(MODE),release)".to_string(),
            "OPT = -O0".to_string(),
            "else".to_string(),
            "OPT = -O2".to_string(),
            "endif".to_string(),
        ];
        let mut included = HashSet::new();
        parse_lines(&lines, &mut db, &mut included, &[], Path::new("Makefile"))
            .unwrap();
        assert_eq!(db.variables.get("OPT").unwrap().value, "-O0");
    }

    // -----------------------------------------------------------------------
    // Include directive
    // -----------------------------------------------------------------------

    #[test]
    fn include_prevents_infinite_loop() {
        // Simulating: if we try to include the same file twice, it should
        // not infinite loop.
        let mut db = MakeDb::new();
        let mut included = HashSet::new();
        // Pre-insert a path as already included.
        included.insert(PathBuf::from("/fake/Makefile"));
        // This should be a no-op since it's already included.
        let result = parse_makefile(
            Path::new("/fake/Makefile"),
            &mut db,
            &mut included,
            &[],
        );
        // It returns Ok because it short-circuits on the included set,
        // never actually reading the file.
        assert!(result.is_ok());
    }

    // -----------------------------------------------------------------------
    // Default target selection
    // -----------------------------------------------------------------------

    #[test]
    fn default_target_is_first() {
        let mut db = MakeDb::new();
        let lines = vec![
            "first: a b".to_string(),
            "second: c d".to_string(),
        ];
        let mut included = HashSet::new();
        parse_lines(&lines, &mut db, &mut included, &[], Path::new("Makefile"))
            .unwrap();
        assert_eq!(db.default_target, Some("first".to_string()));
    }

    #[test]
    fn default_target_skips_dot_prefix() {
        let mut db = MakeDb::new();
        let lines = vec![
            ".PHONY: all".to_string(),
            "all: main".to_string(),
        ];
        let mut included = HashSet::new();
        parse_lines(&lines, &mut db, &mut included, &[], Path::new("Makefile"))
            .unwrap();
        assert_eq!(db.default_target, Some("all".to_string()));
    }

    // -----------------------------------------------------------------------
    // Command-line argument parsing
    // -----------------------------------------------------------------------

    #[test]
    fn parse_args_defaults() {
        let (opts, over) = parse_args(&[]);
        assert!(opts.makefile.is_none());
        assert!(!opts.dry_run);
        assert!(!opts.keep_going);
        assert!(!opts.always_make);
        assert!(!opts.silent);
        assert_eq!(opts.jobs, 1);
        assert!(over.is_empty());
    }

    #[test]
    fn parse_args_makefile() {
        let args: Vec<String> =
            vec!["-f", "build.mk"]
                .into_iter()
                .map(String::from)
                .collect();
        let (opts, _) = parse_args(&args);
        assert_eq!(opts.makefile, Some("build.mk".to_string()));
    }

    #[test]
    fn parse_args_dry_run() {
        let args: Vec<String> = vec!["-n".to_string()];
        let (opts, _) = parse_args(&args);
        assert!(opts.dry_run);
    }

    #[test]
    fn parse_args_keep_going() {
        let args: Vec<String> = vec!["--keep-going".to_string()];
        let (opts, _) = parse_args(&args);
        assert!(opts.keep_going);
    }

    #[test]
    fn parse_args_always_make() {
        let args: Vec<String> = vec!["-B".to_string()];
        let (opts, _) = parse_args(&args);
        assert!(opts.always_make);
    }

    #[test]
    fn parse_args_silent() {
        let args: Vec<String> = vec!["-s".to_string()];
        let (opts, _) = parse_args(&args);
        assert!(opts.silent);
    }

    #[test]
    fn parse_args_jobs() {
        let args: Vec<String> =
            vec!["-j", "4"].into_iter().map(String::from).collect();
        let (opts, _) = parse_args(&args);
        assert_eq!(opts.jobs, 4);
    }

    #[test]
    fn parse_args_directory() {
        let args: Vec<String> =
            vec!["-C", "/tmp"].into_iter().map(String::from).collect();
        let (opts, _) = parse_args(&args);
        assert_eq!(opts.directory, Some("/tmp".to_string()));
    }

    #[test]
    fn parse_args_override() {
        let args: Vec<String> = vec!["CC=clang".to_string()];
        let (opts, over) = parse_args(&args);
        assert!(opts.targets.is_empty());
        assert_eq!(over.len(), 1);
        assert_eq!(over[0], ("CC".to_string(), "clang".to_string()));
    }

    #[test]
    fn parse_args_target() {
        let args: Vec<String> = vec!["clean".to_string()];
        let (opts, _) = parse_args(&args);
        assert_eq!(opts.targets, vec!["clean".to_string()]);
    }

    #[test]
    fn parse_args_mixed() {
        let args: Vec<String> = vec![
            "-f", "build.mk", "-n", "-k", "CC=clang", "all", "install",
        ]
        .into_iter()
        .map(String::from)
        .collect();
        let (opts, over) = parse_args(&args);
        assert_eq!(opts.makefile, Some("build.mk".to_string()));
        assert!(opts.dry_run);
        assert!(opts.keep_going);
        assert_eq!(opts.targets, vec!["all", "install"]);
        assert_eq!(over, vec![("CC".into(), "clang".into())]);
    }

    #[test]
    fn parse_args_print_database() {
        let args: Vec<String> = vec!["-p".to_string()];
        let (opts, _) = parse_args(&args);
        assert!(opts.print_database);
    }

    #[test]
    fn parse_args_question_mode() {
        let args: Vec<String> = vec!["-q".to_string()];
        let (opts, _) = parse_args(&args);
        assert!(opts.question_mode);
    }

    // -----------------------------------------------------------------------
    // Makefile search order
    // -----------------------------------------------------------------------

    #[test]
    fn find_makefile_explicit() {
        let opts = Options {
            makefile: Some("custom.mk".into()),
            ..Options::default()
        };
        let result = find_makefile(&opts);
        assert_eq!(result, Some(PathBuf::from("custom.mk")));
    }

    // -----------------------------------------------------------------------
    // Built-in defaults
    // -----------------------------------------------------------------------

    #[test]
    fn defaults_populated() {
        let mut db = MakeDb::new();
        populate_defaults(&mut db);
        assert_eq!(db.variables.get("CC").unwrap().value, "cc");
        assert_eq!(db.variables.get("CXX").unwrap().value, "c++");
        assert_eq!(db.variables.get("SHELL").unwrap().value, "/bin/sh");
        assert!(db.pattern_rules.len() >= 2);
    }

    // -----------------------------------------------------------------------
    // Variable application (conditional, append)
    // -----------------------------------------------------------------------

    #[test]
    fn apply_conditional_var_not_set() {
        let mut db = MakeDb::new();
        apply_variable(&mut db, "?CC", "gcc", VarFlavour::Recursive, VarOrigin::File);
        assert_eq!(db.variables.get("CC").unwrap().value, "gcc");
    }

    #[test]
    fn apply_conditional_var_already_set() {
        let mut db = MakeDb::new();
        db.variables.insert(
            "CC".into(),
            Variable {
                value: "clang".into(),
                flavour: VarFlavour::Simple,
                origin: VarOrigin::File,
            },
        );
        apply_variable(&mut db, "?CC", "gcc", VarFlavour::Recursive, VarOrigin::File);
        assert_eq!(db.variables.get("CC").unwrap().value, "clang");
    }

    #[test]
    fn apply_append_var_empty() {
        let mut db = MakeDb::new();
        apply_variable(&mut db, "+CFLAGS", "-Wall", VarFlavour::Recursive, VarOrigin::File);
        assert_eq!(db.variables.get("CFLAGS").unwrap().value, "-Wall");
    }

    #[test]
    fn apply_append_var_existing() {
        let mut db = MakeDb::new();
        db.variables.insert(
            "CFLAGS".into(),
            Variable {
                value: "-Wall".into(),
                flavour: VarFlavour::Recursive,
                origin: VarOrigin::File,
            },
        );
        apply_variable(&mut db, "+CFLAGS", "-Werror", VarFlavour::Recursive, VarOrigin::File);
        assert_eq!(db.variables.get("CFLAGS").unwrap().value, "-Wall -Werror");
    }

    // -----------------------------------------------------------------------
    // find_rule_for + pattern rule fallback
    // -----------------------------------------------------------------------

    #[test]
    fn find_concrete_rule() {
        let mut db = MakeDb::new();
        db.rules.push(Rule {
            target: "main.o".into(),
            prerequisites: vec!["main.c".into()],
            recipe: vec!["gcc -c main.c".into()],
            phony: false,
        });
        let r = find_rule_for("main.o", &db);
        assert!(r.is_some());
        let (prereqs, recipe, phony) = r.unwrap();
        assert_eq!(prereqs, vec!["main.c".to_string()]);
        assert_eq!(recipe, vec!["gcc -c main.c".to_string()]);
        assert!(!phony);
    }

    #[test]
    fn find_pattern_rule_fallback() {
        let mut db = MakeDb::new();
        db.pattern_rules.push(PatternRule {
            target_pat: "%.o".into(),
            prereq_pats: vec!["%.c".into()],
            recipe: vec!["$(CC) -c $< -o $@".into()],
        });
        let r = find_rule_for("foo.o", &db);
        assert!(r.is_some());
        let (prereqs, _, _) = r.unwrap();
        assert_eq!(prereqs, vec!["foo.c".to_string()]);
    }

    #[test]
    fn find_no_rule() {
        let db = MakeDb::new();
        let r = find_rule_for("nonexistent", &db);
        assert!(r.is_none());
    }

    // -----------------------------------------------------------------------
    // is_var_name validation
    // -----------------------------------------------------------------------

    #[test]
    fn var_name_valid() {
        assert!(is_var_name("CC"));
        assert!(is_var_name("CFLAGS"));
        assert!(is_var_name("my_var_2"));
        assert!(is_var_name("MAKEFILE_LIST"));
    }

    #[test]
    fn var_name_invalid() {
        assert!(!is_var_name("has space"));
        assert!(!is_var_name("has-dash"));
    }

    // -----------------------------------------------------------------------
    // Rule colon detection
    // -----------------------------------------------------------------------

    #[test]
    fn find_rule_colon_basic() {
        assert_eq!(find_rule_colon("all: main.o"), Some(3));
    }

    #[test]
    fn find_rule_colon_skip_assign() {
        assert_eq!(find_rule_colon("CC := gcc"), None);
    }

    #[test]
    fn find_rule_colon_no_colon() {
        assert_eq!(find_rule_colon("nothing here"), None);
    }

    // -----------------------------------------------------------------------
    // Command-line override in parse context
    // -----------------------------------------------------------------------

    #[test]
    fn cmdline_override_prevents_file_var() {
        let mut db = MakeDb::new();
        let overrides = vec![("CC".to_string(), "clang".to_string())];
        db.variables.insert(
            "CC".into(),
            Variable {
                value: "clang".into(),
                flavour: VarFlavour::Simple,
                origin: VarOrigin::CommandLine,
            },
        );
        let lines = vec!["CC = gcc".to_string()];
        let mut included = HashSet::new();
        parse_lines(
            &lines,
            &mut db,
            &mut included,
            &overrides,
            Path::new("Makefile"),
        )
        .unwrap();
        // The command-line value should still be in effect.
        assert_eq!(db.variables.get("CC").unwrap().value, "clang");
    }

    // -----------------------------------------------------------------------
    // Full mini-makefile parse integration
    // -----------------------------------------------------------------------

    #[test]
    fn full_parse_small_makefile() {
        let mut db = MakeDb::new();
        let lines = vec![
            "CC = gcc".to_string(),
            "CFLAGS = -Wall".to_string(),
            "".to_string(),
            ".PHONY: all clean".to_string(),
            "".to_string(),
            "all: hello".to_string(),
            "".to_string(),
            "hello: hello.o".to_string(),
            "\t$(CC) $(CFLAGS) -o $@ $^".to_string(),
            "".to_string(),
            "hello.o: hello.c".to_string(),
            "\t$(CC) $(CFLAGS) -c -o $@ $<".to_string(),
            "".to_string(),
            "clean:".to_string(),
            "\trm -f hello hello.o".to_string(),
        ];
        let mut included = HashSet::new();
        parse_lines(&lines, &mut db, &mut included, &[], Path::new("Makefile"))
            .unwrap();

        assert_eq!(db.variables.get("CC").unwrap().value, "gcc");
        assert_eq!(db.variables.get("CFLAGS").unwrap().value, "-Wall");
        assert!(db.phony_targets.contains("all"));
        assert!(db.phony_targets.contains("clean"));
        assert_eq!(db.default_target, Some("all".to_string()));

        // Rules: all, hello, hello.o, clean = 4 rules
        assert_eq!(db.rules.len(), 4);
    }
}
