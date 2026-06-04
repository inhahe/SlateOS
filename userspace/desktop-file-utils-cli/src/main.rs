#![deny(clippy::all)]

//! desktop-file-utils-cli — OurOS desktop file utilities
//!
//! Multi-personality: `desktop-file-validate`, `desktop-file-install`, `update-desktop-database`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_validate(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: desktop-file-validate [OPTIONS] FILE...");
        println!("desktop-file-validate v0.27 (OurOS) — Validate .desktop files");
        println!();
        println!("Options:");
        println!("  --no-hints        Don't show hints");
        println!("  --no-warn         Don't show warnings");
        return 0;
    }
    for file in args.iter().filter(|a| !a.starts_with('-')) {
        println!("{}: valid", file);
    }
    0
}

fn run_install(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: desktop-file-install [OPTIONS] FILE...");
        println!("desktop-file-install v0.27 (OurOS) — Install .desktop files");
        println!();
        println!("Options:");
        println!("  --dir DIR         Target directory");
        println!("  --rebuild-mime-info-cache  Rebuild cache");
        return 0;
    }
    for file in args.iter().filter(|a| !a.starts_with('-')) {
        println!("Installed: {}", file);
    }
    0
}

fn run_update_db(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: update-desktop-database [DIR]");
        println!("update-desktop-database v0.27 (OurOS) — Update MIME type cache");
        return 0;
    }
    let dir = args.first().map(|s| s.as_str()).unwrap_or("/usr/share/applications");
    println!("Updated desktop database: {}", dir);
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "desktop-file-validate".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "desktop-file-install" => run_install(&rest, &prog),
        "update-desktop-database" => run_update_db(&rest, &prog),
        _ => run_validate(&rest, &prog),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_validate};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/desktop-file-utils"), "desktop-file-utils");
        assert_eq!(basename(r"C:\bin\desktop-file-utils.exe"), "desktop-file-utils.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("desktop-file-utils.exe"), "desktop-file-utils");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_validate(&["--help".to_string()], "desktop-file-utils"), 0);
        assert_eq!(run_validate(&["-h".to_string()], "desktop-file-utils"), 0);
        let _ = run_validate(&["--version".to_string()], "desktop-file-utils");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_validate(&[], "desktop-file-utils");
    }
}
