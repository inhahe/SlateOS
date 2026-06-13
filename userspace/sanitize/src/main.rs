//! SlateOS Filename Sanitizer
//!
//! A utility that renames files and directories with problematic characters
//! to safe, portable names. Handles spaces, special characters, control
//! characters, unicode normalization issues, and platform-unsafe patterns.
//!
//! # Modes
//!
//! - **Conservative**: replace spaces with underscores, strip control chars
//! - **Strict**: ASCII-only, lowercase, dashes instead of spaces
//! - **Windows-safe**: remove characters illegal on Windows/FAT (: * ? " < > |)
//! - **Custom**: user-defined replacement rules
//!
//! # Commands
//!
//! ```text
//! sanitize [options] <path> [paths...]
//!
//! Options:
//!   --dry-run, -n     Show what would be renamed without doing it
//!   --recursive, -r   Process directories recursively
//!   --mode <mode>     Sanitization mode (conservative|strict|windows|minimal)
//!   --lowercase, -l   Convert to lowercase
//!   --replace <c> <r> Replace character c with string r
//!   --strip <chars>   Strip these characters entirely
//!   --max-len <n>     Maximum filename length (default: 200)
//!   --verbose, -v     Show all files, not just renamed ones
//! ```

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process;

// ============================================================================
// Configuration
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq)]
enum SanitizeMode {
    /// Replace spaces→underscores, strip control chars.
    Conservative,
    /// ASCII-only, lowercase, dashes for spaces.
    Strict,
    /// Remove Windows/FAT-illegal characters.
    Windows,
    /// Minimal: only strip truly dangerous chars (/ and \0).
    Minimal,
}

#[derive(Debug, Clone)]
struct Config {
    mode: SanitizeMode,
    dry_run: bool,
    recursive: bool,
    lowercase: bool,
    max_length: usize,
    verbose: bool,
    custom_replacements: Vec<(char, String)>,
    strip_chars: Vec<char>,
}

impl Config {
    fn default_config() -> Self {
        Config {
            mode: SanitizeMode::Conservative,
            dry_run: false,
            recursive: false,
            lowercase: false,
            max_length: 200,
            verbose: false,
            custom_replacements: Vec::new(),
            strip_chars: Vec::new(),
        }
    }
}

// ============================================================================
// Sanitization engine
// ============================================================================

/// Characters that are illegal on Windows/FAT filesystems.
const WINDOWS_ILLEGAL: &[char] = &[':', '*', '?', '"', '<', '>', '|', '\\'];

