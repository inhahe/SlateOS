#![deny(clippy::all)]

//! mixbus-cli — OurOS Harrison Mixbus analog-modeling DAW
//!
//! Single personality: `mixbus`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_mb(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: mixbus [OPTIONS] [SESSION]");
        println!("Harrison Mixbus 10 (OurOS) — Console-style DAW modeling Harrison 32C analog desk");
        println!();
        println!("Options:");
        println!("  --open FILE            Open .ardour-style session");
        println!("  --32c                  Use 32C (12-bus) mixer model");
        println!("  --hybrid               Harrison Hybrid mode (Mixbus + Live mixing)");
        println!("  --export FILE          Export mix");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Harrison Mixbus 10.0.5 (OurOS)"); return 0; }
    println!("Harrison Mixbus 10.0.5 (OurOS)");
    println!("  Editions: Mixbus (8-bus), Mixbus 32C (12-bus + EQ)");
    println!("  Engine: Built on Ardour (open source DAW)");
    println!("  Channel strip: True analog-modeled Harrison EQ, comp, leveler, saturator");
    println!("  Features: AVB networking, Dolby Atmos, ARA2 (Melodyne)");
    println!("  Used by: Sigur Rós, Russ Long, mastering studios");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "mixbus".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_mb(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_mb};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/mixbus"), "mixbus");
        assert_eq!(basename(r"C:\bin\mixbus.exe"), "mixbus.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("mixbus.exe"), "mixbus");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_mb(&["--help".to_string()], "mixbus"), 0);
        assert_eq!(run_mb(&["-h".to_string()], "mixbus"), 0);
        assert_eq!(run_mb(&["--version".to_string()], "mixbus"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_mb(&[], "mixbus"), 0);
    }
}
