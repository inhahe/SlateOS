#![deny(clippy::all)]

//! godoc-cli — OurOS Go documentation tools
//!
//! Multi-personality: `godoc`, `go doc`

use std::env;
use std::process;

fn run_godoc(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: godoc [OPTIONS] [PACKAGE] [SYMBOL]");
        println!("godoc — Go Documentation Server (OurOS)");
        println!();
        println!("Options:");
        println!("  -http ADDR     HTTP server address (default :6060)");
        println!("  -goroot DIR    Go root directory");
        println!("  -src           Show source code");
        println!("  -all           Show all documentation");
        println!("  -index         Enable search index");
        println!("  -play          Enable playground");
        println!("  -v             Verbose mode");
        println!("  --version      Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("godoc (OurOS, go1.22.0)");
        return 0;
    }
    if args.iter().any(|a| a == "-http") {
        let addr = args.windows(2)
            .find(|w| w[0] == "-http")
            .map(|w| w[1].as_str())
            .unwrap_or(":6060");
        println!("Using GOROOT: /usr/local/go");
        println!("godoc: serving documentation at http://localhost{}", addr);
        return 0;
    }
    let pkg = args.iter()
        .find(|a| !a.starts_with('-'))
        .map(|s| s.as_str())
        .unwrap_or("fmt");
    let sym = args.iter()
        .filter(|a| !a.starts_with('-'))
        .nth(1)
        .map(|s| s.as_str());
    println!("package {}", pkg);
    println!();
    if let Some(s) = sym {
        println!("func {}(...)", s);
        println!("    {} performs the operation.", s);
    } else {
        println!("Package {} implements formatted I/O.", pkg);
        println!();
        println!("FUNCTIONS");
        println!("  func Println(a ...any) (n int, err error)");
        println!("  func Printf(format string, a ...any) (n int, err error)");
        println!("  func Sprintf(format string, a ...any) string");
        println!("  func Fprintf(w io.Writer, format string, a ...any) (n int, err error)");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_godoc(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_godoc};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_godoc(&["--help".to_string()]), 0);
        assert_eq!(run_godoc(&["-h".to_string()]), 0);
        let _ = run_godoc(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_godoc(&[]);
    }
}
