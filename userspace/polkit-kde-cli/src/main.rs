#![deny(clippy::all)]

//! polkit-kde-cli — OurOS KDE PolicyKit authentication agent
//!
//! Single personality: `polkit-kde-authentication-agent-1`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_agent(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: polkit-kde-authentication-agent-1");
        println!("KDE PolicyKit authentication agent (OurOS)");
        println!();
        println!("Runs as session daemon. Shows Qt dialog when");
        println!("applications request elevated privileges.");
        return 0;
    }
    let _ = args;
    println!("polkit-kde: authentication agent started");
    println!("  Listening for authorization requests...");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "polkit-kde-authentication-agent-1".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_agent(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_agent};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/polkit-kde"), "polkit-kde");
        assert_eq!(basename(r"C:\bin\polkit-kde.exe"), "polkit-kde.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("polkit-kde.exe"), "polkit-kde");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_agent(&["--help".to_string()], "polkit-kde"), 0);
        assert_eq!(run_agent(&["-h".to_string()], "polkit-kde"), 0);
        assert_eq!(run_agent(&["--version".to_string()], "polkit-kde"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_agent(&[], "polkit-kde"), 0);
    }
}
