#![deny(clippy::all)]

//! evernote-cli — SlateOS Evernote note-taking app
//!
//! Single personality: `evernote`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_en(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: evernote [OPTIONS]");
        println!("Evernote (Slate OS) — Note-taking, web clipping, document scan");
        println!();
        println!("Options:");
        println!("  --new                  Create new note");
        println!("  --notebook NAME        Open notebook");
        println!("  --scan                 Scannable document/business card capture");
        println!("  --web-clipper          Evernote Web Clipper (browser extension)");
        println!("  --tier TIER            free/personal/professional/teams");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Evernote 10.107.4 (Slate OS)"); return 0; }
    println!("Evernote 10.107.4 (Slate OS)");
    println!("  Owner: Bending Spoons (Italy, acquired Evernote from owner Nov 2022)");
    println!("  Founded: 2008 by Stepan Pachikov; Phil Libin CEO 2010-15");
    println!("  Original 'remember everything' note-taking pioneer");
    println!("  Notes: rich text, attachments, audio, images, PDF, sketches, code blocks");
    println!("  Sync: across devices, full-text search incl. handwriting + image OCR");
    println!("  Free: 50 notes, 1 notebook, 25MB max note, 60MB upload/mo (since 2024)");
    println!("  Personal: $14.99/mo — 100K notes, 250 notebooks, 20GB upload, AI-powered");
    println!("  Professional: $17.99/mo — Tasks, Calendar, AI Edit/Search, geographic search");
    println!("  Teams: $24.99/user/mo — admin console, central billing, shared spaces");
    println!("  Web Clipper: save URLs/articles/screenshots from Chrome/Firefox/Edge/Safari");
    println!("  Scannable: companion iOS app, document/business card scanner");
    println!("  Penultimate: iPad handwriting note app");
    println!("  Notable history: layoffs 2018, China spin-off (Yinxiang Biji), Bending Spoons rebuild");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "evernote".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_en(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_en};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/evernote"), "evernote");
        assert_eq!(basename(r"C:\bin\evernote.exe"), "evernote.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("evernote.exe"), "evernote");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_en(&["--help".to_string()], "evernote"), 0);
        assert_eq!(run_en(&["-h".to_string()], "evernote"), 0);
        let _ = run_en(&["--version".to_string()], "evernote");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_en(&[], "evernote");
    }
}
