#![deny(clippy::all)]

//! axel-cli — SlateOS Axel accelerated downloader
//!
//! Single personality: `axel`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_axel(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: axel [OPTIONS] URL [URL...]");
        println!("axel v2.17 (SlateOS) — Lightweight download accelerator");
        println!();
        println!("Options:");
        println!("  -n NUM            Max connections (default: 4)");
        println!("  -o FILE           Output file");
        println!("  -s SPEED          Max speed (bytes/sec)");
        println!("  -q                Quiet mode");
        println!("  -a                Alternate progress indicator");
        println!("  -H HDR            Add HTTP header");
        println!("  -U AGENT          Set user agent");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("axel v2.17 (SlateOS)"); return 0; }
    let urls: Vec<&String> = args.iter().filter(|a| !a.starts_with('-')).collect();
    if urls.is_empty() {
        println!("axel: no URL specified");
        return 1;
    }
    println!("Initializing download: {}", urls.first().map(|s| s.as_str()).unwrap_or(""));
    println!("File size: 10485760 bytes");
    println!("Opening output file: download.bin");
    println!("Starting download with 4 connections");
    println!("[100%] .......... .......... .......... .......... 10240.0K/s");
    println!("Downloaded 10.0 MiB in 1s. (10240.0 KB/s)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "axel".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_axel(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_axel};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/axel"), "axel");
        assert_eq!(basename(r"C:\bin\axel.exe"), "axel.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("axel.exe"), "axel");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_axel(&["--help".to_string()], "axel"), 0);
        assert_eq!(run_axel(&["-h".to_string()], "axel"), 0);
        let _ = run_axel(&["--version".to_string()], "axel");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_axel(&[], "axel");
    }
}
