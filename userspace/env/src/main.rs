//! OurOS `env` / `printenv` Utility -- Display and Modify Environment Variables
//!
//! Displays environment variables, optionally modifies them, and runs commands
//! in the modified environment. Also operates in `printenv` mode when invoked
//! via `argv[0]` ending in "printenv".
//!
//! # Usage
//!
//! ```text
//! env                             Display all environment variables
//! env NAME=VALUE... CMD [ARGS]    Run command with modified environment
//! env -i CMD [ARGS]               Run command with empty environment
//! env -u NAME CMD [ARGS]          Unset variable before running command
//! env -C DIR CMD [ARGS]           Change directory before running command
//! env -0                          NUL-terminate output lines
//! env --json                      Output environment as JSON
//! env -v                          Verbose: show modifications
//!
//! printenv                        Display all environment variables
//! printenv NAME [NAME...]         Display specific variables (exit 1 if missing)
//! ```

use std::collections::BTreeMap;
use std::env;
use std::path::Path;
use std::process;

// ============================================================================
// Constants
// ============================================================================

const VERSION: &str = "0.1.0";

// ============================================================================
// Parsed configuration
// ============================================================================

/// An environment modification: set or unset a variable.
enum EnvMod {
    /// Set a variable to a value.
    Set { name: String, value: String },
    /// Remove a variable from the environment.
    Unset { name: String },
}

/// Parsed command-line configuration for `env` mode.
struct EnvConfig {
    /// Start with an empty environment.
    ignore_env: bool,
    /// Environment modifications to apply (in order).
    modifications: Vec<EnvMod>,
    /// Change to this directory before running command.
    chdir: Option<String>,
    /// Use NUL instead of newline for output termination.
    null_terminate: bool,
    /// Output as JSON.
    json: bool,
    /// Verbose mode: report modifications to stderr.
    verbose: bool,
    /// Command and arguments to run (empty = just print).
    command: Vec<String>,
}

/// Parsed command-line configuration for `printenv` mode.
struct PrintenvConfig {
    /// Specific variable names to print (empty = print all).
    names: Vec<String>,
    /// Use NUL instead of newline for output termination.
    null_terminate: bool,
}

/// Top-level parsed action.
enum Action {
    Env(EnvConfig),
    Printenv(PrintenvConfig),
    Help { printenv_mode: bool },
    Version { printenv_mode: bool },
}

// ============================================================================
// Argument parsing -- env mode
// ============================================================================

