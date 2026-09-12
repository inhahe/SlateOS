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
use std::path::Path;
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

// `passwd` and `group` are [`pwdb`]'s types rather than two more of this
// program's own. Parsing them here cost two defects at once:
//
//   * `read_to_string` fails for the WHOLE file on a single byte that is not
//     valid UTF-8, and the arm was `Err(_) => Vec::new()` -- so one account
//     whose GECOS field holds a name in Latin-1, which is the ordinary way a
//     person's name gets into that field, made `getent passwd alice` answer
//     "no such user" (exit 2) for every account on the system. A wrong answer,
//     not an error. This filesystem allows every byte but `/` and NUL.
//   * `uid: fields[2].parse().unwrap_or(0)` turned a malformed uid into **0**,
//     which is root. `pwdb` rejects the line instead, which is what glibc's
//     `fgetpwent` does.
//
// The output format is unchanged and still pinned by the tests below; it is
// produced as BYTES now, because a field this program did not choose may
// contain any of them.

/// One `/etc/passwd` record, in the layout `getent passwd` prints.
///
/// Measured against GNU getent on this machine:
/// `root:x:0:0:root:/root:/bin/bash`.
fn format_user(u: &pwdb::User) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&u.name);
    out.push(b':');
    out.extend_from_slice(&u.passwd);
    out.push(b':');
    out.extend_from_slice(u.uid.to_string().as_bytes());
    out.push(b':');
    out.extend_from_slice(u.gid.to_string().as_bytes());
    out.push(b':');
    out.extend_from_slice(&u.gecos);
    out.push(b':');
    out.extend_from_slice(&u.dir);
    out.push(b':');
    out.extend_from_slice(&u.shell);
    out
}

/// One `/etc/group` record, in the layout `getent group` prints.
///
/// Measured against GNU getent: `sudo:x:27:inhahe`, and `root:x:0:` for a
/// group with no supplementary members -- the trailing colon stays.
fn format_group(g: &pwdb::Group) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&g.name);
    out.push(b':');
    out.extend_from_slice(&g.passwd);
    out.push(b':');
    out.extend_from_slice(g.gid.to_string().as_bytes());
    out.push(b':');
    for (i, m) in g.members.iter().enumerate() {
        if i > 0 {
            out.push(b',');
        }
        out.extend_from_slice(m);
    }
    out
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
                self.name,
                self.number,
                self.aliases.join(" ")
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
                self.name,
                self.number,
                self.aliases.join(" ")
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

/// The bytes of a database file, or empty if it is not there.
///
/// A file that is ABSENT and a file that cannot be READ are different answers,
/// and `unwrap_or_default()` gives them the same one. getent's exit contract
/// does not distinguish them -- both end in "no entry", exit 2, which is what
/// GNU does too -- so this does not change the status. What it changes is that
/// the unreadable case now SAYS so on stderr instead of looking like an empty
/// database, because "there is no such user" and "I was not allowed to look"
/// are not the same thing to tell somebody.
fn database_bytes(path: &str) -> Vec<u8> {
    match optionalfile::read_bytes_or_empty(Path::new(path)) {
        Ok(bytes) => bytes,
        Err(e) => {
            // Through `quotef_os`: a path with a newline in it would
            // otherwise forge a line of this program's own stderr.
            eprintln!("getent: {}: {e}", quoting::quotef_os(path));
            Vec::new()
        }
    }
}

fn parse_passwd() -> Vec<pwdb::User> {
    pwdb::users(&database_bytes(PASSWD_FILE))
}

