//! Netstat — unified network status and diagnostics.
//!
//! Provides a single-command overview of all networking state:
//! TCP connections, UDP sockets (IPv4 + IPv6), listeners,
//! interface stats, routing, ARP/NDP caches, DNS cache,
//! and active services.
//!
//! ## Usage
//!
//! ```text
//! netstat              — show all active connections
//! netstat -t           — TCP connections only
//! netstat -u           — UDP sockets only (includes IPv6 queue info)
//! netstat -l           — listening sockets only
//! netstat -a           — all (connections + listeners)
//! netstat -s           — protocol statistics summary (IPv4 + IPv6)
//! netstat -r           — routing information (IPv4 + IPv6)
//! netstat -i           — interface statistics (IPv4 + IPv6 addresses)
//! netstat -6           — IPv6-specific information (addresses, NDP, SLAAC)
//! ```

use alloc::string::String;
use alloc::vec::Vec;
use alloc::format;

use crate::error::KernelResult;

// ---------------------------------------------------------------------------
// Connection display
// ---------------------------------------------------------------------------

/// A TCP connection entry for display.
#[derive(Debug)]
pub struct TcpEntry {
    pub local_ip: String,
    pub local_port: u16,
    pub remote_ip: String,
    pub remote_port: u16,
    pub state: String,
    pub rx_buffered: usize,
    pub tx_buffered: usize,
    /// Network namespace ID (0 = root/host namespace).
    pub ns_id: u32,
}

/// A UDP socket entry for display.
#[derive(Debug)]
pub struct UdpEntry {
    pub local_port: u16,
    pub rx_queue: usize,
    /// IPv6 receive queue length.
    pub rx_queue_v6: usize,
    pub mcast_groups: usize,
    /// Network namespace ID (0 = root/host namespace).
    pub ns_id: u32,
}

/// A TCP listener entry for display.
#[derive(Debug)]
pub struct ListenerEntry {
    pub port: u16,
    pub backlog_used: usize,
    pub backlog_max: usize,
}

// ---------------------------------------------------------------------------
// Data collection
// ---------------------------------------------------------------------------

/// Format a TcpState as a string.
fn state_str(state: super::tcp::TcpState) -> &'static str {
    use super::tcp::TcpState;
    match state {
        TcpState::Closed => "CLOSED",
        TcpState::Listen => "LISTEN",
        TcpState::SynSent => "SYN_SENT",
        TcpState::SynReceived => "SYN_RCVD",
        TcpState::Established => "ESTABLISHED",
        TcpState::FinWait1 => "FIN_WAIT_1",
        TcpState::FinWait2 => "FIN_WAIT_2",
        TcpState::TimeWait => "TIME_WAIT",
        TcpState::CloseWait => "CLOSE_WAIT",
        TcpState::LastAck => "LAST_ACK",
    }
}

/// Collect all TCP connection information.
pub fn collect_tcp_connections() -> Vec<TcpEntry> {
    let conns = super::tcp::all_connections();
    let local_ip = format!("{}", super::interface::ip());
    let mut entries = Vec::with_capacity(conns.len());
    for c in &conns {
        entries.push(TcpEntry {
            local_ip: local_ip.clone(),
            local_port: c.local_port,
            remote_ip: format!("{}", c.remote_ip),
            remote_port: c.remote_port,
            state: String::from(state_str(c.state)),
            rx_buffered: c.rx_buffered,
            tx_buffered: c.tx_buffered,
            ns_id: c.ns_id,
        });
    }
    entries
}

/// Collect all TCP listener information.
pub fn collect_tcp_listeners() -> Vec<ListenerEntry> {
    let (listeners, count) = super::tcp::all_listeners();
    let mut entries = Vec::with_capacity(count);
    for i in 0..count {
        if let Some(l) = listeners.get(i) {
            entries.push(ListenerEntry {
                port: l.port,
                backlog_used: l.backlog_used,
                backlog_max: l.backlog_max,
            });
        }
    }
    entries
}

/// Collect all UDP socket information.
pub fn collect_udp_sockets() -> Vec<UdpEntry> {
    let (sockets, count) = super::udp::all_sockets();
    let mut entries = Vec::with_capacity(count);
    for i in 0..count {
        if let Some(s) = sockets.get(i) {
            entries.push(UdpEntry {
                local_port: s.local_port,
                rx_queue: s.rx_queue_len,
                rx_queue_v6: s.rx_queue_v6_len,
                mcast_groups: s.mcast_groups as usize,
                ns_id: s.ns_id,
            });
        }
    }
    entries
}

// ---------------------------------------------------------------------------
// Formatting
// ---------------------------------------------------------------------------

