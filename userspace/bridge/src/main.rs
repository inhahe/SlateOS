//! Slate OS Bridge, Traffic Control, and Ethernet Bridge Filtering Utility
//!
//! Multi-personality binary providing:
//! - **bridge** -- bridge management (link, fdb, mdb, vlan, monitor)
//! - **tc** -- traffic control (qdisc, class, filter)
//! - **ebtables** -- Ethernet bridge frame filtering
//!
//! Personality is detected from `argv[0]` basename.

#![deny(clippy::all)]

use std::env;
use std::io::Write;

// ============================================================================
// Constants
// ============================================================================

const VERSION: &str = "0.1.0";

// Bridge port states
const BR_STATE_DISABLED: u8 = 0;
const BR_STATE_LISTENING: u8 = 1;
const BR_STATE_LEARNING: u8 = 2;
const BR_STATE_FORWARDING: u8 = 3;
const BR_STATE_BLOCKING: u8 = 4;

// Ebtables default targets
const TARGET_ACCEPT: &str = "ACCEPT";
const TARGET_DROP: &str = "DROP";
// TARGET_CONTINUE is a valid ebtables target that users may specify in rules;
// it is not referenced by the tool's own logic but belongs in the constant set.
#[allow(dead_code)]
const TARGET_CONTINUE: &str = "CONTINUE";
const TARGET_RETURN: &str = "RETURN";

// Ebtables built-in chains
const CHAIN_INPUT: &str = "INPUT";
const CHAIN_OUTPUT: &str = "OUTPUT";
const CHAIN_FORWARD: &str = "FORWARD";

// TC handle constants
const TC_H_ROOT: u32 = 0xFFFF_FFFF;
const TC_H_INGRESS: u32 = 0xFFFF_FFF1;

// ============================================================================
// Personality detection
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tool {
    Bridge,
    Tc,
    Ebtables,
}

fn detect_tool(argv0: &str) -> Tool {
    let bytes = argv0.as_bytes();
    let mut last_sep = 0;
    for (i, &b) in bytes.iter().enumerate() {
        if b == b'/' || b == b'\\' {
            last_sep = i + 1;
        }
    }
    let base = &argv0[last_sep..];
    let base = base.strip_suffix(".exe").unwrap_or(base);
    if base == "tc" {
        Tool::Tc
    } else if base == "ebtables" || base == "ebtables-legacy" {
        Tool::Ebtables
    } else {
        Tool::Bridge
    }
}

// ============================================================================
// Output helpers
// ============================================================================

struct OutputCtx {
    json: bool,
    pretty: bool,
    stats: bool,
    details: bool,
}

impl OutputCtx {
    fn new() -> Self {
        Self {
            json: false,
            pretty: false,
            stats: false,
            details: false,
        }
    }
}

fn write_out(s: &str) {
    let _ = std::io::stdout().write_all(s.as_bytes());
}

fn write_err(s: &str) {
    let _ = std::io::stderr().write_all(s.as_bytes());
}

// ============================================================================
// MAC address parsing / formatting
// ============================================================================

fn parse_mac(s: &str) -> Option<[u8; 6]> {
    let parts: Vec<&str> = s.split(':').collect();
    if parts.len() != 6 {
        return None;
    }
    let mut mac = [0u8; 6];
    for (i, part) in parts.iter().enumerate() {
        mac[i] = u8::from_str_radix(part, 16).ok()?;
    }
    Some(mac)
}

fn format_mac(mac: &[u8; 6]) -> String {
    format!(
        "{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
        mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]
    )
}

fn state_name(state: u8) -> &'static str {
    match state {
        BR_STATE_DISABLED => "disabled",
        BR_STATE_LISTENING => "listening",
        BR_STATE_LEARNING => "learning",
        BR_STATE_FORWARDING => "forwarding",
        BR_STATE_BLOCKING => "blocking",
        _ => "unknown",
    }
}

fn parse_state(s: &str) -> Option<u8> {
    match s {
        "disabled" => Some(BR_STATE_DISABLED),
        "listening" => Some(BR_STATE_LISTENING),
        "learning" => Some(BR_STATE_LEARNING),
        "forwarding" => Some(BR_STATE_FORWARDING),
        "blocking" => Some(BR_STATE_BLOCKING),
        _ => None,
    }
}

// ============================================================================
// Rate / size parsing (used by tc)
// ============================================================================

/// Parse a rate string like "1mbit", "100kbit", "10mbps", "1gbit", etc.
/// Returns bits per second.
fn parse_rate(s: &str) -> Option<u64> {
    let s_low = s.to_lowercase();
    if let Some(n) = s_low.strip_suffix("gbit") {
        n.parse::<u64>().ok().map(|v| v * 1_000_000_000)
    } else if let Some(n) = s_low.strip_suffix("gbps") {
        n.parse::<u64>().ok().map(|v| v * 8_000_000_000)
    } else if let Some(n) = s_low.strip_suffix("mbit") {
        n.parse::<u64>().ok().map(|v| v * 1_000_000)
    } else if let Some(n) = s_low.strip_suffix("mbps") {
        n.parse::<u64>().ok().map(|v| v * 8_000_000)
    } else if let Some(n) = s_low.strip_suffix("kbit") {
        n.parse::<u64>().ok().map(|v| v * 1000)
    } else if let Some(n) = s_low.strip_suffix("kbps") {
        n.parse::<u64>().ok().map(|v| v * 8000)
    } else if let Some(n) = s_low.strip_suffix("bit") {
        n.parse::<u64>().ok()
    } else if let Some(n) = s_low.strip_suffix("bps") {
        n.parse::<u64>().ok().map(|v| v * 8)
    } else {
        // Default: treat as bits/sec
        s.parse::<u64>().ok()
    }
}

/// Format a rate in human-readable form.
fn format_rate(bps: u64) -> String {
    if bps >= 1_000_000_000 && bps.is_multiple_of(1_000_000_000) {
        format!("{}Gbit", bps / 1_000_000_000)
    } else if bps >= 1_000_000 && bps.is_multiple_of(1_000_000) {
        format!("{}Mbit", bps / 1_000_000)
    } else if bps >= 1000 && bps.is_multiple_of(1000) {
        format!("{}Kbit", bps / 1000)
    } else {
        format!("{bps}bit")
    }
}

/// Parse a size string like "1600b", "15kb", "1mb".
fn parse_size(s: &str) -> Option<u64> {
    let s_low = s.to_lowercase();
    if let Some(n) = s_low.strip_suffix("mb") {
        n.parse::<u64>().ok().map(|v| v * 1_048_576)
    } else if let Some(n) = s_low.strip_suffix("kb") {
        n.parse::<u64>().ok().map(|v| v * 1024)
    } else if let Some(n) = s_low.strip_suffix('b') {
        n.parse::<u64>().ok()
    } else {
        s.parse::<u64>().ok()
    }
}

/// Parse a time string like "100ms", "10us", "1s".
/// Returns microseconds.
fn parse_time(s: &str) -> Option<u64> {
    let s_low = s.to_lowercase();
    if let Some(n) = s_low.strip_suffix("ms") {
        n.parse::<u64>().ok().map(|v| v * 1000)
    } else if let Some(n) = s_low.strip_suffix("us") {
        n.parse::<u64>().ok()
    } else if let Some(n) = s_low.strip_suffix('s') {
        n.parse::<u64>().ok().map(|v| v * 1_000_000)
    } else {
        // Default: milliseconds
        s.parse::<u64>().ok().map(|v| v * 1000)
    }
}

/// Format microseconds in human-readable form.
fn format_time(us: u64) -> String {
    if us >= 1_000_000 && us.is_multiple_of(1_000_000) {
        format!("{}s", us / 1_000_000)
    } else if us >= 1000 && us.is_multiple_of(1000) {
        format!("{}ms", us / 1000)
    } else {
        format!("{us}us")
    }
}

/// Parse a TC handle string like "1:0", "1:10", "ffff:fff1".
fn parse_handle(s: &str) -> Option<u32> {
    if s == "root" {
        return Some(TC_H_ROOT);
    }
    if s == "ingress" {
        return Some(TC_H_INGRESS);
    }
    if let Some((major, minor)) = s.split_once(':') {
        let maj = if major.is_empty() {
            0u16
        } else {
            u16::from_str_radix(major, 16).ok()?
        };
        let min = if minor.is_empty() {
            0u16
        } else {
            u16::from_str_radix(minor, 16).ok()?
        };
        Some(((maj as u32) << 16) | (min as u32))
    } else {
        // Bare number treated as major:0
        let maj = u16::from_str_radix(s, 16).ok()?;
        Some((maj as u32) << 16)
    }
}

/// Format a TC handle.
fn format_handle(h: u32) -> String {
    if h == TC_H_ROOT {
        return "root".to_string();
    }
    if h == TC_H_INGRESS {
        return "ingress".to_string();
    }
    let major = (h >> 16) & 0xFFFF;
    let minor = h & 0xFFFF;
    format!("{major:x}:{minor:x}")
}

// ============================================================================
// Bridge data structures
// ============================================================================

#[derive(Debug, Clone)]
struct BridgePort {
    name: String,
    bridge: String,
    state: u8,
    learning: bool,
    flood: bool,
    mcast_flood: bool,
    bcast_flood: bool,
    hairpin: bool,
    guard: bool,
    root_block: bool,
    priority: u16,
    cost: u32,
}

impl BridgePort {
    fn new(name: &str, bridge: &str) -> Self {
        Self {
            name: name.to_string(),
            bridge: bridge.to_string(),
            state: BR_STATE_FORWARDING,
            learning: true,
            flood: true,
            mcast_flood: true,
            bcast_flood: true,
            hairpin: false,
            guard: false,
            root_block: false,
            priority: 128,
            cost: 100,
        }
    }
}

#[derive(Debug, Clone)]
struct FdbEntry {
    mac: [u8; 6],
    port: String,
    vlan: Option<u16>,
    is_local: bool,
    is_static: bool,
    offloaded: bool,
}

#[derive(Debug, Clone)]
struct MdbEntry {
    group: String,
    port: String,
    vlan: Option<u16>,
    is_permanent: bool,
}

#[derive(Debug, Clone)]
struct VlanEntry {
    port: String,
    vid: u16,
    vid_end: Option<u16>,
    pvid: bool,
    untagged: bool,
}

// ============================================================================
// Bridge implementation
// ============================================================================

fn bridge_show_link(ports: &[BridgePort], ctx: &OutputCtx, filter_dev: Option<&str>) {
    let filtered: Vec<&BridgePort> = if let Some(dev) = filter_dev {
        ports.iter().filter(|p| p.name == dev).collect()
    } else {
        ports.iter().collect()
    };

    if ctx.json {
        let indent = if ctx.pretty { "  " } else { "" };
        let nl = if ctx.pretty { "\n" } else { "" };
        write_out(&format!("[{nl}"));
        for (i, port) in filtered.iter().enumerate() {
            let comma = if i + 1 < filtered.len() { "," } else { "" };
            write_out(&format!(
                "{indent}{{{nl}\
                 {indent}{indent}\"ifname\": \"{}\",{nl}\
                 {indent}{indent}\"master\": \"{}\",{nl}\
                 {indent}{indent}\"state\": \"{}\",{nl}\
                 {indent}{indent}\"priority\": {},{nl}\
                 {indent}{indent}\"cost\": {},{nl}\
                 {indent}{indent}\"learning\": {},{nl}\
                 {indent}{indent}\"flood\": {},{nl}\
                 {indent}{indent}\"hairpin\": {}{nl}\
                 {indent}}}{comma}{nl}",
                port.name,
                port.bridge,
                state_name(port.state),
                port.priority,
                port.cost,
                port.learning,
                port.flood,
                port.hairpin,
            ));
        }
        write_out(&format!("]{nl}"));
    } else {
        for port in &filtered {
            write_out(&format!(
                "{}: <BROADCAST,MULTICAST,UP> mtu 1500 master {} state {}\n",
                port.name,
                port.bridge,
                state_name(port.state),
            ));
            if ctx.details {
                write_out(&format!(
                    "    priority {} cost {} learning {} flood {} hairpin {}\n",
                    port.priority,
                    port.cost,
                    if port.learning { "on" } else { "off" },
                    if port.flood { "on" } else { "off" },
                    if port.hairpin { "on" } else { "off" },
                ));
                write_out(&format!(
                    "    mcast_flood {} bcast_flood {} guard {} root_block {}\n",
                    if port.mcast_flood { "on" } else { "off" },
                    if port.bcast_flood { "on" } else { "off" },
                    if port.guard { "on" } else { "off" },
                    if port.root_block { "on" } else { "off" },
                ));
            }
            if ctx.stats {
                write_out("    RX: bytes 0 packets 0 errors 0\n");
                write_out("    TX: bytes 0 packets 0 errors 0\n");
            }
        }
    }
}

fn bridge_set_link(ports: &mut [BridgePort], args: &[String]) -> Result<(), String> {
    // bridge link set dev <dev> [learning on/off] [flood on/off] [state <state>]
    // [hairpin on/off] [guard on/off] [mcast_flood on/off] [bcast_flood on/off]
    // [priority <prio>] [cost <cost>]
    let mut dev: Option<&str> = None;
    let mut i = 0;
    let mut changes: Vec<(&str, &str)> = Vec::new();

    while i < args.len() {
        match args[i].as_str() {
            "dev" => {
                i += 1;
                dev = args.get(i).map(|s| s.as_str());
            }
            "learning" | "flood" | "hairpin" | "guard" | "mcast_flood" | "bcast_flood" => {
                let key = args[i].as_str();
                i += 1;
                let val = args.get(i).map(|s| s.as_str()).unwrap_or("on");
                changes.push((key, val));
            }
            "state" => {
                i += 1;
                let val = args.get(i).map(|s| s.as_str()).unwrap_or("forwarding");
                changes.push(("state", val));
            }
            "priority" => {
                i += 1;
                let val = args.get(i).map(|s| s.as_str()).unwrap_or("128");
                changes.push(("priority", val));
            }
            "cost" => {
                i += 1;
                let val = args.get(i).map(|s| s.as_str()).unwrap_or("100");
                changes.push(("cost", val));
            }
            _ => {}
        }
        i += 1;
    }

    let dev_name = dev.ok_or_else(|| "missing device name".to_string())?;
    let port = ports
        .iter_mut()
        .find(|p| p.name == dev_name)
        .ok_or_else(|| format!("port {dev_name} not found"))?;

    for (key, val) in &changes {
        match *key {
            "learning" => port.learning = *val == "on",
            "flood" => port.flood = *val == "on",
            "hairpin" => port.hairpin = *val == "on",
            "guard" => port.guard = *val == "on",
            "mcast_flood" => port.mcast_flood = *val == "on",
            "bcast_flood" => port.bcast_flood = *val == "on",
            "state" => {
                port.state =
                    parse_state(val).ok_or_else(|| format!("invalid state: {val}"))?;
            }
            "priority" => {
                port.priority = val
                    .parse::<u16>()
                    .map_err(|_| format!("invalid priority: {val}"))?;
            }
            "cost" => {
                port.cost = val
                    .parse::<u32>()
                    .map_err(|_| format!("invalid cost: {val}"))?;
            }
            _ => {}
        }
    }
    Ok(())
}

