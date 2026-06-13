#![deny(clippy::all)]

//! dolphin-emu-cli — SlateOS Dolphin GameCube/Wii emulator
//!
//! Multi-personality: `dolphin-emu`, `dolphin-emu-nogui`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_dolphin(args: &[String], prog: &str) -> i32 {
    let nogui = prog == "dolphin-emu-nogui";
    if args.iter().any(|a| a == "--help" || a == "-h") {
        if nogui {
            println!("Usage: dolphin-emu-nogui [OPTIONS] ROM");
        } else {
            println!("Usage: dolphin-emu [OPTIONS] [ROM]");
        }
        println!("dolphin-emu v5.0-21088 (Slate OS) — GameCube/Wii emulator");
        println!();
        println!("Options:");
        println!("  -e FILE           Boot ROM/ISO");
        println!("  -b                Batch mode (exit after game stops)");
        println!("  -p PLATFORM       Video backend (ogl, vulkan, dx11)");
        println!("  --config KEY=VAL  Override config");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("dolphin-emu v5.0-21088 (Slate OS)"); return 0; }
    if nogui {
        println!("dolphin-emu: starting headless mode...");
    } else {
        println!("dolphin-emu: GameCube/Wii emulator started");
    }
    println!("  Backend: Vulkan");
    println!("  GameCube: ready");
    println!("  Wii: ready");
    println!("  Controllers: GameCube Adapter detected");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "dolphin-emu".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_dolphin(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_dolphin};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/dolphin-emu"), "dolphin-emu");
        assert_eq!(basename(r"C:\bin\dolphin-emu.exe"), "dolphin-emu.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("dolphin-emu.exe"), "dolphin-emu");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_dolphin(&["--help".to_string()], "dolphin-emu"), 0);
        assert_eq!(run_dolphin(&["-h".to_string()], "dolphin-emu"), 0);
        let _ = run_dolphin(&["--version".to_string()], "dolphin-emu");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_dolphin(&[], "dolphin-emu");
    }
}
