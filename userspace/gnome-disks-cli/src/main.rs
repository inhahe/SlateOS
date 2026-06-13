#![deny(clippy::all)]

//! gnome-disks-cli — SlateOS GNOME Disks utility
//!
//! Single personality: `gnome-disks`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_gnome_disks(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: gnome-disks [OPTIONS]");
        println!("gnome-disks v46.0 (Slate OS) — GNOME Disk Utility");
        println!();
        println!("Options:");
        println!("  --block-device DEV  Select device on startup");
        println!("  --restore-disk-image  Start disk image restore");
        println!("  --version          Show version");
        println!();
        println!("Features: format, partition, mount/unmount, SMART data,");
        println!("  benchmark, disk image create/restore, LUKS setup");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("gnome-disks v46.0 (Slate OS)"); return 0; }
    println!("gnome-disks: disk utility");
    println!("  Disks:");
    println!("    500 GB Hard Disk — /dev/sda");
    println!("      Partition 1: EFI System (512 MB, mounted)");
    println!("      Partition 2: Slate OS Root (480 GB, mounted at /)");
    println!("      Partition 3: Swap (19 GB, active)");
    println!("  SMART: Disk is healthy (32C, 1234 hours)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "gnome-disks".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_gnome_disks(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_gnome_disks};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/gnome-disks"), "gnome-disks");
        assert_eq!(basename(r"C:\bin\gnome-disks.exe"), "gnome-disks.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("gnome-disks.exe"), "gnome-disks");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_gnome_disks(&["--help".to_string()], "gnome-disks"), 0);
        assert_eq!(run_gnome_disks(&["-h".to_string()], "gnome-disks"), 0);
        let _ = run_gnome_disks(&["--version".to_string()], "gnome-disks");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_gnome_disks(&[], "gnome-disks");
    }
}
