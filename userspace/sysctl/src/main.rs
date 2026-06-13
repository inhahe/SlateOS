//! SlateOS Sysctl — View and Modify Kernel Parameters
//!
//! Reads and writes kernel tunable parameters exposed via `/proc/sys/` and
//! `/sys/kernel/`. Provides the standard `sysctl` interface for inspecting
//! and changing runtime kernel behavior.
//!
//! # Usage
//!
//! ```text
//! sysctl -a                     List all parameters
//! sysctl <name>                 Read a parameter (dot or slash notation)
//! sysctl <name>=<value>         Write a parameter (inline syntax)
//! sysctl -w <name> <value>      Write a parameter (flag syntax)
//! sysctl -p [file]              Load settings from file (default /etc/sysctl.conf)
//! sysctl --search <pattern>     Search parameter names for substring
//! sysctl -q <name>              Read without printing the key name
//! sysctl -n <name>              Print only the value (no key prefix)
//! ```
//!
//! # Path Resolution
//!
//! Parameters use dot-separated names that map to filesystem paths:
//!
//! - `kernel.hostname` -> `/proc/sys/kernel/hostname`
//! - `net.ipv4.ip_forward` -> `/proc/sys/net/ipv4/ip_forward`
//!
//! Both `/proc/sys/` and `/sys/kernel/` trees are searched. The `/proc/sys/`
//! tree takes precedence when a name is ambiguous.
//!
//! # Config File Format
//!
//! Lines in `/etc/sysctl.conf` (or any file loaded with `-p`):
//!
//! ```text
//! # Comment lines start with hash
//! kernel.hostname = slateos
//! net.ipv4.ip_forward = 1
//! ```

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process;

// ============================================================================
// Constants
// ============================================================================

/// Primary tree for kernel tunables.
const PROC_SYS_ROOT: &str = "/proc/sys";

/// Secondary tree (some parameters live here instead).
const SYS_KERNEL_ROOT: &str = "/sys/kernel";

/// Default configuration file loaded by `sysctl -p`.
const DEFAULT_CONF: &str = "/etc/sysctl.conf";

// ============================================================================
// Command / action types
// ============================================================================

/// What the user asked us to do.
enum Action {
    /// List every parameter under both trees.
    ListAll,
    /// Read one parameter by name.
    Read { name: String, quiet: bool, value_only: bool },
    /// Write a value to a parameter.
    Write { name: String, value: String },
    /// Load key=value pairs from a config file.
    LoadFile { path: String },
    /// Search parameter names for a substring.
    Search { pattern: String },
    /// Show usage help.
    Help,
}

// ============================================================================
// Path conversion helpers
// ============================================================================

/// Convert a dot-separated parameter name to a filesystem path.
///
/// Tries `/proc/sys/` first, then `/sys/kernel/`. Returns `None` if the
/// parameter does not exist under either tree.
fn name_to_path(name: &str) -> Option<PathBuf> {
    // Accept both dot notation and raw path notation.
    let relative = if name.starts_with('/') {
        // Absolute path provided — use directly if it exists.
        let p = PathBuf::from(name);
        if p.exists() {
            return Some(p);
        }
        // Otherwise strip the root prefix and try the other tree.
        if let Ok(stripped) = p.strip_prefix(PROC_SYS_ROOT) {
            stripped.to_path_buf()
        } else if let Ok(stripped) = p.strip_prefix(SYS_KERNEL_ROOT) {
            stripped.to_path_buf()
        } else {
            return None;
        }
    } else {
        // Dot notation: kernel.hostname -> kernel/hostname
        PathBuf::from(name.replace('.', "/"))
    };

    // Try /proc/sys/ first.
    let proc_path = Path::new(PROC_SYS_ROOT).join(&relative);
    if proc_path.exists() {
        return Some(proc_path);
    }

    // Try /sys/kernel/ as fallback.
    let sys_path = Path::new(SYS_KERNEL_ROOT).join(&relative);
    if sys_path.exists() {
        return Some(sys_path);
    }

    None
}

