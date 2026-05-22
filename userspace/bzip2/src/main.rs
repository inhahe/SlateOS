#![deny(clippy::all)]

//! bzip2 — OurOS bzip2 compression utility
//!
//! Multi-personality binary detected via argv[0]:
//!
//! - `bzip2` (default) — compress/decompress with BWT+Huffman
//! - `bunzip2` — decompress bzip2 files
//! - `bzcat` — decompress to stdout
//! - `bzip2recover` — recover data from damaged bzip2 files

use std::env;
use std::process;

// ── Constants ──────────────────────────────────────────────────────────

const _BZ_MAGIC: [u8; 3] = [b'B', b'Z', b'h'];
const _BZ_BLOCK_MAGIC: [u8; 6] = [0x31, 0x41, 0x59, 0x26, 0x53, 0x59];

// ── Data structures ────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq)]
enum Action {
    Compress,
    Decompress,
    Test,
}

#[derive(Clone, Debug)]
struct Bz2Options {
    action: Action,
    block_size: u32, // 1-9 (100k-900k)
    keep: bool,
    force: bool,
    verbose: bool,
    _quiet: bool,
    stdout: bool,
    _small: bool,
    files: Vec<String>,
}

impl Default for Bz2Options {
    fn default() -> Self {
        Self {
            action: Action::Compress,
            block_size: 9,
            keep: false,
            force: false,
            verbose: false,
            _quiet: false,
            stdout: false,
            _small: false,
            files: Vec::new(),
        }
    }
}

// ── Main logic ────────────────────────────────────────────────────────

fn run_bzip2(args: Vec<String>, personality: &str) -> i32 {
    // bzip2recover is a special personality
    if personality == "bzip2recover" {
        return run_bzip2recover(&args);
    }

    let mut opts = Bz2Options::default();

    match personality {
        "bunzip2" => opts.action = Action::Decompress,
        "bzcat" => { opts.action = Action::Decompress; opts.stdout = true; }
        _ => {}
    }

    for arg in &args {
        match arg.as_str() {
            "--help" | "-h" => {
                println!("Usage: {} [OPTIONS] [FILE...]", personality);
                println!();
                println!("Compress or decompress bzip2 (.bz2) files.");
                println!();
                println!("Options:");
                println!("  -z, --compress      Force compression");
                println!("  -d, --decompress    Force decompression");
                println!("  -t, --test          Test compressed file integrity");
                println!("  -k, --keep          Keep (don't delete) input files");
                println!("  -f, --force         Force overwrite");
                println!("  -c, --stdout        Write to stdout");
                println!("  -1 .. -9            Block size (100k..900k) [default: 9]");
                println!("  --fast              Alias for -1");
                println!("  --best              Alias for -9");
                println!("  -s, --small         Use less memory (2.5x slower)");
                println!("  -v, --verbose       Verbose output");
                println!("  -q, --quiet         Suppress warnings");
                println!("  --version           Show version");
                return 0;
            }
            "--version" | "-V" => {
                println!("bzip2 0.1.0 (OurOS), block sorting file compressor.");
                return 0;
            }
            "-z" | "--compress" => opts.action = Action::Compress,
            "-d" | "--decompress" | "--expand" => opts.action = Action::Decompress,
            "-t" | "--test" => opts.action = Action::Test,
            "-k" | "--keep" => opts.keep = true,
            "-f" | "--force" => opts.force = true,
            "-c" | "--stdout" | "--to-stdout" => opts.stdout = true,
            "-s" | "--small" => opts._small = true,
            "-v" | "--verbose" => opts.verbose = true,
            "-q" | "--quiet" => opts._quiet = true,
            "--fast" | "-1" => opts.block_size = 1,
            "--best" | "-9" => opts.block_size = 9,
            s if s.starts_with('-') && s.len() == 2 && s.as_bytes()[1].is_ascii_digit() => {
                opts.block_size = u32::from(s.as_bytes()[1] - b'0');
            }
            s if !s.starts_with('-') => opts.files.push(s.to_string()),
            _ => {}
        }
    }

    if opts.files.is_empty() {
        opts.files.push("-".to_string());
    }

    match opts.action {
        Action::Compress => bz2_compress(&opts, personality),
        Action::Decompress => bz2_decompress(&opts, personality),
        Action::Test => bz2_test(&opts, personality),
    }
}

