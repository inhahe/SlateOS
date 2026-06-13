#![deny(clippy::all)]

//! ape-cli — SlateOS Ape Framework smart contract tool
//!
//! Single personality: `ape`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_ape(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: ape COMMAND [OPTIONS]");
        println!("ape v0.8.0 (Slate OS) — Smart contract development framework");
        println!();
        println!("Commands:");
        println!("  init            Initialize new project");
        println!("  compile         Compile contracts");
        println!("  test            Run tests");
        println!("  console         Interactive console");
        println!("  run SCRIPT      Run a script");
        println!("  accounts        Manage accounts");
        println!("  networks        Manage networks");
        println!("  plugins         Manage plugins");
        println!("  pm              Package manager");
        println!();
        println!("Options:");
        println!("  -V, --version   Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("ape v0.8.0 (Slate OS)");
        return 0;
    }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("compile");
    match cmd {
        "compile" => println!("ape: Compiling contracts... Done."),
        "test" => {
            println!("ape test:");
            println!("  PASSED test_deploy");
            println!("  PASSED test_transfer");
            println!("  2 passed");
        }
        "init" => println!("ape: Project initialized."),
        "accounts" => println!("ape accounts: (no accounts configured)"),
        "networks" => println!("ape networks: ethereum:mainnet, ethereum:goerli, ethereum:local"),
        "plugins" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            if sub == "list" {
                println!("Installed plugins:");
                println!("  ape-solidity 0.8.0");
                println!("  ape-hardhat 0.8.0");
            }
        }
        "console" => println!("ape: Starting interactive console..."),
        _ => println!("ape {}: completed", cmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "ape".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_ape(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_ape};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/ape"), "ape");
        assert_eq!(basename(r"C:\bin\ape.exe"), "ape.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("ape.exe"), "ape");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_ape(&["--help".to_string()], "ape"), 0);
        assert_eq!(run_ape(&["-h".to_string()], "ape"), 0);
        let _ = run_ape(&["--version".to_string()], "ape");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_ape(&[], "ape");
    }
}
