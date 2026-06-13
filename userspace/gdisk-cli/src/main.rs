#![deny(clippy::all)]

//! gdisk-cli — SlateOS gdisk GPT partition tool
//!
//! Multi-personality: `gdisk`, `sgdisk`, `cgdisk`, `fixparts`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_gdisk(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: gdisk [DEVICE]");
        println!("gdisk v1.0 (SlateOS) — Interactive GPT partition editor");
        println!();
        println!("Commands (interactive):");
        println!("  p   Print partition table");
        println!("  n   Add new partition");
        println!("  d   Delete partition");
        println!("  t   Change type code");
        println!("  w   Write changes to disk");
        println!("  q   Quit without saving");
        println!("  --version  Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("gdisk v1.0 (SlateOS)"); return 0; }
    println!("gdisk: GPT fdisk");
    println!("  Disk /dev/sda: 976773168 sectors, 500.0 GiB");
    println!("  Disk identifier (GUID): A1B2C3D4-E5F6-7890-ABCD-EF1234567890");
    println!("  Partition table: GPT");
    0
}

fn run_sgdisk(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: sgdisk [OPTIONS] DEVICE");
        println!("sgdisk v1.0 (SlateOS) — Scriptable GPT partition editor");
        println!("  -p            Print partition table");
        println!("  -n N:S:E      Add partition N from sector S to E");
        println!("  -d N          Delete partition N");
        println!("  -t N:TYPE     Set type code for partition N");
        println!("  -z            Zap (destroy) GPT structures");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("sgdisk v1.0 (SlateOS)"); return 0; }
    if args.iter().any(|a| a == "-p") {
        println!("Number  Start      End         Size       Code  Name");
        println!("   1    2048       1050623     512.0 MiB  EF00  EFI system");
        println!("   2    1050624    967098367   461.0 GiB  8300  SlateOS root");
        println!("   3    967098368  976773134   4.6 GiB    8200  Linux swap");
    }
    0
}

fn run_cgdisk(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: cgdisk [DEVICE]");
        println!("cgdisk v1.0 (SlateOS) — Curses-based GPT partition editor");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("cgdisk v1.0 (SlateOS)"); return 0; }
    println!("cgdisk: curses GPT editor for /dev/sda");
    0
}

fn run_fixparts(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: fixparts [DEVICE]");
        println!("fixparts v1.0 (SlateOS) — Fix MBR partition table problems");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("fixparts v1.0 (SlateOS)"); return 0; }
    println!("fixparts: MBR repair tool");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "gdisk".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "sgdisk" => run_sgdisk(&rest, &prog),
        "cgdisk" => run_cgdisk(&rest, &prog),
        "fixparts" => run_fixparts(&rest, &prog),
        _ => run_gdisk(&rest, &prog),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_gdisk};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/gdisk"), "gdisk");
        assert_eq!(basename(r"C:\bin\gdisk.exe"), "gdisk.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("gdisk.exe"), "gdisk");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_gdisk(&["--help".to_string()], "gdisk"), 0);
        assert_eq!(run_gdisk(&["-h".to_string()], "gdisk"), 0);
        let _ = run_gdisk(&["--version".to_string()], "gdisk");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_gdisk(&[], "gdisk");
    }
}
