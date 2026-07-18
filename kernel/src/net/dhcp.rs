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
//! - Transaction ID (xid) randomized per transaction to prevent spoofed
//!   DHCP responses from LAN attackers.
//! - Parsed options: subnet mask, router, DNS, lease time, renewal time
//!   (T1), rebinding time (T2), domain name, NTP server.
//! - Handles DHCP NAK: resets state to Idle so discovery can retry.
//! - Sends gratuitous ARP after binding (via `interface::configure`).
//!
//! ## Lease renewal (RFC 2131 §4.4.5)
//!
//! After obtaining a lease, the client maintains it via periodic renewal:
//! - At T1 (default 50% of lease), sends unicast REQUEST to the original
//!   server (Renewing state).
//! - At T2 (default 87.5% of lease), broadcasts REQUEST to any server
//!   (Rebinding state) if the original server didn't respond.
//! - At lease expiration, releases the IP, flushes DNS cache, and returns
//!   to Idle for re-discovery.
//!
//! `tick_renewal()` drives this state machine and is called from `net::poll`.

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

/// DHCP message types.
const DHCP_NAK: u8 = 6;

/// DHCP option codes.
const OPT_SUBNET_MASK: u8 = 1;
const OPT_HOSTNAME: u8 = 12;
const OPT_DOMAIN_NAME: u8 = 15;
const OPT_ROUTER: u8 = 3;
const OPT_DNS: u8 = 6;
const OPT_NTP: u8 = 42;
const OPT_REQUESTED_IP: u8 = 50;
const OPT_LEASE_TIME: u8 = 51;
const OPT_MSG_TYPE: u8 = 53;
const OPT_SERVER_ID: u8 = 54;
const OPT_RENEWAL_TIME: u8 = 58;
const OPT_REBINDING_TIME: u8 = 59;
const OPT_END: u8 = 255;

/// DHCP magic cookie (99.130.83.99).
const MAGIC_COOKIE: [u8; 4] = [99, 130, 83, 99];

/// Minimum DHCP message size (without options).
const DHCP_MIN_SIZE: usize = 236;

/// Current DHCP transaction ID.
///
/// Randomized per transaction to prevent spoofed DHCP responses.
/// An attacker on the LAN who can predict the XID could inject
/// a fake OFFER/ACK with a rogue gateway or DNS server.
static CURRENT_XID: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);

/// Generate a new random DHCP transaction ID.
fn new_xid() -> u32 {
    let xid = crate::rng::next_u32();
    CURRENT_XID.store(xid, core::sync::atomic::Ordering::Relaxed);
    xid
}

/// Get the current transaction ID (for matching responses).
fn current_xid() -> u32 {
    CURRENT_XID.load(core::sync::atomic::Ordering::Relaxed)
}

// ---------------------------------------------------------------------------
// DHCP state
// ---------------------------------------------------------------------------

use crate::sync::Mutex;

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
    /// T1 expired, RENEW REQUEST sent unicast to server, waiting for ACK.
    Renewing,
    /// T2 expired, REBIND REQUEST sent broadcast, waiting for ACK.
    Rebinding,
}

/// Pending DHCP offer data.
#[allow(dead_code)] // Spec-defined fields.
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

/// DHCP lease information (stored after successful ACK).
struct DhcpLease {
    /// Lease duration in seconds (0 = infinite).
    lease_time_secs: u32,
    /// T1 renewal time in seconds (default: 50% of lease).
    renewal_time_secs: u32,
    /// T2 rebinding time in seconds (default: 87.5% of lease).
    rebinding_time_secs: u32,
    /// Domain name (null-terminated, max 64 bytes).
    domain_name: [u8; 64],
    /// Domain name length (0 if not provided).
    domain_name_len: usize,
    /// NTP server address.
    ntp_server: Ipv4Addr,
    /// Timestamp when the lease was obtained (hrtimer ns).
    obtained_ns: u64,
    /// DHCP server that granted this lease (for unicast renewal).
    server_ip: Ipv4Addr,
    /// Our assigned IP address.
    client_ip: Ipv4Addr,
    /// Subnet mask.
    subnet_mask: Ipv4Addr,
    /// Default gateway.
    gateway: Ipv4Addr,
    /// DNS server.
    dns: Ipv4Addr,
}

impl DhcpLease {
    const fn empty() -> Self {
        Self {
            lease_time_secs: 0,
            renewal_time_secs: 0,
            rebinding_time_secs: 0,
            domain_name: [0; 64],
            domain_name_len: 0,
            ntp_server: Ipv4Addr::UNSPECIFIED,
            obtained_ns: 0,
            server_ip: Ipv4Addr::UNSPECIFIED,
            client_ip: Ipv4Addr::UNSPECIFIED,
            subnet_mask: Ipv4Addr::UNSPECIFIED,
            gateway: Ipv4Addr::UNSPECIFIED,
            dns: Ipv4Addr::UNSPECIFIED,
        }
    }
}

