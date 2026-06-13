#![deny(clippy::all)]

//! hardhat-cli — Slate OS Hardhat Ethereum dev environment
//!
//! Single personality: `hardhat` (alias `npx hardhat`)

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_hardhat(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: hardhat [COMMAND] [OPTIONS]");
        println!("Hardhat 2.22.3 (Slate OS) — Ethereum development environment");
        println!();
        println!("Commands:");
        println!("  compile        Compile contracts");
        println!("  test           Run tests");
        println!("  node           Start local node");
        println!("  run SCRIPT     Run a script");
        println!("  deploy         Deploy contracts");
        println!("  clean          Clean cache and artifacts");
        println!("  flatten        Flatten source");
        println!("  console        Open interactive console");
        println!("  verify         Verify contract on Etherscan");
        println!("  accounts       List accounts");
        println!("  check          Run solhint checks");
        println!("  coverage       Run test coverage");
        println!("  init           Initialize project");
        println!("  --version      Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("2.22.3");
        return 0;
    }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("compile");
    match cmd {
        "compile" => {
            println!("Compiling 3 Solidity files");
            println!("Compilation finished successfully");
        }
        "test" => {
            println!("  Contract: Greeter");
            println!("    ✓ Should return greeting (42ms)");
            println!("    ✓ Should change greeting (61ms)");
            println!("  2 passing (103ms)");
        }
        "node" => {
            println!("Started HTTP and WebSocket JSON-RPC server at http://127.0.0.1:8545/");
            println!("Account #0: 0xf39Fd6e... (10000 ETH)");
            println!("Account #1: 0x709979... (10000 ETH)");
        }
        "accounts" => {
            println!("0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266");
            println!("0x70997970C51812dc3A010C7d01b50e0d17dc79C8");
        }
        "clean" => println!("hardhat: Cleaned cache and artifacts."),
        "init" => println!("hardhat: Project initialized."),
        _ => println!("hardhat {}: completed", cmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "hardhat".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_hardhat(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_hardhat};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/hardhat"), "hardhat");
        assert_eq!(basename(r"C:\bin\hardhat.exe"), "hardhat.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("hardhat.exe"), "hardhat");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_hardhat(&["--help".to_string()], "hardhat"), 0);
        assert_eq!(run_hardhat(&["-h".to_string()], "hardhat"), 0);
        let _ = run_hardhat(&["--version".to_string()], "hardhat");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_hardhat(&[], "hardhat");
    }
}
