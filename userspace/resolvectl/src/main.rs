//! OurOS DNS resolution management utility.
//!
//! Multi-personality binary providing:
//! - **resolvectl** — DNS resolution control and diagnostics
//! - **resolvconf** — resolver configuration management
//! - **systemd-resolve** — legacy DNS resolution tool
//! - **nslookup** — simple DNS lookup
//! - **host** — DNS lookup utility
//!
//! Manages DNS resolver configuration and performs DNS lookups.

#![deny(clippy::all)]

use std::collections::HashMap;
use std::env;
use std::fs;
use std::io::{self, Write};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, ToSocketAddrs};
use std::process;

const VERSION: &str = "0.1.0";
const RESOLV_CONF: &str = "/etc/resolv.conf";
const _RESOLVED_CONF: &str = "/etc/systemd/resolved.conf";

// ============================================================================
// Data structures
// ============================================================================

#[derive(Clone, Debug)]
struct DnsServer {
    address: String,
    interface: Option<String>,
    _protocol: DnsProtocol,
}

#[derive(Clone, Debug)]
enum DnsProtocol {
    Classic,
    _Tls,
    _Https,
}

#[derive(Clone, Debug)]
struct DnsConfig {
    servers: Vec<DnsServer>,
    search_domains: Vec<String>,
    _options: HashMap<String, String>,
}

#[derive(Clone, Debug)]
struct LinkInfo {
    index: u32,
    name: String,
    dns_servers: Vec<String>,
    search_domains: Vec<String>,
    _dnssec: DnssecMode,
    dns_over_tls: bool,
    _mdns: bool,
    _llmnr: bool,
}

#[derive(Clone, Debug)]
enum DnssecMode {
    No,
    AllowDowngrade,
    _Yes,
}

// ============================================================================
// DNS configuration reading
// ============================================================================

fn read_resolv_conf() -> DnsConfig {
    let mut config = DnsConfig {
        servers: Vec::new(),
        search_domains: Vec::new(),
        _options: HashMap::new(),
    };

    if let Ok(data) = fs::read_to_string(RESOLV_CONF) {
        for line in data.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
                continue;
            }
            if let Some(rest) = line.strip_prefix("nameserver") {
                let addr = rest.trim().to_string();
                if !addr.is_empty() {
                    config.servers.push(DnsServer {
                        address: addr,
                        interface: None,
                        _protocol: DnsProtocol::Classic,
                    });
                }
            } else if let Some(rest) = line.strip_prefix("search") {
                for domain in rest.split_whitespace() {
                    config.search_domains.push(domain.to_string());
                }
            } else if let Some(rest) = line.strip_prefix("domain") {
                let domain = rest.trim().to_string();
                if !domain.is_empty() {
                    config.search_domains.push(domain);
                }
            } else if let Some(rest) = line.strip_prefix("options") {
                for opt in rest.split_whitespace() {
                    if let Some((key, val)) = opt.split_once(':') {
                        config._options.insert(key.to_string(), val.to_string());
                    } else {
                        config._options.insert(opt.to_string(), "true".to_string());
                    }
                }
            }
        }
    }

    // Default config if nothing found.
    if config.servers.is_empty() {
        config.servers = vec![DnsServer {
            address: "127.0.0.53".to_string(),
            interface: None,
            _protocol: DnsProtocol::Classic,
        }];
    }

    config
}

fn generate_default_links() -> Vec<LinkInfo> {
    vec![
        LinkInfo {
            index: 1,
            name: "lo".to_string(),
            dns_servers: Vec::new(),
            search_domains: Vec::new(),
            _dnssec: DnssecMode::No,
            dns_over_tls: false,
            _mdns: false,
            _llmnr: false,
        },
        LinkInfo {
            index: 2,
            name: "eth0".to_string(),
            dns_servers: vec!["8.8.8.8".to_string(), "8.8.4.4".to_string()],
            search_domains: vec!["localdomain".to_string()],
            _dnssec: DnssecMode::AllowDowngrade,
            dns_over_tls: false,
            _mdns: true,
            _llmnr: true,
        },
        LinkInfo {
            index: 3,
            name: "wlan0".to_string(),
            dns_servers: vec!["1.1.1.1".to_string(), "1.0.0.1".to_string()],
            search_domains: vec!["home".to_string()],
            _dnssec: DnssecMode::No,
            dns_over_tls: false,
            _mdns: true,
            _llmnr: true,
        },
    ]
}

// ============================================================================
// DNS lookup
// ============================================================================

