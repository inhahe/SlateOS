#![deny(clippy::all)]

//! roam-cli — SlateOS Roam Research networked-thought tool
//!
//! Single personality: `roam`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_roam(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: roam [OPTIONS]");
        println!("Roam Research (SlateOS) — Bidirectional-linked networked thought");
        println!();
        println!("Options:");
        println!("  --daily                Daily Notes page (date-stamped)");
        println!("  --graph                Knowledge graph view");
        println!("  --search               Find by [[page]] or ((block ref))");
        println!("  --plan PLAN            pro/believer/business");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Roam Research 1.0.42 (SlateOS)"); return 0; }
    println!("Roam Research 1.0.42 (SlateOS)");
    println!("  Vendor: Roam Research (San Francisco, founded 2017)");
    println!("  Founder: Conor White-Sullivan");
    println!("  Concept: outliner + bidirectional links + block references");
    println!("           Inspired by Niklas Luhmann's Zettelkasten + Ted Nelson's Xanadu");
    println!("  Key innovations: [[page links]] auto-create pages, ((block refs)) transclude,");
    println!("                   backlinks panel auto-aggregated, Daily Notes journal");
    println!("  Graph: visualizes pages as nodes, links as edges (Obsidian-style)");
    println!("  Queries: Datalog-like {{[[query]]}} for filtered views");
    println!("  Plans: Pro $15/mo or $165/yr — full features");
    println!("        Believer $500/5yr — lifetime/long-term subscriber, early access");
    println!("        Business — team workspaces, billing centralized");
    println!("  Format: Datomic-style EDN export, Markdown import/export, JSON dump");
    println!("  Plugins: 'Roam/js' scripts, third-party extensions (SmartBlocks, etc.)");
    println!("  Competitors: Obsidian (file-based), Logseq (open source), Notion (DB-style)");
    println!("  Polarizing: birthed 'tools for thought' movement; UI quirks divisive");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "roam".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_roam(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_roam};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/roam"), "roam");
        assert_eq!(basename(r"C:\bin\roam.exe"), "roam.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("roam.exe"), "roam");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_roam(&["--help".to_string()], "roam"), 0);
        assert_eq!(run_roam(&["-h".to_string()], "roam"), 0);
        let _ = run_roam(&["--version".to_string()], "roam");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_roam(&[], "roam");
    }
}
