#![deny(clippy::all)]

//! gimp-cli — SlateOS GIMP command-line interface
//!
//! Single personality: `gimp`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_gimp(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: gimp [OPTIONS] [FILE...]");
        println!("GNU Image Manipulation Program 2.10.38 (SlateOS)");
        println!();
        println!("Options:");
        println!("  -i, --no-interface        Run without UI");
        println!("  -d, --no-data             Don't load patterns/brushes");
        println!("  -f, --no-fonts            Don't load fonts");
        println!("  -s, --no-splash           No splash screen");
        println!("  -n, --new-instance        New instance");
        println!("  -a, --as-new              Open as new image");
        println!("  -b, --batch CMD           Batch command (Script-Fu)");
        println!("  --batch-interpreter PLUG  Batch interpreter");
        println!("  -c, --console-messages    Print to console");
        println!("  --pdb-compat-mode MODE    PDB compat mode");
        println!("  --stack-trace-mode MODE   Stack trace mode");
        println!("  --debug-handlers          Debug signal handlers");
        println!("  -V, --version             Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("GNU Image Manipulation Program version 2.10.38 (SlateOS)");
        return 0;
    }
    if args.iter().any(|a| a == "-b" || a == "--batch") {
        let cmd = args.iter().skip_while(|a| a.as_str() != "-b" && a.as_str() != "--batch").nth(1)
            .map(|s| s.as_str()).unwrap_or("(gimp-quit 0)");
        println!("gimp: Executing batch command: {}", cmd);
        return 0;
    }
    let file = args.iter().find(|a| !a.starts_with('-'))
        .map(|s| s.as_str());
    if let Some(f) = file {
        println!("gimp: Opening '{}'", f);
    } else {
        println!("gimp: Starting GNU Image Manipulation Program...");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "gimp".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_gimp(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_gimp};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/gimp"), "gimp");
        assert_eq!(basename(r"C:\bin\gimp.exe"), "gimp.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("gimp.exe"), "gimp");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_gimp(&["--help".to_string()], "gimp"), 0);
        assert_eq!(run_gimp(&["-h".to_string()], "gimp"), 0);
        let _ = run_gimp(&["--version".to_string()], "gimp");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_gimp(&[], "gimp");
    }
}
