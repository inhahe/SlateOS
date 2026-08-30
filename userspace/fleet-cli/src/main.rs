#![deny(clippy::all)]

//! fleet-cli — Slate OS Fleet device management
//!
//! Single personality: `fleetctl`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_fleetctl(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: fleetctl COMMAND [OPTIONS]");
        println!("fleetctl v4.40 (Slate OS) — Fleet device management CLI");
        println!();
        println!("Commands:");
        println!("  hosts list        List enrolled hosts");
        println!("  query run         Run a live query");
        println!("  policies list     List policies");
        println!("  apply FILE        Apply configuration");
        println!("  get packs         Get query packs");
        println!("  login             Login to Fleet server");
        println!("  version           Show version");
        return 0;
    }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("version");
    match cmd {
        "hosts" => {
            println!("ID    Hostname        Platform     Status");
            println!("1     web-01          slateos        online");
            println!("2     db-01           slateos        online");
            println!("3     dev-laptop      slateos        offline");
        }
        "query" => {
            println!("Running live query...");
            println!("  Targets: 3 hosts");
            println!("  Responded: 2");
            println!("  Results: 142 rows");
        }
        "policies" => {
            println!("ID    Name                     Status");
            println!("1     Disk encryption          passing (2/3)");
            println!("2     Firewall enabled         passing (3/3)");
            println!("3     Updates current           failing (1/3)");
        }
        "apply" => {
            let file = args.get(1).map(|s| s.as_str()).unwrap_or("config.yml");
            println!("Applied: {}", file);
        }
        "version" | "--version" => println!("fleetctl v4.40 (Slate OS)"),
        _ => println!("fleetctl {}: completed", cmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "fleetctl".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_fleetctl(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_fleetctl};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/fleet"), "fleet");
        assert_eq!(basename(r"C:\bin\fleet.exe"), "fleet.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("fleet.exe"), "fleet");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_fleetctl(&["--help".to_string()], "fleet"), 0);
        assert_eq!(run_fleetctl(&["-h".to_string()], "fleet"), 0);
        let _ = run_fleetctl(&["--version".to_string()], "fleet");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_fleetctl(&[], "fleet");
    }
}
