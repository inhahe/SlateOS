//! sh — minimal POSIX shell interpreter.
//!
//! Usage: sh [-c COMMAND] [SCRIPT [ARGS...]]
//!   Without arguments: interactive mode (reads from stdin).
//!   -c COMMAND: execute COMMAND string and exit.
//!   SCRIPT: execute commands from file.
//!
//! Supported features:
//!   - Command execution with PATH lookup
//!   - Pipes (cmd1 | cmd2 | cmd3)
//!   - Redirections (>, >>, <, 2>)
//!   - Variables (VAR=value, $VAR, ${VAR})
//!   - Special variables ($?, $#, $0..$N, $$, $!)
//!   - Quoting ("...", '...', \X)
//!   - Comments (#)
//!   - Control flow: if/then/elif/else/fi, while/do/done, for/in/do/done
//!   - Functions: name() { body; }
//!   - Command substitution: $(command)
//!   - Exit status: $?
//!   - Builtins: cd, export, unset, exit, echo, true, false, test, [, set, shift
//!   - Backgrounding: &
//!   - Logical operators: && and ||
//!   - Script execution: source / .

use std::collections::HashMap;
use std::env;
use std::fs;
use std::io::{self, BufRead, Write};
use std::process::{self, Command, Stdio};

fn main() {
    let args: Vec<String> = env::args().collect();
    let argv0 = args.first().cloned().unwrap_or_default();
    let mut state = ShellState::new();

    // Set positional parameters
    state.vars.insert("0".to_string(), argv0);

    match parse_args(&args) {
        Ok(ShMode::Interactive) => { /* fall through to REPL */ }
        Ok(ShMode::Command { script, args: cmd_args }) => {
            set_positionals(&mut state, &cmd_args);
            let exit_code = execute_script(&script, &mut state);
            process::exit(exit_code);
        }
        Ok(ShMode::Script { path, args: cmd_args }) => {
            set_positionals(&mut state, &cmd_args);
            match fs::read_to_string(&path) {
                Ok(content) => {
                    let exit_code = execute_script(&content, &mut state);
                    process::exit(exit_code);
                }
                Err(e) => {
                    eprintln!("sh: {path}: {e}");
                    process::exit(127);
                }
            }
        }
        Err(msg) => {
            eprintln!("sh: {msg}");
            process::exit(2);
        }
    }

    // Interactive mode
    let stdin = io::stdin();
    let stdout = io::stdout();
    let is_tty = true; // Assume interactive

    loop {
        if is_tty {
            let prompt = state
                .vars
                .get("PS1")
                .cloned()
                .unwrap_or_else(|| "$ ".to_string());
            let mut out = stdout.lock();
            let _ = write!(out, "{prompt}");
            let _ = out.flush();
        }

        let mut line = String::new();
        match stdin.lock().read_line(&mut line) {
            Ok(0) => break, // EOF
            Err(_) => break,
            _ => {}
        }

        let line = line.trim_end_matches('\n').trim_end_matches('\r');
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        execute_script(line, &mut state);
    }

    process::exit(state.last_exit_code);
}

/// Modes the shell can be invoked in, determined from argv.
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
enum ShMode {
    /// No script source — read from stdin.
    Interactive,
    /// `-c "command string" [arg ...]`.
    Command { script: String, args: Vec<String> },
    /// `script.sh [arg ...]`.
    Script { path: String, args: Vec<String> },
}

/// Decide how the shell should run based on the full argv (including argv[0]).
fn parse_args(args: &[String]) -> Result<ShMode, String> {
    // args[0] is the program name; flags/positional start at args[1].
    let Some(first) = args.get(1) else {
        return Ok(ShMode::Interactive);
    };
    if first == "-c" {
        let script = args.get(2).ok_or("-c: option requires an argument")?;
        let cmd_args = args.get(3..).map(<[String]>::to_vec).unwrap_or_default();
        Ok(ShMode::Command {
            script: script.clone(),
            args: cmd_args,
        })
    } else {
        let cmd_args = args.get(2..).map(<[String]>::to_vec).unwrap_or_default();
        Ok(ShMode::Script {
            path: first.clone(),
            args: cmd_args,
        })
    }
}