/// Windows reserved names (case-insensitive).
const WINDOWS_RESERVED: &[&str] = &[
    "CON", "PRN", "AUX", "NUL",
    "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9",
    "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Sanitize a single filename (not the full path — just the name component).
fn sanitize_name(name: &str, config: &Config) -> String {
    let mut result = String::with_capacity(name.len());

    // Apply custom strip first.
    for ch in name.chars() {
        if config.strip_chars.contains(&ch) {
            continue;
        }

        // Apply custom replacements.
        let mut replaced = false;
        for (from, to) in &config.custom_replacements {
            if ch == *from {
                result.push_str(to);
                replaced = true;
                break;
            }
        }
        if replaced {
            continue;
        }

        // Mode-specific transformations.
        match config.mode {
            SanitizeMode::Minimal => {
                // Only strip null bytes (forward slash is path separator,
                // handled by the filesystem).
                if ch == '\0' {
                    continue;
                }
                result.push(ch);
            }
            SanitizeMode::Conservative => {
                if ch.is_control() {
                    continue;
                }
                if ch == ' ' {
                    result.push('_');
                } else {
                    result.push(ch);
                }
            }
            SanitizeMode::Strict => {
                if ch.is_ascii_alphanumeric() || ch == '.' || ch == '-' || ch == '_' {
                    result.push(ch);
                } else if ch == ' ' || ch == '\t' {
                    result.push('-');
                }
                // All other characters are dropped.
            }
            SanitizeMode::Windows => {
                if ch.is_control() {
                    continue;
                }
                if WINDOWS_ILLEGAL.contains(&ch) {
                    result.push('_');
                } else {
                    result.push(ch);
                }
            }
        }
    }

    // Lowercase if requested.
    if config.lowercase {
        result = result.to_lowercase();
    }

    // Collapse consecutive underscores/dashes.
    result = collapse_repeats(&result, '_');
    result = collapse_repeats(&result, '-');

    // Strip leading/trailing dots and spaces.
    result = result.trim_matches(|c: char| c == '.' || c == ' ').to_string();

    // Handle Windows reserved names.
    if config.mode == SanitizeMode::Windows || config.mode == SanitizeMode::Strict {
        let name_upper = result.to_uppercase();
        let base_name = name_upper.split('.').next().unwrap_or("");
        if WINDOWS_RESERVED.contains(&base_name) {
            result = format!("_{result}");
        }
    }

    // Truncate to max length (preserve extension).
    if result.len() > config.max_length {
        if let Some(dot_pos) = result.rfind('.') {
            let ext = &result[dot_pos..];
            let max_base = config.max_length.saturating_sub(ext.len());
            result = format!("{}{}", &result[..max_base], ext);
        } else {
            result.truncate(config.max_length);
        }
    }

    // If result is empty after all transformations, use a fallback.
    if result.is_empty() {
        result = "unnamed".to_string();
    }

    result
}

fn collapse_repeats(s: &str, ch: char) -> String {
    let mut result = String::with_capacity(s.len());
    let mut last_was_target = false;

    for c in s.chars() {
        if c == ch {
            if !last_was_target {
                result.push(c);
            }
            last_was_target = true;
        } else {
            result.push(c);
            last_was_target = false;
        }
    }

    result
}

// ============================================================================
// File processing
// ============================================================================

struct Stats {
    scanned: u64,
    renamed: u64,
    skipped: u64,
    errors: u64,
}

fn process_path(path: &Path, config: &Config, stats: &mut Stats) {
    if path.is_dir() && config.recursive {
        process_directory(path, config, stats);
    } else if path.is_file() || path.is_dir() {
        process_single(path, config, stats);
    } else {
        eprintln!("  skip: {} (not a file or directory)", path.display());
        stats.skipped += 1;
    }
}

fn process_directory(dir: &Path, config: &Config, stats: &mut Stats) {
    let entries = match fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(e) => {
            eprintln!("  error reading {}: {e}", dir.display());
            stats.errors += 1;
            return;
        }
    };

    // Collect entries first (to avoid rename-while-iterating issues).
    let mut paths: Vec<PathBuf> = Vec::new();
    for e in entries.flatten() {
        paths.push(e.path());
    }

    // Process children first (depth-first so renames don't break parent paths).
    for path in &paths {
        if path.is_dir() && config.recursive {
            process_directory(path, config, stats);
        }
    }

    // Then rename entries in this directory.
    for path in &paths {
        process_single(path, config, stats);
    }
}

fn process_single(path: &Path, config: &Config, stats: &mut Stats) {
    stats.scanned += 1;

    let file_name = match path.file_name().and_then(|n| n.to_str()) {
        Some(n) => n,
        None => {
            if config.verbose {
                eprintln!("  skip: {} (non-UTF-8 name)", path.display());
            }
            stats.skipped += 1;
            return;
        }
    };

    let sanitized = sanitize_name(file_name, config);

    if sanitized == file_name {
        if config.verbose {
            println!("  ok:   {file_name}");
        }
        return;
    }

    // Build new path.
    let parent = path.parent().unwrap_or(Path::new("."));
    let new_path = parent.join(&sanitized);

    // Check for collision.
    if new_path.exists() && new_path != path {
        // Try appending a number.
        let (base, ext) = split_name_ext(&sanitized);
        let mut n = 1u32;
        let final_path = loop {
            let candidate = if ext.is_empty() {
                format!("{base}_{n}")
            } else {
                format!("{base}_{n}.{ext}")
            };
            let candidate_path = parent.join(&candidate);
            if !candidate_path.exists() {
                break candidate_path;
            }
            n += 1;
            if n > 999 {
                eprintln!("  error: cannot find unique name for {file_name}");
                stats.errors += 1;
                return;
            }
        };

        println!("  rename: {file_name} → {}", final_path.file_name().unwrap_or_default().to_string_lossy());

        if !config.dry_run
            && let Err(e) = fs::rename(path, &final_path) {
                eprintln!("    error: {e}");
                stats.errors += 1;
                return;
            }
    } else {
        println!("  rename: {file_name} → {sanitized}");

        if !config.dry_run
            && let Err(e) = fs::rename(path, &new_path) {
                eprintln!("    error: {e}");
                stats.errors += 1;
                return;
            }
    }

    stats.renamed += 1;
}

fn split_name_ext(name: &str) -> (String, String) {
    if let Some(dot_pos) = name.rfind('.')
        && dot_pos > 0 {
            return (name[..dot_pos].to_string(), name[dot_pos + 1..].to_string());
        }
    (name.to_string(), String::new())
}

// ============================================================================
// Usage and main
// ============================================================================

