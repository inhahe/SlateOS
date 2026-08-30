#![deny(clippy::all)]

//! desmume-cli — Slate OS DeSmuME Nintendo DS emulator
//!
//! Single personality: `desmume`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_desmume(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: desmume [OPTIONS] [ROM.nds]");
        println!("DeSmuME v0.9.13 (Slate OS) — Nintendo DS emulator");
        println!();
        println!("Options:");
        println!("  --load-slot N     Load save state slot");
        println!("  --opengl-2d       Use OpenGL for 2D rendering");
        println!("  --soft-3d         Software 3D renderer");
        println!("  --3d-engine N     3D engine (0=soft, 1=opengl)");
        println!("  --scale N         Window scale (1-4)");
        println!("  --layout N        Screen layout (0=vertical, 1=horizontal)");
        println!("  --arm9-gdb PORT   ARM9 GDB stub port");
        println!("  --arm7-gdb PORT   ARM7 GDB stub port");
        println!("  --disable-sound   Disable audio");
        println!("  --firmware FILE   Firmware image");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("DeSmuME v0.9.13 (Slate OS)"); return 0; }
    let files: Vec<&String> = args.iter().filter(|a| !a.starts_with('-')).collect();
    if files.is_empty() {
        println!("DeSmuME v0.9.13 (Slate OS) — Nintendo DS Emulator");
        println!("  ARM9: 67 MHz, ARM7: 33 MHz (emulated)");
        println!("  Screens: 256x192 x2 (top + bottom)");
        println!("  3D Engine: software renderer");
        println!("  WiFi: emulated (local only)");
        println!("  Status: waiting for ROM");
        return 0;
    }
    println!("DeSmuME: Loading {}", files[0]);
    println!("  ROM size: 64 MiB");
    println!("  Save type: EEPROM 512K");
    println!("  Renderer: Software 3D");
    println!("  Running...");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "desmume".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_desmume(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_desmume};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/desmume"), "desmume");
        assert_eq!(basename(r"C:\bin\desmume.exe"), "desmume.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("desmume.exe"), "desmume");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_desmume(&["--help".to_string()], "desmume"), 0);
        assert_eq!(run_desmume(&["-h".to_string()], "desmume"), 0);
        let _ = run_desmume(&["--version".to_string()], "desmume");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_desmume(&[], "desmume");
    }
}
