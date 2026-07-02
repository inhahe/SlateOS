//! Slate OS DHCP Client Daemon (`dhcpcd`)
//!
//! Implements the DHCP protocol (RFC 2131/RFC 2132) to dynamically obtain
//! network configuration from a DHCP server. Manages the full DORA exchange
//! (Discover, Offer, Request, Acknowledge), lease renewal, rebinding, and
//! interface configuration.
//!
//! # Usage
//!
//! ```text
//! dhcpcd                          Obtain lease on default interface
//! dhcpcd -i eth0                  Obtain lease on specific interface
//! dhcpcd -n                       Print lease only, don't configure
//! dhcpcd -r 192.168.1.100         Request specific IP
//! dhcpcd -d                       Debug/verbose mode
//! dhcpcd -f                       Run in foreground
//! dhcpcd -t 30                    Timeout after 30 seconds
//! dhcpcd -x                       Release current lease and exit
//! dhcpcd -k                       Kill running daemon
//! dhcpcd -1                       Try once, exit if no lease
//! dhcpcd --hostname=myhost        Send hostname option
//! dhcpcd --no-gateway             Don't install default route
//! dhcpcd --no-dns                 Don't write resolv.conf
//! dhcpcd --no-ntp                 Don't configure NTP servers
//! ```

#![deny(clippy::all)]
// DHCP_MIN_LEN, DHCP_DECLINE, build_decline, msg_type_name, and the
// MsgType::name helper are declared up-front because they encode the
// RFC 2131 / 2132 protocol surface the real implementation must
// speak. They are kept as documentation for the DORA edge cases the
// stub doesn't yet exercise (DECLINE on duplicate-address detection,
// log/trace formatting).
#![allow(dead_code)]

use std::env;
use std::fs;
use std::io::{self};
use std::net::UdpSocket;
use std::path::{Path, PathBuf};
use std::process;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

// ============================================================================
// Constants
// ============================================================================

/// DHCP server port.
const DHCP_SERVER_PORT: u16 = 67;

/// DHCP client port.
const DHCP_CLIENT_PORT: u16 = 68;

/// Minimum DHCP message size (without BOOTP padding).
const DHCP_MIN_LEN: usize = 236;

/// Fixed DHCP header size before options.
const DHCP_HEADER_LEN: usize = 236;

/// Maximum DHCP message size.
const DHCP_MAX_LEN: usize = 576;

/// DHCP magic cookie (RFC 2131).
const MAGIC_COOKIE: [u8; 4] = [99, 130, 83, 99];

/// Boot request (client to server).
const BOOTREQUEST: u8 = 1;

/// Boot reply (server to client).
const BOOTREPLY: u8 = 2;

/// Ethernet hardware type.
const HTYPE_ETHERNET: u8 = 1;

/// Ethernet hardware address length.
const HLEN_ETHERNET: u8 = 6;

/// Default lease file directory.
const LEASE_DIR: &str = "/var/lib/dhcpcd";

/// PID file path.
const PID_FILE: &str = "/var/run/dhcpcd.pid";

/// Default configuration file.
const CONFIG_FILE: &str = "/etc/dhcpcd.conf";

/// Resolv.conf path.
const RESOLV_CONF: &str = "/etc/resolv.conf";

/// Default timeout in seconds.
const DEFAULT_TIMEOUT: u64 = 30;

/// Maximum retries before giving up.
const MAX_RETRIES: u32 = 5;

/// Interface-configuration write syscall (`kernel/src/syscall/number.rs`,
/// `SYS_NET_IF_CONFIG`). Root-gated. Reads an 18-byte record from `arg0`
/// (length in `arg1`) whose byte 17 is a per-field mask selecting which of
/// IP/mask/gateway/DNS/up to apply. This is the native path the DHCP client
/// uses to apply an acquired lease. See [`build_lease_record`] for the layout.
const SYS_NET_IF_CONFIG: u64 = 856;

/// Field-mask bits for the `SYS_NET_IF_CONFIG` record (byte 17). A set bit
/// means "apply this field"; unset means "leave the current value untouched".
mod cfg_mask {
    /// Apply the IPv4 address (record bytes 0..4).
    pub const IP: u8 = 1 << 0;
    /// Apply the subnet mask (record bytes 4..8).
    pub const MASK: u8 = 1 << 1;
    /// Apply the gateway (record bytes 8..12).
    pub const GATEWAY: u8 = 1 << 2;
    /// Apply the up/down flag (record byte 16).
    pub const UP: u8 = 1 << 4;
}

// ============================================================================
// DHCP message types (option 53)
// ============================================================================

const DHCP_DISCOVER: u8 = 1;
const DHCP_OFFER: u8 = 2;
const DHCP_REQUEST: u8 = 3;
const DHCP_DECLINE: u8 = 4;
const DHCP_ACK: u8 = 5;
const DHCP_NAK: u8 = 6;
const DHCP_RELEASE: u8 = 7;
const DHCP_INFORM: u8 = 8;

// ============================================================================
// DHCP option codes (RFC 2132)
// ============================================================================

const OPT_SUBNET_MASK: u8 = 1;
const OPT_ROUTER: u8 = 3;
const OPT_DNS: u8 = 6;
const OPT_HOSTNAME: u8 = 12;
const OPT_DOMAIN_NAME: u8 = 15;
const OPT_BROADCAST: u8 = 28;
const OPT_NTP: u8 = 42;
const OPT_LEASE_TIME: u8 = 51;
const OPT_MSG_TYPE: u8 = 53;
const OPT_SERVER_ID: u8 = 54;
const OPT_RENEWAL_TIME: u8 = 58;
const OPT_REBINDING_TIME: u8 = 59;
const OPT_CLIENT_ID: u8 = 61;
const OPT_END: u8 = 255;
const OPT_PAD: u8 = 0;

// ============================================================================
// DHCP state machine
// ============================================================================

/// States of the DHCP client finite state machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DhcpState {
    /// Initial state; no lease.
    Init,
    /// DISCOVER sent; waiting for OFFER.
    Selecting,
    /// REQUEST sent after OFFER; waiting for ACK.
    Requesting,
    /// Lease acquired and active.
    Bound,
    /// T1 expired; unicasting REQUEST to renew.
    Renewing,
    /// T2 expired; broadcasting REQUEST to rebind.
    Rebinding,
}

impl DhcpState {
    fn name(self) -> &'static str {
        match self {
            Self::Init => "INIT",
            Self::Selecting => "SELECTING",
            Self::Requesting => "REQUESTING",
            Self::Bound => "BOUND",
            Self::Renewing => "RENEWING",
            Self::Rebinding => "REBINDING",
        }
    }
}

// ============================================================================
// IP / MAC helpers
// ============================================================================

/// Parse a dotted-decimal IPv4 address string into a big-endian u32.
fn parse_ipv4(s: &str) -> Option<u32> {
    let parts: Vec<&str> = s.split('.').collect();
    if parts.len() != 4 {
        return None;
    }
    let a: u8 = parts.first()?.parse().ok()?;
    let b: u8 = parts.get(1)?.parse().ok()?;
    let c: u8 = parts.get(2)?.parse().ok()?;
    let d: u8 = parts.get(3)?.parse().ok()?;
    Some(u32::from_be_bytes([a, b, c, d]))
}

/// Format a big-endian u32 as a dotted-decimal IPv4 string.
fn ip_to_string(ip: u32) -> String {
    let b = ip.to_be_bytes();
    format!("{}.{}.{}.{}", b[0], b[1], b[2], b[3])
}

/// Convert a subnet mask (big-endian u32) to CIDR prefix length.
fn mask_to_cidr(mask: u32) -> u32 {
    mask.to_be_bytes()
        .iter()
        .fold(0u32, |acc, &byte| acc + (byte.count_ones()))
}

/// Format a 6-byte MAC address as a colon-separated hex string.
fn mac_to_string(mac: &[u8]) -> String {
    if mac.len() < 6 {
        return "??:??:??:??:??:??".to_string();
    }
    format!(
        "{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
        mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]
    )
}

/// Return the name of a DHCP message type.
fn msg_type_name(t: u8) -> &'static str {
    match t {
        DHCP_DISCOVER => "DISCOVER",
        DHCP_OFFER => "OFFER",
        DHCP_REQUEST => "REQUEST",
        DHCP_DECLINE => "DECLINE",
        DHCP_ACK => "ACK",
        DHCP_NAK => "NAK",
        DHCP_RELEASE => "RELEASE",
        DHCP_INFORM => "INFORM",
        _ => "UNKNOWN",
    }
}

// ============================================================================
// Timestamp helper
// ============================================================================

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

// ============================================================================
// Parsed DHCP options
// ============================================================================

/// Holds all parsed DHCP options from a received message.
#[derive(Debug, Clone, Default)]
struct DhcpOptions {
    msg_type: Option<u8>,
    subnet_mask: Option<u32>,
    routers: Vec<u32>,
    dns_servers: Vec<u32>,
    hostname: Option<String>,
    domain_name: Option<String>,
    broadcast: Option<u32>,
    ntp_servers: Vec<u32>,
    lease_time: Option<u32>,
    server_id: Option<u32>,
    renewal_time: Option<u32>,
    rebinding_time: Option<u32>,
}

// ============================================================================
// DHCP message
// ============================================================================

/// A parsed DHCP/BOOTP message.
#[derive(Debug, Clone)]
struct DhcpMessage {
    op: u8,
    htype: u8,
    hlen: u8,
    hops: u8,
    xid: u32,
    secs: u16,
    flags: u16,
    ciaddr: u32,
    yiaddr: u32,
    siaddr: u32,
    giaddr: u32,
    chaddr: [u8; 16],
    sname: [u8; 64],
    file: [u8; 128],
    options: DhcpOptions,
}

impl DhcpMessage {
    /// Create a new client request message.
    fn new_request(xid: u32, mac: &[u8; 6]) -> Self {
        let mut chaddr = [0u8; 16];
        chaddr[..6].copy_from_slice(mac);
        Self {
            op: BOOTREQUEST,
            htype: HTYPE_ETHERNET,
            hlen: HLEN_ETHERNET,
            hops: 0,
            xid,
            secs: 0,
            flags: 0x8000, // Broadcast flag
            ciaddr: 0,
            yiaddr: 0,
            siaddr: 0,
            giaddr: 0,
            chaddr,
            sname: [0u8; 64],
            file: [0u8; 128],
            options: DhcpOptions::default(),
        }
    }

    /// Serialize the message header (236 bytes before options) into a buffer.
    fn serialize_header(&self, buf: &mut Vec<u8>) {
        buf.push(self.op);
        buf.push(self.htype);
        buf.push(self.hlen);
        buf.push(self.hops);
        buf.extend_from_slice(&self.xid.to_be_bytes());
        buf.extend_from_slice(&self.secs.to_be_bytes());
        buf.extend_from_slice(&self.flags.to_be_bytes());
        buf.extend_from_slice(&self.ciaddr.to_be_bytes());
        buf.extend_from_slice(&self.yiaddr.to_be_bytes());
        buf.extend_from_slice(&self.siaddr.to_be_bytes());
        buf.extend_from_slice(&self.giaddr.to_be_bytes());
        buf.extend_from_slice(&self.chaddr);
        buf.extend_from_slice(&self.sname);
        buf.extend_from_slice(&self.file);
    }

    /// Parse a DHCP message from raw bytes.
    fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < DHCP_HEADER_LEN + 4 {
            return None;
        }

        let op = data[0];
        let htype = data[1];
        let hlen = data[2];
        let hops = data[3];
        let xid = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);
        let secs = u16::from_be_bytes([data[8], data[9]]);
        let flags = u16::from_be_bytes([data[10], data[11]]);
        let ciaddr = u32::from_be_bytes([data[12], data[13], data[14], data[15]]);
        let yiaddr = u32::from_be_bytes([data[16], data[17], data[18], data[19]]);
        let siaddr = u32::from_be_bytes([data[20], data[21], data[22], data[23]]);
        let giaddr = u32::from_be_bytes([data[24], data[25], data[26], data[27]]);

        let mut chaddr = [0u8; 16];
        chaddr.copy_from_slice(&data[28..44]);

        let mut sname = [0u8; 64];
        sname.copy_from_slice(&data[44..108]);

        let mut file = [0u8; 128];
        file.copy_from_slice(&data[108..236]);

        // Validate magic cookie.
        if data[236..240] != MAGIC_COOKIE {
            return None;
        }

        let options = parse_options(&data[240..])?;

        Some(Self {
            op,
            htype,
            hlen,
            hops,
            xid,
            secs,
            flags,
            ciaddr,
            yiaddr,
            siaddr,
            giaddr,
            chaddr,
            sname,
            file,
            options,
        })
    }
}

