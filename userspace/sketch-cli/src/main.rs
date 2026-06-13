#![deny(clippy::all)]

//! sketch-cli — SlateOS Sketch macOS design tool
//!
//! Single personality: `sketch`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_sk(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: sketch [OPTIONS]");
        println!("Sketch 100 (SlateOS) — macOS-native vector design tool");
        println!();
        println!("Options:");
        println!("  --new                  New document");
        println!("  --workspace            Sketch Workspaces (cloud-based collaboration)");
        println!("  --web-viewer           Web viewer for share links");
        println!("  --symbols              Symbols (component-equivalent)");
        println!("  --plugin               Plugin Manager");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Sketch 100.3 (SlateOS)"); return 0; }
    println!("Sketch 100.3 (SlateOS)");
    println!("  Vendor: Sketch B.V. (Den Haag, Netherlands; founded 2010)");
    println!("  Founders: Pieter Omvlee, Emanuel Sa");
    println!("  Platform: macOS only (no Windows, no Linux native — Sketch for Teams web)");
    println!("  Engine: Cocoa + Core Graphics + Metal (macOS-native APIs, fast on Apple Silicon)");
    println!("  Heyday: 2014-2019 — replaced Adobe Photoshop/Illustrator as default UI tool");
    println!("  Decline: lost market share to Figma post-2019 (browser-based + multiplayer + free tier)");
    println!("  Comeback features: Sketch for Teams (cloud collab), Web viewer, Sketch Cloud,");
    println!("                    Library Sync, Smart Layout, Components/Variants, Prototyping");
    println!("  Plans: Standard $10/editor/mo, Business $20/editor/mo (yearly), Free Viewer");
    println!("  Plugins: legendary ecosystem — Anima, Stark (a11y), Looper, Sketch2React, Marvel");
    println!("  Format: .sketch (zip-based; SVG + JSON + assets) — open-ish, well-documented");
    println!("  Export: PNG, JPG, SVG, PDF, WebP, and codegen via plugins");
    println!("  Strengths: native performance, mature plugin ecosystem, designer-friendly UX");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "sketch".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_sk(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_sk};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/sketch"), "sketch");
        assert_eq!(basename(r"C:\bin\sketch.exe"), "sketch.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("sketch.exe"), "sketch");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_sk(&["--help".to_string()], "sketch"), 0);
        assert_eq!(run_sk(&["-h".to_string()], "sketch"), 0);
        let _ = run_sk(&["--version".to_string()], "sketch");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_sk(&[], "sketch");
    }
}
