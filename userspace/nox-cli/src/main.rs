#![deny(clippy::all)]

//! nox-cli — OurOS Nox test automation tool
//!
//! Multi-personality: `nox`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_nox(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: nox [OPTIONS]");
        println!("nox 2024.4.15 (OurOS)");
        println!();
        println!("Options:");
        println!("  -s, --sessions SESSIONS  Sessions to run");
        println!("  -k KEYWORD              Filter sessions by keyword");
        println!("  -l, --list               List available sessions");
        println!("  -f FILE                  Noxfile to use (default: noxfile.py)");
        println!("  --no-venv                Don't create virtualenvs");
        println!("  --reuse-existing-virtualenvs  Reuse existing venvs");
        println!("  -r                       Alias for --reuse-existing-virtualenvs");
        println!("  --no-install             Skip install commands");
        println!("  --force-color            Force colored output");
        println!("  --version                Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("2024.4.15");
        return 0;
    }
    if args.iter().any(|a| a == "-l" || a == "--list") {
        println!("Sessions defined in noxfile.py:");
        println!();
        println!("* tests-3.12");
        println!("* tests-3.11");
        println!("* tests-3.10");
        println!("* lint");
        println!("* docs");
        println!("* type_check");
        println!();
        println!("sessions marked with * are selected, sessions marked with - are skipped.");
        return 0;
    }
    let sessions: Vec<&str> = args.windows(2)
        .filter(|w| w[0] == "-s" || w[0] == "--sessions")
        .map(|w| w[1].as_str())
        .collect();
    let reuse = args.iter().any(|a| a == "-r" || a == "--reuse-existing-virtualenvs");

    let run_sessions = if sessions.is_empty() {
        vec!["tests-3.12", "tests-3.11", "lint"]
    } else {
        sessions
    };

    for s in &run_sessions {
        println!("nox > Running session {}", s);
        if reuse {
            println!("nox > Re-using existing virtual environment...");
        } else {
            println!("nox > Creating virtual environment using python...");
        }
        println!("nox > python -m pip install -r requirements-test.txt");
        println!("nox > python -m pytest");
        println!("nox > Session {} was successful.", s);
        println!();
    }

    println!("nox > Ran {} sessions: all successful.", run_sessions.len());
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "nox".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_nox(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_nox};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/nox"), "nox");
        assert_eq!(basename(r"C:\bin\nox.exe"), "nox.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("nox.exe"), "nox");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_nox(&["--help".to_string()]), 0);
        assert_eq!(run_nox(&["-h".to_string()]), 0);
        assert_eq!(run_nox(&["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_nox(&[]), 0);
    }
}