fn bridge_show_fdb(entries: &[FdbEntry], ctx: &OutputCtx, filter_br: Option<&str>) {
    let filtered: Vec<&FdbEntry> = if let Some(br) = filter_br {
        entries.iter().filter(|e| e.port == br).collect()
    } else {
        entries.iter().collect()
    };

    if ctx.json {
        let indent = if ctx.pretty { "  " } else { "" };
        let nl = if ctx.pretty { "\n" } else { "" };
        write_out(&format!("[{nl}"));
        for (i, e) in filtered.iter().enumerate() {
            let comma = if i + 1 < filtered.len() { "," } else { "" };
            let vlan_str = e
                .vlan
                .map(|v| format!(",{nl}{indent}{indent}\"vlan\": {v}"))
                .unwrap_or_default();
            write_out(&format!(
                "{indent}{{{nl}\
                 {indent}{indent}\"mac\": \"{}\",{nl}\
                 {indent}{indent}\"ifname\": \"{}\",{nl}\
                 {indent}{indent}\"flags\": [{}],{nl}\
                 {indent}{indent}\"state\": \"{}\"{vlan_str}{nl}\
                 {indent}}}{comma}{nl}",
                format_mac(&e.mac),
                e.port,
                if e.is_static { "\"static\"" } else { "\"dynamic\"" },
                if e.is_local { "permanent" } else { "reachable" },
            ));
        }
        write_out(&format!("]{nl}"));
    } else {
        for e in &filtered {
            let flags = if e.is_local {
                "self permanent"
            } else if e.is_static {
                "static"
            } else {
                "dynamic"
            };
            let vlan_str = e
                .vlan
                .map(|v| format!(" vlan {v}"))
                .unwrap_or_default();
            let offload_str = if e.offloaded { " offload" } else { "" };
            write_out(&format!(
                "{} dev {}{} {} master br0{offload_str}\n",
                format_mac(&e.mac),
                e.port,
                vlan_str,
                flags,
            ));
        }
    }
}

fn bridge_add_fdb(
    entries: &mut Vec<FdbEntry>,
    args: &[String],
) -> Result<(), String> {
    // bridge fdb add <mac> dev <dev> [vlan <vid>] [self] [master] [static] [dynamic]
    if args.is_empty() {
        return Err("missing MAC address".to_string());
    }
    let mac = parse_mac(&args[0]).ok_or_else(|| format!("invalid MAC: {}", args[0]))?;
    let mut dev: Option<&str> = None;
    let mut vlan: Option<u16> = None;
    let mut is_static = true;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "dev" => {
                i += 1;
                dev = args.get(i).map(|s| s.as_str());
            }
            "vlan" => {
                i += 1;
                vlan = args.get(i).and_then(|s| s.parse::<u16>().ok());
            }
            "dynamic" => is_static = false,
            "static" | "self" | "master" => {}
            _ => {}
        }
        i += 1;
    }
    let port = dev.ok_or_else(|| "missing dev".to_string())?;
    entries.push(FdbEntry {
        mac,
        port: port.to_string(),
        vlan,
        is_local: false,
        is_static,
        offloaded: false,
    });
    Ok(())
}

fn bridge_del_fdb(
    entries: &mut Vec<FdbEntry>,
    args: &[String],
) -> Result<(), String> {
    if args.is_empty() {
        return Err("missing MAC address".to_string());
    }
    let mac = parse_mac(&args[0]).ok_or_else(|| format!("invalid MAC: {}", args[0]))?;
    let mut dev: Option<&str> = None;
    let mut vlan: Option<u16> = None;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "dev" => {
                i += 1;
                dev = args.get(i).map(|s| s.as_str());
            }
            "vlan" => {
                i += 1;
                vlan = args.get(i).and_then(|s| s.parse::<u16>().ok());
            }
            _ => {}
        }
        i += 1;
    }
    let before = entries.len();
    entries.retain(|e| {
        let mac_match = e.mac == mac;
        let dev_match = dev.is_none() || e.port == dev.unwrap_or("");
        let vlan_match = vlan.is_none() || e.vlan == vlan;
        !(mac_match && dev_match && vlan_match)
    });
    if entries.len() == before {
        return Err("entry not found".to_string());
    }
    Ok(())
}

fn bridge_flush_fdb(entries: &mut Vec<FdbEntry>, args: &[String]) {
    if let Some(dev) = args.iter().position(|a| a == "dev").and_then(|i| args.get(i + 1)) {
        entries.retain(|e| e.port != *dev || e.is_local);
    } else {
        entries.retain(|e| e.is_local);
    }
}

fn bridge_show_mdb(entries: &[MdbEntry], ctx: &OutputCtx, filter_dev: Option<&str>) {
    let filtered: Vec<&MdbEntry> = if let Some(dev) = filter_dev {
        entries.iter().filter(|e| e.port == dev).collect()
    } else {
        entries.iter().collect()
    };

    if ctx.json {
        let indent = if ctx.pretty { "  " } else { "" };
        let nl = if ctx.pretty { "\n" } else { "" };
        write_out(&format!("[{nl}"));
        for (i, e) in filtered.iter().enumerate() {
            let comma = if i + 1 < filtered.len() { "," } else { "" };
            let vlan_str = e
                .vlan
                .map(|v| format!(",{nl}{indent}{indent}\"vlan\": {v}"))
                .unwrap_or_default();
            write_out(&format!(
                "{indent}{{{nl}\
                 {indent}{indent}\"grp\": \"{}\",{nl}\
                 {indent}{indent}\"port\": \"{}\",{nl}\
                 {indent}{indent}\"permanent\": {}{vlan_str}{nl}\
                 {indent}}}{comma}{nl}",
                e.group, e.port, e.is_permanent,
            ));
        }
        write_out(&format!("]{nl}"));
    } else {
        write_out("dev br0 port group\n");
        for e in &filtered {
            let perm = if e.is_permanent { "permanent" } else { "temp" };
            let vlan_str = e
                .vlan
                .map(|v| format!(" vlan {v}"))
                .unwrap_or_default();
            write_out(&format!(
                "  {} {} {}{vlan_str}\n",
                e.port, e.group, perm,
            ));
        }
    }
}

fn bridge_add_mdb(
    entries: &mut Vec<MdbEntry>,
    args: &[String],
) -> Result<(), String> {
    // bridge mdb add dev <br> port <port> grp <group> [permanent] [temp] [vlan <vid>]
    let mut dev: Option<&str> = None;
    let mut port: Option<&str> = None;
    let mut group: Option<&str> = None;
    let mut vlan: Option<u16> = None;
    let mut permanent = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "dev" => {
                i += 1;
                dev = args.get(i).map(|s| s.as_str());
            }
            "port" => {
                i += 1;
                port = args.get(i).map(|s| s.as_str());
            }
            "grp" => {
                i += 1;
                group = args.get(i).map(|s| s.as_str());
            }
            "vlan" => {
                i += 1;
                vlan = args.get(i).and_then(|s| s.parse::<u16>().ok());
            }
            "permanent" => permanent = true,
            "temp" => permanent = false,
            _ => {}
        }
        i += 1;
    }
    let _ = dev.ok_or_else(|| "missing bridge device".to_string())?;
    let p = port.ok_or_else(|| "missing port".to_string())?;
    let g = group.ok_or_else(|| "missing group".to_string())?;
    entries.push(MdbEntry {
        group: g.to_string(),
        port: p.to_string(),
        vlan,
        is_permanent: permanent,
    });
    Ok(())
}

fn bridge_del_mdb(
    entries: &mut Vec<MdbEntry>,
    args: &[String],
) -> Result<(), String> {
    let mut port: Option<&str> = None;
    let mut group: Option<&str> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "port" => {
                i += 1;
                port = args.get(i).map(|s| s.as_str());
            }
            "grp" => {
                i += 1;
                group = args.get(i).map(|s| s.as_str());
            }
            "dev" => {
                i += 1;
                // consume but don't use for filtering beyond port/grp
            }
            _ => {}
        }
        i += 1;
    }
    let g = group.ok_or_else(|| "missing group".to_string())?;
    let before = entries.len();
    entries.retain(|e| {
        let grp_match = e.group == g;
        let port_match = port.is_none() || e.port == port.unwrap_or("");
        !(grp_match && port_match)
    });
    if entries.len() == before {
        return Err("entry not found".to_string());
    }
    Ok(())
}

fn bridge_show_vlan(entries: &[VlanEntry], ctx: &OutputCtx, filter_dev: Option<&str>) {
    let filtered: Vec<&VlanEntry> = if let Some(dev) = filter_dev {
        entries.iter().filter(|e| e.port == dev).collect()
    } else {
        entries.iter().collect()
    };

    if ctx.json {
        let indent = if ctx.pretty { "  " } else { "" };
        let nl = if ctx.pretty { "\n" } else { "" };
        write_out(&format!("[{nl}"));
        for (i, e) in filtered.iter().enumerate() {
            let comma = if i + 1 < filtered.len() { "," } else { "" };
            let flags = build_vlan_flags_json(e.pvid, e.untagged);
            write_out(&format!(
                "{indent}{{{nl}\
                 {indent}{indent}\"port\": \"{}\",{nl}\
                 {indent}{indent}\"vid\": {},{nl}\
                 {indent}{indent}\"flags\": [{flags}]{nl}\
                 {indent}}}{comma}{nl}",
                e.port, e.vid,
            ));
        }
        write_out(&format!("]{nl}"));
    } else {
        write_out("port\tvlan ids\n");
        for e in &filtered {
            let flags = build_vlan_flags_text(e.pvid, e.untagged);
            let range = if let Some(end) = e.vid_end {
                format!("{}-{}", e.vid, end)
            } else {
                format!("{}", e.vid)
            };
            write_out(&format!("{}\t {}{flags}\n", e.port, range));
        }
    }
}

fn build_vlan_flags_json(pvid: bool, untagged: bool) -> String {
    let mut flags = Vec::new();
    if pvid {
        flags.push("\"PVID\"");
    }
    if untagged {
        flags.push("\"Egress Untagged\"");
    }
    flags.join(", ")
}

fn build_vlan_flags_text(pvid: bool, untagged: bool) -> String {
    let mut s = String::new();
    if pvid {
        s.push_str(" PVID");
    }
    if untagged {
        s.push_str(" Egress Untagged");
    }
    s
}

fn bridge_add_vlan(
    entries: &mut Vec<VlanEntry>,
    args: &[String],
) -> Result<(), String> {
    // bridge vlan add dev <dev> vid <vid> [pvid] [untagged] [self] [master]
    let mut dev: Option<&str> = None;
    let mut vid: Option<u16> = None;
    let mut pvid = false;
    let mut untagged = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "dev" => {
                i += 1;
                dev = args.get(i).map(|s| s.as_str());
            }
            "vid" => {
                i += 1;
                vid = args.get(i).and_then(|s| s.parse::<u16>().ok());
            }
            "pvid" => pvid = true,
            "untagged" => untagged = true,
            "self" | "master" => {}
            _ => {}
        }
        i += 1;
    }
    let port = dev.ok_or_else(|| "missing dev".to_string())?;
    let v = vid.ok_or_else(|| "missing vid".to_string())?;
    entries.push(VlanEntry {
        port: port.to_string(),
        vid: v,
        vid_end: None,
        pvid,
        untagged,
    });
    Ok(())
}

fn bridge_del_vlan(
    entries: &mut Vec<VlanEntry>,
    args: &[String],
) -> Result<(), String> {
    let mut dev: Option<&str> = None;
    let mut vid: Option<u16> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "dev" => {
                i += 1;
                dev = args.get(i).map(|s| s.as_str());
            }
            "vid" => {
                i += 1;
                vid = args.get(i).and_then(|s| s.parse::<u16>().ok());
            }
            _ => {}
        }
        i += 1;
    }
    let port = dev.ok_or_else(|| "missing dev".to_string())?;
    let v = vid.ok_or_else(|| "missing vid".to_string())?;
    let before = entries.len();
    entries.retain(|e| !(e.port == port && e.vid == v));
    if entries.len() == before {
        return Err(format!("vlan {v} not found on {port}"));
    }
    Ok(())
}

fn bridge_monitor() {
    write_out("Monitoring bridge events... (press Ctrl-C to stop)\n");
    write_out("[BRIDGE] link change: eth0 state forwarding\n");
    write_out("[BRIDGE] fdb add: 00:11:22:33:44:55 dev eth0\n");
}

// ============================================================================
// Bridge main dispatch
// ============================================================================

fn run_bridge(args: &[String]) -> i32 {
    let mut ctx = OutputCtx::new();
    let mut positional: Vec<String> = Vec::new();

    for arg in args {
        match arg.as_str() {
            "-j" | "--json" => ctx.json = true,
            "-p" | "--pretty" => ctx.pretty = true,
            "-s" | "--statistics" => ctx.stats = true,
            "-d" | "--details" => ctx.details = true,
            "--version" | "-V" => {
                write_out(&format!("bridge utility, Slate OS v{VERSION}\n"));
                return 0;
            }
            "--help" | "-h" => {
                bridge_usage();
                return 0;
            }
            _ => positional.push(arg.clone()),
        }
    }

    if positional.is_empty() {
        bridge_usage();
        return 1;
    }

    let subcmd = positional[0].as_str();
    let rest = &positional[1..];

    match subcmd {
        "link" => bridge_dispatch_link(rest, &ctx),
        "fdb" => bridge_dispatch_fdb(rest, &ctx),
        "mdb" => bridge_dispatch_mdb(rest, &ctx),
        "vlan" => bridge_dispatch_vlan(rest, &ctx),
        "monitor" => {
            bridge_monitor();
            0
        }
        _ => {
            write_err(&format!("bridge: unknown object \"{subcmd}\"\n"));
            1
        }
    }
}

fn bridge_dispatch_link(args: &[String], ctx: &OutputCtx) -> i32 {
    let action = args.first().map(|s| s.as_str()).unwrap_or("show");
    match action {
        "show" | "list" | "ls" => {
            let ports = sample_bridge_ports();
            let filter = args.get(1).and_then(|a| {
                if a == "dev" {
                    args.get(2).map(|s| s.as_str())
                } else {
                    Some(a.as_str())
                }
            });
            bridge_show_link(&ports, ctx, filter);
            0
        }
        "set" => {
            let mut ports = sample_bridge_ports();
            match bridge_set_link(&mut ports, &args[1..]) {
                Ok(()) => 0,
                Err(e) => {
                    write_err(&format!("bridge: {e}\n"));
                    1
                }
            }
        }
        _ => {
            write_err(&format!("bridge: link: unknown action \"{action}\"\n"));
            1
        }
    }
}

fn bridge_dispatch_fdb(args: &[String], ctx: &OutputCtx) -> i32 {
    let action = args.first().map(|s| s.as_str()).unwrap_or("show");
    match action {
        "show" | "list" => {
            let entries = sample_fdb_entries();
            let br = args.get(1).and_then(|a| {
                if a == "br" || a == "dev" {
                    args.get(2).map(|s| s.as_str())
                } else {
                    None
                }
            });
            bridge_show_fdb(&entries, ctx, br);
            0
        }
        "add" | "append" | "replace" => {
            let mut entries = sample_fdb_entries();
            match bridge_add_fdb(&mut entries, &args[1..]) {
                Ok(()) => 0,
                Err(e) => {
                    write_err(&format!("bridge: fdb: {e}\n"));
                    1
                }
            }
        }
        "del" | "delete" => {
            let mut entries = sample_fdb_entries();
            match bridge_del_fdb(&mut entries, &args[1..]) {
                Ok(()) => 0,
                Err(e) => {
                    write_err(&format!("bridge: fdb: {e}\n"));
                    1
                }
            }
        }
        "flush" => {
            let mut entries = sample_fdb_entries();
            bridge_flush_fdb(&mut entries, &args[1..]);
            0
        }
        _ => {
            write_err(&format!("bridge: fdb: unknown action \"{action}\"\n"));
            1
        }
    }
}

