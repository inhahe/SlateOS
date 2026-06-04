#![deny(clippy::all)]

//! figma-cli — OurOS Figma collaborative design tool
//!
//! Single personality: `figma`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_fig(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: figma [OPTIONS]");
        println!("Figma (OurOS) — Browser-first collaborative interface design");
        println!();
        println!("Options:");
        println!("  --design               Figma Design (vector UI design)");
        println!("  --figjam               FigJam (whiteboard for ideation/sticky notes)");
        println!("  --slides               Figma Slides (presentations, 2024)");
        println!("  --dev-mode             Dev Mode (handoff specs, code suggestions)");
        println!("  --figma-make           Figma Make (AI-generate UI from prompt, 2024)");
        println!("  --plan PLAN            free/professional/organization/enterprise");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Figma 124.13.0 Desktop (OurOS)"); return 0; }
    println!("Figma 124.13.0 (OurOS)");
    println!("  Vendor: Figma, Inc. (San Francisco, founded 2012)");
    println!("  Founders: Dylan Field (CEO), Evan Wallace (CTO)");
    println!("  Adobe acquisition: $20B agreed Sep 2022 → BLOCKED Dec 2023 by EU/UK antitrust");
    println!("                     Figma received $1B breakup fee; remained independent");
    println!("  IPO: filed S-1 confidentially Apr 2025");
    println!("  Engine: in-browser WebGL/WebAssembly C++ rendering — Figma's tech moat");
    println!("  Multiplayer: CRDT-based real-time collaboration, presence cursors, live comments");
    println!("  Products: Figma (Design), FigJam (whiteboard), Slides, Dev Mode, Figma Make (AI)");
    println!("  Plans: Free (3 files, 3 pages), Professional $15/editor/mo, Organization $45/editor,");
    println!("        Enterprise $75/editor — SSO, audit, advanced design system tooling");
    println!("  Design Systems: Variables (tokens), Components, Auto Layout, Variants, Branching");
    println!("  Dev handoff: Inspect → Dev Mode — CSS/iOS/Android/Compose code, design tokens export");
    println!("  Plugins: 1500+ in community; First-Party plugins use C++/WASM sandbox");
    println!("  Market: industry default for digital product design, especially mobile/web UX");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "figma".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_fig(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_fig};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/figma"), "figma");
        assert_eq!(basename(r"C:\bin\figma.exe"), "figma.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("figma.exe"), "figma");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_fig(&["--help".to_string()], "figma"), 0);
        assert_eq!(run_fig(&["-h".to_string()], "figma"), 0);
        let _ = run_fig(&["--version".to_string()], "figma");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_fig(&[], "figma");
    }
}