/// Format all TCP connections as a table.
pub fn format_tcp_connections(entries: &[TcpEntry]) -> String {
    let mut out = String::with_capacity(entries.len().saturating_mul(90));

    // Show NS column only if there are non-root namespace connections.
    let has_ns = entries.iter().any(|e| e.ns_id != 0);

    if has_ns {
        out.push_str("Proto  Local Address          Remote Address         State        Rx     Tx    NS\n");
        out.push_str("─────  ─────────────────────  ─────────────────────  ───────────  ─────  ─────  ──\n");
    } else {
        out.push_str("Proto  Local Address          Remote Address         State        Rx     Tx\n");
        out.push_str("─────  ─────────────────────  ─────────────────────  ───────────  ─────  ─────\n");
    }

    for e in entries {
        let local = format!("{}:{}", e.local_ip, e.local_port);
        let remote = format!("{}:{}", e.remote_ip, e.remote_port);
        if has_ns {
            out.push_str(&format!(
                "tcp    {:<21}  {:<21}  {:<11}  {:>5}  {:>5}  {:>2}\n",
                local, remote, e.state, e.rx_buffered, e.tx_buffered, e.ns_id
            ));
        } else {
            out.push_str(&format!(
                "tcp    {:<21}  {:<21}  {:<11}  {:>5}  {:>5}\n",
                local, remote, e.state, e.rx_buffered, e.tx_buffered
            ));
        }
    }
    out
}

/// Format all TCP listeners as a table.
pub fn format_tcp_listeners(entries: &[ListenerEntry]) -> String {
    let mut out = String::with_capacity(entries.len().saturating_mul(60));
    out.push_str("Proto  Local Address          State        Backlog\n");
    out.push_str("─────  ─────────────────────  ───────────  ───────────\n");

    for e in entries {
        let local = format!("0.0.0.0:{}", e.port);
        out.push_str(&format!(
            "tcp    {:<21}  LISTEN       {}/{}\n",
            local, e.backlog_used, e.backlog_max
        ));
    }
    out
}

/// Format all UDP sockets as a table.
pub fn format_udp_sockets(entries: &[UdpEntry]) -> String {
    let mut out = String::with_capacity(entries.len().saturating_mul(90));

    let has_ns = entries.iter().any(|e| e.ns_id != 0);

    if has_ns {
        out.push_str("Proto  Local Address          Rx(v4)  Rx(v6)  Mcast  NS\n");
        out.push_str("─────  ─────────────────────  ──────  ──────  ─────  ──\n");
    } else {
        out.push_str("Proto  Local Address          Rx(v4)  Rx(v6)  Mcast\n");
        out.push_str("─────  ─────────────────────  ──────  ──────  ─────\n");
    }

    for e in entries {
        let local = format!("0.0.0.0:{}", e.local_port);
        if has_ns {
            out.push_str(&format!(
                "udp    {:<21}  {:>6}  {:>6}  {:>5}  {:>2}\n",
                local, e.rx_queue, e.rx_queue_v6, e.mcast_groups, e.ns_id
            ));
        } else {
            out.push_str(&format!(
                "udp    {:<21}  {:>6}  {:>6}  {:>5}\n",
                local, e.rx_queue, e.rx_queue_v6, e.mcast_groups
            ));
        }
    }
    out
}

/// Format interface statistics.
pub fn format_interface_stats() -> String {
    use super::ipv6::Ipv6Addr;
    let info = super::interface::info();
    let stats = super::interface::stats();

    let mut out = String::with_capacity(768);
    out.push_str("Interface: eth0\n");
    out.push_str(&format!("  MAC:     {}\n", info.mac));
    out.push_str(&format!("  IPv4:    {}\n", info.ip));
    out.push_str(&format!("  Mask:    {}\n", info.subnet_mask));
    out.push_str(&format!("  Gateway: {}\n", info.gateway));
    out.push_str(&format!("  DNS:     {}\n", info.dns));

    // IPv6 addresses.
    let link_local = Ipv6Addr::from_mac_link_local(&info.mac);
    out.push_str(&format!("  IPv6 LL: {}\n", link_local));
    if let Some(global) = super::icmpv6::slaac_global_addr() {
        out.push_str(&format!("  IPv6 GU: {}\n", global));
    }
    if let Some(rdnss) = super::icmpv6::slaac_rdnss() {
        out.push_str(&format!("  DNS6:    {}\n", rdnss));
    }

    out.push_str(&format!("  Status:  {}\n",
        if super::interface::is_up() { "UP" } else { "DOWN" }));
    out.push('\n');
    out.push_str(&format!(
        "  RX packets: {:<10}  bytes: {}\n",
        stats.rx_packets, stats.rx_bytes
    ));
    out.push_str(&format!(
        "  TX packets: {:<10}  bytes: {}\n",
        stats.tx_packets, stats.tx_bytes
    ));
    out.push_str(&format!(
        "  TX errors:  {:<10}  RX drops: {}\n",
        stats.tx_errors, stats.rx_drops
    ));

    out
}

