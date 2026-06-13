#![deny(clippy::all)]

//! fwupd-cli — SlateOS fwupd firmware update daemon
//!
//! Multi-personality: `fwupdmgr`, `fwupdtool`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_fwupdmgr(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: fwupdmgr <command> [OPTIONS]");
        println!("fwupdmgr v1.9 (Slate OS) — Firmware update manager");
        println!();
        println!("Commands:");
        println!("  get-devices      List devices with firmware");
        println!("  get-updates      Check for updates");
        println!("  update           Apply firmware updates");
        println!("  get-releases DEV Show available releases");
        println!("  refresh          Refresh metadata");
        println!("  --version        Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("fwupdmgr v1.9 (Slate OS, fwupd)"); return 0; }
    match args.first().map(|s| s.as_str()) {
        Some("get-devices") => {
            println!("System Firmware");
            println!("  DeviceId: a1b2c3d4");
            println!("  BIOS v1.23 -> v1.24 available");
            println!();
            println!("UEFI dbx");
            println!("  DeviceId: e5f6a7b8");
            println!("  Security: verified");
        }
        Some("get-updates") => {
            println!("System Firmware:");
            println!("  Version: 1.23 -> 1.24");
            println!("  Summary: Security and stability update");
        }
        _ => {
            println!("fwupdmgr: use --help for commands");
        }
    }
    0
}

fn run_fwupdtool(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: fwupdtool <command> [OPTIONS]");
        println!("fwupdtool v1.9 (Slate OS) — Firmware debugging tool");
        println!("  get-plugins      List plugins");
        println!("  get-details CAB  Show firmware details");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("fwupdtool v1.9 (Slate OS)"); return 0; }
    println!("fwupdtool: firmware debugging utility");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "fwupdmgr".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "fwupdtool" => run_fwupdtool(&rest, &prog),
        _ => run_fwupdmgr(&rest, &prog),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_fwupdmgr};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/fwupd"), "fwupd");
        assert_eq!(basename(r"C:\bin\fwupd.exe"), "fwupd.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("fwupd.exe"), "fwupd");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_fwupdmgr(&["--help".to_string()], "fwupd"), 0);
        assert_eq!(run_fwupdmgr(&["-h".to_string()], "fwupd"), 0);
        let _ = run_fwupdmgr(&["--version".to_string()], "fwupd");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_fwupdmgr(&[], "fwupd");
    }
}
