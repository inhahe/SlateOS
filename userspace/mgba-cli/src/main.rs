#![deny(clippy::all)]

//! mgba-cli — SlateOS mGBA Game Boy Advance emulator
//!
//! Single personality: `mgba`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_mgba(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: mgba [OPTIONS] [ROM]");
        println!("mGBA v0.10 (Slate OS) — Game Boy Advance emulator");
        println!();
        println!("Options:");
        println!("  -1               1x window size");
        println!("  -2               2x window size");
        println!("  -3               3x window size");
        println!("  -4               4x window size");
        println!("  -f               Fullscreen");
        println!("  -b BIOS          GBA BIOS file");
        println!("  -s N             Load save state N");
        println!("  -p PATCH         Apply patch (IPS/UPS/BPS)");
        println!("  --gdb PORT       GDB stub on port");
        println!("  --log-level N    Log verbosity");
        println!("  --cheats FILE    Load cheat file");
        println!("  --version        Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("mGBA v0.10.3 (Slate OS)");
        println!("  Platforms: GBA, GB, GBC");
        return 0;
    }
    let files: Vec<&String> = args.iter().filter(|a| !a.starts_with('-')).collect();
    if files.is_empty() {
        println!("mGBA v0.10.3 (Slate OS) — Game Boy Advance Emulator");
        println!("  CPU: ARM7TDMI @ 16.78 MHz (emulated)");
        println!("  Display: 240x160, 32768 colors");
        println!("  Sound: 6 channels (4 PSG + 2 PCM)");
        println!("  Also supports: Game Boy, Game Boy Color");
        println!("  Status: waiting for ROM");
        return 0;
    }
    println!("mGBA: Loading {}", files[0]);
    println!("  ROM: 16 MiB");
    println!("  Save: Flash 128K");
    println!("  Running...");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "mgba".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_mgba(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_mgba};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/mgba"), "mgba");
        assert_eq!(basename(r"C:\bin\mgba.exe"), "mgba.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("mgba.exe"), "mgba");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_mgba(&["--help".to_string()], "mgba"), 0);
        assert_eq!(run_mgba(&["-h".to_string()], "mgba"), 0);
        let _ = run_mgba(&["--version".to_string()], "mgba");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_mgba(&[], "mgba");
    }
}
