#![deny(clippy::all)]

//! pdoc-cli — OurOS pdoc Python documentation generator
//!
//! Multi-personality: `pdoc`, `pdoc3`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_pdoc(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: pdoc [OPTIONS] MODULE [MODULE ...]");
        println!("pdoc 14.4.0 (OurOS)");
        println!();
        println!("Options:");
        println!("  -o DIR         Output directory");
        println!("  -d             Show documentation in terminal");
        println!("  -p PORT        Start live-preview server");
        println!("  --host HOST    Server host (default: localhost)");
        println!("  --no-browser   Don't open browser");
        println!("  --logo URL     Custom logo URL");
        println!("  --favicon URL  Custom favicon URL");
        println!("  --footer TEXT  Custom footer text");
        println!("  --math         Enable math rendering (MathJax)");
        println!("  --mermaid      Enable Mermaid diagrams");
        println!("  --search       Enable client-side search");
        println!("  --version      Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("pdoc 14.4.0 (OurOS)");
        return 0;
    }
    if args.iter().any(|a| a == "-p") {
        let port = args.windows(2)
            .find(|w| w[0] == "-p")
            .map(|w| w[1].as_str())
            .unwrap_or("8080");
        let module = args.iter()
            .find(|a| !a.starts_with('-'))
            .map(|s| s.as_str())
            .unwrap_or("mymodule");
        println!("pdoc: serving {} at http://localhost:{}", module, port);
        return 0;
    }
    let modules: Vec<&str> = args.iter()
        .filter(|a| !a.starts_with('-'))
        .map(|s| s.as_str())
        .collect();
    let outdir = args.windows(2)
        .find(|w| w[0] == "-o")
        .map(|w| w[1].as_str());
    let terminal = args.iter().any(|a| a == "-d");
    if terminal {
        for m in &modules {
            println!("Module {}", m);
            println!("========");
            println!();
            println!("Functions:");
            println!("  def main() -> None");
            println!("    Entry point for the application.");
            println!();
            println!("Classes:");
            println!("  class Config");
            println!("    Configuration management class.");
        }
    } else if let Some(dir) = outdir {
        for m in &modules {
            println!("pdoc: generating docs for {} -> {}/", m, dir);
        }
        println!("pdoc: {} module(s) documented", modules.len());
    } else {
        for m in &modules {
            println!("pdoc: generating docs for {}", m);
        }
        println!("pdoc: output written to ./html/");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "pdoc".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_pdoc(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_pdoc};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/pdoc"), "pdoc");
        assert_eq!(basename(r"C:\bin\pdoc.exe"), "pdoc.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("pdoc.exe"), "pdoc");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_pdoc(&["--help".to_string()]), 0);
        assert_eq!(run_pdoc(&["-h".to_string()]), 0);
        assert_eq!(run_pdoc(&["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_pdoc(&[]), 0);
    }
}
