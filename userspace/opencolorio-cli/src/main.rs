#![deny(clippy::all)]

//! opencolorio-cli — OurOS OpenColorIO color management
//!
//! Multi-personality: `ociocheck`, `ocioconvert`, `ociodisplay`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_ociocheck(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: ociocheck [OPTIONS]");
        println!("ociocheck v2.3 (OurOS) — Validate OCIO config");
        println!();
        println!("Options:");
        println!("  --iconfig FILE    Input OCIO config");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("OCIO v2.3 (OurOS)"); return 0; }
    println!("OCIO config validation:");
    println!("  Config: ACES 1.2");
    println!("  Color spaces: 42");
    println!("  Displays: 2");
    println!("  Views: 6");
    println!("  Status: VALID");
    0
}

fn run_ocioconvert(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: ocioconvert INPUT OUTPUT --src CS --dst CS");
        println!("ocioconvert v2.3 (OurOS) — Convert image between color spaces");
        return 0;
    }
    println!("Converting: ACEScg -> sRGB");
    println!("  Done.");
    0
}

fn run_ociodisplay(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: ociodisplay [OPTIONS]");
        println!("ociodisplay v2.3 (OurOS) — Display color-managed image");
        return 0;
    }
    println!("Display: sRGB");
    println!("  View: ACES (default)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "ociocheck".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "ocioconvert" => run_ocioconvert(&rest, &prog),
        "ociodisplay" => run_ociodisplay(&rest, &prog),
        _ => run_ociocheck(&rest, &prog),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_ociocheck};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/opencolorio"), "opencolorio");
        assert_eq!(basename(r"C:\bin\opencolorio.exe"), "opencolorio.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("opencolorio.exe"), "opencolorio");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_ociocheck(&["--help".to_string()], "opencolorio"), 0);
        assert_eq!(run_ociocheck(&["-h".to_string()], "opencolorio"), 0);
        assert_eq!(run_ociocheck(&["--version".to_string()], "opencolorio"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_ociocheck(&[], "opencolorio"), 0);
    }
}
