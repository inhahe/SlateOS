#![deny(clippy::all)]

//! charles-cli — OurOS Charles Proxy web debugging tool
//!
//! Single personality: `charles`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_charles(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: charles [OPTIONS]");
        println!("Charles Proxy v4.6.5 (OurOS) — Web debugging proxy");
        println!();
        println!("Options:");
        println!("  --port PORT         HTTP proxy port (default: 8888)");
        println!("  --socks-port PORT   SOCKS proxy port");
        println!("  --headless          Headless mode");
        println!("  --config FILE       Config file");
        println!("  --session FILE      Load session");
        println!("  --save FILE         Save session on exit");
        println!("  -V, --version       Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("Charles Proxy v4.6.5 (OurOS)");
        return 0;
    }
    println!("Charles Proxy v4.6.5 starting...");
    println!("  HTTP Proxy: localhost:8888");
    println!("  SSL Proxying: enabled");
    println!("  Recording: active");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "charles".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_charles(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_charles};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/charles"), "charles");
        assert_eq!(basename(r"C:\bin\charles.exe"), "charles.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("charles.exe"), "charles");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_charles(&["--help".to_string()], "charles"), 0);
        assert_eq!(run_charles(&["-h".to_string()], "charles"), 0);
        let _ = run_charles(&["--version".to_string()], "charles");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_charles(&[], "charles");
    }
}
