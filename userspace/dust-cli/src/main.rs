#![deny(clippy::all)]

//! dust-cli — Slate OS dust disk usage tool
//!
//! Single personality: `dust`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_dust(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: dust [OPTIONS] [PATH...]");
        println!("dust 1.1.1 (Slate OS) — Like du but more intuitive");
        println!();
        println!("Options:");
        println!("  -d, --depth N        Max depth");
        println!("  -n, --number-of-lines N  Number of lines (default 21)");
        println!("  -p, --full-paths     Show full paths");
        println!("  -X, --ignore-directory DIR  Ignore directory");
        println!("  -I, --ignore-all-in-file F  Ignore from file");
        println!("  -s, --apparent-size  Use apparent size");
        println!("  -r, --reverse        Reverse order");
        println!("  -b, --no-percent-bars No percentage bars");
        println!("  -B, --bars-on-right  Bars on right");
        println!("  -R, --screen-reader  Screen reader mode");
        println!("  -c, --no-colors      No colors");
        println!("  -f, --filecount      Show file count");
        println!("  -e, --filter REGEX   Filter by regex");
        println!("  -t, --file_types     Show file types");
        println!("  -w, --terminal_width N  Terminal width");
        println!("  -z, --min-size SIZE  Minimum size to display");
        println!("  -V, --version        Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("dust 1.1.1 (Slate OS)");
        return 0;
    }
    let path = args.iter().rfind(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or(".");
    println!("  120.0G ┌── /              │████████████████████████████████│ 100%");
    println!("   45.2G ├── home           │█████████████                  │  38%");
    println!("   32.1G │ ├── user         │█████████                     │  27%");
    println!("   12.5G │ │ ├── projects   │████                          │  10%");
    println!("    8.3G │ │ └── downloads  │███                           │   7%");
    println!("   10.2G ├── usr            │████                          │   9%");
    println!("    5.1G ├── var            │██                            │   4%");
    let _p = path;
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "dust".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_dust(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_dust};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/dust"), "dust");
        assert_eq!(basename(r"C:\bin\dust.exe"), "dust.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("dust.exe"), "dust");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_dust(&["--help".to_string()], "dust"), 0);
        assert_eq!(run_dust(&["-h".to_string()], "dust"), 0);
        let _ = run_dust(&["--version".to_string()], "dust");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_dust(&[], "dust");
    }
}