/// Convert a filesystem path back to dot-separated parameter name.
///
/// `/proc/sys/kernel/hostname` -> `kernel.hostname`
/// `/sys/kernel/debug/foo`     -> `debug.foo`
fn path_to_name(path: &Path) -> String {
    let stripped = if let Ok(s) = path.strip_prefix(PROC_SYS_ROOT) {
        s
    } else if let Ok(s) = path.strip_prefix(SYS_KERNEL_ROOT) {
        s
    } else {
        path
    };

    // Convert path separators to dots.
    let mut name = String::new();
    for (i, component) in stripped.components().enumerate() {
        if i > 0 {
            name.push('.');
        }
        if let std::path::Component::Normal(os) = component
            && let Some(s) = os.to_str() {
                name.push_str(s);
            }
    }
    name
}

// ============================================================================
// Parameter reading / writing
// ============================================================================

/// Read a single kernel parameter, returning its trimmed value.
fn read_param(path: &Path) -> Result<String, String> {
    if !path.exists() {
        return Err(format!("parameter not found: {}", path.display()));
    }

    if path.is_dir() {
        return Err(format!("{} is a directory, not a parameter", path.display()));
    }

    fs::read_to_string(path)
        .map(|s| s.trim().to_string())
        .map_err(|e| format!("cannot read {}: {}", path.display(), e))
}

/// Write a value to a kernel parameter file.
fn write_param(path: &Path, value: &str) -> Result<(), String> {
    if !path.exists() {
        return Err(format!("parameter not found: {}", path.display()));
    }

    if path.is_dir() {
        return Err(format!("{} is a directory, not a parameter", path.display()));
    }

    fs::write(path, value)
        .map_err(|e| format!("cannot write {}: {} (are you root?)", path.display(), e))
}

// ============================================================================
// Recursive directory walker
// ============================================================================

/// Walk a directory tree, collecting all leaf files (parameters).
///
/// Each entry is returned as its full filesystem path. Symlinks and
/// unreadable directories are silently skipped.
fn walk_params(root: &Path) -> Vec<PathBuf> {
    let mut results = Vec::new();
    walk_recursive(root, &mut results);
    results.sort();
    results
}

fn walk_recursive(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return, // Permission denied or not a directory — skip.
    };

    for entry in entries {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };

        let path = entry.path();

        // Skip symbolic links to avoid loops.
        if path.is_symlink() {
            continue;
        }

        if path.is_dir() {
            walk_recursive(&path, out);
        } else if path.is_file() {
            out.push(path);
        }
    }
}

// ============================================================================
// List all parameters
// ============================================================================

/// Print every readable parameter in dot notation with its current value.
fn list_all() {
    let mut count = 0;

    // Walk /proc/sys/.
    let proc_root = Path::new(PROC_SYS_ROOT);
    if proc_root.is_dir() {
        for path in walk_params(proc_root) {
            if let Ok(value) = read_param(&path) {
                let name = path_to_name(&path);
                // Collapse multi-line values into single line for display.
                let display_val = collapse_value(&value);
                println!("{name} = {display_val}");
                count += 1;
            }
        }
    }

    // Walk /sys/kernel/.
    let sys_root = Path::new(SYS_KERNEL_ROOT);
    if sys_root.is_dir() {
        for path in walk_params(sys_root) {
            if let Ok(value) = read_param(&path) {
                let name = path_to_name(&path);
                let display_val = collapse_value(&value);
                println!("{name} = {display_val}");
                count += 1;
            }
        }
    }

    if count == 0 {
        eprintln!("sysctl: no parameters found (are /proc/sys and /sys/kernel mounted?)");
    }
}

/// Collapse a multi-line parameter value into a single display line.
///
/// Tabs within values are replaced with spaces. Newlines become tab
/// separators, matching Linux sysctl output convention.
fn collapse_value(value: &str) -> String {
    if value.contains('\n') {
        value.lines().collect::<Vec<_>>().join("\t")
    } else {
        value.to_string()
    }
}

