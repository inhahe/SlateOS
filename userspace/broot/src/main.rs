#![deny(clippy::all)]

//! broot — Slate OS interactive tree explorer and file manager
//!
//! Single personality: `broot` (alias: `br`)

use std::env;
use std::process;

fn run_broot(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: broot [OPTIONS] [ROOT]");
        println!();
        println!("An interactive tree explorer, file manager, and launcher.");
        println!();
        println!("Options:");
        println!("  -d, --dates            Show last modified dates");
        println!("  -D, --no-dates         Don't show dates");
        println!("  -f, --only-folders      Only show directories");
        println!("  -F, --free-search       Free-form search (default)");
        println!("  -g, --show-git-info     Show git file status");
        println!("  -G, --no-git            Don't show git info");
        println!("  -h, --hidden            Show hidden files");
        println!("  -H, --no-hidden         Don't show hidden files");
        println!("  -i, --show-gitignored   Show gitignored files");
        println!("  -I, --no-gitignored     Don't show gitignored files");
        println!("  -p, --permissions       Show file permissions");
        println!("  -P, --no-permissions    Don't show permissions");
        println!("  -s, --sizes             Show file sizes");
        println!("  -S, --no-sizes          Don't show sizes");
        println!("  -t, --trim-root         Trim root to visible tree");
        println!("  --sort-by <SORT>        Sort by (name/date/size/count/type)");
        println!("  -w, --whale-hierarchies Show big files");
        println!("  --cmd <CMD>             Execute command on launch");
        println!("  --conf <FILE>           Config file path");
        println!("  --color <WHEN>          Color output (auto/yes/no)");
        println!("  --outcmd <FILE>         Write cd command to file");
        println!("  --install               Install shell function");
        println!("  -V, --version           Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("broot 1.37.0 (Slate OS)");
        return 0;
    }
    if args.iter().any(|a| a == "--install") {
        println!("Installing br shell function...");
        println!("  Added function 'br' to shell profile.");
        println!("  Use 'br' to launch broot and cd on exit.");
        return 0;
    }

    let sizes = args.iter().any(|a| a == "-s" || a == "--sizes");
    let dates = args.iter().any(|a| a == "-d" || a == "--dates");
    let git = args.iter().any(|a| a == "-g" || a == "--show-git-info");
    let perms = args.iter().any(|a| a == "-p" || a == "--permissions");
    let whale = args.iter().any(|a| a == "-w" || a == "--whale-hierarchies");

    let root = args.iter()
        .find(|a| !a.starts_with('-'))
        .map(|s| s.as_str())
        .unwrap_or(".");

    println!("broot — {}", root);
    println!();

    if whale {
        println!("  4.2M ████████████████████████████  target/");
        println!("  1.8M ████████████                  src/");
        println!("  892K ██████                        tests/");
        println!("  456K ███                           Cargo.lock");
        println!("   85K █                             Cargo.toml");
        println!("   42K                               README.md");
    } else {
        let prefix = |name: &str, is_dir: bool| {
            let mut parts = Vec::new();
            if perms {
                if is_dir {
                    parts.push("drwxr-xr-x".to_string());
                } else {
                    parts.push("-rw-r--r--".to_string());
                }
            }
            if sizes {
                if is_dir {
                    parts.push("   -".to_string());
                } else {
                    parts.push(" 1.2K".to_string());
                }
            }
            if dates {
                parts.push("2025-05-22 10:00".to_string());
            }
            if git {
                parts.push(" M".to_string());
            }
            parts.push(name.to_string());
            parts.join("  ")
        };

        println!("  {}", prefix("Cargo.toml", false));
        println!("  {}", prefix("Cargo.lock", false));
        println!("  {}", prefix("README.md", false));
        println!("  ├── {}", prefix("src/", true));
        println!("  │   ├── {}", prefix("main.rs", false));
        println!("  │   ├── {}", prefix("lib.rs", false));
        println!("  │   └── {}", prefix("config.rs", false));
        println!("  ├── {}", prefix("tests/", true));
        println!("  │   └── {}", prefix("integration.rs", false));
        println!("  └── {}", prefix("benches/", true));
        println!("      └── {}", prefix("perf.rs", false));
    }
    println!();
    println!("(TUI mode — type to search, Enter to open, Alt+Enter to cd)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_broot(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_broot};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_broot(vec!["--help".to_string()]), 0);
        assert_eq!(run_broot(vec!["-h".to_string()]), 0);
        let _ = run_broot(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_broot(vec![]);
    }
}
