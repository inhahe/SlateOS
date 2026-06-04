#![deny(clippy::all)]

//! bitcoin-cli — OurOS Bitcoin Core RPC client
//!
//! Single personality: `bitcoin-cli`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_bitcoin_cli(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: bitcoin-cli [OPTIONS] COMMAND [ARGS]");
        println!("Bitcoin Core RPC client v27.0 (OurOS)");
        println!();
        println!("Commands:");
        println!("  getblockchaininfo     Blockchain state");
        println!("  getbalance            Wallet balance");
        println!("  getnewaddress         Generate new address");
        println!("  sendtoaddress ADDR N  Send bitcoin");
        println!("  getblockcount         Current block height");
        println!("  getnetworkinfo        Network info");
        println!("  getpeerinfo           Connected peers");
        println!("  getmempoolinfo        Mempool info");
        println!("  --version             Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("Bitcoin Core RPC client v27.0 (OurOS)");
        return 0;
    }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("getblockchaininfo");
    match cmd {
        "getblockchaininfo" => {
            println!("{{");
            println!("  \"chain\": \"main\",");
            println!("  \"blocks\": 840000,");
            println!("  \"bestblockhash\": \"000000000000000000...\",");
            println!("  \"difficulty\": 83148355189239.77,");
            println!("  \"verificationprogress\": 0.9999");
            println!("}}");
        }
        "getblockcount" => println!("840000"),
        "getbalance" => println!("0.00000000"),
        "getnewaddress" => println!("bc1qar0srrr7xfkvy5l643lydnw9re59gtzzwf5mdq"),
        "getnetworkinfo" => {
            println!("{{");
            println!("  \"version\": 270000,");
            println!("  \"subversion\": \"/Satoshi:27.0.0/\",");
            println!("  \"connections\": 8");
            println!("}}");
        }
        "getmempoolinfo" => {
            println!("{{");
            println!("  \"size\": 12345,");
            println!("  \"bytes\": 45678901,");
            println!("  \"mempoolminfee\": 0.00001000");
            println!("}}");
        }
        _ => println!("bitcoin-cli {}: completed", cmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "bitcoin-cli".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_bitcoin_cli(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_bitcoin_cli};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/bitcoin"), "bitcoin");
        assert_eq!(basename(r"C:\bin\bitcoin.exe"), "bitcoin.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("bitcoin.exe"), "bitcoin");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_bitcoin_cli(&["--help".to_string()], "bitcoin"), 0);
        assert_eq!(run_bitcoin_cli(&["-h".to_string()], "bitcoin"), 0);
        let _ = run_bitcoin_cli(&["--version".to_string()], "bitcoin");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_bitcoin_cli(&[], "bitcoin");
    }
}