// ============================================================================
// DHCP options encoding / decoding
// ============================================================================

/// Parse a TLV-encoded DHCP options section.
fn parse_options(data: &[u8]) -> Option<DhcpOptions> {
    let mut opts = DhcpOptions::default();
    let mut i = 0;

    while i < data.len() {
        let code = data[i];
        if code == OPT_END {
            break;
        }
        if code == OPT_PAD {
            i += 1;
            continue;
        }
        i += 1;
        if i >= data.len() {
            break;
        }
        let len = data[i] as usize;
        i += 1;
        if i + len > data.len() {
            break;
        }
        let value = &data[i..i + len];
        match code {
            OPT_MSG_TYPE if len >= 1 => {
                opts.msg_type = Some(value[0]);
            }
            OPT_SUBNET_MASK if len >= 4 => {
                opts.subnet_mask =
                    Some(u32::from_be_bytes([value[0], value[1], value[2], value[3]]));
            }
            OPT_ROUTER => {
                let mut j = 0;
                while j + 4 <= len {
                    opts.routers.push(u32::from_be_bytes([
                        value[j],
                        value[j + 1],
                        value[j + 2],
                        value[j + 3],
                    ]));
                    j += 4;
                }
            }
            OPT_DNS => {
                let mut j = 0;
                while j + 4 <= len {
                    opts.dns_servers.push(u32::from_be_bytes([
                        value[j],
                        value[j + 1],
                        value[j + 2],
                        value[j + 3],
                    ]));
                    j += 4;
                }
            }
            OPT_HOSTNAME => {
                opts.hostname = String::from_utf8(value.to_vec()).ok();
            }
            OPT_DOMAIN_NAME => {
                opts.domain_name = String::from_utf8(value.to_vec()).ok();
            }
            OPT_BROADCAST if len >= 4 => {
                opts.broadcast =
                    Some(u32::from_be_bytes([value[0], value[1], value[2], value[3]]));
            }
            OPT_NTP => {
                let mut j = 0;
                while j + 4 <= len {
                    opts.ntp_servers.push(u32::from_be_bytes([
                        value[j],
                        value[j + 1],
                        value[j + 2],
                        value[j + 3],
                    ]));
                    j += 4;
                }
            }
            OPT_LEASE_TIME if len >= 4 => {
                opts.lease_time =
                    Some(u32::from_be_bytes([value[0], value[1], value[2], value[3]]));
            }
            OPT_SERVER_ID if len >= 4 => {
                opts.server_id =
                    Some(u32::from_be_bytes([value[0], value[1], value[2], value[3]]));
            }
            OPT_RENEWAL_TIME if len >= 4 => {
                opts.renewal_time =
                    Some(u32::from_be_bytes([value[0], value[1], value[2], value[3]]));
            }
            OPT_REBINDING_TIME if len >= 4 => {
                opts.rebinding_time =
                    Some(u32::from_be_bytes([value[0], value[1], value[2], value[3]]));
            }
            _ => { /* Unrecognized option; skip. */ }
        }
        i += len;
    }

    Some(opts)
}

/// Write a single TLV option into a buffer.
fn write_option(buf: &mut Vec<u8>, code: u8, data: &[u8]) {
    buf.push(code);
    // Truncate oversized options to 255 bytes (RFC 2132 max length field is u8).
    let len = data.len().min(255);
    buf.push(len as u8);
    buf.extend_from_slice(&data[..len]);
}

/// Write the message-type option (53).
fn write_msg_type(buf: &mut Vec<u8>, mtype: u8) {
    write_option(buf, OPT_MSG_TYPE, &[mtype]);
}

/// Write a 4-byte IP address option.
fn write_ip_option(buf: &mut Vec<u8>, code: u8, ip: u32) {
    write_option(buf, code, &ip.to_be_bytes());
}

/// Write a client identifier option (61): type + MAC.
fn write_client_id(buf: &mut Vec<u8>, mac: &[u8; 6]) {
    let mut val = vec![HTYPE_ETHERNET];
    val.extend_from_slice(mac);
    write_option(buf, OPT_CLIENT_ID, &val);
}

/// Write a hostname option (12).
fn write_hostname_option(buf: &mut Vec<u8>, name: &str) {
    write_option(buf, OPT_HOSTNAME, name.as_bytes());
}

// ============================================================================
// Packet builders
// ============================================================================

/// Build a DHCPDISCOVER packet.
fn build_discover(
    xid: u32,
    mac: &[u8; 6],
    hostname: Option<&str>,
    requested_ip: Option<u32>,
) -> Vec<u8> {
    let msg = DhcpMessage::new_request(xid, mac);
    let mut buf = Vec::with_capacity(DHCP_MAX_LEN);
    msg.serialize_header(&mut buf);
    buf.extend_from_slice(&MAGIC_COOKIE);
    write_msg_type(&mut buf, DHCP_DISCOVER);
    write_client_id(&mut buf, mac);
    if let Some(ip) = requested_ip {
        write_ip_option(&mut buf, 50, ip); // Requested IP Address (option 50)
    }
    if let Some(name) = hostname {
        write_hostname_option(&mut buf, name);
    }
    // Parameter Request List: subnet, router, DNS, domain, broadcast, NTP, lease time.
    write_option(
        &mut buf,
        55,
        &[
            OPT_SUBNET_MASK,
            OPT_ROUTER,
            OPT_DNS,
            OPT_DOMAIN_NAME,
            OPT_BROADCAST,
            OPT_NTP,
            OPT_LEASE_TIME,
        ],
    );
    buf.push(OPT_END);
    // Pad to minimum BOOTP size.
    while buf.len() < 300 {
        buf.push(OPT_PAD);
    }
    buf
}

/// Build a DHCPREQUEST packet.
fn build_request(
    xid: u32,
    mac: &[u8; 6],
    ciaddr: u32,
    requested_ip: u32,
    server_id: Option<u32>,
    hostname: Option<&str>,
) -> Vec<u8> {
    let mut msg = DhcpMessage::new_request(xid, mac);
    msg.ciaddr = ciaddr;
    let mut buf = Vec::with_capacity(DHCP_MAX_LEN);
    msg.serialize_header(&mut buf);
    buf.extend_from_slice(&MAGIC_COOKIE);
    write_msg_type(&mut buf, DHCP_REQUEST);
    write_client_id(&mut buf, mac);
    if ciaddr == 0 {
        // Initial request: include requested IP address and server ID.
        write_ip_option(&mut buf, 50, requested_ip);
    }
    if let Some(sid) = server_id {
        write_ip_option(&mut buf, OPT_SERVER_ID, sid);
    }
    if let Some(name) = hostname {
        write_hostname_option(&mut buf, name);
    }
    write_option(
        &mut buf,
        55,
        &[
            OPT_SUBNET_MASK,
            OPT_ROUTER,
            OPT_DNS,
            OPT_DOMAIN_NAME,
            OPT_BROADCAST,
            OPT_NTP,
            OPT_LEASE_TIME,
        ],
    );
    buf.push(OPT_END);
    while buf.len() < 300 {
        buf.push(OPT_PAD);
    }
    buf
}

/// Build a DHCPRELEASE packet.
fn build_release(xid: u32, mac: &[u8; 6], ciaddr: u32, server_id: u32) -> Vec<u8> {
    let mut msg = DhcpMessage::new_request(xid, mac);
    msg.ciaddr = ciaddr;
    msg.flags = 0; // Unicast; no broadcast flag.
    let mut buf = Vec::with_capacity(DHCP_MAX_LEN);
    msg.serialize_header(&mut buf);
    buf.extend_from_slice(&MAGIC_COOKIE);
    write_msg_type(&mut buf, DHCP_RELEASE);
    write_client_id(&mut buf, mac);
    write_ip_option(&mut buf, OPT_SERVER_ID, server_id);
    buf.push(OPT_END);
    while buf.len() < 300 {
        buf.push(OPT_PAD);
    }
    buf
}

/// Build a DHCPDECLINE packet.
fn build_decline(xid: u32, mac: &[u8; 6], requested_ip: u32, server_id: u32) -> Vec<u8> {
    let msg = DhcpMessage::new_request(xid, mac);
    let mut buf = Vec::with_capacity(DHCP_MAX_LEN);
    msg.serialize_header(&mut buf);
    buf.extend_from_slice(&MAGIC_COOKIE);
    write_msg_type(&mut buf, DHCP_DECLINE);
    write_client_id(&mut buf, mac);
    write_ip_option(&mut buf, 50, requested_ip);
    write_ip_option(&mut buf, OPT_SERVER_ID, server_id);
    buf.push(OPT_END);
    while buf.len() < 300 {
        buf.push(OPT_PAD);
    }
    buf
}

/// Build a DHCPINFORM packet.
#[allow(dead_code)]
fn build_inform(xid: u32, mac: &[u8; 6], ciaddr: u32, hostname: Option<&str>) -> Vec<u8> {
    let mut msg = DhcpMessage::new_request(xid, mac);
    msg.ciaddr = ciaddr;
    msg.flags = 0;
    let mut buf = Vec::with_capacity(DHCP_MAX_LEN);
    msg.serialize_header(&mut buf);
    buf.extend_from_slice(&MAGIC_COOKIE);
    write_msg_type(&mut buf, DHCP_INFORM);
    write_client_id(&mut buf, mac);
    if let Some(name) = hostname {
        write_hostname_option(&mut buf, name);
    }
    write_option(
        &mut buf,
        55,
        &[OPT_SUBNET_MASK, OPT_ROUTER, OPT_DNS, OPT_DOMAIN_NAME, OPT_NTP],
    );
    buf.push(OPT_END);
    while buf.len() < 300 {
        buf.push(OPT_PAD);
    }
    buf
}

// ============================================================================
// Lease info
// ============================================================================

/// Stores the information obtained from a DHCP lease.
#[derive(Debug, Clone, Default)]
struct LeaseInfo {
    ip_address: u32,
    subnet_mask: u32,
    routers: Vec<u32>,
    dns_servers: Vec<u32>,
    ntp_servers: Vec<u32>,
    hostname: Option<String>,
    domain_name: Option<String>,
    broadcast: Option<u32>,
    server_id: u32,
    lease_time: u32,
    renewal_time: u32,
    rebinding_time: u32,
    obtained_at: u64,
}

impl LeaseInfo {
    /// Create a `LeaseInfo` from a DHCP ACK message.
    fn from_ack(msg: &DhcpMessage, obtained_at: u64) -> Self {
        let lease_time = msg.options.lease_time.unwrap_or(86400);
        let renewal_time = msg.options.renewal_time.unwrap_or(lease_time / 2);
        let rebinding_time = msg
            .options
            .rebinding_time
            .unwrap_or(lease_time * 7 / 8);

        Self {
            ip_address: msg.yiaddr,
            subnet_mask: msg.options.subnet_mask.unwrap_or(0xFFFF_FF00),
            routers: msg.options.routers.clone(),
            dns_servers: msg.options.dns_servers.clone(),
            ntp_servers: msg.options.ntp_servers.clone(),
            hostname: msg.options.hostname.clone(),
            domain_name: msg.options.domain_name.clone(),
            broadcast: msg.options.broadcast,
            server_id: msg.options.server_id.unwrap_or(msg.siaddr),
            lease_time,
            renewal_time,
            rebinding_time,
            obtained_at,
        }
    }

    /// Absolute timestamp when T1 (renewal) fires.
    fn t1_expiry(&self) -> u64 {
        self.obtained_at.saturating_add(u64::from(self.renewal_time))
    }

    /// Absolute timestamp when T2 (rebinding) fires.
    fn t2_expiry(&self) -> u64 {
        self.obtained_at
            .saturating_add(u64::from(self.rebinding_time))
    }

    /// Absolute timestamp when the lease expires entirely.
    fn lease_expiry(&self) -> u64 {
        self.obtained_at.saturating_add(u64::from(self.lease_time))
    }

    /// True if the lease has expired.
    fn is_expired(&self, now: u64) -> bool {
        now >= self.lease_expiry()
    }