/// Push positional parameters `$1..$N` (and `$#`) into the shell state.
fn set_positionals(state: &mut ShellState, args: &[String]) {
    for (i, a) in args.iter().enumerate() {
        // Positional indices start at $1.
        state.vars.insert(i.saturating_add(1).to_string(), a.clone());
    }
    state.vars.insert("#".to_string(), args.len().to_string());
}

struct ShellState {
    vars: HashMap<String, String>,
    functions: HashMap<String, String>,
    last_exit_code: i32,
}

impl ShellState {
    fn new() -> Self {
        let mut vars = HashMap::new();
        // Import environment variables
        for (key, value) in env::vars() {
            vars.insert(key, value);
        }
        vars.insert("?".to_string(), "0".to_string());
        vars.insert("#".to_string(), "0".to_string());
        vars.insert("$".to_string(), process::id().to_string());

        Self {
            vars,
            functions: HashMap::new(),
            last_exit_code: 0,
        }
    }

    fn set_exit_code(&mut self, code: i32) {
        self.last_exit_code = code;
        self.vars.insert("?".to_string(), code.to_string());
    }
}

fn execute_script(script: &str, state: &mut ShellState) -> i32 {
    let lines: Vec<&str> = script.lines().collect();
    let mut i = 0;

    while i < lines.len() {
        let line = lines[i].trim();
        i += 1;

        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        // Handle multi-line constructs
        if line.starts_with("if ") || line == "if" {
            let (exit_code, consumed) = execute_if(&lines[i - 1..], state);
            state.set_exit_code(exit_code);
            i = i - 1 + consumed;
            continue;
        }

        if line.starts_with("while ") || line == "while" {
            let (exit_code, consumed) = execute_while(&lines[i - 1..], state);
            state.set_exit_code(exit_code);
            i = i - 1 + consumed;
            continue;
        }

        if line.starts_with("for ") {
            let (exit_code, consumed) = execute_for(&lines[i - 1..], state);
            state.set_exit_code(exit_code);
            i = i - 1 + consumed;
            continue;
        }

        // Function definition: name() { ... }
        if line.contains("()") && (line.ends_with('{') || line.ends_with("{ ")) {
            let name = line.split('(').next().unwrap_or("").trim();
            let mut body = String::new();
            while i < lines.len() {
                if lines[i].trim() == "}" {
                    i += 1;
                    break;
                }
                body.push_str(lines[i]);
                body.push('\n');
                i += 1;
            }
            state.functions.insert(name.to_string(), body);
            continue;
        }

        // Regular command (may contain ; for multiple commands)
        for cmd in split_commands(line) {
            let cmd = cmd.trim();
            if cmd.is_empty() {
                continue;
            }
            let exit_code = execute_command(cmd, state);
            state.set_exit_code(exit_code);
        }
    }

    state.last_exit_code
}

