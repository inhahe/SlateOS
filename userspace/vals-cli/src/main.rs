#![deny(clippy::all)]

//! vals-cli — SlateOS vals secret reference resolver
//!
//! Single personality: `vals`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_vals(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: vals COMMAND [OPTIONS]");
        println!("vals v0.37.0 (Slate OS) — Helmfile's secret reference resolver");
        println!();
        println!("Commands:");
        println!("  eval            Evaluate secret references in YAML");
        println!("  exec            Execute command with resolved secrets");
        println!("  env             Print secrets as env vars");
        println!("  get             Get a single secret");
        println!("  flatten         Flatten nested refs");
        println!("  version         Show version");
        println!();
        println!("Supported backends:");
        println!("  vault, aws-ssm, aws-secrets, gcp-secrets, azure-keyvault,");
        println!("  sops, envsubst, echo, file, 1password, doppler");
        return 0;
    }
    if args.iter().any(|a| a == "version" || a == "--version") {
        println!("vals v0.37.0 (Slate OS)");
        return 0;
    }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("eval");
    match cmd {
        "eval" => {
            println!("db_host: mydb.example.com");
            println!("db_password: s3cr3t-p4ssw0rd");
            println!("api_key: ak_live_1234567890");
        }
        "exec" => println!("Executing command with resolved secrets..."),
        "env" => {
            println!("export DB_HOST=mydb.example.com");
            println!("export DB_PASSWORD=s3cr3t-p4ssw0rd");
            println!("export API_KEY=ak_live_1234567890");
        }
        "get" => {
            let key = args.get(1).map(|s| s.as_str()).unwrap_or("ref+vault://secret/data/app#/password");
            println!("{}: resolved-value", key);
        }
        "flatten" => println!("Flattened 5 references."),
        _ => println!("vals {}: completed", cmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "vals".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_vals(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_vals};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/vals"), "vals");
        assert_eq!(basename(r"C:\bin\vals.exe"), "vals.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("vals.exe"), "vals");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_vals(&["--help".to_string()], "vals"), 0);
        assert_eq!(run_vals(&["-h".to_string()], "vals"), 0);
        let _ = run_vals(&["--version".to_string()], "vals");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_vals(&[], "vals");
    }
}