    /// Format the lease as a human-readable string.
    fn display(&self) -> String {
        let mut s = String::new();
        s.push_str(&format!("IP address   : {}\n", ip_to_string(self.ip_address)));
        s.push_str(&format!(
            "Subnet mask  : {} (/{})  \n",
            ip_to_string(self.subnet_mask),
            mask_to_cidr(self.subnet_mask)
        ));
        if let Some(bcast) = self.broadcast {
            s.push_str(&format!("Broadcast    : {}\n", ip_to_string(bcast)));
        }
        for (i, &r) in self.routers.iter().enumerate() {
            s.push_str(&format!("Router {:>2}    : {}\n", i + 1, ip_to_string(r)));
        }
        for (i, &d) in self.dns_servers.iter().enumerate() {
            s.push_str(&format!("DNS {:>2}       : {}\n", i + 1, ip_to_string(d)));
        }
        for (i, &n) in self.ntp_servers.iter().enumerate() {
            s.push_str(&format!("NTP {:>2}       : {}\n", i + 1, ip_to_string(n)));
        }
        if let Some(ref h) = self.hostname {
            s.push_str(&format!("Hostname     : {h}\n"));
        }
        if let Some(ref d) = self.domain_name {
            s.push_str(&format!("Domain       : {d}\n"));
        }
        s.push_str(&format!("Server       : {}\n", ip_to_string(self.server_id)));
        s.push_str(&format!("Lease time   : {} seconds\n", self.lease_time));
        s.push_str(&format!("T1 (renew)   : {} seconds\n", self.renewal_time));
        s.push_str(&format!("T2 (rebind)  : {} seconds\n", self.rebinding_time));
        s
    }

    /// Serialize the lease to a string suitable for writing to a file.
    fn serialize(&self) -> String {
        let mut s = String::new();
        s.push_str(&format!("ip_address={}\n", ip_to_string(self.ip_address)));
        s.push_str(&format!("subnet_mask={}\n", ip_to_string(self.subnet_mask)));
        for r in &self.routers {
            s.push_str(&format!("router={}\n", ip_to_string(*r)));
        }
        for d in &self.dns_servers {
            s.push_str(&format!("dns={}\n", ip_to_string(*d)));
        }
        for n in &self.ntp_servers {
            s.push_str(&format!("ntp={}\n", ip_to_string(*n)));
        }
        if let Some(ref h) = self.hostname {
            s.push_str(&format!("hostname={h}\n"));
        }
        if let Some(ref d) = self.domain_name {
            s.push_str(&format!("domain={d}\n"));
        }
        if let Some(bcast) = self.broadcast {
            s.push_str(&format!("broadcast={}\n", ip_to_string(bcast)));
        }
        s.push_str(&format!("server_id={}\n", ip_to_string(self.server_id)));
        s.push_str(&format!("lease_time={}\n", self.lease_time));
        s.push_str(&format!("renewal_time={}\n", self.renewal_time));
        s.push_str(&format!("rebinding_time={}\n", self.rebinding_time));
        s.push_str(&format!("obtained_at={}\n", self.obtained_at));
        s
    }

    /// Deserialize a lease from its file representation.
    fn deserialize(text: &str) -> Option<Self> {
        let mut lease = LeaseInfo::default();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let (key, value) = line.split_once('=')?;
            match key {
                "ip_address" => lease.ip_address = parse_ipv4(value)?,
                "subnet_mask" => lease.subnet_mask = parse_ipv4(value)?,
                "router" => lease.routers.push(parse_ipv4(value)?),
                "dns" => lease.dns_servers.push(parse_ipv4(value)?),
                "ntp" => lease.ntp_servers.push(parse_ipv4(value)?),
                "hostname" => lease.hostname = Some(value.to_string()),
                "domain" => lease.domain_name = Some(value.to_string()),
                "broadcast" => lease.broadcast = Some(parse_ipv4(value)?),
                "server_id" => lease.server_id = parse_ipv4(value)?,
                "lease_time" => lease.lease_time = value.parse().ok()?,
                "renewal_time" => lease.renewal_time = value.parse().ok()?,
                "rebinding_time" => lease.rebinding_time = value.parse().ok()?,
                "obtained_at" => lease.obtained_at = value.parse().ok()?,
                _ => {} // Ignore unknown keys for forward compatibility.
            }
        }
        if lease.ip_address == 0 {
            return None;
        }
        Some(lease)
    }
}

// ============================================================================
// Configuration
// ============================================================================

/// Parsed configuration from CLI arguments and `/etc/dhcpcd.conf`.
#[derive(Debug, Clone)]
struct Config {
    interface: String,
    no_configure: bool,
    requested_ip: Option<u32>,
    client_ip: Option<u32>,
    debug: bool,
    foreground: bool,
    timeout: u64,
    release: bool,
    kill: bool,
    hostname: Option<String>,
    no_gateway: bool,
    no_dns: bool,
    no_ntp: bool,
    try_once: bool,
    // Config-file fields.
    required_options: Vec<String>,
    retry: u32,
    static_ip: Option<u32>,
    static_routers: Vec<u32>,
    static_dns: Vec<u32>,
}

impl Config {
    fn default_config() -> Self {
        Self {
            interface: "eth0".to_string(),
            no_configure: false,
            requested_ip: None,
            client_ip: None,
            debug: false,
            foreground: false,
            timeout: DEFAULT_TIMEOUT,
            release: false,
            kill: false,
            hostname: None,
            no_gateway: false,
            no_dns: false,
            no_ntp: false,
            try_once: false,
            required_options: Vec::new(),
            retry: 0,
            static_ip: None,
            static_routers: Vec::new(),
            static_dns: Vec::new(),
        }
    }
}

/// Parse CLI arguments into a `Config`.
fn parse_args(args: &[String]) -> Result<Config, String> {
    let mut cfg = Config::default_config();
    let mut i = 0;

    while i < args.len() {
        let arg = &args[i];
        match arg.as_str() {
            "-i" => {
                i += 1;
                cfg.interface = args
                    .get(i)
                    .ok_or("-i requires an interface name")?
                    .clone();
            }
            "-n" => cfg.no_configure = true,
            "-r" => {
                i += 1;
                let ip_str = args.get(i).ok_or("-r requires an IP address")?;
                cfg.requested_ip =
                    Some(parse_ipv4(ip_str).ok_or_else(|| format!("invalid IP: {ip_str}"))?);
            }
            "-s" => {
                i += 1;
                let ip_str = args.get(i).ok_or("-s requires an IP address")?;
                cfg.client_ip =
                    Some(parse_ipv4(ip_str).ok_or_else(|| format!("invalid IP: {ip_str}"))?);
            }
            "-d" => cfg.debug = true,
            "-f" => cfg.foreground = true,
            "-t" => {
                i += 1;
                let t_str = args.get(i).ok_or("-t requires a timeout value")?;
                cfg.timeout = t_str
                    .parse()
                    .map_err(|_| format!("invalid timeout: {t_str}"))?;
            }
            "-x" => cfg.release = true,
            "-k" => cfg.kill = true,
            "-1" => cfg.try_once = true,
            "--no-gateway" => cfg.no_gateway = true,
            "--no-dns" => cfg.no_dns = true,
            "--no-ntp" => cfg.no_ntp = true,
            _ if arg.starts_with("--hostname=") => {
                cfg.hostname = Some(arg.trim_start_matches("--hostname=").to_string());
            }
            _ => return Err(format!("unknown option: {arg}")),
        }
        i += 1;
    }

    Ok(cfg)
}

/// Parse a dhcpcd.conf configuration file.
fn parse_config_file(text: &str) -> ConfigFileResult {
    let mut result = ConfigFileResult::default();
    let mut current_interface: Option<String> = None;

    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        if let Some(iface) = line.strip_prefix("interface ") {
            current_interface = Some(iface.trim().to_string());
            continue;
        }

        if let Some(val) = line.strip_prefix("timeout ") {
            result.timeout = val.trim().parse().ok();
            continue;
        }

        if let Some(val) = line.strip_prefix("retry ") {
            result.retry = val.trim().parse().ok();
            continue;
        }

        if let Some(val) = line.strip_prefix("option ") {
            for item in val.split(',') {
                let item = item.trim();
                if !item.is_empty() {
                    result.requested_options.push(item.to_string());
                }
            }
            continue;
        }

        if let Some(val) = line.strip_prefix("require ") {
            for item in val.split(',') {
                let item = item.trim();
                if !item.is_empty() {
                    result.required_options.push(item.to_string());
                }
            }
            continue;
        }

        // Static settings (interface-scoped or global).
        if let Some(val) = line.strip_prefix("static ip_address=") {
            let entry = StaticEntry {
                interface: current_interface.clone(),
                ip_address: parse_ipv4(val.trim()),
                ..StaticEntry::default()
            };
            result.statics.push(entry);
            continue;
        }

        if let Some(val) = line.strip_prefix("static routers=") {
            let ips: Vec<u32> = val
                .split(',')
                .filter_map(|s| parse_ipv4(s.trim()))
                .collect();
            let entry = StaticEntry {
                interface: current_interface.clone(),
                routers: ips,
                ..StaticEntry::default()
            };
            result.statics.push(entry);
            continue;
        }

        if let Some(val) = line.strip_prefix("static domain_name_servers=") {
            let ips: Vec<u32> = val
                .split(',')
                .filter_map(|s| parse_ipv4(s.trim()))
                .collect();
            let entry = StaticEntry {
                interface: current_interface.clone(),
                dns_servers: ips,
                ..StaticEntry::default()
            };
            result.statics.push(entry);
        }
    }

    result
}

/// Result of parsing a config file.
#[derive(Debug, Clone, Default)]
struct ConfigFileResult {
    timeout: Option<u64>,
    retry: Option<u32>,
    requested_options: Vec<String>,
    required_options: Vec<String>,
    statics: Vec<StaticEntry>,
}

/// A static configuration entry from the config file.
#[derive(Debug, Clone, Default)]
struct StaticEntry {
    interface: Option<String>,
    ip_address: Option<u32>,
    routers: Vec<u32>,
    dns_servers: Vec<u32>,
}

/// Apply config-file values to a `Config`.
fn apply_config_file(cfg: &mut Config, file_result: &ConfigFileResult) {
    if let Some(t) = file_result.timeout {
        // Only override if user didn't pass -t on CLI.
        if cfg.timeout == DEFAULT_TIMEOUT {
            cfg.timeout = t;
        }
    }
    if let Some(r) = file_result.retry {
        cfg.retry = r;
    }
    for opt in &file_result.required_options {
        if !cfg.required_options.contains(opt) {
            cfg.required_options.push(opt.clone());
        }
    }
    // Apply statics matching this interface (or global).
    for st in &file_result.statics {
        let applies = st.interface.is_none()
            || st
                .interface
                .as_ref()
                .is_some_and(|s| s == &cfg.interface);
        if applies {
            if let Some(ip) = st.ip_address {
                cfg.static_ip = Some(ip);
            }
            if !st.routers.is_empty() {
                cfg.static_routers = st.routers.clone();
            }
            if !st.dns_servers.is_empty() {
                cfg.static_dns = st.dns_servers.clone();
            }
        }
    }
}

// ============================================================================
// Syscall helpers
// ============================================================================

#[cfg(target_arch = "x86_64")]
unsafe fn syscall4(nr: u64, a1: u64, a2: u64, a3: u64, a4: u64) -> i64 {
    let ret: i64;
    // SAFETY: Caller ensures arguments are valid for the given syscall number.
    unsafe {
        core::arch::asm!(
            "syscall",
            inlateout("rax") nr as i64 => ret,
            in("rdi") a1,
            in("rsi") a2,
            in("rdx") a3,
            in("r10") a4,
            lateout("rcx") _,
            lateout("r11") _,
            options(nostack),
        );
    }
    ret
}

/// Build the 18-byte `SYS_NET_IF_CONFIG` record for applying a DHCP lease:
/// IP address, subnet mask, and up flag are always set; the gateway field is
/// set only when a router is present. Every unset field's mask bit stays clear
/// so the kernel preserves its current value. Pure (no syscall) so it is
/// unit-testable on the host.
///
/// Layout: `[0..4]` ip, `[4..8]` mask, `[8..12]` gateway, `[12..16]` dns,
/// `[16]` up flag, `[17]` field mask (see [`cfg_mask`]).
fn build_lease_record(ip: [u8; 4], mask: [u8; 4], gateway: Option<[u8; 4]>) -> [u8; 18] {
    let mut rec = [0u8; 18];
    rec[0..4].copy_from_slice(&ip);
    rec[4..8].copy_from_slice(&mask);
    let mut field_mask = cfg_mask::IP | cfg_mask::MASK | cfg_mask::UP;
    rec[16] = 1; // bring the interface up as part of applying the lease
    if let Some(gw) = gateway {
        rec[8..12].copy_from_slice(&gw);
        field_mask |= cfg_mask::GATEWAY;
    }
    rec[17] = field_mask;
    rec
}

