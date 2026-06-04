#![deny(clippy::all)]

//! nnn-cli — OurOS nnn file manager
//!
//! Single personality: `nnn`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_nnn(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: nnn [OPTIONS] [PATH]");
        println!("nnn v4.9 (OurOS) — The unorthodox terminal file manager");
        println!();
        println!("Options:");
        println!("  -a           Auto-setup temporary NNN_FIFO");
        println!("  -A           No directory auto-enter on unique filter match");
        println!("  -b KEY       Open bookmark KEY");
        println!("  -B           Use bsdtar for archive listing");
        println!("  -c           CLI-only opener");
        println!("  -C           8-color scheme");
        println!("  -d           Detail mode");
        println!("  -D           Show dirs in context color");
        println!("  -e           Text in $VISUAL/$EDITOR/vi");
        println!("  -E           Use $EDITOR for undetached edits");
        println!("  -f           Use readline history file");
        println!("  -g           Regex filters");
        println!("  -H           Show hidden files");
        println!("  -J           No auto-advance on selection");
        println!("  -K           Test for keybind collision");
        println!("  -l N         Lines to show (0=auto)");
        println!("  -n           Type-to-nav mode");
        println!("  -o           Open files on Enter");
        println!("  -p FILE      Copy selection to FILE");
        println!("  -P KEY       Run plugin KEY");
        println!("  -Q           No quit confirmation");
        println!("  -r           Use advcpmv for progress");
        println!("  -R           No rollover at edges");
        println!("  -s NAME      Named session");
        println!("  -S           Persistent session");
        println!("  -t SECS      Idle timeout to lock");
        println!("  -T KEY       Sort by (key: a/d/e/r/s/t/v)");
        println!("  -u           Use selection (no prompt)");
        println!("  -U           Show user and group");
        println!("  -V           Show version");
        println!("  -x           Copy path to system clipboard");
        return 0;
    }
    if args.iter().any(|a| a == "-V") {
        println!("nnn v4.9 (OurOS)");
        return 0;
    }
    let path = args.iter().rfind(|a| !a.starts_with('-'))
        .map(|s| s.as_str()).unwrap_or(".");
    println!("nnn: Opening '{}'", path);
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "nnn".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_nnn(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_nnn};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/nnn"), "nnn");
        assert_eq!(basename(r"C:\bin\nnn.exe"), "nnn.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("nnn.exe"), "nnn");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_nnn(&["--help".to_string()], "nnn"), 0);
        assert_eq!(run_nnn(&["-h".to_string()], "nnn"), 0);
        let _ = run_nnn(&["--version".to_string()], "nnn");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_nnn(&[], "nnn");
    }
}
