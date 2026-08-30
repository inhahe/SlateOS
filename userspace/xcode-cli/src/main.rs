#![deny(clippy::all)]

//! xcode-cli — Slate OS Xcode (Apple's IDE for macOS/iOS/iPadOS/watchOS/tvOS/visionOS)
//!
//! Single personality: `xcode`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_xc(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: xcode [OPTIONS]");
        println!("Xcode 16.1 (Slate OS) — Apple IDE for all Apple platforms");
        println!();
        println!("Options:");
        println!("  --new                  New project (App / Framework / Package / Swift Playground)");
        println!("  --swift-playgrounds    Swift Playgrounds (iPad-native learning app)");
        println!("  --simulator            iOS / watchOS / tvOS / visionOS Simulator");
        println!("  --previews             SwiftUI Previews (live canvas)");
        println!("  --cli                  Xcode Command-Line Tools (xcodebuild, xcrun, etc.)");
        println!("  --predictive-code      Predictive Code Completion (on-device ML, Apple Silicon)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Xcode 16.1 (build 16B40) (Slate OS)"); return 0; }
    println!("Xcode 16.1 (build 16B40) (Slate OS)");
    println!("  Vendor: Apple Inc. (free download from Mac App Store)");
    println!("  Origin: Project Builder (NeXTSTEP, 1990) → Xcode 1.0 (2003)");
    println!("         renamed during Mac OS X Panther era, became default Mac dev IDE");
    println!("  Platform: macOS only (Apple Silicon optimized, Intel deprecated 2024+)");
    println!("  Pricing: FREE — but Apple Developer Program required for App Store submission ($99/yr)");
    println!("  Languages supported: Swift (Apple's modern language), Objective-C, C, C++, Metal Shading Language");
    println!("  Targets: iOS 12+, iPadOS 13+, macOS 11+, watchOS 6+, tvOS 14+, visionOS (Apple Vision Pro)");
    println!("  Components:");
    println!("    - Swift compiler (toolchain), Clang for C/C++/ObjC, lld linker");
    println!("    - Interface Builder (xib/storyboard editor)");
    println!("    - SwiftUI Previews (live UI preview in canvas as you type)");
    println!("    - Instruments (profiler: Time Profiler, Allocations, Leaks, Network, Metal HUD)");
    println!("    - Asset Catalogs, .xcassets for images / colors / data");
    println!("    - LLDB debugger, view hierarchy debugger, memory graph debugger");
    println!("    - StoreKit, TestFlight integration, App Store Connect upload");
    println!("    - XCTest + XCUITest unit/UI testing");
    println!("  Xcode Cloud: $14.99/mo+ for cloud CI builds + test on Apple devices");
    println!("  Predictive Code Completion (Xcode 16): on-device LLM, Apple Silicon only");
    println!("  TestFlight: beta distribution to 10K external testers");
    println!("  Differentiator: only IDE that ships Apple frameworks officially (SDKs only via Xcode)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "xcode".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_xc(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_xc};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/xcode"), "xcode");
        assert_eq!(basename(r"C:\bin\xcode.exe"), "xcode.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("xcode.exe"), "xcode");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_xc(&["--help".to_string()], "xcode"), 0);
        assert_eq!(run_xc(&["-h".to_string()], "xcode"), 0);
        let _ = run_xc(&["--version".to_string()], "xcode");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_xc(&[], "xcode");
    }
}
