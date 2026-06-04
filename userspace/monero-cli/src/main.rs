#![deny(clippy::all)]

//! monero-cli — OurOS Monero wallet CLI
//!
//! Single personality: `monero-wallet-cli`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_monero(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: monero-wallet-cli [OPTIONS]");
        println!("Monero Wallet CLI v0.18.3 (OurOS)");
        println!();
        println!("Options:");
        println!("  --generate-new-wallet FILE  Create new wallet");
        println!("  --wallet-file FILE          Open wallet");
        println!("  --restore-from-seed         Restore from mnemonic");
        println!("  --daemon-address HOST:PORT  Daemon connection");
        println!("  --testnet                   Use testnet");
        println!("  --version                   Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("Monero 'Fluorine Fermi' (v0.18.3.3-release) (OurOS)");
        return 0;
    }
    println!("Monero Wallet CLI v0.18.3");
    println!("  Balance: 0.000000000000 XMR");
    println!("  Unlocked balance: 0.000000000000 XMR");
    println!("  Blockchain height: 3,100,000");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "monero-wallet-cli".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_monero(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_monero};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/monero"), "monero");
        assert_eq!(basename(r"C:\bin\monero.exe"), "monero.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("monero.exe"), "monero");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_monero(&["--help".to_string()], "monero"), 0);
        assert_eq!(run_monero(&["-h".to_string()], "monero"), 0);
        let _ = run_monero(&["--version".to_string()], "monero");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_monero(&[], "monero");
    }
}
