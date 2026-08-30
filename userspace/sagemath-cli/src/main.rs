#![deny(clippy::all)]

//! sagemath-cli — Slate OS SageMath mathematics system
//!
//! Multi-personality: `sage`, `sage-notebook`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_sage(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: sage [OPTIONS] [FILE.sage]");
        println!("sage v10.3 (Slate OS) — Open-source mathematics system");
        println!();
        println!("Options:");
        println!("  -n, --notebook    Launch Jupyter notebook");
        println!("  -c CMD            Execute command and exit");
        println!("  -t FILE           Run tests");
        println!("  --pip PKG         Install Python package");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("sage v10.3 (Slate OS)"); return 0; }
    if args.iter().any(|a| a == "-n" || a == "--notebook") {
        println!("sage: launching Jupyter notebook interface...");
        return 0;
    }
    println!("sage: interactive mathematics shell");
    println!("  Algebra, calculus, number theory, combinatorics,");
    println!("  graph theory, numerical computation, statistics");
    println!("  Type 'exit' to quit");
    0
}

fn run_notebook(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: sage-notebook [OPTIONS]");
        println!("sage-notebook v10.3 (Slate OS) — SageMath notebook server");
        println!();
        println!("Options:");
        println!("  --port PORT       Server port (default: 8888)");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("sage-notebook v10.3 (Slate OS)"); return 0; }
    println!("sage-notebook: server started at http://localhost:8888");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "sage".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "sage-notebook" => run_notebook(&rest, &prog),
        _ => run_sage(&rest, &prog),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_sage};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/sagemath"), "sagemath");
        assert_eq!(basename(r"C:\bin\sagemath.exe"), "sagemath.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("sagemath.exe"), "sagemath");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_sage(&["--help".to_string()], "sagemath"), 0);
        assert_eq!(run_sage(&["-h".to_string()], "sagemath"), 0);
        let _ = run_sage(&["--version".to_string()], "sagemath");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_sage(&[], "sagemath");
    }
}
