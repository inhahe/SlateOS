#![deny(clippy::all)]

//! xd-cli — OurOS Adobe XD (sunset UX/UI design tool)
//!
//! Single personality: `xd`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_xd(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: xd [OPTIONS]");
        println!("Adobe XD CC 57.1 (OurOS) — UX/UI design + prototype (maintenance mode)");
        println!();
        println!("Options:");
        println!("  --new                  New artboard set");
        println!("  --prototype            Prototype interactions/transitions");
        println!("  --component            Components (master + instances)");
        println!("  --voice                Voice prototyping (Amazon Alexa, Google Assistant)");
        println!("  --share                Share link (review/dev specs/embeds)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Adobe XD 57.1.12.2 (OurOS)"); return 0; }
    println!("Adobe XD 57.1.12.2 (OurOS)");
    println!("  Vendor: Adobe Inc. (San Jose, CA)");
    println!("  Status: MAINTENANCE MODE since 2023 — no new features, security/critical fixes only");
    println!("  Reason: Adobe announced $20B Figma acquisition Sep 2022 → blocked Dec 2023 EU/UK antitrust");
    println!("          XD strategy 'reset' since failed Figma deal; team mostly reassigned");
    println!("  Launched: 2016 as 'Adobe Experience Design CC' (then Adobe XD)");
    println!("  Engine: C++ + Coherent UI (Chromium-based) + custom render");
    println!("  Features: artboards, vector design, components, repeat grid, auto-animate,");
    println!("           voice prototyping (Alexa/Google), share-for-review URLs, dev specs export");
    println!("  Pricing: was included in Creative Cloud All Apps; standalone removed 2023");
    println!("  Replacement: Adobe now positions Photoshop + Illustrator + Lightroom for design,");
    println!("              acquired 'Express' for templating, Substance for 3D");
    println!("  Migration: most users moved to Figma, Sketch, or Penpot");
    println!("  Format: .xd (proprietary), Mercurial-like delta storage for cloud docs");
    println!("  Plugins: official store dormant since 2023");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "xd".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_xd(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_xd};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/xd"), "xd");
        assert_eq!(basename(r"C:\bin\xd.exe"), "xd.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("xd.exe"), "xd");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_xd(&["--help".to_string()], "xd"), 0);
        assert_eq!(run_xd(&["-h".to_string()], "xd"), 0);
        let _ = run_xd(&["--version".to_string()], "xd");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_xd(&[], "xd");
    }
}
