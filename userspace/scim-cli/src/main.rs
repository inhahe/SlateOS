#![deny(clippy::all)]

//! scim-cli — OurOS SCIM input method platform
//!
//! Multi-personality: `scim`, `scim-setup`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_scim(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: scim [OPTIONS]");
        println!("scim v1.4 (OurOS) — Smart Common Input Method platform");
        println!();
        println!("Options:");
        println!("  -d                Run as daemon");
        println!("  -l                List input methods");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("scim v1.4 (OurOS)"); return 0; }
    if args.iter().any(|a| a == "-l") {
        println!("Available input methods:");
        println!("  English/European");
        println!("  Chinese/Pinyin");
        println!("  Japanese/Anthy");
        println!("  Korean/Hangul");
        return 0;
    }
    println!("scim: input method daemon started");
    0
}

fn run_setup(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: scim-setup [OPTIONS]");
        println!("scim-setup v1.4 (OurOS) — SCIM configuration");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("scim-setup v1.4 (OurOS)"); return 0; }
    println!("scim-setup: configuration dialog opened");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "scim".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "scim-setup" => run_setup(&rest, &prog),
        _ => run_scim(&rest, &prog),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_scim};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/scim"), "scim");
        assert_eq!(basename(r"C:\bin\scim.exe"), "scim.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("scim.exe"), "scim");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_scim(&["--help".to_string()], "scim"), 0);
        assert_eq!(run_scim(&["-h".to_string()], "scim"), 0);
        assert_eq!(run_scim(&["--version".to_string()], "scim"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_scim(&[], "scim"), 0);
    }
}
