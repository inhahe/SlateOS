#![deny(clippy::all)]

//! mplab-cli — SlateOS Microchip MPLAB X IDE for PIC/AVR/SAM/dsPIC
//!
//! Single personality: `mplab`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_mplab(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: mplab [OPTIONS] [PROJECT]");
        println!("Microchip MPLAB X IDE 6.20 (SlateOS) — PIC/AVR/SAM/dsPIC IDE");
        println!();
        println!("Options:");
        println!("  --open FILE            Open project (.X)");
        println!("  --build                Build project headless");
        println!("  --program              Program target via PICkit/ICD/Snap");
        println!("  --xc8 / --xc16 / --xc32  Use XC compiler family");
        println!("  --mcc                  Launch MPLAB Code Configurator");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Microchip MPLAB X IDE v6.20 (SlateOS)"); return 0; }
    println!("Microchip MPLAB X IDE v6.20 (SlateOS)");
    println!("  Targets: PIC10/12/16/18/24, dsPIC30/33, PIC32 MIPS/RISC-V, SAM Cortex-M, AVR");
    println!("  Compilers: XC8 (PIC10-18/AVR), XC16 (PIC24/dsPIC), XC32 (PIC32/SAM)");
    println!("  Debuggers: ICD 5, Snap, PICkit 5, Real ICE, J-32");
    println!("  MCC: MPLAB Code Configurator (graphical peripheral setup)");
    println!("  Harmony: TCP/IP, USB, Bluetooth, graphics stacks");
    println!("  Based on NetBeans IDE platform");
    println!("  License: Free IDE; compilers Free (limited opt) or Pro (subscription)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "mplab".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_mplab(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_mplab};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/mplab"), "mplab");
        assert_eq!(basename(r"C:\bin\mplab.exe"), "mplab.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("mplab.exe"), "mplab");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_mplab(&["--help".to_string()], "mplab"), 0);
        assert_eq!(run_mplab(&["-h".to_string()], "mplab"), 0);
        let _ = run_mplab(&["--version".to_string()], "mplab");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_mplab(&[], "mplab");
    }
}
