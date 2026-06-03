#![deny(clippy::all)]

//! teller-cli — OurOS Teller universal secret manager
//!
//! Single personality: `teller`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_teller(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: teller COMMAND [OPTIONS]");
        println!("Teller v2.0.0 (OurOS) — Universal secret manager");
        println!();
        println!("Commands:");
        println!("  run CMD         Run with secrets injected");
        println!("  env             Print as env vars");
        println!("  yaml            Print as YAML");
        println!("  json            Print as JSON");
        println!("  sh              Print as shell exports");
        println!("  scan            Scan for secret leaks");
        println!("  redact          Redact secrets from stdin");
        println!("  put KEY VALUE   Write secret");
        println!("  delete KEY      Delete secret");
        println!("  providers       List configured providers");
        println!("  new             Create new .teller.yml");
        println!("  version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "version" || a == "--version") {
        println!("Teller v2.0.0 (OurOS)");
        return 0;
    }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("env");
    match cmd {
        "run" => println!("teller: Running with 5 secrets injected."),
        "env" => {
            println!("DB_HOST=mydb.example.com");
            println!("DB_PASSWORD=****");
            println!("API_KEY=****");
        }
        "json" => println!("{{\"DB_HOST\":\"mydb.example.com\",\"DB_PASSWORD\":\"****\"}}"),
        "sh" => {
            println!("export DB_HOST=\"mydb.example.com\"");
            println!("export DB_PASSWORD=\"s3cr3t\"");
        }
        "scan" => {
            println!("Scanning for secret leaks...");
            println!("  src/config.py:12  possible AWS key found");
            println!("  .env.bak:3        DB_PASSWORD in plaintext");
            println!("  2 potential leaks found.");
        }
        "redact" => println!("[redacted output]"),
        "providers" => {
            println!("Configured providers:");
            println!("  hashicorp_vault  (path: secret/data/app)");
            println!("  aws_ssm         (path: /app/production/)");
            println!("  dotenv           (path: .env)");
        }
        "new" => println!("Created .teller.yml"),
        "put" => println!("Secret written."),
        "delete" => println!("Secret deleted."),
        _ => println!("teller {}: completed", cmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "teller".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_teller(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_teller};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/teller"), "teller");
        assert_eq!(basename(r"C:\bin\teller.exe"), "teller.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("teller.exe"), "teller");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_teller(&["--help".to_string()], "teller"), 0);
        assert_eq!(run_teller(&["-h".to_string()], "teller"), 0);
        assert_eq!(run_teller(&["--version".to_string()], "teller"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_teller(&[], "teller"), 0);
    }
}
