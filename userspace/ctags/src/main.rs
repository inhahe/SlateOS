//! OurOS `ctags` -- Code Tag Generator
//!
//! A multi-personality tag generator compatible with Exuberant Ctags and
//! Emacs etags.  When invoked as `ctags` (the default) it produces a sorted
//! tag file suitable for Vi/Vim.  When invoked as `etags` it produces a
//! TAGS file in the Emacs format.
//!
//! # Supported languages
//!
//! C, C++, Rust, Python, Java, JavaScript/TypeScript, Go, Shell (Bash/sh).
//!
//! # Tag kinds recognised
//!
//! Functions, structs/classes, enums, typedefs/type aliases, macros/defines,
//! global variables/constants, interfaces, traits, methods, modules/packages.
//!
//! # Usage
//!
//! ```text
//! ctags [OPTIONS] [FILE]...
//!
//!   -R, --recurse          Recurse into directories
//!   -f TAGFILE             Write tags to TAGFILE (default: "tags" / "TAGS")
//!   -o TAGFILE             Synonym for -f
//!   -a, --append           Append to tag file instead of overwriting
//!   -e, --etags            Produce Emacs TAGS output (auto when invoked as etags)
//!   -u, --sort=no          Unsorted output
//!       --sort=yes         Sorted output (default for ctags)
//!       --sort=foldcase    Case-insensitive sorted output
//!       --exclude=PATTERN  Exclude files matching glob PATTERN
//!       --fields=FLAGS     Include extra fields (afmikKlnsStz)
//!       --extras=FLAGS     Include extra tag entries (+q for qualified tags)
//!       --help             Display this help and exit
//!       --version          Output version information and exit
//! ```

#![cfg_attr(not(test), no_main)]

use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::io::{self, BufRead, BufReader, Write};
use std::path::Path;

// ============================================================================
// Constants
// ============================================================================

const VERSION: &str = "0.1.0";
const PROGRAM_ETAGS: &str = "etags";

// ============================================================================
// Language / tag kind enums
// ============================================================================

/// Programming languages we can parse.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Language {
    C,
    Cpp,
    Rust,
    Python,
    Java,
    JavaScript,
    Go,
    Shell,
}

/// The kind of source entity a tag represents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TagKind {
    Function,
    Struct,
    Class,
    Enum,
    Typedef,
    Macro,
    Variable,
    Interface,
    Trait,
    Method,
    Module,
    Constant,
}

impl TagKind {
    /// One-letter abbreviation used in the `kind` field of ctags output.
    fn letter(self) -> char {
        match self {
            Self::Function => 'f',
            Self::Struct => 's',
            Self::Class => 'c',
            Self::Enum => 'g',
            Self::Typedef => 't',
            Self::Macro => 'd',
            Self::Variable => 'v',
            Self::Interface => 'i',
            Self::Trait => 'i', // traits are interface-like
            Self::Method => 'f',
            Self::Module => 'n',
            Self::Constant => 'v',
        }
    }

    /// Human-readable name for extended fields.
    #[cfg_attr(not(test), allow(dead_code))]
    fn name(self) -> &'static str {
        match self {
            Self::Function => "function",
            Self::Struct => "struct",
            Self::Class => "class",
            Self::Enum => "enum",
            Self::Typedef => "typedef",
            Self::Macro => "macro",
            Self::Variable => "variable",
            Self::Interface => "interface",
            Self::Trait => "trait",
            Self::Method => "method",
            Self::Module => "module",
            Self::Constant => "constant",
        }
    }
}

impl Language {
    /// Human-readable name.
    fn name(self) -> &'static str {
        match self {
            Self::C => "C",
            Self::Cpp => "C++",
            Self::Rust => "Rust",
            Self::Python => "Python",
            Self::Java => "Java",
            Self::JavaScript => "JavaScript",
            Self::Go => "Go",
            Self::Shell => "Shell",
        }
    }
}

// ============================================================================
// Tag entry
// ============================================================================

/// A single tag entry extracted from a source file.
#[derive(Debug, Clone)]
struct Tag {
    name: String,
    file: String,
    line_number: usize,
    pattern: String,
    kind: TagKind,
    language: Language,
    scope: Option<String>,
}

// ============================================================================
// Output format
// ============================================================================

/// Whether to produce ctags (Vi) or etags (Emacs) output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OutputFormat {
    Ctags,
    Etags,
}

/// Sort mode for ctags output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SortMode {
    Yes,
    No,
    Foldcase,
}

// ============================================================================
// Configuration
// ============================================================================

/// Parsed command-line configuration.
#[derive(Debug, Clone)]
struct Config {
    files: Vec<String>,
    recurse: bool,
    output_file: Option<String>,
    append: bool,
    format: OutputFormat,
    sort: SortMode,
    exclude_patterns: Vec<String>,
    /// Which extended fields to include (a subset of "afmikKlnsStz").
    fields: String,
    /// Extra tag entries (currently just `q` for qualified tags).
    extras: String,
}

impl Config {
    fn new() -> Self {
        Self {
            files: Vec::new(),
            recurse: false,
            output_file: None,
            append: false,
            format: OutputFormat::Ctags,
            sort: SortMode::Yes,
            exclude_patterns: Vec::new(),
            fields: String::from("fks"),
            extras: String::new(),
        }
    }
}

// ============================================================================
// Argument parsing
// ============================================================================

enum ParseResult {
    Help,
    Version,
    Run(Config),
}

fn parse_args(args: &[String]) -> ParseResult {
    let mut config = Config::new();

    // Detect personality from argv[0].
    if let Some(prog) = args.first() {
        let base = Path::new(prog)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("");
        if base == PROGRAM_ETAGS || base.ends_with("etags") {
            config.format = OutputFormat::Etags;
        }
    }

    let mut i = 1;
    while i < args.len() {
        let arg = &args[i];
        match arg.as_str() {
            "--help" | "-h" => return ParseResult::Help,
            "--version" => return ParseResult::Version,
            "-R" | "--recurse" => config.recurse = true,
            "-a" | "--append" => config.append = true,
            "-e" | "--etags" => config.format = OutputFormat::Etags,
            "-f" | "-o" => {
                i += 1;
                if i < args.len() {
                    config.output_file = Some(args[i].clone());
                } else {
                    eprintln!("ctags: option '{arg}' requires an argument");
                    std::process::exit(1);
                }
            }
            "-u" => config.sort = SortMode::No,
            _ if arg.starts_with("--sort=") => {
                let val = &arg["--sort=".len()..];
                config.sort = match val {
                    "yes" | "1" => SortMode::Yes,
                    "no" | "0" => SortMode::No,
                    "foldcase" => SortMode::Foldcase,
                    _ => {
                        eprintln!("ctags: unknown sort mode '{val}'");
                        std::process::exit(1);
                    }
                };
            }
            _ if arg.starts_with("--exclude=") => {
                let pat = arg["--exclude=".len()..].to_string();
                config.exclude_patterns.push(pat);
            }
            _ if arg.starts_with("--fields=") => {
                config.fields = arg["--fields=".len()..].to_string();
            }
            _ if arg.starts_with("--extras=") || arg.starts_with("--extra=") => {
                let eq = arg.find('=').unwrap_or(arg.len());
                config.extras = arg[eq + 1..].to_string();
            }
            _ if arg.starts_with('-') && arg != "-" => {
                // Handle combined short flags like -Rf or -af
                let chars: Vec<char> = arg[1..].chars().collect();
                let mut j = 0;
                while j < chars.len() {
                    match chars[j] {
                        'R' => config.recurse = true,
                        'a' => config.append = true,
                        'e' => config.format = OutputFormat::Etags,
                        'u' => config.sort = SortMode::No,
                        'f' | 'o' => {
                            // Rest of this arg (if any) is the filename;
                            // otherwise next arg is.
                            let rest: String = chars[j + 1..].iter().collect();
                            if !rest.is_empty() {
                                config.output_file = Some(rest);
                            } else {
                                i += 1;
                                if i < args.len() {
                                    config.output_file = Some(args[i].clone());
                                } else {
                                    eprintln!("ctags: option '-f' requires an argument");
                                    std::process::exit(1);
                                }
                            }
                            // Consumed rest of combined arg.
                            j = chars.len();
                            continue;
                        }
                        c => {
                            eprintln!("ctags: unknown option '-{c}'");
                            std::process::exit(1);
                        }
                    }
                    j += 1;
                }
            }
            _ => {
                config.files.push(arg.clone());
            }
        }
        i += 1;
    }

    // Default: read from stdin if no files given and not recursive.
    if config.files.is_empty() && !config.recurse {
        config.files.push("-".to_string());
    }

    ParseResult::Run(config)
}

// ============================================================================
// Language detection
// ============================================================================

/// Determine language from file extension.
fn detect_language(path: &str) -> Option<Language> {
    let ext = Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "c" | "h" => Some(Language::C),
        "cpp" | "cxx" | "cc" | "c++" | "hpp" | "hxx" | "hh" | "h++" => Some(Language::Cpp),
        "rs" => Some(Language::Rust),
        "py" | "pyw" | "pyi" => Some(Language::Python),
        "java" => Some(Language::Java),
        "js" | "jsx" | "ts" | "tsx" | "mjs" | "cjs" => Some(Language::JavaScript),
        "go" => Some(Language::Go),
        "sh" | "bash" | "zsh" | "ksh" | "csh" => Some(Language::Shell),
        _ => None,
    }
}

// ============================================================================
// File collection (recursion + exclusion)
// ============================================================================

/// Collect files to scan, applying recursion and exclusion rules.
fn collect_files(config: &Config) -> Vec<String> {
    let mut result = Vec::new();

    if config.recurse && config.files.is_empty() {
        // Default: recurse from current directory.
        collect_dir(Path::new("."), &config.exclude_patterns, &mut result);
        return result;
    }

    for f in &config.files {
        if f == "-" {
            result.push("-".to_string());
            continue;
        }
        let p = Path::new(f);
        if p.is_dir() && config.recurse {
            collect_dir(p, &config.exclude_patterns, &mut result);
        } else if p.is_file() {
            if !is_excluded(f, &config.exclude_patterns)
                && detect_language(f).is_some() {
                    result.push(f.clone());
                }
        } else if !p.exists() {
            eprintln!("ctags: cannot open '{}': No such file or directory", f);
        }
    }

    result
}

/// Recursively collect files from a directory.
fn collect_dir(dir: &Path, excludes: &[String], out: &mut Vec<String>) {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) => {
            eprintln!(
                "ctags: cannot read directory '{}': {}",
                dir.display(),
                e
            );
            return;
        }
    };

    // Sort entries for deterministic output.
    let mut entries_vec: Vec<_> = entries.filter_map(|e| e.ok()).collect();
    entries_vec.sort_by_key(|e| e.file_name());

    for entry in entries_vec {
        let path = entry.path();
        let path_str = path.to_string_lossy().to_string();

        // Normalise path separators to forward slash.
        let path_str = path_str.replace('\\', "/");

        if is_excluded(&path_str, excludes) {
            continue;
        }

        if path.is_dir() {
            // Skip hidden directories.
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if name_str.starts_with('.') {
                continue;
            }
            collect_dir(&path, excludes, out);
        } else if path.is_file()
            && detect_language(&path_str).is_some() {
                out.push(path_str);
            }
    }
}

