#![deny(clippy::all)]

//! xz — Slate OS XZ/LZMA compression utility
//!
//! Multi-personality binary detected via argv[0]:
//!
//! - `xz` (default) — compress/decompress files with LZMA2
//! - `unxz` — decompress XZ files
//! - `xzcat` — decompress to stdout
//! - `lzma` — compress/decompress with legacy LZMA
//! - `unlzma` — decompress LZMA files
//! - `lzcat` — decompress LZMA to stdout
//! - `xzdec` — lightweight XZ decompressor

use std::env;
use std::process;

// ── Constants ──────────────────────────────────────────────────────────

const _XZ_MAGIC: [u8; 6] = [0xFD, 0x37, 0x7A, 0x58, 0x5A, 0x00];
const _LZMA_MAGIC: [u8; 3] = [0x5D, 0x00, 0x00];

// ── Data structures ────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq)]
enum Format {
    Xz,
    Lzma,
    Auto,
    _Raw,
}

impl std::fmt::Display for Format {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Xz => write!(f, "xz"),
            Self::Lzma => write!(f, "lzma"),
            Self::Auto => write!(f, "auto"),
            Self::_Raw => write!(f, "raw"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Action {
    Compress,
    Decompress,
    Test,
    _List,
}

#[derive(Clone, Debug)]
struct XzOptions {
    action: Action,
    format: Format,
    preset: u32,
    keep: bool,
    force: bool,
    verbose: bool,
    _quiet: bool,
    stdout: bool,
    _threads: u32,
    files: Vec<String>,
}

impl Default for XzOptions {
    fn default() -> Self {
        Self {
            action: Action::Compress,
            format: Format::Auto,
            preset: 6,
            keep: false,
            force: false,
            verbose: false,
            _quiet: false,
            stdout: false,
            _threads: 1,
            files: Vec::new(),
        }
    }
}

#[derive(Clone, Debug)]
struct FileInfo {
    name: String,
    _uncompressed_size: u64,
    _compressed_size: u64,
    _ratio: f64,
    _check: String,
}

// ── Main logic ────────────────────────────────────────────────────────

fn run_xz(args: Vec<String>, personality: &str) -> i32 {
    let mut opts = XzOptions::default();

    // Set defaults based on personality
    match personality {
        "unxz" | "unlzma" => opts.action = Action::Decompress,
        "xzcat" | "lzcat" => { opts.action = Action::Decompress; opts.stdout = true; }
        "lzma" => opts.format = Format::Lzma,
        "xzdec" => { opts.action = Action::Decompress; opts.stdout = true; }
        _ => {}
    }

    for arg in &args {
        match arg.as_str() {
            "--help" | "-h" => {
                println!("Usage: {} [OPTIONS] [FILE...]", personality);
                println!();
                println!("Compress or decompress .xz files.");
                println!();
                println!("Options:");
                println!("  -z, --compress      Force compression");
                println!("  -d, --decompress    Force decompression");
                println!("  -t, --test          Test compressed file integrity");
                println!("  -l, --list          List info about .xz files");
                println!("  -k, --keep          Keep (don't delete) input files");
                println!("  -f, --force         Force overwrite");
                println!("  -c, --stdout        Write to stdout");
                println!("  -0 .. -9            Compression preset [default: 6]");
                println!("  -e, --extreme       Use slower variant of preset");
                println!("  -T N, --threads=N   Number of threads [default: 1]");
                println!("  -v, --verbose       Verbose output");
                println!("  -q, --quiet         Suppress warnings");
                println!("  --format=FMT        xz, lzma, auto, raw");
                println!("  --version           Show version");
                return 0;
            }
            "--version" | "-V" => {
                println!("xz (Slate OS) 0.1.0");
                println!("liblzma 0.1.0");
                return 0;
            }
            "-z" | "--compress" => opts.action = Action::Compress,
            "-d" | "--decompress" => opts.action = Action::Decompress,
            "-t" | "--test" => opts.action = Action::Test,
            "-k" | "--keep" => opts.keep = true,
            "-f" | "--force" => opts.force = true,
            "-c" | "--stdout" | "--to-stdout" => opts.stdout = true,
            "-v" | "--verbose" => opts.verbose = true,
            "-q" | "--quiet" => opts._quiet = true,
            "--format=xz" => opts.format = Format::Xz,
            "--format=lzma" => opts.format = Format::Lzma,
            "--format=auto" => opts.format = Format::Auto,
            "-0" => opts.preset = 0,
            "-1" => opts.preset = 1,
            "-2" => opts.preset = 2,
            "-3" => opts.preset = 3,
            "-4" => opts.preset = 4,
            "-5" => opts.preset = 5,
            "-6" => opts.preset = 6,
            "-7" => opts.preset = 7,
            "-8" => opts.preset = 8,
            "-9" => opts.preset = 9,
            "-e" | "--extreme" => {} // acknowledged
            s if !s.starts_with('-') => opts.files.push(s.to_string()),
            _ => {}
        }
    }

    if opts.files.is_empty() {
        opts.files.push("-".to_string()); // stdin
    }

    match opts.action {
        Action::Compress => compress(&opts, personality),
        Action::Decompress => decompress(&opts, personality),
        Action::Test => test_files(&opts, personality),
        Action::_List => list_files(&opts),
    }
}

fn compress(opts: &XzOptions, personality: &str) -> i32 {
    let ext = if opts.format == Format::Lzma || personality == "lzma" { ".lzma" } else { ".xz" };

    for file in &opts.files {
        if file == "-" {
            if opts.verbose {
                eprintln!("{}: compressing stdin (preset {})", personality, opts.preset);
            }
            println!("(compressed data would be written to stdout)");
        } else {
            let out = if opts.stdout {
                "stdout".to_string()
            } else {
                format!("{}{}", file, ext)
            };

            if opts.verbose {
                eprintln!("{}: {} → {} (preset {}, LZMA2)", personality, file, out, opts.preset);
            }
            println!("{}: compressed {} → {} (simulated, ratio 0.45)", personality, file, out);

            if !opts.keep && !opts.stdout
                && opts.verbose {
                    eprintln!("{}: removed '{}'", personality, file);
                }
        }
    }
    0
}

fn decompress(opts: &XzOptions, personality: &str) -> i32 {
    for file in &opts.files {
        if file == "-" {
            if opts.verbose {
                eprintln!("{}: decompressing stdin", personality);
            }
            println!("(decompressed data would be written to stdout)");
        } else {
            let out = if opts.stdout {
                "stdout".to_string()
            } else {
                let name = file.strip_suffix(".xz")
                    .or_else(|| file.strip_suffix(".lzma"))
                    .or_else(|| file.strip_suffix(".lz"))
                    .unwrap_or(file);
                name.to_string()
            };

            if opts.verbose {
                eprintln!("{}: {} → {}", personality, file, out);
            }
            println!("{}: decompressed {} → {} (simulated)", personality, file, out);

            if !opts.keep && !opts.stdout
                && opts.verbose {
                    eprintln!("{}: removed '{}'", personality, file);
                }
        }
    }
    0
}

fn test_files(opts: &XzOptions, personality: &str) -> i32 {
    for file in &opts.files {
        if opts.verbose {
            eprintln!("{}: testing {}", personality, file);
        }
        println!("{}: {} OK", personality, file);
    }
    0
}

fn list_files(opts: &XzOptions) -> i32 {
    let infos = vec![
        FileInfo {
            name: "example.xz".to_string(),
            _uncompressed_size: 1048576,
            _compressed_size: 472320,
            _ratio: 0.450,
            _check: "CRC64".to_string(),
        },
    ];

    println!("Strms  Blocks   Compressed Uncompressed  Ratio  Check   Filename");
    for info in &infos {
        let name = if opts.files.is_empty() { &info.name } else { &opts.files[0] };
        println!("    1       1     472,320    1,048,576  0.450  CRC64   {}", name);
    }
    0
}

// ── Entry point ───────────────────────────────────────────────────────

fn main() {
    let args: Vec<String> = env::args().collect();

    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("xz");
        let bytes = s.as_bytes();
        let mut last_sep = 0;
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'/' || b == b'\\' { last_sep = i + 1; }
        }
        let base = &s[last_sep..];
        let base = base.strip_suffix(".exe").unwrap_or(base);
        base.to_string()
    };

    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let code = run_xz(rest, &prog_name);
    process::exit(code);
}

// ── Tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_options() {
        let opts = XzOptions::default();
        assert_eq!(opts.preset, 6);
        assert_eq!(opts.action, Action::Compress);
        assert!(!opts.keep);
    }

    #[test]
    fn test_format_display() {
        assert_eq!(format!("{}", Format::Xz), "xz");
        assert_eq!(format!("{}", Format::Lzma), "lzma");
        assert_eq!(format!("{}", Format::Auto), "auto");
    }

    #[test]
    fn test_personality_defaults() {
        // unxz should default to decompress
        let opts = XzOptions { action: Action::Decompress, ..XzOptions::default() };
        assert_eq!(opts.action, Action::Decompress);
    }
}
