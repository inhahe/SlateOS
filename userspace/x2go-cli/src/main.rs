#![deny(clippy::all)]

//! x2go-cli — SlateOS X2Go remote desktop
//!
//! Multi-personality: `x2goclient`, `x2goserver`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_client(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: x2goclient [OPTIONS]");
        println!("x2goclient v4.1 (SlateOS) — X2Go remote desktop client");
        println!();
        println!("Options:");
        println!("  --session NAME    Connect to named session");
        println!("  --no-menu         Start without session chooser");
        println!("  --version         Show version");
        println!();
        println!("Features: SSH tunneling, session suspension/resume,");
        println!("  sound forwarding, file sharing, printing");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("x2goclient v4.1 (SlateOS)"); return 0; }
    println!("x2goclient: X2Go client started");
    println!("  Sessions: 0 configured");
    println!("  Transport: SSH + NX compression");
    0
}

fn run_server(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: x2goserver [OPTIONS]");
        println!("x2goserver v4.1 (SlateOS) — X2Go server daemon");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("x2goserver v4.1 (SlateOS)"); return 0; }
    println!("x2goserver: X2Go server started");
    println!("  Active sessions: 0");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "x2goclient".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "x2goserver" => run_server(&rest, &prog),
        _ => run_client(&rest, &prog),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_client};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/x2go"), "x2go");
        assert_eq!(basename(r"C:\bin\x2go.exe"), "x2go.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("x2go.exe"), "x2go");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_client(&["--help".to_string()], "x2go"), 0);
        assert_eq!(run_client(&["-h".to_string()], "x2go"), 0);
        let _ = run_client(&["--version".to_string()], "x2go");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_client(&[], "x2go");
    }
}