/// Check whether a path matches any exclusion pattern.
/// Supports simple glob patterns: `*` matches any sequence of non-`/` chars,
/// `?` matches any single non-`/` char, and bare names match anywhere in path.
fn is_excluded(path: &str, patterns: &[String]) -> bool {
    for pat in patterns {
        if glob_match(pat, path) {
            return true;
        }
    }
    false
}

/// Minimal glob matcher sufficient for ctags `--exclude` patterns.
fn glob_match(pattern: &str, text: &str) -> bool {
    if !pattern.contains('/') {
        // Match against the basename first.
        let basename = Path::new(text)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(text);
        if glob_match_impl(pattern.as_bytes(), basename.as_bytes()) {
            return true;
        }
        // Also match against each path component (to catch directory names
        // like `node_modules` in `node_modules/foo.js`).
        for component in Path::new(text).components() {
            if let Some(s) = component.as_os_str().to_str()
                && glob_match_impl(pattern.as_bytes(), s.as_bytes()) {
                    return true;
                }
        }
        return false;
    }

    glob_match_impl(pattern.as_bytes(), text.as_bytes())
}

fn glob_match_impl(pat: &[u8], txt: &[u8]) -> bool {
    let mut pi = 0;
    let mut ti = 0;
    let mut star_pi = usize::MAX;
    let mut star_ti = 0;

    while ti < txt.len() {
        if pi < pat.len() && pat[pi] == b'*' {
            star_pi = pi;
            star_ti = ti;
            pi += 1;
            continue;
        }
        if pi < pat.len()
            && (pat[pi] == b'?' || pat[pi] == txt[ti])
        {
            pi += 1;
            ti += 1;
            continue;
        }
        if star_pi != usize::MAX {
            pi = star_pi + 1;
            star_ti += 1;
            ti = star_ti;
            continue;
        }
        return false;
    }

    while pi < pat.len() && pat[pi] == b'*' {
        pi += 1;
    }
    pi == pat.len()
}

// ============================================================================
// Tag extraction - generic helpers
// ============================================================================

/// Remove leading/trailing whitespace and build a vi search pattern from a
/// source line.  The pattern uses a fixed-string search wrapped in `/^..$/`.
fn make_pattern(line: &str) -> String {
    let trimmed = line.trim_end();
    // Escape characters that are special in vi regex.
    let mut escaped = String::with_capacity(trimmed.len() + 4);
    escaped.push_str("/^");
    for c in trimmed.chars() {
        match c {
            '/' | '\\' => {
                escaped.push('\\');
                escaped.push(c);
            }
            _ => escaped.push(c),
        }
    }
    escaped.push_str("$/");
    escaped
}

/// Strip a single pair of balanced quotes from a string value.
#[cfg_attr(not(test), allow(dead_code))]
fn strip_quotes(s: &str) -> &str {
    let s = s.trim();
    if (s.starts_with('"') && s.ends_with('"'))
        || (s.starts_with('\'') && s.ends_with('\''))
    {
        &s[1..s.len() - 1]
    } else {
        s
    }
}

/// Extract an identifier starting at `pos` in `chars`.
fn extract_ident(chars: &[char], pos: usize) -> String {
    let mut end = pos;
    while end < chars.len()
        && (chars[end].is_alphanumeric() || chars[end] == '_')
    {
        end += 1;
    }
    chars[pos..end].iter().collect()
}

/// Skip whitespace in `chars` starting at `pos`, returning the new position.
fn skip_ws(chars: &[char], pos: usize) -> usize {
    let mut p = pos;
    while p < chars.len() && chars[p].is_whitespace() {
        p += 1;
    }
    p
}

/// Check if `line` stripped of leading whitespace starts with `prefix`,
/// returning the rest of the line after the prefix and any whitespace.
fn line_after_keyword(line: &str, keyword: &str) -> Option<String> {
    let trimmed = line.trim_start();
    if let Some(rest) = trimmed.strip_prefix(keyword) {
        // Must be followed by whitespace or certain punctuation.
        if rest.is_empty() {
            return Some(String::new());
        }
        let first = rest.as_bytes()[0];
        if first == b' ' || first == b'\t' || first == b'(' || first == b'<' || first == b'{' {
            return Some(rest.trim_start().to_string());
        }
    }
    None
}

/// Extract the first identifier from `text`.
fn first_ident(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let start = skip_ws(&chars, 0);
    // Allow leading * for pointer declarations in C.
    let start = if start < chars.len() && chars[start] == '*' {
        skip_ws(&chars, start + 1)
    } else {
        start
    };
    extract_ident(&chars, start)
}

// ============================================================================
// Per-language tag extractors
// ============================================================================

/// Extract tags from C source code.
fn extract_c_tags(content: &str, file: &str, language: Language) -> Vec<Tag> {
    let mut tags = Vec::new();
    let lines: Vec<&str> = content.lines().collect();
    let mut in_block_comment = false;

    for (idx, line) in lines.iter().enumerate() {
        let line_num = idx + 1;
        let trimmed = line.trim();

        // Track block comments.
        if in_block_comment {
            if trimmed.contains("*/") {
                in_block_comment = false;
            }
            continue;
        }
        if trimmed.starts_with("/*") && !trimmed.contains("*/") {
            in_block_comment = true;
            continue;
        }
        if trimmed.starts_with("//") {
            continue;
        }

        // #define MACRO
        if trimmed.starts_with("#define")
            && let Some(rest) = line_after_keyword(trimmed, "#define") {
                let name = first_ident(&rest);
                if !name.is_empty() {
                    tags.push(Tag {
                        name,
                        file: file.to_string(),
                        line_number: line_num,
                        pattern: make_pattern(line),
                        kind: TagKind::Macro,
                        language,
                        scope: None,
                    });
                }
            }

        // typedef ... NAME;
        if trimmed.starts_with("typedef") {
            // Look for the last identifier before the semicolon.
            if let Some(semi) = trimmed.rfind(';') {
                let before = trimmed[..semi].trim();
                let name = before
                    .rsplit(|c: char| !c.is_alphanumeric() && c != '_')
                    .next()
                    .unwrap_or("");
                if !name.is_empty() && name != "typedef" {
                    tags.push(Tag {
                        name: name.to_string(),
                        file: file.to_string(),
                        line_number: line_num,
                        pattern: make_pattern(line),
                        kind: TagKind::Typedef,
                        language,
                        scope: None,
                    });
                }
            }
        }

        // struct NAME / union NAME / enum NAME
        for keyword in &["struct", "union", "enum"] {
            if let Some(rest) = line_after_keyword(trimmed, keyword) {
                let name = first_ident(&rest);
                if !name.is_empty() && name != "{" {
                    let kind = if *keyword == "enum" {
                        TagKind::Enum
                    } else {
                        TagKind::Struct
                    };
                    tags.push(Tag {
                        name,
                        file: file.to_string(),
                        line_number: line_num,
                        pattern: make_pattern(line),
                        kind,
                        language,
                        scope: None,
                    });
                }
            }
        }

        // C++ class / namespace
        if language == Language::Cpp {
            if let Some(rest) = line_after_keyword(trimmed, "class") {
                let name = first_ident(&rest);
                if !name.is_empty() {
                    tags.push(Tag {
                        name,
                        file: file.to_string(),
                        line_number: line_num,
                        pattern: make_pattern(line),
                        kind: TagKind::Class,
                        language,
                        scope: None,
                    });
                }
            }
            if let Some(rest) = line_after_keyword(trimmed, "namespace") {
                let name = first_ident(&rest);
                if !name.is_empty() {
                    tags.push(Tag {
                        name,
                        file: file.to_string(),
                        line_number: line_num,
                        pattern: make_pattern(line),
                        kind: TagKind::Module,
                        language,
                        scope: None,
                    });
                }
            }
        }

        // Function-like: TYPE NAME(...) {  or  TYPE NAME(...)
        // Heuristic: line contains `(`, identifier before `(`, and either `{`
        // on this line or the next, or `)` followed by `;` (declaration) is
        // excluded because we want definitions.
        if let Some(paren) = trimmed.find('(')
            && paren > 0
                && !trimmed.starts_with('#')
                && !trimmed.starts_with("typedef")
                && !trimmed.starts_with("//")
                && !trimmed.starts_with("if")
                && !trimmed.starts_with("while")
                && !trimmed.starts_with("for")
                && !trimmed.starts_with("switch")
                && !trimmed.starts_with("return")
            {
                let before_paren = trimmed[..paren].trim();
                // The function name is the last identifier before `(`.
                let name = before_paren
                    .rsplit(|c: char| !c.is_alphanumeric() && c != '_')
                    .next()
                    .unwrap_or("");
                if !name.is_empty()
                    && name != "struct"
                    && name != "enum"
                    && name != "union"
                    && name != "class"
                    && name != "namespace"
                    && name != "define"
                    && name != "if"
                    && name != "while"
                    && name != "for"
                    && name != "switch"
                {
                    // Check that this looks like a definition (has `{`
                    // somewhere nearby, not just a declaration ending in `;`).
                    let has_brace = trimmed.contains('{')
                        || lines
                            .get(idx + 1)
                            .map(|l| l.trim().starts_with('{'))
                            .unwrap_or(false);
                    // Also accept lines that just have `)` (K&R style or
                    // multi-line params) — only if next lines have `{`.
                    let has_brace = has_brace
                        || lines
                            .get(idx + 2)
                            .map(|l| l.trim().starts_with('{'))
                            .unwrap_or(false);

                    if has_brace && !trimmed.ends_with(';') {
                        tags.push(Tag {
                            name: name.to_string(),
                            file: file.to_string(),
                            line_number: line_num,
                            pattern: make_pattern(line),
                            kind: TagKind::Function,
                            language,
                            scope: None,
                        });
                    }
                }
            }
    }

    tags
}

