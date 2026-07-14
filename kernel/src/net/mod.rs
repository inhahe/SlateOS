//! Networking stack (kernel-resident prototype).
//!
//! Provides basic network protocol support:
//! - Ethernet frame parsing/building
//! - ARP (address resolution: IPv4 → MAC)
//! - IPv4 (packet parsing/building with fragmentation reassembly)
//! - IPv6 (packet parsing/building, extension header traversal)
//! - ICMPv6 (echo request/reply, NDP neighbor solicitation/advertisement)
//! - UDP (connectionless datagrams)
//! - DHCP client (automatic IPv4 configuration)
//!
//! ## Design note
//!
//! The design spec calls for the networking stack to run in userspace
//! as a service.  This kernel-resident implementation is a prototype
//! to validate the virtio-net driver and bring up basic connectivity.
//! It will be migrated to userspace once the driver framework supports
//! device access from user processes.

pub mod arp;
pub mod bridge;
pub mod dashboard;
pub mod dhcp;
pub mod dhcpd;
pub mod dhcpv6;
pub mod dns;
pub mod ethernet;
pub mod firewall;
pub mod frag;
pub mod ftp;
pub mod http;
pub mod httpd;
pub mod icmp;
pub mod icmpv6;
pub mod igmp;
pub mod iperf;
pub mod mld;
pub mod ipv6;
pub mod lldp;
pub mod ndisc;
pub mod netcat;
pub mod netstack_client;
pub mod netstat;
pub mod mdns;
pub mod ntp;
pub mod smtp;
pub mod snmp;
pub mod socket;
pub mod socks;
pub mod ssh;
pub mod syslog;
pub mod telnet;
pub mod tftp;
pub mod interface;
pub mod ipv4;
pub mod tcp;
pub mod tls;
pub mod udp;
pub mod pcap;
pub mod qos;
pub mod raw;
pub mod nat;
pub mod traceroute;
pub mod upnp;
pub mod veth;
pub mod vlan;
pub mod websocket;
pub mod wol;

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
    // If a userspace raw owner (the `netstack` daemon) holds the NIC, the
    // in-kernel stack must NOT drain physical-uplink frames — they belong to
    // the daemon, which reads them via `sys_net_raw_rx`.  Skip the physical
    // drain but keep servicing container-internal veth/bridge traffic and
    // periodic maintenance below.  See net::raw / design-decisions.md §63.
    if !raw::is_claimed() {
        // Read all pending packets from the active NIC.
        loop {
            let frame = recv_frame();
            match frame {
                Some(data) => {
                    pcap::capture_rx(&data);
                    interface::record_rx(data.len());
                    if let Err(e) = ethernet::process_frame(&data, crate::netns::ROOT_NS) {
                        interface::record_rx_drop();
                        crate::serial_println!("[net] Error processing frame: {:?}", e);
                    }
                }
                None => break,
            }
        }
    }

    // Layer-2-switch frames among bridged container-network veth ports before
    // the generic drain: a bridged host-end is skipped by `poll_all`, so the
    // bridge must be the one to consume and forward its frames (to same-network
    // peers, and — for unknown/broadcast — to the host stack).
    bridge::forward_all();

    // Drain virtual ethernet (veth) pair RX queues into the protocol stack.
    veth::poll_all();

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
        dhcpv6::tick_renewal();
        frag::tick_expire();
        firewall::tick_conntrack_cleanup();
        ntp::tick();
        igmp::tick();
        mld::tick();
        lldp::tick();
        mdns::tick();
        syslog::tick();
        telnet::tick();
        ssh::tick();
        tftp::tick();
        nat::tick();
        httpd::tick();
        httpd::tick_tls();
        dhcpd::tick_expire();
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

/// Send an Ethernet frame from a specific network namespace.
///
/// For the root namespace — or any namespace that has no veth endpoint
/// (the legacy shared-NIC namespace model) — this is identical to
/// [`send_frame`]: the frame egresses the physical NIC.
///
/// For a container namespace that owns a veth endpoint, the frame is
/// injected into that veth instead of the physical NIC.  `veth::send`
/// enqueues on the *peer* endpoint's RX queue, so sending from the
/// container-side endpoint lands the frame on the host-side endpoint,
/// which is a bridge port — `bridge::forward_all` then switches it to the
/// destination peer (same-network delivery) or floods it to the host
/// stack (gateway / external NAT).  This is what gives a container-bound
/// socket a working *reply* path on a user-defined network.
///
/// # Errors
///
/// - Propagates [`veth::send`] / [`send_frame`] errors.
pub fn send_frame_ns(ns_id: crate::netns::NetNsId, frame: &[u8]) -> KernelResult<()> {
    if ns_id != crate::netns::ROOT_NS {
        if let Some((pair, end)) = veth::find_endpoint_for_ns(ns_id) {
            pcap::capture_tx(frame);
            veth::send(pair, end, frame.to_vec())?;
            interface::record_tx(frame.len());
            return Ok(());
        }
    }
    send_frame(frame)
}

