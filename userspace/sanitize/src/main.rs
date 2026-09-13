//! Slate OS Filename Sanitizer
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
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
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
    result = result
        .trim_matches(|c: char| c == '.' || c == ' ')
        .to_string();

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
            result = if max_base == 0 {
                // The extension alone is longer than the limit. Keeping it
                // whole and calling the result truncated would return a name
                // *over* the limit, which is the one thing this branch is
                // for.
                truncate_chars(&result, config.max_length)
            } else {
                format!("{}{}", truncate_chars(&result[..dot_pos], max_base), ext)
            };
        } else {
            result = truncate_chars(&result, config.max_length);
        }
    }

    // If result is empty after all transformations, use a fallback.
    if result.is_empty() {
        result = "unnamed".to_string();
    }

    result
}

/// At most `max` **bytes**, never splitting a character.
///
/// # Why this is not `String::truncate`
///
/// It was, and `String::truncate` panics when the index is not a UTF-8
/// character boundary. So did `&result[..max_base]` beside it. This program
/// exists to clean up awkward file names, and a name with non-ASCII in it is
/// squarely awkward -- `sanitize --max-len 5` on a file called `€€€€` took
/// the whole program down with
/// `assertion failed: self.is_char_boundary(new_len)`.
///
/// The limit stays a byte count rather than becoming a character count,
/// because that is what a filesystem's name limit is.
fn truncate_chars(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut end = max;
    while end > 0 && !s.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    s.get(..end).unwrap_or("").to_string()
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

        println!(
            "  rename: {file_name} → {}",
            final_path.file_name().unwrap_or_default().to_string_lossy()
        );

        if !config.dry_run
            && let Err(e) = fs::rename(path, &final_path)
        {
            eprintln!("    error: {e}");
            stats.errors += 1;
            return;
        }
    } else {
        println!("  rename: {file_name} → {sanitized}");

        if !config.dry_run
            && let Err(e) = fs::rename(path, &new_path)
        {
            eprintln!("    error: {e}");
            stats.errors += 1;
            return;
        }
    }

    stats.renamed += 1;
}

