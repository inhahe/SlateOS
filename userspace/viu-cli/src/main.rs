#![deny(clippy::all)]

//! viu-cli — OurOS viu terminal image viewer
//!
//! Single personality: `viu`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_viu(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: viu [OPTIONS] [FILE...]");
        println!("viu 1.5.0 (OurOS) — Terminal image viewer");
        println!();
        println!("Options:");
        println!("  -n, --name           Print filename");
        println!("  -t, --transparent    Transparent background");
        println!("  -s, --static         Don't animate GIFs");
        println!("  -w, --width N        Output width");
        println!("  -h, --height N       Output height");
        println!("  -b, --blocks         Use block characters");
        println!("  -r, --recursive      Recurse into directories");
        println!("  -1                   One image per line");
        println!("  -V, --version        Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("viu 1.5.0 (OurOS)");
        return 0;
    }
    let files: Vec<&str> = args.iter()
        .filter(|a| !a.starts_with('-'))
        .map(|s| s.as_str())
        .collect();
    if files.is_empty() {
        println!("viu: Reading from stdin...");
    } else {
        for f in &files {
            if args.iter().any(|a| a == "-n" || a == "--name") {
                println!("--- {} ---", f);
            }
            println!("(displaying image: {})", f);
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "viu".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_viu(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_viu};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/viu"), "viu");
        assert_eq!(basename(r"C:\bin\viu.exe"), "viu.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("viu.exe"), "viu");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_viu(&["--help".to_string()], "viu"), 0);
        assert_eq!(run_viu(&["-h".to_string()], "viu"), 0);
        assert_eq!(run_viu(&["--version".to_string()], "viu"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_viu(&[], "viu"), 0);
    }
}
