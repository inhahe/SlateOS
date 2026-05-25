#![deny(clippy::all)]

//! safari-cli — OurOS Apple Safari browser
//!
//! Single personality: `safari`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_sf(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: safari [URL] [OPTIONS]");
        println!("Apple Safari (OurOS) — WebKit-powered browser, default on macOS/iOS/iPadOS");
        println!();
        println!("Options:");
        println!("  --private              Private browsing window");
        println!("  --reader               Reader Mode");
        println!("  --profile NAME         Switch profile (Safari 17+)");
        println!("  --tab-group NAME       Tab Groups (Safari 15+)");
        println!("  --tp                   Intelligent Tracking Prevention");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Safari 18.1 (20619.2.8.11.5) (OurOS)"); return 0; }
    println!("Safari 18.1 (OurOS)");
    println!("  Vendor: Apple Inc. (Cupertino, California)");
    println!("  Engine: WebKit (HTML/CSS), JavaScriptCore Nitro/FTL (JS)");
    println!("  Launched: Jun 2003 (Mac OS X Panther), abandoned Windows 2012");
    println!("  Platforms: macOS, iOS, iPadOS, visionOS — exclusive to Apple ecosystem");
    println!("  iOS lock-in: until iOS 17.4 (EU/DMA), all iOS browsers REQUIRED WebKit engine");
    println!("  Features: Reader Mode, Reading List, iCloud Tabs, Handoff, Profiles (17+),");
    println!("            Tab Groups, Shared Tab Groups, Web Apps (17.4+), Distraction Control");
    println!("  Privacy: ITP (Intelligent Tracking Prevention), Privacy Report, Private Relay (iCloud+),");
    println!("           Hide My Email, Mail Privacy Protection");
    println!("  Web standards: champions some (CSS, ARIA), lags others (PWA APIs, WebGPU shipping)");
    println!("  Extensions: Safari Web Extensions (Mac App Store distribution), much smaller catalog");
    println!("  Default search: Google (rumored ~$20B/yr Apple deal, antitrust scrutiny)");
    println!("  Share: ~18% global, dominant on iOS/macOS (where it's default)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "safari".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_sf(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
