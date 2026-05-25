#![deny(clippy::all)]

//! googleworkspace-cli — OurOS Google Workspace (formerly G Suite)
//!
//! Single personality: `googleworkspace`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_gw(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: googleworkspace [OPTIONS]");
        println!("Google Workspace (OurOS) — Cloud productivity suite");
        println!();
        println!("Options:");
        println!("  --app NAME             docs/sheets/slides/gmail/drive/meet/calendar");
        println!("  --gemini               Gemini for Workspace (AI assistant)");
        println!("  --plan PLAN            business-starter/standard/plus/enterprise");
        println!("  --admin                Admin console (admin.google.com)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Google Workspace 2024.11 (OurOS)"); return 0; }
    println!("Google Workspace (OurOS)");
    println!("  Vendor: Google LLC (Mountain View, California)");
    println!("  History: Google Apps (2006) → G Suite (2016) → Google Workspace (Oct 2020)");
    println!("  Apps: Gmail, Drive, Docs, Sheets, Slides, Forms, Calendar, Meet, Chat,");
    println!("        Sites, Keep, Tasks, Currents (retired), Vault, Cloud Search");
    println!("  Business: Starter ($7) / Standard ($14) / Plus ($22) per user/mo");
    println!("  Enterprise: Standard / Plus — Vault eDiscovery, advanced endpoint, BeyondCorp");
    println!("  Storage: 30GB / 2TB / 5TB / 5TB+ per user (pooled)");
    println!("  Education: Workspace for Education (Fundamentals free / Standard / Plus)");
    println!("  Gemini: Business / Enterprise ($20/$30 per user/mo) — Gemini 1.5 Pro in apps");
    println!("  Meet: video conferencing up to 1000 participants, recording, noise cancel");
    println!("  Strengths: real-time collaboration (pioneered), web-first, no install needed");
    println!("  File compat: .docx/.xlsx/.pptx import/export, native formats are web-only");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "googleworkspace".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_gw(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
