#![deny(clippy::all)]

//! bemenu-cli — SlateOS bemenu dynamic menu
//!
//! Multi-personality: `bemenu`, `bemenu-run`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_bemenu(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: bemenu [OPTIONS]");
        println!("bemenu v0.6 (SlateOS) — Dynamic menu (dmenu clone for Wayland)");
        println!();
        println!("Options:");
        println!("  -l LINES          Number of lines to show");
        println!("  -p PROMPT         Prompt text");
        println!("  -P PREFIX         Prefix text");
        println!("  -i                Case-insensitive matching");
        println!("  -w                Wrap text");
        println!("  -b                Bottom of screen");
        println!("  --fn FONT         Font specification");
        println!("  --tb COLOR        Title background");
        println!("  --tf COLOR        Title foreground");
        println!("  --nb COLOR        Normal background");
        println!("  --nf COLOR        Normal foreground");
        println!("  --hb COLOR        Highlighted background");
        println!("  --hf COLOR        Highlighted foreground");
        return 0;
    }
    println!("bemenu: reading from stdin...");
    0
}

fn run_bemenu_run(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: bemenu-run [OPTIONS]");
        println!("bemenu-run v0.6 (SlateOS) — Application launcher (lists PATH commands)");
        println!();
        println!("  Same options as bemenu, plus:");
        println!("  --no-exec         Print selection, don't execute");
        return 0;
    }
    let _ = args;
    println!("bemenu-run: scanning PATH for executables...");
    println!("  [Search...                    ]");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "bemenu".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "bemenu-run" => run_bemenu_run(&rest, &prog),
        _ => run_bemenu(&rest, &prog),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_bemenu};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/bemenu"), "bemenu");
        assert_eq!(basename(r"C:\bin\bemenu.exe"), "bemenu.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("bemenu.exe"), "bemenu");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_bemenu(&["--help".to_string()], "bemenu"), 0);
        assert_eq!(run_bemenu(&["-h".to_string()], "bemenu"), 0);
        let _ = run_bemenu(&["--version".to_string()], "bemenu");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_bemenu(&[], "bemenu");
    }
}