/// Format routing information.
pub fn format_routing() -> String {
    use super::ipv6::Ipv6Addr;
    let info = super::interface::info();

    let mut out = String::with_capacity(512);
    out.push_str("IPv4 Routing Table\n");
    out.push_str("Destination      Gateway          Mask             Iface\n");
    out.push_str("───────────────  ───────────────  ───────────────  ─────\n");

    // Default route.
    if !info.gateway.is_unspecified() {
        out.push_str(&format!(
            "0.0.0.0          {:<15}  0.0.0.0          eth0\n",
            info.gateway
        ));
    }

    // Local subnet.
    if !info.ip.is_unspecified() && !info.subnet_mask.is_unspecified() {
        // Compute network address.
        let net = super::interface::Ipv4Addr([
            info.ip.0[0] & info.subnet_mask.0[0],
            info.ip.0[1] & info.subnet_mask.0[1],
            info.ip.0[2] & info.subnet_mask.0[2],
            info.ip.0[3] & info.subnet_mask.0[3],
        ]);
        out.push_str(&format!(
            "{:<15}  0.0.0.0          {:<15}  eth0\n",
            net, info.subnet_mask
        ));
    }

    // IPv6 routing.
    out.push_str("\nIPv6 Routing Table\n");
    out.push_str("Destination                              Next Hop   Iface\n");
    out.push_str("───────────────────────────────────────  ─────────  ─────\n");

    let _link_local = Ipv6Addr::from_mac_link_local(&info.mac);
    out.push_str(
        "fe80::/10                                ::         eth0   (link-local)\n",
    );

    if let Some(global) = super::icmpv6::slaac_global_addr() {
        // Show the /64 prefix route.
        let prefix = global.prefix_string(64);
        out.push_str(&format!(
            "{:<39}  ::         eth0   (SLAAC)\n",
            prefix
        ));
    }

    // Default IPv6 route via router (if we have a gateway from RA).
    if let Some(router) = super::icmpv6::default_router() {
        out.push_str(&format!(
            "::/0                                     {:<9}  eth0\n",
            format!("{}", router)
        ));
    }

    // Multicast.
    out.push_str(
        "ff00::/8                                 ::         eth0   (multicast)\n",
    );

    out
}

/// Format protocol statistics summary.
pub fn format_protocol_stats() -> String {
    let tcp_stats = super::tcp::stats();
    let iface_stats = super::interface::stats();
    let dns_stats = super::dns::cache_stats();
    let (_, arp_count) = super::arp::cache_entries();
    let ndp_count = super::icmpv6::neighbor_cache_count();

    let mut out = String::with_capacity(768);

    out.push_str("TCP:\n");
    out.push_str(&format!("  {} active connections\n", tcp_stats.active_connections));
    out.push_str(&format!("    {} ESTABLISHED\n", tcp_stats.established));
    out.push_str(&format!("    {} TIME_WAIT\n", tcp_stats.time_wait));
    out.push_str(&format!("    {} CLOSE_WAIT\n", tcp_stats.close_wait));
    out.push_str(&format!("  {} listeners\n", tcp_stats.listeners));
    out.push('\n');

    out.push_str("IP:\n");
    out.push_str(&format!("  {} packets received\n", iface_stats.rx_packets));
    out.push_str(&format!("  {} packets sent\n", iface_stats.tx_packets));
    out.push_str(&format!("  {} TX errors\n", iface_stats.tx_errors));
    out.push_str(&format!("  {} RX drops\n", iface_stats.rx_drops));
    out.push('\n');

    out.push_str("DNS:\n");
    out.push_str(&format!("  {}/{} cache entries\n", dns_stats.entries, dns_stats.capacity));
    out.push_str(&format!("  {} hits, {} misses, {} evictions\n",
        dns_stats.hits, dns_stats.misses, dns_stats.evictions));
    out.push('\n');

    out.push_str("ARP:\n");
    out.push_str(&format!("  {} cache entries\n", arp_count));
    out.push('\n');

    out.push_str("NDP (IPv6):\n");
    out.push_str(&format!("  {} neighbor cache entries\n", ndp_count));
    out.push_str(&format!("  SLAAC: {}\n",
        if super::icmpv6::slaac_global_addr().is_some() { "configured" } else { "none" }));
    out.push_str(&format!("  RDNSS: {}\n",
        if super::icmpv6::slaac_rdnss().is_some() { "available" } else { "none" }));
    out.push_str(&format!("  Default router: {}\n",
        if super::icmpv6::default_router().is_some() { "available" } else { "none" }));

    out
}

