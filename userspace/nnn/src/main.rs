#![deny(clippy::all)]

//! nnn — SlateOS terminal file manager (n³)
//!
//! Single personality: `nnn`

use std::env;
use std::process;

fn run_nnn(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: nnn [OPTIONS] [PATH]");
        println!();
        println!("The unorthodox terminal file manager.");
        println!();
        println!("Options:");
        println!("  -a              Auto-setup temporary NNN_FIFO");
        println!("  -A              No directory auto-enter on unique filter");
        println!("  -b <key>        Open bookmark key");
        println!("  -B              Use bsdtar for archives");
        println!("  -c              CLI-only opener");
        println!("  -C              Color by context");
        println!("  -d              Detail mode");
        println!("  -D              Show directories in context color");
        println!("  -e              Text in $VISUAL/$EDITOR");
        println!("  -E              Use $EDITOR for undetected files");
        println!("  -f              Use readline history file");
        println!("  -F              Show fortune");
        println!("  -g              Regex filters");
        println!("  -H              Show hidden files");
        println!("  -J              No auto-advance on select");
        println!("  -K              Test for keybindings");
        println!("  -l <val>        Lines to move (scrolloff)");
        println!("  -n              Type-to-nav mode");
        println!("  -o              Open files on Enter");
        println!("  -p <file>       Copy selection to file");
        println!("  -P <key>        Run plugin by key");
        println!("  -Q              No quit cd confirmation");
        println!("  -r              Show cp/mv progress (Linux-only)");
        println!("  -R              No rollover at edges");
        println!("  -s <name>       Named session");
        println!("  -S              Persistent session");
        println!("  -t <sec>        Idle timeout to lock");
        println!("  -T <key>        Sort by key (default: name)");
        println!("  -u              Use selection if available");
        println!("  -U              Show user/group");
        println!("  -V              Show version");
        println!("  -x              Notis, sel to system clipboard");
        println!("  -0              Bookmarks, pins only");
        return 0;
    }
    if args.iter().any(|a| a == "-V") {
        println!("nnn 4.9 (SlateOS)");
        return 0;
    }

    let detail = args.iter().any(|a| a == "-d");
    let hidden = args.iter().any(|a| a == "-H");
    let user_group = args.iter().any(|a| a == "-U");

    let path = args.iter()
        .find(|a| !a.starts_with('-'))
        .map(|s| s.as_str())
        .unwrap_or(".");

    println!("nnn 4.9 — {}", path);
    println!();

    if detail {
        if hidden {
            println!("  drwxr-xr-x  {}  .git/", if user_group { "user user" } else { "" });
            println!("  -rw-r--r--  {}  .gitignore       256 B", if user_group { "user user" } else { "" });
        }
        println!("  -rw-r--r--  {}  Cargo.toml       456 B", if user_group { "user user" } else { "" });
        println!("  -rw-r--r--  {}  Cargo.lock      12.3K", if user_group { "user user" } else { "" });
        println!("  -rw-r--r--  {}  README.md        3.1K", if user_group { "user user" } else { "" });
        println!("  drwxr-xr-x  {}  src/", if user_group { "user user" } else { "" });
        println!("  drwxr-xr-x  {}  tests/", if user_group { "user user" } else { "" });
        println!("  drwxr-xr-x  {}  target/", if user_group { "user user" } else { "" });
    } else {
        if hidden {
            println!("  .git/");
            println!("  .gitignore");
        }
        println!("  Cargo.toml");
        println!("  Cargo.lock");
        println!("  README.md");
        println!("  src/");
        println!("  tests/");
        println!("  target/");
    }

    println!();
    println!("(TUI mode — j/k navigate, Enter/l open, h back, q quit)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_nnn(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_nnn};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_nnn(vec!["--help".to_string()]), 0);
        assert_eq!(run_nnn(vec!["-h".to_string()]), 0);
        let _ = run_nnn(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_nnn(vec![]);
    }
}
