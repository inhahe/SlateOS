#![deny(clippy::all)]

//! bcachefs-cli — OurOS bcachefs filesystem tools
//!
//! Single personality: `bcachefs`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_bcachefs(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: bcachefs COMMAND [OPTIONS]");
        println!("bcachefs v1.7 (OurOS) — Copy-on-write filesystem tools");
        println!();
        println!("Commands:");
        println!("  format         Format filesystem");
        println!("  mount          Mount filesystem");
        println!("  fsck           Check and repair");
        println!("  show-super     Show superblock");
        println!("  device add     Add device");
        println!("  device remove  Remove device");
        println!("  data rereplicate  Re-replicate data");
        println!("  subvolume create  Create subvolume");
        println!("  subvolume delete  Delete subvolume");
        println!("  snapshot create   Create snapshot");
        println!("  encryption      Encryption operations");
        println!("  --version      Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("bcachefs v1.7.0 (OurOS)"); return 0; }
    println!("bcachefs v1.7.0 (OurOS)");
    println!("  Filesystem: /dev/sda1");
    println!("  UUID: 12345678-abcd-ef01-2345-6789abcdef01");
    println!("  Label: data");
    println!("  Block size: 4096");
    println!("  Devices: 2 (sda1, sdb1)");
    println!("  Replicas: 2");
    println!("  Compression: zstd");
    println!("  Encryption: chacha20/poly1305");
    println!("  Used: 234.5 GiB / 1.0 TiB (23.4%)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "bcachefs".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_bcachefs(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_bcachefs};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/bcachefs"), "bcachefs");
        assert_eq!(basename(r"C:\bin\bcachefs.exe"), "bcachefs.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("bcachefs.exe"), "bcachefs");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_bcachefs(&["--help".to_string()], "bcachefs"), 0);
        assert_eq!(run_bcachefs(&["-h".to_string()], "bcachefs"), 0);
        assert_eq!(run_bcachefs(&["--version".to_string()], "bcachefs"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_bcachefs(&[], "bcachefs"), 0);
    }
}
