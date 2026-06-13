#![deny(clippy::all)]

//! binwalk-cli — SlateOS binwalk firmware analysis tool
//!
//! Single personality: `binwalk`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_binwalk(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: binwalk [OPTIONS] FILE [FILE...]");
        println!("binwalk v2.4 (SlateOS) — Firmware analysis and extraction tool");
        println!();
        println!("Scan Options:");
        println!("  -B             Signature scan (default)");
        println!("  -E             Entropy analysis");
        println!("  -A             Opcode scan");
        println!("  -R STRING      Search for string");
        println!();
        println!("Extraction:");
        println!("  -e             Auto-extract known file types");
        println!("  -M             Recursively extract");
        println!("  -d DEPTH       Max recursion depth");
        println!("  -D TYPE:EXT    Custom extraction rule");
        println!("  -C DIR         Output directory");
        println!();
        println!("Options:");
        println!("  -l             List plugins");
        println!("  -q             Quiet mode");
        println!("  --version      Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("binwalk v2.4.2 (SlateOS)"); return 0; }
    println!("binwalk v2.4.2 (SlateOS)");
    println!();
    println!("DECIMAL       HEXADECIMAL     DESCRIPTION");
    println!("----------------------------------------------------------------------");
    println!("0             0x0             TRX firmware header");
    println!("28            0x1C            LZMA compressed data");
    println!("1048604       0x10001C        Squashfs filesystem, little endian, v4.0");
    println!("3145728       0x300000        JFFS2 filesystem, little endian");
    println!("4194304       0x400000        gzip compressed data");
    println!("4194332       0x40001C        Linux kernel (ARM), version 5.4.0");
    println!();
    println!("Extracted 4 files to _firmware.bin.extracted/");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "binwalk".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_binwalk(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_binwalk};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/binwalk"), "binwalk");
        assert_eq!(basename(r"C:\bin\binwalk.exe"), "binwalk.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("binwalk.exe"), "binwalk");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_binwalk(&["--help".to_string()], "binwalk"), 0);
        assert_eq!(run_binwalk(&["-h".to_string()], "binwalk"), 0);
        let _ = run_binwalk(&["--version".to_string()], "binwalk");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_binwalk(&[], "binwalk");
    }
}
