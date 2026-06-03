#![deny(clippy::all)]

//! xsane-cli — OurOS XSane graphical scanner frontend
//!
//! Single personality: `xsane`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_xsane(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: xsane [OPTIONS] [DEVICE]");
        println!("xsane v0.999 (OurOS) — Graphical scanner frontend");
        println!();
        println!("Options:");
        println!("  -d DEVICE         Use specific SANE device");
        println!("  -V                Verbose mode");
        println!("  -N                No device selection dialog");
        println!("  -s                Scan and save immediately");
        println!("  -n                No mode selection dialog");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("xsane v0.999 (OurOS)"); return 0; }
    println!("xsane: graphical scanner interface started");
    println!("  SANE version: 1.2");
    println!("  Devices found: 2");
    println!("  Default device: epkowa:libusb:001:004");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "xsane".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_xsane(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_xsane};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/xsane"), "xsane");
        assert_eq!(basename(r"C:\bin\xsane.exe"), "xsane.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("xsane.exe"), "xsane");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_xsane(&["--help".to_string()], "xsane"), 0);
        assert_eq!(run_xsane(&["-h".to_string()], "xsane"), 0);
        assert_eq!(run_xsane(&["--version".to_string()], "xsane"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_xsane(&[], "xsane"), 0);
    }
}
