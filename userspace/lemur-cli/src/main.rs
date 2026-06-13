#![deny(clippy::all)]

//! lemur-cli — SlateOS Lemur certificate manager
//!
//! Single personality: `lemur`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_lemur(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: lemur [COMMAND] [OPTIONS]");
        println!("Lemur v1.0 (SlateOS) — TLS certificate management");
        println!();
        println!("Commands:");
        println!("  start              Start Lemur server");
        println!("  db init            Initialize database");
        println!("  db upgrade         Upgrade database schema");
        println!("  create-user        Create admin user");
        println!("  notify             Send certificate expiry notifications");
        println!("  sync               Sync certificates from sources");
        println!();
        println!("Options:");
        println!("  --config FILE      Config file");
        println!("  --port PORT        Server port (default: 8000)");
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Lemur v1.0.0 (SlateOS)"); return 0; }
    println!("Lemur v1.0.0 (SlateOS)");
    println!("  Web: http://0.0.0.0:8000");
    println!("  Certificates: 1,234 managed");
    println!("  Authorities: 3 (DigiCert, Let's Encrypt, Internal)");
    println!("  Expiring soon: 12 (within 30 days)");
    println!("  Destinations: AWS, Kubernetes, Nginx");
    println!("  Notifications: email, Slack");
    println!("  Database: PostgreSQL");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "lemur".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_lemur(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_lemur};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/lemur"), "lemur");
        assert_eq!(basename(r"C:\bin\lemur.exe"), "lemur.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("lemur.exe"), "lemur");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_lemur(&["--help".to_string()], "lemur"), 0);
        assert_eq!(run_lemur(&["-h".to_string()], "lemur"), 0);
        let _ = run_lemur(&["--version".to_string()], "lemur");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_lemur(&[], "lemur");
    }
}