/// Extract tags from Rust source code.
fn extract_rust_tags(content: &str, file: &str) -> Vec<Tag> {
    let mut tags = Vec::new();
    let lines: Vec<&str> = content.lines().collect();
    let mut in_block_comment = false;
    let mut current_scope: Option<String> = None;

    for (idx, line) in lines.iter().enumerate() {
        let line_num = idx + 1;
        let trimmed = line.trim();

        // Track block comments.
        if in_block_comment {
            if trimmed.contains("*/") {
                in_block_comment = false;
            }
            continue;
        }
        if trimmed.starts_with("/*") && !trimmed.contains("*/") {
            in_block_comment = true;
            continue;
        }
        if trimmed.starts_with("//") {
            continue;
        }

        // Strip visibility modifiers and attributes for keyword detection.
        let stripped = trimmed
            .trim_start_matches("pub(crate) ")
            .trim_start_matches("pub(super) ")
            .trim_start_matches("pub ")
            .trim_start_matches("async ")
            .trim_start_matches("const ")
            .trim_start_matches("unsafe ");

        // fn name / fn name<T>
        if let Some(rest) = line_after_keyword(stripped, "fn") {
            let name = first_ident(&rest);
            if !name.is_empty() {
                tags.push(Tag {
                    name,
                    file: file.to_string(),
                    line_number: line_num,
                    pattern: make_pattern(line),
                    kind: if current_scope.is_some() {
                        TagKind::Method
                    } else {
                        TagKind::Function
                    },
                    language: Language::Rust,
                    scope: current_scope.clone(),
                });
            }
        }

        // struct Name
        if let Some(rest) = line_after_keyword(stripped, "struct") {
            let name = first_ident(&rest);
            if !name.is_empty() {
                tags.push(Tag {
                    name,
                    file: file.to_string(),
                    line_number: line_num,
                    pattern: make_pattern(line),
                    kind: TagKind::Struct,
                    language: Language::Rust,
                    scope: None,
                });
            }
        }

        // enum Name
        if let Some(rest) = line_after_keyword(stripped, "enum") {
            let name = first_ident(&rest);
            if !name.is_empty() {
                tags.push(Tag {
                    name,
                    file: file.to_string(),
                    line_number: line_num,
                    pattern: make_pattern(line),
                    kind: TagKind::Enum,
                    language: Language::Rust,
                    scope: None,
                });
            }
        }

        // trait Name
        if let Some(rest) = line_after_keyword(stripped, "trait") {
            let name = first_ident(&rest);
            if !name.is_empty() {
                tags.push(Tag {
                    name,
                    file: file.to_string(),
                    line_number: line_num,
                    pattern: make_pattern(line),
                    kind: TagKind::Trait,
                    language: Language::Rust,
                    scope: None,
                });
            }
        }

        // type Alias = ...
        if let Some(rest) = line_after_keyword(stripped, "type") {
            let name = first_ident(&rest);
            if !name.is_empty() && rest.contains('=') {
                tags.push(Tag {
                    name,
                    file: file.to_string(),
                    line_number: line_num,
                    pattern: make_pattern(line),
                    kind: TagKind::Typedef,
                    language: Language::Rust,
                    scope: current_scope.clone(),
                });
            }
        }

        // mod name
        if let Some(rest) = line_after_keyword(stripped, "mod") {
            let name = first_ident(&rest);
            if !name.is_empty() && name != "tests" {
                tags.push(Tag {
                    name,
                    file: file.to_string(),
                    line_number: line_num,
                    pattern: make_pattern(line),
                    kind: TagKind::Module,
                    language: Language::Rust,
                    scope: None,
                });
            }
        }

        // macro_rules! name
        if let Some(rest) = line_after_keyword(stripped, "macro_rules!") {
            let name = first_ident(&rest);
            if !name.is_empty() {
                tags.push(Tag {
                    name,
                    file: file.to_string(),
                    line_number: line_num,
                    pattern: make_pattern(line),
                    kind: TagKind::Macro,
                    language: Language::Rust,
                    scope: None,
                });
            }
        }

        // static / const at module level (no indentation).
        if (line.len() == trimmed.len() || line.starts_with("pub"))
            && let Some(rest) = line_after_keyword(stripped, "static") {
                let rest = rest.trim_start_matches("mut ");
                let name = first_ident(rest);
                if !name.is_empty() && name != "_" {
                    tags.push(Tag {
                        name,
                        file: file.to_string(),
                        line_number: line_num,
                        pattern: make_pattern(line),
                        kind: TagKind::Variable,
                        language: Language::Rust,
                        scope: None,
                    });
                }
            }

        // impl Name { ... } — track for method scope.
        if let Some(rest) = line_after_keyword(stripped, "impl") {
            let rest_clean = if let Some(pos) = rest.find(" for ") {
                &rest[pos + 5..]
            } else {
                &rest
            };
            // Strip generic parameters.
            let name_part = if let Some(angle) = rest_clean.find('<') {
                &rest_clean[..angle]
            } else {
                rest_clean
            };
            let name = first_ident(name_part);
            if !name.is_empty() {
                current_scope = Some(name);
            }
        }

        // Rough heuristic: if a line is `}` at column 0, we left the impl.
        if trimmed == "}" && line.starts_with('}') {
            current_scope = None;
        }
    }

    tags
}

/// Extract tags from Python source code.
fn extract_python_tags(content: &str, file: &str) -> Vec<Tag> {
    let mut tags = Vec::new();
    let lines: Vec<&str> = content.lines().collect();
    let mut current_class: Option<String> = None;

    for (idx, line) in lines.iter().enumerate() {
        let line_num = idx + 1;
        let trimmed = line.trim();

        if trimmed.starts_with('#') || trimmed.is_empty() {
            continue;
        }

        let indent = line.len() - line.trim_start().len();

        // class Name:  or  class Name(Base):
        if let Some(rest) = line_after_keyword(trimmed, "class") {
            let name_end = rest
                .find(['(', ':', ' '])
                .unwrap_or(rest.len());
            let name = rest[..name_end].trim().to_string();
            if !name.is_empty() {
                current_class = Some(name.clone());
                tags.push(Tag {
                    name,
                    file: file.to_string(),
                    line_number: line_num,
                    pattern: make_pattern(line),
                    kind: TagKind::Class,
                    language: Language::Python,
                    scope: None,
                });
            }
        }

        // def func_name(  or  async def func_name(
        let def_line = if let Some(rest) = line_after_keyword(trimmed, "def") {
            Some(rest)
        } else if let Some(rest) = line_after_keyword(trimmed, "async") {
            line_after_keyword(&rest, "def")
        } else {
            None
        };

        if let Some(rest) = def_line {
            let name_end = rest.find('(').unwrap_or(rest.len());
            let name = rest[..name_end].trim().to_string();
            if !name.is_empty() {
                let (kind, scope) = if indent > 0 && current_class.is_some() {
                    (TagKind::Method, current_class.clone())
                } else {
                    if indent == 0 {
                        current_class = None;
                    }
                    (TagKind::Function, None)
                };
                tags.push(Tag {
                    name,
                    file: file.to_string(),
                    line_number: line_num,
                    pattern: make_pattern(line),
                    kind,
                    language: Language::Python,
                    scope,
                });
            }
        }

        // Module-level assignments: NAME = ...  (simple constant detection)
        if indent == 0 && !trimmed.starts_with("def") && !trimmed.starts_with("class")
            && let Some(eq_pos) = trimmed.find('=')
                && eq_pos > 0
                    && !trimmed[..eq_pos].contains('(')
                    && !trimmed[..eq_pos].contains('[')
                    && trimmed.as_bytes()[eq_pos.saturating_sub(1)] != b'!'
                    && trimmed.as_bytes()[eq_pos.saturating_sub(1)] != b'<'
                    && trimmed.as_bytes()[eq_pos.saturating_sub(1)] != b'>'
                    && trimmed
                        .as_bytes()
                        .get(eq_pos + 1)
                        .is_none_or(|&b| b != b'=')
                {
                    let name = trimmed[..eq_pos].trim();
                    // Must look like a valid identifier (all caps or snake_case).
                    let looks_like_ident = !name.is_empty()
                        && name
                            .chars()
                            .all(|c| c.is_alphanumeric() || c == '_');
                    if looks_like_ident
                        && name.chars().next().is_some_and(|c| c.is_alphabetic() || c == '_')
                    {
                        let kind = if name.chars().all(|c| c.is_uppercase() || c == '_') {
                            TagKind::Constant
                        } else {
                            TagKind::Variable
                        };
                        tags.push(Tag {
                            name: name.to_string(),
                            file: file.to_string(),
                            line_number: line_num,
                            pattern: make_pattern(line),
                            kind,
                            language: Language::Python,
                            scope: None,
                        });
                    }
                }

        // Reset class scope on dedent to 0.
        if indent == 0
            && !trimmed.starts_with("class")
            && !trimmed.starts_with("def")
            && !trimmed.starts_with('@')
            && !trimmed.is_empty()
        {
            current_class = None;
        }
    }

    tags
}

/// Extract tags from Java source code.
fn extract_java_tags(content: &str, file: &str) -> Vec<Tag> {
    let mut tags = Vec::new();
    let lines: Vec<&str> = content.lines().collect();
    let mut in_block_comment = false;
    let mut current_class: Option<String> = None;

    for (idx, line) in lines.iter().enumerate() {
        let line_num = idx + 1;
        let trimmed = line.trim();

        if in_block_comment {
            if trimmed.contains("*/") {
                in_block_comment = false;
            }
            continue;
        }
        if trimmed.starts_with("/*") && !trimmed.contains("*/") {
            in_block_comment = true;
            continue;
        }
        if trimmed.starts_with("//") || trimmed.starts_with('@') {
            continue;
        }

        // Strip modifiers.
        let stripped = trimmed
            .replace("public ", "")
            .replace("private ", "")
            .replace("protected ", "")
            .replace("static ", "")
            .replace("final ", "")
            .replace("abstract ", "")
            .replace("synchronized ", "");
        let stripped = stripped.trim();

        // class / interface / enum
        if let Some(rest) = line_after_keyword(stripped, "class") {
            let name = first_ident(&rest);
            if !name.is_empty() {
                current_class = Some(name.clone());
                tags.push(Tag {
                    name,
                    file: file.to_string(),
                    line_number: line_num,
                    pattern: make_pattern(line),
                    kind: TagKind::Class,
                    language: Language::Java,
                    scope: None,
                });
            }
        }
        if let Some(rest) = line_after_keyword(stripped, "interface") {
            let name = first_ident(&rest);
            if !name.is_empty() {
                tags.push(Tag {
                    name,
                    file: file.to_string(),
                    line_number: line_num,
                    pattern: make_pattern(line),
                    kind: TagKind::Interface,
                    language: Language::Java,
                    scope: None,
                });
            }
        }
        if let Some(rest) = line_after_keyword(stripped, "enum") {
            let name = first_ident(&rest);
            if !name.is_empty() {
                tags.push(Tag {
                    name,
                    file: file.to_string(),
                    line_number: line_num,
                    pattern: make_pattern(line),
                    kind: TagKind::Enum,
                    language: Language::Java,
                    scope: None,
                });
            }
        }

        // Method: TYPE name(...)  inside a class
        if let Some(paren) = stripped.find('(')
            && paren > 0
                && !stripped.starts_with("if")
                && !stripped.starts_with("while")
                && !stripped.starts_with("for")
                && !stripped.starts_with("switch")
                && !stripped.starts_with("return")
                && !stripped.starts_with("class")
                && !stripped.starts_with("interface")
                && !stripped.starts_with("new")
            {
                let before_paren = stripped[..paren].trim();
                let name = before_paren
                    .rsplit(|c: char| !c.is_alphanumeric() && c != '_')
                    .next()
                    .unwrap_or("");
                if !name.is_empty()
                    && name != "if"
                    && name != "while"
                    && name != "for"
                    && name != "catch"
                {
                    let has_brace = trimmed.contains('{')
                        || lines
                            .get(idx + 1)
                            .map(|l| l.trim().starts_with('{'))
                            .unwrap_or(false);
                    if has_brace && !trimmed.ends_with(';') {
                        tags.push(Tag {
                            name: name.to_string(),
                            file: file.to_string(),
                            line_number: line_num,
                            pattern: make_pattern(line),
                            kind: if current_class.is_some() {
                                TagKind::Method
                            } else {
                                TagKind::Function
                            },
                            language: Language::Java,
                            scope: current_class.clone(),
                        });
                    }
                }
            }

        // package declaration
        if let Some(rest) = line_after_keyword(trimmed, "package") {
            let name = rest.trim_end_matches(';').trim().to_string();
            if !name.is_empty() {
                tags.push(Tag {
                    name,
                    file: file.to_string(),
                    line_number: line_num,
                    pattern: make_pattern(line),
                    kind: TagKind::Module,
                    language: Language::Java,
                    scope: None,
                });
            }
        }
    }

    tags
}

