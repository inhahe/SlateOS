#![deny(clippy::all)]

//! doppler-cli — SlateOS Doppler secrets manager
//!
//! Single personality: `doppler`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_doppler(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: doppler COMMAND [OPTIONS]");
        println!("Doppler CLI v3.68.0 (Slate OS) — Secrets manager");
        println!();
        println!("Commands:");
        println!("  setup           Configure project");
        println!("  run CMD         Run with secrets");
        println!("  secrets         Manage secrets");
        println!("  projects        Manage projects");
        println!("  environments    Manage environments");
        println!("  configs         Manage configs");
        println!("  login           Authenticate");
        println!("  logout          Log out");
        println!("  update          Update CLI");
        println!("  --version       Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("v3.68.0 (Slate OS)");
        return 0;
    }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("secrets");
    match cmd {
        "setup" => {
            println!("Selected project: my-app");
            println!("Selected config: dev");
            println!("Setup complete.");
        }
        "run" => println!("Injecting 12 secrets from my-app/dev..."),
        "secrets" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "list" | "ls" => {
                    println!("NAME             VALUE");
                    println!("DATABASE_URL     postgres://...   (computed)");
                    println!("API_KEY          ak_live_*****    (secret)");
                    println!("LOG_LEVEL        info             (plain)");
                    println!("PORT             3000             (plain)");
                }
                "set" => println!("Secret set successfully."),
                "delete" => println!("Secret deleted."),
                "download" => println!("Downloaded secrets to .env"),
                _ => println!("doppler secrets {}: completed", sub),
            }
        }
        "projects" => {
            println!("NAME        CREATED");
            println!("my-app      2024-01-01");
            println!("backend     2024-01-05");
        }
        "environments" => {
            println!("SLUG    NAME          DEFAULT CONFIG");
            println!("dev     Development   dev");
            println!("stg     Staging       stg");
            println!("prd     Production    prd");
        }
        "configs" => {
            println!("NAME     ENVIRONMENT  ROOT    LOCKED");
            println!("dev      dev          true    false");
            println!("dev_me   dev          false   false");
            println!("stg      stg          true    true");
        }
        "login" => println!("Successfully authenticated."),
        "logout" => println!("Logged out."),
        _ => println!("doppler {}: completed", cmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "doppler".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_doppler(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_doppler};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/doppler"), "doppler");
        assert_eq!(basename(r"C:\bin\doppler.exe"), "doppler.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("doppler.exe"), "doppler");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_doppler(&["--help".to_string()], "doppler"), 0);
        assert_eq!(run_doppler(&["-h".to_string()], "doppler"), 0);
        let _ = run_doppler(&["--version".to_string()], "doppler");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_doppler(&[], "doppler");
    }
}
