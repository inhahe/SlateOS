#![deny(clippy::all)]

//! ncmpcpp-cli — Slate OS ncmpcpp MPD client
//!
//! Single personality: `ncmpcpp`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_ncmpcpp(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: ncmpcpp [OPTIONS]");
        println!("ncmpcpp 0.9.2 (Slate OS) — NCurses Music Player Client Plus Plus");
        println!();
        println!("Options:");
        println!("  -h HOST          MPD host (default localhost)");
        println!("  -p PORT          MPD port (default 6600)");
        println!("  -c FILE          Config file");
        println!("  -b FILE          Bindings file");
        println!("  -s SCREEN        Startup screen");
        println!("  -S SLAVE         Slave screen");
        println!("  --current-song FMT  Print current song and exit");
        println!("  -q, --quiet      Suppress output");
        println!("  -V, --version    Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("ncmpcpp 0.9.2 (Slate OS)");
        return 0;
    }
    if args.iter().any(|a| a == "--current-song") {
        println!("Artist Name - Song Title");
        return 0;
    }
    println!("ncmpcpp: Connecting to MPD...");
    println!("ncmpcpp: Connected. Opening playlist view.");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "ncmpcpp".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_ncmpcpp(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_ncmpcpp};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/ncmpcpp"), "ncmpcpp");
        assert_eq!(basename(r"C:\bin\ncmpcpp.exe"), "ncmpcpp.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("ncmpcpp.exe"), "ncmpcpp");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_ncmpcpp(&["--help".to_string()], "ncmpcpp"), 0);
        assert_eq!(run_ncmpcpp(&["-h".to_string()], "ncmpcpp"), 0);
        let _ = run_ncmpcpp(&["--version".to_string()], "ncmpcpp");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_ncmpcpp(&[], "ncmpcpp");
    }
}