/// Extract tags from JavaScript/TypeScript source code.
fn extract_js_tags(content: &str, file: &str) -> Vec<Tag> {
    let mut tags = Vec::new();
    let lines: Vec<&str> = content.lines().collect();
    let mut in_block_comment = false;
    let mut current_class: Option<String> = None;

    for (idx, line) in lines.iter().enumerate() {
        let line_num = idx + 1;
        let trimmed = line.trim();

        if in_block_comment {
            if trimmed.contains("*/") {
                in_block_comment = false;
            }
            continue;
        }
        if trimmed.starts_with("/*") && !trimmed.contains("*/") {
            in_block_comment = true;
            continue;
        }
        if trimmed.starts_with("//") {
            continue;
        }

        // Strip leading export/async/default.
        let stripped = trimmed
            .trim_start_matches("export ")
            .trim_start_matches("default ")
            .trim_start_matches("async ")
            .trim_start_matches("declare ");

        // function name( or function* name(
        let fn_rest = line_after_keyword(stripped, "function")
            .or_else(|| line_after_keyword(stripped, "function*"));
        if let Some(rest) = fn_rest {
            let name = first_ident(&rest);
            if !name.is_empty() {
                tags.push(Tag {
                    name,
                    file: file.to_string(),
                    line_number: line_num,
                    pattern: make_pattern(line),
                    kind: TagKind::Function,
                    language: Language::JavaScript,
                    scope: None,
                });
            }
        }

        // class Name
        if let Some(rest) = line_after_keyword(stripped, "class") {
            let name = first_ident(&rest);
            if !name.is_empty() {
                current_class = Some(name.clone());
                tags.push(Tag {
                    name,
                    file: file.to_string(),
                    line_number: line_num,
                    pattern: make_pattern(line),
                    kind: TagKind::Class,
                    language: Language::JavaScript,
                    scope: None,
                });
            }
        }

        // interface Name (TypeScript)
        if let Some(rest) = line_after_keyword(stripped, "interface") {
            let name = first_ident(&rest);
            if !name.is_empty() {
                tags.push(Tag {
                    name,
                    file: file.to_string(),
                    line_number: line_num,
                    pattern: make_pattern(line),
                    kind: TagKind::Interface,
                    language: Language::JavaScript,
                    scope: None,
                });
            }
        }

        // type Name = ... (TypeScript)
        if let Some(rest) = line_after_keyword(stripped, "type") {
            let name = first_ident(&rest);
            if !name.is_empty() && rest.contains('=') {
                tags.push(Tag {
                    name,
                    file: file.to_string(),
                    line_number: line_num,
                    pattern: make_pattern(line),
                    kind: TagKind::Typedef,
                    language: Language::JavaScript,
                    scope: None,
                });
            }
        }

        // enum Name (TypeScript)
        if let Some(rest) = line_after_keyword(stripped, "enum") {
            let name = first_ident(&rest);
            if !name.is_empty() {
                tags.push(Tag {
                    name,
                    file: file.to_string(),
                    line_number: line_num,
                    pattern: make_pattern(line),
                    kind: TagKind::Enum,
                    language: Language::JavaScript,
                    scope: None,
                });
            }
        }

        // const NAME = ... / let NAME = ... / var NAME = ...
        for kw in &["const", "let", "var"] {
            if let Some(rest) = line_after_keyword(stripped, kw)
                && rest.contains('=') {
                    let name_end = rest
                        .find(['=', ':', ' '])
                        .unwrap_or(rest.len());
                    let name = rest[..name_end].trim().to_string();
                    if !name.is_empty()
                        && name
                            .chars()
                            .all(|c| c.is_alphanumeric() || c == '_' || c == '$')
                    {
                        tags.push(Tag {
                            name,
                            file: file.to_string(),
                            line_number: line_num,
                            pattern: make_pattern(line),
                            kind: TagKind::Variable,
                            language: Language::JavaScript,
                            scope: None,
                        });
                    }
                }
        }

        // Method: name( inside class body (indented, no `function` keyword).
        if current_class.is_some() {
            let indent = line.len() - line.trim_start().len();
            if indent > 0 && !stripped.starts_with("function") {
                // Strip optional async/static/get/set.
                let method_line = stripped
                    .trim_start_matches("static ")
                    .trim_start_matches("async ")
                    .trim_start_matches("get ")
                    .trim_start_matches("set ");
                if let Some(paren) = method_line.find('(') {
                    let name = first_ident(&method_line[..paren]);
                    // Skip control-flow keywords but keep "constructor" as
                    // a real method name. (The expression below simplifies
                    // from a `(... && != "constructor") || == "constructor"`
                    // form clippy flagged as redundant.)
                    if !name.is_empty()
                        && name != "if"
                        && name != "while"
                        && name != "for"
                        && name != "switch"
                        && name != "return"
                    {
                        tags.push(Tag {
                            name,
                            file: file.to_string(),
                            line_number: line_num,
                            pattern: make_pattern(line),
                            kind: TagKind::Method,
                            language: Language::JavaScript,
                            scope: current_class.clone(),
                        });
                    }
                }
            }
        }

        // Rough scope tracking.
        if trimmed == "}" && line.starts_with('}') {
            current_class = None;
        }
    }

    tags
}

/// Extract tags from Go source code.
fn extract_go_tags(content: &str, file: &str) -> Vec<Tag> {
    let mut tags = Vec::new();
    let lines: Vec<&str> = content.lines().collect();
    let mut in_block_comment = false;

    for (idx, line) in lines.iter().enumerate() {
        let line_num = idx + 1;
        let trimmed = line.trim();

        if in_block_comment {
            if trimmed.contains("*/") {
                in_block_comment = false;
            }
            continue;
        }
        if trimmed.starts_with("/*") && !trimmed.contains("*/") {
            in_block_comment = true;
            continue;
        }
        if trimmed.starts_with("//") {
            continue;
        }

        // package name
        if let Some(rest) = line_after_keyword(trimmed, "package") {
            let name = first_ident(&rest);
            if !name.is_empty() {
                tags.push(Tag {
                    name,
                    file: file.to_string(),
                    line_number: line_num,
                    pattern: make_pattern(line),
                    kind: TagKind::Module,
                    language: Language::Go,
                    scope: None,
                });
            }
        }

        // func Name(  or  func (receiver) Name(
        if let Some(rest) = line_after_keyword(trimmed, "func") {
            let rest = rest.trim();
            let name = if rest.starts_with('(') {
                // Method with receiver: func (r *Type) Name(...)
                if let Some(close) = rest.find(')') {
                    let after = rest[close + 1..].trim();
                    first_ident(after)
                } else {
                    String::new()
                }
            } else {
                first_ident(rest)
            };
            if !name.is_empty() {
                let is_method = rest.starts_with('(');
                tags.push(Tag {
                    name,
                    file: file.to_string(),
                    line_number: line_num,
                    pattern: make_pattern(line),
                    kind: if is_method {
                        TagKind::Method
                    } else {
                        TagKind::Function
                    },
                    language: Language::Go,
                    scope: None,
                });
            }
        }

        // type Name struct/interface/...
        if let Some(rest) = line_after_keyword(trimmed, "type") {
            let name = first_ident(&rest);
            if !name.is_empty() {
                let kind = if rest.contains("struct") {
                    TagKind::Struct
                } else if rest.contains("interface") {
                    TagKind::Interface
                } else {
                    TagKind::Typedef
                };
                tags.push(Tag {
                    name,
                    file: file.to_string(),
                    line_number: line_num,
                    pattern: make_pattern(line),
                    kind,
                    language: Language::Go,
                    scope: None,
                });
            }
        }

        // var name / const name
        for kw in &["var", "const"] {
            if let Some(rest) = line_after_keyword(trimmed, kw) {
                let name = first_ident(&rest);
                if !name.is_empty() && name != "(" {
                    tags.push(Tag {
                        name,
                        file: file.to_string(),
                        line_number: line_num,
                        pattern: make_pattern(line),
                        kind: if *kw == "const" {
                            TagKind::Constant
                        } else {
                            TagKind::Variable
                        },
                        language: Language::Go,
                        scope: None,
                    });
                }
            }
        }
    }

    tags
}

/// Extract tags from Shell (Bash/sh) source code.
fn extract_shell_tags(content: &str, file: &str) -> Vec<Tag> {
    let mut tags = Vec::new();
    let lines: Vec<&str> = content.lines().collect();

    for (idx, line) in lines.iter().enumerate() {
        let line_num = idx + 1;
        let trimmed = line.trim();

        if trimmed.starts_with('#') || trimmed.is_empty() {
            continue;
        }

        // function name { ... }   or   function name() { ... }
        if let Some(rest) = line_after_keyword(trimmed, "function") {
            let name_end = rest
                .find(['(', '{', ' '])
                .unwrap_or(rest.len());
            let name = rest[..name_end].trim().to_string();
            if !name.is_empty() {
                tags.push(Tag {
                    name,
                    file: file.to_string(),
                    line_number: line_num,
                    pattern: make_pattern(line),
                    kind: TagKind::Function,
                    language: Language::Shell,
                    scope: None,
                });
            }
        }
        // name() {
        else if let Some(paren) = trimmed.find("()") {
            let candidate = trimmed[..paren].trim();
            if !candidate.is_empty()
                && candidate
                    .chars()
                    .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
                && candidate.chars().next().is_some_and(|c| c.is_alphabetic() || c == '_')
            {
                tags.push(Tag {
                    name: candidate.to_string(),
                    file: file.to_string(),
                    line_number: line_num,
                    pattern: make_pattern(line),
                    kind: TagKind::Function,
                    language: Language::Shell,
                    scope: None,
                });
            }
        }

        // Variable: NAME=value (at top level, all caps for "constants").
        if let Some(eq_pos) = trimmed.find('=')
            && eq_pos > 0
                && !trimmed[..eq_pos].contains(' ')
                && !trimmed.starts_with("if")
                && !trimmed.starts_with("local")
                && !trimmed.starts_with("export")
                && trimmed
                    .as_bytes()
                    .get(eq_pos + 1)
                    .is_none_or(|&b| b != b'=')
                && trimmed.as_bytes()[eq_pos.saturating_sub(1)] != b'!'
            {
                let name = &trimmed[..eq_pos];
                if name
                    .chars()
                    .all(|c| c.is_uppercase() || c == '_' || c.is_ascii_digit())
                    && name.chars().next().is_some_and(|c| c.is_alphabetic())
                {
                    tags.push(Tag {
                        name: name.to_string(),
                        file: file.to_string(),
                        line_number: line_num,
                        pattern: make_pattern(line),
                        kind: TagKind::Variable,
                        language: Language::Shell,
                        scope: None,
                    });
                }
            }
    }

    tags
}

