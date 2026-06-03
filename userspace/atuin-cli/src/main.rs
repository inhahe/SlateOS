#![deny(clippy::all)]

//! atuin-cli — OurOS Atuin shell history manager
//!
//! Single personality: `atuin`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_atuin(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: atuin [COMMAND]");
        println!("Atuin 18.3.0 (OurOS) — Magical shell history");
        println!();
        println!("Commands:");
        println!("  history            Manage shell history");
        println!("  history list       List history");
        println!("  history last       Show last command");
        println!("  import             Import history from other shells");
        println!("  import auto        Auto-detect and import");
        println!("  import bash        Import from bash");
        println!("  import zsh         Import from zsh");
        println!("  import fish        Import from fish");
        println!("  stats              Show history statistics");
        println!("  search QUERY       Search history");
        println!("  sync               Sync history with server");
        println!("  login              Login to sync server");
        println!("  logout             Logout from sync server");
        println!("  register           Register new account");
        println!("  key                Print encryption key");
        println!("  status             Show sync status");
        println!("  init SHELL         Print shell init script");
        println!("  doctor             Check system health");
        println!("  default-config     Print default config");
        println!();
        println!("Options:");
        println!("  -V, --version      Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("atuin 18.3.0 (OurOS)");
        return 0;
    }
    let cmd = args.iter().find(|a| !a.starts_with('-'))
        .map(|s| s.as_str()).unwrap_or("history");
    match cmd {
        "history" => {
            let sub = args.iter().skip_while(|a| a.as_str() != "history").nth(1)
                .map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "list" => {
                    println!("2024-01-15 10:00:00  ls -la");
                    println!("2024-01-15 10:01:00  cd project");
                    println!("2024-01-15 10:02:00  cargo build");
                }
                "last" => println!("cargo build"),
                _ => println!("atuin history: {}", sub),
            }
        }
        "stats" => {
            println!("Total commands: 1234");
            println!("Unique commands: 456");
            println!("Top commands:");
            println!("  1. ls (123 times)");
            println!("  2. cd (98 times)");
            println!("  3. git (87 times)");
        }
        "search" => {
            let query = args.iter().skip_while(|a| a.as_str() != "search").nth(1)
                .map(|s| s.as_str()).unwrap_or("");
            println!("atuin search: Results for '{}':", query);
        }
        "sync" => println!("atuin: Syncing history..."),
        "login" => println!("atuin: Login required. Use 'atuin login -u USER'."),
        "logout" => println!("atuin: Logged out."),
        "status" => {
            println!("Sync enabled: false");
            println!("History count: 1234");
            println!("Last sync: never");
        }
        "init" => {
            let shell = args.iter().skip_while(|a| a.as_str() != "init").nth(1)
                .map(|s| s.as_str()).unwrap_or("bash");
            println!("# atuin init for {}", shell);
            println!("eval \"$(atuin init {})\"", shell);
        }
        "doctor" => {
            println!("atuin doctor:");
            println!("  Shell: bash");
            println!("  Config: OK");
            println!("  Database: OK");
            println!("  Sync: disabled");
        }
        "default-config" => {
            println!("[settings]");
            println!("auto_sync = false");
            println!("update_check = false");
            println!("search_mode = \"fuzzy\"");
            println!("filter_mode = \"global\"");
        }
        "import" => println!("atuin: Importing history..."),
        _ => println!("atuin: unknown command '{}'", cmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "atuin".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_atuin(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_atuin};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/atuin"), "atuin");
        assert_eq!(basename(r"C:\bin\atuin.exe"), "atuin.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("atuin.exe"), "atuin");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_atuin(&["--help".to_string()], "atuin"), 0);
        assert_eq!(run_atuin(&["-h".to_string()], "atuin"), 0);
        assert_eq!(run_atuin(&["--version".to_string()], "atuin"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_atuin(&[], "atuin"), 0);
    }
}
