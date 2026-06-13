#![deny(clippy::all)]

//! ouch — Slate OS painless compression/decompression tool
//!
//! Single personality: `ouch`

use std::env;
use std::process;

fn run_ouch(args: Vec<String>) -> i32 {
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("");
    match cmd {
        "--help" | "-h" | "" => {
            println!("Usage: ouch <COMMAND> [OPTIONS] [FILES]...");
            println!();
            println!("Painless compression and decompression in the terminal.");
            println!();
            println!("Commands:");
            println!("  compress     Compress files (alias: c)");
            println!("  decompress   Decompress files (alias: d)");
            println!("  list         List archive contents (alias: l, ls)");
            println!();
            println!("Supported formats:");
            println!("  .tar, .tar.gz/.tgz, .tar.bz2/.tbz2, .tar.xz/.txz,");
            println!("  .tar.lz4, .tar.lzma, .tar.sz, .tar.zst/.tzst,");
            println!("  .zip, .gz, .bz2, .xz, .lz4, .lzma, .sz, .zst, .7z, .rar");
            println!();
            println!("Options:");
            println!("  -y, --yes          Skip confirmation prompts");
            println!("  -n, --no           Decline all prompts");
            println!("  -A, --accessible   Screen-reader friendly output");
            println!("  -H, --hidden       Include hidden files");
            println!("  -q, --quiet        Suppress output");
            println!("  -g, --gitignore    Respect .gitignore");
            println!("  --format <FMT>     Force output format");
            println!("  --level <N>        Compression level (1-22, format dependent)");
            println!("  -V, --version      Show version");
            0
        }
        "--version" | "-V" => {
            println!("ouch 0.5.1 (Slate OS)");
            0
        }
        "compress" | "c" => {
            let rest: Vec<&str> = args[1..].iter()
                .filter(|a| !a.starts_with('-'))
                .map(|s| s.as_str())
                .collect();

            if rest.len() < 2 {
                eprintln!("Error: compress requires input files and an output archive.");
                eprintln!("Usage: ouch compress <FILES>... <OUTPUT>");
                return 1;
            }

            let output = rest.last().copied().unwrap_or("archive.tar.gz");
            let inputs: Vec<&str> = rest[..rest.len() - 1].to_vec();

            let format = if output.ends_with(".tar.gz") || output.ends_with(".tgz") {
                "tar.gz"
            } else if output.ends_with(".tar.zst") || output.ends_with(".tzst") {
                "tar.zst"
            } else if output.ends_with(".zip") {
                "zip"
            } else if output.ends_with(".tar.xz") || output.ends_with(".txz") {
                "tar.xz"
            } else if output.ends_with(".tar.bz2") || output.ends_with(".tbz2") {
                "tar.bz2"
            } else if output.ends_with(".gz") {
                "gzip"
            } else if output.ends_with(".zst") {
                "zstd"
            } else if output.ends_with(".7z") {
                "7z"
            } else {
                "tar"
            };

            println!("Compressing {} file(s) into '{}' [{}]...", inputs.len(), output, format);
            for input in &inputs {
                println!("  + {}", input);
            }
            println!("Done! {} created (1.2 MiB)", output);
            0
        }
        "decompress" | "d" => {
            let rest: Vec<&str> = args[1..].iter()
                .filter(|a| !a.starts_with('-'))
                .map(|s| s.as_str())
                .collect();

            if rest.is_empty() {
                eprintln!("Error: decompress requires at least one archive file.");
                return 1;
            }

            for archive in &rest {
                println!("Decompressing '{}'...", archive);
                println!("  → Cargo.toml");
                println!("  → src/");
                println!("  → src/main.rs");
                println!("  → src/lib.rs");
                println!("  → tests/");
                println!("  → tests/integration.rs");
                println!("  → README.md");
                println!("Done! 7 files extracted");
            }
            0
        }
        "list" | "l" | "ls" => {
            let archive = args.get(1)
                .filter(|a| !a.starts_with('-'))
                .map(|s| s.as_str())
                .unwrap_or("archive.tar.gz");

            println!("Contents of '{}':", archive);
            println!("  Cargo.toml           456 B");
            println!("  src/                     -");
            println!("  src/main.rs          1.2 KiB");
            println!("  src/lib.rs           2.4 KiB");
            println!("  tests/                   -");
            println!("  tests/integration.rs   890 B");
            println!("  README.md            3.1 KiB");
            println!();
            println!("7 entries, 8.0 KiB total (compressed: 1.2 MiB)");
            0
        }
        _ => {
            eprintln!("Error: unknown command '{}'. See --help.", cmd);
            1
        }
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_ouch(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_ouch};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_ouch(vec!["--help".to_string()]), 0);
        assert_eq!(run_ouch(vec!["-h".to_string()]), 0);
        let _ = run_ouch(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_ouch(vec![]);
    }
}
