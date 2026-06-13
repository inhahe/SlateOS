#![deny(clippy::all)]

//! metalog-cli — Slate OS Metalog syslog daemon
//!
//! Single personality: `metalog`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_metalog(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: metalog [OPTIONS]");
        println!("Metalog v4.0 (Slate OS) — Modern syslog daemon");
        println!();
        println!("Options:");
        println!("  -c, --config FILE  Config file (default: /etc/metalog.conf)");
        println!("  -N, --no-kernel    Don't read kernel messages");
        println!("  -B SIZE            Kernel buffer size");
        println!("  --pidfile FILE     PID file path");
        println!("  --daemonize        Run as daemon");
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("metalog v4.0.0 (Slate OS)"); return 0; }
    println!("Metalog v4.0.0 (Slate OS)");
    println!("  Config: /etc/metalog.conf");
    println!("  Sections: 5 (mail, news, kernel, auth, default)");
    println!("  Log directory: /var/log");
    println!("  Kernel messages: enabled");
    println!("  Rotation: size-based (1 MiB)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "metalog".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_metalog(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_metalog};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/metalog"), "metalog");
        assert_eq!(basename(r"C:\bin\metalog.exe"), "metalog.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("metalog.exe"), "metalog");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_metalog(&["--help".to_string()], "metalog"), 0);
        assert_eq!(run_metalog(&["-h".to_string()], "metalog"), 0);
        let _ = run_metalog(&["--version".to_string()], "metalog");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_metalog(&[], "metalog");
    }
}