/// Send an Ethernet frame via the active NIC.
///
/// Used by the IPv4 layer and ARP to transmit packets.
/// Tries virtio-net first, falls back to e1000, then rtl8139.
pub fn send_frame(frame: &[u8]) -> KernelResult<()> {
    pcap::capture_tx(frame);

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

/// Self-test: verify network interface status and run per-module tests.
///
/// Called from main.rs during boot.  Exercises protocol parsing/building
/// for all core network modules without requiring network hardware.
pub fn self_test() -> KernelResult<()> {
    crate::serial_println!("[net] Running network self-test...");

    // Interface status.
    if interface::is_up() {
        let info = interface::info();
        crate::serial_println!("[net]   Interface: up");
        crate::serial_println!("[net]   MAC: {}", info.mac);
        crate::serial_println!("[net]   IPv4: {}", info.ip);
        crate::serial_println!("[net]   Mask: {}", info.subnet_mask);
        crate::serial_println!("[net]   Gateway: {}", info.gateway);
        crate::serial_println!("[net]   DNS: {}", info.dns);
        // IPv6 link-local address derived from MAC (RFC 4291 Appendix A).
        let ll = ipv6::Ipv6Addr::from_mac_link_local(&info.mac);
        crate::serial_println!("[net]   IPv6 LL: {}", ll);
        // SLAAC global addresses from Router Advertisements.
        let (slaac_addrs, slaac_count) = icmpv6::slaac_addresses();
        if slaac_count > 0 {
            for i in 0..slaac_count {
                if let Some((addr, prefix_len)) = slaac_addrs.get(i) {
                    crate::serial_println!("[net]   IPv6 global: {}/{}", addr, prefix_len);
                }
            }
        }
        if icmpv6::ra_received() {
            if let Some(rdnss) = icmpv6::slaac_rdnss() {
                crate::serial_println!("[net]   IPv6 DNS (RA): {}", rdnss);
            }
        }
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
    let aaaa_count = dns::aaaa_cache_count();
    crate::serial_println!(
        "[net]   DNS cache: {}/{} A entries, {} AAAA entries, {} hits, {} misses, {} evictions",
        dns_stats.entries, dns_stats.capacity, aaaa_count,
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

    // NDP neighbor cache.
    let ndp_count = icmpv6::neighbor_cache_count();
    crate::serial_println!("[net]   NDP neighbor cache: {} entries", ndp_count);

    // UDP socket summary.
    let (udp_socks, udp_count) = udp::all_sockets();
    crate::serial_println!("[net]   UDP: {} active sockets", udp_count);
    for i in 0..udp_count {
        if let Some(sock) = udp_socks.get(i) {
            crate::serial_println!(
                "[net]     port {} (rx_v4={}, rx_v6={}, mcast_groups={})",
                sock.local_port, sock.rx_queue_len, sock.rx_queue_v6_len,
                sock.mcast_groups,
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

    // --- Per-module unit tests ---
    //
    // These exercise protocol parsing and data structure correctness
    // without requiring network hardware.

    ethernet::self_test()?;
    ipv4::self_test()?;
    ipv6::self_test()?;
    icmp::self_test()?;
    icmpv6::self_test()?;
    arp::self_test()?;
    udp::self_test()?;
    dns::self_test()?;
    dhcp::self_test()?;
    frag::self_test()?;
    interface::self_test()?;
    tls::self_test()?;
    // Note: veth::self_test() runs separately after veth::init() in
    // main.rs because veth init requires the heap + netns subsystem,
    // which initializes later in the boot sequence.

    httpd::self_test()?;
    websocket::self_test()?;
    dhcpd::self_test()?;
    dashboard::self_test()?;
    socket::self_test()?;

    crate::serial_println!("[net] Network self-test PASSED");
    Ok(())
}