/// Parse arguments for `env` invocation.
///
/// The parsing is positional: options come first, then NAME=VALUE assignments,
/// then the command. Once a non-option, non-assignment argument is seen,
/// everything from that point onward is the command (including further
/// arguments that look like options).
///
/// The `--` separator explicitly ends option parsing.
fn parse_env_args(args: &[String]) -> Action {
    let mut config = EnvConfig {
        ignore_env: false,
        modifications: Vec::new(),
        chdir: None,
        null_terminate: false,
        json: false,
        verbose: false,
        command: Vec::new(),
    };

    let mut i = 1; // skip argv[0]
    let mut options_done = false;

    while i < args.len() {
        let arg = &args[i];

        // Once we hit a non-option, non-assignment arg, the rest is the command.
        if options_done {
            config.command.push(arg.clone());
            i += 1;
            continue;
        }

        // "--" ends option parsing; next arg starts the command.
        if arg == "--" {
            options_done = true;
            i += 1;
            continue;
        }

        // Long options.
        if arg.starts_with("--") {
            match arg.as_str() {
                "--help" => return Action::Help { printenv_mode: false },
                "--version" => return Action::Version { printenv_mode: false },
                "--ignore-environment" => {
                    config.ignore_env = true;
                }
                "--null" => {
                    config.null_terminate = true;
                }
                "--json" => {
                    config.json = true;
                }
                "--verbose" => {
                    config.verbose = true;
                }
                _ if arg.starts_with("--unset=") => {
                    let name = arg["--unset=".len()..].to_string();
                    if name.is_empty() {
                        eprintln!("env: --unset requires a non-empty variable name");
                        process::exit(125);
                    }
                    config.modifications.push(EnvMod::Unset { name });
                }
                _ if arg.starts_with("--chdir=") => {
                    let dir = arg["--chdir=".len()..].to_string();
                    if dir.is_empty() {
                        eprintln!("env: --chdir requires a non-empty directory path");
                        process::exit(125);
                    }
                    config.chdir = Some(dir);
                }
                _ => {
                    eprintln!("env: unrecognized option '{arg}'");
                    eprintln!("Try 'env --help' for more information.");
                    process::exit(125);
                }
            }
            i += 1;
            continue;
        }

        // Short options (may be combined: -iv).
        if arg.starts_with('-') && arg.len() > 1 {
            let chars: Vec<char> = arg[1..].chars().collect();
            let mut j = 0;
            while j < chars.len() {
                match chars[j] {
                    'i' => {
                        config.ignore_env = true;
                    }
                    '0' => {
                        config.null_terminate = true;
                    }
                    'v' => {
                        config.verbose = true;
                    }
                    'u' => {
                        // -u takes the next piece as the variable name.
                        // If there are more chars in this arg, they are the name.
                        // Otherwise, the next arg is the name.
                        let name = if j + 1 < chars.len() {
                            chars[j + 1..].iter().collect::<String>()
                        } else {
                            i += 1;
                            if i >= args.len() {
                                eprintln!("env: option '-u' requires an argument");
                                process::exit(125);
                            }
                            args[i].clone()
                        };
                        if name.is_empty() {
                            eprintln!("env: -u requires a non-empty variable name");
                            process::exit(125);
                        }
                        config.modifications.push(EnvMod::Unset { name });
                        // Consumed the rest of this short-option cluster.
                        j = chars.len();
                        continue;
                    }
                    'C' => {
                        // -C takes the next piece as the directory.
                        let dir = if j + 1 < chars.len() {
                            chars[j + 1..].iter().collect::<String>()
                        } else {
                            i += 1;
                            if i >= args.len() {
                                eprintln!("env: option '-C' requires an argument");
                                process::exit(125);
                            }
                            args[i].clone()
                        };
                        if dir.is_empty() {
                            eprintln!("env: -C requires a non-empty directory path");
                            process::exit(125);
                        }
                        config.chdir = Some(dir);
                        j = chars.len();
                        continue;
                    }
                    _ => {
                        eprintln!("env: invalid option -- '{}'", chars[j]);
                        eprintln!("Try 'env --help' for more information.");
                        process::exit(125);
                    }
                }
                j += 1;
            }
            i += 1;
            continue;
        }

        // Check for NAME=VALUE assignment (must contain '=' and not start
        // with '=' to be a valid assignment).
        if let Some(eq_pos) = arg.find('=')
            && eq_pos > 0 {
                let name = arg[..eq_pos].to_string();
                let value = arg[eq_pos + 1..].to_string();
                config.modifications.push(EnvMod::Set { name, value });
                i += 1;
                continue;
            }

        // Not an option, not an assignment -- this is the start of the command.
        options_done = true;
        config.command.push(arg.clone());
        i += 1;
    }

    Action::Env(config)
}

// ============================================================================
// Argument parsing -- printenv mode
// ============================================================================

/// Parse arguments for `printenv` invocation.
fn parse_printenv_args(args: &[String]) -> Action {
    let mut config = PrintenvConfig {
        names: Vec::new(),
        null_terminate: false,
    };

    for arg in &args[1..] {
        match arg.as_str() {
            "--help" | "-h" => return Action::Help { printenv_mode: true },
            "--version" => return Action::Version { printenv_mode: true },
            "-0" | "--null" => {
                config.null_terminate = true;
            }
            _ if arg.starts_with('-') && arg.len() > 1 => {
                // Check for combined short flags (e.g., -0).
                let mut valid = true;
                for ch in arg[1..].chars() {
                    if ch == '0' {
                        config.null_terminate = true;
                    } else {
                        valid = false;
                        break;
                    }
                }
                if !valid {
                    eprintln!("printenv: unrecognized option '{arg}'");
                    eprintln!("Try 'printenv --help' for more information.");
                    process::exit(2);
                }
            }
            _ => {
                config.names.push(arg.clone());
            }
        }
    }

    Action::Printenv(config)
}

