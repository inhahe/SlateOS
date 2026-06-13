#![deny(clippy::all)]

//! utm-cli — SlateOS UTM (QEMU GUI for macOS/iOS)
//!
//! Single personality: `utm`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_utm(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: utm [OPTIONS]");
        println!("UTM 4.6 (Slate OS) — QEMU-based virtualization for macOS / iOS / visionOS");
        println!();
        println!("Options:");
        println!("  --new                  New VM (wizard)");
        println!("  --import OVA           Import .utm bundle / qcow2 / OVA");
        println!("  --virtualize           Apple Hypervisor.framework (ARM64 native speed)");
        println!("  --emulate              Full QEMU emulation (any arch, slow)");
        println!("  --gallery              UTM Gallery (community VM images)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("UTM 4.6.4 (Slate OS)"); return 0; }
    println!("UTM 4.6.4 (Slate OS)");
    println!("  Author: osy (Turing Software LLC), open-source maintainer-driven project");
    println!("  License: Apache 2.0 (some files), based on QEMU + libcocoa-helpers");
    println!("  Platforms: macOS 11+ (Big Sur), iOS 14+ (JIT-allowed devices), visionOS");
    println!("  Engine: QEMU (8.x) wrapped in a polished native macOS / SwiftUI interface");
    println!("  Modes: Virtualization (Apple Hypervisor.framework, near-native on ARM64),");
    println!("         Emulation (full QEMU TCG/TCI, any arch — x86, ARM, RISC-V, PPC, SPARC, MIPS)");
    println!("  iOS limit: JIT not available on stock iOS; UTM SE (slow interpreter only) on App Store,");
    println!("             AltStore version uses jitterbug for JIT — sideload required");
    println!("  Use cases on Apple Silicon Mac: ARM Linux (Ubuntu/Debian/Fedora), Windows 11 ARM,");
    println!("              x86 Linux/Windows (slow via emulation), macOS guests, BSDs");
    println!("  Free: distributed free from getutm.app (Mac App Store version paid to fund development)");
    println!("  Differentiator: only well-polished free desktop QEMU front-end on Mac");
    println!("  UTM Gallery: prebuilt VM images for many distros — one-click install");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "utm".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_utm(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_utm};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/utm"), "utm");
        assert_eq!(basename(r"C:\bin\utm.exe"), "utm.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("utm.exe"), "utm");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_utm(&["--help".to_string()], "utm"), 0);
        assert_eq!(run_utm(&["-h".to_string()], "utm"), 0);
        let _ = run_utm(&["--version".to_string()], "utm");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_utm(&[], "utm");
    }
}
