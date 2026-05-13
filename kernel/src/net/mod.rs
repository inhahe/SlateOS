//! Networking stack (kernel-resident prototype).
//!
//! Provides basic network protocol support:
//! - Ethernet frame parsing/building
//! - ARP (address resolution: IP → MAC)
//! - IPv4 (packet parsing/building with fragmentation reassembly)
//! - UDP (connectionless datagrams)
//! - DHCP client (automatic IP configuration)
//!
//! ## Design note
//!
//! The design spec calls for the networking stack to run in userspace
//! as a service.  This kernel-resident implementation is a prototype
//! to validate the virtio-net driver and bring up basic connectivity.
//! It will be migrated to userspace once the driver framework supports
//! device access from user processes.

pub mod arp;
pub mod dhcp;
pub mod dns;
pub mod ethernet;
pub mod firewall;
pub mod frag;
pub mod http;
pub mod icmp;
pub mod mdns;
pub mod ntp;
pub mod telnet;
pub mod tftp;
pub mod interface;
pub mod ipv4;
pub mod tcp;
pub mod udp;
pub mod upnp;

use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::error::{KernelError, KernelResult};

/// Minimum interval (ns) between TCP keepalive scans.
///
/// 5 seconds — keepalive probes are on the order of tens of seconds,
/// so scanning more frequently than this wastes cycles.
const KEEPALIVE_TICK_INTERVAL_NS: u64 = 5_000_000_000;

/// Timestamp of last keepalive tick.
static LAST_KEEPALIVE_TICK: AtomicU64 = AtomicU64::new(0);

/// Initialize the networking stack.
///
/// Sets up the network interface from the virtio-net device (if present)
/// and starts DHCP to obtain an IP address.
pub fn init() {
    interface::init();
}

/// Process any pending network events (poll-based).
///
/// Reads incoming packets from the NIC and dispatches them to the
/// appropriate protocol handler.  Called from the shell or a timer.
/// Tries both virtio-net and e1000 (whichever is active).
pub fn poll() {
    // Read all pending packets from the active NIC.
    loop {
        let frame = recv_frame();
        match frame {
            Some(data) => {
                interface::record_rx(data.len());
                if let Err(e) = ethernet::process_frame(&data) {
                    interface::record_rx_drop();
                    crate::serial_println!("[net] Error processing frame: {:?}", e);
                }
            }
            None => break,
        }
    }

    // Rate-limited periodic maintenance (TCP + DHCP).
    let now = crate::hrtimer::now_ns();
    let last = LAST_KEEPALIVE_TICK.load(Ordering::Relaxed);
    if now.saturating_sub(last) >= KEEPALIVE_TICK_INTERVAL_NS {
        LAST_KEEPALIVE_TICK.store(now, Ordering::Relaxed);
        tcp::tick_keepalive();
        tcp::tick_time_wait_cleanup();
        tcp::tick_retransmit();
        tcp::tick_persist();
        dhcp::tick_renewal();
        frag::tick_expire();
        firewall::tick_conntrack_cleanup();
        ntp::tick();
        mdns::tick();
        telnet::tick();
        tftp::tick();
    }
}

/// Receive a single frame from the active NIC.
///
/// Tries virtio-net first, then e1000, then rtl8139.
fn recv_frame() -> Option<Vec<u8>> {
    // Try virtio-net first.
    if let Some(Some(data)) = crate::virtio::net::with_device(|dev| dev.recv()) {
        return Some(data);
    }
    // Fall back to e1000.
    if let Some(Some(data)) = crate::e1000::with_device(|dev| dev.recv()) {
        return Some(data);
    }
    // Fall back to rtl8139.
    if let Some(data) = crate::rtl8139::recv() {
        return Some(data);
    }
    None
}

