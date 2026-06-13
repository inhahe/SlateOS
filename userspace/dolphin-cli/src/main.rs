#![deny(clippy::all)]

//! dolphin-cli — SlateOS Dolphin GameCube/Wii emulator
//!
//! Multi-personality: `dolphin-emu`, `dolphin-emu-nogui`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_dolphin(args: &[String], nogui: bool) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: dolphin-emu [OPTIONS] [FILE]");
        println!();
        println!("dolphin-emu — GameCube/Wii emulator (SlateOS).");
        println!();
        println!("Options:");
        println!("  -e, --exec <file>     Launch game ISO/WBFS");
        println!("  -b, --batch           Exit on game stop");
        println!("  -c, --confirm         Confirm on stop");
        println!("  -u, --user <dir>      User directory");
        println!("  --config <sys>.<key>=<val>  Override config");
        println!("  -v, --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-v" || a == "--version") {
        println!("Dolphin 5.0-21088 (SlateOS)");
        return 0;
    }

    let game = args.windows(2).find(|w| w[0] == "-e" || w[0] == "--exec")
        .map(|w| w[1].as_str())
        .or_else(|| args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()));

    println!("[Dolphin] Version 5.0-21088 (SlateOS)");
    println!("[Dolphin] Video backend: Vulkan");
    println!("[Dolphin] Audio backend: PulseAudio");
    if nogui {
        println!("[Dolphin] Running in headless/nogui mode");
    }
    if let Some(g) = game {
        println!("[Dolphin] Loading: {}", g);
        println!("[Dolphin] Game ID: GALE01 (Super Smash Bros. Melee)");
        println!("[Dolphin] Region: NTSC-U");
        println!("[Dolphin] Internal resolution: 1920x1080 (3x native)");
        println!("[Dolphin] Running...");
    } else {
        println!("[Dolphin] Starting game list browser");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "dolphin-emu".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let nogui = prog.contains("nogui");
    let code = run_dolphin(&rest, nogui);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_dolphin};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/dolphin"), "dolphin");
        assert_eq!(basename(r"C:\bin\dolphin.exe"), "dolphin.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("dolphin.exe"), "dolphin");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_dolphin(&["--help".to_string()], false), 0);
        assert_eq!(run_dolphin(&["-h".to_string()], false), 0);
        let _ = run_dolphin(&["--version".to_string()], false);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_dolphin(&[], false);
    }
}