fn split_name_ext(name: &str) -> (String, String) {
    if let Some(dot_pos) = name.rfind('.')
        && dot_pos > 0
    {
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

/// What a command line asked for.
///
/// Returned rather than acted on, so a test can see a refusal. The parse
/// used to live inside `main` reading `env::args()`, which made the
/// destructive case -- a mistyped `--dry-run` -- reachable only by running
/// the binary.
#[derive(Debug)]
enum Parsed {
    /// Run with this configuration over these paths.
    Run(Config, Vec<String>),
    /// `--help`: print usage, succeed.
    Usage,
    /// Refuse, printing this and exiting 1.
    Error(String),
}

fn parse_args(args: &[String]) -> Parsed {
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
                    return Parsed::Error("error: --mode requires a value".to_string());
                }
                config.mode = match args[i + 1].as_str() {
                    "conservative" | "con" => SanitizeMode::Conservative,
                    "strict" | "str" => SanitizeMode::Strict,
                    "windows" | "win" => SanitizeMode::Windows,
                    "minimal" | "min" => SanitizeMode::Minimal,
                    other => {
                        return Parsed::Error(format!("error: unknown mode: {other}"));
                    }
                };
                i += 2;
            }
            "--max-len" => {
                if i + 1 >= args.len() {
                    return Parsed::Error("error: --max-len requires a value".to_string());
                }
                config.max_length = args[i + 1].parse().unwrap_or(200);
                i += 2;
            }
            "--replace" => {
                if i + 2 >= args.len() {
                    return Parsed::Error(
                        "error: --replace requires two arguments: <char> <replacement>".to_string(),
                    );
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
                    return Parsed::Error("error: --strip requires a character list".to_string());
                }
                config.strip_chars.extend(args[i + 1].chars());
                i += 2;
            }
            "--help" | "-h" | "help" => {
                return Parsed::Usage;
            }
            // Everything after `--` is a path, however it is spelled. This
            // program exists to rename files with awkward names, so a file
            // called `-n` is squarely within its remit and it needs a way to
            // be handed one.
            "--" => {
                i += 1;
                while i < args.len() {
                    paths.push(args[i].clone());
                    i += 1;
                }
            }
            // An unrecognised option used to become a *path to rename*.
            //
            // That is not merely untidy here. `sanitize --dry-runn DIR`
            // would fail to set dry-run, add the typo as a path that does
            // not exist, and then **rename every file under DIR** -- the
            // user asked for a preview and got the real thing. A renaming
            // tool cannot treat a mistyped flag as an operand.
            other if other.starts_with('-') && other.len() > 1 => {
                return Parsed::Error(format!(
                    "sanitize: {}
Run 'sanitize --help' for usage.",
                    usageerror::unknown_option(other.as_bytes())
                ));
            }
            other => {
                paths.push(other.to_string());
                i += 1;
            }
        }
    }

    Parsed::Run(config, paths)
}

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        print_usage();
        process::exit(0);
    }

    let (config, paths) = match parse_args(&args) {
        Parsed::Run(config, paths) => (config, paths),
        Parsed::Usage => {
            print_usage();
            process::exit(0);
        }
        Parsed::Error(message) => {
            eprintln!("{message}");
            process::exit(1);
        }
    };

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
    println!(
        "Scanned: {}, Renamed: {}, Skipped: {}, Errors: {}",
        stats.scanned, stats.renamed, stats.skipped, stats.errors
    );

    if config.dry_run && stats.renamed > 0 {
        println!("(dry run — run without -n to apply changes)");
    }

    // The count was already kept and already printed -- "Errors: 1" on stdout
    // -- and then thrown away, so `sanitize /nonexistent` reported the error
    // twice and exited 0 both times. Nothing was missing but the last step.
    if stats.errors > 0 {
        process::exit(1);
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ── the command line, now reachable ──

    fn argv(words: &[&str]) -> Vec<String> {
        std::iter::once("sanitize".to_string())
            .chain(words.iter().map(|w| (*w).to_string()))
            .collect()
    }

    /// The destructive case. Before the refusal, this set no dry-run flag,
    /// added `--dry-runn` as a path, and renamed everything under `docs`.
    #[test]
    fn a_mistyped_flag_is_refused_and_never_becomes_a_path() {
        match parse_args(&argv(&["--dry-runn", "docs"])) {
            Parsed::Error(msg) => {
                assert!(msg.contains("unrecognized option"), "{msg}");
                assert!(msg.contains("--dry-runn"), "{msg}");
            }
            other => panic!("expected a refusal, got {other:?}"),
        }
    }

    #[test]
    fn the_correct_flag_still_sets_dry_run() {
        match parse_args(&argv(&["--dry-run", "docs"])) {
            Parsed::Run(config, paths) => {
                assert!(config.dry_run);
                assert_eq!(paths, vec!["docs"]);
            }
            other => panic!("expected a run, got {other:?}"),
        }
    }

    /// This program exists to rename awkward names, so it must be able to
    /// accept one that begins with a dash.
    #[test]
    fn double_dash_hands_over_a_dashed_filename() {
        match parse_args(&argv(&["--", "-n", "--dry-run"])) {
            Parsed::Run(config, paths) => {
                assert!(!config.dry_run, "words after -- are names, not flags");
                assert_eq!(paths, vec!["-n", "--dry-run"]);
            }
            other => panic!("expected a run, got {other:?}"),
        }
    }

    #[test]
    fn help_is_a_request_not_an_error() {
        assert!(matches!(parse_args(&argv(&["--help"])), Parsed::Usage));
    }

    #[test]
    fn a_bad_mode_and_a_missing_value_are_both_refused() {
        assert!(matches!(
            parse_args(&argv(&["--mode", "sideways", "d"])),
            Parsed::Error(_)
        ));
        assert!(matches!(
            parse_args(&argv(&["--max-len"])),
            Parsed::Error(_)
        ));
    }

    /// The crash this crate shipped with. `String::truncate` and `&s[..n]`
    /// panic when `n` is not a UTF-8 character boundary, and this program
    /// exists to clean up awkward names -- non-ASCII is squarely awkward.
    /// `sanitize --max-len 5` on a file called `€€€€` took the whole program
    /// down with `assertion failed: self.is_char_boundary(new_len)`.
    #[test]
    fn truncation_never_splits_a_character() {
        // Three bytes each, so a limit of 5 lands mid-character.
        assert_eq!(truncate_chars("€€€€", 5), "€");
        assert_eq!(truncate_chars("€€€€", 6), "€€");
        // A limit below the first character yields nothing rather than
        // half of one.
        assert_eq!(truncate_chars("€", 1), "");
        assert_eq!(truncate_chars("€", 2), "");
        assert_eq!(truncate_chars("€", 3), "€");
    }

    #[test]
    fn truncation_leaves_a_short_name_alone() {
        assert_eq!(truncate_chars("short.txt", 200), "short.txt");
        assert_eq!(truncate_chars("", 5), "");
    }

    /// The limit is a byte count, because that is what a filesystem's name
    /// limit is -- four euro signs are 4 characters and 12 bytes.
    #[test]
    fn the_limit_counts_bytes_not_characters() {
        assert_eq!("€€€€".chars().count(), 4);
        assert_eq!("€€€€".len(), 12);
        assert!(truncate_chars("€€€€", 7).len() <= 7);
    }

    #[test]
    fn an_extension_is_kept_when_the_base_is_cut() {
        let cfg = Config {
            max_length: 12,
            ..Config::default_config()
        };
        let out = sanitize_name("averylongbasename.txt", &cfg);
        assert!(out.len() <= 12, "{out}");
        assert!(out.ends_with(".txt"), "{out}");
    }

    /// An extension longer than the whole limit used to produce a name
    /// *over* the limit, which is the one thing the truncation branch is
    /// for.
    #[test]
    fn an_over_long_extension_still_respects_the_limit() {
        let cfg = Config {
            max_length: 4,
            ..Config::default_config()
        };
        let out = sanitize_name("a.averylongextension", &cfg);
        assert!(out.len() <= 4, "{out}");
    }

    #[test]
    fn a_name_with_spaces_is_the_ordinary_case() {
        let cfg = Config::default_config();
        assert_eq!(sanitize_name("my file .txt", &cfg), "my_file_.txt");
    }

    #[test]
    fn splitting_a_name_finds_the_last_dot_only() {
        assert_eq!(
            split_name_ext("archive.tar.gz"),
            ("archive.tar".to_string(), "gz".to_string())
        );
        // A leading dot is not an extension separator.
        assert_eq!(
            split_name_ext(".hidden"),
            (".hidden".to_string(), String::new())
        );
        assert_eq!(
            split_name_ext("noext"),
            ("noext".to_string(), String::new())
        );
    }

    #[test]
    fn repeats_collapse_to_one() {
        assert_eq!(collapse_repeats("a___b", '_'), "a_b");
        assert_eq!(collapse_repeats("___", '_'), "_");
        assert_eq!(collapse_repeats("ab", '_'), "ab");
    }
}
