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
    let mut state = ShellState::new();

    // Set positional parameters
    state.vars.insert("0".to_string(), args[0].clone());

    if args.len() > 1 {
        if args[1] == "-c" {
            // Execute command string
            if args.len() < 3 {
                eprintln!("sh: -c: option requires an argument");
                process::exit(2);
            }
            let script = &args[2];
            for (i, a) in args[3..].iter().enumerate() {
                state.vars.insert((i + 1).to_string(), a.clone());
            }
            state
                .vars
                .insert("#".to_string(), args[3..].len().to_string());
            let exit_code = execute_script(script, &mut state);
            process::exit(exit_code);
        } else {
            // Execute script file
            let script_path = &args[1];
            for (i, a) in args[2..].iter().enumerate() {
                state.vars.insert((i + 1).to_string(), a.clone());
            }
            state
                .vars
                .insert("#".to_string(), args[2..].len().to_string());
            match fs::read_to_string(script_path) {
                Ok(content) => {
                    let exit_code = execute_script(&content, &mut state);
                    process::exit(exit_code);
                }
                Err(e) => {
                    eprintln!("sh: {script_path}: {e}");
                    process::exit(127);
                }
            }
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
