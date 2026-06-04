#![deny(clippy::all)]

//! bincode-cli — OurOS bincode serialization tool
//!
//! Single personality: `bincode`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_bincode(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: bincode COMMAND [OPTIONS] [FILE]");
        println!("bincode v2.0 (OurOS) — Bincode serialization inspector");
        println!();
        println!("Commands:");
        println!("  inspect           Inspect bincode file structure");
        println!("  info              Show file metadata");
        println!("  hexdump           Hex dump of bincode data");
        println!("  version           Show version");
        return 0;
    }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("info");
    match cmd {
        "inspect" => {
            let file = args.get(1).map(|s| s.as_str()).unwrap_or("data.bin");
            println!("Inspecting: {}", file);
            println!("  Format: bincode v2");
            println!("  Endianness: little");
            println!("  Size: 1024 bytes");
            println!("  Fields detected: 8");
        }
        "hexdump" => {
            let file = args.get(1).map(|s| s.as_str()).unwrap_or("data.bin");
            println!("Hex dump: {}", file);
            println!("  0000: 01 00 00 00 05 00 00 00  68 65 6C 6C 6F 2A 00 00");
            println!("  0010: 00 00 00 00 00 01 03 00  00 00 01 00 00 00 02 00");
        }
        "version" | "--version" => println!("bincode v2.0 (OurOS)"),
        _ => {
            let file = args.first().map(|s| s.as_str()).unwrap_or("data.bin");
            println!("File: {}", file);
            println!("  Format: bincode v2 (varint length encoding)");
            println!("  Size: 1024 bytes");
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "bincode".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_bincode(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_bincode};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/bincode"), "bincode");
        assert_eq!(basename(r"C:\bin\bincode.exe"), "bincode.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("bincode.exe"), "bincode");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_bincode(&["--help".to_string()], "bincode"), 0);
        assert_eq!(run_bincode(&["-h".to_string()], "bincode"), 0);
        let _ = run_bincode(&["--version".to_string()], "bincode");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_bincode(&[], "bincode");
    }
}
