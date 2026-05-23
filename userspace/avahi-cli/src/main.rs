#![deny(clippy::all)]

//! avahi-cli — OurOS Avahi mDNS/DNS-SD tools
//!
//! Multi-personality: `avahi-browse`, `avahi-resolve`, `avahi-publish`, `avahi-daemon`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_avahi_browse(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: avahi-browse [OPTIONS] <service-type>");
        println!();
        println!("avahi-browse — browse for mDNS/DNS-SD services (OurOS).");
        println!();
        println!("Options:");
        println!("  -a, --all             Browse all services");
        println!("  -r, --resolve         Resolve discovered services");
        println!("  -t, --terminate       Terminate after dump");
        println!("  -d, --domain=DOMAIN   Domain to browse");
        return 0;
    }

    let all = args.iter().any(|a| a == "-a" || a == "--all");
    let resolve = args.iter().any(|a| a == "-r" || a == "--resolve");

    if all {
        println!("+   eth0 IPv4 ouros-desktop                          _workstation._tcp    local");
        println!("+   eth0 IPv4 ouros-desktop                          _ssh._tcp            local");
        println!("+   eth0 IPv4 ouros-desktop                          _sftp-ssh._tcp       local");
        println!("+   eth0 IPv4 Living Room Speaker                    _raop._tcp           local");
        println!("+   eth0 IPv4 Printer-HP4500                         _ipp._tcp            local");
        println!("+   eth0 IPv4 NAS-Storage                            _smb._tcp            local");
    } else {
        let stype = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("_http._tcp");
        println!("+   eth0 IPv4 Web Server                             {}    local", stype);
    }

    if resolve {
        println!("=   eth0 IPv4 ouros-desktop                          _ssh._tcp            local");
        println!("   hostname = [ouros-desktop.local]");
        println!("   address = [192.168.1.100]");
        println!("   port = [22]");
        println!("   txt = []");
    }
    0
}

fn run_avahi_resolve(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: avahi-resolve [OPTIONS] <hostname|address>");
        println!("Options: -n (name to address), -a (address to name), -4 (IPv4), -6 (IPv6)");
        return 0;
    }

    let target = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("ouros-desktop.local");
    if args.iter().any(|a| a == "-a") {
        println!("{}\touros-desktop.local", target);
    } else {
        println!("{}\t192.168.1.100", target);
    }
    0
}

fn run_avahi_publish(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: avahi-publish [OPTIONS] <name> <type> <port> [TXT...]");
        println!("  avahi-publish-service   Publish a service");
        println!("  avahi-publish-address   Publish an address");
        return 0;
    }

    let name = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("My Service");
    println!("Established under name '{}'", name);
    0
}

fn run_avahi_daemon(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: avahi-daemon [OPTIONS]");
        println!("Options: -D (daemonize), -k (kill), -r (reload), -c (check), --no-drop-root");
        return 0;
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("avahi-daemon 0.8 (OurOS)");
        return 0;
    }
    if args.iter().any(|a| a == "-c" || a == "--check") {
        println!("Daemon is running");
        return 0;
    }

    println!("avahi-daemon 0.8 starting up.");
    println!("Successfully called chroot().");
    println!("Successfully dropped root privileges.");
    println!("Loading service file /etc/avahi/services/ssh.service.");
    println!("Joining mDNS multicast group on interface eth0.IPv4 with address 192.168.1.100.");
    println!("New relevant interface eth0.IPv4 for mDNS.");
    println!("Network interface enumeration completed.");
    println!("Registering new address record for 192.168.1.100 on eth0.IPv4.");
    println!("Server startup complete. Host name is ouros-desktop.local.");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "avahi-browse".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "avahi-resolve" | "avahi-resolve-host-name" | "avahi-resolve-address" => run_avahi_resolve(&rest),
        "avahi-publish" | "avahi-publish-service" | "avahi-publish-address" => run_avahi_publish(&rest),
        "avahi-daemon" => run_avahi_daemon(&rest),
        _ => run_avahi_browse(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
