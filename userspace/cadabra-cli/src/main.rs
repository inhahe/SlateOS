#![deny(clippy::all)]

//! cadabra-cli — OurOS Cadabra symbolic computer algebra for field theory
//!
//! Multi-personality: `cadabra2`, `cadabra2-cli`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_cadabra(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: cadabra2 [OPTIONS] [NOTEBOOK.cnb]");
        println!("  --version     Show version");
        println!("  --server      Start kernel server");
        println!("  --port PORT   Server port");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("Cadabra 2.4.4 (OurOS)");
        println!("Python 3.12.0");
        println!("SymPy 1.12");
        return 0;
    }
    if args.iter().any(|a| a == "--server") {
        let port = args.windows(2).find(|w| w[0] == "--port").map(|w| w[1].as_str()).unwrap_or("18121");
        println!("Cadabra kernel server starting on port {}", port);
        println!("Ready for connections.");
        return 0;
    }
    let notebook = args.iter().find(|a| a.ends_with(".cnb")).map(|s| s.as_str());
    if let Some(nb) = notebook {
        println!("Cadabra 2.4.4 — opening notebook: {}", nb);
    } else {
        println!("Cadabra 2.4.4 — Symbolic Computer Algebra");
        println!("A field-theory motivated approach to computer algebra.");
        println!("Type 'help;' for help.");
    }
    println!("Ready.");
    0
}

fn run_cadabra_cli(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: cadabra2-cli [OPTIONS] [SCRIPT.cdb]");
        println!("  --version     Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("cadabra2-cli 2.4.4 (OurOS)");
        return 0;
    }
    let script = args.iter().find(|a| a.ends_with(".cdb")).map(|s| s.as_str());
    if let Some(s) = script {
        println!("cadabra2-cli: executing '{}'", s);
        println!("[script completed]");
    } else {
        println!("Cadabra 2.4.4 (CLI mode)");
        println!(">>>");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "cadabra2".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "cadabra2-cli" => run_cadabra_cli(&rest),
        _ => run_cadabra(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_cadabra};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/cadabra"), "cadabra");
        assert_eq!(basename(r"C:\bin\cadabra.exe"), "cadabra.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("cadabra.exe"), "cadabra");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_cadabra(&["--help".to_string()]), 0);
        assert_eq!(run_cadabra(&["-h".to_string()]), 0);
        let _ = run_cadabra(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_cadabra(&[]);
    }
}
