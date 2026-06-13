#![deny(clippy::all)]

//! mc-cli — Slate OS Midnight Commander
//!
//! Multi-personality: `mc`, `mcedit`, `mcview`, `mcdiff`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_mc(args: &[String], prog: &str) -> i32 {
    match prog {
        "mcedit" => {
            if args.iter().any(|a| a == "--help" || a == "-h") {
                println!("Usage: mcedit [OPTIONS] [FILE[:LINE]]");
                println!("mcedit — Midnight Commander internal editor");
                println!();
                println!("Options:");
                println!("  -h, --help    Show help");
                println!("  -V, --version Show version");
                return 0;
            }
            if args.iter().any(|a| a == "-V" || a == "--version") {
                println!("mcedit (mc) 4.8.31 (Slate OS)");
                return 0;
            }
            let file = args.iter().rfind(|a| !a.starts_with('-'))
                .map(|s| s.as_str()).unwrap_or("untitled");
            println!("mcedit: Editing '{}'", file);
            return 0;
        }
        "mcview" => {
            if args.iter().any(|a| a == "--help" || a == "-h") {
                println!("Usage: mcview [OPTIONS] FILE");
                println!("mcview — Midnight Commander internal viewer");
                return 0;
            }
            let file = args.iter().rfind(|a| !a.starts_with('-'))
                .map(|s| s.as_str()).unwrap_or("file");
            println!("mcview: Viewing '{}'", file);
            return 0;
        }
        "mcdiff" => {
            if args.iter().any(|a| a == "--help" || a == "-h") {
                println!("Usage: mcdiff FILE1 FILE2");
                println!("mcdiff — Midnight Commander visual diff");
                return 0;
            }
            let f1 = args.first().map(|s| s.as_str()).unwrap_or("file1");
            let f2 = args.get(1).map(|s| s.as_str()).unwrap_or("file2");
            println!("mcdiff: Comparing '{}' and '{}'", f1, f2);
            return 0;
        }
        _ => {}
    }
    // mc
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: mc [OPTIONS] [THIS_DIR [OTHER_DIR]]");
        println!("GNU Midnight Commander 4.8.31 (Slate OS)");
        println!();
        println!("Options:");
        println!("  -a, --stickchars       No langstrstrstrstrstrstrstr strstrstrstrstr");
        println!("  -c, --color            Force color mode");
        println!("  -C COLORS              Custom colors");
        println!("  -d, --nomouse          Disable mouse");
        println!("  -e [FILE]              Edit file (mcedit)");
        println!("  -f, --datadir          Print data directory");
        println!("  -k, --resetsoft        Reset softkeys");
        println!("  -l LOG                 Log file");
        println!("  -P FILE                Print last dir to file");
        println!("  -s, --slow             Slow terminal mode");
        println!("  -S SKIN                Skin file");
        println!("  -t, --termcap          Use termcap");
        println!("  -u, --nostrstrstrstr    No strstrstrstrstr");
        println!("  -U, --strstrstrstrstr   Enable strstrstrstrstr");
        println!("  -v FILE                View file (mcview)");
        println!("  -V, --version          Show version");
        println!("  -x, --xterm            Force xterm mode");
        return 0;
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("GNU Midnight Commander 4.8.31 (Slate OS)");
        return 0;
    }
    if args.iter().any(|a| a == "-f" || a == "--datadir") {
        println!("/usr/share/mc");
        return 0;
    }
    let dir = args.iter().find(|a| !a.starts_with('-'))
        .map(|s| s.as_str()).unwrap_or(".");
    println!("mc: Opening panels at '{}'", dir);
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "mc".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_mc(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_mc};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/mc"), "mc");
        assert_eq!(basename(r"C:\bin\mc.exe"), "mc.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("mc.exe"), "mc");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_mc(&["--help".to_string()], "mc"), 0);
        assert_eq!(run_mc(&["-h".to_string()], "mc"), 0);
        let _ = run_mc(&["--version".to_string()], "mc");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_mc(&[], "mc");
    }
}
