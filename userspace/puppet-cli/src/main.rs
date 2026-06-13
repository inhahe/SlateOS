#![deny(clippy::all)]

//! puppet-cli — Slate OS Puppet configuration management
//!
//! Multi-personality: `puppet`, `facter`, `hiera`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_puppet(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: puppet <subcommand> [options] [<action>]");
        println!();
        println!("puppet — configuration management (Slate OS).");
        println!();
        println!("Subcommands:");
        println!("  agent          Run puppet agent");
        println!("  apply          Apply a manifest");
        println!("  resource       Manage resources");
        println!("  module         Manage modules");
        println!("  config         Manage config");
        println!("  parser         Validate manifests");
        println!("  facts          Show facts");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("puppet 8.4.0 (Slate OS)");
        return 0;
    }

    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("agent");
    match subcmd {
        "agent" => {
            println!("Info: Using environment 'production'");
            println!("Info: Retrieving pluginfacts");
            println!("Info: Retrieving plugin");
            println!("Info: Caching catalog for slateos-desktop.local");
            println!("Info: Applying configuration version '1716364800'");
            println!("Notice: Applied catalog in 3.45 seconds");
        }
        "apply" => {
            let manifest = args.get(1).map(|s| s.as_str()).unwrap_or("site.pp");
            println!("Notice: Compiled catalog for slateos-desktop.local in environment production");
            println!("Info: Applying configuration from '{}' version '1716364800'", manifest);
            println!("Notice: /Stage[main]/Main/Package[nginx]/ensure: created");
            println!("Notice: /Stage[main]/Main/Service[nginx]/ensure: changed 'stopped' to 'running'");
            println!("Notice: Applied catalog in 5.67 seconds");
        }
        "module" => {
            let cmd = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            if cmd == "list" {
                println!("/etc/puppet/modules");
                println!("├── puppetlabs-apt (v9.1.0)");
                println!("├── puppetlabs-concat (v9.0.1)");
                println!("├── puppetlabs-stdlib (v9.4.1)");
                println!("└── puppetlabs-nginx (v4.0.0)");
            } else {
                println!("puppet module {} completed", cmd);
            }
        }
        "facts" => {
            println!("hostname => slateos-desktop");
            println!("fqdn => slateos-desktop.local");
            println!("operatingsystem => Slate OS");
            println!("osfamily => Slate OS");
            println!("kernel => slateos");
            println!("architecture => x86_64");
            println!("memorysize => 16.00 GB");
            println!("processorcount => 8");
        }
        "parser" => {
            let cmd = args.get(1).map(|s| s.as_str()).unwrap_or("validate");
            if cmd == "validate" {
                let file = args.get(2).map(|s| s.as_str()).unwrap_or("site.pp");
                println!("Notice: Parsed '{}' with no errors", file);
            } else {
                println!("puppet parser {} completed", cmd);
            }
        }
        _ => println!("puppet: subcommand '{}' completed", subcmd),
    }
    0
}

fn run_facter(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: facter [OPTIONS] [QUERY]");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("4.6.1 (Slate OS)");
        return 0;
    }

    let query = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str());
    if let Some(fact) = query {
        match fact {
            "os" => println!("{{name => Slate OS, family => Slate OS, release => {{major => 1, full => 1.0}}}}"),
            "networking" => println!("{{hostname => slateos-desktop, ip => 192.168.1.100, mac => 00:11:22:33:44:55}}"),
            "memory" => println!("{{system => {{total => 16.00 GiB, used => 8.50 GiB, available => 7.50 GiB}}}}"),
            _ => println!("{} => (not found)", fact),
        }
    } else {
        println!("os.name => Slate OS");
        println!("os.family => Slate OS");
        println!("os.release.full => 1.0");
        println!("networking.hostname => slateos-desktop");
        println!("networking.ip => 192.168.1.100");
        println!("processors.count => 8");
        println!("memory.system.total => 16.00 GiB");
        println!("kernel => slateos");
        println!("architecture => x86_64");
    }
    0
}

fn run_hiera(args: &[String]) -> i32 {
    if args.is_empty() {
        println!("Usage: hiera [OPTIONS] <key>");
        return 0;
    }
    let key = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("ntp::servers");
    println!("{} => [\"0.pool.ntp.org\", \"1.pool.ntp.org\"]", key);
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "puppet".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "facter" => run_facter(&rest),
        "hiera" => run_hiera(&rest),
        _ => run_puppet(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_puppet};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/puppet"), "puppet");
        assert_eq!(basename(r"C:\bin\puppet.exe"), "puppet.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("puppet.exe"), "puppet");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_puppet(&["--help".to_string()]), 0);
        assert_eq!(run_puppet(&["-h".to_string()]), 0);
        let _ = run_puppet(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_puppet(&[]);
    }
}
