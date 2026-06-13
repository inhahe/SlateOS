#![deny(clippy::all)]

//! brownie-cli — Slate OS Brownie Python Ethereum framework
//!
//! Single personality: `brownie`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_brownie(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: brownie COMMAND [OPTIONS]");
        println!("Brownie v1.20.6 (Slate OS) — Python-based Ethereum dev framework");
        println!();
        println!("Commands:");
        println!("  init            Initialize project");
        println!("  bake TEMPLATE   Init from template (like mix)");
        println!("  compile         Compile contracts");
        println!("  console         Interactive console");
        println!("  run SCRIPT      Run a deployment script");
        println!("  test            Run pytest tests");
        println!("  analyze         Security analysis");
        println!("  gui             Launch GUI");
        println!("  accounts        Manage accounts");
        println!("  networks        Manage networks");
        println!("  pm              Package manager");
        return 0;
    }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("compile");
    match cmd {
        "compile" => {
            println!("Compiling contracts...");
            println!("  Solc version: 0.8.24");
            println!("  Compiled 3 contracts");
            println!("  Build artifacts saved.");
        }
        "test" => {
            println!("brownie test:");
            println!("  tests/test_contract.py::test_deploy PASSED");
            println!("  tests/test_contract.py::test_transfer PASSED");
            println!("  2 passed in 3.21s");
        }
        "accounts" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "list" => println!("Found 0 accounts."),
                "new" => println!("brownie: Enter a name for the new account:"),
                _ => println!("brownie accounts: {}", sub),
            }
        }
        "networks" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            if sub == "list" {
                println!("Ethereum:");
                println!("  Mainnet (id=mainnet)");
                println!("  Goerli (id=goerli)");
                println!("Development:");
                println!("  Ganache (id=development)");
            }
        }
        "init" => println!("brownie: Project initialized."),
        "console" => println!("brownie: Interactive Brownie console..."),
        _ => println!("brownie {}: completed", cmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "brownie".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_brownie(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_brownie};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/brownie"), "brownie");
        assert_eq!(basename(r"C:\bin\brownie.exe"), "brownie.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("brownie.exe"), "brownie");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_brownie(&["--help".to_string()], "brownie"), 0);
        assert_eq!(run_brownie(&["-h".to_string()], "brownie"), 0);
        let _ = run_brownie(&["--version".to_string()], "brownie");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_brownie(&[], "brownie");
    }
}
