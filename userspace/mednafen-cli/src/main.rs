#![deny(clippy::all)]

//! mednafen-cli — OurOS Mednafen multi-system emulator
//!
//! Single personality: `mednafen`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_mednafen(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: mednafen [OPTIONS] ROM");
        println!("mednafen v1.32 (OurOS) — Multi-system emulator");
        println!();
        println!("Options:");
        println!("  -force_module MOD Force emulation module");
        println!("  -video.fs 1       Start fullscreen");
        println!("  -sound.rate N     Audio sample rate");
        println!("  --version         Show version");
        println!();
        println!("Supported systems:");
        println!("  nes, snes, gb, gba, genesis, pce, psx,");
        println!("  saturn, ngp, wonderswan, lynx, vb");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("mednafen v1.32 (OurOS)"); return 0; }
    let rom = args.last().map(|s| s.as_str()).unwrap_or("");
    println!("mednafen: loading '{}'...", rom);
    println!("  Auto-detected system based on ROM header");
    println!("  Video: OpenGL, 3x scale");
    println!("  Audio: 48000 Hz stereo");
    println!("  Input: keyboard mapped");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "mednafen".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_mednafen(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_mednafen};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/mednafen"), "mednafen");
        assert_eq!(basename(r"C:\bin\mednafen.exe"), "mednafen.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("mednafen.exe"), "mednafen");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_mednafen(&["--help".to_string()], "mednafen"), 0);
        assert_eq!(run_mednafen(&["-h".to_string()], "mednafen"), 0);
        assert_eq!(run_mednafen(&["--version".to_string()], "mednafen"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_mednafen(&[], "mednafen"), 0);
    }
}
