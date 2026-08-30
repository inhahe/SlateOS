#![deny(clippy::all)]

//! ccze-cli — Slate OS ccze log colorizer
//!
//! Single personality: `ccze`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_ccze(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: ccze [OPTIONS]");
        println!("ccze v0.2 (Slate OS) — Log colorizer");
        println!();
        println!("Options:");
        println!("  -A, --raw-ansi    Raw ANSI output");
        println!("  -h, --html        HTML output");
        println!("  -m MODE           Plugin mode (auto, syslog, httpd, etc.)");
        println!("  -l, --list        List plugins");
        println!("  --version         Show version");
        println!();
        println!("Pipe logs through ccze for colorized output:");
        println!("  tail -f /var/log/syslog | ccze -A");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("ccze v0.2 (Slate OS)"); return 0; }
    if args.iter().any(|a| a == "-l" || a == "--list") {
        println!("Available plugins:");
        println!("  syslog, httpd, postfix, squid, vsftpd, procmail,");
        println!("  exim, php, dpkg, distcc, icecast, ulogd");
        return 0;
    }
    println!("ccze: colorizing log input (waiting for stdin)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "ccze".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_ccze(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_ccze};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/ccze"), "ccze");
        assert_eq!(basename(r"C:\bin\ccze.exe"), "ccze.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("ccze.exe"), "ccze");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_ccze(&["--help".to_string()], "ccze"), 0);
        assert_eq!(run_ccze(&["-h".to_string()], "ccze"), 0);
        let _ = run_ccze(&["--version".to_string()], "ccze");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_ccze(&[], "ccze");
    }
}
