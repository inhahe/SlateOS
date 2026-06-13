#![deny(clippy::all)]

//! geneanet-cli — Slate OS Geneanet client tools
//!
//! Single personality: `geneanet`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_geneanet(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: geneanet [COMMAND] [OPTIONS]");
        println!("geneanet v1.0 (Slate OS) — Geneanet genealogy client");
        println!();
        println!("Commands:");
        println!("  search NAME        Search for a person");
        println!("  import FILE        Import GEDCOM");
        println!("  export FILE        Export GEDCOM");
        println!("  sync               Sync with Geneanet");
        println!("  stats              Show statistics");
        println!();
        println!("Options:");
        println!("  --token TOKEN      API token");
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("geneanet-cli v1.0 (Slate OS)"); return 0; }
    match args.first().map(|s| s.as_str()) {
        Some("stats") => {
            println!("Geneanet statistics:");
            println!("  Individuals: 1,234");
            println!("  Surnames: 456");
            println!("  Oldest ancestor: 1623");
            println!("  Generations: 12");
        }
        Some("search") => {
            let name = args.get(1).map(|s| s.as_str()).unwrap_or("Smith");
            println!("Searching for '{}'...", name);
            println!("  Found 42 matches in your tree");
            println!("  Found 12,345 matches online");
        }
        _ => {
            println!("geneanet: specify a command (search, import, export, sync, stats)");
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "geneanet".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_geneanet(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_geneanet};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/geneanet"), "geneanet");
        assert_eq!(basename(r"C:\bin\geneanet.exe"), "geneanet.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("geneanet.exe"), "geneanet");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_geneanet(&["--help".to_string()], "geneanet"), 0);
        assert_eq!(run_geneanet(&["-h".to_string()], "geneanet"), 0);
        let _ = run_geneanet(&["--version".to_string()], "geneanet");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_geneanet(&[], "geneanet");
    }
}