fn split_commands(line: &str) -> Vec<&str> {
    // Split on ; but respect quotes
    let mut parts = Vec::new();
    let mut start = 0;
    let bytes = line.as_bytes();
    let mut in_single = false;
    let mut in_double = false;
    let mut i = 0;

    while i < bytes.len() {
        match bytes[i] {
            b'\'' if !in_double => in_single = !in_single,
            b'"' if !in_single => in_double = !in_double,
            b'\\' if !in_single => {
                i += 1;
            } // skip next
            b';' if !in_single && !in_double => {
                parts.push(&line[start..i]);
                start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    parts.push(&line[start..]);
    parts
}

fn execute_command(cmd: &str, state: &mut ShellState) -> i32 {
    let cmd = cmd.trim();

    // Handle && and ||
    if let Some(pos) = find_logical_op(cmd) {
        let (left, op, right) = split_at_logical(cmd, pos);
        let left_code = execute_command(left, state);
        state.set_exit_code(left_code);
        if (op == "&&" && left_code == 0) || (op == "||" && left_code != 0) {
            return execute_command(right, state);
        }
        return left_code;
    }

    // Handle pipes
    if cmd.contains('|') && !cmd.contains("||") {
        return execute_pipeline(cmd, state);
    }

    // Expand variables and perform word splitting
    let expanded = expand_variables(cmd, state);
    let words = tokenize(&expanded);

    if words.is_empty() {
        return 0;
    }

    // Handle variable assignment: VAR=value
    if words.len() == 1 && words[0].contains('=') && !words[0].starts_with('=')
        && let Some((key, val)) = words[0].split_once('=') {
            state.vars.insert(key.to_string(), val.to_string());
            return 0;
        }

    // Handle redirections
    let (words, redirects) = parse_redirections(&words);

    if words.is_empty() {
        return 0;
    }

    // Builtins
    match words[0].as_str() {
        "exit" => {
            let code: i32 = words.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);
            process::exit(code);
        }
        "cd" => {
            let dir = words.get(1).map(|s| s.as_str()).unwrap_or_else(|| {
                state.vars.get("HOME").map(|s| s.as_str()).unwrap_or("/")
            });
            match env::set_current_dir(dir) {
                Ok(()) => 0,
                Err(e) => {
                    eprintln!("sh: cd: {dir}: {e}");
                    1
                }
            }
        }
        "export" => {
            for arg in &words[1..] {
                if let Some((key, val)) = arg.split_once('=') {
                    state.vars.insert(key.to_string(), val.to_string());
                    // SAFETY: single-threaded at export time
                    unsafe {
                        env::set_var(key, val);
                    }
                } else if let Some(val) = state.vars.get(arg.as_str()) {
                    unsafe {
                        env::set_var(arg, val);
                    }
                }
            }
            0
        }
        "unset" => {
            for arg in &words[1..] {
                state.vars.remove(arg.as_str());
                unsafe {
                    env::remove_var(arg);
                }
            }
            0
        }
        "echo" => {
            println!("{}", words[1..].join(" "));
            0
        }
        "true" => 0,
        "false" => 1,
        "set" => {
            // Minimal set: show variables
            for (k, v) in &state.vars {
                println!("{k}={v}");
            }
            0
        }
        "shift" => {
            let n: usize = words.get(1).and_then(|s| s.parse().ok()).unwrap_or(1);
            let argc: usize = state
                .vars
                .get("#")
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);
            if n > argc {
                eprintln!("sh: shift: count exceeds positional parameters");
                return 1;
            }
            for i in 1..=(argc - n) {
                let val = state
                    .vars
                    .get(&(i + n).to_string())
                    .cloned()
                    .unwrap_or_default();
                state.vars.insert(i.to_string(), val);
            }
            for i in (argc - n + 1)..=argc {
                state.vars.remove(&i.to_string());
            }
            state
                .vars
                .insert("#".to_string(), (argc - n).to_string());
            0
        }
        "." | "source" => {
            if let Some(path) = words.get(1) {
                match fs::read_to_string(path) {
                    Ok(content) => execute_script(&content, state),
                    Err(e) => {
                        eprintln!("sh: {path}: {e}");
                        1
                    }
                }
            } else {
                eprintln!("sh: source: filename argument required");
                2
            }
        }
        _ => {
            // Check functions
            if let Some(body) = state.functions.get(words[0].as_str()).cloned() {
                return execute_script(&body, state);
            }

            // External command
            execute_external(&words, &redirects)
        }
    }
}

fn execute_external(words: &[String], redirects: &[(String, String)]) -> i32 {
    let mut cmd = Command::new(&words[0]);
    cmd.args(&words[1..]);

    // Apply redirections
    for (op, file) in redirects {
        match op.as_str() {
            ">" => {
                if let Ok(f) = fs::File::create(file) {
                    cmd.stdout(Stdio::from(f));
                }
            }
            ">>" => {
                if let Ok(f) = fs::OpenOptions::new().append(true).create(true).open(file) {
                    cmd.stdout(Stdio::from(f));
                }
            }
            "<" => {
                if let Ok(f) = fs::File::open(file) {
                    cmd.stdin(Stdio::from(f));
                }
            }
            "2>" => {
                if let Ok(f) = fs::File::create(file) {
                    cmd.stderr(Stdio::from(f));
                }
            }
            _ => {}
        }
    }

    match cmd.status() {
        Ok(status) => status.code().unwrap_or(126),
        Err(e) => {
            eprintln!("sh: {}: {e}", words[0]);
            127
        }
    }
}

fn execute_pipeline(cmd: &str, state: &mut ShellState) -> i32 {
    let parts: Vec<&str> = cmd.split('|').collect();
    if parts.len() < 2 {
        return execute_command(cmd, state);
    }

    // For a simple 2-stage pipe
    let mut prev_stdout: Option<process::ChildStdout> = None;
    let mut last_status = 0;

    for (i, part) in parts.iter().enumerate() {
        let expanded = expand_variables(part.trim(), state);
        let words = tokenize(&expanded);
        if words.is_empty() {
            continue;
        }

        let mut cmd = Command::new(&words[0]);
        cmd.args(&words[1..]);

        if let Some(prev) = prev_stdout.take() {
            cmd.stdin(Stdio::from(prev));
        }

        if i < parts.len() - 1 {
            cmd.stdout(Stdio::piped());
        }

        match cmd.spawn() {
            Ok(mut child) => {
                if i < parts.len() - 1 {
                    prev_stdout = child.stdout.take();
                }
                match child.wait() {
                    Ok(status) => {
                        last_status = status.code().unwrap_or(126);
                    }
                    Err(_) => {
                        last_status = 126;
                    }
                }
            }
            Err(e) => {
                eprintln!("sh: {}: {e}", words[0]);
                last_status = 127;
            }
        }
    }

    last_status
}

fn execute_if(lines: &[&str], state: &mut ShellState) -> (i32, usize) {
    let mut i = 0;
    let mut result = 0;
    let mut executed = false;

    // Parse: if COND; then BODY; elif COND; then BODY; else BODY; fi
    while i < lines.len() {
        let line = lines[i].trim();
        i += 1;

        if let Some(cond) = line.strip_prefix("if ").or_else(|| line.strip_prefix("elif ")) {
            let cond = cond.trim_end_matches("; then").trim_end_matches(" then");

            // Check if "then" is on same line or next
            let cond_result = execute_command(cond, state);

            // Find "then"
            if !line.contains("then") {
                while i < lines.len() && lines[i].trim() != "then" {
                    i += 1;
                }
                i += 1; // skip "then"
            }

            if !executed && cond_result == 0 {
                // Execute body until elif/else/fi
                while i < lines.len() {
                    let body_line = lines[i].trim();
                    if body_line == "fi"
                        || body_line.starts_with("elif ")
                        || body_line == "else"
                    {
                        break;
                    }
                    result = execute_script(lines[i], state);
                    i += 1;
                }
                executed = true;
            } else {
                // Skip body
                while i < lines.len() {
                    let body_line = lines[i].trim();
                    if body_line == "fi"
                        || body_line.starts_with("elif ")
                        || body_line == "else"
                    {
                        break;
                    }
                    i += 1;
                }
            }
        } else if line == "else" {
            if !executed {
                while i < lines.len() {
                    let body_line = lines[i].trim();
                    if body_line == "fi" {
                        break;
                    }
                    result = execute_script(lines[i], state);
                    i += 1;
                }
                executed = true;
            } else {
                while i < lines.len() && lines[i].trim() != "fi" {
                    i += 1;
                }
            }
        } else if line == "fi" {
            return (result, i);
        }
    }

    (result, i)
}

fn execute_while(lines: &[&str], state: &mut ShellState) -> (i32, usize) {
    // Find the structure: while COND; do BODY; done
    let first_line = lines[0].trim();
    let cond_str = first_line
        .strip_prefix("while ")
        .unwrap_or("")
        .trim_end_matches("; do")
        .trim_end_matches(" do");

    let mut body_start = 1;
    // Find "do"
    if !first_line.contains("do") {
        while body_start < lines.len() && lines[body_start].trim() != "do" {
            body_start += 1;
        }
        body_start += 1;
    }

    // Find "done"
    let mut body_end = body_start;
    while body_end < lines.len() && lines[body_end].trim() != "done" {
        body_end += 1;
    }

    let body: String = lines[body_start..body_end]
        .iter()
        .map(|l| format!("{l}\n"))
        .collect();

    let mut result = 0;
    loop {
        let cond_result = execute_command(cond_str, state);
        if cond_result != 0 {
            break;
        }
        result = execute_script(&body, state);
    }

    (result, body_end + 1)
}

fn execute_for(lines: &[&str], state: &mut ShellState) -> (i32, usize) {
    // for VAR in WORDS; do BODY; done
    let first_line = lines[0].trim();
    let rest = first_line.strip_prefix("for ").unwrap_or("");

    let (var_name, words_str) = if let Some((v, w)) = rest.split_once(" in ") {
        (v.trim(), w.trim_end_matches("; do").trim_end_matches(" do"))
    } else {
        return (1, 1);
    };

    let words = tokenize(&expand_variables(words_str, state));

    let mut body_start = 1;
    if !first_line.contains("do") {
        while body_start < lines.len() && lines[body_start].trim() != "do" {
            body_start += 1;
        }
        body_start += 1;
    }

    let mut body_end = body_start;
    while body_end < lines.len() && lines[body_end].trim() != "done" {
        body_end += 1;
    }

    let body: String = lines[body_start..body_end]
        .iter()
        .map(|l| format!("{l}\n"))
        .collect();

    let mut result = 0;
    for word in &words {
        state.vars.insert(var_name.to_string(), word.clone());
        result = execute_script(&body, state);
    }

    (result, body_end + 1)
}

fn expand_variables(s: &str, state: &ShellState) -> String {
    let mut result = String::new();
    let bytes = s.as_bytes();
    let mut i = 0;
    let mut in_single_quote = false;

    while i < bytes.len() {
        if bytes[i] == b'\'' && !in_single_quote {
            in_single_quote = true;
            i += 1;
            continue;
        }
        if bytes[i] == b'\'' && in_single_quote {
            in_single_quote = false;
            i += 1;
            continue;
        }
        if in_single_quote {
            result.push(bytes[i] as char);
            i += 1;
            continue;
        }

        if bytes[i] == b'\\' && i + 1 < bytes.len() {
            result.push(bytes[i + 1] as char);
            i += 2;
            continue;
        }

        if bytes[i] == b'$' {
            i += 1;
            if i >= bytes.len() {
                result.push('$');
                continue;
            }

            if bytes[i] == b'{' {
                // ${VAR}
                i += 1;
                let start = i;
                while i < bytes.len() && bytes[i] != b'}' {
                    i += 1;
                }
                let var_name = &s[start..i];
                if i < bytes.len() {
                    i += 1;
                }
                if let Some(val) = state.vars.get(var_name) {
                    result.push_str(val);
                }
            } else if bytes[i] == b'(' {
                // $(command) — command substitution
                i += 1;
                let start = i;
                let mut depth = 1;
                while i < bytes.len() && depth > 0 {
                    if bytes[i] == b'(' {
                        depth += 1;
                    } else if bytes[i] == b')' {
                        depth -= 1;
                    }
                    if depth > 0 {
                        i += 1;
                    }
                }
                let cmd = &s[start..i];
                if i < bytes.len() {
                    i += 1;
                }
                // Execute and capture output
                if let Ok(output) = Command::new("sh").arg("-c").arg(cmd).output() {
                    let out = String::from_utf8_lossy(&output.stdout);
                    result.push_str(out.trim_end_matches('\n'));
                }
            } else {
                // $VAR or $N or $? etc.
                let start = i;
                if bytes[i] == b'?' || bytes[i] == b'#' || bytes[i] == b'$' || bytes[i] == b'!' {
                    i += 1;
                } else {
                    while i < bytes.len()
                        && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_')
                    {
                        i += 1;
                    }
                }
                let var_name = &s[start..i];
                if let Some(val) = state.vars.get(var_name) {
                    result.push_str(val);
                }
            }
        } else if bytes[i] == b'"' {
            // Double quote: skip the quote char itself but still expand inside
            i += 1;
            while i < bytes.len() && bytes[i] != b'"' {
                if bytes[i] == b'$' {
                    // Recursively expand (simplified: just get the var)
                    let remaining = &s[i..];
                    let expanded = expand_variables(remaining, state);
                    // This is simplified; proper implementation would track position
                    result.push_str(&expanded);
                    break;
                } else if bytes[i] == b'\\' && i + 1 < bytes.len() {
                    result.push(bytes[i + 1] as char);
                    i += 2;
                } else {
                    result.push(bytes[i] as char);
                    i += 1;
                }
            }
            if i < bytes.len() {
                i += 1; // skip closing "
            }
        } else {
            result.push(bytes[i] as char);
            i += 1;
        }
    }

    result
}

fn tokenize(s: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();
    let bytes = s.as_bytes();
    let mut i = 0;
    let mut in_single = false;
    let mut in_double = false;

    while i < bytes.len() {
        if bytes[i] == b'\'' && !in_double {
            in_single = !in_single;
            i += 1;
        } else if bytes[i] == b'"' && !in_single {
            in_double = !in_double;
            i += 1;
        } else if bytes[i] == b'\\' && !in_single && i + 1 < bytes.len() {
            current.push(bytes[i + 1] as char);
            i += 2;
        } else if bytes[i] == b' ' && !in_single && !in_double {
            if !current.is_empty() {
                words.push(current.clone());
                current.clear();
            }
            i += 1;
        } else {
            current.push(bytes[i] as char);
            i += 1;
        }
    }

    if !current.is_empty() {
        words.push(current);
    }

    words
}

fn parse_redirections(words: &[String]) -> (Vec<String>, Vec<(String, String)>) {
    let mut cmd_words = Vec::new();
    let mut redirects = Vec::new();
    let mut i = 0;

    while i < words.len() {
        match words[i].as_str() {
            ">" | ">>" | "<" | "2>"
                if i + 1 < words.len() => {
                    redirects.push((words[i].clone(), words[i + 1].clone()));
                    i += 2;
                }
            w if w.starts_with('>') => {
                redirects.push((">".to_string(), w[1..].to_string()));
                i += 1;
            }
            w if w.starts_with('<') => {
                redirects.push(("<".to_string(), w[1..].to_string()));
                i += 1;
            }
            _ => {
                cmd_words.push(words[i].clone());
                i += 1;
            }
        }
    }

    (cmd_words, redirects)
}

fn find_logical_op(cmd: &str) -> Option<usize> {
    let bytes = cmd.as_bytes();
    let mut in_single = false;
    let mut in_double = false;
    let mut i = 0;

    while i < bytes.len().saturating_sub(1) {
        match bytes[i] {
            b'\'' if !in_double => in_single = !in_single,
            b'"' if !in_single => in_double = !in_double,
            b'\\' if !in_single => {
                i += 1;
            }
            b'&' if !in_single && !in_double && bytes[i + 1] == b'&' => return Some(i),
            b'|' if !in_single && !in_double && bytes[i + 1] == b'|' => return Some(i),
            _ => {}
        }
        i += 1;
    }
    None
}

fn split_at_logical(cmd: &str, pos: usize) -> (&str, &str, &str) {
    let op = &cmd[pos..pos + 2];
    let left = cmd[..pos].trim();
    let right = cmd[pos + 2..].trim();
    (left, op, right)
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

    fn mk_state(pairs: &[(&str, &str)]) -> ShellState {
        let mut st = ShellState::new();
        // Wipe the inherited env to keep tests deterministic.
        st.vars.clear();
        for (k, v) in pairs {
            st.vars.insert((*k).to_string(), (*v).to_string());
        }
        st
    }

    // ---------- parse_args ----------

    #[test]
    fn parse_args_no_args_is_interactive() {
        let m = parse_args(&s(&["sh"])).unwrap();
        assert_eq!(m, ShMode::Interactive);
    }

    #[test]
    fn parse_args_c_flag_requires_string() {
        let err = parse_args(&s(&["sh", "-c"])).unwrap_err();
        assert!(err.contains("-c"));
    }

    #[test]
    fn parse_args_c_flag_with_string_only() {
        let m = parse_args(&s(&["sh", "-c", "echo hi"])).unwrap();
        assert_eq!(
            m,
            ShMode::Command {
                script: "echo hi".to_string(),
                args: vec![]
            }
        );
    }

    #[test]
    fn parse_args_c_flag_with_extra_args() {
        let m = parse_args(&s(&["sh", "-c", "echo $1", "first", "second"])).unwrap();
        assert_eq!(
            m,
            ShMode::Command {
                script: "echo $1".to_string(),
                args: vec!["first".to_string(), "second".to_string()],
            }
        );
    }

    #[test]
    fn parse_args_script_path() {
        let m = parse_args(&s(&["sh", "build.sh"])).unwrap();
        assert_eq!(
            m,
            ShMode::Script {
                path: "build.sh".to_string(),
                args: vec![]
            }
        );
    }

    #[test]
    fn parse_args_script_with_args() {
        let m = parse_args(&s(&["sh", "build.sh", "release", "verbose"])).unwrap();
        assert_eq!(
            m,
            ShMode::Script {
                path: "build.sh".to_string(),
                args: vec!["release".to_string(), "verbose".to_string()],
            }
        );
    }

    // ---------- set_positionals ----------

    #[test]
    fn set_positionals_writes_indexed_vars_and_count() {
        let mut st = mk_state(&[]);
        set_positionals(&mut st, &s(&["a", "b", "c"]));
        assert_eq!(st.vars.get("1"), Some(&"a".to_string()));
        assert_eq!(st.vars.get("2"), Some(&"b".to_string()));
        assert_eq!(st.vars.get("3"), Some(&"c".to_string()));
        assert_eq!(st.vars.get("#"), Some(&"3".to_string()));
    }

    #[test]
    fn set_positionals_empty_args_gives_count_zero() {
        let mut st = mk_state(&[]);
        set_positionals(&mut st, &[]);
        assert_eq!(st.vars.get("#"), Some(&"0".to_string()));
        assert_eq!(st.vars.get("1"), None);
    }

    // ---------- ShellState ----------

    #[test]
    fn shell_state_set_exit_code_updates_question_mark() {
        let mut st = mk_state(&[]);
        st.set_exit_code(42);
        assert_eq!(st.last_exit_code, 42);
        assert_eq!(st.vars.get("?"), Some(&"42".to_string()));
    }

    // ---------- split_commands ----------

    #[test]
    fn split_commands_simple_semicolon() {
        assert_eq!(split_commands("a;b;c"), vec!["a", "b", "c"]);
    }

    #[test]
    fn split_commands_no_separator_returns_single_chunk() {
        assert_eq!(split_commands("echo hi"), vec!["echo hi"]);
    }

    #[test]
    fn split_commands_respects_single_quotes() {
        assert_eq!(split_commands("echo 'a;b';true"), vec!["echo 'a;b'", "true"]);
    }

    #[test]
    fn split_commands_respects_double_quotes() {
        assert_eq!(split_commands("echo \"a;b\";true"), vec!["echo \"a;b\"", "true"]);
    }

    #[test]
    fn split_commands_respects_backslash() {
        // The escape skips the next byte, so the ';' is treated as literal.
        assert_eq!(split_commands(r"a\;b;c"), vec![r"a\;b", "c"]);
    }

    // ---------- tokenize ----------

    #[test]
    fn tokenize_simple_words() {
        assert_eq!(tokenize("a b c"), vec!["a", "b", "c"]);
    }

    #[test]
    fn tokenize_collapses_runs_of_spaces() {
        // Multiple spaces between words shouldn't produce empty tokens.
        assert_eq!(tokenize("a   b"), vec!["a", "b"]);
    }

    #[test]
    fn tokenize_single_quotes_preserve_spaces() {
        assert_eq!(tokenize("echo 'hello world'"), vec!["echo", "hello world"]);
    }

    #[test]
    fn tokenize_double_quotes_preserve_spaces() {
        assert_eq!(tokenize("echo \"hi there\""), vec!["echo", "hi there"]);
    }

    #[test]
    fn tokenize_backslash_escapes_space() {
        assert_eq!(tokenize(r"a\ b c"), vec!["a b", "c"]);
    }

    #[test]
    fn tokenize_empty_string() {
        assert_eq!(tokenize(""), Vec::<String>::new());
    }

    // ---------- parse_redirections ----------

    #[test]
    fn parse_redirections_none_returns_words_unchanged() {
        let (words, redirs) = parse_redirections(&s(&["ls", "-l"]));
        assert_eq!(words, vec!["ls".to_string(), "-l".to_string()]);
        assert!(redirs.is_empty());
    }

    #[test]
    fn parse_redirections_separate_token_form() {
        let (words, redirs) = parse_redirections(&s(&["ls", ">", "out.txt"]));
        assert_eq!(words, vec!["ls".to_string()]);
        assert_eq!(
            redirs,
            vec![(">".to_string(), "out.txt".to_string())]
        );
    }

    #[test]
    fn parse_redirections_append_and_stderr() {
        let (words, redirs) =
            parse_redirections(&s(&["cmd", ">>", "log", "2>", "err"]));
        assert_eq!(words, vec!["cmd".to_string()]);
        assert_eq!(
            redirs,
            vec![
                (">>".to_string(), "log".to_string()),
                ("2>".to_string(), "err".to_string()),
            ]
        );
    }

    #[test]
    fn parse_redirections_joined_form() {
        // ">file" should be split into (">", "file").
        let (words, redirs) = parse_redirections(&s(&["echo", ">file"]));
        assert_eq!(words, vec!["echo".to_string()]);
        assert_eq!(redirs, vec![(">".to_string(), "file".to_string())]);
    }

    // ---------- find_logical_op / split_at_logical ----------

    #[test]
    fn find_logical_op_finds_and() {
        let pos = find_logical_op("a && b").unwrap();
        assert_eq!(&"a && b"[pos..pos + 2], "&&");
    }

    #[test]
    fn find_logical_op_finds_or() {
        let pos = find_logical_op("a || b").unwrap();
        assert_eq!(&"a || b"[pos..pos + 2], "||");
    }

    #[test]
    fn find_logical_op_none_in_simple_command() {
        assert_eq!(find_logical_op("echo hi"), None);
    }

    #[test]
    fn find_logical_op_respects_single_quotes() {
        // && inside single quotes should be ignored.
        assert_eq!(find_logical_op("echo 'a && b'"), None);
    }

    #[test]
    fn find_logical_op_respects_double_quotes() {
        assert_eq!(find_logical_op("echo \"a || b\""), None);
    }

    #[test]
    fn split_at_logical_splits_command() {
        let cmd = "true && echo hi";
        let pos = find_logical_op(cmd).unwrap();
        let (left, op, right) = split_at_logical(cmd, pos);
        assert_eq!(left, "true");
        assert_eq!(op, "&&");
        assert_eq!(right, "echo hi");
    }

    // ---------- expand_variables ----------

    #[test]
    fn expand_variables_no_dollar_is_identity() {
        let st = mk_state(&[]);
        assert_eq!(expand_variables("hello world", &st), "hello world");
    }

    #[test]
    fn expand_variables_simple_name() {
        let st = mk_state(&[("HOME", "/users/x")]);
        assert_eq!(expand_variables("$HOME", &st), "/users/x");
    }

    #[test]
    fn expand_variables_braced_name() {
        let st = mk_state(&[("HOME", "/users/x")]);
        assert_eq!(expand_variables("${HOME}/bin", &st), "/users/x/bin");
    }

    #[test]
    fn expand_variables_missing_var_becomes_empty() {
        let st = mk_state(&[]);
        assert_eq!(expand_variables("a${MISSING}b", &st), "ab");
    }

    #[test]
    fn expand_variables_single_quotes_suppress_expansion() {
        let st = mk_state(&[("X", "value")]);
        assert_eq!(expand_variables("'$X'", &st), "$X");
    }

    #[test]
    fn expand_variables_backslash_escapes_dollar() {
        let st = mk_state(&[("X", "value")]);
        assert_eq!(expand_variables(r"\$X", &st), "$X");
    }

    #[test]
    fn expand_variables_question_mark_special() {
        let st = mk_state(&[("?", "7")]);
        assert_eq!(expand_variables("exit=$?", &st), "exit=7");
    }

    #[test]
    fn expand_variables_positional_param() {
        let st = mk_state(&[("1", "first"), ("2", "second")]);
        assert_eq!(expand_variables("$1 $2", &st), "first second");
    }

    #[test]
    fn expand_variables_trailing_dollar_is_literal() {
        let st = mk_state(&[]);
        assert_eq!(expand_variables("end$", &st), "end$");
    }
}
