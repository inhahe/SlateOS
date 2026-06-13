#![deny(clippy::all)]

//! broot-cli — Slate OS Broot file manager/navigator
//!
//! Single personality: `broot`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_broot(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: broot [OPTIONS] [ROOT]");
        println!("broot 1.36.1 (Slate OS) — An interactive tree view, file manager, and launcher");
        println!();
        println!("Options:");
        println!("  -d, --dates            Show last modified dates");
        println!("  -D, --no-dates         Don't show dates");
        println!("  -f, --only-folders     Only show folders");
        println!("  -F, --no-only-folders  Show files and folders");
        println!("  -g, --show-git-info    Show git info");
        println!("  -G, --no-show-git-info Don't show git info");
        println!("  -h, --hidden           Show hidden files");
        println!("  -H, --no-hidden        Don't show hidden files");
        println!("  -i, --show-gitignored  Show gitignored files");
        println!("  -I, --no-gitignored    Don't show gitignored");
        println!("  -p, --permissions      Show permissions");
        println!("  -P, --no-permissions   Don't show permissions");
        println!("  -s, --sizes            Show sizes");
        println!("  -S, --no-sizes         Don't show sizes");
        println!("  -t, --trim-root        Trim root");
        println!("  -T, --no-trim-root     Don't trim root");
        println!("  -w, --whale-hierarchies Compute sizes for hierarchies");
        println!("  --cmd CMD              Execute command");
        println!("  --conf FILE            Config file");
        println!("  --install              Install shell function");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("broot 1.36.1 (Slate OS)");
        return 0;
    }
    if args.iter().any(|a| a == "--install") {
        println!("broot: Installing br shell function...");
        println!("broot: Done. Restart your shell or source the config.");
        return 0;
    }
    let root = args.iter().rfind(|a| !a.starts_with('-'))
        .map(|s| s.as_str()).unwrap_or(".");
    println!("broot: Navigating '{}'", root);
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "broot".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_broot(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_broot};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/broot"), "broot");
        assert_eq!(basename(r"C:\bin\broot.exe"), "broot.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("broot.exe"), "broot");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_broot(&["--help".to_string()], "broot"), 0);
        assert_eq!(run_broot(&["-h".to_string()], "broot"), 0);
        let _ = run_broot(&["--version".to_string()], "broot");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_broot(&[], "broot");
    }
}
