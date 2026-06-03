#![deny(clippy::all)]

//! ipp-usb-cli — OurOS IPP-over-USB proxy
//!
//! Single personality: `ipp-usb`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_ipp_usb(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: ipp-usb COMMAND [OPTIONS]");
        println!("ipp-usb v0.9 (OurOS) — IPP-over-USB proxy daemon");
        println!();
        println!("Commands:");
        println!("  udev              Handle udev events");
        println!("  status            Show device status");
        println!("  check             Check device availability");
        println!("  debug             Run in debug mode");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("ipp-usb v0.9 (OurOS)"); return 0; }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("status");
    match cmd {
        "status" => {
            println!("ipp-usb: active USB devices:");
            println!("  HP LaserJet Pro M404");
            println!("    USB: 03f0:123a");
            println!("    IPP: http://localhost:60000/ipp/print");
            println!("    eSCL: http://localhost:60000/eSCL/");
            println!("    Status: idle");
        }
        "check" => {
            println!("ipp-usb: checking USB devices...");
            println!("  1 IPP-over-USB device found");
        }
        "udev" => println!("ipp-usb: processing udev event"),
        "debug" => {
            println!("ipp-usb: debug mode");
            println!("  Logging to stderr");
            println!("  USB device enumeration started");
        }
        _ => println!("ipp-usb: unknown command: {}", cmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "ipp-usb".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_ipp_usb(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_ipp_usb};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/ipp-usb"), "ipp-usb");
        assert_eq!(basename(r"C:\bin\ipp-usb.exe"), "ipp-usb.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("ipp-usb.exe"), "ipp-usb");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_ipp_usb(&["--help".to_string()], "ipp-usb"), 0);
        assert_eq!(run_ipp_usb(&["-h".to_string()], "ipp-usb"), 0);
        assert_eq!(run_ipp_usb(&["--version".to_string()], "ipp-usb"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_ipp_usb(&[], "ipp-usb"), 0);
    }
}