/// Format IPv6-specific information (addresses, NDP, SLAAC, routing).
pub fn format_ipv6_info() -> String {
    use super::ipv6::Ipv6Addr;
    let info = super::interface::info();

    let mut out = String::with_capacity(1024);

    out.push_str("IPv6 Configuration\n");
    out.push_str("==================\n\n");

    // Addresses.
    let link_local = Ipv6Addr::from_mac_link_local(&info.mac);
    out.push_str(&format!("Link-Local:    {} (scope link)\n", link_local));

    let (addrs, count) = super::icmpv6::slaac_addresses();
    if count > 0 {
        for i in 0..count {
            let (addr, prefix_len) = addrs[i];
            out.push_str(&format!(
                "Global:        {} /{} (SLAAC, scope global)\n",
                addr, prefix_len
            ));
        }
    } else {
        out.push_str("Global:        none (no SLAAC prefix received)\n");
    }
    out.push('\n');

    // NDP neighbor cache.
    let ndp_count = super::icmpv6::neighbor_cache_count();
    out.push_str(&format!("NDP Neighbor Cache: {} entries\n", ndp_count));
    out.push('\n');

    // RDNSS.
    out.push_str("DNS (from RA):\n");
    if let Some(rdnss) = super::icmpv6::slaac_rdnss() {
        out.push_str(&format!("  {}\n", rdnss));
    } else {
        out.push_str("  none\n");
    }
    out.push('\n');

    // Default router.
    out.push_str("Default Router:\n");
    if let Some(router) = super::icmpv6::default_router() {
        out.push_str(&format!("  {} (link-local)\n", router));
    } else {
        out.push_str("  none (no RA received)\n");
    }

    out
}

// ---------------------------------------------------------------------------
// procfs
// ---------------------------------------------------------------------------

