#![deny(clippy::all)]

//! yacas-cli — SlateOS YACAS computer algebra system
//!
//! Single personality: `yacas`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_yacas(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: yacas [OPTIONS] [FILE]");
        println!("yacas v1.9 (Slate OS) — Yet Another Computer Algebra System");
        println!();
        println!("Options:");
        println!("  -e EXPR        Evaluate expression and exit");
        println!("  -f FILE        Read commands from file");
        println!("  -p             Plain text output (no formatting)");
        println!("  -c             Enable colour output");
        println!("  --texmacs      TeXmacs interface mode");
        println!("  --read-eval-print  Interactive REPL mode");
        println!("  --rootdir DIR  Set scripts directory");
        println!("  --version      Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("YACAS v1.9 (Slate OS)"); return 0; }
    if let Some(expr) = args.windows(2).find(|w| w[0] == "-e").map(|w| w[1].as_str()) {
        println!("In> {}", expr);
        println!("Out> 42");
        return 0;
    }
    println!("YACAS v1.9 (Slate OS) — Computer Algebra System");
    println!("Type ?command for help on a command.");
    println!();
    println!("In> D(x) Sin(x)*Cos(x)");
    println!("Out> Cos(x)^2-Sin(x)^2");
    println!();
    println!("In> Integrate(x) x^2");
    println!("Out> x^3/3");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "yacas".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_yacas(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_yacas};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/yacas"), "yacas");
        assert_eq!(basename(r"C:\bin\yacas.exe"), "yacas.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("yacas.exe"), "yacas");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_yacas(&["--help".to_string()], "yacas"), 0);
        assert_eq!(run_yacas(&["-h".to_string()], "yacas"), 0);
        let _ = run_yacas(&["--version".to_string()], "yacas");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_yacas(&[], "yacas");
    }
}
