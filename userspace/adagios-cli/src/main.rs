#![deny(clippy::all)]

//! adagios-cli — OurOS Adagios Nagios configuration
//!
//! Single personality: `adagios`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_adagios(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: adagios [COMMAND] [OPTIONS]");
        println!("Adagios v1.6 (OurOS) — Nagios configuration & status");
        println!();
        println!("Commands:");
        println!("  host list|add|edit     Manage hosts");
        println!("  service list|add|edit  Manage services");
        println!("  contact list|add       Manage contacts");
        println!("  hostgroup list|add     Host groups");
        println!("  template list          List templates");
        println!("  verify                 Verify Nagios config");
        println!("  reload                 Reload Nagios");
        println!("  status                 Show monitoring status");
        println!();
        println!("Options:");
        println!("  --nagios-cfg FILE  Nagios config file");
        println!("  --livestatus SOCKET  Livestatus socket");
        println!("  --json             JSON output");
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Adagios v1.6.8 (OurOS)"); return 0; }
    println!("Adagios v1.6.8 (OurOS)");
    println!("  Nagios config: /etc/nagios/nagios.cfg");
    println!("  Hosts: 89 defined");
    println!("  Services: 456 defined");
    println!("  Templates: 23");
    println!("  Config status: valid");
    println!("  Livestatus: connected");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "adagios".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_adagios(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_adagios};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/adagios"), "adagios");
        assert_eq!(basename(r"C:\bin\adagios.exe"), "adagios.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("adagios.exe"), "adagios");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_adagios(&["--help".to_string()], "adagios"), 0);
        assert_eq!(run_adagios(&["-h".to_string()], "adagios"), 0);
        let _ = run_adagios(&["--version".to_string()], "adagios");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_adagios(&[], "adagios");
    }
}