// ============================================================================
// Search parameters
// ============================================================================

/// Search both trees for parameters whose dot-name contains `pattern`.
fn search_params(pattern: &str) {
    let pattern_lower = pattern.to_lowercase();
    let mut found = 0;

    for root_path in &[PROC_SYS_ROOT, SYS_KERNEL_ROOT] {
        let root = Path::new(root_path);
        if !root.is_dir() {
            continue;
        }
        for path in walk_params(root) {
            let name = path_to_name(&path);
            if name.to_lowercase().contains(&pattern_lower)
                && let Ok(value) = read_param(&path) {
                    let display_val = collapse_value(&value);
                    println!("{name} = {display_val}");
                    found += 1;
                }
        }
    }

    if found == 0 {
        eprintln!("sysctl: no parameters matching '{pattern}'");
        process::exit(1);
    }
}

// ============================================================================
// Load config file
// ============================================================================

/// Parse and apply a sysctl configuration file.
///
/// Format: one `key = value` per line. Lines starting with `#` or `;` are
/// comments. Blank lines are ignored.
fn load_config(path: &str) {
    let content = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("sysctl: cannot read {path}: {e}");
            process::exit(1);
        }
    };

    let mut errors = 0u32;

    for (line_num, raw_line) in content.lines().enumerate() {
        let line = raw_line.trim();

        // Skip empty lines and comments.
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }

        // Parse key = value (or key=value).
        let (key, value) = match line.split_once('=') {
            Some((k, v)) => (k.trim(), v.trim()),
            None => {
                eprintln!(
                    "sysctl: {}:{}: invalid syntax (expected key = value): {}",
                    path,
                    line_num + 1,
                    line,
                );
                errors = errors.saturating_add(1);
                continue;
            }
        };

        if key.is_empty() {
            eprintln!("sysctl: {}:{}: empty key", path, line_num + 1);
            errors = errors.saturating_add(1);
            continue;
        }

        // Resolve the parameter path.
        match name_to_path(key) {
            Some(param_path) => {
                match write_param(&param_path, value) {
                    Ok(()) => println!("{key} = {value}"),
                    Err(e) => {
                        eprintln!("sysctl: {e}");
                        errors = errors.saturating_add(1);
                    }
                }
            }
            None => {
                eprintln!("sysctl: unknown parameter: {key}");
                errors = errors.saturating_add(1);
            }
        }
    }

    if errors > 0 {
        eprintln!("sysctl: {errors} error(s) while loading {path}");
        process::exit(1);
    }
}

// ============================================================================
// Argument parsing
// ============================================================================

/// Parse command-line arguments into an `Action`.
fn parse_args(args: &[String]) -> Action {
    if args.len() <= 1 {
        return Action::Help;
    }

    let mut i = 1;
    let mut quiet = false;
    let mut value_only = false;

    while i < args.len() {
        let arg = args[i].as_str();

        match arg {
            "-h" | "--help" | "help" => {
                return Action::Help;
            }

            "-a" | "--all" => {
                return Action::ListAll;
            }

            "-p" | "--load" => {
                // -p [file] — load config, optional path.
                let path = if i + 1 < args.len() && !args[i + 1].starts_with('-') {
                    i += 1;
                    args[i].clone()
                } else {
                    DEFAULT_CONF.to_string()
                };
                return Action::LoadFile { path };
            }

            "--search" | "--pattern" => {
                if i + 1 >= args.len() {
                    eprintln!("sysctl: --search requires a pattern argument");
                    process::exit(1);
                }
                return Action::Search { pattern: args[i + 1].clone() };
            }

            "-w" | "--write" => {
                // -w <name> <value>
                if i + 2 >= args.len() {
                    eprintln!("sysctl: -w requires <name> <value>");
                    process::exit(1);
                }
                let name = args[i + 1].clone();
                let value = args[i + 2].clone();
                return Action::Write { name, value };
            }

            "-q" | "--quiet" => {
                quiet = true;
                i += 1;
                continue;
            }

            "-n" | "--values-only" => {
                value_only = true;
                i += 1;
                continue;
            }

            _ => {
                // Check for inline assignment: name=value
                if let Some((name, value)) = arg.split_once('=')
                    && !name.is_empty() {
                        return Action::Write {
                            name: name.to_string(),
                            value: value.to_string(),
                        };
                    }

                // Otherwise treat as a read request.
                return Action::Read {
                    name: arg.to_string(),
                    quiet,
                    value_only,
                };
            }
        }
    }

    // If we consumed only modifier flags (-q/-n) with no operand, show help.
    Action::Help
}

