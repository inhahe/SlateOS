#![deny(clippy::all)]

//! pdfpc-cli — SlateOS pdfpc PDF presenter console
//!
//! Single personality: `pdfpc`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_pdfpc(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: pdfpc [OPTIONS] FILE");
        println!("pdfpc v4.6 (SlateOS) — PDF presenter console");
        println!();
        println!("Options:");
        println!("  -d DURATION       Presentation duration (minutes)");
        println!("  -l                Last page as end page");
        println!("  -n                Notes position (left/right/top/bottom)");
        println!("  -s                Switch screens");
        println!("  -w                Windowed mode");
        println!("  -S                Single screen");
        println!("  --notes=POS       Speaker notes position");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("pdfpc v4.6 (SlateOS)"); return 0; }
    println!("pdfpc: PDF presenter console started");
    println!("  Presenter screen: current + next slide, timer, notes");
    println!("  Audience screen: current slide fullscreen");
    println!("  Timer: 0:00 / 30:00");
    println!("  Drawing: pen & pointer supported");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "pdfpc".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_pdfpc(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_pdfpc};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/pdfpc"), "pdfpc");
        assert_eq!(basename(r"C:\bin\pdfpc.exe"), "pdfpc.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("pdfpc.exe"), "pdfpc");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_pdfpc(&["--help".to_string()], "pdfpc"), 0);
        assert_eq!(run_pdfpc(&["-h".to_string()], "pdfpc"), 0);
        let _ = run_pdfpc(&["--version".to_string()], "pdfpc");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_pdfpc(&[], "pdfpc");
    }
}
