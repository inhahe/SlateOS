#![deny(clippy::all)]

//! centreon-cli — OurOS Centreon IT monitoring
//!
//! Single personality: `centreon`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_centreon(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: centreon [COMMAND] [OPTIONS]");
        println!("Centreon v24.04 (OurOS) — IT infrastructure monitoring");
        println!();
        println!("Commands:");
        println!("  host list|add|del    Manage hosts");
        println!("  service list|add     Manage services");
        println!("  hostgroup list|add   Manage host groups");
        println!("  poller list|reload   Manage pollers");
        println!("  config generate      Generate engine config");
        println!("  config deploy        Deploy configuration");
        println!("  downtime add|list    Manage downtimes");
        println!();
        println!("Options:");
        println!("  --url URL          Centreon API URL");
        println!("  --username USER    API username");
        println!("  --password PASS    API password");
        println!("  --output json|csv  Output format");
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Centreon v24.04.2 (OurOS)"); return 0; }
    println!("Centreon v24.04.2 (OurOS)");
    println!("  Pollers: 3 (all running)");
    println!("  Hosts: 345 (330 up, 15 down)");
    println!("  Services: 5,678 (5,234 ok, 234 warning, 210 critical)");
    println!("  Host groups: 12");
    println!("  Active downtimes: 8");
    println!("  Last config deploy: 2h ago");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "centreon".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_centreon(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_centreon};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/centreon"), "centreon");
        assert_eq!(basename(r"C:\bin\centreon.exe"), "centreon.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("centreon.exe"), "centreon");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_centreon(&["--help".to_string()], "centreon"), 0);
        assert_eq!(run_centreon(&["-h".to_string()], "centreon"), 0);
        assert_eq!(run_centreon(&["--version".to_string()], "centreon"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_centreon(&[], "centreon"), 0);
    }
}