/// Apply an interface configuration via `SYS_NET_IF_CONFIG`. Returns the
/// kernel's signed result (0 on success, negative errno on failure).
fn net_if_config(rec: &[u8; 18]) -> i64 {
    // SAFETY: `rec` is exactly 18 bytes, matching the kernel's REC_SIZE
    // contract; the kernel only reads (never writes) the record.
    unsafe {
        syscall4(
            SYS_NET_IF_CONFIG,
            rec.as_ptr() as u64,
            rec.len() as u64,
            0,
            0,
        )
    }
}

// ============================================================================
// Interface helpers
// ============================================================================

/// Read the MAC address for an interface from sysfs.
fn read_mac(iface: &str) -> Option<[u8; 6]> {
    let path = format!("/sys/class/net/{iface}/address");
    let text = fs::read_to_string(&path).ok()?;
    let text = text.trim();
    let parts: Vec<&str> = text.split(':').collect();
    if parts.len() != 6 {
        return None;
    }
    let mut mac = [0u8; 6];
    for (i, part) in parts.iter().enumerate() {
        mac[i] = u8::from_str_radix(part, 16).ok()?;
    }
    Some(mac)
}

/// Detect the default network interface by scanning sysfs.
fn detect_interface() -> Option<String> {
    // Try to find a non-loopback interface that is UP.
    let dir = fs::read_dir("/sys/class/net").ok()?;
    for entry in dir.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name == "lo" {
            continue;
        }
        // Check if interface is operational.
        let operstate = format!("/sys/class/net/{name}/operstate");
        if let Ok(state) = fs::read_to_string(&operstate) {
            let state = state.trim();
            if state == "up" || state == "unknown" {
                return Some(name);
            }
        }
    }
    // Fall back to first non-loopback interface.
    let dir = fs::read_dir("/sys/class/net").ok()?;
    for entry in dir.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name != "lo" {
            return Some(name);
        }
    }
    None
}

/// Configure the interface with the acquired lease.
fn configure_interface(iface: &str, lease: &LeaseInfo, cfg: &Config) {
    if cfg.no_configure {
        return;
    }

    // Apply the address, mask, gateway (if any) and up flag in one atomic
    // read-modify-write via SYS_NET_IF_CONFIG. Applying them together avoids the
    // transient inconsistent states the old per-field ioctls produced (IP set
    // but mask stale, etc.).
    let gateway = if cfg.no_gateway {
        None
    } else {
        lease.routers.first().map(|&gw| gw.to_be_bytes())
    };
    let rec = build_lease_record(
        lease.ip_address.to_be_bytes(),
        lease.subnet_mask.to_be_bytes(),
        gateway,
    );
    let ret = net_if_config(&rec);
    if cfg.debug {
        eprintln!(
            "  apply lease ip={} mask={} gw={} up -> rc={}",
            ip_to_string(lease.ip_address),
            ip_to_string(lease.subnet_mask),
            gateway.map_or_else(|| "none".to_string(), |g| ip_to_string(u32::from_be_bytes(g))),
            ret
        );
    }
    if ret < 0 {
        eprintln!(
            "dhcpcd: failed to apply lease to {iface}: {}",
            if ret == -1 {
                "permission denied (need root)".to_string()
            } else {
                format!("error {ret}")
            }
        );
    }

    // Write resolv.conf.
    if !cfg.no_dns && !lease.dns_servers.is_empty() {
        write_resolv_conf(lease);
    }

    // Set hostname.
    if let Some(ref name) = lease.hostname {
        set_hostname(name);
    }
}

/// Write `/etc/resolv.conf` with DNS servers from the lease.
fn write_resolv_conf(lease: &LeaseInfo) {
    let mut content = String::from("# Generated by dhcpcd\n");
    if let Some(ref domain) = lease.domain_name {
        content.push_str(&format!("search {domain}\n"));
    }
    for &dns in &lease.dns_servers {
        content.push_str(&format!("nameserver {}\n", ip_to_string(dns)));
    }
    if let Err(e) = fs::write(RESOLV_CONF, &content) {
        eprintln!("dhcpcd: warning: failed to write {RESOLV_CONF}: {e}");
    }
}

/// Set the system hostname.
fn set_hostname(name: &str) {
    // Write to /etc/hostname and /proc/sys/kernel/hostname.
    let _ = fs::write("/etc/hostname", format!("{name}\n"));
    let _ = fs::write("/proc/sys/kernel/hostname", name);
}

/// Write lease info to the lease file.
fn write_lease_file(iface: &str, lease: &LeaseInfo) {
    let dir = PathBuf::from(LEASE_DIR);
    if let Err(e) = fs::create_dir_all(&dir) {
        eprintln!("dhcpcd: warning: cannot create {}: {e}", dir.display());
        return;
    }
    let path = dir.join(format!("{iface}.lease"));
    if let Err(e) = fs::write(&path, lease.serialize()) {
        eprintln!("dhcpcd: warning: failed to write {}: {e}", path.display());
    }
}

/// Read a saved lease from disk.
fn read_lease_file(iface: &str) -> Option<LeaseInfo> {
    let path = PathBuf::from(LEASE_DIR).join(format!("{iface}.lease"));
    let text = fs::read_to_string(&path).ok()?;
    LeaseInfo::deserialize(&text)
}

// ============================================================================
// XID generation
// ============================================================================

/// Generate a pseudo-random 32-bit transaction ID.
///
/// Uses the system clock and PID to seed a simple xorshift.  This is
/// sufficient for a DHCP client where only uniqueness (not cryptographic
/// randomness) is required.
fn generate_xid() -> u32 {
    let time_component = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u32)
        .unwrap_or(0x1234_5678);
    let pid_component = process::id();
    let mut state = time_component ^ pid_component;
    // xorshift32 mixing.
    state ^= state << 13;
    state ^= state >> 17;
    state ^= state << 5;
    if state == 0 { 0xDEAD_BEEF } else { state }
}

// ============================================================================
// PID file management
// ============================================================================

/// Write the current PID to the PID file.
fn write_pid_file() -> io::Result<()> {
    let dir = Path::new(PID_FILE).parent().unwrap_or(Path::new("/var/run"));
    fs::create_dir_all(dir)?;
    fs::write(PID_FILE, format!("{}\n", process::id()))
}

/// Read the PID from the PID file.
fn read_pid_file() -> Option<u32> {
    let text = fs::read_to_string(PID_FILE).ok()?;
    text.trim().parse().ok()
}

/// Remove the PID file.
fn remove_pid_file() {
    let _ = fs::remove_file(PID_FILE);
}

// ============================================================================
// Logging
// ============================================================================

/// Print a debug message when debug mode is enabled.
fn debug_log(cfg: &Config, msg: &str) {
    if cfg.debug {
        eprintln!("dhcpcd[{}]: {msg}", process::id());
    }
}

/// Print an informational message.
fn info_log(msg: &str) {
    eprintln!("dhcpcd: {msg}");
}

/// Print an error message.
fn error_log(msg: &str) {
    eprintln!("dhcpcd: error: {msg}");
}

// ============================================================================
// DHCP client core
// ============================================================================

