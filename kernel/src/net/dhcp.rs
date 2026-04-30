//! DHCP client implementation (RFC 2131).
//!
//! Performs DHCP discovery to obtain an IPv4 address, subnet mask,
//! default gateway, and DNS server from a DHCP server on the network.
//!
//! ## DHCP transaction flow
//!
//! 1. **DISCOVER** — Client broadcasts requesting any available address.
//! 2. **OFFER** — Server responds with an available address.
//! 3. **REQUEST** — Client formally requests the offered address.
//! 4. **ACK** — Server confirms the lease.
//!
//! ## Implementation notes
//!
//! - Uses raw MAC-level broadcast (0.0.0.0 → 255.255.255.255) since
//!   we have no IP address yet when starting DHCP.
//! - Transaction ID (xid) is used to match replies to our request.
//! - Only handles the basic options we need: subnet mask, router, DNS.

use alloc::vec::Vec;

use crate::error::{KernelError, KernelResult};
use crate::virtio::net::MacAddress;

use super::ethernet::{self, BROADCAST_MAC, ETHERTYPE_IPV4};
use super::interface::{self, Ipv4Addr};

// ---------------------------------------------------------------------------
// DHCP constants
// ---------------------------------------------------------------------------

/// DHCP server port.
const DHCP_SERVER_PORT: u16 = 67;
/// DHCP client port.
const DHCP_CLIENT_PORT: u16 = 68;

/// DHCP message types.
const DHCP_DISCOVER: u8 = 1;
const DHCP_OFFER: u8 = 2;
const DHCP_REQUEST: u8 = 3;
const DHCP_ACK: u8 = 5;

/// DHCP option codes.
const OPT_SUBNET_MASK: u8 = 1;
const OPT_ROUTER: u8 = 3;
const OPT_DNS: u8 = 6;
const OPT_REQUESTED_IP: u8 = 50;
const OPT_MSG_TYPE: u8 = 53;
const OPT_SERVER_ID: u8 = 54;
const OPT_END: u8 = 255;

/// DHCP magic cookie (99.130.83.99).
const MAGIC_COOKIE: [u8; 4] = [99, 130, 83, 99];

/// Minimum DHCP message size (without options).
const DHCP_MIN_SIZE: usize = 236;

/// Fixed transaction ID (simple, single-session client).
const XID: u32 = 0x1234_5678;

// ---------------------------------------------------------------------------
// DHCP state
// ---------------------------------------------------------------------------

use spin::Mutex;

/// DHCP client state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DhcpState {
    /// No DHCP transaction in progress.
    Idle,
    /// DISCOVER sent, waiting for OFFER.
    Discovering,
    /// REQUEST sent, waiting for ACK.
    Requesting,
    /// Lease obtained.
    Bound,
}

/// Pending DHCP offer data.
struct DhcpOffer {
    /// Offered IP address.
    ip: Ipv4Addr,
    /// DHCP server identifier.
    server_ip: Ipv4Addr,
    /// Subnet mask.
    mask: Ipv4Addr,
    /// Default gateway.
    gateway: Ipv4Addr,
    /// DNS server.
    dns: Ipv4Addr,
}

static DHCP_STATE: Mutex<DhcpState> = Mutex::new(DhcpState::Idle);
static PENDING_OFFER: Mutex<Option<DhcpOffer>> = Mutex::new(None);

// ---------------------------------------------------------------------------
// DHCP packet building
// ---------------------------------------------------------------------------

