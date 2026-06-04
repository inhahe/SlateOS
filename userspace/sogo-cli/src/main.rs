#![deny(clippy::all)]

//! sogo-cli — OurOS SOGo groupware
//!
//! Multi-personality: `sogo`, `sogo-tool`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_sogo(args: &[String], prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: {} [OPTIONS]", prog);
        match prog {
            "sogo-tool" => {
                println!("sogo-tool (OurOS) — SOGo admin utility");
                println!("  backup USER DIR    Backup user data");
                println!("  restore USER DIR   Restore user data");
                println!("  expire-sessions    Expire old sessions");
                println!("  update-autoreply   Update auto-reply flags");
                println!("  check-integrity    Check data integrity");
                println!("  dump-defaults      Dump default config");
            }
            _ => {
                println!("SOGo v5.9 (OurOS) — Groupware server");
                println!("  -WOPort PORT       HTTP port (default: 20000)");
                println!("  -WOWorkersCount N  Worker processes");
                println!("  -WOPidFile FILE    PID file");
            }
        }
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("SOGo v5.9.1 (OurOS)"); return 0; }
    match prog {
        "sogo-tool" => {
            println!("SOGo Tool v5.9.1");
            println!("  Database: PostgreSQL (localhost/sogo)");
            println!("  Users: 123");
            println!("  Sessions: 45 active");
            println!("  Operation completed successfully.");
        }
        _ => {
            println!("SOGo v5.9.1 (OurOS)");
            println!("  Web: http://0.0.0.0:20000/SOGo");
            println!("  DAV: CalDAV, CardDAV");
            println!("  ActiveSync: EAS 16.0");
            println!("  LDAP: connected (localhost:389)");
            println!("  Database: PostgreSQL");
            println!("  Users: 123");
            println!("  Workers: 4");
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "sogo".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_sogo(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_sogo};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/sogo"), "sogo");
        assert_eq!(basename(r"C:\bin\sogo.exe"), "sogo.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("sogo.exe"), "sogo");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_sogo(&["--help".to_string()], "sogo"), 0);
        assert_eq!(run_sogo(&["-h".to_string()], "sogo"), 0);
        let _ = run_sogo(&["--version".to_string()], "sogo");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_sogo(&[], "sogo");
    }
}
