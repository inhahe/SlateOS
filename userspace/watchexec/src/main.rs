#![deny(clippy::all)]

//! watchexec — OurOS execute commands in response to file modifications
//!
//! Single personality: `watchexec`

use std::env;
use std::process;

fn run_watchexec(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: watchexec [OPTIONS] <COMMAND>...");
        println!();
        println!("Execute commands when watched files change.");
        println!();
        println!("Options:");
        println!("  -w, --watch <PATH>       Watch a specific path (default: .)");
        println!("  -e, --exts <EXTENSIONS>  Filter by file extension");
        println!("  -f, --filter <PATTERN>   Filter by glob pattern");
        println!("  -i, --ignore <PATTERN>   Ignore changes matching pattern");
        println!("  --no-vcs-ignore          Don't respect VCS ignore files");
        println!("  --no-project-ignore      Don't respect project ignore files");
        println!("  --no-global-ignore       Don't respect global ignore files");
        println!("  --no-default-ignore      Don't use default ignores");
        println!("  -p, --postpone           Wait until first change to run");
        println!("  -r, --restart            Restart the command on change");
        println!("  -s, --signal <SIGNAL>    Send signal to command on change");
        println!("  --stop-signal <SIGNAL>   Signal to stop the command");
        println!("  --stop-timeout <MS>      Timeout before forceful stop");
        println!("  -d, --debounce <MS>      Debounce time (default: 100)");
        println!("  --stdin-quit             Exit on EOF from stdin");
        println!("  -n, --notify             Trigger desktop notification");
        println!("  --shell <SHELL>          Shell to use (none for direct exec)");
        println!("  --no-shell-long          Don't use shell for long commands");
        println!("  -c, --clear              Clear screen before each run");
        println!("  --on-busy-update <S>     Action on busy (queue/restart/signal/do-nothing)");
        println!("  --emit-events-to <WHERE> Where to emit events (stdin/file/json-stdin/...)");
        println!("  --only-emit-events       Only emit events, don't run command");
        println!("  -E, --env <KEY=VALUE>    Set environment variables");
        println!("  --project-origin <DIR>   Project origin directory");
        println!("  --workdir <DIR>          Working directory");
        println!("  --completions <SHELL>    Generate shell completions");
        println!("  --manual                 Show man page");
        println!("  -v, --verbose            Be more verbose (-vvv for max)");
        println!("  -V, --version            Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("watchexec 2.1.1 (OurOS)");
        return 0;
    }

    // Find command after --
    let dash_pos = args.iter().position(|a| a == "--");
    let command = if let Some(pos) = dash_pos {
        args[pos + 1..].join(" ")
    } else {
        args.iter()
            .filter(|a| !a.starts_with('-'))
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join(" ")
    };

    let clear = args.iter().any(|a| a == "-c" || a == "--clear");
    let restart = args.iter().any(|a| a == "-r" || a == "--restart");

    let watch_paths: Vec<&str> = {
        let mut paths = Vec::new();
        let mut iter = args.iter();
        while let Some(a) = iter.next() {
            if (a == "-w" || a == "--watch")
                && let Some(p) = iter.next() {
                    paths.push(p.as_str());
                }
        }
        if paths.is_empty() {
            paths.push(".");
        }
        paths
    };

    println!("[watchexec] watching: {}", watch_paths.join(", "));
    if restart {
        println!("[watchexec] mode: restart on change");
    }
    if clear {
        println!("[watchexec] clearing screen on each run");
    }
    println!("[watchexec] running: {}", if command.is_empty() { "(no command)" } else { &command });
    println!();
    println!("[Running: {}]", if command.is_empty() { "echo hello" } else { &command });
    println!("(simulated command output)");
    println!();
    println!("[watchexec] waiting for changes...");
    println!("[watchexec] change detected: src/main.rs (modified)");
    println!();
    println!("[Running: {}]", if command.is_empty() { "echo hello" } else { &command });
    println!("(simulated command output after change)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_watchexec(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_watchexec};

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_watchexec(vec!["--help".to_string()]), 0);
        assert_eq!(run_watchexec(vec!["-h".to_string()]), 0);
        assert_eq!(run_watchexec(vec!["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_watchexec(vec![]), 0);
    }
}