/// Build a DHCP message.
///
/// Returns UDP payload bytes (DHCP message including magic cookie and
/// options).
#[allow(clippy::arithmetic_side_effects)]
fn build_dhcp_message(msg_type: u8, our_mac: &MacAddress, options: &[(u8, &[u8])]) -> Vec<u8> {
    // DHCP messages are at least 300 bytes by convention.
    let mut msg = Vec::with_capacity(300);

    // op (1=BOOTREQUEST).
    msg.push(1);
    // htype (1=Ethernet).
    msg.push(1);
    // hlen (6=MAC length).
    msg.push(6);
    // hops.
    msg.push(0);
    // xid (transaction ID).
    msg.extend_from_slice(&XID.to_be_bytes());
    // secs (seconds since start).
    msg.extend_from_slice(&0u16.to_be_bytes());
    // flags (0x8000 = broadcast flag — request broadcast reply).
    msg.extend_from_slice(&0x8000u16.to_be_bytes());
    // ciaddr (client IP — 0 if we don't have one yet).
    msg.extend_from_slice(&[0, 0, 0, 0]);
    // yiaddr (your/offered IP — server fills this).
    msg.extend_from_slice(&[0, 0, 0, 0]);
    // siaddr (server IP).
    msg.extend_from_slice(&[0, 0, 0, 0]);
    // giaddr (relay agent IP).
    msg.extend_from_slice(&[0, 0, 0, 0]);
    // chaddr (client hardware address, 16 bytes, 6 used).
    msg.extend_from_slice(&our_mac.0);
    msg.extend_from_slice(&[0u8; 10]); // Padding to 16 bytes.
    // sname (64 bytes, unused).
    msg.extend_from_slice(&[0u8; 64]);
    // file (128 bytes, unused).
    msg.extend_from_slice(&[0u8; 128]);
    // Magic cookie.
    msg.extend_from_slice(&MAGIC_COOKIE);

    // Option: DHCP Message Type.
    msg.push(OPT_MSG_TYPE);
    msg.push(1);
    msg.push(msg_type);

    // Additional options.
    for (code, data) in options {
        msg.push(*code);
        msg.push(data.len() as u8);
        msg.extend_from_slice(data);
    }

    // End option.
    msg.push(OPT_END);

    // Pad to minimum 300 bytes.
    while msg.len() < 300 {
        msg.push(0);
    }

    msg
}

/// Build a raw UDP/IP packet for DHCP (0.0.0.0:68 → 255.255.255.255:67).
///
/// DHCP uses broadcast at both IP and Ethernet layers since we have
/// no IP address yet.
#[allow(clippy::arithmetic_side_effects)]
fn build_dhcp_ip_udp(dhcp_payload: &[u8]) -> Vec<u8> {
    let udp_len = 8 + dhcp_payload.len();
    let ip_total_len = 20 + udp_len;

    let mut pkt = Vec::with_capacity(ip_total_len);

    // --- IPv4 header (20 bytes) ---
    pkt.push(0x45);        // Version + IHL.
    pkt.push(0);           // DSCP/ECN.
    pkt.extend_from_slice(&(ip_total_len as u16).to_be_bytes());
    pkt.extend_from_slice(&0u16.to_be_bytes()); // ID.
    pkt.extend_from_slice(&0u16.to_be_bytes()); // Flags + Fragment.
    pkt.push(128);         // TTL.
    pkt.push(17);          // Protocol = UDP.
    let checksum_offset = pkt.len();
    pkt.extend_from_slice(&[0, 0]); // Checksum placeholder.
    pkt.extend_from_slice(&[0, 0, 0, 0]); // Src IP: 0.0.0.0.
    pkt.extend_from_slice(&[255, 255, 255, 255]); // Dst IP: broadcast.

    // Compute IP header checksum.
    let checksum = super::ipv4::ip_checksum(&pkt[..20]);
    pkt[checksum_offset] = (checksum >> 8) as u8;
    pkt[checksum_offset + 1] = checksum as u8;

    // --- UDP header (8 bytes) ---
    pkt.extend_from_slice(&DHCP_CLIENT_PORT.to_be_bytes());  // Src port: 68.
    pkt.extend_from_slice(&DHCP_SERVER_PORT.to_be_bytes());  // Dst port: 67.
    pkt.extend_from_slice(&(udp_len as u16).to_be_bytes());  // UDP length.
    pkt.extend_from_slice(&0u16.to_be_bytes());               // Checksum (0 = disabled).

    // --- DHCP payload ---
    pkt.extend_from_slice(dhcp_payload);

    pkt
}