// ============================================================================
// Unified tag extraction
// ============================================================================

/// Extract all tags from one source file, dispatching to the language-specific
/// extractor.
fn extract_tags_from_file(path: &str) -> Vec<Tag> {
    let language = match detect_language(path) {
        Some(l) => l,
        None => return Vec::new(),
    };

    let content = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("ctags: cannot read '{}': {}", path, e);
            return Vec::new();
        }
    };

    extract_tags_from_content(&content, path, language)
}

/// Extract tags from source content for a known language.
fn extract_tags_from_content(content: &str, file: &str, language: Language) -> Vec<Tag> {
    match language {
        Language::C => extract_c_tags(content, file, Language::C),
        Language::Cpp => extract_c_tags(content, file, Language::Cpp),
        Language::Rust => extract_rust_tags(content, file),
        Language::Python => extract_python_tags(content, file),
        Language::Java => extract_java_tags(content, file),
        Language::JavaScript => extract_js_tags(content, file),
        Language::Go => extract_go_tags(content, file),
        Language::Shell => extract_shell_tags(content, file),
    }
}

/// Read file list from stdin (one path per line).
fn read_stdin_filelist() -> Vec<String> {
    let stdin = io::stdin();
    let reader = BufReader::new(stdin.lock());
    reader
        .lines()
        .map_while(Result::ok)
        .filter(|l| !l.is_empty())
        .collect()
}

// ============================================================================
// Output — ctags format
// ============================================================================

/// Sort tags according to the chosen mode.
fn sort_tags(tags: &mut [Tag], mode: SortMode) {
    match mode {
        SortMode::Yes => {
            tags.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.file.cmp(&b.file)));
        }
        SortMode::Foldcase => {
            tags.sort_by(|a, b| {
                a.name
                    .to_ascii_lowercase()
                    .cmp(&b.name.to_ascii_lowercase())
                    .then_with(|| a.file.cmp(&b.file))
            });
        }
        SortMode::No => {} // preserve insertion order
    }
}

/// Write ctags (Vi-compatible) output.
fn write_ctags<W: Write>(
    tags: &[Tag],
    fields: &str,
    extras: &str,
    sort_mode: SortMode,
    out: &mut W,
) -> io::Result<()> {
    // Header.
    writeln!(out, "!_TAG_FILE_FORMAT\t2\t/extended format/")?;
    writeln!(
        out,
        "!_TAG_FILE_SORTED\t{}\t/0=unsorted, 1=sorted, 2=foldcase/",
        match sort_mode {
            SortMode::Yes => '1',
            SortMode::No => '0',
            SortMode::Foldcase => '2',
        }
    )?;
    writeln!(
        out,
        "!_TAG_PROGRAM_NAME\tctags\t/OurOS ctags/"
    )?;
    writeln!(
        out,
        "!_TAG_PROGRAM_VERSION\t{}\t//",
        VERSION
    )?;

    for tag in tags {
        // Basic format: name<TAB>file<TAB>pattern
        write!(out, "{}\t{}\t{}", tag.name, tag.file, tag.pattern)?;

        // Extended fields.
        let mut ext_parts: Vec<String> = Vec::new();

        if fields.contains('k') || fields.contains('K') {
            ext_parts.push(format!("kind:{}", tag.kind.letter()));
        }
        if fields.contains('l') {
            ext_parts.push(format!("language:{}", tag.language.name()));
        }
        if fields.contains('n') {
            ext_parts.push(format!("line:{}", tag.line_number));
        }
        if (fields.contains('s') || fields.contains('S'))
            && let Some(ref scope) = tag.scope {
                ext_parts.push(format!("scope:{}", scope));
            }

        // Always write the kind in the compact `;" <TAB> kind:x` form.
        if ext_parts.is_empty() {
            // Minimal: just the kind letter.
            write!(out, ";\"\t{}", tag.kind.letter())?;
        } else {
            write!(out, ";\"\t{}", ext_parts.join("\t"))?;
        }

        // Qualified tag extra.
        if extras.contains('q')
            && let Some(ref scope) = tag.scope {
                // Emit extra qualified entry.
                write!(
                    out,
                    "\n{}.{}\t{}\t{};\"\t{}",
                    scope,
                    tag.name,
                    tag.file,
                    tag.pattern,
                    tag.kind.letter()
                )?;
            }

        writeln!(out)?;
    }

    Ok(())
}

// ============================================================================
// Output — etags format
// ============================================================================

/// Write etags (Emacs TAGS) output.
fn write_etags<W: Write>(tags: &[Tag], out: &mut W) -> io::Result<()> {
    // Group tags by file.
    let mut files_order: Vec<String> = Vec::new();
    let mut files_seen = BTreeSet::new();

    for tag in tags {
        if files_seen.insert(tag.file.clone()) {
            files_order.push(tag.file.clone());
        }
    }

    for file in &files_order {
        let file_tags: Vec<&Tag> = tags.iter().filter(|t| t.file == *file).collect();
        if file_tags.is_empty() {
            continue;
        }

        // Build section body first (to know byte length).
        let mut section = Vec::new();
        for tag in &file_tags {
            // etags format: DEFINITION\x7fNAME\x01LINE,OFFSET
            // We use the pattern text (without /^ $/) as the definition.
            let def_text = tag
                .pattern
                .trim_start_matches("/^")
                .trim_end_matches("$/");
            writeln!(
                section,
                "{}\x7f{}\x01{},0",
                def_text, tag.name, tag.line_number
            )?;
        }

        // File header: \x0c\nFILE,SIZE\n
        write!(out, "\x0c\n{},{}\n", file, section.len())?;
        out.write_all(&section)?;
    }

    Ok(())
}

// ============================================================================
// Read existing tags (for append mode)
// ============================================================================

/// Read existing ctags entries from a file, returning them as raw lines.
fn read_existing_ctags(path: &str) -> Vec<String> {
    let content = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };

    content
        .lines()
        .filter(|l| !l.starts_with("!_TAG_"))
        .map(|l| l.to_string())
        .collect()
}

/// Parse a raw ctags line back into a Tag (best-effort).
fn parse_ctags_line(line: &str) -> Option<Tag> {
    // Format: name<TAB>file<TAB>pattern;"<TAB>kind...
    let mut parts = line.splitn(3, '\t');
    let name = parts.next()?.to_string();
    let file = parts.next()?.to_string();
    let rest = parts.next()?;

    let (pattern, _ext) = if let Some(pos) = rest.find(";\"") {
        (rest[..pos].to_string(), &rest[pos + 2..])
    } else {
        (rest.to_string(), "")
    };

    Some(Tag {
        name,
        file,
        line_number: 0,
        pattern,
        kind: TagKind::Function, // default; we don't fully parse extended fields
        language: Language::C,
        scope: None,
    })
}

// ============================================================================
// Help / version
// ============================================================================

fn print_help() {
    println!(
        "\
Usage: ctags [OPTIONS] [FILE]...

Generate tag index files for source code.

Options:
  -R, --recurse          Recurse into directories
  -f TAGFILE             Write tags to TAGFILE (default: \"tags\" / \"TAGS\")
  -o TAGFILE             Synonym for -f
  -a, --append           Append to tag file instead of overwriting
  -e, --etags            Produce Emacs TAGS output (auto when invoked as etags)
  -u                     Unsorted output
      --sort=MODE        Sort mode: yes (default), no, foldcase
      --exclude=PATTERN  Exclude files matching glob PATTERN
      --fields=FLAGS     Include extra fields (afmikKlnsStz)
      --extras=FLAGS     Include extra tag entries (+q for qualified tags)
      --help             Display this help and exit
      --version          Output version information and exit

Supported languages:
  C (.c, .h), C++ (.cpp, .cxx, .cc, .hpp), Rust (.rs),
  Python (.py), Java (.java), JavaScript/TypeScript (.js, .ts, .jsx, .tsx),
  Go (.go), Shell (.sh, .bash, .zsh)

Tag kinds generated:
  f  function/method     s  struct           c  class
  g  enum                t  typedef/alias    d  macro/define
  v  variable/constant   i  interface/trait  n  module/namespace"
    );
}

// ============================================================================
// Core run logic
// ============================================================================

fn run_main() -> i32 {
    let args: Vec<String> = env::args().collect();

    match parse_args(&args) {
        ParseResult::Help => {
            print_help();
            0
        }
        ParseResult::Version => {
            println!("ctags (OurOS) {VERSION}");
            0
        }
        ParseResult::Run(config) => run(&config),
    }
}

fn run(config: &Config) -> i32 {
    // Collect files.
    let files = collect_files(config);

    // Extract tags from all files.
    let mut all_tags: Vec<Tag> = Vec::new();

    for file in &files {
        if file == "-" {
            // Read file list from stdin.
            let stdin_files = read_stdin_filelist();
            for sf in &stdin_files {
                let tags = extract_tags_from_file(sf);
                all_tags.extend(tags);
            }
        } else {
            let tags = extract_tags_from_file(file);
            all_tags.extend(tags);
        }
    }

    // In append mode, merge with existing tags.
    let output_name = config
        .output_file
        .clone()
        .unwrap_or_else(|| {
            if config.format == OutputFormat::Etags {
                "TAGS".to_string()
            } else {
                "tags".to_string()
            }
        });

    if config.append && config.format == OutputFormat::Ctags {
        let existing_lines = read_existing_ctags(&output_name);
        // Remove tags from files we are re-scanning.
        let scanned_files: BTreeSet<&str> = files
            .iter()
            .filter(|f| *f != "-")
            .map(|f| f.as_str())
            .collect();
        for line in &existing_lines {
            if let Some(tag) = parse_ctags_line(line)
                && !scanned_files.contains(tag.file.as_str()) {
                    all_tags.push(tag);
                }
        }
    }

    // Sort.
    if config.format == OutputFormat::Ctags {
        sort_tags(&mut all_tags, config.sort);
    }

    // Write output.
    let write_result: io::Result<()> = if output_name == "-" {
        let stdout = io::stdout();
        let mut out = stdout.lock();
        match config.format {
            OutputFormat::Ctags => {
                write_ctags(&all_tags, &config.fields, &config.extras, config.sort, &mut out)
            }
            OutputFormat::Etags => write_etags(&all_tags, &mut out),
        }
    } else {
        let file = if config.append && config.format == OutputFormat::Etags {
            fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&output_name)
        } else {
            fs::File::create(&output_name)
        };
        match file {
            Ok(f) => {
                let mut out = io::BufWriter::new(f);
                let result = match config.format {
                    OutputFormat::Ctags => {
                        write_ctags(
                            &all_tags,
                            &config.fields,
                            &config.extras,
                            config.sort,
                            &mut out,
                        )
                    }
                    OutputFormat::Etags => write_etags(&all_tags, &mut out),
                };
                if result.is_ok()
                    && let Err(e) = out.flush() {
                        eprintln!("ctags: write error: {}", e);
                        return 1;
                    }
                result
            }
            Err(e) => {
                eprintln!("ctags: cannot open '{}': {}", output_name, e);
                return 1;
            }
        }
    };

    if let Err(e) = write_result {
        eprintln!("ctags: write error: {}", e);
        return 1;
    }

    0
}