// ============================================================================
// Usage / help
// ============================================================================

fn print_usage() {
    println!("Slate OS Kernel Parameter Utility v0.1.0");
    println!();
    println!("View and modify kernel runtime parameters from /proc/sys/ and /sys/kernel/.");
    println!();
    println!("USAGE:");
    println!("  sysctl -a                     List all parameters");
    println!("  sysctl <name>                 Read a parameter");
    println!("  sysctl <name>=<value>         Write a parameter (inline)");
    println!("  sysctl -w <name> <value>      Write a parameter (flag syntax)");
    println!("  sysctl -p [file]              Load from config file");
    println!("  sysctl --search <pattern>     Search parameter names");
    println!();
    println!("OPTIONS:");
    println!("  -a, --all           List all readable parameters");
    println!("  -w, --write         Write mode: -w <name> <value>");
    println!("  -p, --load [file]   Load settings from file (default: {DEFAULT_CONF})");
    println!("  --search <pattern>  Search names for substring (case-insensitive)");
    println!("  -n, --values-only   Print only the value, not the key");
    println!("  -q, --quiet         Suppress the key name on read");
    println!("  -h, --help          Show this help");
    println!();
    println!("PARAMETER NAMES:");
    println!("  Use dot notation:     kernel.hostname");
    println!("  Or path notation:     /proc/sys/kernel/hostname");
    println!();
    println!("CONFIG FILE FORMAT ({DEFAULT_CONF}):");
    println!("  # Lines starting with # or ; are comments");
    println!("  kernel.hostname = slateos");
    println!("  net.ipv4.ip_forward = 1");
}

// ============================================================================
// Main
// ============================================================================

fn main() {
    let args: Vec<String> = env::args().collect();
    let action = parse_args(&args);

    match action {
        Action::Help => {
            print_usage();
        }

        Action::ListAll => {
            list_all();
        }

        Action::Read { name, quiet, value_only } => {
            let path = match name_to_path(&name) {
                Some(p) => p,
                None => {
                    eprintln!("sysctl: unknown parameter: {name}");
                    process::exit(1);
                }
            };

            match read_param(&path) {
                Ok(value) => {
                    let display_val = collapse_value(&value);
                    if value_only || quiet {
                        println!("{display_val}");
                    } else {
                        let dot_name = path_to_name(&path);
                        println!("{dot_name} = {display_val}");
                    }
                }
                Err(e) => {
                    eprintln!("sysctl: {e}");
                    process::exit(1);
                }
            }
        }

        Action::Write { name, value } => {
            let path = match name_to_path(&name) {
                Some(p) => p,
                None => {
                    eprintln!("sysctl: unknown parameter: {name}");
                    process::exit(1);
                }
            };

            match write_param(&path, &value) {
                Ok(()) => {
                    let dot_name = path_to_name(&path);
                    println!("{dot_name} = {value}");
                }
                Err(e) => {
                    eprintln!("sysctl: {e}");
                    process::exit(1);
                }
            }
        }

        Action::LoadFile { path } => {
            load_config(&path);
        }

        Action::Search { pattern } => {
            search_params(&pattern);
        }
    }
}
