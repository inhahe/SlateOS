#![deny(clippy::all)]

//! stm32cubeide-cli — Slate OS STMicroelectronics STM32CubeIDE
//!
//! Single personality: `stm32cubeide`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_cubeide(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: stm32cubeide [OPTIONS] [WORKSPACE]");
        println!("STMicroelectronics STM32CubeIDE 1.16 (Slate OS) — STM32 development IDE");
        println!();
        println!("Options:");
        println!("  -data WORKSPACE        Workspace path");
        println!("  -import PROJECT        Import .project");
        println!("  -build CONFIG          Headless build");
        println!("  --cubemx               Launch STM32CubeMX configurator");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("STMicroelectronics STM32CubeIDE 1.16.0 (Slate OS)"); return 0; }
    println!("STMicroelectronics STM32CubeIDE 1.16.0 (Slate OS)");
    println!("  Targets: STM32 Cortex-M0/M3/M4/M7/M33/M55, STM32MP1/MP2 (A7/A35)");
    println!("  Based on: Eclipse CDT + GNU MCU plugins + STM32CubeMX integration");
    println!("  CubeMX: pin/clock/peripheral configuration with code generation");
    println!("  Compiler: GNU Arm Embedded Toolchain (free, bundled)");
    println!("  Debug: ST-LINK V2/V3, GDB server, SWV trace, live variables");
    println!("  Middleware: HAL/LL, FreeRTOS, lwIP, FatFS, USB, TouchGFX");
    println!("  License: Free (Mix Ultimate Liberty + GPL components)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "stm32cubeide".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_cubeide(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_cubeide};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/stm32cubeide"), "stm32cubeide");
        assert_eq!(basename(r"C:\bin\stm32cubeide.exe"), "stm32cubeide.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("stm32cubeide.exe"), "stm32cubeide");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_cubeide(&["--help".to_string()], "stm32cubeide"), 0);
        assert_eq!(run_cubeide(&["-h".to_string()], "stm32cubeide"), 0);
        let _ = run_cubeide(&["--version".to_string()], "stm32cubeide");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_cubeide(&[], "stm32cubeide");
    }
}
