#![deny(clippy::all)]

//! cargo-watch — Slate OS watches over your project's source for changes
//!
//! Single personality: `cargo-watch`

use std::env;
use std::process;

fn run_cargo_watch(args: Vec<String>) -> i32 {
    // Invoked as `cargo watch`, first arg may be "watch"
    let subargs: Vec<String> = if args.first().map(|s| s.as_str()) == Some("watch") {
        args[1..].to_vec()
    } else {
        args
    };

    if subargs.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: cargo watch [OPTIONS]");
        println!();
        println!("Watches over your Rust project's source for changes.");
        println!();
        println!("Options:");
        println!("  -x, --exec <CMD>         Cargo command to execute (default: check)");
        println!("  -s, --shell <CMD>         Shell command to execute");
        println!("  -c, --clear              Clear screen before each run");
        println!("  -q, --quiet              Suppress cargo-watch output");
        println!("  -d, --delay <SECS>       Delay between change and execution");
        println!("  -w, --watch <PATH>       Watch path (default: .)");
        println!("  -i, --ignore <PATTERN>   Ignore pattern");
        println!("  --no-gitignore           Don't use .gitignore");
        println!("  --use-shell <SHELL>      Shell to use");
        println!("  --poll <MS>              Use polling with interval");
        println!("  -B <N>                   Inject rust-backtrace=N");
        println!("  --features <FEATURES>    Features to pass to cargo");
        println!("  --why                    Show which files triggered reload");
        println!("  -V, --version            Show version");
        return 0;
    }
    if subargs.iter().any(|a| a == "-V" || a == "--version") {
        println!("cargo-watch 8.5.2 (Slate OS)");
        return 0;
    }

    // Collect -x commands
    let mut commands: Vec<String> = Vec::new();
    let mut iter = subargs.iter();
    while let Some(a) = iter.next() {
        if (a == "-x" || a == "--exec")
            && let Some(cmd) = iter.next() {
                commands.push(cmd.clone());
            }
    }
    if commands.is_empty() {
        commands.push("check".to_string());
    }

    let clear = subargs.iter().any(|a| a == "-c" || a == "--clear");
    let show_why = subargs.iter().any(|a| a == "--why");

    println!("[cargo-watch] watching for changes...");
    if clear {
        println!("[cargo-watch] clearing screen on each run");
    }

    for cmd in &commands {
        println!("[cargo-watch] running `cargo {}`", cmd);
    }
    println!();
    println!("    Checking my-project v1.0.0");
    println!("    Finished `dev` profile target(s) in 0.82s");
    println!();
    println!("[cargo-watch] waiting for changes...");

    if show_why {
        println!("[cargo-watch] change detected: src/main.rs (modified)");
    } else {
        println!("[cargo-watch] change detected, rerunning...");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_cargo_watch(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_cargo_watch};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_cargo_watch(vec!["--help".to_string()]), 0);
        assert_eq!(run_cargo_watch(vec!["-h".to_string()]), 0);
        let _ = run_cargo_watch(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_cargo_watch(vec![]);
    }
}
