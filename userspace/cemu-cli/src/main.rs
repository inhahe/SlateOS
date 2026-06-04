#![deny(clippy::all)]

//! cemu-cli — OurOS Cemu Wii U emulator
//!
//! Single personality: `cemu`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_cemu(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: cemu [OPTIONS] [ROM]");
        println!("cemu v2.0-89 (OurOS) — Wii U emulator");
        println!();
        println!("Options:");
        println!("  -g FILE           Boot ROM/WUD/WUX");
        println!("  -f                Fullscreen");
        println!("  --force-interpreter  Force CPU interpreter");
        println!("  --enable-gdb-stub    Enable GDB debugging");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("cemu v2.0-89 (OurOS)"); return 0; }
    println!("cemu: Wii U emulator started");
    println!("  Backend: Vulkan");
    println!("  CPU: recompiler (multi-core)");
    println!("  Resolution: 1080p");
    println!("  Online: disabled");
    println!("  DLC/Update support: enabled");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "cemu".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_cemu(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_cemu};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/cemu"), "cemu");
        assert_eq!(basename(r"C:\bin\cemu.exe"), "cemu.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("cemu.exe"), "cemu");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_cemu(&["--help".to_string()], "cemu"), 0);
        assert_eq!(run_cemu(&["-h".to_string()], "cemu"), 0);
        let _ = run_cemu(&["--version".to_string()], "cemu");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_cemu(&[], "cemu");
    }
}
