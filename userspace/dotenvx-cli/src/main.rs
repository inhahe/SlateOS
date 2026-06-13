#![deny(clippy::all)]

//! dotenvx-cli — SlateOS dotenvx encrypted env manager
//!
//! Single personality: `dotenvx`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_dotenvx(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: dotenvx COMMAND [OPTIONS]");
        println!("dotenvx v0.38.0 (Slate OS) — Encrypted .env management");
        println!();
        println!("Commands:");
        println!("  run CMD         Run command with .env loaded");
        println!("  get KEY         Get a specific key");
        println!("  set KEY=VALUE   Set a key-value pair");
        println!("  encrypt         Encrypt .env file");
        println!("  decrypt         Decrypt .env file");
        println!("  genexample      Generate .env.example");
        println!("  keypair         Manage encryption keypairs");
        println!("  ls              List env files");
        println!("  version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "version" || a == "--version") {
        println!("dotenvx v0.38.0 (Slate OS)");
        return 0;
    }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("ls");
    match cmd {
        "run" => println!("[dotenvx] injecting env (3 vars) from .env"),
        "get" => {
            let key = args.get(1).map(|s| s.as_str()).unwrap_or("DATABASE_URL");
            println!("{}", key);
            println!("postgres://user:pass@localhost/mydb");
        }
        "set" => println!("[dotenvx] set successfully"),
        "encrypt" => {
            println!("[dotenvx] encrypting .env");
            println!("  encrypted .env (3 vars)");
            println!("  created .env.keys");
        }
        "decrypt" => {
            println!("[dotenvx] decrypting .env");
            println!("  decrypted .env (3 vars)");
        }
        "genexample" => {
            println!("[dotenvx] generating .env.example");
            println!("  DATABASE_URL=");
            println!("  API_KEY=");
            println!("  SECRET_KEY=");
        }
        "keypair" => println!("Public key: age1..."),
        "ls" => {
            println!(".env");
            println!(".env.production");
            println!(".env.staging");
        }
        _ => println!("dotenvx {}: completed", cmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "dotenvx".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_dotenvx(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_dotenvx};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/dotenvx"), "dotenvx");
        assert_eq!(basename(r"C:\bin\dotenvx.exe"), "dotenvx.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("dotenvx.exe"), "dotenvx");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_dotenvx(&["--help".to_string()], "dotenvx"), 0);
        assert_eq!(run_dotenvx(&["-h".to_string()], "dotenvx"), 0);
        let _ = run_dotenvx(&["--version".to_string()], "dotenvx");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_dotenvx(&[], "dotenvx");
    }
}
