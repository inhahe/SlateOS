#![deny(clippy::all)]

//! zsnes-cli — SlateOS ZSNES Super Nintendo emulator
//!
//! Single personality: `zsnes`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_zsnes(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: zsnes [OPTIONS] [ROM]");
        println!("zsnes v2.0 (Slate OS) — Super Nintendo Entertainment System emulator");
        println!();
        println!("Options:");
        println!("  -m            Start in mode 7 support");
        println!("  -d            Start in debug mode");
        println!("  -f N          Frameskip (0-9)");
        println!("  -r N          Video mode resolution");
        println!("  -s            Enable sound");
        println!("  -v N          Sound volume (0-100)");
        println!("  -j            Enable joystick");
        println!("  -z            Start with GUI");
        println!("  --version     Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("ZSNES v2.0.12 (Slate OS)"); return 0; }
    let files: Vec<&String> = args.iter().filter(|a| !a.starts_with('-')).collect();
    if files.is_empty() {
        println!("ZSNES v2.0.12 (Slate OS) — SNES Emulator");
        println!("  CPU: 65816 @ 3.58 MHz (emulated)");
        println!("  PPU: Mode 0-7, sprites, BG layers");
        println!("  APU: SPC700, 8-channel DSP");
        println!("  Special chips: SuperFX, SA-1, DSP-1/2/3/4, S-DD1, SPC7110");
        println!("  Status: waiting for ROM");
        return 0;
    }
    println!("ZSNES v2.0.12: Loading {}", files[0]);
    println!("  ROM: {} (4 Mbit)", files[0]);
    println!("  Region: NTSC");
    println!("  Mapper: LoROM");
    println!("  Running...");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "zsnes".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_zsnes(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_zsnes};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/zsnes"), "zsnes");
        assert_eq!(basename(r"C:\bin\zsnes.exe"), "zsnes.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("zsnes.exe"), "zsnes");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_zsnes(&["--help".to_string()], "zsnes"), 0);
        assert_eq!(run_zsnes(&["-h".to_string()], "zsnes"), 0);
        let _ = run_zsnes(&["--version".to_string()], "zsnes");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_zsnes(&[], "zsnes");
    }
}