fn bridge_dispatch_mdb(args: &[String], ctx: &OutputCtx) -> i32 {
    let action = args.first().map(|s| s.as_str()).unwrap_or("show");
    match action {
        "show" | "list" => {
            let entries = sample_mdb_entries();
            let dev = args.iter().position(|a| a == "dev").and_then(|i| args.get(i + 1)).map(|s| s.as_str());
            bridge_show_mdb(&entries, ctx, dev);
            0
        }
        "add" => {
            let mut entries = sample_mdb_entries();
            match bridge_add_mdb(&mut entries, &args[1..]) {
                Ok(()) => 0,
                Err(e) => {
                    write_err(&format!("bridge: mdb: {e}\n"));
                    1
                }
            }
        }
        "del" | "delete" => {
            let mut entries = sample_mdb_entries();
            match bridge_del_mdb(&mut entries, &args[1..]) {
                Ok(()) => 0,
                Err(e) => {
                    write_err(&format!("bridge: mdb: {e}\n"));
                    1
                }
            }
        }
        _ => {
            write_err(&format!("bridge: mdb: unknown action \"{action}\"\n"));
            1
        }
    }
}

fn bridge_dispatch_vlan(args: &[String], ctx: &OutputCtx) -> i32 {
    let action = args.first().map(|s| s.as_str()).unwrap_or("show");
    match action {
        "show" | "list" => {
            let entries = sample_vlan_entries();
            let dev = args.iter().position(|a| a == "dev").and_then(|i| args.get(i + 1)).map(|s| s.as_str());
            bridge_show_vlan(&entries, ctx, dev);
            0
        }
        "add" => {
            let mut entries = sample_vlan_entries();
            match bridge_add_vlan(&mut entries, &args[1..]) {
                Ok(()) => 0,
                Err(e) => {
                    write_err(&format!("bridge: vlan: {e}\n"));
                    1
                }
            }
        }
        "del" | "delete" => {
            let mut entries = sample_vlan_entries();
            match bridge_del_vlan(&mut entries, &args[1..]) {
                Ok(()) => 0,
                Err(e) => {
                    write_err(&format!("bridge: vlan: {e}\n"));
                    1
                }
            }
        }
        _ => {
            write_err(&format!("bridge: vlan: unknown action \"{action}\"\n"));
            1
        }
    }
}

fn bridge_usage() {
    write_out("Usage: bridge [ OPTIONS ] OBJECT { COMMAND | help }\n");
    write_out("where  OBJECT := { link | fdb | mdb | vlan | monitor }\n");
    write_out("       OPTIONS := { -j[son] | -p[retty] | -s[tatistics] | -d[etails] }\n");
}

// ============================================================================
// Sample data (for display when no real kernel state is available)
// ============================================================================

fn sample_bridge_ports() -> Vec<BridgePort> {
    vec![
        BridgePort::new("eth0", "br0"),
        BridgePort::new("eth1", "br0"),
    ]
}

fn sample_fdb_entries() -> Vec<FdbEntry> {
    vec![
        FdbEntry {
            mac: [0x00, 0x11, 0x22, 0x33, 0x44, 0x55],
            port: "eth0".to_string(),
            vlan: None,
            is_local: true,
            is_static: true,
            offloaded: false,
        },
        FdbEntry {
            mac: [0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF],
            port: "eth1".to_string(),
            vlan: Some(100),
            is_local: false,
            is_static: false,
            offloaded: true,
        },
    ]
}

fn sample_mdb_entries() -> Vec<MdbEntry> {
    vec![MdbEntry {
        group: "239.1.1.1".to_string(),
        port: "eth0".to_string(),
        vlan: None,
        is_permanent: true,
    }]
}

fn sample_vlan_entries() -> Vec<VlanEntry> {
    vec![
        VlanEntry {
            port: "eth0".to_string(),
            vid: 1,
            vid_end: None,
            pvid: true,
            untagged: true,
        },
        VlanEntry {
            port: "eth0".to_string(),
            vid: 100,
            vid_end: None,
            pvid: false,
            untagged: false,
        },
    ]
}

// ============================================================================
// TC data structures
// ============================================================================

#[derive(Debug, Clone)]
enum QdiscKind {
    PfifoFast,
    Tbf {
        rate: u64,
        burst: u64,
        latency: u64,
        peakrate: Option<u64>,
        mtu: Option<u64>,
    },
    Htb {
        default_class: u32,
    },
    Sfq {
        perturb: u32,
        quantum: u32,
    },
    FqCodel {
        target: u64,
        interval: u64,
        quantum: u32,
        limit: u32,
    },
    Ingress,
    Netem {
        delay: u64,
        jitter: Option<u64>,
        loss: Option<f64>,
        duplicate: Option<f64>,
        corrupt: Option<f64>,
        reorder: Option<f64>,
    },
}

#[derive(Debug, Clone)]
struct Qdisc {
    handle: u32,
    parent: u32,
    dev: String,
    kind: QdiscKind,
    bytes: u64,
    packets: u64,
    drops: u64,
    overlimits: u64,
}

#[derive(Debug, Clone)]
struct TcClass {
    handle: u32,
    parent: u32,
    dev: String,
    kind: String,
    rate: u64,
    ceil: u64,
    burst: u64,
    cburst: u64,
    prio: u32,
    bytes: u64,
    packets: u64,
}

#[derive(Debug, Clone)]
enum FilterKind {
    U32 { match_field: String, match_value: String, mask: String },
    Fw { fwmark: u32 },
    Basic { expr: String },
    Matchall,
}

#[derive(Debug, Clone)]
struct TcFilter {
    parent: u32,
    dev: String,
    prio: u32,
    protocol: String,
    kind: FilterKind,
    flowid: Option<u32>,
    action: Option<String>,
}

// ============================================================================
// TC implementation
// ============================================================================

fn tc_show_qdisc(qdiscs: &[Qdisc], ctx: &OutputCtx, filter_dev: Option<&str>) {
    let filtered: Vec<&Qdisc> = if let Some(dev) = filter_dev {
        qdiscs.iter().filter(|q| q.dev == dev).collect()
    } else {
        qdiscs.iter().collect()
    };

    if ctx.json {
        let indent = if ctx.pretty { "  " } else { "" };
        let nl = if ctx.pretty { "\n" } else { "" };
        write_out(&format!("[{nl}"));
        for (i, q) in filtered.iter().enumerate() {
            let comma = if i + 1 < filtered.len() { "," } else { "" };
            write_out(&format!(
                "{indent}{{{nl}\
                 {indent}{indent}\"kind\": \"{}\",{nl}\
                 {indent}{indent}\"handle\": \"{}\",{nl}\
                 {indent}{indent}\"parent\": \"{}\",{nl}\
                 {indent}{indent}\"dev\": \"{}\"{nl}\
                 {indent}}}{comma}{nl}",
                qdisc_kind_name(&q.kind),
                format_handle(q.handle),
                format_handle(q.parent),
                q.dev,
            ));
        }
        write_out(&format!("]{nl}"));
    } else {
        for q in &filtered {
            write_out(&format!(
                "qdisc {} {} dev {} parent {}\n",
                qdisc_kind_name(&q.kind),
                format_handle(q.handle),
                q.dev,
                format_handle(q.parent),
            ));
            tc_show_qdisc_params(&q.kind, ctx);
            if ctx.stats {
                write_out(&format!(
                    "  Sent {} bytes {} pkt (dropped {}, overlimits {})\n",
                    q.bytes, q.packets, q.drops, q.overlimits,
                ));
            }
        }
    }
}

fn tc_show_qdisc_params(kind: &QdiscKind, _ctx: &OutputCtx) {
    match kind {
        QdiscKind::PfifoFast => write_out("  bands 3 priomap 1 2 2 2 1 2 0 0\n"),
        QdiscKind::Tbf { rate, burst, latency, peakrate, mtu } => {
            write_out(&format!(
                "  rate {} burst {} latency {}\n",
                format_rate(*rate),
                *burst,
                format_time(*latency),
            ));
            if let Some(pr) = peakrate {
                write_out(&format!("  peakrate {}\n", format_rate(*pr)));
            }
            if let Some(m) = mtu {
                write_out(&format!("  mtu {m}\n"));
            }
        }
        QdiscKind::Htb { default_class } => {
            write_out(&format!("  default {}\n", format_handle(*default_class)));
        }
        QdiscKind::Sfq { perturb, quantum } => {
            write_out(&format!("  perturb {perturb}sec quantum {quantum}\n"));
        }
        QdiscKind::FqCodel { target, interval, quantum, limit } => {
            write_out(&format!(
                "  target {} interval {} quantum {quantum} limit {limit}\n",
                format_time(*target),
                format_time(*interval),
            ));
        }
        QdiscKind::Ingress => write_out("  -------- (ingress) ----------\n"),
        QdiscKind::Netem { delay, jitter, loss, duplicate, corrupt, reorder } => {
            write_out(&format!("  delay {}", format_time(*delay)));
            if let Some(j) = jitter {
                write_out(&format!(" {}", format_time(*j)));
            }
            write_out("\n");
            if let Some(l) = loss {
                write_out(&format!("  loss {l:.1}%\n"));
            }
            if let Some(d) = duplicate {
                write_out(&format!("  duplicate {d:.1}%\n"));
            }
            if let Some(c) = corrupt {
                write_out(&format!("  corrupt {c:.1}%\n"));
            }
            if let Some(r) = reorder {
                write_out(&format!("  reorder {r:.1}%\n"));
            }
        }
    }
}

fn qdisc_kind_name(kind: &QdiscKind) -> &'static str {
    match kind {
        QdiscKind::PfifoFast => "pfifo_fast",
        QdiscKind::Tbf { .. } => "tbf",
        QdiscKind::Htb { .. } => "htb",
        QdiscKind::Sfq { .. } => "sfq",
        QdiscKind::FqCodel { .. } => "fq_codel",
        QdiscKind::Ingress => "ingress",
        QdiscKind::Netem { .. } => "netem",
    }
}

fn tc_parse_qdisc(args: &[String]) -> Result<(String, u32, QdiscKind), String> {
    let mut dev: Option<&str> = None;
    let mut parent: Option<u32> = None;
    let mut handle: Option<u32> = None;
    let mut kind_name: Option<&str> = None;
    let mut i = 0;

    while i < args.len() {
        match args[i].as_str() {
            "dev" => {
                i += 1;
                dev = args.get(i).map(|s| s.as_str());
            }
            "parent" => {
                i += 1;
                parent = args.get(i).and_then(|s| parse_handle(s));
            }
            "root" => {
                parent = Some(TC_H_ROOT);
            }
            "ingress" => {
                kind_name = Some("ingress");
                parent = Some(TC_H_INGRESS);
            }
            "handle" => {
                i += 1;
                handle = args.get(i).and_then(|s| parse_handle(s));
            }
            "pfifo_fast" | "tbf" | "htb" | "sfq" | "fq_codel" | "netem" => {
                kind_name = Some(args[i].as_str());
            }
            _ => {}
        }
        i += 1;
    }

    let device = dev.ok_or_else(|| "missing dev".to_string())?;
    let p = parent.unwrap_or(TC_H_ROOT);
    let _ = handle; // used by kernel, we just parse it

    let kind = match kind_name {
        Some("pfifo_fast") | None => QdiscKind::PfifoFast,
        Some("tbf") => tc_parse_tbf_params(args)?,
        Some("htb") => tc_parse_htb_params(args),
        Some("sfq") => tc_parse_sfq_params(args),
        Some("fq_codel") => tc_parse_fqcodel_params(args),
        Some("ingress") => QdiscKind::Ingress,
        Some("netem") => tc_parse_netem_params(args)?,
        Some(other) => return Err(format!("unknown qdisc: {other}")),
    };

    Ok((device.to_string(), p, kind))
}

fn tc_parse_tbf_params(args: &[String]) -> Result<QdiscKind, String> {
    let mut rate: Option<u64> = None;
    let mut burst: Option<u64> = None;
    let mut latency: Option<u64> = None;
    let mut peakrate: Option<u64> = None;
    let mut mtu: Option<u64> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "rate" => {
                i += 1;
                rate = args.get(i).and_then(|s| parse_rate(s));
            }
            "burst" | "buffer" | "maxburst" => {
                i += 1;
                burst = args.get(i).and_then(|s| parse_size(s));
            }
            "latency" => {
                i += 1;
                latency = args.get(i).and_then(|s| parse_time(s));
            }
            "peakrate" => {
                i += 1;
                peakrate = args.get(i).and_then(|s| parse_rate(s));
            }
            "mtu" | "minburst" => {
                i += 1;
                mtu = args.get(i).and_then(|s| parse_size(s));
            }
            _ => {}
        }
        i += 1;
    }
    Ok(QdiscKind::Tbf {
        rate: rate.unwrap_or(1_000_000),
        burst: burst.unwrap_or(1600),
        latency: latency.unwrap_or(50_000),
        peakrate,
        mtu,
    })
}

fn tc_parse_htb_params(args: &[String]) -> QdiscKind {
    let mut default_class = 0u32;
    let mut i = 0;
    while i < args.len() {
        if args[i] == "default" {
            i += 1;
            if let Some(s) = args.get(i) {
                default_class = parse_handle(s).unwrap_or(0);
            }
        }
        i += 1;
    }
    QdiscKind::Htb { default_class }
}

fn tc_parse_sfq_params(args: &[String]) -> QdiscKind {
    let mut perturb = 10u32;
    let mut quantum = 1514u32;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "perturb" => {
                i += 1;
                perturb = args.get(i).and_then(|s| s.parse().ok()).unwrap_or(10);
            }
            "quantum" => {
                i += 1;
                quantum = args.get(i).and_then(|s| s.parse().ok()).unwrap_or(1514);
            }
            _ => {}
        }
        i += 1;
    }
    QdiscKind::Sfq { perturb, quantum }
}

fn tc_parse_fqcodel_params(args: &[String]) -> QdiscKind {
    let mut target = 5000u64; // 5ms in us
    let mut interval = 100_000u64; // 100ms in us
    let mut quantum = 1514u32;
    let mut limit = 10240u32;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "target" => {
                i += 1;
                target = args.get(i).and_then(|s| parse_time(s)).unwrap_or(5000);
            }
            "interval" => {
                i += 1;
                interval = args.get(i).and_then(|s| parse_time(s)).unwrap_or(100_000);
            }
            "quantum" => {
                i += 1;
                quantum = args.get(i).and_then(|s| s.parse().ok()).unwrap_or(1514);
            }
            "limit" => {
                i += 1;
                limit = args.get(i).and_then(|s| s.parse().ok()).unwrap_or(10240);
            }
            _ => {}
        }
        i += 1;
    }
    QdiscKind::FqCodel { target, interval, quantum, limit }
}

