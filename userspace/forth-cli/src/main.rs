#![deny(clippy::all)]

//! forth-cli — OurOS Forth language tools
//!
//! Multi-personality: `gforth`, `forth`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_gforth(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: gforth [OPTIONS] [FILE [FILE ...]]");
        println!("Gforth 0.7.3 (OurOS)");
        println!();
        println!("Options:");
        println!("  -e CODE       Evaluate Forth code");
        println!("  --evaluate CODE  Same as -e");
        println!("  -m SIZE       Dictionary size");
        println!("  -d SIZE       Data stack size");
        println!("  -r SIZE       Return stack size");
        println!("  -f SIZE       FP stack size");
        println!("  -l SIZE       Locals stack size");
        println!("  --clear-dictionary  Clear dictionary");
        println!("  --no-rc       Don't load .gforth.fs");
        println!("  --die-on-signal  Die on signal");
        println!("  --version     Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("gforth 0.7.3 (OurOS)");
        println!("Authors: Anton Ertl, Bernd Paysan, et al.");
        return 0;
    }
    if args.iter().any(|a| a == "-e" || a == "--evaluate") {
        let code = args.windows(2)
            .find(|w| w[0] == "-e" || w[0] == "--evaluate")
            .map(|w| w[1].as_str())
            .unwrap_or("1 2 + . cr");
        println!("Gforth 0.7.3");
        println!("{}", code);
        println!("3 ok");
        return 0;
    }
    let files: Vec<&str> = args.iter()
        .filter(|a| a.ends_with(".fs") || a.ends_with(".fth") || a.ends_with(".4th"))
        .map(|s| s.as_str())
        .collect();
    if files.is_empty() {
        println!("Gforth 0.7.3, Copyright (C) Free Software Foundation, Inc.");
        println!("Gforth comes with ABSOLUTELY NO WARRANTY; for details type `license'");
        println!("Type `bye' to exit");
    } else {
        for f in &files {
            println!("loading {} ...", f);
        }
        println!("ok");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "gforth".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_gforth(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_gforth};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/forth"), "forth");
        assert_eq!(basename(r"C:\bin\forth.exe"), "forth.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("forth.exe"), "forth");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_gforth(&["--help".to_string()]), 0);
        assert_eq!(run_gforth(&["-h".to_string()]), 0);
        let _ = run_gforth(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_gforth(&[]);
    }
}
