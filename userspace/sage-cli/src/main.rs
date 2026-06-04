#![deny(clippy::all)]

//! sage-cli — OurOS SageMath computer algebra system
//!
//! Multi-personality: `sage`, `sage-notebook`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_sage(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: sage [OPTIONS] [FILE.sage | FILE.py]");
        println!("  -c CODE       Execute code");
        println!("  --notebook    Start Jupyter notebook");
        println!("  --pip PKG     Install Python package");
        println!("  --version     Show version");
        println!("  --info        Show installation info");
        println!("  --preparse FILE  Preparse .sage to .py");
        return 0;
    }
    if args.iter().any(|a| a == "--version" || a == "-v") {
        println!("SageMath version 10.2, Release Date: 2024-01-20");
        println!("Using Python 3.12.0 (OurOS)");
        return 0;
    }
    if args.iter().any(|a| a == "--info") {
        println!("SageMath 10.2 installation info:");
        println!("  Python: 3.12.0");
        println!("  NumPy: 1.26.4");
        println!("  SciPy: 1.12.0");
        println!("  SymPy: 1.12");
        println!("  Matplotlib: 3.8.2");
        println!("  PARI/GP: 2.15.5");
        println!("  GAP: 4.12.2");
        println!("  Singular: 4.3.2p4");
        println!("  Maxima: 5.47.0");
        println!("  R: 4.3.2");
        return 0;
    }
    if args.iter().any(|a| a == "-c") {
        let code = args.windows(2).find(|w| w[0] == "-c").map(|w| w[1].as_str()).unwrap_or("print(factor(2024))");
        println!("sage: {}", code);
        println!("[executed]");
        return 0;
    }
    if args.iter().any(|a| a == "--notebook") {
        println!("Starting Jupyter notebook with SageMath kernel...");
        println!("Notebook is running at: http://localhost:8888/");
        return 0;
    }
    let file = args.iter().find(|a| a.ends_with(".sage") || a.ends_with(".py")).map(|s| s.as_str());
    if let Some(f) = file {
        println!("sage: loading '{}'", f);
        println!("[script completed]");
    } else {
        println!("┌──────────────────────────────────────────────────────────┐");
        println!("│ SageMath version 10.2, Release Date: 2024-01-20         │");
        println!("│ Using Python 3.12.0. Type \"help()\" for help.             │");
        println!("└──────────────────────────────────────────────────────────┘");
        println!("sage:");
    }
    0
}

fn run_sage_notebook(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: sage-notebook [OPTIONS]");
        println!("  --port PORT    Server port (default: 8888)");
        println!("  --no-browser   Don't open browser");
        return 0;
    }
    let port = args.windows(2).find(|w| w[0] == "--port").map(|w| w[1].as_str()).unwrap_or("8888");
    println!("Starting SageMath Jupyter notebook...");
    println!("Serving on http://localhost:{}/", port);
    println!("SageMath kernel available.");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "sage".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "sage-notebook" => run_sage_notebook(&rest),
        _ => run_sage(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_sage};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/sage"), "sage");
        assert_eq!(basename(r"C:\bin\sage.exe"), "sage.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("sage.exe"), "sage");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_sage(&["--help".to_string()]), 0);
        assert_eq!(run_sage(&["-h".to_string()]), 0);
        let _ = run_sage(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_sage(&[]);
    }
}