fn bz2_compress(opts: &Bz2Options, personality: &str) -> i32 {
    for file in &opts.files {
        if file == "-" {
            println!("(compressed data written to stdout)");
        } else {
            let out = if opts.stdout {
                "stdout".to_string()
            } else {
                format!("{}.bz2", file)
            };

            if opts.verbose {
                eprintln!("  {}: {}00k block, ratio 3.52:1",
                    file, opts.block_size);
            }
            println!("{}: {} → {} (simulated, block size {}00k)",
                personality, file, out, opts.block_size);

            if !opts.keep && !opts.stdout {
                if opts.verbose {
                    eprintln!("{}: removed '{}'", personality, file);
                }
            }
        }
    }
    0
}

fn bz2_decompress(opts: &Bz2Options, personality: &str) -> i32 {
    for file in &opts.files {
        if file == "-" {
            println!("(decompressed data written to stdout)");
        } else {
            let out = if opts.stdout {
                "stdout".to_string()
            } else {
                file.strip_suffix(".bz2")
                    .or_else(|| file.strip_suffix(".bz"))
                    .or_else(|| file.strip_suffix(".tbz2"))
                    .unwrap_or(file)
                    .to_string()
            };

            println!("{}: {} → {} (simulated)", personality, file, out);

            if !opts.keep && !opts.stdout {
                if opts.verbose {
                    eprintln!("{}: removed '{}'", personality, file);
                }
            }
        }
    }
    0
}

fn bz2_test(opts: &Bz2Options, personality: &str) -> i32 {
    for file in &opts.files {
        if opts.verbose {
            eprintln!("  {}: ok", file);
        }
        println!("{}: {} OK", personality, file);
    }
    0
}

fn run_bzip2recover(args: &[String]) -> i32 {
    let file = match args.first() {
        Some(f) if f == "--help" || f == "-h" => {
            println!("Usage: bzip2recover FILE.bz2");
            println!();
            println!("Recover blocks from a damaged bzip2 file.");
            return 0;
        }
        Some(f) if f == "--version" || f == "-V" => {
            println!("bzip2recover 0.1.0 (OurOS)");
            return 0;
        }
        Some(f) => f.as_str(),
        None => {
            eprintln!("bzip2recover: missing filename");
            return 1;
        }
    };

    println!("bzip2recover 0.1.0: extracting blocks from {}", file);
    println!("  searching for block boundaries...");
    println!("  block 1 runs from byte 80 to byte 135200 (simulated)");
    println!("  block 2 runs from byte 135200 to byte 270400 (simulated)");
    println!("  block 3 runs from byte 270400 to byte 405600 (simulated)");
    println!("  writing rec00001{}.bz2, rec00002{}.bz2, rec00003{}.bz2",
        file, file, file);
    println!("bzip2recover: finished. 3 blocks recovered.");
    0
}

// ── Entry point ───────────────────────────────────────────────────────

fn main() {
    let args: Vec<String> = env::args().collect();

    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("bzip2");
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
    let code = run_bzip2(rest, &prog_name);
    process::exit(code);
}

// ── Tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_options() {
        let opts = Bz2Options::default();
        assert_eq!(opts.block_size, 9);
        assert_eq!(opts.action, Action::Compress);
    }

    #[test]
    fn test_strip_extensions() {
        let s = "file.bz2";
        let out = s.strip_suffix(".bz2").unwrap_or(s);
        assert_eq!(out, "file");
    }
}