fn tc_parse_netem_params(args: &[String]) -> Result<QdiscKind, String> {
    let mut delay = 0u64;
    let mut jitter: Option<u64> = None;
    let mut loss: Option<f64> = None;
    let mut duplicate: Option<f64> = None;
    let mut corrupt: Option<f64> = None;
    let mut reorder: Option<f64> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "delay" => {
                i += 1;
                delay = args.get(i).and_then(|s| parse_time(s)).unwrap_or(0);
                // Check for jitter (next arg might be a time value)
                if let Some(next) = args.get(i + 1)
                    && let Some(j) = parse_time(next) {
                        // Only treat as jitter if it looks like a time value
                        // and not a keyword
                        if !is_netem_keyword(next) {
                            jitter = Some(j);
                            i += 1;
                        }
                    }
            }
            "jitter" => {
                i += 1;
                jitter = args.get(i).and_then(|s| parse_time(s));
            }
            "loss" => {
                i += 1;
                loss = args.get(i).and_then(|s| parse_percent(s));
            }
            "duplicate" => {
                i += 1;
                duplicate = args.get(i).and_then(|s| parse_percent(s));
            }
            "corrupt" => {
                i += 1;
                corrupt = args.get(i).and_then(|s| parse_percent(s));
            }
            "reorder" => {
                i += 1;
                reorder = args.get(i).and_then(|s| parse_percent(s));
            }
            _ => {}
        }
        i += 1;
    }
    Ok(QdiscKind::Netem { delay, jitter, loss, duplicate, corrupt, reorder })
}

fn is_netem_keyword(s: &str) -> bool {
    matches!(
        s,
        "delay" | "jitter" | "loss" | "duplicate" | "corrupt" | "reorder"
            | "dev" | "parent" | "root" | "handle"
    )
}

fn parse_percent(s: &str) -> Option<f64> {
    let s = s.strip_suffix('%').unwrap_or(s);
    s.parse::<f64>().ok()
}

fn tc_show_class(classes: &[TcClass], ctx: &OutputCtx, filter_dev: Option<&str>) {
    let filtered: Vec<&TcClass> = if let Some(dev) = filter_dev {
        classes.iter().filter(|c| c.dev == dev).collect()
    } else {
        classes.iter().collect()
    };

    if ctx.json {
        let indent = if ctx.pretty { "  " } else { "" };
        let nl = if ctx.pretty { "\n" } else { "" };
        write_out(&format!("[{nl}"));
        for (i, c) in filtered.iter().enumerate() {
            let comma = if i + 1 < filtered.len() { "," } else { "" };
            write_out(&format!(
                "{indent}{{{nl}\
                 {indent}{indent}\"kind\": \"{}\",{nl}\
                 {indent}{indent}\"handle\": \"{}\",{nl}\
                 {indent}{indent}\"parent\": \"{}\",{nl}\
                 {indent}{indent}\"rate\": {},{nl}\
                 {indent}{indent}\"ceil\": {}{nl}\
                 {indent}}}{comma}{nl}",
                c.kind,
                format_handle(c.handle),
                format_handle(c.parent),
                c.rate,
                c.ceil,
            ));
        }
        write_out(&format!("]{nl}"));
    } else {
        for c in &filtered {
            write_out(&format!(
                "class {} {} dev {} parent {} prio {}\n",
                c.kind,
                format_handle(c.handle),
                c.dev,
                format_handle(c.parent),
                c.prio,
            ));
            write_out(&format!(
                "  rate {} ceil {} burst {} cburst {}\n",
                format_rate(c.rate),
                format_rate(c.ceil),
                c.burst,
                c.cburst,
            ));
            if ctx.stats {
                write_out(&format!(
                    "  Sent {} bytes {} pkt\n",
                    c.bytes, c.packets,
                ));
            }
        }
    }
}

fn tc_parse_class(args: &[String]) -> Result<TcClass, String> {
    let mut dev: Option<&str> = None;
    let mut parent: Option<u32> = None;
    let mut classid: Option<u32> = None;
    let mut rate = 0u64;
    let mut ceil = 0u64;
    let mut burst = 0u64;
    let mut cburst = 0u64;
    let mut prio = 0u32;
    let mut kind = String::new();
    let mut i = 0;

    while i < args.len() {
        match args[i].as_str() {
            "dev" => {
                i += 1;
                dev = args.get(i).map(|s| s.as_str());
            }
            "parent" => {
                i += 1;
                parent = args.get(i).and_then(|s| parse_handle(s));
            }
            "classid" => {
                i += 1;
                classid = args.get(i).and_then(|s| parse_handle(s));
            }
            "htb" | "hfsc" | "cbq" => {
                kind = args[i].clone();
            }
            "rate" => {
                i += 1;
                rate = args.get(i).and_then(|s| parse_rate(s)).unwrap_or(0);
            }
            "ceil" => {
                i += 1;
                ceil = args.get(i).and_then(|s| parse_rate(s)).unwrap_or(0);
            }
            "burst" => {
                i += 1;
                burst = args.get(i).and_then(|s| parse_size(s)).unwrap_or(0);
            }
            "cburst" => {
                i += 1;
                cburst = args.get(i).and_then(|s| parse_size(s)).unwrap_or(0);
            }
            "prio" => {
                i += 1;
                prio = args.get(i).and_then(|s| s.parse().ok()).unwrap_or(0);
            }
            _ => {}
        }
        i += 1;
    }

    let device = dev.ok_or_else(|| "missing dev".to_string())?;
    if ceil == 0 {
        ceil = rate;
    }

    Ok(TcClass {
        handle: classid.unwrap_or(0),
        parent: parent.unwrap_or(TC_H_ROOT),
        dev: device.to_string(),
        kind,
        rate,
        ceil,
        burst,
        cburst,
        prio,
        bytes: 0,
        packets: 0,
    })
}

fn tc_show_filter(filters: &[TcFilter], ctx: &OutputCtx, filter_dev: Option<&str>) {
    let filtered: Vec<&TcFilter> = if let Some(dev) = filter_dev {
        filters.iter().filter(|f| f.dev == dev).collect()
    } else {
        filters.iter().collect()
    };

    if ctx.json {
        let indent = if ctx.pretty { "  " } else { "" };
        let nl = if ctx.pretty { "\n" } else { "" };
        write_out(&format!("[{nl}"));
        for (i, f) in filtered.iter().enumerate() {
            let comma = if i + 1 < filtered.len() { "," } else { "" };
            write_out(&format!(
                "{indent}{{{nl}\
                 {indent}{indent}\"kind\": \"{}\",{nl}\
                 {indent}{indent}\"protocol\": \"{}\",{nl}\
                 {indent}{indent}\"prio\": {},{nl}\
                 {indent}{indent}\"parent\": \"{}\"{nl}\
                 {indent}}}{comma}{nl}",
                filter_kind_name(&f.kind),
                f.protocol,
                f.prio,
                format_handle(f.parent),
            ));
        }
        write_out(&format!("]{nl}"));
    } else {
        for f in &filtered {
            write_out(&format!(
                "filter parent {} protocol {} pref {} {} \n",
                format_handle(f.parent),
                f.protocol,
                f.prio,
                filter_kind_name(&f.kind),
            ));
            tc_show_filter_params(&f.kind);
            if let Some(ref flowid) = f.flowid {
                write_out(&format!("  flowid {}\n", format_handle(*flowid)));
            }
            if let Some(ref action) = f.action {
                write_out(&format!("  action {action}\n"));
            }
        }
    }
}

fn filter_kind_name(kind: &FilterKind) -> &'static str {
    match kind {
        FilterKind::U32 { .. } => "u32",
        FilterKind::Fw { .. } => "fw",
        FilterKind::Basic { .. } => "basic",
        FilterKind::Matchall => "matchall",
    }
}

fn tc_show_filter_params(kind: &FilterKind) {
    match kind {
        FilterKind::U32 { match_field, match_value, mask } => {
            write_out(&format!("  match {match_field} {match_value} mask {mask}\n"));
        }
        FilterKind::Fw { fwmark } => {
            write_out(&format!("  handle 0x{fwmark:x} fw\n"));
        }
        FilterKind::Basic { expr } => {
            write_out(&format!("  {expr}\n"));
        }
        FilterKind::Matchall => {
            write_out("  not_in_hw\n");
        }
    }
}

fn tc_parse_filter(args: &[String]) -> Result<TcFilter, String> {
    let mut dev: Option<&str> = None;
    let mut parent: Option<u32> = None;
    let mut prio = 0u32;
    let mut protocol = "ip".to_string();
    let mut kind: Option<FilterKind> = None;
    let mut flowid: Option<u32> = None;
    let mut action: Option<String> = None;
    let mut i = 0;

    while i < args.len() {
        match args[i].as_str() {
            "dev" => {
                i += 1;
                dev = args.get(i).map(|s| s.as_str());
            }
            "parent" => {
                i += 1;
                parent = args.get(i).and_then(|s| parse_handle(s));
            }
            "prio" | "preference" | "pref" => {
                i += 1;
                prio = args.get(i).and_then(|s| s.parse().ok()).unwrap_or(0);
            }
            "protocol" => {
                i += 1;
                if let Some(s) = args.get(i) {
                    protocol = s.clone();
                }
            }
            "u32" => {
                kind = Some(tc_parse_u32_filter(&args[i + 1..]));
            }
            "fw" => {
                kind = Some(tc_parse_fw_filter(&args[i + 1..]));
            }
            "basic" => {
                kind = Some(tc_parse_basic_filter(&args[i + 1..]));
            }
            "matchall" => {
                kind = Some(FilterKind::Matchall);
            }
            "flowid" | "classid" => {
                i += 1;
                flowid = args.get(i).and_then(|s| parse_handle(s));
            }
            "action" => {
                i += 1;
                action = args.get(i).cloned();
            }
            _ => {}
        }
        i += 1;
    }

    let device = dev.ok_or_else(|| "missing dev".to_string())?;
    let filter_kind = kind.unwrap_or(FilterKind::Matchall);

    Ok(TcFilter {
        parent: parent.unwrap_or(TC_H_ROOT),
        dev: device.to_string(),
        prio,
        protocol,
        kind: filter_kind,
        flowid,
        action,
    })
}

fn tc_parse_u32_filter(args: &[String]) -> FilterKind {
    let mut match_field = String::new();
    let mut match_value = String::new();
    let mut mask = "0xffffffff".to_string();
    let mut i = 0;
    while i < args.len() {
        if args[i] == "match" {
            if let Some(field) = args.get(i + 1) {
                match_field = field.clone();
            }
            if let Some(val) = args.get(i + 2) {
                match_value = val.clone();
            }
            if let Some(m) = args.get(i + 3) {
                mask = m.clone();
            }
            break;
        }
        i += 1;
    }
    FilterKind::U32 { match_field, match_value, mask }
}

fn tc_parse_fw_filter(args: &[String]) -> FilterKind {
    let mut fwmark = 0u32;
    for (i, arg) in args.iter().enumerate() {
        if arg == "handle" {
            if let Some(h) = args.get(i + 1) {
                fwmark = h.strip_prefix("0x").and_then(|s| u32::from_str_radix(s, 16).ok())
                    .or_else(|| h.parse::<u32>().ok())
                    .unwrap_or(0);
            }
            break;
        }
    }
    FilterKind::Fw { fwmark }
}

fn tc_parse_basic_filter(args: &[String]) -> FilterKind {
    let expr = if args.is_empty() {
        "match all".to_string()
    } else {
        args.iter()
            .take_while(|a| a.as_str() != "flowid" && a.as_str() != "action")
            .cloned()
            .collect::<Vec<_>>()
            .join(" ")
    };
    FilterKind::Basic { expr }
}

// ============================================================================
// TC main dispatch
// ============================================================================

fn run_tc(args: &[String]) -> i32 {
    let mut ctx = OutputCtx::new();
    let mut positional: Vec<String> = Vec::new();

    for arg in args {
        match arg.as_str() {
            "-s" | "--statistics" | "-stats" => ctx.stats = true,
            "-d" | "--details" => ctx.details = true,
            "-j" | "--json" => ctx.json = true,
            "-p" | "--pretty" => ctx.pretty = true,
            "--version" | "-V" => {
                write_out(&format!("tc utility, Slate OS v{VERSION}\n"));
                return 0;
            }
            "--help" | "-h" => {
                tc_usage();
                return 0;
            }
            _ => positional.push(arg.clone()),
        }
    }

    if positional.is_empty() {
        tc_usage();
        return 1;
    }

    let obj = positional[0].as_str();
    let rest = &positional[1..];

    match obj {
        "qdisc" => tc_dispatch_qdisc(rest, &ctx),
        "class" => tc_dispatch_class(rest, &ctx),
        "filter" => tc_dispatch_filter(rest, &ctx),
        _ => {
            write_err(&format!("tc: unknown object \"{obj}\"\n"));
            1
        }
    }
}

fn tc_dispatch_qdisc(args: &[String], ctx: &OutputCtx) -> i32 {
    let action = args.first().map(|s| s.as_str()).unwrap_or("show");
    match action {
        "show" | "list" | "ls" => {
            let qdiscs = sample_qdiscs();
            let dev = args.iter().position(|a| a == "dev").and_then(|i| args.get(i + 1)).map(|s| s.as_str());
            tc_show_qdisc(&qdiscs, ctx, dev);
            0
        }
        "add" | "replace" | "change" => {
            match tc_parse_qdisc(&args[1..]) {
                Ok((dev, _parent, kind)) => {
                    write_out(&format!(
                        "qdisc {} added on {dev}\n",
                        qdisc_kind_name(&kind),
                    ));
                    0
                }
                Err(e) => {
                    write_err(&format!("tc: qdisc: {e}\n"));
                    1
                }
            }
        }
        "del" | "delete" => {
            let dev = args.iter().position(|a| a == "dev").and_then(|i| args.get(i + 1));
            if let Some(d) = dev {
                write_out(&format!("qdisc deleted on {d}\n"));
                0
            } else {
                write_err("tc: qdisc del: missing dev\n");
                1
            }
        }
        _ => {
            write_err(&format!("tc: qdisc: unknown action \"{action}\"\n"));
            1
        }
    }
}

fn tc_dispatch_class(args: &[String], ctx: &OutputCtx) -> i32 {
    let action = args.first().map(|s| s.as_str()).unwrap_or("show");
    match action {
        "show" | "list" | "ls" => {
            let classes = sample_classes();
            let dev = args.iter().position(|a| a == "dev").and_then(|i| args.get(i + 1)).map(|s| s.as_str());
            tc_show_class(&classes, ctx, dev);
            0
        }
        "add" | "replace" | "change" => {
            match tc_parse_class(&args[1..]) {
                Ok(class) => {
                    write_out(&format!(
                        "class {} {} added on {}\n",
                        class.kind,
                        format_handle(class.handle),
                        class.dev,
                    ));
                    0
                }
                Err(e) => {
                    write_err(&format!("tc: class: {e}\n"));
                    1
                }
            }
        }
        "del" | "delete" => {
            let dev = args.iter().position(|a| a == "dev").and_then(|i| args.get(i + 1));
            if let Some(d) = dev {
                write_out(&format!("class deleted on {d}\n"));
                0
            } else {
                write_err("tc: class del: missing dev\n");
                1
            }
        }
        _ => {
            write_err(&format!("tc: class: unknown action \"{action}\"\n"));
            1
        }
    }
}

