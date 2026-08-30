#![deny(clippy::all)]

//! pomfort-cli — Slate OS Pomfort on-set workflow tools
//!
//! Single personality: `pomfort`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_pomfort(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: pomfort [COMMAND] [OPTIONS]");
        println!("Pomfort Suite (Slate OS) — DIT on-set data management & live color");
        println!();
        println!("Commands:");
        println!("  silverstack            Silverstack 8 (asset management)");
        println!("  livegrade              LiveGrade Pro 6 (on-set color)");
        println!("  offload                Offload Manager (camera offload)");
        println!("  printer-lights         Printer Lights CDL grading");
        println!("  ");
        println!("Options:");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Pomfort Silverstack 8 / LiveGrade 6 (Slate OS)"); return 0; }
    println!("Pomfort Suite (Slate OS)");
    println!("  Silverstack: Camera card backup, MHL verification, metadata workflow");
    println!("  LiveGrade: On-set CDL/LUT live monitoring, multi-camera matching");
    println!("  Offload Manager: Free tool for camera card backup with verification");
    println!("  Used by: DITs on Hollywood features, TV episodics, commercials");
    println!("  License: subscription / perpetual");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "pomfort".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_pomfort(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_pomfort};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/pomfort"), "pomfort");
        assert_eq!(basename(r"C:\bin\pomfort.exe"), "pomfort.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("pomfort.exe"), "pomfort");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_pomfort(&["--help".to_string()], "pomfort"), 0);
        assert_eq!(run_pomfort(&["-h".to_string()], "pomfort"), 0);
        let _ = run_pomfort(&["--version".to_string()], "pomfort");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_pomfort(&[], "pomfort");
    }
}
