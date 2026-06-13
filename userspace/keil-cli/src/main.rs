#![deny(clippy::all)]

//! keil-cli — SlateOS Arm Keil MDK (Microcontroller Development Kit)
//!
//! Single personality: `keil`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_keil(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: keil [OPTIONS] [PROJECT]");
        println!("Arm Keil MDK 5.41 (SlateOS) — Cortex-M development kit");
        println!();
        println!("Options:");
        println!("  -b PROJECT             Build project (.uvprojx)");
        println!("  -j0                    Headless silent mode");
        println!("  --compiler armclang    Use Arm Compiler 6 (LLVM-based)");
        println!("  --pack PACK            Install Software Pack (CMSIS, vendor)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Arm Keil MDK-Professional 5.41 (SlateOS)"); return 0; }
    println!("Arm Keil MDK-Professional 5.41 (SlateOS)");
    println!("  Targets: Arm Cortex-M0/M0+/M3/M4/M7/M23/M33/M55/M85");
    println!("  IDE: µVision5 (project mgmt, source, debug, profiler)");
    println!("  Compiler: Arm Compiler 6 (armclang/armlink/armar/fromelf) — LLVM-based");
    println!("  Debug: ULINK family, J-Link, ST-Link via CMSIS-DAP");
    println!("  CMSIS: pack manager, RTOS2, DSP, NN library");
    println!("  Middleware: Keil RTX RTOS, Network, USB, File System, GUI");
    println!("  Editions: Lite (free, 32KB), Essential, Plus, Professional");
    println!("  License: subscription / perpetual");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "keil".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_keil(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_keil};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/keil"), "keil");
        assert_eq!(basename(r"C:\bin\keil.exe"), "keil.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("keil.exe"), "keil");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_keil(&["--help".to_string()], "keil"), 0);
        assert_eq!(run_keil(&["-h".to_string()], "keil"), 0);
        let _ = run_keil(&["--version".to_string()], "keil");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_keil(&[], "keil");
    }
}
