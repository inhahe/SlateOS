#![deny(clippy::all)]

//! clonezilla-cli — OurOS Clonezilla disk/partition cloning
//!
//! Multi-personality: `clonezilla`, `ocs-sr`, `ocs-onthefly`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_clonezilla(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: clonezilla [OPTIONS]");
        println!("clonezilla v3.1 (OurOS) — Disk/partition cloning");
        println!();
        println!("Modes:");
        println!("  device-image    Save/restore device to/from image");
        println!("  device-device   Clone device to device");
        println!("  remote          Network cloning (multicast)");
        println!();
        println!("Options:");
        println!("  --version       Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("clonezilla v3.1 (OurOS)"); return 0; }
    println!("clonezilla: disk cloning system");
    println!("  Mode: device-image");
    println!("  Select: savedisk / restoredisk / saveparts / restoreparts");
    0
}

fn run_ocs_sr(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: ocs-sr [OPTIONS] <ACTION> <IMAGE> <DEVICES>");
        println!("ocs-sr v3.1 (OurOS) — Clonezilla save/restore");
        println!();
        println!("Actions:");
        println!("  savedisk       Save whole disk as image");
        println!("  restoredisk    Restore disk from image");
        println!("  saveparts      Save partitions as image");
        println!("  restoreparts   Restore partitions from image");
        println!();
        println!("Options:");
        println!("  -q2            Use partclone (default)");
        println!("  -z1p           Use pigz compression");
        println!("  -j2            Clone hidden data");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("ocs-sr v3.1 (OurOS)"); return 0; }
    println!("ocs-sr: clonezilla save/restore");
    println!("  Image repository: /home/partimag");
    0
}

fn run_ocs_onthefly(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: ocs-onthefly [OPTIONS] <SOURCE> <TARGET>");
        println!("ocs-onthefly v3.1 (OurOS) — Direct disk-to-disk clone");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("ocs-onthefly v3.1 (OurOS)"); return 0; }
    println!("ocs-onthefly: direct disk-to-disk clone");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "clonezilla".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "ocs-sr" => run_ocs_sr(&rest, &prog),
        "ocs-onthefly" => run_ocs_onthefly(&rest, &prog),
        _ => run_clonezilla(&rest, &prog),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_clonezilla};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/clonezilla"), "clonezilla");
        assert_eq!(basename(r"C:\bin\clonezilla.exe"), "clonezilla.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("clonezilla.exe"), "clonezilla");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_clonezilla(&["--help".to_string()], "clonezilla"), 0);
        assert_eq!(run_clonezilla(&["-h".to_string()], "clonezilla"), 0);
        assert_eq!(run_clonezilla(&["--version".to_string()], "clonezilla"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_clonezilla(&[], "clonezilla"), 0);
    }
}