fn tc_dispatch_filter(args: &[String], ctx: &OutputCtx) -> i32 {
    let action = args.first().map(|s| s.as_str()).unwrap_or("show");
    match action {
        "show" | "list" | "ls" => {
            let filters = sample_filters();
            let dev = args.iter().position(|a| a == "dev").and_then(|i| args.get(i + 1)).map(|s| s.as_str());
            tc_show_filter(&filters, ctx, dev);
            0
        }
        "add" | "replace" | "change" => {
            match tc_parse_filter(&args[1..]) {
                Ok(f) => {
                    write_out(&format!(
                        "filter {} added on {}\n",
                        filter_kind_name(&f.kind),
                        f.dev,
                    ));
                    0
                }
                Err(e) => {
                    write_err(&format!("tc: filter: {e}\n"));
                    1
                }
            }
        }
        "del" | "delete" => {
            let dev = args.iter().position(|a| a == "dev").and_then(|i| args.get(i + 1));
            if let Some(d) = dev {
                write_out(&format!("filter deleted on {d}\n"));
                0
            } else {
                write_err("tc: filter del: missing dev\n");
                1
            }
        }
        _ => {
            write_err(&format!("tc: filter: unknown action \"{action}\"\n"));
            1
        }
    }
}

fn tc_usage() {
    write_out("Usage: tc [ OPTIONS ] OBJECT { COMMAND | help }\n");
    write_out("where  OBJECT := { qdisc | class | filter }\n");
    write_out("       OPTIONS := { -s[tatistics] | -d[etails] | -j[son] | -p[retty] }\n");
    write_out("\n");
    write_out("Qdiscs: pfifo_fast, tbf, htb, sfq, fq_codel, ingress, netem\n");
    write_out("Filters: u32, fw, basic, matchall\n");
}

fn sample_qdiscs() -> Vec<Qdisc> {
    vec![
        Qdisc {
            handle: 0x0001_0000,
            parent: TC_H_ROOT,
            dev: "eth0".to_string(),
            kind: QdiscKind::Htb { default_class: 0x0001_0030 },
            bytes: 123456,
            packets: 789,
            drops: 2,
            overlimits: 10,
        },
        Qdisc {
            handle: 0xFFFF_0000,
            parent: TC_H_INGRESS,
            dev: "eth0".to_string(),
            kind: QdiscKind::Ingress,
            bytes: 0,
            packets: 0,
            drops: 0,
            overlimits: 0,
        },
    ]
}

fn sample_classes() -> Vec<TcClass> {
    vec![TcClass {
        handle: 0x0001_0010,
        parent: 0x0001_0000,
        dev: "eth0".to_string(),
        kind: "htb".to_string(),
        rate: 10_000_000,
        ceil: 100_000_000,
        burst: 1600,
        cburst: 1600,
        prio: 0,
        bytes: 5000,
        packets: 50,
    }]
}

fn sample_filters() -> Vec<TcFilter> {
    vec![TcFilter {
        parent: 0x0001_0000,
        dev: "eth0".to_string(),
        prio: 1,
        protocol: "ip".to_string(),
        kind: FilterKind::U32 {
            match_field: "ip".to_string(),
            match_value: "dst".to_string(),
            mask: "0xffffff00".to_string(),
        },
        flowid: Some(0x0001_0010),
        action: None,
    }]
}

// ============================================================================
// Ebtables data structures
// ============================================================================

#[derive(Debug, Clone)]
struct EbtablesRule {
    protocol: Option<String>,
    src_mac: Option<[u8; 6]>,
    dst_mac: Option<[u8; 6]>,
    in_if: Option<String>,
    out_if: Option<String>,
    vlan_id: Option<u16>,
    target: String,
}

#[derive(Debug, Clone)]
struct EbtablesChain {
    name: String,
    policy: String,
    rules: Vec<EbtablesRule>,
    is_builtin: bool,
}

fn default_chains() -> Vec<EbtablesChain> {
    vec![
        EbtablesChain {
            name: CHAIN_INPUT.to_string(),
            policy: TARGET_ACCEPT.to_string(),
            rules: Vec::new(),
            is_builtin: true,
        },
        EbtablesChain {
            name: CHAIN_OUTPUT.to_string(),
            policy: TARGET_ACCEPT.to_string(),
            rules: Vec::new(),
            is_builtin: true,
        },
        EbtablesChain {
            name: CHAIN_FORWARD.to_string(),
            policy: TARGET_ACCEPT.to_string(),
            rules: Vec::new(),
            is_builtin: true,
        },
    ]
}

fn ebt_parse_rule(args: &[String]) -> Result<(String, EbtablesRule), String> {
    // Parse: [-p proto] [-s src] [-d dst] [-i in-if] [-o out-if] [--vlan-id id] -j target
    // The first arg is the chain name.
    if args.is_empty() {
        return Err("missing chain name".to_string());
    }
    let chain = args[0].clone();
    let mut protocol: Option<String> = None;
    let mut src_mac: Option<[u8; 6]> = None;
    let mut dst_mac: Option<[u8; 6]> = None;
    let mut in_if: Option<String> = None;
    let mut out_if: Option<String> = None;
    let mut vlan_id: Option<u16> = None;
    let mut target = TARGET_ACCEPT.to_string();
    let mut i = 1;

    while i < args.len() {
        match args[i].as_str() {
            "-p" | "--protocol" => {
                i += 1;
                protocol = args.get(i).cloned();
            }
            "-s" | "--source" => {
                i += 1;
                src_mac = args.get(i).and_then(|s| parse_mac(s));
            }
            "-d" | "--destination" => {
                i += 1;
                dst_mac = args.get(i).and_then(|s| parse_mac(s));
            }
            "-i" | "--in-interface" => {
                i += 1;
                in_if = args.get(i).cloned();
            }
            "-o" | "--out-interface" => {
                i += 1;
                out_if = args.get(i).cloned();
            }
            "--vlan-id" => {
                i += 1;
                vlan_id = args.get(i).and_then(|s| s.parse::<u16>().ok());
            }
            "-j" | "--jump" => {
                i += 1;
                if let Some(t) = args.get(i) {
                    target = t.clone();
                }
            }
            _ => {}
        }
        i += 1;
    }

    Ok((
        chain,
        EbtablesRule {
            protocol,
            src_mac,
            dst_mac,
            in_if,
            out_if,
            vlan_id,
            target,
        },
    ))
}

fn ebt_format_rule(r: &EbtablesRule) -> String {
    let mut s = String::new();
    if let Some(ref p) = r.protocol {
        s.push_str(&format!("-p {p} "));
    }
    if let Some(ref mac) = r.src_mac {
        s.push_str(&format!("-s {} ", format_mac(mac)));
    }
    if let Some(ref mac) = r.dst_mac {
        s.push_str(&format!("-d {} ", format_mac(mac)));
    }
    if let Some(ref iface) = r.in_if {
        s.push_str(&format!("-i {iface} "));
    }
    if let Some(ref iface) = r.out_if {
        s.push_str(&format!("-o {iface} "));
    }
    if let Some(vid) = r.vlan_id {
        s.push_str(&format!("--vlan-id {vid} "));
    }
    s.push_str(&format!("-j {}", r.target));
    s
}

fn ebt_list_chains(chains: &[EbtablesChain], filter: Option<&str>) {
    for chain in chains {
        if let Some(f) = filter
            && chain.name != f {
                continue;
            }
        write_out(&format!(
            "Bridge chain: {}, entries: {}, policy: {}\n",
            chain.name,
            chain.rules.len(),
            chain.policy,
        ));
        for (i, rule) in chain.rules.iter().enumerate() {
            write_out(&format!("{}. {}\n", i + 1, ebt_format_rule(rule)));
        }
        write_out("\n");
    }
}

fn ebt_add_rule(
    chains: &mut [EbtablesChain],
    chain_name: &str,
    rule: EbtablesRule,
) -> Result<(), String> {
    let chain = chains
        .iter_mut()
        .find(|c| c.name == chain_name)
        .ok_or_else(|| format!("chain {chain_name} not found"))?;
    chain.rules.push(rule);
    Ok(())
}

fn ebt_insert_rule(
    chains: &mut [EbtablesChain],
    chain_name: &str,
    rule: EbtablesRule,
    pos: usize,
) -> Result<(), String> {
    let chain = chains
        .iter_mut()
        .find(|c| c.name == chain_name)
        .ok_or_else(|| format!("chain {chain_name} not found"))?;
    let idx = if pos == 0 { 0 } else { pos.min(chain.rules.len()) };
    chain.rules.insert(idx, rule);
    Ok(())
}

fn ebt_delete_rule(
    chains: &mut [EbtablesChain],
    chain_name: &str,
    rule_num: usize,
) -> Result<(), String> {
    let chain = chains
        .iter_mut()
        .find(|c| c.name == chain_name)
        .ok_or_else(|| format!("chain {chain_name} not found"))?;
    if rule_num == 0 || rule_num > chain.rules.len() {
        return Err(format!("rule number {rule_num} out of range"));
    }
    chain.rules.remove(rule_num - 1);
    Ok(())
}

fn ebt_flush(chains: &mut [EbtablesChain], filter: Option<&str>) {
    for chain in chains.iter_mut() {
        if let Some(f) = filter
            && chain.name != f {
                continue;
            }
        chain.rules.clear();
    }
}

fn ebt_set_policy(
    chains: &mut [EbtablesChain],
    chain_name: &str,
    policy: &str,
) -> Result<(), String> {
    let chain = chains
        .iter_mut()
        .find(|c| c.name == chain_name)
        .ok_or_else(|| format!("chain {chain_name} not found"))?;
    if !chain.is_builtin {
        return Err(format!(
            "cannot set policy on user-defined chain {chain_name}"
        ));
    }
    match policy {
        TARGET_ACCEPT | TARGET_DROP | TARGET_RETURN => {
            chain.policy = policy.to_string();
            Ok(())
        }
        _ => Err(format!("invalid policy: {policy}")),
    }
}

fn ebt_new_chain(
    chains: &mut Vec<EbtablesChain>,
    name: &str,
) -> Result<(), String> {
    if chains.iter().any(|c| c.name == name) {
        return Err(format!("chain {name} already exists"));
    }
    chains.push(EbtablesChain {
        name: name.to_string(),
        policy: TARGET_ACCEPT.to_string(),
        rules: Vec::new(),
        is_builtin: false,
    });
    Ok(())
}

fn ebt_delete_chain(
    chains: &mut Vec<EbtablesChain>,
    name: &str,
) -> Result<(), String> {
    let idx = chains
        .iter()
        .position(|c| c.name == name)
        .ok_or_else(|| format!("chain {name} not found"))?;
    if chains[idx].is_builtin {
        return Err(format!("cannot delete built-in chain {name}"));
    }
    if !chains[idx].rules.is_empty() {
        return Err(format!("chain {name} is not empty"));
    }
    chains.remove(idx);
    Ok(())
}

// ============================================================================
// Ebtables main dispatch
// ============================================================================

fn run_ebtables(args: &[String]) -> i32 {
    if args.is_empty() {
        ebtables_usage();
        return 1;
    }

    let mut chains = default_chains();

    match args[0].as_str() {
        "-h" | "--help" => {
            ebtables_usage();
            0
        }
        "--version" | "-V" => {
            write_out(&format!("ebtables v{VERSION} (Slate OS)\n"));
            0
        }
        "-L" | "--list" => {
            let filter = args.get(1).map(|s| s.as_str());
            ebt_list_chains(&chains, filter);
            0
        }
        "-A" | "--append" => {
            match ebt_parse_rule(&args[1..]) {
                Ok((chain_name, rule)) => match ebt_add_rule(&mut chains, &chain_name, rule) {
                    Ok(()) => 0,
                    Err(e) => {
                        write_err(&format!("ebtables: {e}\n"));
                        1
                    }
                },
                Err(e) => {
                    write_err(&format!("ebtables: {e}\n"));
                    1
                }
            }
        }
        "-I" | "--insert" => {
            // -I chain [rulenum] rule-spec
            let rest = &args[1..];
            if rest.is_empty() {
                write_err("ebtables: missing chain for -I\n");
                return 1;
            }
            // Check if second arg is a number (insert position)
            let pos = rest
                .get(1)
                .and_then(|s| s.parse::<usize>().ok())
                .unwrap_or(0);
            let skip = if rest.get(1).and_then(|s| s.parse::<usize>().ok()).is_some() {
                2
            } else {
                1
            };
            let mut rule_args = vec![rest[0].clone()];
            rule_args.extend_from_slice(&rest[skip..]);
            match ebt_parse_rule(&rule_args) {
                Ok((chain_name, rule)) => {
                    match ebt_insert_rule(&mut chains, &chain_name, rule, pos) {
                        Ok(()) => 0,
                        Err(e) => {
                            write_err(&format!("ebtables: {e}\n"));
                            1
                        }
                    }
                }
                Err(e) => {
                    write_err(&format!("ebtables: {e}\n"));
                    1
                }
            }
        }
        "-D" | "--delete" => {
            if args.len() < 3 {
                write_err("ebtables: missing chain/rule number for -D\n");
                return 1;
            }
            let chain_name = &args[1];
            let rule_num = args[2]
                .parse::<usize>()
                .map_err(|_| "invalid rule number");
            match rule_num {
                Ok(num) => match ebt_delete_rule(&mut chains, chain_name, num) {
                    Ok(()) => 0,
                    Err(e) => {
                        write_err(&format!("ebtables: {e}\n"));
                        1
                    }
                },
                Err(e) => {
                    write_err(&format!("ebtables: {e}\n"));
                    1
                }
            }
        }
        "-F" | "--flush" => {
            let filter = args.get(1).map(|s| s.as_str());
            ebt_flush(&mut chains, filter);
            0
        }
        "-P" | "--policy" => {
            if args.len() < 3 {
                write_err("ebtables: -P requires chain and policy\n");
                return 1;
            }
            match ebt_set_policy(&mut chains, &args[1], &args[2]) {
                Ok(()) => 0,
                Err(e) => {
                    write_err(&format!("ebtables: {e}\n"));
                    1
                }
            }
        }
        "-N" | "--new-chain" => {
            if args.len() < 2 {
                write_err("ebtables: -N requires chain name\n");
                return 1;
            }
            match ebt_new_chain(&mut chains, &args[1]) {
                Ok(()) => 0,
                Err(e) => {
                    write_err(&format!("ebtables: {e}\n"));
                    1
                }
            }
        }
        "-X" | "--delete-chain" => {
            if args.len() < 2 {
                write_err("ebtables: -X requires chain name\n");
                return 1;
            }
            match ebt_delete_chain(&mut chains, &args[1]) {
                Ok(()) => 0,
                Err(e) => {
                    write_err(&format!("ebtables: {e}\n"));
                    1
                }
            }
        }
        other => {
            write_err(&format!("ebtables: unknown option \"{other}\"\n"));
            1
        }
    }
}

fn ebtables_usage() {
    write_out("Usage: ebtables [options]\n");
    write_out("  -A chain rule     Append rule to chain\n");
    write_out("  -I chain [n] rule Insert rule at position n\n");
    write_out("  -D chain rulenum  Delete rule from chain\n");
    write_out("  -L [chain]        List rules\n");
    write_out("  -F [chain]        Flush rules\n");
    write_out("  -P chain target   Set chain policy\n");
    write_out("  -N chain          Create user-defined chain\n");
    write_out("  -X chain          Delete user-defined chain\n");
    write_out("\nMatches:\n");
    write_out("  -p protocol  -s src-mac  -d dst-mac\n");
    write_out("  -i in-if     -o out-if   --vlan-id VID\n");
    write_out("\nTargets: ACCEPT, DROP, CONTINUE, RETURN\n");
}

