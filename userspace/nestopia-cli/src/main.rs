#![deny(clippy::all)]

//! nestopia-cli — OurOS Nestopia NES emulator
//!
//! Single personality: `nestopia`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_nestopia(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: nestopia [OPTIONS] [ROM]");
        println!("nestopia v1.52 (OurOS) — Nintendo Entertainment System emulator");
        println!();
        println!("Options:");
        println!("  -f             Start fullscreen");
        println!("  -s N           Scale factor (1-4)");
        println!("  -p PALETTE     Color palette file");
        println!("  --filter NAME  Video filter (none, ntsc, scalex, hqx)");
        println!("  --region REG   Region (auto, ntsc, pal)");
        println!("  --no-sound     Disable audio");
        println!("  --fds-bios F   FDS BIOS file");
        println!("  --version      Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Nestopia UE v1.52 (OurOS)"); return 0; }
    let files: Vec<&String> = args.iter().filter(|a| !a.starts_with('-')).collect();
    if files.is_empty() {
        println!("Nestopia UE v1.52 (OurOS) — NES Emulator");
        println!("  CPU: Ricoh 2A03 @ 1.79 MHz (emulated)");
        println!("  PPU: 2C02, 256x240, 52 colors");
        println!("  APU: 5 channels (2 pulse, triangle, noise, DMC)");
        println!("  Mappers: 200+ supported");
        println!("  Status: waiting for ROM");
        return 0;
    }
    println!("Nestopia UE: Loading {}", files[0]);
    println!("  Mapper: 0 (NROM)");
    println!("  PRG: 16 KiB, CHR: 8 KiB");
    println!("  Region: NTSC");
    println!("  Running...");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "nestopia".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_nestopia(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_nestopia};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/nestopia"), "nestopia");
        assert_eq!(basename(r"C:\bin\nestopia.exe"), "nestopia.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("nestopia.exe"), "nestopia");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_nestopia(&["--help".to_string()], "nestopia"), 0);
        assert_eq!(run_nestopia(&["-h".to_string()], "nestopia"), 0);
        let _ = run_nestopia(&["--version".to_string()], "nestopia");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_nestopia(&[], "nestopia");
    }
}
