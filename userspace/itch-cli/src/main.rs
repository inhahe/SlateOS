#![deny(clippy::all)]

//! itch-cli — Slate OS itch.io desktop app (indie game store)
//!
//! Single personality: `itch`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_itch(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: itch [OPTIONS]");
        println!("itch app 25.6 (Slate OS) — itch.io desktop client (indie game store)");
        println!();
        println!("Options:");
        println!("  --library              Owned games library");
        println!("  --bundles              Owned bundles (Bundle for Racial Justice and Equality, etc.)");
        println!("  --browse               Browse itch.io store");
        println!("  --pay-what-you-want    PWYW games (pay $0 or more — creator-friendly)");
        println!("  --jam                  Game jams (Global Game Jam, GMTK, Ludum Dare archived)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("itch 25.6.2 (Slate OS)"); return 0; }
    println!("itch 25.6.2 (Slate OS)");
    println!("  Vendor: itch corp (Toronto, Canada) — wholly owned by founder, no VC");
    println!("  Founder: Leaf Corcoran (solo dev) — itch.io launched March 3 2013");
    println!("  Platform: Windows, macOS, Linux (made with Electron + Node.js + Go)");
    println!("  Free open-source: itch desktop app is MIT-licensed on GitHub (itchio/itch)");
    println!("  Niche: indie games — open submission, no curation gate, anyone can publish");
    println!("  Revenue model: creators choose the cut itch takes (default 10%, range 0-100%)");
    println!("                'Pay what you want' is encouraged ($0 min welcome)");
    println!("  Famous milestones:");
    println!("    - Bundle for Racial Justice and Equality (2020): $8.1M raised, 1741 games included");
    println!("    - Annual GMTK Jam (Mark Brown's), Ludum Dare archive");
    println!("    - Hosts Celeste / Hotline Miami / Hyper Light Drifter / Undertale (initial release)");
    println!("    - Famous-by-itch.io: Cuphead, Undertale, Risk of Rain, Disco Elysium demos");
    println!("  Game jams: integrated jam hosting + voting, the de facto home of indie jams");
    println!("  Asset packs: also sells pixel art / music / sprites / templates for game devs");
    println!("  Other content: comic books, zines, TTRPG PDFs, soundtracks, books");
    println!("  itch desktop app: download manager, sandboxed launching, multi-version updates");
    println!("  Differentiator: most creator-friendly digital store on the internet, no gatekeeping");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "itch".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_itch(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_itch};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/itch"), "itch");
        assert_eq!(basename(r"C:\bin\itch.exe"), "itch.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("itch.exe"), "itch");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_itch(&["--help".to_string()], "itch"), 0);
        assert_eq!(run_itch(&["-h".to_string()], "itch"), 0);
        let _ = run_itch(&["--version".to_string()], "itch");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_itch(&[], "itch");
    }
}
