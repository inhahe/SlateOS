#![deny(clippy::all)]

//! sui-cli — Slate OS Sui Move blockchain tool
//!
//! Single personality: `sui`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_sui(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: sui COMMAND [OPTIONS]");
        println!("sui 1.25.0 (Slate OS) — Sui blockchain CLI");
        println!();
        println!("Commands:");
        println!("  client          Client commands");
        println!("  console         Interactive console");
        println!("  genesis         Bootstrap genesis");
        println!("  keytool         Key management");
        println!("  move            Move build/test");
        println!("  network         Start local network");
        println!("  validator       Validator commands");
        println!("  fire-drill      Fire-drill exercises");
        println!("  start           Start Sui network");
        println!();
        println!("Options:");
        println!("  -V, --version   Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("sui 1.25.0 (Slate OS)");
        return 0;
    }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("client");
    match cmd {
        "client" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("help");
            match sub {
                "gas" => {
                    println!("╭──────────────────────────────────────╮");
                    println!("│ gasCoinId │ gasBalance               │");
                    println!("├──────────────────────────────────────┤");
                    println!("│ 0xabc...  │ 1000000000               │");
                    println!("╰──────────────────────────────────────╯");
                }
                "objects" => {
                    println!("╭──────────────────────────────────────╮");
                    println!("│ objectId  │ objectType │ version     │");
                    println!("├──────────────────────────────────────┤");
                    println!("│ 0xdef...  │ Coin<SUI>  │ 42          │");
                    println!("╰──────────────────────────────────────╯");
                }
                "publish" => println!("sui: Publishing package..."),
                "call" => println!("sui: Calling Move function..."),
                "transfer" => println!("sui: Transferring object..."),
                "envs" => {
                    println!("╭─────────────────────────────────────────╮");
                    println!("│ alias   │ url                │ active   │");
                    println!("├─────────────────────────────────────────┤");
                    println!("│ devnet  │ https://devnet...   │ *        │");
                    println!("│ testnet │ https://testnet...  │          │");
                    println!("│ mainnet │ https://mainnet...  │          │");
                    println!("╰─────────────────────────────────────────╯");
                }
                _ => println!("sui client: {}", sub),
            }
        }
        "move" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("build");
            match sub {
                "build" => {
                    println!("BUILDING package...");
                    println!("Successfully built package.");
                }
                "test" => {
                    println!("BUILDING package...");
                    println!("Running Move unit tests...");
                    println!("[ PASS    ] 0x0::my_module::test_init");
                    println!("[ PASS    ] 0x0::my_module::test_transfer");
                    println!("Test result: OK. Total tests: 2; passed: 2; failed: 0");
                }
                "new" => {
                    let name = args.get(2).map(|s| s.as_str()).unwrap_or("my_project");
                    println!("Created new Move project: {}", name);
                }
                _ => println!("sui move: {}", sub),
            }
        }
        "keytool" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "list" => {
                    println!("╭───────────────────────────────────────────────╮");
                    println!("│ suiAddress          │ scheme  │ flag          │");
                    println!("├───────────────────────────────────────────────┤");
                    println!("│ 0x1234...abcd       │ ed25519 │ *             │");
                    println!("╰───────────────────────────────────────────────╯");
                }
                "generate" => {
                    println!("Generated new keypair for address: 0xnew...");
                    println!("Secret Recovery Phrase: [word1 word2 ... word12]");
                }
                "import" => println!("sui keytool: Enter mnemonic phrase:"),
                _ => println!("sui keytool: {}", sub),
            }
        }
        "genesis" => println!("sui: Generating genesis..."),
        "network" => println!("sui: Starting local network on 127.0.0.1:9000..."),
        "validator" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("info");
            match sub {
                "info" => println!("Validator info: address=0xval..., stake=1000000 SUI"),
                "make-validator" => println!("sui: Creating validator metadata..."),
                _ => println!("sui validator: {}", sub),
            }
        }
        "start" => println!("sui: Starting Sui fullnode..."),
        _ => println!("sui {}: completed", cmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "sui".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_sui(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_sui};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/sui"), "sui");
        assert_eq!(basename(r"C:\bin\sui.exe"), "sui.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("sui.exe"), "sui");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_sui(&["--help".to_string()], "sui"), 0);
        assert_eq!(run_sui(&["-h".to_string()], "sui"), 0);
        let _ = run_sui(&["--version".to_string()], "sui");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_sui(&[], "sui");
    }
}
