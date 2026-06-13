//! Slate OS name service lookup utility.
//!
//! Multi-personality binary providing:
//! - **getent** — get entries from Name Service Switch databases
//!
//! Supports databases: passwd, group, hosts, services, protocols, networks, shadow.

#![deny(clippy::all)]

use std::env;
use std::fs;
use std::io::{self, Write};
use std::process;

const VERSION: &str = "0.1.0";

// ============================================================================
// Database file paths
// ============================================================================

const PASSWD_FILE: &str = "/etc/passwd";
const GROUP_FILE: &str = "/etc/group";
const SHADOW_FILE: &str = "/etc/shadow";
const HOSTS_FILE: &str = "/etc/hosts";
const SERVICES_FILE: &str = "/etc/services";
const PROTOCOLS_FILE: &str = "/etc/protocols";
const NETWORKS_FILE: &str = "/etc/networks";
const ETHERS_FILE: &str = "/etc/ethers";
const RPC_FILE: &str = "/etc/rpc";

// ============================================================================
// Database entry types
// ============================================================================

#[derive(Clone, Debug)]
struct PasswdEntry {
    name: String,
    passwd: String,
    uid: u32,
    gid: u32,
    gecos: String,
    home: String,
    shell: String,
}

impl PasswdEntry {
    fn format(&self) -> String {
        format!(
            "{}:{}:{}:{}:{}:{}:{}",
            self.name, self.passwd, self.uid, self.gid, self.gecos, self.home, self.shell
        )
    }
}

#[derive(Clone, Debug)]
struct GroupEntry {
    name: String,
    passwd: String,
    gid: u32,
    members: Vec<String>,
}

impl GroupEntry {
    fn format(&self) -> String {
        format!(
            "{}:{}:{}:{}",
            self.name, self.passwd, self.gid, self.members.join(",")
        )
    }
}

#[derive(Clone, Debug)]
struct HostEntry {
    addr: String,
    names: Vec<String>,
}

impl HostEntry {
    fn format(&self) -> String {
        format!("{:<16}{}", self.addr, self.names.join(" "))
    }
}

#[derive(Clone, Debug)]
struct ServiceEntry {
    name: String,
    port: u16,
    protocol: String,
    aliases: Vec<String>,
}

impl ServiceEntry {
    fn format(&self) -> String {
        if self.aliases.is_empty() {
            format!("{:<24}{}/{}", self.name, self.port, self.protocol)
        } else {
            format!(
                "{:<24}{}/{}  {}",
                self.name,
                self.port,
                self.protocol,
                self.aliases.join(" ")
            )
        }
    }
}

#[derive(Clone, Debug)]
struct ProtocolEntry {
    name: String,
    number: u32,
    aliases: Vec<String>,
}

impl ProtocolEntry {
    fn format(&self) -> String {
        if self.aliases.is_empty() {
            format!("{:<24}{}", self.name, self.number)
        } else {
            format!(
                "{:<24}{}  {}",
                self.name, self.number, self.aliases.join(" ")
            )
        }
    }
}

#[derive(Clone, Debug)]
struct NetworkEntry {
    name: String,
    number: String,
    aliases: Vec<String>,
}

impl NetworkEntry {
    fn format(&self) -> String {
        if self.aliases.is_empty() {
            format!("{:<24}{}", self.name, self.number)
        } else {
            format!(
                "{:<24}{}  {}",
                self.name, self.number, self.aliases.join(" ")
            )
        }
    }
}

#[derive(Clone, Debug)]
struct ShadowEntry {
    name: String,
    hash: String,
    last_change: String,
    min: String,
    max: String,
    warn: String,
    inactive: String,
    expire: String,
    reserved: String,
}

impl ShadowEntry {
    fn format(&self) -> String {
        format!(
            "{}:{}:{}:{}:{}:{}:{}:{}:{}",
            self.name,
            self.hash,
            self.last_change,
            self.min,
            self.max,
            self.warn,
            self.inactive,
            self.expire,
            self.reserved
        )
    }
}

// ============================================================================
// Database parsers
// ============================================================================

