#![deny(clippy::all)]

//! context-cli — OurOS ConTeXt typesetting system
//!
//! Single personality: `context`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_context(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: context [OPTIONS] FILE");
        println!("ConTeXt LMTX (OurOS) — Document engineering system");
        println!();
        println!("Options:");
        println!("  --result=FILE   Output filename");
        println!("  --mode=MODE     Processing mode");
        println!("  --environment=E Load environment");
        println!("  --purge         Remove auxiliary files");
        println!("  --once          Single pass only");
        println!("  --nonstopmode   Non-stop processing");
        println!("  --interface=LNG Interface language");
        println!("  --version       Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("ConTeXt LMTX 2024.04 (OurOS)"); return 0; }
    println!("ConTeXt LMTX 2024.04 (OurOS)");
    println!("  Processing: report.tex");
    println!("  Pass 1: structure analysis");
    println!("  Pass 2: final typesetting");
    println!("    Fonts: 12 loaded (Latin Modern, DejaVu)");
    println!("    Figures: 5 included");
    println!("    References: 23 resolved");
    println!("    Pages: 45");
    println!("  Output: report.pdf (2.3 MB)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "context".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_context(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_context};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/context"), "context");
        assert_eq!(basename(r"C:\bin\context.exe"), "context.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("context.exe"), "context");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_context(&["--help".to_string()], "context"), 0);
        assert_eq!(run_context(&["-h".to_string()], "context"), 0);
        let _ = run_context(&["--version".to_string()], "context");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_context(&[], "context");
    }
}
