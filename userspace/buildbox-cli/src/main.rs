#![deny(clippy::all)]

//! buildbox-cli — OurOS Buildbox (no-code mobile game maker)
//!
//! Single personality: `buildbox`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_bb(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: buildbox [OPTIONS]");
        println!("Buildbox 4 (OurOS) — No-code 3D/2D mobile game maker");
        println!();
        println!("Options:");
        println!("  --new                  New project");
        println!("  --3d                   3D Game Mode (since Buildbox 3)");
        println!("  --templates            Brainboxes (template games — drop assets + ship)");
        println!("  --smart-assets         Smart Assets (preconfigured logic blocks)");
        println!("  --export TARGET        Export iOS / Android / Steam / HTML5");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Buildbox 4.1 (OurOS)"); return 0; }
    println!("Buildbox 4.1 (OurOS)");
    println!("  Vendor: Buildbox Inc. (Las Vegas, NV — founded 2014)");
    println!("  Founder: Trey Smith");
    println!("  Pricing: Free Plan (limited), Plus $19.99/mo, Pro $49.99/mo, Premier $99.99/mo");
    println!("  Niche: no-code mobile hyper-casual game maker");
    println!("        target audience: marketers / non-programmers who want to publish to App Store");
    println!("  Philosophy: drag-and-drop only — NO scripting at all (vs. GameMaker's GML)");
    println!("             'Smart Assets' bundle logic + animation + collision into one prefab");
    println!("  Engine: proprietary OpenGL/Metal renderer, built on C++ core");
    println!("  2D + 3D: Buildbox 3 added 3D (2018), Buildbox 4 (2023) added improved 3D + better physics");
    println!("  Brainboxes: pre-built template game projects (endless runner, puzzle, action), edit and ship");
    println!("  Multi-platform export: iOS, Android, HTML5, Steam (Windows/Mac), Amazon");
    println!("  Famous Buildbox games:");
    println!("    - The Line Zen (2016 — 5M downloads first month)");
    println!("    - Color Switch (Buildbox graduate — 100M+ downloads, Forbes story)");
    println!("    - Sky (multiple iOS top-10 entries)");
    println!("    - many ad-supported hyper-casual mobile games on the App Store top charts");
    println!("  Hyper-casual genre: Buildbox is the favorite tool of the genre's prolific publishers");
    println!("  Critique: limited for complex games; great for one specific niche (HC mobile)");
    println!("  Differentiator: ZERO programming required — only true no-code game engine that ships polished products");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "buildbox".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_bb(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_bb};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/buildbox"), "buildbox");
        assert_eq!(basename(r"C:\bin\buildbox.exe"), "buildbox.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("buildbox.exe"), "buildbox");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_bb(&["--help".to_string()], "buildbox"), 0);
        assert_eq!(run_bb(&["-h".to_string()], "buildbox"), 0);
        let _ = run_bb(&["--version".to_string()], "buildbox");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_bb(&[], "buildbox");
    }
}
