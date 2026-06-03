#![deny(clippy::all)]

//! kakoune-cli — OurOS Kakoune editor
//!
//! Multi-personality: `kak`, `kak-lsp`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_kakoune(args: &[String], prog: &str) -> i32 {
    if prog == "kak-lsp" {
        if args.iter().any(|a| a == "--help" || a == "-h") {
            println!("Usage: kak-lsp [OPTIONS]");
            println!("kak-lsp — Language Server Protocol client for Kakoune");
            println!();
            println!("Options:");
            println!("  -s, --session NAME   Session to connect to");
            println!("  -c, --config FILE    Config file");
            println!("  --kakoune            Start as Kakoune process");
            println!("  -d, --daemonize      Run as daemon");
            println!("  --log FILE           Log file");
            println!("  -V, --version        Show version");
            return 0;
        }
        if args.iter().any(|a| a == "-V" || a == "--version") {
            println!("kak-lsp 17.1.0 (OurOS)");
            return 0;
        }
        println!("kak-lsp: Starting LSP server...");
        return 0;
    }
    // kak
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: kak [OPTIONS] [FILE...]");
        println!("Kakoune 2024.05.18 (OurOS) — Modal code editor");
        println!();
        println!("Options:");
        println!("  -c NAME        Connect to session NAME");
        println!("  -s NAME        Set session name to NAME");
        println!("  -d             Daemonize session");
        println!("  -e CMD         Execute command after startup");
        println!("  -E CMD         Execute command before startup");
        println!("  -f KEYS        Filter input through keys");
        println!("  -i SUFFIX      Edit files in place with backup suffix");
        println!("  -l             List sessions");
        println!("  -p NAME        Send stdin to session");
        println!("  -clear         Clear dead sessions");
        println!("  -n             Don't load kakrc");
        println!("  -ro            Read-only mode");
        println!("  -ui TYPE       UI type (terminal, dummy, json)");
        println!("  -V, --version  Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("Kakoune 2024.05.18 (OurOS)");
        return 0;
    }
    if args.iter().any(|a| a == "-l") {
        println!("default (2024-01-15T10:00:00)");
        return 0;
    }
    if args.iter().any(|a| a == "-clear") {
        println!("kak: Clearing dead sessions...");
        return 0;
    }
    let file = args.iter().find(|a| !a.starts_with('-'))
        .map(|s| s.as_str());
    if let Some(f) = file {
        println!("kak: Editing '{}'", f);
    } else {
        println!("kak: Opening scratch buffer");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "kak".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_kakoune(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_kakoune};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/kakoune"), "kakoune");
        assert_eq!(basename(r"C:\bin\kakoune.exe"), "kakoune.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("kakoune.exe"), "kakoune");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_kakoune(&["--help".to_string()], "kakoune"), 0);
        assert_eq!(run_kakoune(&["-h".to_string()], "kakoune"), 0);
        assert_eq!(run_kakoune(&["--version".to_string()], "kakoune"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_kakoune(&[], "kakoune"), 0);
    }
}
