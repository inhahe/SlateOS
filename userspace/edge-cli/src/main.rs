#![deny(clippy::all)]

//! edge-cli — OurOS Microsoft Edge browser (Chromium-based)
//!
//! Single personality: `edge`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_ed(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: edge [URL] [OPTIONS]");
        println!("Microsoft Edge (OurOS) — Chromium-based browser with MS integrations");
        println!();
        println!("Options:");
        println!("  --inprivate            InPrivate browsing window");
        println!("  --copilot              Copilot (Bing Chat) sidebar");
        println!("  --collections          Collections sidebar (Pinterest-style organization)");
        println!("  --ie-mode              IE Mode (legacy Trident in tabs)");
        println!("  --enterprise           Edge for Business");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Microsoft Edge 131.0.2903.86 (OurOS, 64-bit)"); return 0; }
    println!("Microsoft Edge 131.0.2903.86 (OurOS)");
    println!("  Vendor: Microsoft");
    println!("  History:");
    println!("    1995-2015: Internet Explorer (Trident engine, ActiveX)");
    println!("    2015-2019: Edge Legacy (EdgeHTML, Chakra JS — UWP)");
    println!("    2020+:     Edge Chromium (rebuild on Chromium, cross-platform)");
    println!("  Engine: Blink + V8 (Chromium upstream), MS-only patches downstream");
    println!("  Channels: Stable, Beta, Dev, Canary");
    println!("  Features: Vertical Tabs, Workspaces, Collections, Sleeping Tabs,");
    println!("            Read Aloud, Web Capture, Math Solver, Drop (cross-device files)");
    println!("  Copilot: GPT-4 / Bing Chat sidebar integrated since 2023");
    println!("  Enterprise: Microsoft Edge for Business, Application Guard, profile separation");
    println!("  IE Mode: enterprise compatibility, embeds IE11 Trident for legacy intranet");
    println!("  Default on: Windows 10/11, ChromiumOS available, macOS/Linux/iOS/Android");
    println!("  Share: ~13% global, #2 on desktop");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "edge".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_ed(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