// ============================================================================
// Entry point
// ============================================================================

fn main() {
    let args: Vec<String> = env::args().collect();

    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("bridge");
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
    let tool = detect_tool(&prog_name);

    let code = match tool {
        Tool::Bridge => run_bridge(&rest),
        Tool::Tc => run_tc(&rest),
        Tool::Ebtables => run_ebtables(&rest),
    };

    std::process::exit(code);
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ---- Personality detection ----

    #[test]
    fn test_detect_bridge() {
        assert_eq!(detect_tool("bridge"), Tool::Bridge);
    }

    #[test]
    fn test_detect_bridge_with_path() {
        assert_eq!(detect_tool("/usr/sbin/bridge"), Tool::Bridge);
    }

    #[test]
    fn test_detect_bridge_windows_path() {
        assert_eq!(detect_tool("C:\\bin\\bridge.exe"), Tool::Bridge);
    }

    #[test]
    fn test_detect_tc() {
        assert_eq!(detect_tool("tc"), Tool::Tc);
    }

    #[test]
    fn test_detect_tc_with_path() {
        assert_eq!(detect_tool("/sbin/tc"), Tool::Tc);
    }

    #[test]
    fn test_detect_ebtables() {
        assert_eq!(detect_tool("ebtables"), Tool::Ebtables);
    }

    #[test]
    fn test_detect_ebtables_legacy() {
        assert_eq!(detect_tool("ebtables-legacy"), Tool::Ebtables);
    }

    #[test]
    fn test_detect_unknown_defaults_bridge() {
        assert_eq!(detect_tool("something_else"), Tool::Bridge);
    }

    // ---- MAC address parsing ----

    #[test]
    fn test_parse_mac_valid() {
        assert_eq!(
            parse_mac("00:11:22:33:44:55"),
            Some([0x00, 0x11, 0x22, 0x33, 0x44, 0x55])
        );
    }

    #[test]
    fn test_parse_mac_uppercase() {
        assert_eq!(
            parse_mac("AA:BB:CC:DD:EE:FF"),
            Some([0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF])
        );
    }

    #[test]
    fn test_parse_mac_invalid_too_short() {
        assert_eq!(parse_mac("00:11:22:33:44"), None);
    }

    #[test]
    fn test_parse_mac_invalid_hex() {
        assert_eq!(parse_mac("00:11:22:33:44:GG"), None);
    }

    #[test]
    fn test_format_mac() {
        assert_eq!(
            format_mac(&[0x00, 0x11, 0x22, 0x33, 0x44, 0x55]),
            "00:11:22:33:44:55"
        );
    }

    #[test]
    fn test_format_mac_upper_values() {
        assert_eq!(
            format_mac(&[0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF]),
            "aa:bb:cc:dd:ee:ff"
        );
    }

    // ---- State names ----

    #[test]
    fn test_state_name_forwarding() {
        assert_eq!(state_name(BR_STATE_FORWARDING), "forwarding");
    }

    #[test]
    fn test_state_name_disabled() {
        assert_eq!(state_name(BR_STATE_DISABLED), "disabled");
    }

    #[test]
    fn test_state_name_blocking() {
        assert_eq!(state_name(BR_STATE_BLOCKING), "blocking");
    }

    #[test]
    fn test_parse_state_forwarding() {
        assert_eq!(parse_state("forwarding"), Some(BR_STATE_FORWARDING));
    }

    #[test]
    fn test_parse_state_invalid() {
        assert_eq!(parse_state("invalid"), None);
    }

    #[test]
    fn test_parse_state_learning() {
        assert_eq!(parse_state("learning"), Some(BR_STATE_LEARNING));
    }

    #[test]
    fn test_parse_state_listening() {
        assert_eq!(parse_state("listening"), Some(BR_STATE_LISTENING));
    }

    // ---- Rate parsing ----

    #[test]
    fn test_parse_rate_mbit() {
        assert_eq!(parse_rate("10mbit"), Some(10_000_000));
    }

    #[test]
    fn test_parse_rate_kbit() {
        assert_eq!(parse_rate("100kbit"), Some(100_000));
    }

    #[test]
    fn test_parse_rate_gbit() {
        assert_eq!(parse_rate("1gbit"), Some(1_000_000_000));
    }

    #[test]
    fn test_parse_rate_mbps() {
        assert_eq!(parse_rate("10mbps"), Some(80_000_000));
    }

    #[test]
    fn test_parse_rate_kbps() {
        assert_eq!(parse_rate("100kbps"), Some(800_000));
    }

    #[test]
    fn test_parse_rate_bare_number() {
        assert_eq!(parse_rate("1000000"), Some(1_000_000));
    }

    #[test]
    fn test_parse_rate_bit() {
        assert_eq!(parse_rate("500bit"), Some(500));
    }

    #[test]
    fn test_parse_rate_bps() {
        assert_eq!(parse_rate("100bps"), Some(800));
    }

    #[test]
    fn test_parse_rate_gbps() {
        assert_eq!(parse_rate("1gbps"), Some(8_000_000_000));
    }

    #[test]
    fn test_parse_rate_invalid() {
        assert_eq!(parse_rate("abc"), None);
    }

    #[test]
    fn test_format_rate_gbit() {
        assert_eq!(format_rate(1_000_000_000), "1Gbit");
    }

    #[test]
    fn test_format_rate_mbit() {
        assert_eq!(format_rate(10_000_000), "10Mbit");
    }

    #[test]
    fn test_format_rate_kbit() {
        assert_eq!(format_rate(100_000), "100Kbit");
    }

    #[test]
    fn test_format_rate_bits() {
        assert_eq!(format_rate(500), "500bit");
    }

    // ---- Size parsing ----

    #[test]
    fn test_parse_size_bytes() {
        assert_eq!(parse_size("1600b"), Some(1600));
    }

    #[test]
    fn test_parse_size_kb() {
        assert_eq!(parse_size("15kb"), Some(15360));
    }

    #[test]
    fn test_parse_size_mb() {
        assert_eq!(parse_size("1mb"), Some(1_048_576));
    }

    #[test]
    fn test_parse_size_bare() {
        assert_eq!(parse_size("1500"), Some(1500));
    }

    #[test]
    fn test_parse_size_invalid() {
        assert_eq!(parse_size("xyz"), None);
    }

    // ---- Time parsing ----

    #[test]
    fn test_parse_time_ms() {
        assert_eq!(parse_time("100ms"), Some(100_000));
    }

    #[test]
    fn test_parse_time_us() {
        assert_eq!(parse_time("500us"), Some(500));
    }

    #[test]
    fn test_parse_time_s() {
        assert_eq!(parse_time("1s"), Some(1_000_000));
    }

    #[test]
    fn test_parse_time_bare() {
        // Bare number defaults to ms
        assert_eq!(parse_time("50"), Some(50_000));
    }

    #[test]
    fn test_format_time_seconds() {
        assert_eq!(format_time(1_000_000), "1s");
    }

    #[test]
    fn test_format_time_ms() {
        assert_eq!(format_time(5000), "5ms");
    }

    #[test]
    fn test_format_time_us() {
        assert_eq!(format_time(123), "123us");
    }

    // ---- Handle parsing ----

    #[test]
    fn test_parse_handle_root() {
        assert_eq!(parse_handle("root"), Some(TC_H_ROOT));
    }

    #[test]
    fn test_parse_handle_ingress() {
        assert_eq!(parse_handle("ingress"), Some(TC_H_INGRESS));
    }

    #[test]
    fn test_parse_handle_major_minor() {
        assert_eq!(parse_handle("1:0"), Some(0x0001_0000));
    }

    #[test]
    fn test_parse_handle_with_minor() {
        assert_eq!(parse_handle("1:10"), Some(0x0001_0010));
    }

    #[test]
    fn test_parse_handle_bare() {
        assert_eq!(parse_handle("1"), Some(0x0001_0000));
    }

    #[test]
    fn test_parse_handle_empty_major() {
        assert_eq!(parse_handle(":10"), Some(0x0000_0010));
    }

    #[test]
    fn test_format_handle_root() {
        assert_eq!(format_handle(TC_H_ROOT), "root");
    }

    #[test]
    fn test_format_handle_ingress() {
        assert_eq!(format_handle(TC_H_INGRESS), "ingress");
    }

    #[test]
    fn test_format_handle_numeric() {
        assert_eq!(format_handle(0x0001_0010), "1:10");
    }

    #[test]
    fn test_format_handle_roundtrip() {
        let h = parse_handle("a:ff").unwrap();
        assert_eq!(format_handle(h), "a:ff");
    }

    // ---- Bridge port operations ----

    #[test]
    fn test_bridge_port_new() {
        let p = BridgePort::new("eth0", "br0");
        assert_eq!(p.name, "eth0");
        assert_eq!(p.bridge, "br0");
        assert_eq!(p.state, BR_STATE_FORWARDING);
        assert!(p.learning);
        assert!(p.flood);
    }

    #[test]
    fn test_bridge_set_link_learning_off() {
        let mut ports = vec![BridgePort::new("eth0", "br0")];
        let args: Vec<String> = vec!["dev", "eth0", "learning", "off"]
            .into_iter().map(String::from).collect();
        bridge_set_link(&mut ports, &args).unwrap();
        assert!(!ports[0].learning);
    }

    #[test]
    fn test_bridge_set_link_state() {
        let mut ports = vec![BridgePort::new("eth0", "br0")];
        let args: Vec<String> = vec!["dev", "eth0", "state", "blocking"]
            .into_iter().map(String::from).collect();
        bridge_set_link(&mut ports, &args).unwrap();
        assert_eq!(ports[0].state, BR_STATE_BLOCKING);
    }

    #[test]
    fn test_bridge_set_link_missing_dev() {
        let mut ports = vec![BridgePort::new("eth0", "br0")];
        let args: Vec<String> = vec!["learning", "off"]
            .into_iter().map(String::from).collect();
        assert!(bridge_set_link(&mut ports, &args).is_err());
    }

    #[test]
    fn test_bridge_set_link_unknown_port() {
        let mut ports = vec![BridgePort::new("eth0", "br0")];
        let args: Vec<String> = vec!["dev", "eth9", "learning", "off"]
            .into_iter().map(String::from).collect();
        assert!(bridge_set_link(&mut ports, &args).is_err());
    }

    #[test]
    fn test_bridge_set_link_priority() {
        let mut ports = vec![BridgePort::new("eth0", "br0")];
        let args: Vec<String> = vec!["dev", "eth0", "priority", "64"]
            .into_iter().map(String::from).collect();
        bridge_set_link(&mut ports, &args).unwrap();
        assert_eq!(ports[0].priority, 64);
    }

    #[test]
    fn test_bridge_set_link_cost() {
        let mut ports = vec![BridgePort::new("eth0", "br0")];
        let args: Vec<String> = vec!["dev", "eth0", "cost", "200"]
            .into_iter().map(String::from).collect();
        bridge_set_link(&mut ports, &args).unwrap();
        assert_eq!(ports[0].cost, 200);
    }

    #[test]
    fn test_bridge_set_link_hairpin() {
        let mut ports = vec![BridgePort::new("eth0", "br0")];
        let args: Vec<String> = vec!["dev", "eth0", "hairpin", "on"]
            .into_iter().map(String::from).collect();
        bridge_set_link(&mut ports, &args).unwrap();
        assert!(ports[0].hairpin);
    }

    #[test]
    fn test_bridge_set_link_flood_off() {
        let mut ports = vec![BridgePort::new("eth0", "br0")];
        let args: Vec<String> = vec!["dev", "eth0", "flood", "off"]
            .into_iter().map(String::from).collect();
        bridge_set_link(&mut ports, &args).unwrap();
        assert!(!ports[0].flood);
    }

    // ---- FDB operations ----

    #[test]
    fn test_fdb_add() {
        let mut entries = Vec::new();
        let args: Vec<String> = vec!["00:11:22:33:44:55", "dev", "eth0"]
            .into_iter().map(String::from).collect();
        bridge_add_fdb(&mut entries, &args).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].mac, [0x00, 0x11, 0x22, 0x33, 0x44, 0x55]);
        assert_eq!(entries[0].port, "eth0");
    }

    #[test]
    fn test_fdb_add_with_vlan() {
        let mut entries = Vec::new();
        let args: Vec<String> = vec!["AA:BB:CC:DD:EE:FF", "dev", "eth1", "vlan", "100"]
            .into_iter().map(String::from).collect();
        bridge_add_fdb(&mut entries, &args).unwrap();
        assert_eq!(entries[0].vlan, Some(100));
    }

    #[test]
    fn test_fdb_add_dynamic() {
        let mut entries = Vec::new();
        let args: Vec<String> = vec!["00:11:22:33:44:55", "dev", "eth0", "dynamic"]
            .into_iter().map(String::from).collect();
        bridge_add_fdb(&mut entries, &args).unwrap();
        assert!(!entries[0].is_static);
    }

    #[test]
    fn test_fdb_add_missing_mac() {
        let mut entries = Vec::new();
        let args: Vec<String> = Vec::new();
        assert!(bridge_add_fdb(&mut entries, &args).is_err());
    }

    #[test]
    fn test_fdb_add_invalid_mac() {
        let mut entries = Vec::new();
        let args: Vec<String> = vec!["invalid_mac", "dev", "eth0"]
            .into_iter().map(String::from).collect();
        assert!(bridge_add_fdb(&mut entries, &args).is_err());
    }

    #[test]
    fn test_fdb_del() {
        let mut entries = vec![FdbEntry {
            mac: [0x00, 0x11, 0x22, 0x33, 0x44, 0x55],
            port: "eth0".to_string(),
            vlan: None,
            is_local: false,
            is_static: true,
            offloaded: false,
        }];
        let args: Vec<String> = vec!["00:11:22:33:44:55", "dev", "eth0"]
            .into_iter().map(String::from).collect();
        bridge_del_fdb(&mut entries, &args).unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn test_fdb_del_not_found() {
        let mut entries = Vec::new();
        let args: Vec<String> = vec!["00:11:22:33:44:55"]
            .into_iter().map(String::from).collect();
        assert!(bridge_del_fdb(&mut entries, &args).is_err());
    }

    #[test]
    fn test_fdb_flush_all() {
        let mut entries = vec![
            FdbEntry {
                mac: [0x00, 0x11, 0x22, 0x33, 0x44, 0x55],
                port: "eth0".to_string(),
                vlan: None,
                is_local: false,
                is_static: true,
                offloaded: false,
            },
            FdbEntry {
                mac: [0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF],
                port: "eth0".to_string(),
                vlan: None,
                is_local: true,
                is_static: true,
                offloaded: false,
            },
        ];
        let args: Vec<String> = Vec::new();
        bridge_flush_fdb(&mut entries, &args);
        // Only local entries are kept
        assert_eq!(entries.len(), 1);
        assert!(entries[0].is_local);
    }

    #[test]
    fn test_fdb_flush_per_dev() {
        let mut entries = vec![
            FdbEntry {
                mac: [0x00, 0x11, 0x22, 0x33, 0x44, 0x55],
                port: "eth0".to_string(),
                vlan: None,
                is_local: false,
                is_static: true,
                offloaded: false,
            },
            FdbEntry {
                mac: [0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF],
                port: "eth1".to_string(),
                vlan: None,
                is_local: false,
                is_static: true,
                offloaded: false,
            },
        ];
        let args: Vec<String> = vec!["dev", "eth0"]
            .into_iter().map(String::from).collect();
        bridge_flush_fdb(&mut entries, &args);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].port, "eth1");
    }

    // ---- MDB operations ----

    #[test]
    fn test_mdb_add() {
        let mut entries = Vec::new();
        let args: Vec<String> = vec!["dev", "br0", "port", "eth0", "grp", "239.1.1.1", "permanent"]
            .into_iter().map(String::from).collect();
        bridge_add_mdb(&mut entries, &args).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].group, "239.1.1.1");
        assert!(entries[0].is_permanent);
    }

    #[test]
    fn test_mdb_add_temp() {
        let mut entries = Vec::new();
        let args: Vec<String> = vec!["dev", "br0", "port", "eth0", "grp", "239.1.1.2", "temp"]
            .into_iter().map(String::from).collect();
        bridge_add_mdb(&mut entries, &args).unwrap();
        assert!(!entries[0].is_permanent);
    }

    #[test]
    fn test_mdb_add_with_vlan() {
        let mut entries = Vec::new();
        let args: Vec<String> = vec!["dev", "br0", "port", "eth0", "grp", "239.1.1.1", "vlan", "100"]
            .into_iter().map(String::from).collect();
        bridge_add_mdb(&mut entries, &args).unwrap();
        assert_eq!(entries[0].vlan, Some(100));
    }

    #[test]
    fn test_mdb_add_missing_port() {
        let mut entries = Vec::new();
        let args: Vec<String> = vec!["dev", "br0", "grp", "239.1.1.1"]
            .into_iter().map(String::from).collect();
        assert!(bridge_add_mdb(&mut entries, &args).is_err());
    }

    #[test]
    fn test_mdb_del() {
        let mut entries = vec![MdbEntry {
            group: "239.1.1.1".to_string(),
            port: "eth0".to_string(),
            vlan: None,
            is_permanent: true,
        }];
        let args: Vec<String> = vec!["dev", "br0", "port", "eth0", "grp", "239.1.1.1"]
            .into_iter().map(String::from).collect();
        bridge_del_mdb(&mut entries, &args).unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn test_mdb_del_not_found() {
        let mut entries = Vec::new();
        let args: Vec<String> = vec!["dev", "br0", "port", "eth0", "grp", "239.1.1.1"]
            .into_iter().map(String::from).collect();
        assert!(bridge_del_mdb(&mut entries, &args).is_err());
    }

    // ---- VLAN operations ----

    #[test]
    fn test_vlan_add() {
        let mut entries = Vec::new();
        let args: Vec<String> = vec!["dev", "eth0", "vid", "100", "pvid", "untagged"]
            .into_iter().map(String::from).collect();
        bridge_add_vlan(&mut entries, &args).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].vid, 100);
        assert!(entries[0].pvid);
        assert!(entries[0].untagged);
    }

    #[test]
    fn test_vlan_add_no_flags() {
        let mut entries = Vec::new();
        let args: Vec<String> = vec!["dev", "eth0", "vid", "200"]
            .into_iter().map(String::from).collect();
        bridge_add_vlan(&mut entries, &args).unwrap();
        assert!(!entries[0].pvid);
        assert!(!entries[0].untagged);
    }

    #[test]
    fn test_vlan_add_missing_vid() {
        let mut entries = Vec::new();
        let args: Vec<String> = vec!["dev", "eth0"]
            .into_iter().map(String::from).collect();
        assert!(bridge_add_vlan(&mut entries, &args).is_err());
    }

    #[test]
    fn test_vlan_del() {
        let mut entries = vec![VlanEntry {
            port: "eth0".to_string(),
            vid: 100,
            vid_end: None,
            pvid: false,
            untagged: false,
        }];
        let args: Vec<String> = vec!["dev", "eth0", "vid", "100"]
            .into_iter().map(String::from).collect();
        bridge_del_vlan(&mut entries, &args).unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn test_vlan_del_not_found() {
        let mut entries = Vec::new();
        let args: Vec<String> = vec!["dev", "eth0", "vid", "999"]
            .into_iter().map(String::from).collect();
        assert!(bridge_del_vlan(&mut entries, &args).is_err());
    }

    #[test]
    fn test_build_vlan_flags_text_pvid_untagged() {
        assert_eq!(build_vlan_flags_text(true, true), " PVID Egress Untagged");
    }

    #[test]
    fn test_build_vlan_flags_text_none() {
        assert_eq!(build_vlan_flags_text(false, false), "");
    }

    #[test]
    fn test_build_vlan_flags_json_pvid() {
        assert_eq!(build_vlan_flags_json(true, false), "\"PVID\"");
    }

    #[test]
    fn test_build_vlan_flags_json_both() {
        assert_eq!(
            build_vlan_flags_json(true, true),
            "\"PVID\", \"Egress Untagged\""
        );
    }

    // ---- TC qdisc parsing ----

    #[test]
    fn test_tc_parse_qdisc_htb() {
        let args: Vec<String> = vec!["dev", "eth0", "root", "handle", "1:", "htb", "default", "30"]
            .into_iter().map(String::from).collect();
        let (dev, parent, kind) = tc_parse_qdisc(&args).unwrap();
        assert_eq!(dev, "eth0");
        assert_eq!(parent, TC_H_ROOT);
        assert!(matches!(kind, QdiscKind::Htb { default_class } if default_class == parse_handle("30").unwrap()));
    }

    #[test]
    fn test_tc_parse_qdisc_tbf() {
        let args: Vec<String> = vec!["dev", "eth0", "root", "tbf", "rate", "1mbit", "burst", "1600b", "latency", "50ms"]
            .into_iter().map(String::from).collect();
        let (dev, _parent, kind) = tc_parse_qdisc(&args).unwrap();
        assert_eq!(dev, "eth0");
        match kind {
            QdiscKind::Tbf { rate, burst, latency, .. } => {
                assert_eq!(rate, 1_000_000);
                assert_eq!(burst, 1600);
                assert_eq!(latency, 50_000);
            }
            _ => panic!("expected Tbf"),
        }
    }

    #[test]
    fn test_tc_parse_qdisc_netem() {
        let args: Vec<String> = vec!["dev", "eth0", "root", "netem", "delay", "100ms", "loss", "5%"]
            .into_iter().map(String::from).collect();
        let (_, _, kind) = tc_parse_qdisc(&args).unwrap();
        match kind {
            QdiscKind::Netem { delay, loss, .. } => {
                assert_eq!(delay, 100_000);
                assert_eq!(loss, Some(5.0));
            }
            _ => panic!("expected Netem"),
        }
    }

    #[test]
    fn test_tc_parse_qdisc_sfq() {
        let args: Vec<String> = vec!["dev", "eth0", "root", "sfq", "perturb", "15"]
            .into_iter().map(String::from).collect();
        let (_, _, kind) = tc_parse_qdisc(&args).unwrap();
        match kind {
            QdiscKind::Sfq { perturb, .. } => assert_eq!(perturb, 15),
            _ => panic!("expected Sfq"),
        }
    }

    #[test]
    fn test_tc_parse_qdisc_fqcodel() {
        let args: Vec<String> = vec!["dev", "eth0", "root", "fq_codel", "target", "5ms", "limit", "2048"]
            .into_iter().map(String::from).collect();
        let (_, _, kind) = tc_parse_qdisc(&args).unwrap();
        match kind {
            QdiscKind::FqCodel { target, limit, .. } => {
                assert_eq!(target, 5000);
                assert_eq!(limit, 2048);
            }
            _ => panic!("expected FqCodel"),
        }
    }

    #[test]
    fn test_tc_parse_qdisc_ingress() {
        let args: Vec<String> = vec!["dev", "eth0", "ingress"]
            .into_iter().map(String::from).collect();
        let (_, parent, kind) = tc_parse_qdisc(&args).unwrap();
        assert!(matches!(kind, QdiscKind::Ingress));
        assert_eq!(parent, TC_H_INGRESS);
    }

    #[test]
    fn test_tc_parse_qdisc_missing_dev() {
        let args: Vec<String> = vec!["root", "htb"]
            .into_iter().map(String::from).collect();
        assert!(tc_parse_qdisc(&args).is_err());
    }

    // ---- TC class parsing ----

    #[test]
    fn test_tc_parse_class_htb() {
        let args: Vec<String> = vec![
            "dev", "eth0", "parent", "1:0", "classid", "1:10",
            "htb", "rate", "10mbit", "ceil", "100mbit", "prio", "1",
        ].into_iter().map(String::from).collect();
        let class = tc_parse_class(&args).unwrap();
        assert_eq!(class.dev, "eth0");
        assert_eq!(class.rate, 10_000_000);
        assert_eq!(class.ceil, 100_000_000);
        assert_eq!(class.prio, 1);
    }

    #[test]
    fn test_tc_parse_class_ceil_defaults_to_rate() {
        let args: Vec<String> = vec![
            "dev", "eth0", "parent", "1:0", "htb", "rate", "5mbit",
        ].into_iter().map(String::from).collect();
        let class = tc_parse_class(&args).unwrap();
        assert_eq!(class.ceil, 5_000_000);
    }

    #[test]
    fn test_tc_parse_class_missing_dev() {
        let args: Vec<String> = vec!["parent", "1:0", "htb", "rate", "10mbit"]
            .into_iter().map(String::from).collect();
        assert!(tc_parse_class(&args).is_err());
    }

    // ---- TC filter parsing ----

    #[test]
    fn test_tc_parse_filter_u32() {
        let args: Vec<String> = vec![
            "dev", "eth0", "parent", "1:0", "protocol", "ip", "prio", "1",
            "u32", "match", "ip", "dst", "0xffffff00", "flowid", "1:10",
        ].into_iter().map(String::from).collect();
        let f = tc_parse_filter(&args).unwrap();
        assert_eq!(f.dev, "eth0");
        assert!(matches!(f.kind, FilterKind::U32 { .. }));
        assert_eq!(f.flowid, Some(0x0001_0010));
    }

    #[test]
    fn test_tc_parse_filter_fw() {
        let args: Vec<String> = vec![
            "dev", "eth0", "parent", "1:0", "fw", "handle", "0x1",
            "flowid", "1:10",
        ].into_iter().map(String::from).collect();
        let f = tc_parse_filter(&args).unwrap();
        match &f.kind {
            FilterKind::Fw { fwmark } => assert_eq!(*fwmark, 1),
            _ => panic!("expected Fw"),
        }
    }

    #[test]
    fn test_tc_parse_filter_matchall() {
        let args: Vec<String> = vec![
            "dev", "eth0", "parent", "ffff:", "matchall", "action", "drop",
        ].into_iter().map(String::from).collect();
        let f = tc_parse_filter(&args).unwrap();
        assert!(matches!(f.kind, FilterKind::Matchall));
        assert_eq!(f.action, Some("drop".to_string()));
    }

    #[test]
    fn test_tc_parse_filter_basic() {
        let args: Vec<String> = vec![
            "dev", "eth0", "parent", "1:0", "basic", "flowid", "1:10",
        ].into_iter().map(String::from).collect();
        let f = tc_parse_filter(&args).unwrap();
        assert!(matches!(f.kind, FilterKind::Basic { .. }));
    }

    // ---- Ebtables operations ----

    #[test]
    fn test_ebt_default_chains() {
        let chains = default_chains();
        assert_eq!(chains.len(), 3);
        assert_eq!(chains[0].name, CHAIN_INPUT);
        assert_eq!(chains[1].name, CHAIN_OUTPUT);
        assert_eq!(chains[2].name, CHAIN_FORWARD);
    }

    #[test]
    fn test_ebt_parse_rule_basic() {
        let args: Vec<String> = vec!["INPUT", "-j", "DROP"]
            .into_iter().map(String::from).collect();
        let (chain, rule) = ebt_parse_rule(&args).unwrap();
        assert_eq!(chain, "INPUT");
        assert_eq!(rule.target, "DROP");
    }

    #[test]
    fn test_ebt_parse_rule_full() {
        let args: Vec<String> = vec![
            "FORWARD", "-p", "IPv4", "-s", "00:11:22:33:44:55",
            "-d", "AA:BB:CC:DD:EE:FF", "-i", "eth0", "-o", "eth1",
            "--vlan-id", "100", "-j", "ACCEPT",
        ].into_iter().map(String::from).collect();
        let (chain, rule) = ebt_parse_rule(&args).unwrap();
        assert_eq!(chain, "FORWARD");
        assert_eq!(rule.protocol, Some("IPv4".to_string()));
        assert_eq!(rule.src_mac, Some([0x00, 0x11, 0x22, 0x33, 0x44, 0x55]));
        assert_eq!(rule.dst_mac, Some([0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF]));
        assert_eq!(rule.in_if, Some("eth0".to_string()));
        assert_eq!(rule.out_if, Some("eth1".to_string()));
        assert_eq!(rule.vlan_id, Some(100));
        assert_eq!(rule.target, "ACCEPT");
    }

    #[test]
    fn test_ebt_parse_rule_missing_chain() {
        let args: Vec<String> = Vec::new();
        assert!(ebt_parse_rule(&args).is_err());
    }

    #[test]
    fn test_ebt_add_rule() {
        let mut chains = default_chains();
        let rule = EbtablesRule {
            protocol: None,
            src_mac: None,
            dst_mac: None,
            in_if: None,
            out_if: None,
            vlan_id: None,
            target: TARGET_DROP.to_string(),
        };
        ebt_add_rule(&mut chains, "INPUT", rule).unwrap();
        assert_eq!(chains[0].rules.len(), 1);
    }

    #[test]
    fn test_ebt_add_rule_unknown_chain() {
        let mut chains = default_chains();
        let rule = EbtablesRule {
            protocol: None,
            src_mac: None,
            dst_mac: None,
            in_if: None,
            out_if: None,
            vlan_id: None,
            target: TARGET_DROP.to_string(),
        };
        assert!(ebt_add_rule(&mut chains, "NONEXIST", rule).is_err());
    }

    #[test]
    fn test_ebt_insert_rule_at_zero() {
        let mut chains = default_chains();
        let rule1 = EbtablesRule {
            protocol: Some("IPv4".to_string()),
            src_mac: None, dst_mac: None, in_if: None, out_if: None,
            vlan_id: None,
            target: TARGET_ACCEPT.to_string(),
        };
        let rule2 = EbtablesRule {
            protocol: Some("ARP".to_string()),
            src_mac: None, dst_mac: None, in_if: None, out_if: None,
            vlan_id: None,
            target: TARGET_DROP.to_string(),
        };
        ebt_add_rule(&mut chains, "INPUT", rule1).unwrap();
        ebt_insert_rule(&mut chains, "INPUT", rule2, 0).unwrap();
        assert_eq!(chains[0].rules[0].protocol, Some("ARP".to_string()));
        assert_eq!(chains[0].rules[1].protocol, Some("IPv4".to_string()));
    }

    #[test]
    fn test_ebt_delete_rule() {
        let mut chains = default_chains();
        let rule = EbtablesRule {
            protocol: None, src_mac: None, dst_mac: None,
            in_if: None, out_if: None, vlan_id: None,
            target: TARGET_DROP.to_string(),
        };
        ebt_add_rule(&mut chains, "INPUT", rule).unwrap();
        ebt_delete_rule(&mut chains, "INPUT", 1).unwrap();
        assert!(chains[0].rules.is_empty());
    }

    #[test]
    fn test_ebt_delete_rule_out_of_range() {
        let mut chains = default_chains();
        assert!(ebt_delete_rule(&mut chains, "INPUT", 1).is_err());
    }

    #[test]
    fn test_ebt_delete_rule_zero() {
        let mut chains = default_chains();
        assert!(ebt_delete_rule(&mut chains, "INPUT", 0).is_err());
    }

    #[test]
    fn test_ebt_flush_all() {
        let mut chains = default_chains();
        let rule = EbtablesRule {
            protocol: None, src_mac: None, dst_mac: None,
            in_if: None, out_if: None, vlan_id: None,
            target: TARGET_DROP.to_string(),
        };
        ebt_add_rule(&mut chains, "INPUT", rule).unwrap();
        ebt_flush(&mut chains, None);
        assert!(chains[0].rules.is_empty());
    }

    #[test]
    fn test_ebt_flush_specific_chain() {
        let mut chains = default_chains();
        let rule1 = EbtablesRule {
            protocol: None, src_mac: None, dst_mac: None,
            in_if: None, out_if: None, vlan_id: None,
            target: TARGET_DROP.to_string(),
        };
        let rule2 = EbtablesRule {
            protocol: None, src_mac: None, dst_mac: None,
            in_if: None, out_if: None, vlan_id: None,
            target: TARGET_ACCEPT.to_string(),
        };
        ebt_add_rule(&mut chains, "INPUT", rule1).unwrap();
        ebt_add_rule(&mut chains, "OUTPUT", rule2).unwrap();
        ebt_flush(&mut chains, Some("INPUT"));
        assert!(chains[0].rules.is_empty());
        assert_eq!(chains[1].rules.len(), 1);
    }

    #[test]
    fn test_ebt_set_policy_accept() {
        let mut chains = default_chains();
        ebt_set_policy(&mut chains, "INPUT", TARGET_DROP).unwrap();
        assert_eq!(chains[0].policy, TARGET_DROP);
    }

    #[test]
    fn test_ebt_set_policy_invalid() {
        let mut chains = default_chains();
        assert!(ebt_set_policy(&mut chains, "INPUT", "INVALID").is_err());
    }

    #[test]
    fn test_ebt_set_policy_user_chain() {
        let mut chains = default_chains();
        ebt_new_chain(&mut chains, "MYCHAIN").unwrap();
        assert!(ebt_set_policy(&mut chains, "MYCHAIN", TARGET_DROP).is_err());
    }

    #[test]
    fn test_ebt_new_chain() {
        let mut chains = default_chains();
        ebt_new_chain(&mut chains, "CUSTOM").unwrap();
        assert_eq!(chains.len(), 4);
        assert_eq!(chains[3].name, "CUSTOM");
        assert!(!chains[3].is_builtin);
    }

    #[test]
    fn test_ebt_new_chain_duplicate() {
        let mut chains = default_chains();
        assert!(ebt_new_chain(&mut chains, "INPUT").is_err());
    }

    #[test]
    fn test_ebt_delete_chain() {
        let mut chains = default_chains();
        ebt_new_chain(&mut chains, "CUSTOM").unwrap();
        ebt_delete_chain(&mut chains, "CUSTOM").unwrap();
        assert_eq!(chains.len(), 3);
    }

    #[test]
    fn test_ebt_delete_builtin_chain() {
        let mut chains = default_chains();
        assert!(ebt_delete_chain(&mut chains, "INPUT").is_err());
    }

    #[test]
    fn test_ebt_delete_nonempty_chain() {
        let mut chains = default_chains();
        ebt_new_chain(&mut chains, "CUSTOM").unwrap();
        let rule = EbtablesRule {
            protocol: None, src_mac: None, dst_mac: None,
            in_if: None, out_if: None, vlan_id: None,
            target: TARGET_ACCEPT.to_string(),
        };
        ebt_add_rule(&mut chains, "CUSTOM", rule).unwrap();
        assert!(ebt_delete_chain(&mut chains, "CUSTOM").is_err());
    }

    #[test]
    fn test_ebt_format_rule_basic() {
        let rule = EbtablesRule {
            protocol: None, src_mac: None, dst_mac: None,
            in_if: None, out_if: None, vlan_id: None,
            target: TARGET_DROP.to_string(),
        };
        assert_eq!(ebt_format_rule(&rule), "-j DROP");
    }

    #[test]
    fn test_ebt_format_rule_full() {
        let rule = EbtablesRule {
            protocol: Some("IPv4".to_string()),
            src_mac: Some([0x00, 0x11, 0x22, 0x33, 0x44, 0x55]),
            dst_mac: Some([0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF]),
            in_if: Some("eth0".to_string()),
            out_if: Some("eth1".to_string()),
            vlan_id: Some(100),
            target: TARGET_ACCEPT.to_string(),
        };
        let s = ebt_format_rule(&rule);
        assert!(s.contains("-p IPv4"));
        assert!(s.contains("-s 00:11:22:33:44:55"));
        assert!(s.contains("-d aa:bb:cc:dd:ee:ff"));
        assert!(s.contains("-i eth0"));
        assert!(s.contains("-o eth1"));
        assert!(s.contains("--vlan-id 100"));
        assert!(s.contains("-j ACCEPT"));
    }

    // ---- Percent parsing ----

    #[test]
    fn test_parse_percent_with_sign() {
        assert_eq!(parse_percent("5%"), Some(5.0));
    }

    #[test]
    fn test_parse_percent_without_sign() {
        assert_eq!(parse_percent("10.5"), Some(10.5));
    }

    #[test]
    fn test_parse_percent_invalid() {
        assert_eq!(parse_percent("abc"), None);
    }

    // ---- Qdisc kind name ----

    #[test]
    fn test_qdisc_kind_name_all() {
        assert_eq!(qdisc_kind_name(&QdiscKind::PfifoFast), "pfifo_fast");
        assert_eq!(qdisc_kind_name(&QdiscKind::Ingress), "ingress");
        assert_eq!(
            qdisc_kind_name(&QdiscKind::Tbf {
                rate: 0, burst: 0, latency: 0, peakrate: None, mtu: None,
            }),
            "tbf"
        );
        assert_eq!(
            qdisc_kind_name(&QdiscKind::Htb { default_class: 0 }),
            "htb"
        );
        assert_eq!(
            qdisc_kind_name(&QdiscKind::Sfq { perturb: 0, quantum: 0 }),
            "sfq"
        );
        assert_eq!(
            qdisc_kind_name(&QdiscKind::FqCodel {
                target: 0, interval: 0, quantum: 0, limit: 0,
            }),
            "fq_codel"
        );
        assert_eq!(
            qdisc_kind_name(&QdiscKind::Netem {
                delay: 0, jitter: None, loss: None,
                duplicate: None, corrupt: None, reorder: None,
            }),
            "netem"
        );
    }

    // ---- Filter kind name ----

    #[test]
    fn test_filter_kind_name_all() {
        assert_eq!(
            filter_kind_name(&FilterKind::U32 {
                match_field: String::new(),
                match_value: String::new(),
                mask: String::new(),
            }),
            "u32"
        );
        assert_eq!(filter_kind_name(&FilterKind::Fw { fwmark: 0 }), "fw");
        assert_eq!(
            filter_kind_name(&FilterKind::Basic {
                expr: String::new()
            }),
            "basic"
        );
        assert_eq!(filter_kind_name(&FilterKind::Matchall), "matchall");
    }

    // ---- Netem keyword detection ----

    #[test]
    fn test_is_netem_keyword() {
        assert!(is_netem_keyword("delay"));
        assert!(is_netem_keyword("loss"));
        assert!(is_netem_keyword("dev"));
        assert!(!is_netem_keyword("100ms"));
        assert!(!is_netem_keyword("5%"));
    }

    // ---- Dispatch-level integration tests ----

    #[test]
    fn test_run_bridge_version() {
        let _ = run_bridge(&["--version".to_string()]);
    }

    #[test]
    fn test_run_bridge_help() {
        assert_eq!(run_bridge(&["--help".to_string()]), 0);
    }

    #[test]
    fn test_run_bridge_unknown_object() {
        assert_eq!(run_bridge(&["bogus".to_string()]), 1);
    }

    #[test]
    fn test_run_bridge_empty() {
        assert_eq!(run_bridge(&[]), 1);
    }

    #[test]
    fn test_run_tc_version() {
        let _ = run_tc(&["--version".to_string()]);
    }

    #[test]
    fn test_run_tc_help() {
        assert_eq!(run_tc(&["--help".to_string()]), 0);
    }

    #[test]
    fn test_run_tc_unknown_object() {
        assert_eq!(run_tc(&["bogus".to_string()]), 1);
    }

    #[test]
    fn test_run_tc_empty() {
        assert_eq!(run_tc(&[]), 1);
    }

    #[test]
    fn test_run_ebtables_version() {
        let _ = run_ebtables(&["--version".to_string()]);
    }

    #[test]
    fn test_run_ebtables_help() {
        assert_eq!(run_ebtables(&["--help".to_string()]), 0);
    }

    #[test]
    fn test_run_ebtables_empty() {
        assert_eq!(run_ebtables(&[]), 1);
    }

    #[test]
    fn test_run_ebtables_unknown() {
        assert_eq!(run_ebtables(&["--bogus".to_string()]), 1);
    }

    #[test]
    fn test_run_ebtables_list() {
        assert_eq!(run_ebtables(&["-L".to_string()]), 0);
    }

    #[test]
    fn test_run_ebtables_list_chain() {
        assert_eq!(
            run_ebtables(&["-L".to_string(), "INPUT".to_string()]),
            0
        );
    }

    #[test]
    fn test_run_ebtables_flush() {
        assert_eq!(run_ebtables(&["-F".to_string()]), 0);
    }

    #[test]
    fn test_run_ebtables_new_chain() {
        assert_eq!(
            run_ebtables(&["-N".to_string(), "TEST".to_string()]),
            0
        );
    }

    #[test]
    fn test_run_ebtables_policy() {
        assert_eq!(
            run_ebtables(&[
                "-P".to_string(),
                "INPUT".to_string(),
                "DROP".to_string(),
            ]),
            0
        );
    }

    #[test]
    fn test_run_ebtables_policy_missing_args() {
        assert_eq!(run_ebtables(&["-P".to_string()]), 1);
    }

    #[test]
    fn test_run_ebtables_delete_missing_args() {
        assert_eq!(run_ebtables(&["-D".to_string()]), 1);
    }

    #[test]
    fn test_run_ebtables_append() {
        assert_eq!(
            run_ebtables(&[
                "-A".to_string(),
                "INPUT".to_string(),
                "-j".to_string(),
                "DROP".to_string(),
            ]),
            0
        );
    }

    #[test]
    fn test_run_ebtables_insert() {
        assert_eq!(
            run_ebtables(&[
                "-I".to_string(),
                "INPUT".to_string(),
                "-j".to_string(),
                "ACCEPT".to_string(),
            ]),
            0
        );
    }

    #[test]
    fn test_run_ebtables_insert_missing_chain() {
        assert_eq!(run_ebtables(&["-I".to_string()]), 1);
    }

    // ---- Bridge link show/set dispatch ----

    #[test]
    fn test_bridge_dispatch_link_show() {
        let ctx = OutputCtx::new();
        assert_eq!(bridge_dispatch_link(&["show".to_string()], &ctx), 0);
    }

    #[test]
    fn test_bridge_dispatch_link_unknown() {
        let ctx = OutputCtx::new();
        assert_eq!(bridge_dispatch_link(&["bogus".to_string()], &ctx), 1);
    }

    #[test]
    fn test_bridge_dispatch_fdb_show() {
        let ctx = OutputCtx::new();
        assert_eq!(bridge_dispatch_fdb(&["show".to_string()], &ctx), 0);
    }

    #[test]
    fn test_bridge_dispatch_fdb_unknown() {
        let ctx = OutputCtx::new();
        assert_eq!(bridge_dispatch_fdb(&["bogus".to_string()], &ctx), 1);
    }

    #[test]
    fn test_bridge_dispatch_mdb_show() {
        let ctx = OutputCtx::new();
        assert_eq!(bridge_dispatch_mdb(&["show".to_string()], &ctx), 0);
    }

    #[test]
    fn test_bridge_dispatch_vlan_show() {
        let ctx = OutputCtx::new();
        assert_eq!(bridge_dispatch_vlan(&["show".to_string()], &ctx), 0);
    }

    #[test]
    fn test_bridge_dispatch_vlan_unknown() {
        let ctx = OutputCtx::new();
        assert_eq!(bridge_dispatch_vlan(&["bogus".to_string()], &ctx), 1);
    }

    // ---- TC dispatch ----

    #[test]
    fn test_tc_dispatch_qdisc_show() {
        let ctx = OutputCtx::new();
        assert_eq!(tc_dispatch_qdisc(&["show".to_string()], &ctx), 0);
    }

    #[test]
    fn test_tc_dispatch_qdisc_del_missing() {
        let ctx = OutputCtx::new();
        assert_eq!(tc_dispatch_qdisc(&["del".to_string()], &ctx), 1);
    }

    #[test]
    fn test_tc_dispatch_class_show() {
        let ctx = OutputCtx::new();
        assert_eq!(tc_dispatch_class(&["show".to_string()], &ctx), 0);
    }

    #[test]
    fn test_tc_dispatch_filter_show() {
        let ctx = OutputCtx::new();
        assert_eq!(tc_dispatch_filter(&["show".to_string()], &ctx), 0);
    }

    #[test]
    fn test_tc_dispatch_qdisc_unknown() {
        let ctx = OutputCtx::new();
        assert_eq!(tc_dispatch_qdisc(&["bogus".to_string()], &ctx), 1);
    }

    // ---- Bridge set_link guard and mcast_flood ----

    #[test]
    fn test_bridge_set_link_guard() {
        let mut ports = vec![BridgePort::new("eth0", "br0")];
        let args: Vec<String> = vec!["dev", "eth0", "guard", "on"]
            .into_iter().map(String::from).collect();
        bridge_set_link(&mut ports, &args).unwrap();
        assert!(ports[0].guard);
    }

    #[test]
    fn test_bridge_set_link_mcast_flood_off() {
        let mut ports = vec![BridgePort::new("eth0", "br0")];
        let args: Vec<String> = vec!["dev", "eth0", "mcast_flood", "off"]
            .into_iter().map(String::from).collect();
        bridge_set_link(&mut ports, &args).unwrap();
        assert!(!ports[0].mcast_flood);
    }

    #[test]
    fn test_bridge_set_link_bcast_flood_off() {
        let mut ports = vec![BridgePort::new("eth0", "br0")];
        let args: Vec<String> = vec!["dev", "eth0", "bcast_flood", "off"]
            .into_iter().map(String::from).collect();
        bridge_set_link(&mut ports, &args).unwrap();
        assert!(!ports[0].bcast_flood);
    }

    #[test]
    fn test_bridge_set_link_invalid_state() {
        let mut ports = vec![BridgePort::new("eth0", "br0")];
        let args: Vec<String> = vec!["dev", "eth0", "state", "bogus"]
            .into_iter().map(String::from).collect();
        assert!(bridge_set_link(&mut ports, &args).is_err());
    }
}