static DHCP_STATE: Mutex<DhcpState> = Mutex::new(DhcpState::Idle);
static PENDING_OFFER: Mutex<Option<DhcpOffer>> = Mutex::new(None);
static CURRENT_LEASE: Mutex<DhcpLease> = Mutex::new(DhcpLease::empty());

/// Renewal retry state for exponential backoff (RFC 2131 §4.4.5).
///
/// Tracks the timestamp of the last renewal/rebind attempt and a retry
/// counter for exponential backoff.  Initial retry interval is 4 seconds,
/// doubling each attempt (4s, 8s, 16s, 32s, ...).
struct RenewalRetry {
    /// Nanosecond timestamp of the last renewal/rebind REQUEST sent.
    last_attempt_ns: u64,
    /// Number of retries since entering the current renewal state.
    retries: u32,
}

impl RenewalRetry {
    const fn new() -> Self {
        Self { last_attempt_ns: 0, retries: 0 }
    }

    /// Reset retry state (e.g., when transitioning to a new renewal phase).
    fn reset(&mut self) {
        self.last_attempt_ns = 0;
        self.retries = 0;
    }

    /// Compute the current retry interval in nanoseconds.
    ///
    /// Starts at 4 seconds, doubles each retry, capped at 64 seconds.
    fn interval_ns(&self) -> u64 {
        let base_secs: u64 = 4;
        let secs = base_secs.saturating_mul(1u64.checked_shl(self.retries).unwrap_or(u64::MAX));
        secs.min(64).saturating_mul(1_000_000_000)
    }
}

static RENEWAL_RETRY: Mutex<RenewalRetry> = Mutex::new(RenewalRetry::new());

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
    // xid (transaction ID) — use current random XID.
    msg.extend_from_slice(&current_xid().to_be_bytes());
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

