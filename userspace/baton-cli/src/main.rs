#![deny(clippy::all)]

//! baton-cli — OurOS Baton identity connector
//!
//! Single personality: `baton`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_baton(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: baton [COMMAND] [OPTIONS]");
        println!("Baton v0.9 (OurOS) — Identity & access connector");
        println!();
        println!("Commands:");
        println!("  sync               Sync identity data");
        println!("  resources list     List resources");
        println!("  entitlements list  List entitlements");
        println!("  grants list        List grants");
        println!("  revoke GRANT       Revoke a grant");
        println!("  export             Export to C1Z format");
        println!();
        println!("Options:");
        println!("  --connector NAME   Connector to use");
        println!("  --config FILE      Config file");
        println!("  --output FILE      Output file");
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Baton v0.9.2 (OurOS)"); return 0; }
    println!("Baton v0.9.2 (OurOS)");
    println!("  Connectors: 8 available");
    println!("  Resources: 234 synced");
    println!("  Entitlements: 56");
    println!("  Grants: 1,234");
    println!("  Last sync: 2h ago");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "baton".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_baton(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_baton};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/baton"), "baton");
        assert_eq!(basename(r"C:\bin\baton.exe"), "baton.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("baton.exe"), "baton");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_baton(&["--help".to_string()], "baton"), 0);
        assert_eq!(run_baton(&["-h".to_string()], "baton"), 0);
        let _ = run_baton(&["--version".to_string()], "baton");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_baton(&[], "baton");
    }
}