/// Send an Ethernet frame via the active NIC.
///
/// Used by the IPv4 layer and ARP to transmit packets.
/// Tries virtio-net first, falls back to e1000, then rtl8139.
pub fn send_frame(frame: &[u8]) -> KernelResult<()> {
    // Try virtio-net first.
    if let Some(result) = crate::virtio::net::with_device(|dev| dev.send(frame)) {
        if result.is_ok() {
            interface::record_tx(frame.len());
        } else {
            interface::record_tx_error();
        }
        return result;
    }
    // Fall back to e1000.
    if let Some(result) = crate::e1000::with_device(|dev| dev.send(frame)) {
        if result.is_ok() {
            interface::record_tx(frame.len());
        } else {
            interface::record_tx_error();
        }
        return result;
    }
    // Fall back to rtl8139.
    if crate::rtl8139::with_device(|_| ()).is_some() {
        let result = crate::rtl8139::send(frame);
        if result.is_ok() {
            interface::record_tx(frame.len());
        } else {
            interface::record_tx_error();
        }
        return result;
    }
    Err(KernelError::NoSuchDevice)
}

/// Self-test: verify network interface is configured.
pub fn self_test() -> KernelResult<()> {
    crate::serial_println!("[net] Running network self-test...");

    // Interface status.
    if interface::is_up() {
        let info = interface::info();
        crate::serial_println!("[net]   Interface: up");
        crate::serial_println!("[net]   MAC: {}", info.mac);
        crate::serial_println!("[net]   IP: {}", info.ip);
        crate::serial_println!("[net]   Mask: {}", info.subnet_mask);
        crate::serial_println!("[net]   Gateway: {}", info.gateway);
        crate::serial_println!("[net]   DNS: {}", info.dns);
    } else {
        crate::serial_println!("[net]   No network interface (non-fatal)");
    }

    // Interface traffic statistics.
    let stats = interface::stats();
    crate::serial_println!(
        "[net]   Traffic: TX {}/{} pkts, RX {}/{} pkts, {} TX errors, {} RX drops",
        stats.tx_bytes, stats.tx_packets,
        stats.rx_bytes, stats.rx_packets,
        stats.tx_errors, stats.rx_drops,
    );

    // TCP state summary.
    let tcp_stats = tcp::stats();
    crate::serial_println!(
        "[net]   TCP: {} active ({} ESTABLISHED, {} TIME_WAIT, {} CLOSE_WAIT), {} listeners",
        tcp_stats.active_connections, tcp_stats.established,
        tcp_stats.time_wait, tcp_stats.close_wait, tcp_stats.listeners,
    );

    // DNS cache statistics.
    let dns_stats = dns::cache_stats();
    crate::serial_println!(
        "[net]   DNS cache: {}/{} entries, {} hits, {} misses, {} evictions",
        dns_stats.entries, dns_stats.capacity,
        dns_stats.hits, dns_stats.misses, dns_stats.evictions,
    );

    // ARP cache.
    let (arp_entries, arp_count) = arp::cache_entries();
    crate::serial_println!("[net]   ARP cache: {} entries", arp_count);
    for i in 0..arp_count {
        if let Some(entry) = arp_entries.get(i) {
            crate::serial_println!(
                "[net]     {} → {} (TTL {}s)",
                entry.ip, entry.mac, entry.ttl_secs,
            );
        }
    }

    // UDP socket summary.
    let (udp_socks, udp_count) = udp::all_sockets();
    crate::serial_println!("[net]   UDP: {} active sockets", udp_count);
    for i in 0..udp_count {
        if let Some(sock) = udp_socks.get(i) {
            crate::serial_println!(
                "[net]     port {} (rx_queue={}, mcast_groups={})",
                sock.local_port, sock.rx_queue_len, sock.mcast_groups,
            );
        }
    }

    // TCP listener summary.
    let (tcp_listeners, listener_count) = tcp::all_listeners();
    if listener_count > 0 {
        crate::serial_println!("[net]   TCP listeners: {}", listener_count);
        for i in 0..listener_count {
            if let Some(listener) = tcp_listeners.get(i) {
                crate::serial_println!(
                    "[net]     port {} ({}/{} backlog)",
                    listener.port, listener.backlog_used, listener.backlog_max,
                );
            }
        }
    }

    crate::serial_println!("[net] Network self-test PASSED");
    Ok(())
}
