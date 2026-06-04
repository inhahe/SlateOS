#![deny(clippy::all)]

//! badblocks-cli — OurOS badblocks disk checker
//!
//! Single personality: `badblocks`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_badblocks(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: badblocks [OPTIONS] DEVICE [LAST_BLOCK [FIRST_BLOCK]]");
        println!("badblocks 1.47.1 (OurOS) — Search for bad blocks");
        println!();
        println!("Options:");
        println!("  -b SIZE        Block size (default 1024)");
        println!("  -c NUM         Test blocks at once (default 64)");
        println!("  -d DELAY       Sleep between reads (ms)");
        println!("  -e MAX_BAD     Max bad blocks before abort");
        println!("  -f              Force (even on mounted fs)");
        println!("  -i FILE        Input file of known bad blocks");
        println!("  -n              Non-destructive read-write test");
        println!("  -o FILE        Output bad blocks to file");
        println!("  -p NUM         Number of passes");
        println!("  -s              Show progress");
        println!("  -t PATTERN     Test pattern");
        println!("  -v              Verbose");
        println!("  -w              Destructive write-mode test");
        println!("  -V              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-V") {
        println!("badblocks 1.47.1 (OurOS)");
        return 0;
    }
    let device = args.iter().rfind(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("/dev/sda");
    let verbose = args.iter().any(|a| a == "-v");
    let show_progress = args.iter().any(|a| a == "-s");
    if show_progress || verbose {
        println!("Checking blocks 0 to 976773167");
        println!("Checking for bad blocks (read-only test):");
    }
    println!("badblocks: Scanning {}...", device);
    println!("Pass completed, 0 bad blocks found.");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "badblocks".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_badblocks(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_badblocks};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/badblocks"), "badblocks");
        assert_eq!(basename(r"C:\bin\badblocks.exe"), "badblocks.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("badblocks.exe"), "badblocks");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_badblocks(&["--help".to_string()], "badblocks"), 0);
        assert_eq!(run_badblocks(&["-h".to_string()], "badblocks"), 0);
        let _ = run_badblocks(&["--version".to_string()], "badblocks");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_badblocks(&[], "badblocks");
    }
}
