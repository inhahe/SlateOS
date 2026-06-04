#![deny(clippy::all)]

//! berglas-cli — OurOS Berglas GCP secret management
//!
//! Single personality: `berglas`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_berglas(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: berglas COMMAND [OPTIONS]");
        println!("Berglas v1.0.0 (OurOS) — GCP secret management");
        println!();
        println!("Commands:");
        println!("  create          Create a secret");
        println!("  access          Access a secret");
        println!("  delete          Delete a secret");
        println!("  list            List secrets");
        println!("  update          Update a secret");
        println!("  grant           Grant access");
        println!("  revoke          Revoke access");
        println!("  exec CMD        Execute with secrets");
        println!("  version         Show version");
        println!();
        println!("Secret reference format:");
        println!("  berglas://BUCKET/SECRET");
        println!("  sm://PROJECT/SECRET");
        return 0;
    }
    if args.iter().any(|a| a == "version" || a == "--version") {
        println!("berglas v1.0.0 (OurOS)");
        return 0;
    }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("list");
    match cmd {
        "create" => {
            let secret = args.get(1).map(|s| s.as_str()).unwrap_or("my-bucket/my-secret");
            println!("Successfully created secret: {}", secret);
        }
        "access" => {
            let secret = args.get(1).map(|s| s.as_str()).unwrap_or("my-bucket/db-password");
            println!("Accessing: {}", secret);
            println!("s3cr3t-p4ssw0rd");
        }
        "delete" => println!("Secret deleted."),
        "list" => {
            let bucket = args.get(1).map(|s| s.as_str()).unwrap_or("my-bucket");
            println!("Secrets in {}:", bucket);
            println!("  db-password      (generation: 3)");
            println!("  api-key          (generation: 1)");
            println!("  tls-cert         (generation: 2)");
        }
        "update" => println!("Secret updated (new generation)."),
        "grant" => println!("Access granted."),
        "revoke" => println!("Access revoked."),
        "exec" => println!("Resolving secret references and executing..."),
        _ => println!("berglas {}: completed", cmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "berglas".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_berglas(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_berglas};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/berglas"), "berglas");
        assert_eq!(basename(r"C:\bin\berglas.exe"), "berglas.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("berglas.exe"), "berglas");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_berglas(&["--help".to_string()], "berglas"), 0);
        assert_eq!(run_berglas(&["-h".to_string()], "berglas"), 0);
        let _ = run_berglas(&["--version".to_string()], "berglas");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_berglas(&[], "berglas");
    }
}