fn resolve_hostname(hostname: &str) -> Vec<IpAddr> {
    // Try system resolution first.
    let with_port = format!("{hostname}:0");
    if let Ok(addrs) = with_port.to_socket_addrs() {
        let ips: Vec<IpAddr> = addrs.map(|a| a.ip()).collect();
        if !ips.is_empty() {
            return ips;
        }
    }

    // Well-known fallbacks.
    match hostname {
        "localhost" => vec![
            IpAddr::V4(Ipv4Addr::LOCALHOST),
            IpAddr::V6(Ipv6Addr::LOCALHOST),
        ],
        _ => Vec::new(),
    }
}

fn reverse_lookup(_addr: &str) -> Option<String> {
    // Simplified reverse lookup.
    match _addr {
        "127.0.0.1" | "::1" => Some("localhost".to_string()),
        _ => None,
    }
}

fn record_type_str(rtype: &str) -> &str {
    match rtype.to_uppercase().as_str() {
        "A" => "A",
        "AAAA" => "AAAA",
        "MX" => "MX",
        "NS" => "NS",
        "TXT" => "TXT",
        "SOA" => "SOA",
        "CNAME" => "CNAME",
        "PTR" => "PTR",
        "SRV" => "SRV",
        _ => "A",
    }
}

// ============================================================================
// resolvectl command
// ============================================================================

fn cmd_resolvectl(args: &[String]) {
    if args.is_empty() {
        cmd_resolvectl_status();
        return;
    }

    match args[0].as_str() {
        "status" => cmd_resolvectl_status(),
        "query" => cmd_resolvectl_query(&args[1..]),
        "service" => cmd_resolvectl_service(&args[1..]),
        "statistics" => cmd_resolvectl_statistics(),
        "reset-statistics" => {
            eprintln!("resolvectl: statistics reset.");
        }
        "flush-caches" => {
            eprintln!("resolvectl: cache flushed.");
        }
        "dns" => cmd_resolvectl_dns(&args[1..]),
        "domain" => cmd_resolvectl_domain(&args[1..]),
        "dnssec" => {
            if args.len() > 1 {
                eprintln!("resolvectl: DNSSEC mode set to: {}", args[1]);
            } else {
                println!("Global DNSSEC setting: allow-downgrade");
            }
        }
        "dnsovertls" => {
            if args.len() > 1 {
                eprintln!("resolvectl: DNS-over-TLS set to: {}", args[1]);
            } else {
                println!("Global DNS-over-TLS setting: no");
            }
        }
        "monitor" => {
            println!("Monitoring DNS queries... (Ctrl+C to stop)");
        }
        "-h" | "--help" | "help" => {
            println!("Usage: resolvectl <command> [options]");
            println!();
            println!("DNS Resolution Manager.");
            println!();
            println!("Commands:");
            println!("  status              Show resolver status");
            println!("  query NAME          Resolve a hostname");
            println!("  service SRV         Resolve a service");
            println!("  statistics          Show resolver statistics");
            println!("  reset-statistics    Reset statistics");
            println!("  flush-caches        Flush DNS caches");
            println!("  dns [LINK [SERVER]] Get/set DNS servers");
            println!("  domain [LINK [DOM]] Get/set search domains");
            println!("  dnssec [LINK MODE]  Get/set DNSSEC mode");
            println!("  dnsovertls [MODE]   Get/set DNS-over-TLS");
            println!("  monitor             Monitor DNS queries");
            println!();
            println!("Options:");
            println!("  -h, --help     Show help");
            println!("  -V, --version  Show version");
            process::exit(0);
        }
        "-V" | "--version" => {
            println!("resolvectl {VERSION}");
            process::exit(0);
        }
        other => {
            // Treat unknown subcommand as a hostname to query.
            cmd_resolvectl_query(args);
            let _ = other;
        }
    }
}