fn parse_passwd() -> Vec<PasswdEntry> {
    let content = match fs::read_to_string(PASSWD_FILE) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };

    let mut entries = Vec::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let fields: Vec<&str> = line.split(':').collect();
        if fields.len() >= 7 {
            entries.push(PasswdEntry {
                name: fields[0].to_string(),
                passwd: fields[1].to_string(),
                uid: fields[2].parse().unwrap_or(0),
                gid: fields[3].parse().unwrap_or(0),
                gecos: fields[4].to_string(),
                home: fields[5].to_string(),
                shell: fields[6].to_string(),
            });
        }
    }
    entries
}

fn parse_group() -> Vec<GroupEntry> {
    let content = match fs::read_to_string(GROUP_FILE) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };

    let mut entries = Vec::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let fields: Vec<&str> = line.split(':').collect();
        if fields.len() >= 4 {
            let members = if fields[3].is_empty() {
                Vec::new()
            } else {
                fields[3].split(',').map(|s| s.to_string()).collect()
            };
            entries.push(GroupEntry {
                name: fields[0].to_string(),
                passwd: fields[1].to_string(),
                gid: fields[2].parse().unwrap_or(0),
                members,
            });
        }
    }
    entries
}

fn parse_hosts() -> Vec<HostEntry> {
    let content = match fs::read_to_string(HOSTS_FILE) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };

    let mut entries = Vec::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        // Remove inline comments.
        let line = if let Some(idx) = line.find('#') {
            line[..idx].trim()
        } else {
            line
        };
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 2 {
            entries.push(HostEntry {
                addr: parts[0].to_string(),
                names: parts[1..].iter().map(|s| s.to_string()).collect(),
            });
        }
    }
    entries
}

fn parse_services() -> Vec<ServiceEntry> {
    let content = match fs::read_to_string(SERVICES_FILE) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };

    let mut entries = Vec::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = if let Some(idx) = line.find('#') {
            line[..idx].trim()
        } else {
            line
        };
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 2 {
            let port_proto: Vec<&str> = parts[1].split('/').collect();
            if port_proto.len() == 2
                && let Ok(port) = port_proto[0].parse::<u16>() {
                    entries.push(ServiceEntry {
                        name: parts[0].to_string(),
                        port,
                        protocol: port_proto[1].to_string(),
                        aliases: parts[2..].iter().map(|s| s.to_string()).collect(),
                    });
                }
        }
    }
    entries
}

fn parse_protocols() -> Vec<ProtocolEntry> {
    let content = match fs::read_to_string(PROTOCOLS_FILE) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };

    let mut entries = Vec::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = if let Some(idx) = line.find('#') {
            line[..idx].trim()
        } else {
            line
        };
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 2
            && let Ok(number) = parts[1].parse::<u32>() {
                entries.push(ProtocolEntry {
                    name: parts[0].to_string(),
                    number,
                    aliases: parts[2..].iter().map(|s| s.to_string()).collect(),
                });
            }
    }
    entries
}

fn parse_networks() -> Vec<NetworkEntry> {
    let content = match fs::read_to_string(NETWORKS_FILE) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };

    let mut entries = Vec::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = if let Some(idx) = line.find('#') {
            line[..idx].trim()
        } else {
            line
        };
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 2 {
            entries.push(NetworkEntry {
                name: parts[0].to_string(),
                number: parts[1].to_string(),
                aliases: parts[2..].iter().map(|s| s.to_string()).collect(),
            });
        }
    }
    entries
}

fn parse_shadow() -> Vec<ShadowEntry> {
    let content = match fs::read_to_string(SHADOW_FILE) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };

    let mut entries = Vec::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let fields: Vec<&str> = line.split(':').collect();
        if fields.len() >= 9 {
            entries.push(ShadowEntry {
                name: fields[0].to_string(),
                hash: fields[1].to_string(),
                last_change: fields[2].to_string(),
                min: fields[3].to_string(),
                max: fields[4].to_string(),
                warn: fields[5].to_string(),
                inactive: fields[6].to_string(),
                expire: fields[7].to_string(),
                reserved: fields[8].to_string(),
            });
        }
    }
    entries
}

