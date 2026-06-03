#![deny(clippy::all)]

//! segger-cli — OurOS SEGGER Embedded Studio
//!
//! Single personality: `segger`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_segger(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: segger [OPTIONS] [PROJECT]");
        println!("SEGGER Embedded Studio 8.20 (OurOS) — Cross-platform embedded IDE");
        println!();
        println!("Options:");
        println!("  --open FILE            Open .emProject");
        println!("  --build                Headless build via emBuild");
        println!("  --target ARCH          arm/risc-v");
        println!("  --jlink                J-Link debug probe");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("SEGGER Embedded Studio for ARM 8.20a (OurOS)"); return 0; }
    println!("SEGGER Embedded Studio for ARM 8.20a (OurOS)");
    println!("  Targets: Arm Cortex-M/A/R, RISC-V (RV32I/RV32E/RV64)");
    println!("  Platforms: Windows, Linux, macOS (cross-platform IDE)");
    println!("  Compiler: SEGGER Compiler (LLVM-based) + GCC alternative");
    println!("  Debug: J-Link (best-in-class debug probe), RTT, SystemView profiling");
    println!("  Middleware: embOS RTOS, embOS/IP, emFile, emWin, emUSB, emCrypt");
    println!("  Ozone: standalone J-Link debugger (also from SEGGER)");
    println!("  License: Free for non-commercial, education, Nordic SDK use");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "segger".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_segger(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_segger};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/segger"), "segger");
        assert_eq!(basename(r"C:\bin\segger.exe"), "segger.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("segger.exe"), "segger");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_segger(&["--help".to_string()], "segger"), 0);
        assert_eq!(run_segger(&["-h".to_string()], "segger"), 0);
        assert_eq!(run_segger(&["--version".to_string()], "segger"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_segger(&[], "segger"), 0);
    }
}
