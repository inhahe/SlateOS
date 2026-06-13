#![deny(clippy::all)]

//! obsidian-cli — SlateOS Obsidian knowledge base
//!
//! Single personality: `obsidian`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_obsidian(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: obsidian [OPTIONS] [VAULT_PATH]");
        println!("obsidian v1.5 (SlateOS) — Knowledge base & note editor");
        println!();
        println!("Options:");
        println!("  --vault PATH      Open specific vault");
        println!("  --new             Create new vault");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("obsidian v1.5 (SlateOS)"); return 0; }
    println!("obsidian: knowledge base started");
    println!("  Vault: ~/Documents/Notes");
    println!("  Notes: 342");
    println!("  Graph: 1,250 links");
    println!("  Plugins: 8 community, 3 core");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "obsidian".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_obsidian(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_obsidian};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/obsidian"), "obsidian");
        assert_eq!(basename(r"C:\bin\obsidian.exe"), "obsidian.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("obsidian.exe"), "obsidian");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_obsidian(&["--help".to_string()], "obsidian"), 0);
        assert_eq!(run_obsidian(&["-h".to_string()], "obsidian"), 0);
        let _ = run_obsidian(&["--version".to_string()], "obsidian");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_obsidian(&[], "obsidian");
    }
}
