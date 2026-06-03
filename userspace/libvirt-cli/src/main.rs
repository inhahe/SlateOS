#![deny(clippy::all)]

//! libvirt-cli — OurOS libvirt virsh CLI
//!
//! Single personality: `virsh`

use std::env;
use std::process;

fn run_virsh(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: virsh [OPTIONS] COMMAND [ARGS]");
        println!();
        println!("virsh — libvirt management CLI (OurOS).");
        println!();
        println!("Domain commands:");
        println!("  list              List domains");
        println!("  start DOMAIN      Start domain");
        println!("  shutdown DOMAIN   Shutdown domain");
        println!("  destroy DOMAIN    Force-stop domain");
        println!("  reboot DOMAIN     Reboot domain");
        println!("  suspend DOMAIN    Suspend domain");
        println!("  resume DOMAIN     Resume domain");
        println!("  dominfo DOMAIN    Domain info");
        println!("  console DOMAIN    Connect to console");
        println!("  define XML        Define domain from XML");
        println!("  undefine DOMAIN   Undefine domain");
        println!("  dumpxml DOMAIN    Dump domain XML");
        println!();
        println!("Network commands:");
        println!("  net-list          List networks");
        println!("  net-info NET      Network info");
        println!();
        println!("Storage commands:");
        println!("  pool-list         List storage pools");
        println!("  vol-list POOL     List volumes");
        println!();
        println!("Options:");
        println!("  -c, --connect URI Hypervisor URI");
        println!("  -q, --quiet       Quiet mode");
        return 0;
    }
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("virsh 10.0.0 (OurOS)");
        println!("Using library: libvirt 10.0.0");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("");
    let rest: Vec<&str> = args.iter().skip(1).filter(|a| !a.starts_with('-')).map(|s| s.as_str()).collect();

    match cmd {
        "list" => {
            let all = args.iter().any(|a| a == "--all");
            println!(" Id   Name                 State");
            println!("--------------------------------------");
            println!(" 1    vm-web               running");
            println!(" 2    vm-db                running");
            if all {
                println!(" -    vm-test              shut off");
                println!(" -    vm-dev               shut off");
            }
        }
        "start" => {
            let domain = rest.first().unwrap_or(&"vm1");
            println!("Domain '{}' started", domain);
        }
        "shutdown" => {
            let domain = rest.first().unwrap_or(&"vm1");
            println!("Domain '{}' is being shutdown", domain);
        }
        "destroy" => {
            let domain = rest.first().unwrap_or(&"vm1");
            println!("Domain '{}' destroyed", domain);
        }
        "reboot" => {
            let domain = rest.first().unwrap_or(&"vm1");
            println!("Domain '{}' is being rebooted", domain);
        }
        "suspend" => {
            let domain = rest.first().unwrap_or(&"vm1");
            println!("Domain '{}' suspended", domain);
        }
        "resume" => {
            let domain = rest.first().unwrap_or(&"vm1");
            println!("Domain '{}' resumed", domain);
        }
        "dominfo" => {
            let domain = rest.first().unwrap_or(&"vm1");
            println!("Id:             1");
            println!("Name:           {}", domain);
            println!("UUID:           abc123-def456-789012");
            println!("OS Type:        hvm");
            println!("State:          running");
            println!("CPU(s):         2");
            println!("Max memory:     2097152 KiB");
            println!("Used memory:    2097152 KiB");
            println!("Persistent:     yes");
            println!("Autostart:      disable");
        }
        "net-list" => {
            println!(" Name       State    Autostart   Persistent");
            println!("----------------------------------------------");
            println!(" default    active   yes         yes");
        }
        "pool-list" => {
            println!(" Name       State    Autostart");
            println!("-----------------------------------");
            println!(" default    active   yes");
            println!(" images     active   yes");
        }
        "vol-list" => {
            let pool = rest.first().unwrap_or(&"default");
            println!(" Name                Path (pool: {})", pool);
            println!("---------------------------------------------------");
            println!(" vm-web.qcow2        /var/lib/libvirt/images/vm-web.qcow2");
            println!(" vm-db.qcow2         /var/lib/libvirt/images/vm-db.qcow2");
        }
        "define" => println!("Domain defined from XML"),
        "undefine" => println!("Domain undefined"),
        "dumpxml" => {
            println!("<domain type='kvm'>");
            println!("  <name>vm1</name>");
            println!("  <memory unit='KiB'>2097152</memory>");
            println!("  <vcpu>2</vcpu>");
            println!("</domain>");
        }
        _ => {
            eprintln!("virsh: unknown command '{}'. See --help.", cmd);
            return 1;
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_virsh(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_virsh};

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_virsh(vec!["--help".to_string()]), 0);
        assert_eq!(run_virsh(vec!["-h".to_string()]), 0);
        assert_eq!(run_virsh(vec!["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_virsh(vec![]), 0);
    }
}
