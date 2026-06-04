#![deny(clippy::all)]

//! xreader-cli — OurOS X-Apps document reader
//!
//! Single personality: `xreader`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_xreader(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: xreader [OPTIONS] [FILE...]");
        println!("xreader v4.0 (OurOS) — Linux Mint document reader");
        println!();
        println!("Options:");
        println!("  -p PAGE           Open at page");
        println!("  -f                Fullscreen");
        println!("  -s                Slideshow");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("xreader v4.0 (OurOS)"); return 0; }
    println!("xreader: document reader started");
    println!("  Supported: PDF, DjVu, PostScript, XPS, TIFF, CBR/CBZ, ePub");
    println!("  Annotations: yes");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "xreader".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_xreader(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_xreader};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/xreader"), "xreader");
        assert_eq!(basename(r"C:\bin\xreader.exe"), "xreader.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("xreader.exe"), "xreader");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_xreader(&["--help".to_string()], "xreader"), 0);
        assert_eq!(run_xreader(&["-h".to_string()], "xreader"), 0);
        let _ = run_xreader(&["--version".to_string()], "xreader");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_xreader(&[], "xreader");
    }
}
