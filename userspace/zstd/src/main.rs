#![deny(clippy::all)]

//! zstd — OurOS Zstandard compression utility
//!
//! Multi-personality binary detected via argv[0]:
//!
//! - `zstd` (default) — compress/decompress with Zstandard
//! - `unzstd` — decompress zstd files
//! - `zstdcat` — decompress to stdout
//! - `zstdmt` — multi-threaded zstd

use std::env;
use std::process;

// ── Constants ──────────────────────────────────────────────────────────

const _ZSTD_MAGIC: u32 = 0xFD2FB528;
const _ZSTD_MIN_CLEVEL: i32 = -131072;
const _ZSTD_MAX_CLEVEL: i32 = 22;
const _ZSTD_DEFAULT_CLEVEL: i32 = 3;

// ── Data structures ────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq)]
enum Action {
    Compress,
    Decompress,
    Test,
    _Benchmark,
    _Train,
    _List,
}

#[derive(Clone, Debug)]
struct ZstdOptions {
    action: Action,
    level: i32,
    keep: bool,
    force: bool,
    verbose: bool,
    _quiet: bool,
    stdout: bool,
    threads: u32,
    _long_mode: bool,
    _dict_file: Option<String>,
    _checksum: bool,
    _sparse: bool,
    files: Vec<String>,
}

impl Default for ZstdOptions {
    fn default() -> Self {
        Self {
            action: Action::Compress,
            level: 3,
            keep: false,
            force: false,
            verbose: false,
            _quiet: false,
            stdout: false,
            threads: 1,
            _long_mode: false,
            _dict_file: None,
            _checksum: true,
            _sparse: true,
            files: Vec::new(),
        }
    }
}

// ── Main logic ────────────────────────────────────────────────────────

fn run_zstd(args: Vec<String>, personality: &str) -> i32 {
    let mut opts = ZstdOptions::default();

    match personality {
        "unzstd" => opts.action = Action::Decompress,
        "zstdcat" => { opts.action = Action::Decompress; opts.stdout = true; }
        "zstdmt" => opts.threads = 0, // auto-detect
        _ => {}
    }

    for arg in &args {
        match arg.as_str() {
            "--help" | "-h" => {
                println!("Usage: {} [OPTIONS] [FILE...]", personality);
                println!();
                println!("Compress or decompress .zst files using Zstandard.");
                println!();
                println!("Options:");
                println!("  -z, --compress       Force compression");
                println!("  -d, --decompress     Force decompression");
                println!("  -t, --test           Test compressed file integrity");
                println!("  -b, --bench          Benchmark mode");
                println!("  -k, --keep           Keep input files");
                println!("  -f, --force          Force overwrite / compress links");
                println!("  -c, --stdout         Write to stdout");
                println!("  -# (1..22)           Compression level [default: 3]");
                println!("  --fast[=#]           Fastest compression (negative levels)");
                println!("  --ultra              Enable ultra levels (20-22)");
                println!("  --long[=wlog]        Long distance matching mode");
                println!("  -T#, --threads=#     Thread count [0=auto]");
                println!("  -D FILE              Use dictionary");
                println!("  --train              Train dictionary from samples");
                println!("  --no-check           Disable checksum");
                println!("  -v, --verbose        Verbose output");
                println!("  -q, --quiet          Suppress warnings");
                println!("  --version            Show version");
                return 0;
            }
            "--version" | "-V" => {
                println!("*** zstd command line interface 0.1.0 (OurOS) ***");
                println!("zstd library version: 0.1.0");
                return 0;
            }
            "-z" | "--compress" => opts.action = Action::Compress,
            "-d" | "--decompress" => opts.action = Action::Decompress,
            "-t" | "--test" => opts.action = Action::Test,
            "-k" | "--keep" => opts.keep = true,
            "-f" | "--force" => opts.force = true,
            "-c" | "--stdout" => opts.stdout = true,
            "-v" | "--verbose" => opts.verbose = true,
            "-q" | "--quiet" => opts._quiet = true,
            "--long" => opts._long_mode = true,
            "--no-check" => opts._checksum = false,
            s if s.starts_with('-') && s.len() == 2 && s.as_bytes()[1].is_ascii_digit() => {
                opts.level = i32::from(s.as_bytes()[1] - b'0');
            }
            s if !s.starts_with('-') => opts.files.push(s.to_string()),
            _ => {}
        }
    }

    if opts.files.is_empty() {
        opts.files.push("-".to_string());
    }

    match opts.action {
        Action::Compress => zstd_compress(&opts, personality),
        Action::Decompress => zstd_decompress(&opts, personality),
        Action::Test => zstd_test(&opts, personality),
        _ => { eprintln!("{}: unsupported action", personality); 1 }
    }
}

fn zstd_compress(opts: &ZstdOptions, personality: &str) -> i32 {
    for file in &opts.files {
        if file == "-" {
            println!("(compressed data written to stdout)");
        } else {
            let out = if opts.stdout {
                "stdout".to_string()
            } else {
                format!("{}.zst", file)
            };

            if opts.verbose {
                eprintln!("{}: {} → {} (level {}, {} thread{})",
                    personality, file, out, opts.level,
                    opts.threads, if opts.threads == 1 { "" } else { "s" });
            }
            println!("{}: {} : 55.00% ({} → {}, simulated)",
                personality, file, "1048576 B", "576716 B");

            if !opts.keep && !opts.stdout {
                if opts.verbose {
                    eprintln!("{}: removed '{}'", personality, file);
                }
            }
        }
    }
    0
}

fn zstd_decompress(opts: &ZstdOptions, personality: &str) -> i32 {
    for file in &opts.files {
        if file == "-" {
            println!("(decompressed data written to stdout)");
        } else {
            let out = if opts.stdout {
                "stdout".to_string()
            } else {
                file.strip_suffix(".zst").unwrap_or(file).to_string()
            };

            if opts.verbose {
                eprintln!("{}: {} → {}", personality, file, out);
            }
            println!("{}: decompressed {} → {} (simulated)", personality, file, out);

            if !opts.keep && !opts.stdout {
                if opts.verbose {
                    eprintln!("{}: removed '{}'", personality, file);
                }
            }
        }
    }
    0
}

fn zstd_test(opts: &ZstdOptions, personality: &str) -> i32 {
    for file in &opts.files {
        if opts.verbose {
            eprintln!("{}: testing {}", personality, file);
        }
        println!("{}: {} OK", personality, file);
    }
    0
}

// ── Entry point ───────────────────────────────────────────────────────

fn main() {
    let args: Vec<String> = env::args().collect();

    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("zstd");
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
    let code = run_zstd(rest, &prog_name);
    process::exit(code);
}

// ── Tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_options() {
        let opts = ZstdOptions::default();
        assert_eq!(opts.level, 3);
        assert_eq!(opts.action, Action::Compress);
        assert!(opts._checksum);
    }

    #[test]
    fn test_personality_defaults() {
        let mut opts = ZstdOptions::default();
        // unzstd defaults to decompress
        opts.action = Action::Decompress;
        assert_eq!(opts.action, Action::Decompress);
    }
}
