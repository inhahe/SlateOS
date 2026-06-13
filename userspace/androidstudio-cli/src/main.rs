#![deny(clippy::all)]

//! androidstudio-cli — SlateOS Android Studio (Google's official Android IDE)
//!
//! Single personality: `androidstudio`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_as(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: androidstudio [OPTIONS]");
        println!("Android Studio Ladybug 2024.2.2 (SlateOS) — Google's IDE for Android development");
        println!();
        println!("Options:");
        println!("  --new                  New project (Compose / Views / Wear / Auto / TV)");
        println!("  --emulator             Android Emulator (AVD Manager)");
        println!("  --gemini               Gemini in Android Studio (AI assistant — free + paid tiers)");
        println!("  --profiler             Profiler (CPU/Memory/Network/Energy)");
        println!("  --sdk-manager          Android SDK Manager");
        println!("  --gradle               Gradle build system (or KSP / KAPT)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Android Studio Ladybug | 2024.2.2 Patch 1 (SlateOS)"); return 0; }
    println!("Android Studio Ladybug | 2024.2.2 Patch 1 (SlateOS)");
    println!("  Vendor: Google LLC");
    println!("  Origin: replaced Eclipse-based ADT (Android Developer Tools) plugin — May 2013 announce");
    println!("         GA Dec 2014 (Android Studio 1.0)");
    println!("  Built on: JetBrains IntelliJ Platform (IntelliJ IDEA Community Edition fork)");
    println!("  Pricing: FREE — Google distributes it (Apache 2.0 + bundled IntelliJ Community)");
    println!("  Versioning: insect-themed (Hedgehog → Iguana → Jellyfish → Koala → Ladybug → ...)");
    println!("             Each named release roughly maps to one IntelliJ Platform release");
    println!("  Languages: Kotlin (preferred, Google-blessed 2019), Java (legacy), C++ (NDK), Dart (via plugin → Flutter)");
    println!("  UI frameworks: Jetpack Compose (modern declarative — like SwiftUI), XML Views (legacy)");
    println!("  Compose Preview: live UI preview as you type (inspired by SwiftUI Previews)");
    println!("  Emulator: AVD Manager — boots Android x86/ARM images in QEMU+KVM/HAXM, snapshot resume");
    println!("           Real device debugging via USB or wireless ADB");
    println!("  Gradle: Android Gradle Plugin (AGP) is the build system — slow but extremely flexible");
    println!("          AGP 8.x with build cache + KSP (Kotlin Symbol Processing)");
    println!("  Layout tools: Layout Inspector, Database Inspector, Background Task Inspector,");
    println!("                Network Inspector, Profileable Build, MotionLayout editor");
    println!("  Gemini in Android Studio: AI code suggestions, Studio Bot — free Code Completions, paid for chat");
    println!("  Modules: app, feature modules, dynamic feature delivery, App Bundles (.aab), Play Asset Delivery");
    println!("  Differentiator: only official IDE supported by Google for Android dev (+ first-class Wear/Auto/TV)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "androidstudio".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_as(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_as};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/androidstudio"), "androidstudio");
        assert_eq!(basename(r"C:\bin\androidstudio.exe"), "androidstudio.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("androidstudio.exe"), "androidstudio");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_as(&["--help".to_string()], "androidstudio"), 0);
        assert_eq!(run_as(&["-h".to_string()], "androidstudio"), 0);
        let _ = run_as(&["--version".to_string()], "androidstudio");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_as(&[], "androidstudio");
    }
}