// ============================================================================
// Environment building
// ============================================================================

/// Build the environment map according to the configuration.
///
/// If `ignore_env` is set, starts with an empty map. Otherwise starts with
/// the current process environment. Then applies all modifications in order.
fn build_environment(config: &EnvConfig) -> BTreeMap<String, String> {
    let mut environ: BTreeMap<String, String> = if config.ignore_env {
        BTreeMap::new()
    } else {
        env::vars().collect()
    };

    for modification in &config.modifications {
        match modification {
            EnvMod::Set { name, value } => {
                if config.verbose {
                    eprintln!("env: setting {name}={value}");
                }
                environ.insert(name.clone(), value.clone());
            }
            EnvMod::Unset { name } => {
                if config.verbose {
                    if environ.contains_key(name) {
                        eprintln!("env: unsetting {name}");
                    } else {
                        eprintln!("env: unsetting {name} (was not set)");
                    }
                }
                environ.remove(name);
            }
        }
    }

    environ
}

// ============================================================================
// Output helpers
// ============================================================================

/// Print all environment variables from the map.
fn print_env(environ: &BTreeMap<String, String>, terminator: &str) {
    for (name, value) in environ {
        print!("{name}={value}{terminator}");
    }
}

/// Print environment as JSON.
///
/// Hand-built to avoid pulling in serde/serde_json as dependencies for a
/// tiny utility. Values are escaped for JSON safety.
fn print_env_json(environ: &BTreeMap<String, String>) {
    println!("{{");
    let count = environ.len();
    for (i, (name, value)) in environ.iter().enumerate() {
        let comma = if i + 1 < count { "," } else { "" };
        println!("  {}: {}{comma}", json_escape(name), json_escape(value));
    }
    println!("}}");
}