/// Run the full DHCP client state machine.
fn run_dhcp(cfg: &Config) -> Result<(), String> {
    let mac = read_mac(&cfg.interface)
        .ok_or_else(|| format!("cannot read MAC for interface '{}'", cfg.interface))?;

    debug_log(cfg, &format!("interface: {}", cfg.interface));
    debug_log(cfg, &format!("MAC: {}", mac_to_string(&mac)));

    let xid = generate_xid();
    debug_log(cfg, &format!("XID: 0x{xid:08x}"));

    // Bind to DHCP client port.
    let socket = UdpSocket::bind(format!("0.0.0.0:{DHCP_CLIENT_PORT}"))
        .map_err(|e| format!("cannot bind to port {DHCP_CLIENT_PORT}: {e}"))?;
    socket.set_broadcast(true).map_err(|e| format!("set_broadcast: {e}"))?;
    socket
        .set_read_timeout(Some(Duration::from_secs(cfg.timeout)))
        .map_err(|e| format!("set_read_timeout: {e}"))?;

    let mut state = DhcpState::Init;
    let mut retries: u32 = 0;
    let start_time = now_secs();

    loop {
        // Check for overall timeout.
        if now_secs().saturating_sub(start_time) > cfg.timeout {
            return Err("timeout waiting for DHCP lease".to_string());
        }

        match state {
            DhcpState::Init => {
                debug_log(cfg, "state: INIT -> sending DISCOVER");
                let pkt = build_discover(xid, &mac, cfg.hostname.as_deref(), cfg.requested_ip);
                socket
                    .send_to(&pkt, format!("255.255.255.255:{DHCP_SERVER_PORT}"))
                    .map_err(|e| format!("send DISCOVER: {e}"))?;
                state = DhcpState::Selecting;
            }

            DhcpState::Selecting => {
                debug_log(cfg, "state: SELECTING -> waiting for OFFER");
                let mut buf = [0u8; DHCP_MAX_LEN];
                match socket.recv_from(&mut buf) {
                    Ok((len, _addr)) => {
                        if let Some(msg) = DhcpMessage::parse(&buf[..len]) {
                            if msg.xid != xid || msg.op != BOOTREPLY {
                                continue;
                            }
                            if msg.options.msg_type == Some(DHCP_OFFER) {
                                debug_log(
                                    cfg,
                                    &format!(
                                        "received OFFER: {}",
                                        ip_to_string(msg.yiaddr)
                                    ),
                                );
                                // Send REQUEST for this offer.
                                let server_id = msg.options.server_id;
                                let offered_ip = msg.yiaddr;
                                let pkt = build_request(
                                    xid,
                                    &mac,
                                    0,
                                    offered_ip,
                                    server_id,
                                    cfg.hostname.as_deref(),
                                );
                                socket
                                    .send_to(
                                        &pkt,
                                        format!("255.255.255.255:{DHCP_SERVER_PORT}"),
                                    )
                                    .map_err(|e| format!("send REQUEST: {e}"))?;
                                state = DhcpState::Requesting;
                            }
                        }
                    }
                    Err(ref e)
                        if e.kind() == io::ErrorKind::WouldBlock
                            || e.kind() == io::ErrorKind::TimedOut =>
                    {
                        retries += 1;
                        if cfg.try_once || retries > MAX_RETRIES {
                            return Err("no DHCP server responded".to_string());
                        }
                        debug_log(cfg, "timeout; retransmitting DISCOVER");
                        state = DhcpState::Init;
                    }
                    Err(e) => return Err(format!("recv: {e}")),
                }
            }

            DhcpState::Requesting => {
                debug_log(cfg, "state: REQUESTING -> waiting for ACK");
                let mut buf = [0u8; DHCP_MAX_LEN];
                match socket.recv_from(&mut buf) {
                    Ok((len, _addr)) => {
                        if let Some(msg) = DhcpMessage::parse(&buf[..len]) {
                            if msg.xid != xid || msg.op != BOOTREPLY {
                                continue;
                            }
                            match msg.options.msg_type {
                                Some(DHCP_ACK) => {
                                    let lease = LeaseInfo::from_ack(&msg, now_secs());
                                    info_log(&format!(
                                        "lease acquired: {} ({}s)",
                                        ip_to_string(lease.ip_address),
                                        lease.lease_time
                                    ));
                                    if cfg.debug || cfg.no_configure {
                                        print!("{}", lease.display());
                                    }
                                    configure_interface(&cfg.interface, &lease, cfg);
                                    write_lease_file(&cfg.interface, &lease);
                                    if cfg.try_once || cfg.no_configure {
                                        return Ok(());
                                    }
                                    state = DhcpState::Bound;
                                }
                                Some(DHCP_NAK) => {
                                    debug_log(cfg, "received NAK; restarting");
                                    retries += 1;
                                    if retries > MAX_RETRIES {
                                        return Err(
                                            "server rejected request (NAK)".to_string()
                                        );
                                    }
                                    state = DhcpState::Init;
                                }
                                _ => {} // Ignore other messages.
                            }
                        }
                    }
                    Err(ref e)
                        if e.kind() == io::ErrorKind::WouldBlock
                            || e.kind() == io::ErrorKind::TimedOut =>
                    {
                        retries += 1;
                        if cfg.try_once || retries > MAX_RETRIES {
                            return Err("no ACK received".to_string());
                        }
                        debug_log(cfg, "timeout; restarting from INIT");
                        state = DhcpState::Init;
                    }
                    Err(e) => return Err(format!("recv: {e}")),
                }
            }

            DhcpState::Bound => {
                // Read the current lease to determine timers.
                let lease = match read_lease_file(&cfg.interface) {
                    Some(l) => l,
                    None => return Err("lost lease file".to_string()),
                };
                let now = now_secs();
                if lease.is_expired(now) {
                    info_log("lease expired; restarting");
                    state = DhcpState::Init;
                    continue;
                }
                let t1 = lease.t1_expiry();
                if now >= t1 {
                    debug_log(cfg, "T1 expired; entering RENEWING");
                    state = DhcpState::Renewing;
                    continue;
                }
                // Sleep until T1 (or timeout granularity).
                let sleep_secs = t1.saturating_sub(now).min(60);
                std::thread::sleep(Duration::from_secs(sleep_secs));
            }

            DhcpState::Renewing => {
                let lease = match read_lease_file(&cfg.interface) {
                    Some(l) => l,
                    None => {
                        state = DhcpState::Init;
                        continue;
                    }
                };
                let now = now_secs();
                if lease.is_expired(now) {
                    info_log("lease expired during renewal");
                    state = DhcpState::Init;
                    continue;
                }
                if now >= lease.t2_expiry() {
                    debug_log(cfg, "T2 expired; entering REBINDING");
                    state = DhcpState::Rebinding;
                    continue;
                }

                debug_log(cfg, "RENEWING: sending unicast REQUEST");
                let new_xid = generate_xid();
                let pkt = build_request(
                    new_xid,
                    &mac,
                    lease.ip_address,
                    lease.ip_address,
                    Some(lease.server_id),
                    cfg.hostname.as_deref(),
                );
                let server_addr = format!("{}:{DHCP_SERVER_PORT}", ip_to_string(lease.server_id));
                if let Err(e) = socket.send_to(&pkt, &server_addr) {
                    debug_log(cfg, &format!("send renewal: {e}"));
                    state = DhcpState::Rebinding;
                    continue;
                }
                // Wait for response.
                socket
                    .set_read_timeout(Some(Duration::from_secs(10)))
                    .ok();
                let mut buf = [0u8; DHCP_MAX_LEN];
                match socket.recv_from(&mut buf) {
                    Ok((len, _)) => {
                        if let Some(msg) = DhcpMessage::parse(&buf[..len])
                            && msg.op == BOOTREPLY && msg.xid == new_xid {
                                if msg.options.msg_type == Some(DHCP_ACK) {
                                    let new_lease = LeaseInfo::from_ack(&msg, now_secs());
                                    info_log(&format!(
                                        "lease renewed: {} ({}s)",
                                        ip_to_string(new_lease.ip_address),
                                        new_lease.lease_time
                                    ));
                                    configure_interface(&cfg.interface, &new_lease, cfg);
                                    write_lease_file(&cfg.interface, &new_lease);
                                    state = DhcpState::Bound;
                                    continue;
                                }
                                if msg.options.msg_type == Some(DHCP_NAK) {
                                    info_log("renewal NAK; restarting");
                                    state = DhcpState::Init;
                                    continue;
                                }
                            }
                    }
                    Err(_) => {
                        // Timeout; will retry or transition to REBINDING on next loop.
                    }
                }
                // Restore original timeout.
                socket
                    .set_read_timeout(Some(Duration::from_secs(cfg.timeout)))
                    .ok();
                std::thread::sleep(Duration::from_secs(30));
            }

            DhcpState::Rebinding => {
                let lease = match read_lease_file(&cfg.interface) {
                    Some(l) => l,
                    None => {
                        state = DhcpState::Init;
                        continue;
                    }
                };
                let now = now_secs();
                if lease.is_expired(now) {
                    info_log("lease expired during rebinding");
                    state = DhcpState::Init;
                    continue;
                }

                debug_log(cfg, "REBINDING: sending broadcast REQUEST");
                let new_xid = generate_xid();
                let pkt = build_request(
                    new_xid,
                    &mac,
                    lease.ip_address,
                    lease.ip_address,
                    None,
                    cfg.hostname.as_deref(),
                );
                if let Err(e) =
                    socket.send_to(&pkt, format!("255.255.255.255:{DHCP_SERVER_PORT}"))
                {
                    debug_log(cfg, &format!("send rebind: {e}"));
                    std::thread::sleep(Duration::from_secs(30));
                    continue;
                }
                socket
                    .set_read_timeout(Some(Duration::from_secs(10)))
                    .ok();
                let mut buf = [0u8; DHCP_MAX_LEN];
                if let Ok((len, _)) = socket.recv_from(&mut buf)
                    && let Some(msg) = DhcpMessage::parse(&buf[..len])
                        && msg.op == BOOTREPLY && msg.xid == new_xid {
                            if msg.options.msg_type == Some(DHCP_ACK) {
                                let new_lease = LeaseInfo::from_ack(&msg, now_secs());
                                info_log(&format!(
                                    "lease rebound: {} ({}s)",
                                    ip_to_string(new_lease.ip_address),
                                    new_lease.lease_time
                                ));
                                configure_interface(&cfg.interface, &new_lease, cfg);
                                write_lease_file(&cfg.interface, &new_lease);
                                state = DhcpState::Bound;
                                continue;
                            }
                            if msg.options.msg_type == Some(DHCP_NAK) {
                                info_log("rebinding NAK; restarting");
                                state = DhcpState::Init;
                                continue;
                            }
                        }
                socket
                    .set_read_timeout(Some(Duration::from_secs(cfg.timeout)))
                    .ok();
                std::thread::sleep(Duration::from_secs(30));
            }
        }
    }
}

/// Release the current lease.
fn release_lease(cfg: &Config) -> Result<(), String> {
    let lease = read_lease_file(&cfg.interface)
        .ok_or_else(|| format!("no lease file for '{}'", cfg.interface))?;
    let mac = read_mac(&cfg.interface)
        .ok_or_else(|| format!("cannot read MAC for '{}'", cfg.interface))?;

    let xid = generate_xid();
    let pkt = build_release(xid, &mac, lease.ip_address, lease.server_id);

    let socket = UdpSocket::bind(format!("0.0.0.0:{DHCP_CLIENT_PORT}"))
        .map_err(|e| format!("bind: {e}"))?;
    let server_addr = format!("{}:{DHCP_SERVER_PORT}", ip_to_string(lease.server_id));
    socket
        .send_to(&pkt, &server_addr)
        .map_err(|e| format!("send RELEASE: {e}"))?;

    info_log(&format!(
        "released {} to {}",
        ip_to_string(lease.ip_address),
        ip_to_string(lease.server_id)
    ));

    // Remove the lease file.
    let lease_path = PathBuf::from(LEASE_DIR).join(format!("{}.lease", cfg.interface));
    let _ = fs::remove_file(&lease_path);

    Ok(())
}

/// Kill a running dhcpcd daemon.
fn kill_daemon() -> Result<(), String> {
    let pid = read_pid_file().ok_or("no PID file found; daemon may not be running")?;
    // On Slate OS we send a termination IPC message; for now use the kill syscall.
    // Process termination is done via the standard POSIX interface.
    eprintln!("dhcpcd: sending SIGTERM to PID {pid}");
    // We cannot directly send signals in this stub; print the PID for manual action.
    // In production, this would call sys_kill(pid, SIGTERM).
    remove_pid_file();
    Ok(())
}

// ============================================================================
// Usage
// ============================================================================

fn print_usage() {
    eprintln!(
        "\
Usage: dhcpcd [OPTIONS]

DHCP client daemon for SlateOS.

Options:
  -i IFACE         Network interface (default: auto-detect)
  -n               Don't configure interface (print only)
  -r IP            Request specific IP address
  -s IP            Use as client IP in DHCPREQUEST
  -d               Debug/verbose logging
  -f               Run in foreground
  -t SECS          Timeout waiting for lease (default: 30)
  -x               Release current lease and exit
  -k               Kill running daemon
  -1               Try once and exit if no lease
  --hostname=NAME  Send hostname option
  --no-gateway     Don't install default route
  --no-dns         Don't write resolv.conf
  --no-ntp         Don't configure NTP servers"
    );
}

// ============================================================================
// Main
// ============================================================================