/// Build a DHCP renewal message with `ciaddr` set to our current IP.
///
/// Per RFC 2131 §4.3.2: RENEWING/REBINDING clients set `ciaddr` to their
/// current IP and do NOT include `OPT_REQUESTED_IP` or `OPT_SERVER_ID`.
#[allow(clippy::arithmetic_side_effects)]
fn build_dhcp_renew_message(our_mac: &MacAddress, our_ip: Ipv4Addr) -> Vec<u8> {
    let mut msg = Vec::with_capacity(300);

    msg.push(1);  // op = BOOTREQUEST
    msg.push(1);  // htype = Ethernet
    msg.push(6);  // hlen = 6
    msg.push(0);  // hops
    msg.extend_from_slice(&current_xid().to_be_bytes());
    msg.extend_from_slice(&0u16.to_be_bytes()); // secs
    msg.extend_from_slice(&0u16.to_be_bytes()); // flags = 0 (unicast OK)
    msg.extend_from_slice(&our_ip.0);           // ciaddr = our current IP
    msg.extend_from_slice(&[0, 0, 0, 0]);       // yiaddr
    msg.extend_from_slice(&[0, 0, 0, 0]);       // siaddr
    msg.extend_from_slice(&[0, 0, 0, 0]);       // giaddr
    msg.extend_from_slice(&our_mac.0);
    msg.extend_from_slice(&[0u8; 10]);
    msg.extend_from_slice(&[0u8; 64]);  // sname
    msg.extend_from_slice(&[0u8; 128]); // file
    msg.extend_from_slice(&MAGIC_COOKIE);

    // Option: DHCP Message Type = REQUEST
    msg.push(OPT_MSG_TYPE);
    msg.push(1);
    msg.push(DHCP_REQUEST);

    msg.push(OPT_END);

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

/// Build a unicast UDP/IP packet for DHCP renewal (our_ip:68 → server_ip:67).
///
/// Used for T1 renewal where we send directly to the DHCP server.
#[allow(clippy::arithmetic_side_effects)]
fn build_dhcp_ip_udp_unicast(dhcp_payload: &[u8], src_ip: Ipv4Addr, dst_ip: Ipv4Addr) -> Vec<u8> {
    let udp_len = 8 + dhcp_payload.len();
    let ip_total_len = 20 + udp_len;

    let mut pkt = Vec::with_capacity(ip_total_len);

    // --- IPv4 header (20 bytes) ---
    pkt.push(0x45);
    pkt.push(0);
    pkt.extend_from_slice(&(ip_total_len as u16).to_be_bytes());
    pkt.extend_from_slice(&0u16.to_be_bytes());
    pkt.extend_from_slice(&0u16.to_be_bytes());
    pkt.push(128);  // TTL
    pkt.push(17);   // Protocol = UDP
    let checksum_offset = pkt.len();
    pkt.extend_from_slice(&[0, 0]);
    pkt.extend_from_slice(&src_ip.0);
    pkt.extend_from_slice(&dst_ip.0);

    let checksum = super::ipv4::ip_checksum(&pkt[..20]);
    pkt[checksum_offset] = (checksum >> 8) as u8;
    pkt[checksum_offset + 1] = checksum as u8;

    // --- UDP header ---
    pkt.extend_from_slice(&DHCP_CLIENT_PORT.to_be_bytes());
    pkt.extend_from_slice(&DHCP_SERVER_PORT.to_be_bytes());
    pkt.extend_from_slice(&(udp_len as u16).to_be_bytes());
    pkt.extend_from_slice(&0u16.to_be_bytes()); // Checksum disabled

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

    super::send_frame(&frame)?;

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

    super::send_frame(&frame)?;

    *DHCP_STATE.lock() = DhcpState::Requesting;
    crate::serial_println!("[dhcp] REQUEST sent for {}", requested_ip);

    Ok(())
}

/// Send a DHCP RENEW request (unicast to server, RFC 2131 §4.3.2).
///
/// Used at T1 — extends the lease by contacting the original server.
fn send_renew(our_ip: Ipv4Addr, server_ip: Ipv4Addr) -> KernelResult<()> {
    let our_mac = interface::mac();
    // NOTE: do NOT call new_xid() here — the XID is set once at the
    // start of the renewal transaction (Bound→Renewing transition).
    // Retransmits must reuse the same XID so the server's ACK matches.

    let dhcp_msg = build_dhcp_renew_message(&our_mac, our_ip);
    let ip_udp_packet = build_dhcp_ip_udp_unicast(&dhcp_msg, our_ip, server_ip);

    // Unicast — need server's MAC.  Use ARP-resolved MAC or fall back
    // to gateway MAC (the server might be on a different subnet).
    let dst_mac = super::arp::lookup(server_ip)
        .or_else(|| {
            let gw = interface::info().gateway;
            super::arp::lookup(gw)
        })
        .unwrap_or(BROADCAST_MAC);

    let frame = ethernet::build_frame(
        &dst_mac,
        &our_mac,
        ETHERTYPE_IPV4,
        &ip_udp_packet,
    );

    super::send_frame(&frame)?;

    *DHCP_STATE.lock() = DhcpState::Renewing;
    crate::serial_println!("[dhcp] RENEW sent to {} for {}", server_ip, our_ip);

    Ok(())
}

/// Send a DHCP REBIND request (broadcast, RFC 2131 §4.3.2).
///
/// Used at T2 — the original server didn't respond to RENEW, so
/// broadcast to find any server willing to extend the lease.
fn send_rebind(our_ip: Ipv4Addr) -> KernelResult<()> {
    let our_mac = interface::mac();

    let dhcp_msg = build_dhcp_renew_message(&our_mac, our_ip);
    let ip_udp_packet = build_dhcp_ip_udp(&dhcp_msg);

    let frame = ethernet::build_frame(
        &BROADCAST_MAC,
        &our_mac,
        ETHERTYPE_IPV4,
        &ip_udp_packet,
    );

    super::send_frame(&frame)?;

    *DHCP_STATE.lock() = DhcpState::Rebinding;
    crate::serial_println!("[dhcp] REBIND broadcast for {}", our_ip);

    Ok(())
}

// ---------------------------------------------------------------------------
// DHCP lease renewal tick
// ---------------------------------------------------------------------------

/// Periodic check for DHCP lease renewal (called from net::poll).
///
/// RFC 2131 §4.4.5 renewal timers:
/// - **T1 (default 50% of lease)**: send unicast REQUEST to original server.
/// - **T2 (default 87.5% of lease)**: broadcast REQUEST to any server.
/// - **Lease expiration**: release the IP, transition to Idle for re-discovery.
///
/// State transitions:
/// - Bound → Renewing (at T1)
/// - Renewing → Rebinding (at T2, if server didn't respond)
/// - Rebinding → Idle (at expiration, if no server responded)
#[allow(clippy::arithmetic_side_effects)]
pub fn tick_renewal() {
    let state = *DHCP_STATE.lock();

    match state {
        DhcpState::Bound => {
            // Check if T1 has passed → start renewal.
            let lease = CURRENT_LEASE.lock();
            if lease.lease_time_secs == 0 || lease.obtained_ns == 0 {
                return; // Infinite lease or not configured.
            }
            let elapsed_ns = crate::hrtimer::now_ns().saturating_sub(lease.obtained_ns);
            let elapsed_secs = elapsed_ns / 1_000_000_000;
            let t1 = lease.renewal_time_secs as u64;

            if elapsed_secs >= t1 {
                let our_ip = lease.client_ip;
                let server = lease.server_ip;
                drop(lease);
                // Generate a fresh XID for the entire renewal transaction.
                // Retransmits will reuse this XID so server ACKs match.
                new_xid();
                // Reset retry state for the new renewal phase.
                let mut retry = RENEWAL_RETRY.lock();
                retry.reset();
                retry.last_attempt_ns = crate::hrtimer::now_ns();
                retry.retries = 1;
                drop(retry);
                crate::serial_println!(
                    "[dhcp] T1 expired ({}s elapsed) — initiating renewal",
                    elapsed_secs
                );
                // Renewal send failure is non-fatal — we'll retry on next tick.
                let _ = send_renew(our_ip, server);
            }
        }
        DhcpState::Renewing => {
            let now = crate::hrtimer::now_ns();
            let lease = CURRENT_LEASE.lock();
            if lease.obtained_ns == 0 {
                return;
            }
            let elapsed_ns = now.saturating_sub(lease.obtained_ns);
            let elapsed_secs = elapsed_ns / 1_000_000_000;
            let t2 = lease.rebinding_time_secs as u64;

            if elapsed_secs >= t2 {
                // T2 expired — escalate to rebinding (broadcast).
                let our_ip = lease.client_ip;
                drop(lease);
                // Generate a fresh XID for the rebinding transaction.
                new_xid();
                // Reset retry state and stamp the first attempt so we
                // don't retransmit immediately on the next tick.
                let mut retry = RENEWAL_RETRY.lock();
                retry.reset();
                retry.last_attempt_ns = crate::hrtimer::now_ns();
                retry.retries = 1;
                drop(retry);
                crate::serial_println!(
                    "[dhcp] T2 expired ({}s elapsed) — escalating to rebind",
                    elapsed_secs
                );
                // Rebind send failure is non-fatal — we'll retry on next tick.
                let _ = send_rebind(our_ip);
            } else {
                // Still within T1..T2 — retransmit renewal REQUEST
                // with exponential backoff (RFC 2131 §4.4.5).
                let our_ip = lease.client_ip;
                let server = lease.server_ip;
                drop(lease);

                let mut retry = RENEWAL_RETRY.lock();
                let since_last = now.saturating_sub(retry.last_attempt_ns);
                if since_last >= retry.interval_ns() {
                    retry.last_attempt_ns = now;
                    retry.retries = retry.retries.saturating_add(1);
                    let attempt = retry.retries;
                    drop(retry);
                    crate::serial_println!(
                        "[dhcp] RENEW retransmit #{} to {} for {}",
                        attempt, server, our_ip
                    );
                    // Send failure is non-fatal — will retry with backoff.
                    let _ = send_renew(our_ip, server);
                }
            }
        }
        DhcpState::Rebinding => {
            let now = crate::hrtimer::now_ns();
            let lease = CURRENT_LEASE.lock();
            if lease.obtained_ns == 0 {
                return;
            }
            let elapsed_ns = now.saturating_sub(lease.obtained_ns);
            let elapsed_secs = elapsed_ns / 1_000_000_000;
            let total = lease.lease_time_secs as u64;

            if elapsed_secs >= total {
                drop(lease);
                RENEWAL_RETRY.lock().reset();
                crate::serial_println!(
                    "[dhcp] Lease expired ({}s) — releasing IP, returning to idle",
                    total
                );
                // Clear the IP configuration.
                interface::configure(
                    Ipv4Addr::UNSPECIFIED,
                    Ipv4Addr::UNSPECIFIED,
                    Ipv4Addr::UNSPECIFIED,
                    Ipv4Addr::UNSPECIFIED,
                );
                super::dns::flush_cache();
                super::arp::flush_cache();
                *DHCP_STATE.lock() = DhcpState::Idle;
                *CURRENT_LEASE.lock() = DhcpLease::empty();
            } else {
                // Retransmit broadcast REQUEST with exponential backoff.
                let our_ip = lease.client_ip;
                drop(lease);

                let mut retry = RENEWAL_RETRY.lock();
                let since_last = now.saturating_sub(retry.last_attempt_ns);
                if since_last >= retry.interval_ns() {
                    retry.last_attempt_ns = now;
                    retry.retries = retry.retries.saturating_add(1);
                    let attempt = retry.retries;
                    drop(retry);
                    crate::serial_println!(
                        "[dhcp] REBIND retransmit #{} for {}", attempt, our_ip
                    );
                    let _ = send_rebind(our_ip);
                }
            }
        }
        _ => {} // Idle, Discovering, Requesting — nothing to do.
    }
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
    if xid != current_xid() {
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
    let mut lease_time: u32 = 0;
    let mut renewal_time: u32 = 0;
    let mut rebinding_time: u32 = 0;
    let mut domain_name = [0u8; 64];
    let mut domain_name_len: usize = 0;
    let mut ntp_server = Ipv4Addr::UNSPECIFIED;

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
            OPT_LEASE_TIME if opt_len >= 4 => {
                lease_time = u32::from_be_bytes([
                    opt_data[0], opt_data[1], opt_data[2], opt_data[3],
                ]);
            }
            OPT_RENEWAL_TIME if opt_len >= 4 => {
                renewal_time = u32::from_be_bytes([
                    opt_data[0], opt_data[1], opt_data[2], opt_data[3],
                ]);
            }
            OPT_REBINDING_TIME if opt_len >= 4 => {
                rebinding_time = u32::from_be_bytes([
                    opt_data[0], opt_data[1], opt_data[2], opt_data[3],
                ]);
            }
            OPT_DOMAIN_NAME => {
                // Copy domain name (truncate to buffer size, leave room
                // for null terminator).
                let copy_len = opt_len.min(domain_name.len().saturating_sub(1));
                domain_name[..copy_len].copy_from_slice(&opt_data[..copy_len]);
                // Explicitly null-terminate to prevent unterminated reads.
                if let Some(slot) = domain_name.get_mut(copy_len) {
                    *slot = 0;
                }
                domain_name_len = copy_len;
            }
            OPT_NTP if opt_len >= 4 => {
                let mut n = [0u8; 4];
                n.copy_from_slice(&opt_data[..4]);
                ntp_server = Ipv4Addr(n);
            }
            OPT_HOSTNAME => { /* Informational — we don't set hostname from DHCP. */ }
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
        Some(DHCP_ACK) if state == DhcpState::Requesting
            || state == DhcpState::Renewing
            || state == DhcpState::Rebinding => {
            // During renewal/rebinding, the server may leave yiaddr=0
            // (RFC 2131 §4.3.2) meaning "keep your current IP".  Substitute
            // the current lease IP so downstream logic works correctly.
            let offered_ip = if (state == DhcpState::Renewing || state == DhcpState::Rebinding)
                && offered_ip.is_unspecified()
            {
                CURRENT_LEASE.lock().client_ip
            } else {
                offered_ip
            };

            // During renewal/rebinding, verify the ACK is for the IP we
            // currently hold.  A spoofed ACK for a different IP could
            // silently reassign our address.
            if (state == DhcpState::Renewing || state == DhcpState::Rebinding)
                && !offered_ip.is_unspecified()
            {
                let current = CURRENT_LEASE.lock().client_ip;
                if !current.is_unspecified() && offered_ip != current {
                    crate::serial_println!(
                        "[dhcp] ACK IP {} doesn't match current lease {} — ignoring",
                        offered_ip, current,
                    );
                    return Ok(());
                }
            }

            // Reject obviously invalid offered IPs.
            if offered_ip.is_unspecified()
                || offered_ip.is_broadcast()
                || offered_ip.is_multicast()
                || offered_ip.0[0] == 127
            {
                crate::serial_println!(
                    "[dhcp] ACK offered invalid IP {} — ignoring", offered_ip,
                );
                return Ok(());
            }

            // Compute default T1/T2 if server didn't provide them.
            // lease_time == 0 means "infinite lease" — no renewal needed.
            let mut t1 = if renewal_time > 0 {
                renewal_time
            } else if lease_time > 0 {
                lease_time / 2 // 50% of lease (RFC 2131 default).
            } else {
                0 // Infinite lease — tick_renewal() skips when T1/lease == 0.
            };
            let mut t2 = if rebinding_time > 0 {
                rebinding_time
            } else if lease_time > 0 {
                // 87.5% of lease (RFC 2131 default).
                lease_time.saturating_mul(7) / 8
            } else {
                0
            };

            // Sanitize: enforce T1 ≤ T2 ≤ lease_time (RFC 2131 §4.4.5).
            // A misconfigured or malicious server could send values that
            // violate this ordering, causing premature lease expiry or
            // renewal storms.
            if lease_time > 0 {
                if t2 > lease_time {
                    t2 = lease_time.saturating_mul(7) / 8;
                }
                if t1 > t2 {
                    t1 = t2 / 2;
                }
            }

            crate::serial_println!(
                "[dhcp] ACK: IP {} mask {} gw {} dns {} lease={}s T1={}s T2={}s",
                offered_ip, subnet_mask, router, dns,
                lease_time, t1, t2
            );
            if domain_name_len > 0 {
                // Log domain name as best-effort UTF-8 (only for serial
                // display; the stored bytes are the canonical form).
                if let Ok(name) = core::str::from_utf8(&domain_name[..domain_name_len]) {
                    crate::serial_println!("[dhcp]   domain: {}", name);
                }
            }
            if !ntp_server.is_unspecified() {
                crate::serial_println!("[dhcp]   NTP: {}", ntp_server);
            }

            // Apply the lease.
            interface::configure(offered_ip, subnet_mask, router, dns);

            // Flush DNS cache — the DNS server may have changed, and
            // cached results from the old server may be stale or wrong.
            super::dns::flush_cache();

            // Flush ARP cache — the gateway or neighbors may have
            // changed MAC addresses after a lease renewal.  The
            // gratuitous ARP sent by configure() handles our own
            // announcement, but stale entries for the gateway/peers
            // could cause misrouted frames.
            super::arp::flush_cache();

            *DHCP_STATE.lock() = DhcpState::Bound;

            // Reset renewal retry state on successful lease/renewal.
            RENEWAL_RETRY.lock().reset();

            // Store lease details.
            let mut lease = CURRENT_LEASE.lock();
            lease.lease_time_secs = lease_time;
            lease.renewal_time_secs = t1;
            lease.rebinding_time_secs = t2;
            lease.domain_name[..domain_name_len].copy_from_slice(&domain_name[..domain_name_len]);
            lease.domain_name_len = domain_name_len;
            lease.ntp_server = ntp_server;
            lease.obtained_ns = crate::hrtimer::now_ns();
            lease.server_ip = server_id;
            lease.client_ip = offered_ip;
            lease.subnet_mask = subnet_mask;
            lease.gateway = router;
            lease.dns = dns;
        }
        Some(DHCP_NAK) if state == DhcpState::Requesting
            || state == DhcpState::Renewing
            || state == DhcpState::Rebinding => {
            crate::serial_println!(
                "[dhcp] NAK from server {} — offer rejected, restarting",
                server_id
            );
            // If we were renewing/rebinding, the server is telling us our
            // current IP is no longer valid.  Release the interface config
            // so we don't continue using a stale address.
            if state == DhcpState::Renewing || state == DhcpState::Rebinding {
                interface::configure(
                    Ipv4Addr::UNSPECIFIED,
                    Ipv4Addr::UNSPECIFIED,
                    Ipv4Addr::UNSPECIFIED,
                    Ipv4Addr::UNSPECIFIED,
                );
                super::dns::flush_cache();
                super::arp::flush_cache();
                *CURRENT_LEASE.lock() = DhcpLease::empty();
                crate::serial_println!(
                    "[dhcp] Released IP configuration after renewal NAK"
                );
            }
            // Clear offer and return to Idle so discovery can retry.
            *PENDING_OFFER.lock() = None;
            *DHCP_STATE.lock() = DhcpState::Idle;
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

    // Generate a fresh random transaction ID for this discovery.
    let xid = new_xid();
    crate::serial_println!("[dhcp] Starting DHCP discovery (xid=0x{:08x})...", xid);
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
        DhcpState::Renewing => "renewing",
        DhcpState::Rebinding => "rebinding",
    }
}

/// Return the current lease duration in seconds (0 if not bound or
/// server didn't provide a lease time).
#[allow(dead_code)] // Diagnostic/status API.
pub fn lease_time_secs() -> u32 {
    CURRENT_LEASE.lock().lease_time_secs
}

/// Return the remaining lease time in seconds, or 0 if not bound.
#[allow(dead_code, clippy::arithmetic_side_effects)]
pub fn lease_remaining_secs() -> u64 {
    let lease = CURRENT_LEASE.lock();
    if lease.lease_time_secs == 0 || lease.obtained_ns == 0 {
        return 0;
    }
    let elapsed_ns = crate::hrtimer::now_ns().saturating_sub(lease.obtained_ns);
    let elapsed_secs = elapsed_ns / 1_000_000_000;
    let total = lease.lease_time_secs as u64;
    total.saturating_sub(elapsed_secs)
}

/// Return the NTP server address from the DHCP lease (UNSPECIFIED if
/// the server didn't provide one).
#[allow(dead_code)] // Will be used when NTP client is implemented.
pub fn ntp_server() -> Ipv4Addr {
    CURRENT_LEASE.lock().ntp_server
}

/// Return the domain name from the DHCP lease as a byte slice.
///
/// Returns an empty slice if the server didn't provide a domain name.
#[allow(dead_code)] // Status/diagnostic API.
pub fn domain_name() -> ([u8; 64], usize) {
    let lease = CURRENT_LEASE.lock();
    (lease.domain_name, lease.domain_name_len)
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// DHCP unit tests — exercises packet building, magic cookie placement,
/// IP header checksum, renewal retry intervals, and state string lookup.
pub fn self_test() -> KernelResult<()> {
    crate::serial_println!("[dhcp] Running DHCP self-test...");

    test_build_dhcp_message_structure()?;
    test_build_dhcp_ip_udp_header()?;
    test_renewal_retry_interval()?;
    test_state_str()?;
    test_build_renew_message()?;

    crate::serial_println!("[dhcp] DHCP self-test PASSED (5 tests)");
    Ok(())
}

/// Test that build_dhcp_message produces correct structure.
#[allow(clippy::arithmetic_side_effects)]
fn test_build_dhcp_message_structure() -> KernelResult<()> {
    let mac = MacAddress([0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF]);

    // Set a known XID for reproducible tests.
    CURRENT_XID.store(0x12345678, core::sync::atomic::Ordering::Relaxed);

    let msg = build_dhcp_message(DHCP_DISCOVER, &mac, &[]);

    // Minimum 300 bytes.
    if msg.len() < 300 {
        crate::serial_println!("[dhcp]   FAIL: message too short ({})", msg.len());
        return Err(KernelError::InternalError);
    }

    // op = BOOTREQUEST (1).
    if msg[0] != 1 {
        crate::serial_println!("[dhcp]   FAIL: op = {}", msg[0]);
        return Err(KernelError::InternalError);
    }

    // htype = Ethernet (1).
    if msg[1] != 1 {
        crate::serial_println!("[dhcp]   FAIL: htype = {}", msg[1]);
        return Err(KernelError::InternalError);
    }

    // hlen = 6.
    if msg[2] != 6 {
        crate::serial_println!("[dhcp]   FAIL: hlen = {}", msg[2]);
        return Err(KernelError::InternalError);
    }

    // XID = 0x12345678.
    let xid = u32::from_be_bytes([msg[4], msg[5], msg[6], msg[7]]);
    if xid != 0x12345678 {
        crate::serial_println!("[dhcp]   FAIL: xid = {:#010x}", xid);
        return Err(KernelError::InternalError);
    }

    // chaddr: first 6 bytes should be our MAC.
    if msg[28..34] != mac.0 {
        crate::serial_println!("[dhcp]   FAIL: chaddr doesn't match MAC");
        return Err(KernelError::InternalError);
    }

    // Magic cookie at offset 236.
    if msg[236..240] != MAGIC_COOKIE {
        crate::serial_println!("[dhcp]   FAIL: magic cookie wrong");
        return Err(KernelError::InternalError);
    }

    // First option should be MSG_TYPE = DISCOVER (1).
    if msg[240] != OPT_MSG_TYPE || msg[241] != 1 || msg[242] != DHCP_DISCOVER {
        crate::serial_println!("[dhcp]   FAIL: msg_type option wrong");
        return Err(KernelError::InternalError);
    }

    crate::serial_println!("[dhcp]   build message structure: OK");
    Ok(())
}

/// Test that build_dhcp_ip_udp produces a valid IP/UDP header.
fn test_build_dhcp_ip_udp_header() -> KernelResult<()> {
    let mac = MacAddress([0x11; 6]);
    CURRENT_XID.store(0xDEADBEEF, core::sync::atomic::Ordering::Relaxed);

    let dhcp_msg = build_dhcp_message(DHCP_DISCOVER, &mac, &[]);
    let pkt = build_dhcp_ip_udp(&dhcp_msg);

    // Must start with IPv4 (version 4, IHL 5).
    if pkt[0] != 0x45 {
        crate::serial_println!("[dhcp]   FAIL: IP version/IHL = {:#04x}", pkt[0]);
        return Err(KernelError::InternalError);
    }

    // Protocol = UDP (17).
    if pkt[9] != 17 {
        crate::serial_println!("[dhcp]   FAIL: IP protocol = {}", pkt[9]);
        return Err(KernelError::InternalError);
    }

    // Source IP = 0.0.0.0.
    if pkt[12..16] != [0, 0, 0, 0] {
        crate::serial_println!("[dhcp]   FAIL: src IP not 0.0.0.0");
        return Err(KernelError::InternalError);
    }

    // Dest IP = 255.255.255.255.
    if pkt[16..20] != [255, 255, 255, 255] {
        crate::serial_println!("[dhcp]   FAIL: dst IP not broadcast");
        return Err(KernelError::InternalError);
    }

    // Verify IP header checksum.
    let cksum = super::ipv4::ip_checksum(&pkt[..20]);
    if cksum != 0 {
        crate::serial_println!("[dhcp]   FAIL: IP checksum = {:#06x}", cksum);
        return Err(KernelError::InternalError);
    }

    // UDP source port = 68 (DHCP client).
    let src_port = u16::from_be_bytes([pkt[20], pkt[21]]);
    if src_port != DHCP_CLIENT_PORT {
        crate::serial_println!("[dhcp]   FAIL: UDP src port = {}", src_port);
        return Err(KernelError::InternalError);
    }

    // UDP dest port = 67 (DHCP server).
    let dst_port = u16::from_be_bytes([pkt[22], pkt[23]]);
    if dst_port != DHCP_SERVER_PORT {
        crate::serial_println!("[dhcp]   FAIL: UDP dst port = {}", dst_port);
        return Err(KernelError::InternalError);
    }

    crate::serial_println!("[dhcp]   build IP/UDP header: OK");
    Ok(())
}

/// Test RenewalRetry interval exponential backoff.
fn test_renewal_retry_interval() -> KernelResult<()> {
    let mut retry = RenewalRetry::new();

    // Initial interval: 4 seconds.
    let expected_ns = 4_000_000_000u64;
    if retry.interval_ns() != expected_ns {
        crate::serial_println!("[dhcp]   FAIL: initial interval = {}", retry.interval_ns());
        return Err(KernelError::InternalError);
    }

    // After 1 retry: 8 seconds.
    retry.retries = 1;
    if retry.interval_ns() != 8_000_000_000 {
        crate::serial_println!("[dhcp]   FAIL: retry 1 interval = {}", retry.interval_ns());
        return Err(KernelError::InternalError);
    }

    // After 2 retries: 16 seconds.
    retry.retries = 2;
    if retry.interval_ns() != 16_000_000_000 {
        crate::serial_println!("[dhcp]   FAIL: retry 2 interval = {}", retry.interval_ns());
        return Err(KernelError::InternalError);
    }

    // Capped at 64 seconds.
    retry.retries = 10; // 4 * 1024 = 4096 > 64, so should cap.
    if retry.interval_ns() != 64_000_000_000 {
        crate::serial_println!("[dhcp]   FAIL: capped interval = {}", retry.interval_ns());
        return Err(KernelError::InternalError);
    }

    // Reset brings retries back to 0.
    retry.reset();
    if retry.retries != 0 || retry.last_attempt_ns != 0 {
        crate::serial_println!("[dhcp]   FAIL: reset didn't clear state");
        return Err(KernelError::InternalError);
    }

    crate::serial_println!("[dhcp]   renewal retry interval: OK");
    Ok(())
}

/// Test state_str returns correct strings.
fn test_state_str() -> KernelResult<()> {
    // Save and restore original state.
    let original = *DHCP_STATE.lock();

    *DHCP_STATE.lock() = DhcpState::Idle;
    if state_str() != "idle" {
        crate::serial_println!("[dhcp]   FAIL: Idle → '{}'", state_str());
        *DHCP_STATE.lock() = original;
        return Err(KernelError::InternalError);
    }

    *DHCP_STATE.lock() = DhcpState::Bound;
    if state_str() != "bound" {
        crate::serial_println!("[dhcp]   FAIL: Bound → '{}'", state_str());
        *DHCP_STATE.lock() = original;
        return Err(KernelError::InternalError);
    }

    *DHCP_STATE.lock() = DhcpState::Renewing;
    if state_str() != "renewing" {
        crate::serial_println!("[dhcp]   FAIL: Renewing → '{}'", state_str());
        *DHCP_STATE.lock() = original;
        return Err(KernelError::InternalError);
    }

    // Restore.
    *DHCP_STATE.lock() = original;

    crate::serial_println!("[dhcp]   state_str: OK");
    Ok(())
}

/// Test that build_dhcp_renew_message sets ciaddr correctly.
fn test_build_renew_message() -> KernelResult<()> {
    let mac = MacAddress([0x22; 6]);
    let our_ip = Ipv4Addr([192, 168, 1, 50]);
    CURRENT_XID.store(0xCAFEBABE, core::sync::atomic::Ordering::Relaxed);

    let msg = build_dhcp_renew_message(&mac, our_ip);

    if msg.len() < 300 {
        crate::serial_println!("[dhcp]   FAIL: renew message too short");
        return Err(KernelError::InternalError);
    }

    // ciaddr (offset 12-15) should be our_ip.
    if msg[12..16] != our_ip.0 {
        crate::serial_println!("[dhcp]   FAIL: ciaddr not set to our IP");
        return Err(KernelError::InternalError);
    }

    // flags should be 0 (unicast OK for renewal).
    let flags = u16::from_be_bytes([msg[10], msg[11]]);
    if flags != 0 {
        crate::serial_println!("[dhcp]   FAIL: renew flags = {:#06x}, expected 0", flags);
        return Err(KernelError::InternalError);
    }

    // msg_type option should be REQUEST (3).
    // After magic cookie at 240: opt_code, opt_len, opt_value.
    if msg[240] != OPT_MSG_TYPE || msg[241] != 1 || msg[242] != DHCP_REQUEST {
        crate::serial_println!("[dhcp]   FAIL: renew msg_type not REQUEST");
        return Err(KernelError::InternalError);
    }

    crate::serial_println!("[dhcp]   build renew message: OK");
    Ok(())
}
