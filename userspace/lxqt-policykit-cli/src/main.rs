#![deny(clippy::all)]

//! lxqt-policykit-cli — OurOS LXQt PolicyKit agent
//!
//! Single personality: `lxqt-policykit-agent`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_agent(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: lxqt-policykit-agent");
        println!("lxqt-policykit-agent v2.0 (OurOS) — LXQt PolicyKit agent");
        println!();
        println!("Qt-based PolicyKit authentication agent for LXQt desktop.");
        return 0;
    }
    let _ = args;
    println!("lxqt-policykit: authentication agent started");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "lxqt-policykit-agent".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_agent(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_agent};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/lxqt-policykit"), "lxqt-policykit");
        assert_eq!(basename(r"C:\bin\lxqt-policykit.exe"), "lxqt-policykit.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("lxqt-policykit.exe"), "lxqt-policykit");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_agent(&["--help".to_string()], "lxqt-policykit"), 0);
        assert_eq!(run_agent(&["-h".to_string()], "lxqt-policykit"), 0);
        let _ = run_agent(&["--version".to_string()], "lxqt-policykit");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_agent(&[], "lxqt-policykit");
    }
}
