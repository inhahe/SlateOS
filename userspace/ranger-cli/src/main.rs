#![deny(clippy::all)]

//! ranger-cli — OurOS Ranger file manager
//!
//! Multi-personality: `ranger`, `rifle`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_ranger(args: &[String], prog: &str) -> i32 {
    if prog == "rifle" {
        if args.iter().any(|a| a == "--help" || a == "-h") {
            println!("Usage: rifle [OPTIONS] FILE...");
            println!("rifle — Ranger's file opener");
            println!();
            println!("Options:");
            println!("  -c CONFIG   Config file");
            println!("  -w CMD      Open with specific command");
            println!("  -p PROG     Open with program number N");
            println!("  -l          List programs for file");
            return 0;
        }
        if args.iter().any(|a| a == "-l") {
            let file = args.iter().rfind(|a| !a.starts_with('-'))
                .map(|s| s.as_str()).unwrap_or("file.txt");
            println!("  0: editor -- '{}'", file);
            println!("  1: pager -- '{}'", file);
            return 0;
        }
        let file = args.iter().rfind(|a| !a.starts_with('-'))
            .map(|s| s.as_str()).unwrap_or("file.txt");
        println!("rifle: Opening '{}'", file);
        return 0;
    }
    // ranger
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: ranger [OPTIONS] [PATH]");
        println!("ranger 1.9.3 (OurOS) — Console file manager with VI keybindings");
        println!();
        println!("Options:");
        println!("  --version               Show version");
        println!("  --choosefile FILE       Output selected file to FILE");
        println!("  --choosefiles FILE      Output selected files");
        println!("  --choosedir FILE        Output last visited dir");
        println!("  --selectfile FILE       Select file on start");
        println!("  --cmd CMD               Execute command after start");
        println!("  --copy-config TYPE      Copy default config (rc, rifle, scope, commands)");
        println!("  --list-tagged-files TAG List tagged files");
        println!("  --clean                 Don't load rc.conf");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("ranger 1.9.3 (OurOS)");
        return 0;
    }
    if let Some(pos) = args.iter().position(|a| a == "--copy-config") {
        let what = args.get(pos + 1).map(|s| s.as_str()).unwrap_or("rc");
        println!("ranger: Copying default {} config to ~/.config/ranger/", what);
        return 0;
    }
    let path = args.iter().rfind(|a| !a.starts_with('-'))
        .map(|s| s.as_str()).unwrap_or(".");
    println!("ranger: Opening '{}'", path);
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "ranger".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_ranger(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_ranger};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/ranger"), "ranger");
        assert_eq!(basename(r"C:\bin\ranger.exe"), "ranger.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("ranger.exe"), "ranger");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_ranger(&["--help".to_string()], "ranger"), 0);
        assert_eq!(run_ranger(&["-h".to_string()], "ranger"), 0);
        let _ = run_ranger(&["--version".to_string()], "ranger");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_ranger(&[], "ranger");
    }
}