// ============================================================================
// Entry point
// ============================================================================

#[cfg(not(test))]
#[unsafe(no_mangle)]
pub extern "C" fn main(_argc: i32, _argv: *const *const u8) -> i32 {
    run_main()
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ---- Language detection ----

    #[test]
    fn detect_c_file() {
        assert_eq!(detect_language("foo.c"), Some(Language::C));
        assert_eq!(detect_language("bar.h"), Some(Language::C));
    }

    #[test]
    fn detect_cpp_file() {
        assert_eq!(detect_language("foo.cpp"), Some(Language::Cpp));
        assert_eq!(detect_language("bar.hpp"), Some(Language::Cpp));
        assert_eq!(detect_language("baz.cxx"), Some(Language::Cpp));
        assert_eq!(detect_language("x.cc"), Some(Language::Cpp));
    }

    #[test]
    fn detect_rust_file() {
        assert_eq!(detect_language("lib.rs"), Some(Language::Rust));
    }

    #[test]
    fn detect_python_file() {
        assert_eq!(detect_language("script.py"), Some(Language::Python));
        assert_eq!(detect_language("stubs.pyi"), Some(Language::Python));
    }

    #[test]
    fn detect_java_file() {
        assert_eq!(detect_language("Main.java"), Some(Language::Java));
    }

    #[test]
    fn detect_js_file() {
        assert_eq!(detect_language("app.js"), Some(Language::JavaScript));
        assert_eq!(detect_language("app.ts"), Some(Language::JavaScript));
        assert_eq!(detect_language("component.tsx"), Some(Language::JavaScript));
        assert_eq!(detect_language("module.mjs"), Some(Language::JavaScript));
    }

    #[test]
    fn detect_go_file() {
        assert_eq!(detect_language("main.go"), Some(Language::Go));
    }

    #[test]
    fn detect_shell_file() {
        assert_eq!(detect_language("script.sh"), Some(Language::Shell));
        assert_eq!(detect_language("run.bash"), Some(Language::Shell));
        assert_eq!(detect_language("init.zsh"), Some(Language::Shell));
    }

    #[test]
    fn detect_unknown_extension() {
        assert_eq!(detect_language("data.csv"), None);
        assert_eq!(detect_language("readme.md"), None);
    }

    // ---- Glob matching ----

    #[test]
    fn glob_exact_match() {
        assert!(glob_match("foo.c", "foo.c"));
    }

    #[test]
    fn glob_star_match() {
        assert!(glob_match("*.c", "foo.c"));
        assert!(glob_match("*.c", "bar/baz.c"));
    }

    #[test]
    fn glob_question_match() {
        assert!(glob_match("?.c", "x.c"));
        assert!(!glob_match("?.c", "xy.c"));
    }

    #[test]
    fn glob_no_match() {
        assert!(!glob_match("*.py", "foo.c"));
    }

    #[test]
    fn glob_basename_match() {
        // Pattern without `/` matches basename.
        assert!(glob_match("*.c", "src/foo.c"));
    }

    #[test]
    fn glob_with_path() {
        assert!(glob_match("build/*", "build/out.o"));
    }

    // ---- Pattern generation ----

    #[test]
    fn make_pattern_simple() {
        let pat = make_pattern("int main() {");
        assert_eq!(pat, "/^int main() {$/");
    }

    #[test]
    fn make_pattern_escapes_slash() {
        let pat = make_pattern("a/b");
        assert_eq!(pat, "/^a\\/b$/");
    }

    #[test]
    fn make_pattern_escapes_backslash() {
        let pat = make_pattern("a\\b");
        assert_eq!(pat, "/^a\\\\b$/");
    }

    #[test]
    fn make_pattern_trims_trailing_ws() {
        let pat = make_pattern("hello   ");
        assert_eq!(pat, "/^hello$/");
    }

    // ---- Helper functions ----

    #[test]
    fn first_ident_simple() {
        assert_eq!(first_ident("foo_bar"), "foo_bar");
    }

    #[test]
    fn first_ident_with_leading_space() {
        assert_eq!(first_ident("  hello"), "hello");
    }

    #[test]
    fn first_ident_with_pointer() {
        assert_eq!(first_ident("*ptr"), "ptr");
    }

    #[test]
    fn first_ident_stops_at_paren() {
        assert_eq!(first_ident("func(args)"), "func");
    }

    #[test]
    fn line_after_keyword_match() {
        let result = line_after_keyword("  fn hello()", "fn");
        assert_eq!(result, Some("hello()".to_string()));
    }

    #[test]
    fn line_after_keyword_no_match() {
        assert!(line_after_keyword("  fno hello()", "fn").is_none());
    }

    #[test]
    fn strip_quotes_double() {
        assert_eq!(strip_quotes("\"hello\""), "hello");
    }

    #[test]
    fn strip_quotes_single() {
        assert_eq!(strip_quotes("'world'"), "world");
    }

    #[test]
    fn strip_quotes_none() {
        assert_eq!(strip_quotes("plain"), "plain");
    }

    // ---- C tag extraction ----

    #[test]
    fn c_define() {
        let src = "#define MAX_SIZE 100\n";
        let tags = extract_tags_from_content(src, "test.c", Language::C);
        assert!(tags.iter().any(|t| t.name == "MAX_SIZE" && t.kind == TagKind::Macro));
    }

    #[test]
    fn c_typedef() {
        let src = "typedef unsigned int uint32;\n";
        let tags = extract_tags_from_content(src, "test.c", Language::C);
        assert!(tags.iter().any(|t| t.name == "uint32" && t.kind == TagKind::Typedef));
    }

    #[test]
    fn c_struct() {
        let src = "struct Point {\n    int x;\n    int y;\n};\n";
        let tags = extract_tags_from_content(src, "test.c", Language::C);
        assert!(tags.iter().any(|t| t.name == "Point" && t.kind == TagKind::Struct));
    }

    #[test]
    fn c_enum() {
        let src = "enum Color {\n    RED,\n    GREEN\n};\n";
        let tags = extract_tags_from_content(src, "test.c", Language::C);
        assert!(tags.iter().any(|t| t.name == "Color" && t.kind == TagKind::Enum));
    }

    #[test]
    fn c_function() {
        let src = "int main(int argc, char **argv) {\n    return 0;\n}\n";
        let tags = extract_tags_from_content(src, "test.c", Language::C);
        assert!(tags.iter().any(|t| t.name == "main" && t.kind == TagKind::Function));
    }

    #[test]
    fn c_function_next_line_brace() {
        let src = "void foo(void)\n{\n    return;\n}\n";
        let tags = extract_tags_from_content(src, "test.c", Language::C);
        assert!(tags.iter().any(|t| t.name == "foo" && t.kind == TagKind::Function));
    }

    #[test]
    fn c_skips_comments() {
        let src = "// #define COMMENTED 1\nint real_func() {\n}\n";
        let tags = extract_tags_from_content(src, "test.c", Language::C);
        assert!(tags.iter().all(|t| t.name != "COMMENTED"));
    }

    #[test]
    fn c_block_comment() {
        let src = "/* this is\na block comment */\nint after() {\n}\n";
        let tags = extract_tags_from_content(src, "test.c", Language::C);
        assert!(tags.iter().any(|t| t.name == "after"));
    }

    // ---- C++ extras ----

    #[test]
    fn cpp_class() {
        let src = "class Widget {\npublic:\n    void draw();\n};\n";
        let tags = extract_tags_from_content(src, "test.cpp", Language::Cpp);
        assert!(tags.iter().any(|t| t.name == "Widget" && t.kind == TagKind::Class));
    }

    #[test]
    fn cpp_namespace() {
        let src = "namespace gui {\n}\n";
        let tags = extract_tags_from_content(src, "test.cpp", Language::Cpp);
        assert!(tags.iter().any(|t| t.name == "gui" && t.kind == TagKind::Module));
    }

    // ---- Rust tag extraction ----

    #[test]
    fn rust_function() {
        let src = "fn hello_world() {\n    println!(\"hello\");\n}\n";
        let tags = extract_tags_from_content(src, "test.rs", Language::Rust);
        assert!(tags.iter().any(|t| t.name == "hello_world" && t.kind == TagKind::Function));
    }

    #[test]
    fn rust_pub_function() {
        let src = "pub fn greet(name: &str) {\n}\n";
        let tags = extract_tags_from_content(src, "test.rs", Language::Rust);
        assert!(tags.iter().any(|t| t.name == "greet"));
    }

    #[test]
    fn rust_async_fn() {
        let src = "pub async fn serve() {\n}\n";
        let tags = extract_tags_from_content(src, "test.rs", Language::Rust);
        assert!(tags.iter().any(|t| t.name == "serve" && t.kind == TagKind::Function));
    }

    #[test]
    fn rust_struct() {
        let src = "pub struct Config {\n    verbose: bool,\n}\n";
        let tags = extract_tags_from_content(src, "test.rs", Language::Rust);
        assert!(tags.iter().any(|t| t.name == "Config" && t.kind == TagKind::Struct));
    }

    #[test]
    fn rust_enum() {
        let src = "enum Direction {\n    North,\n    South,\n}\n";
        let tags = extract_tags_from_content(src, "test.rs", Language::Rust);
        assert!(tags.iter().any(|t| t.name == "Direction" && t.kind == TagKind::Enum));
    }

    #[test]
    fn rust_trait() {
        let src = "pub trait Drawable {\n    fn draw(&self);\n}\n";
        let tags = extract_tags_from_content(src, "test.rs", Language::Rust);
        assert!(tags.iter().any(|t| t.name == "Drawable" && t.kind == TagKind::Trait));
    }

    #[test]
    fn rust_type_alias() {
        let src = "type Result<T> = std::result::Result<T, MyError>;\n";
        let tags = extract_tags_from_content(src, "test.rs", Language::Rust);
        assert!(tags.iter().any(|t| t.name == "Result" && t.kind == TagKind::Typedef));
    }

    #[test]
    fn rust_macro_rules() {
        let src = "macro_rules! my_macro {\n    () => {};\n}\n";
        let tags = extract_tags_from_content(src, "test.rs", Language::Rust);
        assert!(tags.iter().any(|t| t.name == "my_macro" && t.kind == TagKind::Macro));
    }

    #[test]
    fn rust_module() {
        let src = "pub mod parser {\n}\n";
        let tags = extract_tags_from_content(src, "test.rs", Language::Rust);
        assert!(tags.iter().any(|t| t.name == "parser" && t.kind == TagKind::Module));
    }

    #[test]
    fn rust_impl_method() {
        let src = "impl Config {\n    pub fn new() -> Self {\n        Self { verbose: false }\n    }\n}\n";
        let tags = extract_tags_from_content(src, "test.rs", Language::Rust);
        assert!(tags.iter().any(|t| t.name == "new" && t.kind == TagKind::Method));
        let method = tags.iter().find(|t| t.name == "new").unwrap();
        assert_eq!(method.scope.as_deref(), Some("Config"));
    }

    #[test]
    fn rust_static_variable() {
        let src = "static COUNTER: AtomicUsize = AtomicUsize::new(0);\n";
        let tags = extract_tags_from_content(src, "test.rs", Language::Rust);
        assert!(tags.iter().any(|t| t.name == "COUNTER" && t.kind == TagKind::Variable));
    }

    // ---- Python tag extraction ----

    #[test]
    fn python_function() {
        let src = "def hello():\n    pass\n";
        let tags = extract_tags_from_content(src, "test.py", Language::Python);
        assert!(tags.iter().any(|t| t.name == "hello" && t.kind == TagKind::Function));
    }

    #[test]
    fn python_class() {
        let src = "class MyClass:\n    pass\n";
        let tags = extract_tags_from_content(src, "test.py", Language::Python);
        assert!(tags.iter().any(|t| t.name == "MyClass" && t.kind == TagKind::Class));
    }

    #[test]
    fn python_method() {
        let src = "class Foo:\n    def bar(self):\n        pass\n";
        let tags = extract_tags_from_content(src, "test.py", Language::Python);
        assert!(tags.iter().any(|t| t.name == "bar" && t.kind == TagKind::Method));
    }

    #[test]
    fn python_async_def() {
        let src = "async def fetch():\n    pass\n";
        let tags = extract_tags_from_content(src, "test.py", Language::Python);
        assert!(tags.iter().any(|t| t.name == "fetch" && t.kind == TagKind::Function));
    }

    #[test]
    fn python_constant() {
        let src = "MAX_RETRIES = 3\n";
        let tags = extract_tags_from_content(src, "test.py", Language::Python);
        assert!(tags.iter().any(|t| t.name == "MAX_RETRIES" && t.kind == TagKind::Constant));
    }

    #[test]
    fn python_variable() {
        let src = "config_path = '/etc/app'\n";
        let tags = extract_tags_from_content(src, "test.py", Language::Python);
        assert!(tags.iter().any(|t| t.name == "config_path" && t.kind == TagKind::Variable));
    }

    #[test]
    fn python_class_with_bases() {
        let src = "class Child(Parent, Mixin):\n    pass\n";
        let tags = extract_tags_from_content(src, "test.py", Language::Python);
        assert!(tags.iter().any(|t| t.name == "Child" && t.kind == TagKind::Class));
    }

    // ---- Java tag extraction ----

    #[test]
    fn java_class() {
        let src = "public class Main {\n}\n";
        let tags = extract_tags_from_content(src, "Main.java", Language::Java);
        assert!(tags.iter().any(|t| t.name == "Main" && t.kind == TagKind::Class));
    }

    #[test]
    fn java_interface() {
        let src = "public interface Runnable {\n    void run();\n}\n";
        let tags = extract_tags_from_content(src, "Runnable.java", Language::Java);
        assert!(tags.iter().any(|t| t.name == "Runnable" && t.kind == TagKind::Interface));
    }

    #[test]
    fn java_enum() {
        let src = "public enum Color {\n    RED, GREEN, BLUE\n}\n";
        let tags = extract_tags_from_content(src, "Color.java", Language::Java);
        assert!(tags.iter().any(|t| t.name == "Color" && t.kind == TagKind::Enum));
    }

    #[test]
    fn java_method() {
        let src = "public class Calc {\n    public int add(int a, int b) {\n        return a + b;\n    }\n}\n";
        let tags = extract_tags_from_content(src, "Calc.java", Language::Java);
        assert!(tags.iter().any(|t| t.name == "add" && t.kind == TagKind::Method));
    }

    #[test]
    fn java_package() {
        let src = "package com.example.app;\n\npublic class App {\n}\n";
        let tags = extract_tags_from_content(src, "App.java", Language::Java);
        assert!(tags.iter().any(|t| t.name == "com.example.app" && t.kind == TagKind::Module));
    }

    // ---- JavaScript / TypeScript tag extraction ----

    #[test]
    fn js_function() {
        let src = "function greet(name) {\n    console.log(name);\n}\n";
        let tags = extract_tags_from_content(src, "app.js", Language::JavaScript);
        assert!(tags.iter().any(|t| t.name == "greet" && t.kind == TagKind::Function));
    }

    #[test]
    fn js_export_function() {
        let src = "export function doStuff() {\n}\n";
        let tags = extract_tags_from_content(src, "lib.js", Language::JavaScript);
        assert!(tags.iter().any(|t| t.name == "doStuff" && t.kind == TagKind::Function));
    }

    #[test]
    fn js_class() {
        let src = "class Widget {\n    constructor() {}\n}\n";
        let tags = extract_tags_from_content(src, "widget.js", Language::JavaScript);
        assert!(tags.iter().any(|t| t.name == "Widget" && t.kind == TagKind::Class));
    }

    #[test]
    fn ts_interface() {
        let src = "export interface Props {\n    title: string;\n}\n";
        let tags = extract_tags_from_content(src, "types.ts", Language::JavaScript);
        assert!(tags.iter().any(|t| t.name == "Props" && t.kind == TagKind::Interface));
    }

    #[test]
    fn ts_type_alias() {
        let src = "type Result<T> = Success<T> | Failure;\n";
        let tags = extract_tags_from_content(src, "types.ts", Language::JavaScript);
        assert!(tags.iter().any(|t| t.name == "Result" && t.kind == TagKind::Typedef));
    }

    #[test]
    fn ts_enum() {
        let src = "enum Direction {\n    Up,\n    Down,\n}\n";
        let tags = extract_tags_from_content(src, "enums.ts", Language::JavaScript);
        assert!(tags.iter().any(|t| t.name == "Direction" && t.kind == TagKind::Enum));
    }

    #[test]
    fn js_const_variable() {
        let src = "const MAX_COUNT = 100;\n";
        let tags = extract_tags_from_content(src, "config.js", Language::JavaScript);
        assert!(tags.iter().any(|t| t.name == "MAX_COUNT" && t.kind == TagKind::Variable));
    }

    #[test]
    fn js_method_in_class() {
        let src = "class Foo {\n    bar() {\n    }\n}\n";
        let tags = extract_tags_from_content(src, "foo.js", Language::JavaScript);
        assert!(tags.iter().any(|t| t.name == "bar" && t.kind == TagKind::Method));
    }

    // ---- Go tag extraction ----

    #[test]
    fn go_function() {
        let src = "func main() {\n    fmt.Println(\"hello\")\n}\n";
        let tags = extract_tags_from_content(src, "main.go", Language::Go);
        assert!(tags.iter().any(|t| t.name == "main" && t.kind == TagKind::Function));
    }

    #[test]
    fn go_method() {
        let src = "func (s *Server) Start() error {\n    return nil\n}\n";
        let tags = extract_tags_from_content(src, "server.go", Language::Go);
        assert!(tags.iter().any(|t| t.name == "Start" && t.kind == TagKind::Method));
    }

    #[test]
    fn go_struct() {
        let src = "type Config struct {\n    Port int\n}\n";
        let tags = extract_tags_from_content(src, "config.go", Language::Go);
        assert!(tags.iter().any(|t| t.name == "Config" && t.kind == TagKind::Struct));
    }

    #[test]
    fn go_interface() {
        let src = "type Reader interface {\n    Read(p []byte) (n int, err error)\n}\n";
        let tags = extract_tags_from_content(src, "io.go", Language::Go);
        assert!(tags.iter().any(|t| t.name == "Reader" && t.kind == TagKind::Interface));
    }

    #[test]
    fn go_type_alias() {
        let src = "type Duration int64\n";
        let tags = extract_tags_from_content(src, "time.go", Language::Go);
        assert!(tags.iter().any(|t| t.name == "Duration" && t.kind == TagKind::Typedef));
    }

    #[test]
    fn go_const() {
        let src = "const MaxRetries = 3\n";
        let tags = extract_tags_from_content(src, "const.go", Language::Go);
        assert!(tags.iter().any(|t| t.name == "MaxRetries" && t.kind == TagKind::Constant));
    }

    #[test]
    fn go_package() {
        let src = "package main\n";
        let tags = extract_tags_from_content(src, "main.go", Language::Go);
        assert!(tags.iter().any(|t| t.name == "main" && t.kind == TagKind::Module));
    }

    // ---- Shell tag extraction ----

    #[test]
    fn shell_function_keyword() {
        let src = "function setup() {\n    echo setup\n}\n";
        let tags = extract_tags_from_content(src, "run.sh", Language::Shell);
        assert!(tags.iter().any(|t| t.name == "setup" && t.kind == TagKind::Function));
    }

    #[test]
    fn shell_function_parens() {
        let src = "cleanup() {\n    rm -rf /tmp/work\n}\n";
        let tags = extract_tags_from_content(src, "run.sh", Language::Shell);
        assert!(tags.iter().any(|t| t.name == "cleanup" && t.kind == TagKind::Function));
    }

    #[test]
    fn shell_variable() {
        let src = "MAX_JOBS=4\n";
        let tags = extract_tags_from_content(src, "config.sh", Language::Shell);
        assert!(tags.iter().any(|t| t.name == "MAX_JOBS" && t.kind == TagKind::Variable));
    }

    // ---- Output format tests ----

    #[test]
    fn ctags_output_has_header() {
        let tags = vec![Tag {
            name: "foo".to_string(),
            file: "test.c".to_string(),
            line_number: 1,
            pattern: "/^int foo() {$/".to_string(),
            kind: TagKind::Function,
            language: Language::C,
            scope: None,
        }];
        let mut buf = Vec::new();
        write_ctags(&tags, "fks", "", SortMode::Yes, &mut buf).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("!_TAG_FILE_FORMAT\t2"));
        assert!(output.contains("!_TAG_FILE_SORTED\t1"));
        assert!(output.contains("foo\ttest.c\t/^int foo() {$/"));
    }

    #[test]
    fn ctags_output_kind_field() {
        let tags = vec![Tag {
            name: "MyStruct".to_string(),
            file: "test.rs".to_string(),
            line_number: 5,
            pattern: "/^pub struct MyStruct {$/".to_string(),
            kind: TagKind::Struct,
            language: Language::Rust,
            scope: None,
        }];
        let mut buf = Vec::new();
        write_ctags(&tags, "k", "", SortMode::Yes, &mut buf).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("kind:s"));
    }

    #[test]
    fn ctags_output_line_field() {
        let tags = vec![Tag {
            name: "x".to_string(),
            file: "t.c".to_string(),
            line_number: 42,
            pattern: "/^int x = 0;$/".to_string(),
            kind: TagKind::Variable,
            language: Language::C,
            scope: None,
        }];
        let mut buf = Vec::new();
        write_ctags(&tags, "kn", "", SortMode::Yes, &mut buf).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("line:42"));
    }

    #[test]
    fn ctags_output_language_field() {
        let tags = vec![Tag {
            name: "f".to_string(),
            file: "t.py".to_string(),
            line_number: 1,
            pattern: "/^def f():$/".to_string(),
            kind: TagKind::Function,
            language: Language::Python,
            scope: None,
        }];
        let mut buf = Vec::new();
        write_ctags(&tags, "kl", "", SortMode::Yes, &mut buf).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("language:Python"));
    }

    #[test]
    fn ctags_output_scope_field() {
        let tags = vec![Tag {
            name: "method".to_string(),
            file: "t.rs".to_string(),
            line_number: 10,
            pattern: "/^    fn method() {$/".to_string(),
            kind: TagKind::Method,
            language: Language::Rust,
            scope: Some("MyImpl".to_string()),
        }];
        let mut buf = Vec::new();
        write_ctags(&tags, "ks", "", SortMode::Yes, &mut buf).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("scope:MyImpl"));
    }

    #[test]
    fn ctags_qualified_extra() {
        let tags = vec![Tag {
            name: "method".to_string(),
            file: "t.rs".to_string(),
            line_number: 10,
            pattern: "/^    fn method() {$/".to_string(),
            kind: TagKind::Method,
            language: Language::Rust,
            scope: Some("MyImpl".to_string()),
        }];
        let mut buf = Vec::new();
        write_ctags(&tags, "k", "q", SortMode::Yes, &mut buf).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("MyImpl.method"));
    }

    #[test]
    fn etags_output_format() {
        let tags = vec![Tag {
            name: "hello".to_string(),
            file: "test.c".to_string(),
            line_number: 3,
            pattern: "/^void hello() {$/".to_string(),
            kind: TagKind::Function,
            language: Language::C,
            scope: None,
        }];
        let mut buf = Vec::new();
        write_etags(&tags, &mut buf).unwrap();
        let output = String::from_utf8(buf).unwrap();
        // File header with \x0c.
        assert!(output.contains("\x0c\ntest.c,"));
        // Tag entry with \x7f and \x01.
        assert!(output.contains("\x7fhello\x01"));
    }

    // ---- Sort modes ----

    #[test]
    fn sort_yes() {
        let mut tags = vec![
            Tag {
                name: "zebra".to_string(),
                file: "a.c".to_string(),
                line_number: 1,
                pattern: String::new(),
                kind: TagKind::Function,
                language: Language::C,
                scope: None,
            },
            Tag {
                name: "apple".to_string(),
                file: "a.c".to_string(),
                line_number: 2,
                pattern: String::new(),
                kind: TagKind::Function,
                language: Language::C,
                scope: None,
            },
        ];
        sort_tags(&mut tags, SortMode::Yes);
        assert_eq!(tags[0].name, "apple");
        assert_eq!(tags[1].name, "zebra");
    }

    #[test]
    fn sort_foldcase() {
        let mut tags = vec![
            Tag {
                name: "Zebra".to_string(),
                file: "a.c".to_string(),
                line_number: 1,
                pattern: String::new(),
                kind: TagKind::Function,
                language: Language::C,
                scope: None,
            },
            Tag {
                name: "apple".to_string(),
                file: "a.c".to_string(),
                line_number: 2,
                pattern: String::new(),
                kind: TagKind::Function,
                language: Language::C,
                scope: None,
            },
        ];
        sort_tags(&mut tags, SortMode::Foldcase);
        assert_eq!(tags[0].name, "apple");
        assert_eq!(tags[1].name, "Zebra");
    }

    #[test]
    fn sort_no_preserves_order() {
        let mut tags = vec![
            Tag {
                name: "zebra".to_string(),
                file: "a.c".to_string(),
                line_number: 1,
                pattern: String::new(),
                kind: TagKind::Function,
                language: Language::C,
                scope: None,
            },
            Tag {
                name: "apple".to_string(),
                file: "a.c".to_string(),
                line_number: 2,
                pattern: String::new(),
                kind: TagKind::Function,
                language: Language::C,
                scope: None,
            },
        ];
        sort_tags(&mut tags, SortMode::No);
        assert_eq!(tags[0].name, "zebra");
        assert_eq!(tags[1].name, "apple");
    }

    // ---- Argument parsing ----

    #[test]
    fn parse_help() {
        let args = vec!["ctags".to_string(), "--help".to_string()];
        assert!(matches!(parse_args(&args), ParseResult::Help));
    }

    #[test]
    fn parse_version() {
        let args = vec!["ctags".to_string(), "--version".to_string()];
        assert!(matches!(parse_args(&args), ParseResult::Version));
    }

    #[test]
    fn parse_recurse() {
        let args = vec!["ctags".to_string(), "-R".to_string()];
        if let ParseResult::Run(c) = parse_args(&args) {
            assert!(c.recurse);
        } else {
            panic!("expected Run");
        }
    }

    #[test]
    fn parse_output_file() {
        let args = vec![
            "ctags".to_string(),
            "-f".to_string(),
            "mytags".to_string(),
            "a.c".to_string(),
        ];
        if let ParseResult::Run(c) = parse_args(&args) {
            assert_eq!(c.output_file.as_deref(), Some("mytags"));
            assert_eq!(c.files, vec!["a.c"]);
        } else {
            panic!("expected Run");
        }
    }

    #[test]
    fn parse_etags_mode() {
        let args = vec!["ctags".to_string(), "-e".to_string(), "a.c".to_string()];
        if let ParseResult::Run(c) = parse_args(&args) {
            assert_eq!(c.format, OutputFormat::Etags);
        } else {
            panic!("expected Run");
        }
    }

    #[test]
    fn parse_etags_personality() {
        let args = vec!["etags".to_string(), "a.c".to_string()];
        if let ParseResult::Run(c) = parse_args(&args) {
            assert_eq!(c.format, OutputFormat::Etags);
        } else {
            panic!("expected Run");
        }
    }

    #[test]
    fn parse_sort_modes() {
        let args = vec![
            "ctags".to_string(),
            "--sort=foldcase".to_string(),
            "a.c".to_string(),
        ];
        if let ParseResult::Run(c) = parse_args(&args) {
            assert_eq!(c.sort, SortMode::Foldcase);
        } else {
            panic!("expected Run");
        }
    }

    #[test]
    fn parse_unsorted() {
        let args = vec!["ctags".to_string(), "-u".to_string(), "a.c".to_string()];
        if let ParseResult::Run(c) = parse_args(&args) {
            assert_eq!(c.sort, SortMode::No);
        } else {
            panic!("expected Run");
        }
    }

    #[test]
    fn parse_exclude() {
        let args = vec![
            "ctags".to_string(),
            "--exclude=*.o".to_string(),
            "--exclude=build".to_string(),
            "a.c".to_string(),
        ];
        if let ParseResult::Run(c) = parse_args(&args) {
            assert_eq!(c.exclude_patterns, vec!["*.o", "build"]);
        } else {
            panic!("expected Run");
        }
    }

    #[test]
    fn parse_fields() {
        let args = vec![
            "ctags".to_string(),
            "--fields=+l".to_string(),
            "a.c".to_string(),
        ];
        if let ParseResult::Run(c) = parse_args(&args) {
            assert_eq!(c.fields, "+l");
        } else {
            panic!("expected Run");
        }
    }

    #[test]
    fn parse_extras() {
        let args = vec![
            "ctags".to_string(),
            "--extras=+q".to_string(),
            "a.c".to_string(),
        ];
        if let ParseResult::Run(c) = parse_args(&args) {
            assert_eq!(c.extras, "+q");
        } else {
            panic!("expected Run");
        }
    }

    #[test]
    fn parse_append() {
        let args = vec!["ctags".to_string(), "-a".to_string(), "a.c".to_string()];
        if let ParseResult::Run(c) = parse_args(&args) {
            assert!(c.append);
        } else {
            panic!("expected Run");
        }
    }

    #[test]
    fn parse_combined_flags() {
        let args = vec!["ctags".to_string(), "-Rae".to_string(), "a.c".to_string()];
        if let ParseResult::Run(c) = parse_args(&args) {
            assert!(c.recurse);
            assert!(c.append);
            assert_eq!(c.format, OutputFormat::Etags);
        } else {
            panic!("expected Run");
        }
    }

    #[test]
    fn parse_combined_f_with_value() {
        let args = vec![
            "ctags".to_string(),
            "-Rf".to_string(),
            "out.tags".to_string(),
        ];
        if let ParseResult::Run(c) = parse_args(&args) {
            assert!(c.recurse);
            assert_eq!(c.output_file.as_deref(), Some("out.tags"));
        } else {
            panic!("expected Run");
        }
    }

    // ---- Exclusion / file collection ----

    #[test]
    fn is_excluded_glob() {
        assert!(is_excluded("build/foo.o", &["*.o".to_string()]));
        assert!(!is_excluded("src/main.c", &["*.o".to_string()]));
    }

    #[test]
    fn is_excluded_dir_name() {
        assert!(is_excluded("node_modules/foo.js", &["node_modules".to_string()]));
    }

    // ---- Tag kind properties ----

    #[test]
    fn tag_kind_letters() {
        assert_eq!(TagKind::Function.letter(), 'f');
        assert_eq!(TagKind::Struct.letter(), 's');
        assert_eq!(TagKind::Class.letter(), 'c');
        assert_eq!(TagKind::Enum.letter(), 'g');
        assert_eq!(TagKind::Typedef.letter(), 't');
        assert_eq!(TagKind::Macro.letter(), 'd');
        assert_eq!(TagKind::Variable.letter(), 'v');
        assert_eq!(TagKind::Interface.letter(), 'i');
        assert_eq!(TagKind::Trait.letter(), 'i');
    }

    #[test]
    fn tag_kind_names() {
        assert_eq!(TagKind::Function.name(), "function");
        assert_eq!(TagKind::Struct.name(), "struct");
        assert_eq!(TagKind::Class.name(), "class");
        assert_eq!(TagKind::Module.name(), "module");
    }

    #[test]
    fn language_names() {
        assert_eq!(Language::C.name(), "C");
        assert_eq!(Language::Cpp.name(), "C++");
        assert_eq!(Language::Rust.name(), "Rust");
        assert_eq!(Language::Python.name(), "Python");
        assert_eq!(Language::Java.name(), "Java");
        assert_eq!(Language::JavaScript.name(), "JavaScript");
        assert_eq!(Language::Go.name(), "Go");
        assert_eq!(Language::Shell.name(), "Shell");
    }

    // ---- Parse ctags line ----

    #[test]
    fn parse_ctags_line_basic() {
        let line = "foo\ttest.c\t/^int foo() {$/;\"\tf";
        let tag = parse_ctags_line(line).unwrap();
        assert_eq!(tag.name, "foo");
        assert_eq!(tag.file, "test.c");
    }

    // ---- Default output filenames ----

    #[test]
    fn default_stdin_when_no_files() {
        let args = vec!["ctags".to_string()];
        if let ParseResult::Run(c) = parse_args(&args) {
            assert_eq!(c.files, vec!["-"]);
        } else {
            panic!("expected Run");
        }
    }
}
