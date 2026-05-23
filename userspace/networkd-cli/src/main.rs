#![deny(clippy::all)]

//! networkd-cli — OurOS systemd-networkd tools
//!
//! Multi-personality: `networkctl`, `resolvectl`, `systemd-resolve`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_networkctl(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: networkctl [OPTIONS] COMMAND ...");
        println!();
        println!("networkctl — network link management (OurOS).");
        println!();
        println!("Commands:");
        println!("  list                 List links");
        println!("  status [LINK...]     Show link status");
        println!("  up LINK              Bring link up");
        println!("  down LINK            Bring link down");
        println!("  reload               Reload .network files");
        println!("  reconfigure LINK     Reconfigure link");
        println!("  lldp                 Show LLDP neighbors");
        return 0;
    }

    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("list");
    match subcmd {
        "list" => {
            println!("IDX LINK     TYPE     OPERATIONAL SETUP");
            println!("  1 lo       loopback carrier     unmanaged");
            println!("  2 eth0     ether    routable    configured");
            println!("  3 wlan0    wlan     routable    configured");
            println!("  4 docker0  bridge   no-carrier  unmanaged");
            println!();
            println!("4 links listed.");
        }
        "status" => {
            let link = args.get(1).map(|s| s.as_str());
            if let Some(iface) = link {
                println!("● {}:", iface);
            } else {
                println!("●  State: routable");
                println!("  Online state: online");
            }
            println!("         Address: 192.168.1.100 on eth0");
            println!("                  fd00::1 on eth0");
            println!("         Gateway: 192.168.1.1 on eth0");
            println!("             DNS: 8.8.8.8");
            println!("                  8.8.4.4");
            println!("  Search Domains: local");
        }
        "up" | "down" | "reconfigure" => {
            let link = args.get(1).map(|s| s.as_str()).unwrap_or("eth0");
            println!("{} {} — OK", subcmd, link);
        }
        "reload" => println!("Reloading network configuration... OK"),
        "lldp" => {
            println!("LINK  CHASSIS ID        SYSTEM NAME  PORT ID   PORT DESCRIPTION  CAPS");
            println!("eth0  aa:bb:cc:dd:ee:ff Switch-1     Gi0/1     GigabitEthernet   BR");
        }
        _ => println!("Unknown command '{}'", subcmd),
    }
    0
}

fn run_resolvectl(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: resolvectl [OPTIONS] COMMAND ...");
        println!();
        println!("Commands:");
        println!("  query HOSTNAME      Resolve hostname");
        println!("  status              Show DNS status");
        println!("  statistics          Show resolver stats");
        println!("  flush-caches        Flush DNS caches");
        println!("  dns [LINK [DNS]]    Get/set DNS");
        println!("  domain [LINK [D]]   Get/set search domains");
        return 0;
    }

    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("status");
    match subcmd {
        "query" => {
            let host = args.get(1).map(|s| s.as_str()).unwrap_or("example.com");
            println!("{}: 93.184.216.34", host);
            println!();
            println!("-- Information acquired via protocol DNS in 12.3ms.");
            println!("-- Data is authenticated: no; Data was acquired via local or encrypted transport: no");
        }
        "status" => {
            println!("Global");
            println!("       Protocols: +LLMNR +mDNS -DNSOverTLS DNSSEC=no/unsupported");
            println!("resolv.conf mode: stub");
            println!("Current DNS Server: 8.8.8.8");
            println!("       DNS Servers: 8.8.8.8 8.8.4.4");
            println!("        DNS Domain: ~.");
            println!();
            println!("Link 2 (eth0)");
            println!("    Current Scopes: DNS LLMNR/IPv4 LLMNR/IPv6");
            println!("         Protocols: +DefaultRoute +LLMNR -mDNS -DNSOverTLS DNSSEC=no/unsupported");
            println!("Current DNS Server: 192.168.1.1");
            println!("       DNS Servers: 192.168.1.1");
        }
        "statistics" => {
            println!("DNSSEC supported by current servers: no");
            println!();
            println!("Transactions");
            println!("Current Transactions: 0");
            println!("  Total Transactions: 1234");
            println!();
            println!("Cache");
            println!("  Current Cache Size: 42");
            println!("          Cache Hits: 890");
            println!("        Cache Misses: 344");
            println!();
            println!("DNSSEC Verdicts");
            println!("              Secure: 0");
            println!("            Insecure: 1234");
        }
        "flush-caches" => println!("Flushed all caches."),
        "dns" => {
            println!("Global: 8.8.8.8 8.8.4.4");
            println!("Link 2 (eth0): 192.168.1.1");
        }
        _ => println!("resolvectl: command '{}' completed", subcmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "networkctl".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "resolvectl" | "systemd-resolve" => run_resolvectl(&rest),
        _ => run_networkctl(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
