#![deny(clippy::all)]

//! navi-cli — OurOS navi interactive cheatsheet
//!
//! Single personality: `navi`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_navi(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: navi [COMMAND] [OPTIONS]");
        println!("navi 2.23.0 (OurOS) — Interactive cheatsheet tool");
        println!();
        println!("Commands:");
        println!("  (default)          Browse and select snippets");
        println!("  fn FUNC            Execute a cheat function");
        println!("  info               Show environment info");
        println!("  repo add URL       Add cheat repository");
        println!("  repo browse        Browse available repos");
        println!("  widget SHELL       Print shell widget for eval");
        println!("  best QUERY         Non-interactive best match");
        println!();
        println!("Options:");
        println!("  --cheatsh QUERY    Query cheat.sh");
        println!("  --tldr CMD         Query tldr pages");
        println!("  --path PATH        Extra cheatsheet path");
        println!("  --print            Print command instead of exec");
        println!("  -V, --version      Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("navi 2.23.0 (OurOS)");
        return 0;
    }
    let cmd = args.iter().find(|a| !a.starts_with('-'))
        .map(|s| s.as_str());
    match cmd {
        Some("info") => {
            println!("navi info:");
            println!("  cheats path: ~/.local/share/navi/cheats/");
            println!("  config path: ~/.config/navi/config.yaml");
            println!("  shell: bash");
        }
        Some("repo") => {
            let sub = args.iter().skip_while(|a| a.as_str() != "repo").nth(1)
                .map(|s| s.as_str()).unwrap_or("browse");
            match sub {
                "add" => {
                    let url = args.iter().skip_while(|a| a.as_str() != "add").nth(1)
                        .map(|s| s.as_str()).unwrap_or("<url>");
                    println!("navi: Adding repo '{}'...", url);
                }
                "browse" => println!("navi: Opening cheat repo browser..."),
                _ => println!("navi repo: {}", sub),
            }
        }
        Some("widget") => {
            let shell = args.iter().skip_while(|a| a.as_str() != "widget").nth(1)
                .map(|s| s.as_str()).unwrap_or("bash");
            println!("# navi widget for {}", shell);
            println!("eval \"$(navi widget {})\"", shell);
        }
        Some("best") => {
            let query = args.iter().skip_while(|a| a.as_str() != "best").nth(1)
                .map(|s| s.as_str()).unwrap_or("find");
            println!("navi best match for '{}': find . -name '*.txt'", query);
        }
        Some("fn") => {
            let func = args.iter().skip_while(|a| a.as_str() != "fn").nth(1)
                .map(|s| s.as_str()).unwrap_or("<func>");
            println!("navi fn: Executing '{}'", func);
        }
        _ => {
            if args.iter().any(|a| a == "--cheatsh") {
                let query = args.iter().skip_while(|a| a.as_str() != "--cheatsh").nth(1)
                    .map(|s| s.as_str()).unwrap_or("tar");
                println!("navi (cheat.sh): {}", query);
            } else if args.iter().any(|a| a == "--tldr") {
                let query = args.iter().skip_while(|a| a.as_str() != "--tldr").nth(1)
                    .map(|s| s.as_str()).unwrap_or("tar");
                println!("navi (tldr): {}", query);
            } else {
                println!("navi: Interactive cheatsheet browser...");
            }
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "navi".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_navi(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_navi};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/navi"), "navi");
        assert_eq!(basename(r"C:\bin\navi.exe"), "navi.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("navi.exe"), "navi");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_navi(&["--help".to_string()], "navi"), 0);
        assert_eq!(run_navi(&["-h".to_string()], "navi"), 0);
        assert_eq!(run_navi(&["--version".to_string()], "navi"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_navi(&[], "navi"), 0);
    }
}