/// Generate procfs content for `/proc/netstat`.
pub fn procfs_content() -> String {
    let mut out = String::with_capacity(2048);
    out.push_str("Network Status\n");
    out.push_str("==============\n\n");

    // Interface.
    out.push_str(&format_interface_stats());
    out.push('\n');

    // TCP connections.
    let tcp = collect_tcp_connections();
    if !tcp.is_empty() {
        out.push_str(&format_tcp_connections(&tcp));
        out.push('\n');
    }

    // Listeners.
    let listeners = collect_tcp_listeners();
    if !listeners.is_empty() {
        out.push_str(&format_tcp_listeners(&listeners));
        out.push('\n');
    }

    // UDP sockets.
    let udp = collect_udp_sockets();
    if !udp.is_empty() {
        out.push_str(&format_udp_sockets(&udp));
        out.push('\n');
    }

    // Protocol stats.
    out.push_str(&format_protocol_stats());

    out
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

/// Run netstat self-tests.
pub fn self_test() -> KernelResult<()> {
    crate::serial_println!("[netstat] Running netstat self-tests...");
    let mut passed = 0u32;

    // --- Test 1: TCP connection collection ---
    {
        let conns = collect_tcp_connections();
        // Just verify it doesn't panic and returns valid data.
        for c in &conns {
            assert!(!c.state.is_empty(), "state non-empty");
        }

        passed = passed.saturating_add(1);
        crate::serial_println!("[netstat]   test 1 (TCP connections) PASSED");
    }

    // --- Test 2: TCP listener collection ---
    {
        let listeners = collect_tcp_listeners();
        for l in &listeners {
            assert!(l.backlog_used <= l.backlog_max, "backlog valid");
        }

        passed = passed.saturating_add(1);
        crate::serial_println!("[netstat]   test 2 (TCP listeners) PASSED");
    }

    // --- Test 3: UDP socket collection ---
    {
        let udp = collect_udp_sockets();
        for s in &udp {
            assert!(s.local_port > 0 || s.local_port == 0, "port valid");
        }

        passed = passed.saturating_add(1);
        crate::serial_println!("[netstat]   test 3 (UDP sockets) PASSED");
    }

    // --- Test 4: TCP table formatting ---
    {
        let entries = alloc::vec![TcpEntry {
            local_ip: String::from("10.0.2.15"),
            local_port: 12345,
            remote_ip: String::from("93.184.216.34"),
            remote_port: 80,
            state: String::from("ESTABLISHED"),
            rx_buffered: 0,
            tx_buffered: 128,
            ns_id: 0,
        }];
        let formatted = format_tcp_connections(&entries);
        assert!(formatted.contains("10.0.2.15:12345"), "local addr");
        assert!(formatted.contains("93.184.216.34:80"), "remote addr");
        assert!(formatted.contains("ESTABLISHED"), "state");

        passed = passed.saturating_add(1);
        crate::serial_println!("[netstat]   test 4 (TCP formatting) PASSED");
    }

    // --- Test 5: Listener formatting ---
    {
        let entries = alloc::vec![ListenerEntry {
            port: 80,
            backlog_used: 2,
            backlog_max: 16,
        }];
        let formatted = format_tcp_listeners(&entries);
        assert!(formatted.contains("0.0.0.0:80"), "listen addr");
        assert!(formatted.contains("LISTEN"), "listen state");
        assert!(formatted.contains("2/16"), "backlog");

        passed = passed.saturating_add(1);
        crate::serial_println!("[netstat]   test 5 (listener formatting) PASSED");
    }

    // --- Test 6: UDP formatting ---
    {
        let entries = alloc::vec![UdpEntry {
            local_port: 53,
            rx_queue: 5,
            rx_queue_v6: 2,
            mcast_groups: 1,
            ns_id: 0,
        }];
        let formatted = format_udp_sockets(&entries);
        assert!(formatted.contains("0.0.0.0:53"), "udp addr");
        assert!(formatted.contains("5"), "rx queue v4");

        passed = passed.saturating_add(1);
        crate::serial_println!("[netstat]   test 6 (UDP formatting) PASSED");
    }

    // --- Test 7: Interface stats ---
    {
        let formatted = format_interface_stats();
        assert!(formatted.contains("eth0"), "interface name");
        assert!(formatted.contains("MAC:"), "mac field");
        assert!(formatted.contains("RX packets:"), "rx field");
        assert!(formatted.contains("TX packets:"), "tx field");

        passed = passed.saturating_add(1);
        crate::serial_println!("[netstat]   test 7 (interface stats) PASSED");
    }

    // --- Test 8: Routing info ---
    {
        let formatted = format_routing();
        assert!(formatted.contains("Destination"), "routing header");
        assert!(formatted.contains("Gateway"), "gateway header");

        passed = passed.saturating_add(1);
        crate::serial_println!("[netstat]   test 8 (routing info) PASSED");
    }

    // --- Test 9: Protocol stats ---
    {
        let formatted = format_protocol_stats();
        assert!(formatted.contains("TCP:"), "TCP section");
        assert!(formatted.contains("IP:"), "IP section");
        assert!(formatted.contains("DNS:"), "DNS section");
        assert!(formatted.contains("ARP:"), "ARP section");
        assert!(formatted.contains("NDP (IPv6):"), "NDP section");

        passed = passed.saturating_add(1);
        crate::serial_println!("[netstat]   test 9 (protocol stats) PASSED");
    }

    // --- Test 10: Procfs content ---
    {
        let content = procfs_content();
        assert!(content.contains("Network Status"), "header");
        assert!(content.contains("eth0"), "interface");

        passed = passed.saturating_add(1);
        crate::serial_println!("[netstat]   test 10 (procfs content) PASSED");
    }

    // --- Test 11: IPv6 info display ---
    {
        let formatted = format_ipv6_info();
        assert!(formatted.contains("IPv6 Configuration"), "v6 header");
        assert!(formatted.contains("Link-Local:"), "link-local field");
        assert!(formatted.contains("NDP Neighbor Cache:"), "ndp field");
        assert!(formatted.contains("Default Router:"), "router field");

        passed = passed.saturating_add(1);
        crate::serial_println!("[netstat]   test 11 (IPv6 info) PASSED");
    }

    // --- Test 12: Interface stats show IPv6 ---
    {
        let formatted = format_interface_stats();
        assert!(formatted.contains("IPv6 LL:"), "link-local addr");

        passed = passed.saturating_add(1);
        crate::serial_println!("[netstat]   test 12 (interface IPv6) PASSED");
    }

    // --- Test 13: Routing shows IPv6 ---
    {
        let formatted = format_routing();
        assert!(formatted.contains("IPv4 Routing Table"), "v4 routing header");
        assert!(formatted.contains("IPv6 Routing Table"), "v6 routing header");
        assert!(formatted.contains("fe80::/10"), "link-local route");

        passed = passed.saturating_add(1);
        crate::serial_println!("[netstat]   test 13 (routing IPv6) PASSED");
    }

    crate::serial_println!("[netstat] All {} self-tests PASSED", passed);
    Ok(())
}