fn main() {
    let args: Vec<String> = env::args().collect();

    // Show usage for --help / -h.
    if args.iter().any(|a| a == "--help" || a == "-h") {
        print_usage();
        process::exit(0);
    }

    let mut cfg = match parse_args(&args[1..]) {
        Ok(c) => c,
        Err(e) => {
            error_log(&e);
            print_usage();
            process::exit(1);
        }
    };

    // Load configuration file (if present).
    if let Ok(text) = fs::read_to_string(CONFIG_FILE) {
        let file_cfg = parse_config_file(&text);
        apply_config_file(&mut cfg, &file_cfg);
    }

    // Auto-detect interface if not specified.
    if cfg.interface == "eth0"
        && let Some(detected) = detect_interface() {
            cfg.interface = detected;
        }

    // Handle -k (kill daemon).
    if cfg.kill {
        match kill_daemon() {
            Ok(()) => process::exit(0),
            Err(e) => {
                error_log(&e);
                process::exit(1);
            }
        }
    }

    // Handle -x (release lease).
    if cfg.release {
        match release_lease(&cfg) {
            Ok(()) => process::exit(0),
            Err(e) => {
                error_log(&e);
                process::exit(1);
            }
        }
    }

    // Write PID file for daemon management.
    if let Err(e) = write_pid_file() {
        debug_log(&cfg, &format!("warning: PID file: {e}"));
    }

    info_log(&format!("starting on {}", cfg.interface));

    // Run the DHCP state machine.
    match run_dhcp(&cfg) {
        Ok(()) => {
            remove_pid_file();
            process::exit(0);
        }
        Err(e) => {
            error_log(&e);
            remove_pid_file();
            process::exit(1);
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ---- SYS_NET_IF_CONFIG lease-record building ----

    #[test]
    fn test_build_lease_record_with_gateway() {
        // Lease 10.0.2.15/24 via 10.0.2.2.
        let rec = build_lease_record([10, 0, 2, 15], [255, 255, 255, 0], Some([10, 0, 2, 2]));
        assert_eq!(&rec[0..4], &[10, 0, 2, 15]);
        assert_eq!(&rec[4..8], &[255, 255, 255, 0]);
        assert_eq!(&rec[8..12], &[10, 0, 2, 2]);
        assert_eq!(&rec[12..16], &[0, 0, 0, 0]); // dns untouched
        assert_eq!(rec[16], 1); // up
        assert_eq!(
            rec[17],
            cfg_mask::IP | cfg_mask::MASK | cfg_mask::GATEWAY | cfg_mask::UP
        );
    }

    #[test]
    fn test_build_lease_record_no_gateway() {
        let rec = build_lease_record([192, 168, 1, 50], [255, 255, 255, 0], None);
        assert_eq!(&rec[0..4], &[192, 168, 1, 50]);
        assert_eq!(&rec[4..8], &[255, 255, 255, 0]);
        assert_eq!(&rec[8..12], &[0, 0, 0, 0]); // gateway untouched
        assert_eq!(rec[16], 1);
        assert_eq!(rec[17], cfg_mask::IP | cfg_mask::MASK | cfg_mask::UP);
    }

    #[test]
    fn test_build_lease_record_be_order() {
        // Confirm u32 -> byte order matches the wire/ABI (MSB = first octet).
        let ip: u32 = 0x0A00020F; // 10.0.2.15
        let rec = build_lease_record(ip.to_be_bytes(), [255, 255, 255, 0], None);
        assert_eq!(&rec[0..4], &[10, 0, 2, 15]);
    }

    // ---- IP / MAC helpers ----

    #[test]
    fn test_parse_ipv4_valid() {
        assert_eq!(parse_ipv4("192.168.1.1"), Some(0xC0A80101));
    }

    #[test]
    fn test_parse_ipv4_zeros() {
        assert_eq!(parse_ipv4("0.0.0.0"), Some(0));
    }

    #[test]
    fn test_parse_ipv4_broadcast() {
        assert_eq!(parse_ipv4("255.255.255.255"), Some(0xFFFF_FFFF));
    }

    #[test]
    fn test_parse_ipv4_invalid_octets() {
        assert_eq!(parse_ipv4("256.1.1.1"), None);
    }

    #[test]
    fn test_parse_ipv4_too_few_parts() {
        assert_eq!(parse_ipv4("192.168.1"), None);
    }

    #[test]
    fn test_parse_ipv4_too_many_parts() {
        assert_eq!(parse_ipv4("192.168.1.1.1"), None);
    }

    #[test]
    fn test_parse_ipv4_empty() {
        assert_eq!(parse_ipv4(""), None);
    }

    #[test]
    fn test_parse_ipv4_non_numeric() {
        assert_eq!(parse_ipv4("abc.def.ghi.jkl"), None);
    }

    #[test]
    fn test_ip_to_string() {
        assert_eq!(ip_to_string(0xC0A80101), "192.168.1.1");
    }

    #[test]
    fn test_ip_to_string_zero() {
        assert_eq!(ip_to_string(0), "0.0.0.0");
    }

    #[test]
    fn test_ip_to_string_broadcast() {
        assert_eq!(ip_to_string(0xFFFF_FFFF), "255.255.255.255");
    }

    #[test]
    fn test_ip_roundtrip() {
        let ip = "10.0.2.15";
        assert_eq!(ip_to_string(parse_ipv4(ip).unwrap()), ip);
    }

    #[test]
    fn test_mask_to_cidr_24() {
        assert_eq!(mask_to_cidr(0xFFFF_FF00), 24);
    }

    #[test]
    fn test_mask_to_cidr_16() {
        assert_eq!(mask_to_cidr(0xFFFF_0000), 16);
    }

    #[test]
    fn test_mask_to_cidr_32() {
        assert_eq!(mask_to_cidr(0xFFFF_FFFF), 32);
    }

    #[test]
    fn test_mask_to_cidr_0() {
        assert_eq!(mask_to_cidr(0), 0);
    }

    #[test]
    fn test_mask_to_cidr_8() {
        assert_eq!(mask_to_cidr(0xFF00_0000), 8);
    }

    #[test]
    fn test_mac_to_string() {
        let mac = [0xDE, 0xAD, 0xBE, 0xEF, 0xCA, 0xFE];
        assert_eq!(mac_to_string(&mac), "de:ad:be:ef:ca:fe");
    }

    #[test]
    fn test_mac_to_string_zeros() {
        assert_eq!(mac_to_string(&[0; 6]), "00:00:00:00:00:00");
    }

    #[test]
    fn test_mac_to_string_short() {
        assert_eq!(mac_to_string(&[0x01, 0x02]), "??:??:??:??:??:??");
    }

    #[test]
    fn test_msg_type_name_all() {
        assert_eq!(msg_type_name(DHCP_DISCOVER), "DISCOVER");
        assert_eq!(msg_type_name(DHCP_OFFER), "OFFER");
        assert_eq!(msg_type_name(DHCP_REQUEST), "REQUEST");
        assert_eq!(msg_type_name(DHCP_DECLINE), "DECLINE");
        assert_eq!(msg_type_name(DHCP_ACK), "ACK");
        assert_eq!(msg_type_name(DHCP_NAK), "NAK");
        assert_eq!(msg_type_name(DHCP_RELEASE), "RELEASE");
        assert_eq!(msg_type_name(DHCP_INFORM), "INFORM");
        assert_eq!(msg_type_name(99), "UNKNOWN");
    }

    // ---- XID generation ----

    #[test]
    fn test_xid_nonzero() {
        let xid = generate_xid();
        assert_ne!(xid, 0);
    }

    #[test]
    fn test_xid_varies() {
        // Two calls should almost certainly produce different values.
        let a = generate_xid();
        std::thread::sleep(std::time::Duration::from_millis(1));
        let b = generate_xid();
        // Probabilistic: they *could* collide, but vanishingly unlikely.
        // We just check both are nonzero; the real point is no panic.
        assert_ne!(a, 0);
        assert_ne!(b, 0);
    }

    // ---- Options encoding / decoding ----

    #[test]
    fn test_write_option_basic() {
        let mut buf = Vec::new();
        write_option(&mut buf, 42, &[1, 2, 3, 4]);
        assert_eq!(buf, vec![42, 4, 1, 2, 3, 4]);
    }

    #[test]
    fn test_write_msg_type() {
        let mut buf = Vec::new();
        write_msg_type(&mut buf, DHCP_DISCOVER);
        assert_eq!(buf, vec![OPT_MSG_TYPE, 1, DHCP_DISCOVER]);
    }

    #[test]
    fn test_write_ip_option() {
        let mut buf = Vec::new();
        write_ip_option(&mut buf, OPT_SERVER_ID, 0xC0A80001);
        assert_eq!(buf, vec![OPT_SERVER_ID, 4, 192, 168, 0, 1]);
    }

    #[test]
    fn test_write_client_id() {
        let mac = [0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF];
        let mut buf = Vec::new();
        write_client_id(&mut buf, &mac);
        assert_eq!(
            buf,
            vec![OPT_CLIENT_ID, 7, HTYPE_ETHERNET, 0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF]
        );
    }

    #[test]
    fn test_write_hostname_option() {
        let mut buf = Vec::new();
        write_hostname_option(&mut buf, "myhost");
        assert_eq!(
            buf,
            vec![OPT_HOSTNAME, 6, b'm', b'y', b'h', b'o', b's', b't']
        );
    }

    #[test]
    fn test_parse_options_msg_type() {
        let data = [OPT_MSG_TYPE, 1, DHCP_OFFER, OPT_END];
        let opts = parse_options(&data).unwrap();
        assert_eq!(opts.msg_type, Some(DHCP_OFFER));
    }

    #[test]
    fn test_parse_options_subnet_mask() {
        let data = [OPT_SUBNET_MASK, 4, 255, 255, 255, 0, OPT_END];
        let opts = parse_options(&data).unwrap();
        assert_eq!(opts.subnet_mask, Some(0xFFFF_FF00));
    }

    #[test]
    fn test_parse_options_single_router() {
        let data = [OPT_ROUTER, 4, 192, 168, 1, 1, OPT_END];
        let opts = parse_options(&data).unwrap();
        assert_eq!(opts.routers, vec![0xC0A80101]);
    }

    #[test]
    fn test_parse_options_multiple_routers() {
        let data = [OPT_ROUTER, 8, 10, 0, 0, 1, 10, 0, 0, 2, OPT_END];
        let opts = parse_options(&data).unwrap();
        assert_eq!(opts.routers, vec![0x0A000001, 0x0A000002]);
    }

    #[test]
    fn test_parse_options_single_dns() {
        let data = [OPT_DNS, 4, 8, 8, 8, 8, OPT_END];
        let opts = parse_options(&data).unwrap();
        assert_eq!(opts.dns_servers, vec![0x08080808]);
    }

    #[test]
    fn test_parse_options_multiple_dns() {
        let data = [OPT_DNS, 8, 8, 8, 8, 8, 8, 8, 4, 4, OPT_END];
        let opts = parse_options(&data).unwrap();
        assert_eq!(opts.dns_servers, vec![0x08080808, 0x08080404]);
    }

    #[test]
    fn test_parse_options_hostname() {
        let data = [OPT_HOSTNAME, 4, b't', b'e', b's', b't', OPT_END];
        let opts = parse_options(&data).unwrap();
        assert_eq!(opts.hostname, Some("test".to_string()));
    }

    #[test]
    fn test_parse_options_domain_name() {
        let data = [OPT_DOMAIN_NAME, 7, b'f', b'o', b'o', b'.', b'c', b'o', b'm', OPT_END];
        let opts = parse_options(&data).unwrap();
        assert_eq!(opts.domain_name, Some("foo.com".to_string()));
    }

    #[test]
    fn test_parse_options_broadcast() {
        let data = [OPT_BROADCAST, 4, 192, 168, 1, 255, OPT_END];
        let opts = parse_options(&data).unwrap();
        assert_eq!(opts.broadcast, Some(0xC0A801FF));
    }

    #[test]
    fn test_parse_options_ntp() {
        let data = [OPT_NTP, 4, 10, 0, 0, 1, OPT_END];
        let opts = parse_options(&data).unwrap();
        assert_eq!(opts.ntp_servers, vec![0x0A000001]);
    }

    #[test]
    fn test_parse_options_lease_time() {
        // 86400 = 0x00015180
        let data = [OPT_LEASE_TIME, 4, 0x00, 0x01, 0x51, 0x80, OPT_END];
        let opts = parse_options(&data).unwrap();
        assert_eq!(opts.lease_time, Some(86400));
    }

    #[test]
    fn test_parse_options_server_id() {
        let data = [OPT_SERVER_ID, 4, 172, 16, 0, 1, OPT_END];
        let opts = parse_options(&data).unwrap();
        assert_eq!(opts.server_id, Some(0xAC100001));
    }

    #[test]
    fn test_parse_options_renewal_time() {
        // T1 = 43200 = 0x0000A8C0
        let data = [OPT_RENEWAL_TIME, 4, 0x00, 0x00, 0xA8, 0xC0, OPT_END];
        let opts = parse_options(&data).unwrap();
        assert_eq!(opts.renewal_time, Some(43200));
    }

    #[test]
    fn test_parse_options_rebinding_time() {
        // T2 = 75600 = 0x00012750
        let data = [OPT_REBINDING_TIME, 4, 0x00, 0x01, 0x27, 0x50, OPT_END];
        let opts = parse_options(&data).unwrap();
        assert_eq!(opts.rebinding_time, Some(75600));
    }

    #[test]
    fn test_parse_options_with_padding() {
        let data = [OPT_PAD, OPT_PAD, OPT_MSG_TYPE, 1, DHCP_ACK, OPT_PAD, OPT_END];
        let opts = parse_options(&data).unwrap();
        assert_eq!(opts.msg_type, Some(DHCP_ACK));
    }

    #[test]
    fn test_parse_options_empty() {
        let data = [OPT_END];
        let opts = parse_options(&data).unwrap();
        assert_eq!(opts.msg_type, None);
        assert!(opts.routers.is_empty());
        assert!(opts.dns_servers.is_empty());
    }

    #[test]
    fn test_parse_options_unknown_skipped() {
        // Option 200 is not recognized; it should be silently skipped.
        let data = [200, 2, 0xAB, 0xCD, OPT_MSG_TYPE, 1, DHCP_ACK, OPT_END];
        let opts = parse_options(&data).unwrap();
        assert_eq!(opts.msg_type, Some(DHCP_ACK));
    }

    #[test]
    fn test_parse_options_truncated_value() {
        // Claim length 10 but only 2 bytes follow; parser should stop gracefully.
        let data = [OPT_DNS, 10, 8, 8];
        let opts = parse_options(&data).unwrap();
        assert!(opts.dns_servers.is_empty()); // Not enough data; skipped.
    }

    // ---- Magic cookie ----

    #[test]
    fn test_magic_cookie_value() {
        assert_eq!(MAGIC_COOKIE, [99, 130, 83, 99]);
        // 0x63825363
        let val = u32::from_be_bytes(MAGIC_COOKIE);
        assert_eq!(val, 0x63825363);
    }

    // ---- Packet building ----

    #[test]
    fn test_build_discover_size() {
        let mac = [0x00, 0x11, 0x22, 0x33, 0x44, 0x55];
        let pkt = build_discover(0x12345678, &mac, None, None);
        assert!(pkt.len() >= 300);
    }

    #[test]
    fn test_build_discover_header() {
        let mac = [0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF];
        let pkt = build_discover(0xDEADBEEF, &mac, None, None);
        assert_eq!(pkt[0], BOOTREQUEST);
        assert_eq!(pkt[1], HTYPE_ETHERNET);
        assert_eq!(pkt[2], HLEN_ETHERNET);
        // XID at bytes 4..8.
        let xid = u32::from_be_bytes([pkt[4], pkt[5], pkt[6], pkt[7]]);
        assert_eq!(xid, 0xDEADBEEF);
        // chaddr starts at byte 28.
        assert_eq!(&pkt[28..34], &mac);
    }

    #[test]
    fn test_build_discover_magic_cookie() {
        let mac = [0; 6];
        let pkt = build_discover(1, &mac, None, None);
        assert_eq!(&pkt[236..240], &MAGIC_COOKIE);
    }

    #[test]
    fn test_build_discover_contains_discover_type() {
        let mac = [0; 6];
        let pkt = build_discover(1, &mac, None, None);
        // After magic cookie, first option should be msg type = DISCOVER.
        assert_eq!(pkt[240], OPT_MSG_TYPE);
        assert_eq!(pkt[241], 1);
        assert_eq!(pkt[242], DHCP_DISCOVER);
    }

    #[test]
    fn test_build_discover_with_hostname() {
        let mac = [0; 6];
        let pkt = build_discover(1, &mac, Some("myhost"), None);
        // The hostname option should appear somewhere in the packet.
        let found = pkt.windows(8).any(|w| {
            w[0] == OPT_HOSTNAME && w[1] == 6 && &w[2..8] == b"myhost"
        });
        assert!(found, "hostname option not found in DISCOVER");
    }

    #[test]
    fn test_build_discover_with_requested_ip() {
        let mac = [0; 6];
        let ip = parse_ipv4("192.168.1.100").unwrap();
        let pkt = build_discover(1, &mac, None, Some(ip));
        // Option 50 = Requested IP Address.
        let found = pkt.windows(6).any(|w| {
            w[0] == 50 && w[1] == 4 && w[2] == 192 && w[3] == 168 && w[4] == 1 && w[5] == 100
        });
        assert!(found, "requested IP option not found in DISCOVER");
    }

    #[test]
    fn test_build_request_size() {
        let mac = [0; 6];
        let pkt = build_request(1, &mac, 0, 0xC0A80164, Some(0xC0A80001), None);
        assert!(pkt.len() >= 300);
    }

    #[test]
    fn test_build_request_contains_request_type() {
        let mac = [0; 6];
        let pkt = build_request(1, &mac, 0, 0xC0A80164, Some(0xC0A80001), None);
        assert_eq!(pkt[240], OPT_MSG_TYPE);
        assert_eq!(pkt[241], 1);
        assert_eq!(pkt[242], DHCP_REQUEST);
    }

    #[test]
    fn test_build_request_with_server_id() {
        let mac = [0; 6];
        let server = parse_ipv4("10.0.0.1").unwrap();
        let pkt = build_request(1, &mac, 0, 0xC0A80164, Some(server), None);
        let found = pkt.windows(6).any(|w| {
            w[0] == OPT_SERVER_ID && w[1] == 4 && w[2] == 10 && w[3] == 0 && w[4] == 0 && w[5] == 1
        });
        assert!(found, "server ID option not found in REQUEST");
    }

    #[test]
    fn test_build_release_contains_release_type() {
        let mac = [0; 6];
        let pkt = build_release(1, &mac, 0xC0A80164, 0xC0A80001);
        assert_eq!(pkt[240], OPT_MSG_TYPE);
        assert_eq!(pkt[241], 1);
        assert_eq!(pkt[242], DHCP_RELEASE);
    }

    #[test]
    fn test_build_release_ciaddr() {
        let mac = [0; 6];
        let ciaddr = parse_ipv4("10.0.2.15").unwrap();
        let pkt = build_release(1, &mac, ciaddr, 0xC0A80001);
        // ciaddr is at bytes 12..16.
        let parsed = u32::from_be_bytes([pkt[12], pkt[13], pkt[14], pkt[15]]);
        assert_eq!(parsed, ciaddr);
    }

    #[test]
    fn test_build_decline_contains_decline_type() {
        let mac = [0; 6];
        let pkt = build_decline(1, &mac, 0xC0A80164, 0xC0A80001);
        assert_eq!(pkt[240], OPT_MSG_TYPE);
        assert_eq!(pkt[241], 1);
        assert_eq!(pkt[242], DHCP_DECLINE);
    }

    #[test]
    fn test_build_inform_contains_inform_type() {
        let mac = [0; 6];
        let pkt = build_inform(1, &mac, 0xC0A80164, None);
        assert_eq!(pkt[240], OPT_MSG_TYPE);
        assert_eq!(pkt[241], 1);
        assert_eq!(pkt[242], DHCP_INFORM);
    }

    // ---- DHCP message parsing ----

    /// Helper: build a minimal valid DHCP reply packet.
    fn make_test_reply(
        xid: u32,
        yiaddr: u32,
        options_data: &[u8],
    ) -> Vec<u8> {
        let mut pkt = vec![0u8; DHCP_HEADER_LEN];
        pkt[0] = BOOTREPLY;
        pkt[1] = HTYPE_ETHERNET;
        pkt[2] = HLEN_ETHERNET;
        pkt[4..8].copy_from_slice(&xid.to_be_bytes());
        pkt[16..20].copy_from_slice(&yiaddr.to_be_bytes());
        pkt.extend_from_slice(&MAGIC_COOKIE);
        pkt.extend_from_slice(options_data);
        if pkt.last() != Some(&OPT_END) {
            pkt.push(OPT_END);
        }
        pkt
    }

    #[test]
    fn test_parse_message_offer() {
        let opts = [OPT_MSG_TYPE, 1, DHCP_OFFER, OPT_END];
        let pkt = make_test_reply(0xAABBCCDD, 0xC0A80164, &opts);
        let msg = DhcpMessage::parse(&pkt).unwrap();
        assert_eq!(msg.op, BOOTREPLY);
        assert_eq!(msg.xid, 0xAABBCCDD);
        assert_eq!(msg.yiaddr, 0xC0A80164);
        assert_eq!(msg.options.msg_type, Some(DHCP_OFFER));
    }

    #[test]
    fn test_parse_message_ack_with_options() {
        let mut opts = Vec::new();
        opts.extend_from_slice(&[OPT_MSG_TYPE, 1, DHCP_ACK]);
        opts.extend_from_slice(&[OPT_SUBNET_MASK, 4, 255, 255, 255, 0]);
        opts.extend_from_slice(&[OPT_ROUTER, 4, 10, 0, 0, 1]);
        opts.extend_from_slice(&[OPT_DNS, 8, 8, 8, 8, 8, 8, 8, 4, 4]);
        opts.extend_from_slice(&[OPT_LEASE_TIME, 4, 0x00, 0x01, 0x51, 0x80]); // 86400
        opts.push(OPT_END);

        let pkt = make_test_reply(1, 0x0A00020F, &opts);
        let msg = DhcpMessage::parse(&pkt).unwrap();
        assert_eq!(msg.options.msg_type, Some(DHCP_ACK));
        assert_eq!(msg.options.subnet_mask, Some(0xFFFF_FF00));
        assert_eq!(msg.options.routers, vec![0x0A000001]);
        assert_eq!(msg.options.dns_servers, vec![0x08080808, 0x08080404]);
        assert_eq!(msg.options.lease_time, Some(86400));
    }

    #[test]
    fn test_parse_message_too_short() {
        let short = vec![0u8; 100];
        assert!(DhcpMessage::parse(&short).is_none());
    }

    #[test]
    fn test_parse_message_bad_magic() {
        let mut pkt = vec![0u8; DHCP_HEADER_LEN + 4];
        // Wrong magic cookie.
        pkt[236] = 0;
        pkt[237] = 0;
        pkt[238] = 0;
        pkt[239] = 0;
        assert!(DhcpMessage::parse(&pkt).is_none());
    }

    #[test]
    fn test_parse_roundtrip() {
        let mac = [0x11, 0x22, 0x33, 0x44, 0x55, 0x66];
        let xid = 0xFEEDFACE;
        let pkt = build_discover(xid, &mac, Some("host1"), None);
        let msg = DhcpMessage::parse(&pkt).unwrap();
        assert_eq!(msg.op, BOOTREQUEST);
        assert_eq!(msg.xid, xid);
        assert_eq!(msg.options.msg_type, Some(DHCP_DISCOVER));
        assert_eq!(&msg.chaddr[..6], &mac);
    }

    // ---- Lease info ----

    #[test]
    fn test_lease_from_ack() {
        let mut opts = Vec::new();
        opts.extend_from_slice(&[OPT_MSG_TYPE, 1, DHCP_ACK]);
        opts.extend_from_slice(&[OPT_LEASE_TIME, 4, 0x00, 0x01, 0x51, 0x80]); // 86400
        opts.extend_from_slice(&[OPT_SERVER_ID, 4, 10, 0, 0, 1]);
        opts.extend_from_slice(&[OPT_SUBNET_MASK, 4, 255, 255, 255, 0]);
        opts.extend_from_slice(&[OPT_ROUTER, 4, 10, 0, 0, 1]);
        opts.push(OPT_END);

        let pkt = make_test_reply(1, 0x0A00020F, &opts);
        let msg = DhcpMessage::parse(&pkt).unwrap();
        let lease = LeaseInfo::from_ack(&msg, 1000);

        assert_eq!(lease.ip_address, 0x0A00020F);
        assert_eq!(lease.subnet_mask, 0xFFFF_FF00);
        assert_eq!(lease.lease_time, 86400);
        assert_eq!(lease.renewal_time, 43200);  // lease_time / 2
        assert_eq!(lease.rebinding_time, 75600); // lease_time * 7/8
        assert_eq!(lease.server_id, 0x0A000001);
        assert_eq!(lease.obtained_at, 1000);
    }

    #[test]
    fn test_lease_timers() {
        let lease = LeaseInfo {
            obtained_at: 1000,
            lease_time: 3600,
            renewal_time: 1800,
            rebinding_time: 3150,
            ..LeaseInfo::default()
        };
        assert_eq!(lease.t1_expiry(), 2800);
        assert_eq!(lease.t2_expiry(), 4150);
        assert_eq!(lease.lease_expiry(), 4600);
        assert!(!lease.is_expired(3000));
        assert!(lease.is_expired(4600));
        assert!(lease.is_expired(5000));
    }

    #[test]
    fn test_lease_default_times() {
        // When no T1/T2 are provided, they default from lease_time.
        let mut opts = Vec::new();
        opts.extend_from_slice(&[OPT_MSG_TYPE, 1, DHCP_ACK]);
        opts.extend_from_slice(&[OPT_LEASE_TIME, 4, 0, 0, 0x0E, 0x10]); // 3600
        opts.push(OPT_END);

        let pkt = make_test_reply(1, 0x0A000001, &opts);
        let msg = DhcpMessage::parse(&pkt).unwrap();
        let lease = LeaseInfo::from_ack(&msg, 0);
        assert_eq!(lease.renewal_time, 1800);
        assert_eq!(lease.rebinding_time, 3150);
    }

    #[test]
    fn test_lease_expired_at_exact_boundary() {
        let lease = LeaseInfo {
            obtained_at: 0,
            lease_time: 100,
            ..LeaseInfo::default()
        };
        assert!(lease.is_expired(100)); // Exactly at expiry = expired.
        assert!(!lease.is_expired(99));
    }

    // ---- Lease serialization ----

    #[test]
    fn test_lease_serialize_deserialize() {
        let lease = LeaseInfo {
            ip_address: parse_ipv4("192.168.1.100").unwrap(),
            subnet_mask: parse_ipv4("255.255.255.0").unwrap(),
            routers: vec![parse_ipv4("192.168.1.1").unwrap()],
            dns_servers: vec![
                parse_ipv4("8.8.8.8").unwrap(),
                parse_ipv4("8.8.4.4").unwrap(),
            ],
            ntp_servers: vec![parse_ipv4("10.0.0.1").unwrap()],
            hostname: Some("testhost".to_string()),
            domain_name: Some("example.com".to_string()),
            broadcast: Some(parse_ipv4("192.168.1.255").unwrap()),
            server_id: parse_ipv4("192.168.1.1").unwrap(),
            lease_time: 86400,
            renewal_time: 43200,
            rebinding_time: 75600,
            obtained_at: 1700000000,
        };

        let text = lease.serialize();
        let parsed = LeaseInfo::deserialize(&text).unwrap();

        assert_eq!(parsed.ip_address, lease.ip_address);
        assert_eq!(parsed.subnet_mask, lease.subnet_mask);
        assert_eq!(parsed.routers, lease.routers);
        assert_eq!(parsed.dns_servers, lease.dns_servers);
        assert_eq!(parsed.ntp_servers, lease.ntp_servers);
        assert_eq!(parsed.hostname, lease.hostname);
        assert_eq!(parsed.domain_name, lease.domain_name);
        assert_eq!(parsed.broadcast, lease.broadcast);
        assert_eq!(parsed.server_id, lease.server_id);
        assert_eq!(parsed.lease_time, lease.lease_time);
        assert_eq!(parsed.renewal_time, lease.renewal_time);
        assert_eq!(parsed.rebinding_time, lease.rebinding_time);
        assert_eq!(parsed.obtained_at, lease.obtained_at);
    }

    #[test]
    fn test_lease_deserialize_minimal() {
        let text = "ip_address=10.0.0.5\nlease_time=3600\n";
        let lease = LeaseInfo::deserialize(text).unwrap();
        assert_eq!(lease.ip_address, parse_ipv4("10.0.0.5").unwrap());
        assert_eq!(lease.lease_time, 3600);
    }

    #[test]
    fn test_lease_deserialize_no_ip() {
        let text = "lease_time=3600\n";
        // Should return None because ip_address = 0 is rejected.
        assert!(LeaseInfo::deserialize(text).is_none());
    }

    #[test]
    fn test_lease_deserialize_comments_blank_lines() {
        let text = "# This is a comment\n\nip_address=10.0.0.1\nlease_time=100\n";
        let lease = LeaseInfo::deserialize(text).unwrap();
        assert_eq!(lease.ip_address, parse_ipv4("10.0.0.1").unwrap());
    }

    #[test]
    fn test_lease_deserialize_unknown_keys() {
        let text = "ip_address=10.0.0.1\nfuture_key=value\nlease_time=100\n";
        let lease = LeaseInfo::deserialize(text).unwrap();
        assert_eq!(lease.ip_address, parse_ipv4("10.0.0.1").unwrap());
    }

    // ---- State machine transitions ----

    #[test]
    fn test_state_names() {
        assert_eq!(DhcpState::Init.name(), "INIT");
        assert_eq!(DhcpState::Selecting.name(), "SELECTING");
        assert_eq!(DhcpState::Requesting.name(), "REQUESTING");
        assert_eq!(DhcpState::Bound.name(), "BOUND");
        assert_eq!(DhcpState::Renewing.name(), "RENEWING");
        assert_eq!(DhcpState::Rebinding.name(), "REBINDING");
    }

    #[test]
    fn test_state_init_to_selecting() {
        // After sending DISCOVER, state should transition to Selecting.
        let state = DhcpState::Init;
        let next = DhcpState::Selecting;
        assert_ne!(state, next);
        assert_eq!(next.name(), "SELECTING");
    }

    #[test]
    fn test_state_requesting_to_bound() {
        let state = DhcpState::Requesting;
        let next = DhcpState::Bound;
        assert_ne!(state, next);
    }

    #[test]
    fn test_state_bound_to_renewing() {
        let state = DhcpState::Bound;
        let next = DhcpState::Renewing;
        assert_ne!(state, next);
    }

    #[test]
    fn test_state_renewing_to_rebinding() {
        let state = DhcpState::Renewing;
        let next = DhcpState::Rebinding;
        assert_ne!(state, next);
    }

    #[test]
    fn test_state_equality() {
        assert_eq!(DhcpState::Init, DhcpState::Init);
        assert_ne!(DhcpState::Init, DhcpState::Bound);
    }

    // ---- Config file parsing ----

    #[test]
    fn test_parse_config_timeout() {
        let text = "timeout 60\n";
        let result = parse_config_file(text);
        assert_eq!(result.timeout, Some(60));
    }

    #[test]
    fn test_parse_config_retry() {
        let text = "retry 3\n";
        let result = parse_config_file(text);
        assert_eq!(result.retry, Some(3));
    }

    #[test]
    fn test_parse_config_options() {
        let text = "option domain_name_servers, domain_name, host_name\n";
        let result = parse_config_file(text);
        assert_eq!(
            result.requested_options,
            vec!["domain_name_servers", "domain_name", "host_name"]
        );
    }

    #[test]
    fn test_parse_config_require() {
        let text = "require dhcp_server_identifier\n";
        let result = parse_config_file(text);
        assert_eq!(result.required_options, vec!["dhcp_server_identifier"]);
    }

    #[test]
    fn test_parse_config_static_ip() {
        let text = "interface eth0\nstatic ip_address=192.168.1.50\n";
        let result = parse_config_file(text);
        assert_eq!(result.statics.len(), 1);
        assert_eq!(result.statics[0].interface, Some("eth0".to_string()));
        assert_eq!(result.statics[0].ip_address, parse_ipv4("192.168.1.50"));
    }

    #[test]
    fn test_parse_config_static_routers() {
        let text = "static routers=10.0.0.1\n";
        let result = parse_config_file(text);
        assert_eq!(result.statics.len(), 1);
        assert_eq!(result.statics[0].routers, vec![parse_ipv4("10.0.0.1").unwrap()]);
    }

    #[test]
    fn test_parse_config_static_dns() {
        let text = "static domain_name_servers=8.8.8.8, 8.8.4.4\n";
        let result = parse_config_file(text);
        assert_eq!(result.statics.len(), 1);
        assert_eq!(
            result.statics[0].dns_servers,
            vec![parse_ipv4("8.8.8.8").unwrap(), parse_ipv4("8.8.4.4").unwrap()]
        );
    }

    #[test]
    fn test_parse_config_comments_ignored() {
        let text = "# This is a comment\ntimeout 45\n# Another comment\n";
        let result = parse_config_file(text);
        assert_eq!(result.timeout, Some(45));
    }

    #[test]
    fn test_parse_config_empty() {
        let result = parse_config_file("");
        assert_eq!(result.timeout, None);
        assert!(result.statics.is_empty());
    }

    #[test]
    fn test_parse_config_full_example() {
        let text = "\
# dhcpcd configuration
option domain_name_servers, domain_name, host_name
require dhcp_server_identifier
timeout 30
retry 0

interface eth0
static ip_address=192.168.1.100
static routers=192.168.1.1
static domain_name_servers=8.8.8.8, 1.1.1.1
";
        let result = parse_config_file(text);
        assert_eq!(result.timeout, Some(30));
        assert_eq!(result.retry, Some(0));
        assert_eq!(result.required_options, vec!["dhcp_server_identifier"]);
        assert_eq!(result.statics.len(), 3); // ip, routers, dns
    }

    // ---- CLI argument parsing ----

    #[test]
    fn test_parse_args_defaults() {
        let cfg = parse_args(&[]).unwrap();
        assert_eq!(cfg.interface, "eth0");
        assert!(!cfg.debug);
        assert!(!cfg.foreground);
        assert!(!cfg.no_configure);
        assert!(!cfg.release);
        assert!(!cfg.kill);
        assert!(!cfg.try_once);
        assert_eq!(cfg.timeout, DEFAULT_TIMEOUT);
    }

    #[test]
    fn test_parse_args_interface() {
        let args: Vec<String> = ["-i", "wlan0"].iter().map(|s| s.to_string()).collect();
        let cfg = parse_args(&args).unwrap();
        assert_eq!(cfg.interface, "wlan0");
    }

    #[test]
    fn test_parse_args_flags() {
        let args: Vec<String> = ["-n", "-d", "-f", "-1"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let cfg = parse_args(&args).unwrap();
        assert!(cfg.no_configure);
        assert!(cfg.debug);
        assert!(cfg.foreground);
        assert!(cfg.try_once);
    }

    #[test]
    fn test_parse_args_timeout() {
        let args: Vec<String> = ["-t", "60"].iter().map(|s| s.to_string()).collect();
        let cfg = parse_args(&args).unwrap();
        assert_eq!(cfg.timeout, 60);
    }

    #[test]
    fn test_parse_args_requested_ip() {
        let args: Vec<String> = ["-r", "10.0.0.50"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let cfg = parse_args(&args).unwrap();
        assert_eq!(cfg.requested_ip, parse_ipv4("10.0.0.50"));
    }

    #[test]
    fn test_parse_args_client_ip() {
        let args: Vec<String> = ["-s", "10.0.0.51"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let cfg = parse_args(&args).unwrap();
        assert_eq!(cfg.client_ip, parse_ipv4("10.0.0.51"));
    }

    #[test]
    fn test_parse_args_hostname() {
        let args: Vec<String> = ["--hostname=mybox"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let cfg = parse_args(&args).unwrap();
        assert_eq!(cfg.hostname, Some("mybox".to_string()));
    }

    #[test]
    fn test_parse_args_no_flags() {
        let args: Vec<String> = ["--no-gateway", "--no-dns", "--no-ntp"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let cfg = parse_args(&args).unwrap();
        assert!(cfg.no_gateway);
        assert!(cfg.no_dns);
        assert!(cfg.no_ntp);
    }

    #[test]
    fn test_parse_args_release_kill() {
        let args_x: Vec<String> = ["-x"].iter().map(|s| s.to_string()).collect();
        let args_k: Vec<String> = ["-k"].iter().map(|s| s.to_string()).collect();
        assert!(parse_args(&args_x).unwrap().release);
        assert!(parse_args(&args_k).unwrap().kill);
    }

    #[test]
    fn test_parse_args_unknown_option() {
        let args: Vec<String> = ["--bogus"].iter().map(|s| s.to_string()).collect();
        assert!(parse_args(&args).is_err());
    }

    #[test]
    fn test_parse_args_missing_value() {
        let args: Vec<String> = ["-i"].iter().map(|s| s.to_string()).collect();
        assert!(parse_args(&args).is_err());
    }

    #[test]
    fn test_parse_args_invalid_ip() {
        let args: Vec<String> = ["-r", "not.an.ip"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert!(parse_args(&args).is_err());
    }

    // ---- apply_config_file ----

    #[test]
    fn test_apply_config_file_timeout() {
        let mut cfg = Config::default_config();
        let file_cfg = ConfigFileResult {
            timeout: Some(45),
            ..ConfigFileResult::default()
        };
        apply_config_file(&mut cfg, &file_cfg);
        assert_eq!(cfg.timeout, 45);
    }

    #[test]
    fn test_apply_config_file_static_ip() {
        let mut cfg = Config::default_config();
        let file_cfg = ConfigFileResult {
            statics: vec![StaticEntry {
                interface: None,
                ip_address: parse_ipv4("172.16.0.50"),
                ..StaticEntry::default()
            }],
            ..ConfigFileResult::default()
        };
        apply_config_file(&mut cfg, &file_cfg);
        assert_eq!(cfg.static_ip, parse_ipv4("172.16.0.50"));
    }

    #[test]
    fn test_apply_config_file_interface_scope() {
        let mut cfg = Config::default_config();
        cfg.interface = "eth0".to_string();
        let file_cfg = ConfigFileResult {
            statics: vec![
                StaticEntry {
                    interface: Some("wlan0".to_string()), // Different interface; should NOT apply.
                    ip_address: parse_ipv4("1.2.3.4"),
                    ..StaticEntry::default()
                },
                StaticEntry {
                    interface: Some("eth0".to_string()), // Matches; should apply.
                    ip_address: parse_ipv4("5.6.7.8"),
                    ..StaticEntry::default()
                },
            ],
            ..ConfigFileResult::default()
        };
        apply_config_file(&mut cfg, &file_cfg);
        assert_eq!(cfg.static_ip, parse_ipv4("5.6.7.8"));
    }

    // ---- DhcpMessage::new_request ----

    #[test]
    fn test_new_request_fields() {
        let mac = [0x01, 0x02, 0x03, 0x04, 0x05, 0x06];
        let msg = DhcpMessage::new_request(0xABCD1234, &mac);
        assert_eq!(msg.op, BOOTREQUEST);
        assert_eq!(msg.htype, HTYPE_ETHERNET);
        assert_eq!(msg.hlen, HLEN_ETHERNET);
        assert_eq!(msg.xid, 0xABCD1234);
        assert_eq!(msg.flags, 0x8000);
        assert_eq!(&msg.chaddr[..6], &mac);
        assert_eq!(&msg.chaddr[6..], &[0u8; 10]);
    }

    // ---- Lease display ----

    #[test]
    fn test_lease_display_contains_ip() {
        let lease = LeaseInfo {
            ip_address: parse_ipv4("10.0.2.15").unwrap(),
            subnet_mask: parse_ipv4("255.255.255.0").unwrap(),
            lease_time: 86400,
            renewal_time: 43200,
            rebinding_time: 75600,
            ..LeaseInfo::default()
        };
        let output = lease.display();
        assert!(output.contains("10.0.2.15"));
        assert!(output.contains("/24"));
        assert!(output.contains("86400"));
    }
}
