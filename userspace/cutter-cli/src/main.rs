#![deny(clippy::all)]

//! cutter-cli — OurOS Cutter reverse engineering GUI
//!
//! Single personality: `cutter`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_cutter(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: cutter [OPTIONS] [FILE]");
        println!("Cutter v2.3 (OurOS) — GUI for Rizin reverse engineering");
        println!();
        println!("Options:");
        println!("  -A               Auto-analyze on open");
        println!("  --no-plugins     Disable plugins");
        println!("  --script FILE    Run script on startup");
        println!("  --pythonhome DIR Python home directory");
        println!("  --version        Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Cutter v2.3.4 (OurOS) (Rizin v0.7.2)"); return 0; }
    println!("Cutter v2.3.4 (OurOS)");
    println!("  Backend: Rizin v0.7.2");
    println!("  Loaded: malware_sample.exe");
    println!("  Format: PE32+, x86_64, Windows");
    println!("  Analysis:");
    println!("    Functions: 234");
    println!("    Strings: 567");
    println!("    Imports: 89 (kernel32, ntdll, ws2_32)");
    println!("  Widgets: Disassembly, Graph, Decompiler, Hex, Strings");
    println!("  Plugins: Ghidra decompiler bridge active");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "cutter".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_cutter(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_cutter};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/cutter"), "cutter");
        assert_eq!(basename(r"C:\bin\cutter.exe"), "cutter.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("cutter.exe"), "cutter");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_cutter(&["--help".to_string()], "cutter"), 0);
        assert_eq!(run_cutter(&["-h".to_string()], "cutter"), 0);
        assert_eq!(run_cutter(&["--version".to_string()], "cutter"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_cutter(&[], "cutter"), 0);
    }
}