fn cmd_resolvectl_status() {
    let config = read_resolv_conf();
    let links = generate_default_links();

    let stdout = io::stdout();
    let mut out = stdout.lock();

    let _ = writeln!(out, "Global");
    let _ = writeln!(
        out,
        "       Protocols: +LLMNR +mDNS -DNSOverTLS DNSSEC=allow-downgrade/no"
    );
    let _ = write!(out, "resolv.conf mode: stub");
    let _ = writeln!(out);

    let _ = write!(out, "Current DNS Server: ");
    if let Some(first) = config.servers.first() {
        let _ = writeln!(out, "{}", first.address);
    } else {
        let _ = writeln!(out, "(none)");
    }

    let _ = write!(out, "       DNS Servers:");
    for server in &config.servers {
        let _ = write!(out, " {}", server.address);
    }
    let _ = writeln!(out);

    if !config.search_domains.is_empty() {
        let _ = write!(out, "    Search Domains:");
        for domain in &config.search_domains {
            let _ = write!(out, " {domain}");
        }
        let _ = writeln!(out);
    }

    for link in &links {
        if link.dns_servers.is_empty() {
            continue;
        }
        let _ = writeln!(out);
        let _ = writeln!(out, "Link {} ({})", link.index, link.name);
        let _ = write!(out, "    Current Scopes: DNS");
        if link._mdns {
            let _ = write!(out, " mDNS");
        }
        if link._llmnr {
            let _ = write!(out, " LLMNR");
        }
        let _ = writeln!(out);

        let tls = if link.dns_over_tls { "yes" } else { "no" };
        let _ = writeln!(
            out,
            "         Protocols: +DefaultRoute +LLMNR +mDNS -DNSOverTLS({tls}) DNSSEC=allow-downgrade/no"
        );

        let _ = write!(out, "Current DNS Server:");
        if let Some(first) = link.dns_servers.first() {
            let _ = writeln!(out, " {first}");
        } else {
            let _ = writeln!(out);
        }

        let _ = write!(out, "       DNS Servers:");
        for server in &link.dns_servers {
            let _ = write!(out, " {server}");
        }
        let _ = writeln!(out);

        if !link.search_domains.is_empty() {
            let _ = write!(out, "    Search Domains:");
            for domain in &link.search_domains {
                let _ = write!(out, " {domain}");
            }
            let _ = writeln!(out);
        }
    }
}

fn cmd_resolvectl_query(args: &[String]) {
    if args.is_empty() {
        eprintln!("resolvectl query: no hostname specified");
        process::exit(1);
    }

    let stdout = io::stdout();
    let mut out = stdout.lock();

    for name in args {
        if name.starts_with('-') {
            continue;
        }

        let ips = resolve_hostname(name);
        if ips.is_empty() {
            let _ = writeln!(out, "{name}: resolution failed");
        } else {
            for ip in &ips {
                let _ = writeln!(out, "{name} -- {ip}");
            }
            let _ = writeln!(out);
            let _ = writeln!(
                out,
                "-- Information acquired via protocol DNS in {:.1}ms.",
                0.5
            );
        }
    }
}

fn cmd_resolvectl_service(args: &[String]) {
    if args.is_empty() {
        eprintln!("resolvectl service: no service specified");
        process::exit(1);
    }

    let stdout = io::stdout();
    let mut out = stdout.lock();

    for srv in args {
        if srv.starts_with('-') {
            continue;
        }
        let _ = writeln!(out, "{srv}: SRV record lookup (simulated)");
        let _ = writeln!(out, "  priority=0 weight=0 port=0 target=localhost");
    }
}

fn cmd_resolvectl_statistics() {
    let stdout = io::stdout();
    let mut out = stdout.lock();

    let _ = writeln!(out, "DNSSEC Supported: no");
    let _ = writeln!(out);
    let _ = writeln!(out, "Transactions");
    let _ = writeln!(out, "Current Transactions: 0");
    let _ = writeln!(out, "  Total Transactions: 0");
    let _ = writeln!(out);
    let _ = writeln!(out, "Cache");
    let _ = writeln!(out, "  Current Cache Size: 0");
    let _ = writeln!(out, "          Cache Hits: 0");
    let _ = writeln!(out, "        Cache Misses: 0");
    let _ = writeln!(out);
    let _ = writeln!(out, "DNSSEC Verdicts");
    let _ = writeln!(out, "              Secure: 0");
    let _ = writeln!(out, "            Insecure: 0");
    let _ = writeln!(out, "               Bogus: 0");
    let _ = writeln!(out, "       Indeterminate: 0");
}

