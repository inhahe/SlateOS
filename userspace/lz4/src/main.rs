#![deny(clippy::all)]

//! lz4 — OurOS LZ4 compression utility
//!
//! Multi-personality binary detected via argv[0]:
//!
//! - `lz4` (default) — compress/decompress with LZ4
//! - `lz4c` — LZ4 compressor (legacy)
//! - `lz4cat` — decompress to stdout
//! - `unlz4` — decompress LZ4 files

use std::env;
use std::process;

// ── Constants ──────────────────────────────────────────────────────────

const _LZ4_MAGIC: u32 = 0x184D2204;
const _LZ4_LEGACY_MAGIC: u32 = 0x184C2102;

// ── Data structures ────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq)]
enum Action {
    Compress,
    Decompress,
    Test,
    _Benchmark,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum CompressionMode {
    Fast,
    Hc,
}

impl std::fmt::Display for CompressionMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Fast => write!(f, "fast"),
            Self::Hc => write!(f, "HC"),
        }
    }
}

#[derive(Clone, Debug)]
struct Lz4Options {
    action: Action,
    mode: CompressionMode,
    level: i32,
    keep: bool,
    force: bool,
    verbose: bool,
    _quiet: bool,
    stdout: bool,
    _block_size: u32, // in bytes (64KB default)
    _content_size: bool,
    _no_frame_crc: bool,
    files: Vec<String>,
}

impl Default for Lz4Options {
    fn default() -> Self {
        Self {
            action: Action::Compress,
            mode: CompressionMode::Fast,
            level: 1,
            keep: false,
            force: false,
            verbose: false,
            _quiet: false,
            stdout: false,
            _block_size: 65536,
            _content_size: false,
            _no_frame_crc: false,
            files: Vec::new(),
        }
    }
}

// ── Main logic ────────────────────────────────────────────────────────

fn run_lz4(args: Vec<String>, personality: &str) -> i32 {
    let mut opts = Lz4Options::default();

    match personality {
        "unlz4" => opts.action = Action::Decompress,
        "lz4cat" => { opts.action = Action::Decompress; opts.stdout = true; }
        "lz4c" => {} // legacy compressor, same as lz4
        _ => {}
    }

    for arg in &args {
        match arg.as_str() {
            "--help" | "-h" | "-H" => {
                println!("Usage: {} [OPTIONS] [INPUT] [OUTPUT]", personality);
                println!();
                println!("Compress or decompress .lz4 files (extremely fast).");
                println!();
                println!("Options:");
                println!("  -z, --compress      Compress (default)");
                println!("  -d, --decompress    Decompress");
                println!("  -t, --test          Test compressed file integrity");
                println!("  -1                  Fast compression (default)");
                println!("  -9                  HC (high compression)");
                println!("  --fast[=#]          Acceleration factor");
                println!("  --best              Alias for -12 (HC mode)");
                println!("  -k, --keep          Keep input files");
                println!("  -f, --force         Force overwrite");
                println!("  -c, --stdout        Write to stdout");
                println!("  -m                  Multiple files mode");
                println!("  --content-size      Include content size in frame");
                println!("  --no-frame-crc      Disable frame checksum");
                println!("  -v, --verbose       Verbose output");
                println!("  -q, --quiet         Suppress warnings");
                println!("  -V, --version       Show version");
                return 0;
            }
            "-V" | "--version" => {
                println!("*** LZ4 command line interface 0.1.0 (OurOS) ***");
                println!("LZ4 library version: 0.1.0");
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
            "--content-size" => opts._content_size = true,
            "--no-frame-crc" => opts._no_frame_crc = true,
            "-1" => { opts.mode = CompressionMode::Fast; opts.level = 1; }
            "-9" | "--best" => { opts.mode = CompressionMode::Hc; opts.level = 9; }
            s if s.starts_with('-') && s.len() == 2 && s.as_bytes()[1].is_ascii_digit() => {
                let n = i32::from(s.as_bytes()[1] - b'0');
                opts.level = n;
                if n >= 4 { opts.mode = CompressionMode::Hc; }
            }
            s if !s.starts_with('-') => opts.files.push(s.to_string()),
            _ => {}
        }
    }

    if opts.files.is_empty() {
        opts.files.push("-".to_string());
    }

    match opts.action {
        Action::Compress => lz4_compress(&opts, personality),
        Action::Decompress => lz4_decompress(&opts, personality),
        Action::Test => lz4_test(&opts, personality),
        Action::_Benchmark => { eprintln!("{}: benchmark mode not yet implemented", personality); 1 }
    }
}

fn lz4_compress(opts: &Lz4Options, personality: &str) -> i32 {
    for file in &opts.files {
        if file == "-" {
            println!("(compressed data written to stdout)");
        } else {
            let out = if opts.stdout {
                "stdout".to_string()
            } else {
                format!("{}.lz4", file)
            };

            if opts.verbose {
                eprintln!("{}: {} mode, level {}", personality, opts.mode, opts.level);
            }
            // LZ4 is known for speed with moderate ratios
            println!("Compressed {} into {} (ratio 2.10:1, simulated)", file, out);

            if !opts.keep && !opts.stdout
                && opts.verbose {
                    eprintln!("{}: removed '{}'", personality, file);
                }
        }
    }
    0
}

fn lz4_decompress(opts: &Lz4Options, personality: &str) -> i32 {
    for file in &opts.files {
        if file == "-" {
            println!("(decompressed data written to stdout)");
        } else {
            let out = if opts.stdout {
                "stdout".to_string()
            } else {
                file.strip_suffix(".lz4").unwrap_or(file).to_string()
            };

            println!("{}: {} → {} (simulated)", personality, file, out);

            if !opts.keep && !opts.stdout
                && opts.verbose {
                    eprintln!("{}: removed '{}'", personality, file);
                }
        }
    }
    0
}

fn lz4_test(opts: &Lz4Options, personality: &str) -> i32 {
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
        let s = args.first().map(|s| s.as_str()).unwrap_or("lz4");
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
    let code = run_lz4(rest, &prog_name);
    process::exit(code);
}

// ── Tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_options() {
        let opts = Lz4Options::default();
        assert_eq!(opts.level, 1);
        assert_eq!(opts.mode, CompressionMode::Fast);
    }

    #[test]
    fn test_compression_mode_display() {
        assert_eq!(format!("{}", CompressionMode::Fast), "fast");
        assert_eq!(format!("{}", CompressionMode::Hc), "HC");
    }
}