/// Escape a string for JSON output. Wraps the result in double quotes.
fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => {
                // Unicode escape for other control characters.
                for unit in c.encode_utf16(&mut [0u16; 2]) {
                    out.push_str(&format!("\\u{unit:04x}"));
                }
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

// ============================================================================
// Command execution
// ============================================================================

/// Execute a command with the given environment.
///
/// Replaces the process environment entirely with `environ`, optionally
/// changes directory first, then execs the command.
///
/// Returns the exit code. Uses 126 for permission/exec errors and 127 for
/// command-not-found, matching POSIX shell conventions.
fn exec_command(
    command: &[String],
    environ: &BTreeMap<String, String>,
    chdir: Option<&str>,
) -> i32 {
    let Some(program) = command.first() else {
        // No command to run -- should not happen; caller checks.
        return 0;
    };

    // Change directory if requested.
    if let Some(dir) = chdir
        && let Err(e) = env::set_current_dir(dir) {
            eprintln!("env: cannot change directory to '{dir}': {e}");
            return 125;
        }

    let mut cmd = process::Command::new(program);

    // Set arguments (everything after the program name).
    if command.len() > 1 {
        cmd.args(&command[1..]);
    }

    // Clear inherited environment and set the computed one.
    cmd.env_clear();
    for (name, value) in environ {
        cmd.env(name, value);
    }

    match cmd.status() {
        Ok(status) => {
            status.code().unwrap_or(128)
        }
        Err(e) => {
            let kind = e.kind();
            eprintln!("env: '{program}': {e}");
            match kind {
                std::io::ErrorKind::NotFound => 127,
                std::io::ErrorKind::PermissionDenied => 126,
                _ => 125,
            }
        }
    }
}

// ============================================================================
// Help text
// ============================================================================

fn print_env_help() {
    println!("OurOS env v{VERSION}");
    println!();
    println!("Display, modify, or clear environment variables, optionally");
    println!("running a command in the modified environment.");
    println!();
    println!("USAGE:");
    println!("  env [OPTION]... [NAME=VALUE]... [COMMAND [ARGS]...]");
    println!();
    println!("With no COMMAND, print the resulting environment.");
    println!();
    println!("OPTIONS:");
    println!("  -i, --ignore-environment  Start with an empty environment");
    println!("  -u NAME, --unset=NAME     Remove variable from the environment");
    println!("  -C DIR, --chdir=DIR       Change working directory to DIR");
    println!("  -0, --null                End each line with NUL, not newline");
    println!("      --json                Output environment as JSON");
    println!("  -v, --verbose             Report modifications to stderr");
    println!("      --help                Display this help and exit");
    println!("      --version             Display version and exit");
    println!();
    println!("EXIT STATUS:");
    println!("  0     Success (no command), or command exited 0");
    println!("  1-124 Command exit status");
    println!("  125   env itself failed (bad option, chdir error)");
    println!("  126   Command found but could not be executed");
    println!("  127   Command not found");
}

fn print_printenv_help() {
    println!("OurOS printenv v{VERSION}");
    println!();
    println!("Print the values of specified environment variables.");
    println!();
    println!("USAGE:");
    println!("  printenv [OPTION]... [NAME]...");
    println!();
    println!("With no NAME, print all environment variables.");
    println!();
    println!("OPTIONS:");
    println!("  -0, --null     End each line with NUL, not newline");
    println!("  -h, --help     Display this help and exit");
    println!("      --version  Display version and exit");
    println!();
    println!("EXIT STATUS:");
    println!("  0  All specified variables were found");
    println!("  1  One or more variables were not found");
    println!("  2  Usage error");
}

// ============================================================================
// Main
// ============================================================================

fn main() {
    let args: Vec<String> = env::args().collect();

    // Detect printenv mode from argv[0].
    let printenv_mode = args
        .first()
        .map(|a| {
            Path::new(a)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                == "printenv"
        })
        .unwrap_or(false);

    let action = if printenv_mode {
        parse_printenv_args(&args)
    } else {
        parse_env_args(&args)
    };

    let exit_code = run(action);
    process::exit(exit_code);
}

/// Execute the parsed action. Returns the exit code.
fn run(action: Action) -> i32 {
    match action {
        Action::Help { printenv_mode } => {
            if printenv_mode {
                print_printenv_help();
            } else {
                print_env_help();
            }
            0
        }

        Action::Version { printenv_mode } => {
            let name = if printenv_mode { "printenv" } else { "env" };
            println!("{name} (OurOS) {VERSION}");
            0
        }

        Action::Printenv(config) => run_printenv(config),
        Action::Env(config) => run_env(config),
    }
}

/// Execute printenv mode.
///
/// With no names: print all variables (sorted). With names: print each
/// requested variable's value, returning 1 if any were not found.
fn run_printenv(config: PrintenvConfig) -> i32 {
    let terminator = if config.null_terminate { "\0" } else { "\n" };

    if config.names.is_empty() {
        // Print all environment variables, sorted.
        let environ: BTreeMap<String, String> = env::vars().collect();
        for (name, value) in &environ {
            print!("{name}={value}{terminator}");
        }
        return 0;
    }

    // Print specific variables.
    let mut exit_code = 0;
    for name in &config.names {
        match env::var(name) {
            Ok(value) => {
                print!("{value}{terminator}");
            }
            Err(_) => {
                // Variable not found -- note the failure but continue
                // printing others.
                exit_code = 1;
            }
        }
    }

    exit_code
}

/// Execute env mode.
///
/// Builds the environment, then either prints it or runs a command in it.
fn run_env(config: EnvConfig) -> i32 {
    if config.verbose && config.ignore_env {
        eprintln!("env: starting with empty environment");
    }

    let environ = build_environment(&config);

    // If there is no command, print the environment and exit.
    if config.command.is_empty() {
        if config.json {
            print_env_json(&environ);
        } else {
            let terminator = if config.null_terminate { "\0" } else { "\n" };
            print_env(&environ, terminator);
        }
        return 0;
    }

    // A command was given -- execute it.
    exec_command(&config.command, &environ, config.chdir.as_deref())
}