fn parse_group() -> Vec<pwdb::Group> {
    pwdb::groups(&database_bytes(GROUP_FILE))
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
                && let Ok(port) = port_proto[0].parse::<u16>()
            {
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
            && let Ok(number) = parts[1].parse::<u32>()
        {
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

/// Write one already-formatted record and a newline, as bytes.
///
/// `writeln!("{}", ...)` cannot be used for these two databases any more: the
/// fields are bytes, and a name this program did not choose may hold any of
/// them. Going through `String` would have to either refuse the record or
/// corrupt it, and `from_utf8_lossy` corrupting it silently is the worse half.
fn write_record(out: &mut impl Write, record: &[u8]) {
    let _ = out.write_all(record);
    let _ = out.write_all(b"\n");
}

fn lookup_passwd(keys: &[String]) -> i32 {
    let entries = parse_passwd();
    let stdout = io::stdout();
    let mut out = stdout.lock();

    if keys.is_empty() {
        // Print all.
        for e in &entries {
            write_record(&mut out, &format_user(e));
        }
        return 0;
    }

    let mut ret = 0;
    for key in keys {
        let found = if let Ok(uid) = key.parse::<u32>() {
            entries.iter().find(|e| e.uid == uid)
        } else {
            entries.iter().find(|e| e.name == key.as_bytes())
        };
        match found {
            Some(e) => write_record(&mut out, &format_user(e)),
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
            write_record(&mut out, &format_group(e));
        }
        return 0;
    }

    let mut ret = 0;
    for key in keys {
        let found = if let Ok(gid) = key.parse::<u32>() {
            entries.iter().find(|e| e.gid == gid)
        } else {
            entries.iter().find(|e| e.name == key.as_bytes())
        };
        match found {
            Some(e) => write_record(&mut out, &format_group(e)),
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

    /// The one record, parsed and reprinted. Round-tripping through the parser
    /// is what a hand-built struct could not check: the format tests used to
    /// assert that a struct someone filled in by hand printed the way they had
    /// filled it in, which is true of any formatter.
    fn reprint_user(line: &str) -> String {
        let users = pwdb::users(line.as_bytes());
        assert_eq!(users.len(), 1, "fixture should hold exactly one record");
        String::from_utf8(format_user(&users[0])).expect("ascii fixture")
    }

    fn reprint_group(line: &str) -> String {
        let groups = pwdb::groups(line.as_bytes());
        assert_eq!(groups.len(), 1, "fixture should hold exactly one record");
        String::from_utf8(format_group(&groups[0])).expect("ascii fixture")
    }

    #[test]
    fn test_passwd_entry_format() {
        // Measured against GNU getent on this machine.
        let line = "root:x:0:0:root:/root:/bin/bash";
        assert_eq!(reprint_user(line), line);
    }

    #[test]
    fn test_group_entry_format() {
        assert_eq!(
            reprint_group("wheel:x:10:alice,bob"),
            "wheel:x:10:alice,bob"
        );
    }

    #[test]
    fn test_group_entry_no_members() {
        // GNU prints the trailing colon: `getent group root` is `root:x:0:`.
        assert_eq!(reprint_group("nogroup:x:65534:"), "nogroup:x:65534:");
    }

    #[test]
    fn a_gecos_field_that_is_not_utf8_still_yields_its_record() {
        // A person's name in Latin-1 is the ordinary way a non-UTF-8 byte gets
        // into /etc/passwd. `read_to_string` failed for the WHOLE file on it,
        // and the arm was `Err(_) => Vec::new()`, so `getent passwd alice`
        // answered "no such user" -- exit 2 -- for every account on the system.
        let mut line = Vec::from(&b"jose:x:1000:1000:Jos"[..]);
        line.push(0xE9); // 'e-acute' in Latin-1; not valid UTF-8 on its own
        line.extend_from_slice(b":/home/jose:/bin/sh");
        assert!(
            core::str::from_utf8(&line).is_err(),
            "fixture must not be utf-8, or this test proves nothing"
        );

        let users = pwdb::users(&line);
        assert_eq!(users.len(), 1, "the record must survive");
        assert_eq!(users[0].name, b"jose");
        // And it is reprinted byte-for-byte, not replaced with U+FFFD.
        assert_eq!(format_user(&users[0]), line);
    }

    #[test]
    fn a_malformed_uid_is_not_root() {
        // `uid: fields[2].parse().unwrap_or(0)` reported this line as uid 0.
        // glibc's fgetpwent rejects it, and so does pwdb.
        let users = pwdb::users(b"broken:x:notanumber:5:::/bin/sh");
        assert!(
            users.is_empty(),
            "a line with no usable uid is not a record"
        );
        // The sane neighbour on the next line still parses, so one bad line
        // does not cost the file.
        let users = pwdb::users(b"broken:x:notanumber:5:::/bin/sh\nok:x:7:7:::/bin/sh");
        assert_eq!(users.len(), 1);
        assert_eq!(users[0].uid, 7);
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

    /// Replaces `test_passwd_entry_clone`, which built a struct by hand,
    /// cloned it, and asserted the clone had the fields it was given -- a test
    /// of `#[derive(Clone)]`, which the compiler already guarantees, and of a
    /// type this program no longer owns. This asserts something the program
    /// can actually get wrong: which field a key is matched against.
    #[test]
    fn a_numeric_key_selects_by_id_and_a_name_key_by_name() {
        let db = "alice:x:1000:1000::/home/alice:/bin/sh
1000:x:7:7::/home/odd:/bin/sh
";
        let users = pwdb::users(db.as_bytes());
        assert_eq!(users.len(), 2);

        // The second account is *named* "1000". A numeric key is a uid, so it
        // must select alice (uid 1000) and not the account called 1000.
        let by_uid = users.iter().find(|e| e.uid == 1000).expect("uid 1000");
        assert_eq!(by_uid.name, b"alice");

        // A key that does not parse as a number is matched against the name,
        // as bytes.
        let by_name = users
            .iter()
            .find(|e| e.name == "1000".as_bytes())
            .expect("named 1000");
        assert_eq!(by_name.uid, 7);
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
