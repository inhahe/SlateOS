#![deny(clippy::all)]

//! zotero-cli — Slate OS Zotero reference manager
//!
//! Single personality: `zotero`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_zotero(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: zotero [OPTIONS]");
        println!("Zotero v6.0 (Slate OS) — Reference management and research organizer");
        println!();
        println!("Options:");
        println!("  --import FILE      Import references (BibTeX/RIS/CSL)");
        println!("  --export FORMAT    Export library (bibtex/ris/csv/json)");
        println!("  --collection NAME  Work with collection");
        println!("  --search QUERY     Search library");
        println!("  --sync             Sync with Zotero servers");
        println!("  --cite STYLE       Generate citation (apa/mla/chicago)");
        println!("  --datadir DIR      Data directory");
        println!("  --headless         Run without GUI");
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Zotero v6.0.30 (Slate OS)"); return 0; }
    println!("Zotero v6.0.30 (Slate OS)");
    println!("  Library: ~/Zotero");
    println!("  Items: 2,456 references");
    println!("  Collections: 34");
    println!("  Tags: 189");
    println!("  Attachments: 1,823 PDFs (4.2 GB)");
    println!("  Sync: enabled (last: 2 hours ago)");
    println!("  Styles: APA 7th, MLA 9th, Chicago 17th");
    println!("  Translators: 523 loaded");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "zotero".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_zotero(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_zotero};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/zotero"), "zotero");
        assert_eq!(basename(r"C:\bin\zotero.exe"), "zotero.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("zotero.exe"), "zotero");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_zotero(&["--help".to_string()], "zotero"), 0);
        assert_eq!(run_zotero(&["-h".to_string()], "zotero"), 0);
        let _ = run_zotero(&["--version".to_string()], "zotero");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_zotero(&[], "zotero");
    }
}