fn print_usage() {
    println!("Slate OS Filename Sanitizer v0.1.0");
    println!();
    println!("Clean up problematic filenames (spaces, special chars, control chars).");
    println!();
    println!("USAGE:");
    println!("  sanitize [options] <path> [paths...]");
    println!();
    println!("OPTIONS:");
    println!("  --dry-run, -n       Show what would be renamed without doing it");
    println!("  --recursive, -r     Process directories recursively");
    println!("  --mode <mode>       Sanitization mode:");
    println!("                        conservative — spaces→underscores, strip control (default)");
    println!("                        strict       — ASCII-only, lowercase, dashes");
    println!("                        windows      — remove FAT/NTFS illegal chars");
    println!("                        minimal      — only strip null bytes");
    println!("  --lowercase, -l     Convert filenames to lowercase");
    println!("  --max-len <n>       Maximum filename length (default: 200)");
    println!("  --verbose, -v       Show all files, not just renamed ones");
    println!("  --replace <c> <r>   Replace character c with string r");
    println!("  --strip <chars>     Strip these characters entirely");
    println!();
    println!("EXAMPLES:");
    println!("  sanitize -n .                # dry-run current directory");
    println!("  sanitize -r --mode strict /home/user/downloads");
    println!("  sanitize --mode windows -r /mnt/usb");
    println!("  sanitize -l --replace ' ' '-' *.txt");
    println!("  sanitize --strip '()[]' -r .");
}

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        print_usage();
        process::exit(0);
    }

    let mut config = Config::default_config();
    let mut paths: Vec<String> = Vec::new();
    let mut i = 1;

    while i < args.len() {
        match args[i].as_str() {
            "--dry-run" | "-n" => {
                config.dry_run = true;
                i += 1;
            }
            "--recursive" | "-r" => {
                config.recursive = true;
                i += 1;
            }
            "--lowercase" | "-l" => {
                config.lowercase = true;
                i += 1;
            }
            "--verbose" | "-v" => {
                config.verbose = true;
                i += 1;
            }
            "--mode" => {
                if i + 1 >= args.len() {
                    eprintln!("error: --mode requires a value");
                    process::exit(1);
                }
                config.mode = match args[i + 1].as_str() {
                    "conservative" | "con" => SanitizeMode::Conservative,
                    "strict" | "str" => SanitizeMode::Strict,
                    "windows" | "win" => SanitizeMode::Windows,
                    "minimal" | "min" => SanitizeMode::Minimal,
                    other => {
                        eprintln!("error: unknown mode: {other}");
                        process::exit(1);
                    }
                };
                i += 2;
            }
            "--max-len" => {
                if i + 1 >= args.len() {
                    eprintln!("error: --max-len requires a value");
                    process::exit(1);
                }
                config.max_length = args[i + 1].parse().unwrap_or(200);
                i += 2;
            }
            "--replace" => {
                if i + 2 >= args.len() {
                    eprintln!("error: --replace requires two arguments: <char> <replacement>");
                    process::exit(1);
                }
                let from_str = &args[i + 1];
                let to_str = args[i + 2].clone();
                if let Some(ch) = from_str.chars().next() {
                    config.custom_replacements.push((ch, to_str));
                }
                i += 3;
            }
            "--strip" => {
                if i + 1 >= args.len() {
                    eprintln!("error: --strip requires a character list");
                    process::exit(1);
                }
                config.strip_chars.extend(args[i + 1].chars());
                i += 2;
            }
            "--help" | "-h" | "help" => {
                print_usage();
                process::exit(0);
            }
            other => {
                paths.push(other.to_string());
                i += 1;
            }
        }
    }

    if paths.is_empty() {
        eprintln!("error: no paths specified");
        eprintln!("Run 'sanitize --help' for usage.");
        process::exit(1);
    }

    if config.dry_run {
        println!("(dry run — no files will be renamed)");
    }

    let mut stats = Stats {
        scanned: 0,
        renamed: 0,
        skipped: 0,
        errors: 0,
    };

    for path_str in &paths {
        let path = Path::new(path_str);
        if !path.exists() {
            eprintln!("  error: {path_str} does not exist");
            stats.errors += 1;
            continue;
        }

        if path.is_dir() && !config.recursive {
            // Process contents of directory (one level).
            if let Ok(read_dir) = fs::read_dir(path) {
                for entry in read_dir.flatten() {
                    process_single(&entry.path(), &config, &mut stats);
                }
            }
        } else {
            process_path(path, &config, &mut stats);
        }
    }

    println!();
    println!("Scanned: {}, Renamed: {}, Skipped: {}, Errors: {}",
        stats.scanned, stats.renamed, stats.skipped, stats.errors);

    if config.dry_run && stats.renamed > 0 {
        println!("(dry run — run without -n to apply changes)");
    }
}
