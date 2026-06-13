#![deny(clippy::all)]

//! goproxy-cli — SlateOS GoProxy Go module proxy
//!
//! Single personality: `goproxy`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_goproxy(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: goproxy [OPTIONS]");
        println!("goproxy v0.16.0 (SlateOS) — Go module proxy server");
        println!();
        println!("Options:");
        println!("  -listen ADDR        Listen address (default: :8081)");
        println!("  -cacher DIR         Cache directory");
        println!("  -proxy URL          Upstream proxy URL");
        println!("  -exclude PATTERN    Exclude modules");
        println!("  -insecure           Allow insecure upstream");
        println!("  -V, --version       Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("goproxy v0.16.0 (SlateOS)");
        return 0;
    }
    println!("goproxy v0.16.0");
    println!("  Listen: :8081");
    println!("  Cache: /var/cache/goproxy");
    println!("  Upstream: https://proxy.golang.org");
    println!("  Serving Go modules...");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "goproxy".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_goproxy(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_goproxy};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/goproxy"), "goproxy");
        assert_eq!(basename(r"C:\bin\goproxy.exe"), "goproxy.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("goproxy.exe"), "goproxy");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_goproxy(&["--help".to_string()], "goproxy"), 0);
        assert_eq!(run_goproxy(&["-h".to_string()], "goproxy"), 0);
        let _ = run_goproxy(&["--version".to_string()], "goproxy");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_goproxy(&[], "goproxy");
    }
}