// ---------------------------------------------------------------------------
// DHCP client operations
// ---------------------------------------------------------------------------

/// Send a DHCP DISCOVER message.
fn send_discover() -> KernelResult<()> {
    let our_mac = interface::mac();

    let dhcp_msg = build_dhcp_message(DHCP_DISCOVER, &our_mac, &[]);
    let ip_udp_packet = build_dhcp_ip_udp(&dhcp_msg);

    // Wrap in Ethernet frame (broadcast).
    let frame = ethernet::build_frame(
        &BROADCAST_MAC,
        &our_mac,
        ETHERTYPE_IPV4,
        &ip_udp_packet,
    );

    crate::virtio::net::with_device(|dev| dev.send(&frame))
        .unwrap_or(Err(KernelError::NoSuchDevice))?;

    *DHCP_STATE.lock() = DhcpState::Discovering;
    crate::serial_println!("[dhcp] DISCOVER sent");

    Ok(())
}

/// Send a DHCP REQUEST for a specific IP.
fn send_request(requested_ip: Ipv4Addr, server_ip: Ipv4Addr) -> KernelResult<()> {
    let our_mac = interface::mac();

    let options: &[(u8, &[u8])] = &[
        (OPT_REQUESTED_IP, &requested_ip.0),
        (OPT_SERVER_ID, &server_ip.0),
    ];

    let dhcp_msg = build_dhcp_message(DHCP_REQUEST, &our_mac, options);
    let ip_udp_packet = build_dhcp_ip_udp(&dhcp_msg);

    let frame = ethernet::build_frame(
        &BROADCAST_MAC,
        &our_mac,
        ETHERTYPE_IPV4,
        &ip_udp_packet,
    );

    crate::virtio::net::with_device(|dev| dev.send(&frame))
        .unwrap_or(Err(KernelError::NoSuchDevice))?;

    *DHCP_STATE.lock() = DhcpState::Requesting;
    crate::serial_println!("[dhcp] REQUEST sent for {}", requested_ip);

    Ok(())
}

// ---------------------------------------------------------------------------
// DHCP response processing
// ---------------------------------------------------------------------------