// ============================================================================
// Lookup functions
// ============================================================================

fn lookup_passwd(keys: &[String]) -> i32 {
    let entries = parse_passwd();
    let stdout = io::stdout();
    let mut out = stdout.lock();

    if keys.is_empty() {
        // Print all.
        for e in &entries {
            let _ = writeln!(out, "{}", e.format());
        }
        return 0;
    }

    let mut ret = 0;
    for key in keys {
        let found = if let Ok(uid) = key.parse::<u32>() {
            entries.iter().find(|e| e.uid == uid)
        } else {
            entries.iter().find(|e| e.name == *key)
        };
        match found {
            Some(e) => {
                let _ = writeln!(out, "{}", e.format());
            }
            None => ret = 2,
        }
    }
    ret
}

fn lookup_group(keys: &[String]) -> i32 {
    let entries = parse_group();
    let stdout = io::stdout();
    let mut out = stdout.lock();

    if keys.is_empty() {
        for e in &entries {
            let _ = writeln!(out, "{}", e.format());
        }
        return 0;
    }

    let mut ret = 0;
    for key in keys {
        let found = if let Ok(gid) = key.parse::<u32>() {
            entries.iter().find(|e| e.gid == gid)
        } else {
            entries.iter().find(|e| e.name == *key)
        };
        match found {
            Some(e) => {
                let _ = writeln!(out, "{}", e.format());
            }
            None => ret = 2,
        }
    }
    ret
}

fn lookup_hosts(keys: &[String]) -> i32 {
    let entries = parse_hosts();
    let stdout = io::stdout();
    let mut out = stdout.lock();

    if keys.is_empty() {
        for e in &entries {
            let _ = writeln!(out, "{}", e.format());
        }
        return 0;
    }

    let mut ret = 0;
    for key in keys {
        let found: Vec<&HostEntry> = entries
            .iter()
            .filter(|e| e.addr == *key || e.names.iter().any(|n| n == key))
            .collect();
        if found.is_empty() {
            ret = 2;
        } else {
            for e in found {
                let _ = writeln!(out, "{}", e.format());
            }
        }
    }
    ret
}

fn lookup_services(keys: &[String]) -> i32 {
    let entries = parse_services();
    let stdout = io::stdout();
    let mut out = stdout.lock();

    if keys.is_empty() {
        for e in &entries {
            let _ = writeln!(out, "{}", e.format());
        }
        return 0;
    }

    let mut ret = 0;
    for key in keys {
        // Key can be "name" or "port/protocol".
        let found: Vec<&ServiceEntry> = if key.contains('/') {
            let parts: Vec<&str> = key.split('/').collect();
            if parts.len() == 2 {
                if let Ok(port) = parts[0].parse::<u16>() {
                    entries
                        .iter()
                        .filter(|e| e.port == port && e.protocol == parts[1])
                        .collect()
                } else {
                    entries
                        .iter()
                        .filter(|e| e.name == parts[0] && e.protocol == parts[1])
                        .collect()
                }
            } else {
                Vec::new()
            }
        } else if let Ok(port) = key.parse::<u16>() {
            entries.iter().filter(|e| e.port == port).collect()
        } else {
            entries
                .iter()
                .filter(|e| e.name == *key || e.aliases.iter().any(|a| a == key))
                .collect()
        };

        if found.is_empty() {
            ret = 2;
        } else {
            for e in found {
                let _ = writeln!(out, "{}", e.format());
            }
        }
    }
    ret
}

fn lookup_protocols(keys: &[String]) -> i32 {
    let entries = parse_protocols();
    let stdout = io::stdout();
    let mut out = stdout.lock();

    if keys.is_empty() {
        for e in &entries {
            let _ = writeln!(out, "{}", e.format());
        }
        return 0;
    }

    let mut ret = 0;
    for key in keys {
        let found = if let Ok(num) = key.parse::<u32>() {
            entries.iter().find(|e| e.number == num)
        } else {
            entries
                .iter()
                .find(|e| e.name == *key || e.aliases.iter().any(|a| a == key))
        };
        match found {
            Some(e) => {
                let _ = writeln!(out, "{}", e.format());
            }
            None => ret = 2,
        }
    }
    ret
}

