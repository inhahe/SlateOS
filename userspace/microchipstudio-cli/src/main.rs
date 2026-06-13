#![deny(clippy::all)]

//! microchipstudio-cli — SlateOS Microchip Studio (formerly Atmel Studio)
//!
//! Single personality: `microchipstudio`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_ms(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: microchipstudio [OPTIONS] [SOLUTION]");
        println!("Microchip Studio 7.0 (Slate OS) — AVR/SAM development IDE (was Atmel Studio)");
        println!();
        println!("Options:");
        println!("  --open FILE            Open .atsln solution");
        println!("  --build                Headless build via atbackend.exe");
        println!("  --atmel-start          Open Atmel START code configurator (web)");
        println!("  --atprogram           atprogram CLI (program AVR/SAM)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Microchip Studio 7.0.2594 (Slate OS)"); return 0; }
    println!("Microchip Studio 7.0.2594 (Slate OS)");
    println!("  Targets: 8-bit AVR (tiny/mega), 32-bit AVR, SAM (Cortex-M0+/M4/M7)");
    println!("  Based on: Visual Studio Isolated Shell");
    println!("  Compiler: avr-gcc, arm-none-eabi-gcc (free, bundled)");
    println!("  Atmel START: web-based configurator with code generation");
    println!("  Debuggers: Atmel-ICE, Power Debugger, EDBG, SAM-ICE, JTAGICE3");
    println!("  Simulator: cycle-accurate simulator for all AVR cores");
    println!("  License: Free (closed-source IDE, GCC components GPL)");
    println!("  Note: replaced by MPLAB X for new development (Atmel merger)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "microchipstudio".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_ms(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_ms};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/microchipstudio"), "microchipstudio");
        assert_eq!(basename(r"C:\bin\microchipstudio.exe"), "microchipstudio.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("microchipstudio.exe"), "microchipstudio");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_ms(&["--help".to_string()], "microchipstudio"), 0);
        assert_eq!(run_ms(&["-h".to_string()], "microchipstudio"), 0);
        let _ = run_ms(&["--version".to_string()], "microchipstudio");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_ms(&[], "microchipstudio");
    }
}