/// Process an incoming DHCP response (called from UDP layer).
#[allow(clippy::arithmetic_side_effects)]
pub fn process_dhcp_response(data: &[u8]) -> KernelResult<()> {
    if data.len() < DHCP_MIN_SIZE + 4 {
        return Err(KernelError::InvalidArgument);
    }

    // Verify op = BOOTREPLY (2).
    if data[0] != 2 {
        return Ok(());
    }

    // Verify xid matches.
    let xid = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);
    if xid != XID {
        return Ok(());
    }

    // Offered IP address (yiaddr).
    let mut offered_ip = [0u8; 4];
    offered_ip.copy_from_slice(&data[16..20]);
    let offered_ip = Ipv4Addr(offered_ip);

    // Parse options (after magic cookie at offset 236).
    let options_start = DHCP_MIN_SIZE + 4; // 236 + 4 = 240.
    if data.len() < options_start {
        return Err(KernelError::InvalidArgument);
    }

    // Verify magic cookie.
    if data[236..240] != MAGIC_COOKIE {
        return Err(KernelError::InvalidArgument);
    }

    let mut msg_type: Option<u8> = None;
    let mut subnet_mask = Ipv4Addr::UNSPECIFIED;
    let mut router = Ipv4Addr::UNSPECIFIED;
    let mut dns = Ipv4Addr::UNSPECIFIED;
    let mut server_id = Ipv4Addr::UNSPECIFIED;

    let mut i = options_start;
    while i < data.len() {
        let opt_code = data[i];
        if opt_code == OPT_END {
            break;
        }
        if opt_code == 0 {
            // Padding.
            i += 1;
            continue;
        }

        i += 1;
        if i >= data.len() {
            break;
        }
        let opt_len = data[i] as usize;
        i += 1;

        if i + opt_len > data.len() {
            break;
        }

        let opt_data = &data[i..i + opt_len];
        match opt_code {
            OPT_MSG_TYPE if opt_len >= 1 => {
                msg_type = Some(opt_data[0]);
            }
            OPT_SUBNET_MASK if opt_len >= 4 => {
                let mut m = [0u8; 4];
                m.copy_from_slice(&opt_data[..4]);
                subnet_mask = Ipv4Addr(m);
            }
            OPT_ROUTER if opt_len >= 4 => {
                let mut r = [0u8; 4];
                r.copy_from_slice(&opt_data[..4]);
                router = Ipv4Addr(r);
            }
            OPT_DNS if opt_len >= 4 => {
                let mut d = [0u8; 4];
                d.copy_from_slice(&opt_data[..4]);
                dns = Ipv4Addr(d);
            }
            OPT_SERVER_ID if opt_len >= 4 => {
                let mut s = [0u8; 4];
                s.copy_from_slice(&opt_data[..4]);
                server_id = Ipv4Addr(s);
            }
            _ => { /* Unknown option — skip. */ }
        }

        i += opt_len;
    }

    let state = *DHCP_STATE.lock();

    match msg_type {
        Some(DHCP_OFFER) if state == DhcpState::Discovering => {
            crate::serial_println!(
                "[dhcp] OFFER: IP {} from server {}",
                offered_ip, server_id
            );

            // Store the offer.
            *PENDING_OFFER.lock() = Some(DhcpOffer {
                ip: offered_ip,
                server_ip: server_id,
                mask: subnet_mask,
                gateway: router,
                dns,
            });

            // Send REQUEST for this offer.
            send_request(offered_ip, server_id)?;
        }
        Some(DHCP_ACK) if state == DhcpState::Requesting => {
            crate::serial_println!(
                "[dhcp] ACK: IP {} mask {} gw {} dns {}",
                offered_ip, subnet_mask, router, dns
            );

            // Apply the lease.
            interface::configure(offered_ip, subnet_mask, router, dns);
            *DHCP_STATE.lock() = DhcpState::Bound;
        }
        _ => {
            // Unexpected message type or state — ignore.
        }
    }

    Ok(())
}

/// Run the full DHCP discovery process.
///
/// Sends DISCOVER, polls for OFFER, sends REQUEST, polls for ACK.
/// Blocks with polling until an IP is obtained or timeout.
///
/// Returns the assigned IP address on success.
#[allow(clippy::arithmetic_side_effects)]
pub fn discover() -> KernelResult<Ipv4Addr> {
    if !interface::is_up() {
        return Err(KernelError::NoSuchDevice);
    }

    crate::serial_println!("[dhcp] Starting DHCP discovery...");
    send_discover()?;

    // Poll for responses (up to ~5 seconds).
    for _ in 0..5000 {
        // Poll the NIC.
        super::poll();

        let state = *DHCP_STATE.lock();
        if state == DhcpState::Bound {
            let ip = interface::ip();
            crate::serial_println!("[dhcp] Bound to {}", ip);
            return Ok(ip);
        }

        // Brief spin delay (~1ms per iteration).
        for _ in 0..10_000 {
            core::hint::spin_loop();
        }
    }

    crate::serial_println!("[dhcp] Discovery timed out");
    Err(KernelError::TimedOut)
}

/// Return the current DHCP state as a human-readable string.
pub fn state_str() -> &'static str {
    match *DHCP_STATE.lock() {
        DhcpState::Idle => "idle",
        DhcpState::Discovering => "discovering",
        DhcpState::Requesting => "requesting",
        DhcpState::Bound => "bound",
    }
}
