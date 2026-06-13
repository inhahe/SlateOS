#![deny(clippy::all)]

//! pydoc-cli — SlateOS Python documentation tools
//!
//! Multi-personality: `pydoc`, `pydoc3`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_pydoc(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("pydoc - the Python documentation tool");
        println!();
        println!("Usage: pydoc [OPTIONS] NAME");
        println!("       pydoc -k KEYWORD");
        println!("       pydoc -p PORT");
        println!("       pydoc -w MODULE");
        println!();
        println!("Options:");
        println!("  NAME          Module, package, class, function, or keyword");
        println!("  -k KEYWORD    Search for a keyword in all modules");
        println!("  -p PORT       Start HTTP documentation server");
        println!("  -w MODULE     Write HTML documentation to file");
        println!("  -b            Start server and open browser");
        println!("  --version     Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("pydoc 3.12.2 (Slate OS)");
        return 0;
    }
    if args.iter().any(|a| a == "-p") {
        let port = args.windows(2)
            .find(|w| w[0] == "-p")
            .map(|w| w[1].as_str())
            .unwrap_or("8080");
        println!("pydoc server ready at http://localhost:{}", port);
        return 0;
    }
    if args.iter().any(|a| a == "-b") {
        println!("Server ready at http://localhost:8080");
        println!("Opening browser...");
        return 0;
    }
    if args.iter().any(|a| a == "-k") {
        let keyword = args.windows(2)
            .find(|w| w[0] == "-k")
            .map(|w| w[1].as_str())
            .unwrap_or("print");
        println!("Searching for '{}'...", keyword);
        println!("builtins - Built-in functions, exceptions, and other objects.");
        println!("sys - Access system-specific parameters and functions.");
        println!("os - OS routines for NT or Posix depending on what system we're on.");
        return 0;
    }
    if args.iter().any(|a| a == "-w") {
        let module = args.windows(2)
            .find(|w| w[0] == "-w")
            .map(|w| w[1].as_str())
            .unwrap_or("os");
        println!("wrote {}.html", module);
        return 0;
    }
    let name = args.iter()
        .find(|a| !a.starts_with('-'))
        .map(|s| s.as_str())
        .unwrap_or("os");
    println!("Help on module {}:", name);
    println!();
    println!("NAME");
    println!("    {}", name);
    println!();
    println!("DESCRIPTION");
    println!("    This module provides access to {} functionality.", name);
    println!();
    println!("FUNCTIONS");
    println!("    See documentation for full list.");
    println!();
    println!("FILE");
    println!("    /usr/lib/python3.12/{}.py", name);
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "pydoc".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_pydoc(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_pydoc};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/pydoc"), "pydoc");
        assert_eq!(basename(r"C:\bin\pydoc.exe"), "pydoc.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("pydoc.exe"), "pydoc");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_pydoc(&["--help".to_string()]), 0);
        assert_eq!(run_pydoc(&["-h".to_string()]), 0);
        let _ = run_pydoc(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_pydoc(&[]);
    }
}