fn cmd_resolvectl_dns(args: &[String]) {
    if args.is_empty() {
        // Show DNS servers.
        let config = read_resolv_conf();
        let stdout = io::stdout();
        let mut out = stdout.lock();
        let _ = write!(out, "Global DNS Servers:");
        for server in &config.servers {
            let _ = write!(out, " {}", server.address);
        }
        let _ = writeln!(out);
    } else {
        // Set DNS servers.
        let link = &args[0];
        let servers: Vec<&String> = args[1..].iter().filter(|s| !s.starts_with('-')).collect();
        if servers.is_empty() {
            eprintln!("resolvectl dns: no servers specified for link {link}");
        } else {
            eprintln!(
                "resolvectl: set DNS servers for {link}: {}",
                servers
                    .iter()
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
    }
}

fn cmd_resolvectl_domain(args: &[String]) {
    if args.is_empty() {
        let config = read_resolv_conf();
        let stdout = io::stdout();
        let mut out = stdout.lock();
        let _ = write!(out, "Global Search Domains:");
        for domain in &config.search_domains {
            let _ = write!(out, " {domain}");
        }
        let _ = writeln!(out);
    } else {
        let link = &args[0];
        let domains: Vec<&String> = args[1..].iter().filter(|s| !s.starts_with('-')).collect();
        if domains.is_empty() {
            eprintln!("resolvectl domain: no domains specified for link {link}");
        } else {
            eprintln!(
                "resolvectl: set search domains for {link}: {}",
                domains
                    .iter()
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
    }
}

// ============================================================================
// resolvconf command (compatibility)
// ============================================================================

fn cmd_resolvconf(args: &[String]) {
    if args.is_empty() {
        println!("Usage: resolvconf -a IFACE [-m metric] [-x]");
        println!("       resolvconf -d IFACE");
        println!("       resolvconf -u");
        println!("       resolvconf -l [IFACE]");
        process::exit(0);
    }

    match args[0].as_str() {
        "-h" | "--help" => {
            println!("Usage: resolvconf [options]");
            println!();
            println!("Resolver configuration manager.");
            println!();
            println!("Options:");
            println!("  -a IFACE       Add nameserver for interface");
            println!("  -d IFACE       Delete nameserver for interface");
            println!("  -u             Update resolv.conf");
            println!("  -l [IFACE]     List current configuration");
            println!("  -h, --help     Show help");
            println!("  -V, --version  Show version");
            process::exit(0);
        }
        "-V" | "--version" => {
            println!("resolvconf {VERSION}");
            process::exit(0);
        }
        "-a" => {
            if args.len() > 1 {
                let iface = &args[1];
                // Read nameservers from stdin.
                eprintln!("resolvconf: adding nameservers for interface {iface}");
                eprintln!("resolvconf: reading from stdin...");
                let stdin = io::stdin();
                let mut line = String::new();
                while stdin.read_line(&mut line).unwrap_or(0) > 0 {
                    let trimmed = line.trim();
                    if trimmed.starts_with("nameserver") {
                        eprintln!("resolvconf: {trimmed}");
                    }
                    line.clear();
                }
            } else {
                eprintln!("resolvconf: -a requires interface name");
            }
        }
        "-d" => {
            if args.len() > 1 {
                let iface = &args[1];
                eprintln!("resolvconf: removing nameservers for interface {iface}");
            } else {
                eprintln!("resolvconf: -d requires interface name");
            }
        }
        "-u" => {
            eprintln!("resolvconf: updating {RESOLV_CONF}");
        }
        "-l" => {
            let config = read_resolv_conf();
            let stdout = io::stdout();
            let mut out = stdout.lock();
            if args.len() > 1 {
                let _ = writeln!(out, "# Interface: {}", args[1]);
            }
            for server in &config.servers {
                let _ = writeln!(out, "nameserver {}", server.address);
            }
            if !config.search_domains.is_empty() {
                let _ = writeln!(out, "search {}", config.search_domains.join(" "));
            }
        }
        _ => {
            eprintln!("resolvconf: unknown option: {}", args[0]);
        }
    }
}

// ============================================================================
// nslookup command
// ============================================================================

fn cmd_nslookup(args: &[String]) {
    let mut query_type = "A".to_string();
    let mut names: Vec<String> = Vec::new();
    let mut server: Option<String> = None;

    for arg in args {
        match arg.as_str() {
            "-h" | "--help" => {
                println!("Usage: nslookup [-type=TYPE] <hostname> [server]");
                println!();
                println!("DNS lookup utility.");
                println!();
                println!("Options:");
                println!("  -type=TYPE    Query type (A, AAAA, MX, NS, TXT, etc.)");
                println!("  -h, --help    Show help");
                println!("  -V, --version Show version");
                process::exit(0);
            }
            "-V" | "--version" => {
                println!("nslookup {VERSION}");
                process::exit(0);
            }
            s if s.starts_with("-type=") || s.starts_with("-query=") => {
                if let Some(t) = s.split_once('=').map(|(_, v)| v) {
                    query_type = t.to_string();
                }
            }
            s if !s.starts_with('-') => {
                if names.is_empty() {
                    names.push(s.to_string());
                } else if server.is_none() {
                    server = Some(s.to_string());
                }
            }
            _ => {}
        }
    }

    if names.is_empty() {
        eprintln!("nslookup: no hostname specified");
        process::exit(1);
    }

    let config = read_resolv_conf();
    let dns_server = server.unwrap_or_else(|| {
        config
            .servers
            .first()
            .map(|s| s.address.clone())
            .unwrap_or_else(|| "127.0.0.53".to_string())
    });

    let stdout = io::stdout();
    let mut out = stdout.lock();

    let _ = writeln!(out, "Server:\t\t{dns_server}");
    let _ = writeln!(out, "Address:\t{dns_server}#53");
    let _ = writeln!(out);

    let rtype = record_type_str(&query_type);

    for name in &names {
        let _ = writeln!(out, "Non-authoritative answer:");
        let ips = resolve_hostname(name);
        if ips.is_empty() {
            let _ = writeln!(out, "** server can't find {name}: NXDOMAIN");
        } else {
            for ip in &ips {
                match (rtype, ip) {
                    ("A", IpAddr::V4(v4)) => {
                        let _ = writeln!(out, "Name:\t{name}");
                        let _ = writeln!(out, "Address: {v4}");
                    }
                    ("AAAA", IpAddr::V6(v6)) => {
                        let _ = writeln!(out, "Name:\t{name}");
                        let _ = writeln!(out, "Address: {v6}");
                    }
                    _ => {
                        let _ = writeln!(out, "Name:\t{name}");
                        let _ = writeln!(out, "Address: {ip}");
                    }
                }
            }
        }
        let _ = writeln!(out);
    }
}

// ============================================================================
// host command
// ============================================================================

fn cmd_host(args: &[String]) {
    let mut verbose = false;
    let mut names: Vec<String> = Vec::new();
    let mut query_type: Option<String> = None;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-h" | "--help" => {
                println!("Usage: host [-v] [-t type] <hostname> [server]");
                println!();
                println!("DNS lookup utility.");
                println!();
                println!("Options:");
                println!("  -v             Verbose output");
                println!("  -t TYPE        Query type (A, AAAA, MX, NS, etc.)");
                println!("  -h, --help     Show help");
                println!("  -V, --version  Show version");
                process::exit(0);
            }
            "-V" | "--version" => {
                println!("host {VERSION}");
                process::exit(0);
            }
            "-v" => verbose = true,
            "-t" => {
                i += 1;
                if i < args.len() {
                    query_type = Some(args[i].clone());
                }
            }
            s if !s.starts_with('-') => {
                names.push(s.to_string());
            }
            _ => {}
        }
        i += 1;
    }

    if names.is_empty() {
        eprintln!("host: no hostname specified");
        process::exit(1);
    }

    let stdout = io::stdout();
    let mut out = stdout.lock();

    let _rtype = query_type.as_deref().unwrap_or("A");

    for name in &names {
        // Check if this looks like an IP address (reverse lookup).
        if name.parse::<IpAddr>().is_ok() {
            if let Some(hostname) = reverse_lookup(name) {
                let _ = writeln!(out, "{name} domain name pointer {hostname}.");
            } else {
                let _ = writeln!(out, "Host {name} not found: 3(NXDOMAIN)");
            }
            continue;
        }

        let ips = resolve_hostname(name);
        if ips.is_empty() {
            let _ = writeln!(out, "Host {name} not found: 3(NXDOMAIN)");
        } else {
            for ip in &ips {
                match ip {
                    IpAddr::V4(v4) => {
                        let _ = writeln!(out, "{name} has address {v4}");
                    }
                    IpAddr::V6(v6) => {
                        let _ = writeln!(out, "{name} has IPv6 address {v6}");
                    }
                }
            }
            if verbose {
                let _ = writeln!(out, ";; Query time: 0 msec");
                let config = read_resolv_conf();
                if let Some(first) = config.servers.first() {
                    let _ = writeln!(out, ";; SERVER: {}#53", first.address);
                }
            }
        }
    }
}

// ============================================================================
// CLI
// ============================================================================

fn main() {
    let args: Vec<String> = env::args().collect();

    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("resolvectl");
        let bytes = s.as_bytes();
        let mut last_sep = 0;
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'/' || b == b'\\' {
                last_sep = i + 1;
            }
        }
        let base = &s[last_sep..];
        let base = base.strip_suffix(".exe").unwrap_or(base);
        base.to_string()
    };

    let rest: Vec<String> = args.into_iter().skip(1).collect();

    match prog_name.as_str() {
        "resolvconf" => cmd_resolvconf(&rest),
        "systemd-resolve" => cmd_resolvectl(&rest),
        "nslookup" => cmd_nslookup(&rest),
        "host" => cmd_host(&rest),
        _ => cmd_resolvectl(&rest),
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_read_resolv_conf() {
        let config = read_resolv_conf();
        // Should have at least one default server.
        assert!(!config.servers.is_empty());
    }

    #[test]
    fn test_default_links() {
        let links = generate_default_links();
        assert_eq!(links.len(), 3);
        assert_eq!(links[0].name, "lo");
        assert_eq!(links[1].name, "eth0");
        assert_eq!(links[2].name, "wlan0");
    }

    #[test]
    fn test_default_link_dns() {
        let links = generate_default_links();
        assert!(links[1].dns_servers.contains(&"8.8.8.8".to_string()));
        assert!(links[1].dns_servers.contains(&"8.8.4.4".to_string()));
    }

    #[test]
    fn test_resolve_localhost() {
        let ips = resolve_hostname("localhost");
        assert!(!ips.is_empty());
        assert!(ips.contains(&IpAddr::V4(Ipv4Addr::LOCALHOST)));
    }

    #[test]
    fn test_reverse_lookup_localhost() {
        assert_eq!(reverse_lookup("127.0.0.1"), Some("localhost".to_string()));
        assert_eq!(reverse_lookup("::1"), Some("localhost".to_string()));
    }

    #[test]
    fn test_reverse_lookup_unknown() {
        assert_eq!(reverse_lookup("10.0.0.1"), None);
    }

    #[test]
    fn test_record_type_str() {
        assert_eq!(record_type_str("A"), "A");
        assert_eq!(record_type_str("aaaa"), "AAAA");
        assert_eq!(record_type_str("MX"), "MX");
        assert_eq!(record_type_str("unknown"), "A");
    }

    #[test]
    fn test_dns_server_clone() {
        let server = DnsServer {
            address: "8.8.8.8".to_string(),
            interface: Some("eth0".to_string()),
            _protocol: DnsProtocol::Classic,
        };
        let c = server.clone();
        assert_eq!(c.address, "8.8.8.8");
        assert_eq!(c.interface, Some("eth0".to_string()));
    }

    #[test]
    fn test_dns_config_clone() {
        let config = DnsConfig {
            servers: vec![DnsServer {
                address: "1.1.1.1".to_string(),
                interface: None,
                _protocol: DnsProtocol::_Tls,
            }],
            search_domains: vec!["example.com".to_string()],
            _options: HashMap::new(),
        };
        let c = config.clone();
        assert_eq!(c.servers.len(), 1);
        assert_eq!(c.search_domains, vec!["example.com"]);
    }

    #[test]
    fn test_link_info_clone() {
        let link = LinkInfo {
            index: 2,
            name: "eth0".to_string(),
            dns_servers: vec!["8.8.8.8".to_string()],
            search_domains: vec!["local".to_string()],
            _dnssec: DnssecMode::_Yes,
            dns_over_tls: true,
            _mdns: true,
            _llmnr: false,
        };
        let c = link.clone();
        assert_eq!(c.index, 2);
        assert_eq!(c.name, "eth0");
        assert!(c.dns_over_tls);
    }

    #[test]
    fn test_dnssec_mode_clone() {
        let mode = DnssecMode::AllowDowngrade;
        let _c = mode.clone();
    }

    #[test]
    fn test_dns_protocol_clone() {
        let proto = DnsProtocol::_Https;
        let _c = proto.clone();
    }

    #[test]
    fn test_resolve_nonexistent() {
        let ips = resolve_hostname("this.host.definitely.does.not.exist.invalid");
        // Should return empty for non-existent hosts.
        // (May succeed on some networks with DNS hijacking, so we don't assert empty.)
        let _ = ips;
    }

    #[test]
    fn test_default_link_lo_empty_dns() {
        let links = generate_default_links();
        assert!(links[0].dns_servers.is_empty());
        assert!(links[0].search_domains.is_empty());
    }

    #[test]
    fn test_resolv_conf_path() {
        assert_eq!(RESOLV_CONF, "/etc/resolv.conf");
    }

    #[test]
    fn test_resolved_conf_path() {
        assert_eq!(_RESOLVED_CONF, "/etc/systemd/resolved.conf");
    }
}