fn lookup_networks(keys: &[String]) -> i32 {
    let entries = parse_networks();
    let stdout = io::stdout();
    let mut out = stdout.lock();

    if keys.is_empty() {
        for e in &entries {
            let _ = writeln!(out, "{}", e.format());
        }
        return 0;
    }

    let mut ret = 0;
    for key in keys {
        let found = entries
            .iter()
            .find(|e| e.name == *key || e.number == *key || e.aliases.iter().any(|a| a == key));
        match found {
            Some(e) => {
                let _ = writeln!(out, "{}", e.format());
            }
            None => ret = 2,
        }
    }
    ret
}

fn lookup_shadow(keys: &[String]) -> i32 {
    let entries = parse_shadow();
    let stdout = io::stdout();
    let mut out = stdout.lock();

    if keys.is_empty() {
        for e in &entries {
            let _ = writeln!(out, "{}", e.format());
        }
        return 0;
    }

    let mut ret = 0;
    for key in keys {
        let found = entries.iter().find(|e| e.name == *key);
        match found {
            Some(e) => {
                let _ = writeln!(out, "{}", e.format());
            }
            None => ret = 2,
        }
    }
    ret
}

// ============================================================================
// Main
// ============================================================================

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();

    if rest.is_empty() {
        eprintln!("Usage: getent database [key ...]");
        process::exit(1);
    }

    let first = &rest[0];

    match first.as_str() {
        "-h" | "--help" => {
            println!("Usage: getent [option...] database [key ...]");
            println!();
            println!("Get entries from administrative database.");
            println!();
            println!("Databases:");
            println!("  passwd     User account information");
            println!("  group      Group information");
            println!("  hosts      Hostname/address mappings");
            println!("  services   Network services (port/protocol)");
            println!("  protocols  Network protocols");
            println!("  networks   Network names and numbers");
            println!("  shadow     Shadow password entries");
            println!();
            println!("Options:");
            println!("  -h, --help     Show this help");
            println!("  --version      Show version");
            process::exit(0);
        }
        "--version" => {
            println!("getent {VERSION}");
            process::exit(0);
        }
        _ => {}
    }

    let database = first.as_str();
    let keys: Vec<String> = rest[1..].to_vec();

    let ret = match database {
        "passwd" => lookup_passwd(&keys),
        "group" => lookup_group(&keys),
        "hosts" => lookup_hosts(&keys),
        "services" => lookup_services(&keys),
        "protocols" => lookup_protocols(&keys),
        "networks" => lookup_networks(&keys),
        "shadow" => lookup_shadow(&keys),
        "ethers" | "rpc" => {
            // Minimal support — just print the file.
            let path = if database == "ethers" {
                ETHERS_FILE
            } else {
                RPC_FILE
            };
            match fs::read_to_string(path) {
                Ok(content) => {
                    let stdout = io::stdout();
                    let mut out = stdout.lock();
                    for line in content.lines() {
                        let line = line.trim();
                        if !line.is_empty() && !line.starts_with('#') {
                            if keys.is_empty() {
                                let _ = writeln!(out, "{line}");
                            } else {
                                for key in &keys {
                                    if line.contains(key.as_str()) {
                                        let _ = writeln!(out, "{line}");
                                    }
                                }
                            }
                        }
                    }
                    0
                }
                Err(_) => {
                    eprintln!("getent: cannot open {path}");
                    2
                }
            }
        }
        _ => {
            eprintln!("getent: unknown database: {database}");
            process::exit(1);
        }
    };

    process::exit(ret);
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_passwd_entry_format() {
        let e = PasswdEntry {
            name: "root".to_string(),
            passwd: "x".to_string(),
            uid: 0,
            gid: 0,
            gecos: "root".to_string(),
            home: "/root".to_string(),
            shell: "/bin/bash".to_string(),
        };
        assert_eq!(e.format(), "root:x:0:0:root:/root:/bin/bash");
    }

    #[test]
    fn test_group_entry_format() {
        let e = GroupEntry {
            name: "wheel".to_string(),
            passwd: "x".to_string(),
            gid: 10,
            members: vec!["alice".to_string(), "bob".to_string()],
        };
        assert_eq!(e.format(), "wheel:x:10:alice,bob");
    }

    #[test]
    fn test_group_entry_no_members() {
        let e = GroupEntry {
            name: "nogroup".to_string(),
            passwd: "x".to_string(),
            gid: 65534,
            members: Vec::new(),
        };
        assert_eq!(e.format(), "nogroup:x:65534:");
    }

    #[test]
    fn test_host_entry_format() {
        let e = HostEntry {
            addr: "127.0.0.1".to_string(),
            names: vec!["localhost".to_string(), "localhost.localdomain".to_string()],
        };
        let f = e.format();
        assert!(f.contains("127.0.0.1"));
        assert!(f.contains("localhost"));
    }

    #[test]
    fn test_service_entry_format() {
        let e = ServiceEntry {
            name: "ssh".to_string(),
            port: 22,
            protocol: "tcp".to_string(),
            aliases: Vec::new(),
        };
        let f = e.format();
        assert!(f.contains("ssh"));
        assert!(f.contains("22/tcp"));
    }

    #[test]
    fn test_service_entry_with_aliases() {
        let e = ServiceEntry {
            name: "http".to_string(),
            port: 80,
            protocol: "tcp".to_string(),
            aliases: vec!["www".to_string()],
        };
        let f = e.format();
        assert!(f.contains("www"));
    }

    #[test]
    fn test_protocol_entry_format() {
        let e = ProtocolEntry {
            name: "tcp".to_string(),
            number: 6,
            aliases: vec!["TCP".to_string()],
        };
        let f = e.format();
        assert!(f.contains("tcp"));
        assert!(f.contains("6"));
    }

    #[test]
    fn test_network_entry_format() {
        let e = NetworkEntry {
            name: "loopback".to_string(),
            number: "127.0.0.0".to_string(),
            aliases: Vec::new(),
        };
        let f = e.format();
        assert!(f.contains("loopback"));
        assert!(f.contains("127.0.0.0"));
    }

    #[test]
    fn test_shadow_entry_format() {
        let e = ShadowEntry {
            name: "root".to_string(),
            hash: "!".to_string(),
            last_change: "19000".to_string(),
            min: "0".to_string(),
            max: "99999".to_string(),
            warn: "7".to_string(),
            inactive: "".to_string(),
            expire: "".to_string(),
            reserved: "".to_string(),
        };
        assert_eq!(e.format(), "root:!:19000:0:99999:7:::");
    }

    #[test]
    fn test_passwd_entry_clone() {
        let e = PasswdEntry {
            name: "test".to_string(),
            passwd: "x".to_string(),
            uid: 1000,
            gid: 1000,
            gecos: "Test".to_string(),
            home: "/home/test".to_string(),
            shell: "/bin/sh".to_string(),
        };
        let c = e.clone();
        assert_eq!(c.name, "test");
        assert_eq!(c.uid, 1000);
    }

    #[test]
    fn test_host_entry_clone() {
        let e = HostEntry {
            addr: "::1".to_string(),
            names: vec!["localhost".to_string()],
        };
        let c = e.clone();
        assert_eq!(c.addr, "::1");
        assert_eq!(c.names.len(), 1);
    }

    #[test]
    fn test_parse_passwd_no_crash() {
        // Should not panic even if file doesn't exist.
        let _ = parse_passwd();
    }

    #[test]
    fn test_parse_group_no_crash() {
        let _ = parse_group();
    }

    #[test]
    fn test_parse_hosts_no_crash() {
        let _ = parse_hosts();
    }

    #[test]
    fn test_parse_services_no_crash() {
        let _ = parse_services();
    }

    #[test]
    fn test_parse_protocols_no_crash() {
        let _ = parse_protocols();
    }

    #[test]
    fn test_parse_networks_no_crash() {
        let _ = parse_networks();
    }

    #[test]
    fn test_parse_shadow_no_crash() {
        let _ = parse_shadow();
    }
}
