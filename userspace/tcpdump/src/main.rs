//! OurOS Network Packet Analyzer
//!
//! Capture and analyze network packets. Similar to tcpdump on Linux.
//!
//! # Usage
//!
//! ```text
//! tcpdump                         Capture all packets
//! tcpdump -i eth0                 Capture on specific interface
//! tcpdump -c 10                   Capture N packets then stop
//! tcpdump -n                      Numeric output (no DNS)
//! tcpdump -v                      Verbose output
//! tcpdump -w file.pcap            Write to pcap file
//! tcpdump -r file.pcap            Read from pcap file
//! tcpdump tcp                     Filter by protocol
//! tcpdump host 10.0.2.15          Filter by host
//! tcpdump port 80                 Filter by port
//! tcpdump -X                      Hex dump packets
//! ```

// IPv4Header / TcpHeader / IcmpHeader / ArpHeader unread fields (version,
// flags, fragment_offset, header_checksum, urgent_ptr, code, hw_type,
// proto_type, hw_len, proto_len, target_mac) document the on-wire packet
// layouts the verbose-mode output and pcap export must surface. The stub
// pretty-printer only renders a subset; the full vocabulary is intentionally
// preserved so the future driver-attached implementation can drop in.
#![allow(dead_code)]

use std::env;
use std::fs;
use std::io::{Read, Write};
use std::process;
use std::time::{SystemTime, UNIX_EPOCH};

// ============================================================================
// Syscall interface
// ============================================================================

// Native OurOS monotonic clock (kernel syscall/number.rs); no-arg, returns
// boot-relative nanoseconds in rax.  (Syscall 30 is SYS_IRQ_REGISTER.)
const SYS_CLOCK_MONOTONIC: u64 = 10;

#[cfg(target_arch = "x86_64")]
unsafe fn syscall3(nr: u64, a1: u64, a2: u64, a3: u64) -> i64 {
    let ret: i64;
    unsafe {
        core::arch::asm!(
            "syscall",
            inlateout("rax") nr as i64 => ret,
            in("rdi") a1,
            in("rsi") a2,
            in("rdx") a3,
            lateout("rcx") _,
            lateout("r11") _,
            options(nostack),
        );
    }
    ret
}

fn clock_monotonic_ns() -> u64 {
    let ret = unsafe { syscall3(SYS_CLOCK_MONOTONIC, 0, 0, 0) };
    if ret < 0 { 0 } else { ret as u64 }
}

// ============================================================================
// Packet structures
// ============================================================================

/// Ethernet frame header (14 bytes).
struct EthernetHeader {
    dst_mac: [u8; 6],
    src_mac: [u8; 6],
    ethertype: u16,
}

/// IPv4 header.
struct Ipv4Header {
    version: u8,
    ihl: u8,
    tos: u8,
    total_length: u16,
    identification: u16,
    flags: u8,
    fragment_offset: u16,
    ttl: u8,
    protocol: u8,
    header_checksum: u16,
    src_ip: u32,
    dst_ip: u32,
}

/// TCP header.
struct TcpHeader {
    src_port: u16,
    dst_port: u16,
    seq_num: u32,
    ack_num: u32,
    data_offset: u8,
    flags: u8,
    window: u16,
    checksum: u16,
    urgent_ptr: u16,
}

/// UDP header.
struct UdpHeader {
    src_port: u16,
    dst_port: u16,
    length: u16,
    checksum: u16,
}

/// ICMP header.
struct IcmpHeader {
    icmp_type: u8,
    code: u8,
    checksum: u16,
    id: u16,
    seq: u16,
}

/// ARP header.
struct ArpHeader {
    hw_type: u16,
    proto_type: u16,
    hw_len: u8,
    proto_len: u8,
    operation: u16,
    sender_mac: [u8; 6],
    sender_ip: u32,
    target_mac: [u8; 6],
    target_ip: u32,
}

// Protocol numbers.
const PROTO_ICMP: u8 = 1;
const PROTO_TCP: u8 = 6;
const PROTO_UDP: u8 = 17;

// Ethertypes.
const ETHER_IPV4: u16 = 0x0800;
const ETHER_ARP: u16 = 0x0806;
const ETHER_IPV6: u16 = 0x86DD;

// TCP flags.
const TCP_FIN: u8 = 0x01;
const TCP_SYN: u8 = 0x02;
const TCP_RST: u8 = 0x04;
const TCP_PSH: u8 = 0x08;
const TCP_ACK: u8 = 0x10;
const TCP_URG: u8 = 0x20;

// ============================================================================
// Parsing
// ============================================================================

fn read_u16_be(data: &[u8], offset: usize) -> u16 {
    if offset + 2 > data.len() {
        return 0;
    }
    u16::from_be_bytes([data[offset], data[offset + 1]])
}

fn read_u32_be(data: &[u8], offset: usize) -> u32 {
    if offset + 4 > data.len() {
        return 0;
    }
    u32::from_be_bytes([
        data[offset],
        data[offset + 1],
        data[offset + 2],
        data[offset + 3],
    ])
}

fn parse_ethernet(data: &[u8]) -> Option<EthernetHeader> {
    if data.len() < 14 {
        return None;
    }
    let mut dst_mac = [0u8; 6];
    let mut src_mac = [0u8; 6];
    dst_mac.copy_from_slice(&data[0..6]);
    src_mac.copy_from_slice(&data[6..12]);
    let ethertype = read_u16_be(data, 12);
    Some(EthernetHeader {
        dst_mac,
        src_mac,
        ethertype,
    })
}

fn parse_ipv4(data: &[u8]) -> Option<Ipv4Header> {
    if data.len() < 20 {
        return None;
    }
    let version = (data[0] >> 4) & 0xF;
    let ihl = data[0] & 0xF;
    if version != 4 || ihl < 5 {
        return None;
    }
    Some(Ipv4Header {
        version,
        ihl,
        tos: data[1],
        total_length: read_u16_be(data, 2),
        identification: read_u16_be(data, 4),
        flags: (data[6] >> 5) & 0x7,
        fragment_offset: read_u16_be(data, 6) & 0x1FFF,
        ttl: data[8],
        protocol: data[9],
        header_checksum: read_u16_be(data, 10),
        src_ip: read_u32_be(data, 12),
        dst_ip: read_u32_be(data, 16),
    })
}

fn parse_tcp(data: &[u8]) -> Option<TcpHeader> {
    if data.len() < 20 {
        return None;
    }
    let data_offset = (data[12] >> 4) & 0xF;
    Some(TcpHeader {
        src_port: read_u16_be(data, 0),
        dst_port: read_u16_be(data, 2),
        seq_num: read_u32_be(data, 4),
        ack_num: read_u32_be(data, 8),
        data_offset,
        flags: data[13],
        window: read_u16_be(data, 14),
        checksum: read_u16_be(data, 16),
        urgent_ptr: read_u16_be(data, 18),
    })
}

fn parse_udp(data: &[u8]) -> Option<UdpHeader> {
    if data.len() < 8 {
        return None;
    }
    Some(UdpHeader {
        src_port: read_u16_be(data, 0),
        dst_port: read_u16_be(data, 2),
        length: read_u16_be(data, 4),
        checksum: read_u16_be(data, 6),
    })
}

fn parse_icmp(data: &[u8]) -> Option<IcmpHeader> {
    if data.len() < 8 {
        return None;
    }
    Some(IcmpHeader {
        icmp_type: data[0],
        code: data[1],
        checksum: read_u16_be(data, 2),
        id: read_u16_be(data, 4),
        seq: read_u16_be(data, 6),
    })
}

fn parse_arp(data: &[u8]) -> Option<ArpHeader> {
    if data.len() < 28 {
        return None;
    }
    let mut sender_mac = [0u8; 6];
    let mut target_mac = [0u8; 6];
    sender_mac.copy_from_slice(&data[8..14]);
    target_mac.copy_from_slice(&data[18..24]);
    Some(ArpHeader {
        hw_type: read_u16_be(data, 0),
        proto_type: read_u16_be(data, 2),
        hw_len: data[4],
        proto_len: data[5],
        operation: read_u16_be(data, 6),
        sender_mac,
        sender_ip: read_u32_be(data, 14),
        target_mac,
        target_ip: read_u32_be(data, 24),
    })
}

// ============================================================================
// Formatting helpers
// ============================================================================

fn format_mac(mac: &[u8; 6]) -> String {
    format!(
        "{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
        mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]
    )
}

fn format_ip(ip: u32) -> String {
    let b = ip.to_be_bytes();
    format!("{}.{}.{}.{}", b[0], b[1], b[2], b[3])
}

fn tcp_flags_string(flags: u8) -> String {
    // Flag ordering matches real tcpdump's `tcp_flag_values` table:
    // F (FIN), S (SYN), R (RST), P (PSH), . (ACK), U (URG).
    let mut s = String::from("[");
    if flags & TCP_FIN != 0 {
        s.push('F');
    }
    if flags & TCP_SYN != 0 {
        s.push('S');
    }
    if flags & TCP_RST != 0 {
        s.push('R');
    }
    if flags & TCP_PSH != 0 {
        s.push('P');
    }
    if flags & TCP_ACK != 0 {
        s.push('.');
    }
    if flags & TCP_URG != 0 {
        s.push('U');
    }
    s.push(']');
    s
}

fn icmp_type_name(icmp_type: u8) -> &'static str {
    match icmp_type {
        0 => "echo reply",
        3 => "destination unreachable",
        4 => "source quench",
        5 => "redirect",
        8 => "echo request",
        11 => "time exceeded",
        12 => "parameter problem",
        13 => "timestamp request",
        14 => "timestamp reply",
        _ => "unknown",
    }
}

fn format_timestamp(ns: u64, mode: TimestampMode) -> String {
    match mode {
        TimestampMode::None => String::new(),
        TimestampMode::Epoch => format!("{}.{:06} ", ns / 1_000_000_000, (ns / 1000) % 1_000_000),
        TimestampMode::Default => {
            // Use wall clock time.
            let epoch_secs = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            let secs_today = epoch_secs % 86400;
            let hours = secs_today / 3600;
            let minutes = (secs_today % 3600) / 60;
            let seconds = secs_today % 60;
            let micros = (ns / 1000) % 1_000_000;
            format!("{:02}:{:02}:{:02}.{:06} ", hours, minutes, seconds, micros)
        }
        TimestampMode::Delta => {
            // Delta from previous — handled externally.
            String::new()
        }
    }
}

// ============================================================================
// Hex dump
// ============================================================================

fn hex_dump(data: &[u8], max_bytes: usize) {
    let limit = data.len().min(max_bytes);
    let mut offset = 0;

    while offset < limit {
        print!("\t0x{:04x}:  ", offset);

        // Hex bytes (groups of 2).
        let row_end = (offset + 16).min(limit);
        for i in offset..offset + 16 {
            if let Some(b) = data.get(i).filter(|_| i < row_end) {
                print!("{:02x}", b);
            } else {
                print!("  ");
            }
            if i % 2 == 1 {
                print!(" ");
            }
        }

        print!(" ");

        // ASCII printable.
        for &ch in data.get(offset..row_end).unwrap_or(&[]) {
            if (0x20..=0x7E).contains(&ch) {
                print!("{}", ch as char);
            } else {
                print!(".");
            }
        }
        println!();

        offset += 16;
    }
}

// ============================================================================
// Packet filter
// ============================================================================

#[derive(Default)]
struct Filter {
    protocol: Option<u8>, // PROTO_TCP, PROTO_UDP, PROTO_ICMP
    host: Option<u32>,    // Match src or dst IP
    src_host: Option<u32>,
    dst_host: Option<u32>,
    port: Option<u16>, // Match src or dst port
    src_port: Option<u16>,
    dst_port: Option<u16>,
    arp_only: bool,
}

impl Filter {
    fn matches(
        &self,
        eth: &EthernetHeader,
        ip: Option<&Ipv4Header>,
        sport: u16,
        dport: u16,
    ) -> bool {
        // ARP filter.
        if self.arp_only {
            return eth.ethertype == ETHER_ARP;
        }

        // Protocol filter.
        if let Some(proto) = self.protocol {
            if let Some(ip_hdr) = ip {
                if ip_hdr.protocol != proto {
                    return false;
                }
            } else {
                return false; // Need IP for protocol filter, but no IP header.
            }
        }

        // Host filter.
        if let Some(host) = self.host {
            if let Some(ip_hdr) = ip {
                if ip_hdr.src_ip != host && ip_hdr.dst_ip != host {
                    return false;
                }
            } else {
                return false;
            }
        }

        if let Some(host) = self.src_host {
            if let Some(ip_hdr) = ip {
                if ip_hdr.src_ip != host {
                    return false;
                }
            } else {
                return false;
            }
        }

        if let Some(host) = self.dst_host {
            if let Some(ip_hdr) = ip {
                if ip_hdr.dst_ip != host {
                    return false;
                }
            } else {
                return false;
            }
        }

        // Port filter.
        if let Some(port) = self.port
            && sport != port && dport != port {
                return false;
            }

        if let Some(port) = self.src_port
            && sport != port {
                return false;
            }

        if let Some(port) = self.dst_port
            && dport != port {
                return false;
            }

        true
    }
}

// ============================================================================
// PCAP file format
// ============================================================================

const PCAP_MAGIC: u32 = 0xA1B2C3D4;
const PCAP_VERSION_MAJOR: u16 = 2;
const PCAP_VERSION_MINOR: u16 = 4;
const PCAP_LINKTYPE_ETHERNET: u32 = 1;

struct PcapWriter {
    file: fs::File,
}

impl PcapWriter {
    fn new(path: &str, snaplen: u32) -> Result<Self, String> {
        let mut file =
            fs::File::create(path).map_err(|e| format!("cannot create {}: {}", path, e))?;

        // Write global header.
        let mut hdr = Vec::with_capacity(24);
        hdr.extend_from_slice(&PCAP_MAGIC.to_le_bytes());
        hdr.extend_from_slice(&PCAP_VERSION_MAJOR.to_le_bytes());
        hdr.extend_from_slice(&PCAP_VERSION_MINOR.to_le_bytes());
        hdr.extend_from_slice(&0i32.to_le_bytes()); // thiszone
        hdr.extend_from_slice(&0u32.to_le_bytes()); // sigfigs
        hdr.extend_from_slice(&snaplen.to_le_bytes());
        hdr.extend_from_slice(&PCAP_LINKTYPE_ETHERNET.to_le_bytes());

        file.write_all(&hdr)
            .map_err(|e| format!("write error: {}", e))?;
        Ok(PcapWriter { file })
    }

    fn write_packet(&mut self, ts_sec: u32, ts_usec: u32, data: &[u8]) -> Result<(), String> {
        let incl_len = data.len() as u32;
        let orig_len = incl_len;

        let mut rec = Vec::with_capacity(16 + data.len());
        rec.extend_from_slice(&ts_sec.to_le_bytes());
        rec.extend_from_slice(&ts_usec.to_le_bytes());
        rec.extend_from_slice(&incl_len.to_le_bytes());
        rec.extend_from_slice(&orig_len.to_le_bytes());
        rec.extend_from_slice(data);

        self.file
            .write_all(&rec)
            .map_err(|e| format!("write error: {}", e))
    }
}

struct PcapReader {
    data: Vec<u8>,
    offset: usize,
    #[allow(dead_code)]
    snaplen: u32,
}

impl PcapReader {
    fn open(path: &str) -> Result<Self, String> {
        let data = fs::read(path).map_err(|e| format!("cannot read {}: {}", path, e))?;
        if data.len() < 24 {
            return Err("not a pcap file (too short)".to_string());
        }

        let magic = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
        if magic != PCAP_MAGIC {
            return Err(format!("not a pcap file (bad magic: 0x{:08x})", magic));
        }

        let snaplen = u32::from_le_bytes([data[16], data[17], data[18], data[19]]);
        let linktype = u32::from_le_bytes([data[20], data[21], data[22], data[23]]);
        if linktype != PCAP_LINKTYPE_ETHERNET {
            return Err(format!("unsupported link type: {}", linktype));
        }

        Ok(PcapReader {
            data,
            offset: 24,
            snaplen,
        })
    }

    fn next_packet(&mut self) -> Option<(u32, u32, &[u8])> {
        if self.offset + 16 > self.data.len() {
            return None;
        }

        let ts_sec = u32::from_le_bytes([
            self.data[self.offset],
            self.data[self.offset + 1],
            self.data[self.offset + 2],
            self.data[self.offset + 3],
        ]);
        let ts_usec = u32::from_le_bytes([
            self.data[self.offset + 4],
            self.data[self.offset + 5],
            self.data[self.offset + 6],
            self.data[self.offset + 7],
        ]);
        let incl_len = u32::from_le_bytes([
            self.data[self.offset + 8],
            self.data[self.offset + 9],
            self.data[self.offset + 10],
            self.data[self.offset + 11],
        ]) as usize;

        self.offset += 16;

        if self.offset + incl_len > self.data.len() {
            return None;
        }

        let pkt_data = &self.data[self.offset..self.offset + incl_len];
        self.offset += incl_len;

        Some((ts_sec, ts_usec, pkt_data))
    }
}

// ============================================================================
// Packet display
// ============================================================================

#[derive(Clone, Copy, PartialEq)]
enum TimestampMode {
    Default,
    None,
    Epoch,
    Delta,
}

struct DisplayOpts {
    verbose: u8, // 0, 1, 2, 3
    numeric: bool,
    hex_dump: bool,
    timestamp: TimestampMode,
    snaplen: u32,
}

fn display_packet(data: &[u8], opts: &DisplayOpts, ts_ns: u64, prev_ts_ns: u64) {
    let ts_str = if opts.timestamp == TimestampMode::Delta {
        let delta = ts_ns.saturating_sub(prev_ts_ns);
        format!(
            "{}.{:06} ",
            delta / 1_000_000_000,
            (delta / 1000) % 1_000_000
        )
    } else {
        format_timestamp(ts_ns, opts.timestamp)
    };

    let eth = match parse_ethernet(data) {
        Some(e) => e,
        None => {
            println!("{}[truncated ethernet frame, {} bytes]", ts_str, data.len());
            return;
        }
    };

    let payload = &data[14..];

    match eth.ethertype {
        ETHER_IPV4 => {
            let ip = match parse_ipv4(payload) {
                Some(ip) => ip,
                None => {
                    println!("{}IP [truncated]", ts_str);
                    return;
                }
            };

            let ip_hdr_len = (ip.ihl as usize) * 4;
            if payload.len() < ip_hdr_len {
                println!("{}IP [truncated header]", ts_str);
                return;
            }
            let transport = &payload[ip_hdr_len..];
            let src = format_ip(ip.src_ip);
            let dst = format_ip(ip.dst_ip);

            match ip.protocol {
                PROTO_TCP => {
                    if let Some(tcp) = parse_tcp(transport) {
                        let flags = tcp_flags_string(tcp.flags);
                        let payload_len = ip.total_length as i32
                            - ip_hdr_len as i32
                            - (tcp.data_offset as i32 * 4);
                        let payload_len = payload_len.max(0) as u32;

                        print!(
                            "{}IP {}.{} > {}.{}: Flags {}, seq {}, ack {}, win {}, length {}",
                            ts_str,
                            src,
                            tcp.src_port,
                            dst,
                            tcp.dst_port,
                            flags,
                            tcp.seq_num,
                            tcp.ack_num,
                            tcp.window,
                            payload_len,
                        );

                        if opts.verbose >= 1 {
                            print!(", cksum 0x{:04x}", tcp.checksum);
                        }
                        if opts.verbose >= 2 {
                            print!(
                                ", id {}, ttl {}, tos 0x{:02x}",
                                ip.identification, ip.ttl, ip.tos
                            );
                        }
                        println!();
                    } else {
                        println!("{}IP {} > {}: TCP [truncated]", ts_str, src, dst);
                    }
                }
                PROTO_UDP => {
                    if let Some(udp) = parse_udp(transport) {
                        print!(
                            "{}IP {}.{} > {}.{}: UDP, length {}",
                            ts_str, src, udp.src_port, dst, udp.dst_port, udp.length,
                        );
                        if opts.verbose >= 1 {
                            print!(", cksum 0x{:04x}", udp.checksum);
                        }
                        println!();
                    } else {
                        println!("{}IP {} > {}: UDP [truncated]", ts_str, src, dst);
                    }
                }
                PROTO_ICMP => {
                    if let Some(icmp) = parse_icmp(transport) {
                        print!(
                            "{}IP {} > {}: ICMP {}, id {}, seq {}, length {}",
                            ts_str,
                            src,
                            dst,
                            icmp_type_name(icmp.icmp_type),
                            icmp.id,
                            icmp.seq,
                            ip.total_length as usize - ip_hdr_len,
                        );
                        if opts.verbose >= 1 {
                            print!(", cksum 0x{:04x}", icmp.checksum);
                        }
                        println!();
                    } else {
                        println!("{}IP {} > {}: ICMP [truncated]", ts_str, src, dst);
                    }
                }
                other => {
                    println!(
                        "{}IP {} > {}: protocol {}, length {}",
                        ts_str, src, dst, other, ip.total_length,
                    );
                }
            }

            if opts.verbose >= 3 {
                println!(
                    "\t{} > {}: ethertype IPv4 (0x0800), length {}",
                    format_mac(&eth.src_mac),
                    format_mac(&eth.dst_mac),
                    data.len(),
                );
            }
        }
        ETHER_ARP => {
            if let Some(arp) = parse_arp(payload) {
                let op = match arp.operation {
                    1 => "Request",
                    2 => "Reply",
                    _ => "?",
                };
                if arp.operation == 1 {
                    println!(
                        "{}ARP, {} who-has {} tell {}, length {}",
                        ts_str,
                        op,
                        format_ip(arp.target_ip),
                        format_ip(arp.sender_ip),
                        payload.len(),
                    );
                } else {
                    println!(
                        "{}ARP, {} {} is-at {}, length {}",
                        ts_str,
                        op,
                        format_ip(arp.sender_ip),
                        format_mac(&arp.sender_mac),
                        payload.len(),
                    );
                }
            } else {
                println!("{}ARP [truncated]", ts_str);
            }
        }
        ETHER_IPV6 => {
            println!(
                "{}IP6 {} > {} [IPv6 not fully parsed]",
                ts_str,
                format_mac(&eth.src_mac),
                format_mac(&eth.dst_mac)
            );
        }
        other => {
            println!(
                "{}ethertype 0x{:04x}, {} > {}, length {}",
                ts_str,
                other,
                format_mac(&eth.src_mac),
                format_mac(&eth.dst_mac),
                data.len(),
            );
        }
    }

    if opts.hex_dump {
        let limit = data.len().min(opts.snaplen as usize);
        hex_dump(data, limit);
    }
}

// ============================================================================
// Live capture
// ============================================================================

fn capture_live(
    iface: &str,
    count: Option<u32>,
    filter: &Filter,
    opts: &DisplayOpts,
    write_path: Option<&str>,
) {
    // Try reading packets from /proc/net/capture or /dev/netraw.
    let capture_path = format!("/proc/net/capture/{}", iface);
    let alt_path = "/proc/net/capture";
    let dev_path = "/dev/netraw";

    let mut source_path = None;
    for path in &[capture_path.as_str(), alt_path, dev_path] {
        if fs::metadata(path).is_ok() {
            source_path = Some(*path);
            break;
        }
    }

    let source = match source_path {
        Some(p) => p,
        None => {
            // Fall back to reading /proc/net/packets (text-based packet log).
            if fs::metadata("/proc/net/packets").is_ok() {
                capture_from_proc_net(count, filter, opts, write_path);
                return;
            }
            eprintln!(
                "tcpdump: cannot open capture interface (no /proc/net/capture or /dev/netraw)"
            );
            eprintln!("hint: packet capture may not be enabled in the kernel");
            process::exit(1);
        }
    };

    let mut pcap_writer = write_path.map(|p| {
        PcapWriter::new(p, opts.snaplen).unwrap_or_else(|e| {
            eprintln!("tcpdump: {}", e);
            process::exit(1);
        })
    });

    if !opts.numeric {
        eprintln!(
            "tcpdump: listening on {}, link-type EN10MB (Ethernet), snapshot length {} bytes",
            iface, opts.snaplen
        );
    }

    let mut captured = 0u32;
    let mut prev_ts = 0u64;

    // Try to open for binary read.
    let mut file = match fs::File::open(source) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("tcpdump: {}: {}", source, e);
            process::exit(1);
        }
    };

    let mut buf = vec![0u8; opts.snaplen as usize + 4]; // +4 for length prefix

    loop {
        if let Some(max) = count
            && captured >= max {
                break;
            }

        // Read a packet (format: 4-byte LE length prefix + raw frame).
        let n = match file.read(&mut buf) {
            Ok(0) => {
                // EOF — might be a non-blocking source. Try again.
                std::thread::sleep(std::time::Duration::from_millis(10));
                continue;
            }
            Ok(n) => n,
            Err(_) => break,
        };

        if n < 4 {
            continue;
        }

        let pkt_len = u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]) as usize;
        let pkt_data = if pkt_len > 0 && n >= 4 + pkt_len {
            &buf[4..4 + pkt_len]
        } else {
            // No length prefix — treat entire read as raw frame.
            &buf[..n]
        };

        let ts_ns = clock_monotonic_ns();

        // Parse enough to filter.
        let eth = parse_ethernet(pkt_data);
        let (ip_hdr, sport, dport) = if let Some(ref eth) = eth {
            if eth.ethertype == ETHER_IPV4 && pkt_data.len() > 14 {
                let ip = parse_ipv4(&pkt_data[14..]);
                let (sp, dp) = if let Some(ref ip) = ip {
                    let ip_hdr_len = (ip.ihl as usize) * 4;
                    let transport = &pkt_data[14 + ip_hdr_len..];
                    match ip.protocol {
                        PROTO_TCP
                            if transport.len() >= 4 => {
                                (read_u16_be(transport, 0), read_u16_be(transport, 2))
                            }
                        PROTO_UDP
                            if transport.len() >= 4 => {
                                (read_u16_be(transport, 0), read_u16_be(transport, 2))
                            }
                        _ => (0, 0),
                    }
                } else {
                    (0, 0)
                };
                (ip, sp, dp)
            } else {
                (None, 0, 0)
            }
        } else {
            (None, 0, 0)
        };

        if let Some(ref eth_hdr) = eth
            && !filter.matches(eth_hdr, ip_hdr.as_ref(), sport, dport) {
                continue;
            }

        display_packet(pkt_data, opts, ts_ns, prev_ts);

        if let Some(ref mut writer) = pcap_writer {
            let secs = (ts_ns / 1_000_000_000) as u32;
            let usecs = ((ts_ns % 1_000_000_000) / 1000) as u32;
            if let Err(e) = writer.write_packet(secs, usecs, pkt_data) {
                eprintln!("tcpdump: write error: {}", e);
            }
        }

        prev_ts = ts_ns;
        captured += 1;
    }

    eprintln!();
    eprintln!("{} packets captured", captured);
}

/// Fallback: parse text-based packet log from /proc/net/packets.
fn capture_from_proc_net(
    count: Option<u32>,
    filter: &Filter,
    opts: &DisplayOpts,
    _write_path: Option<&str>,
) {
    let content = match fs::read_to_string("/proc/net/packets") {
        Ok(c) => c,
        Err(e) => {
            eprintln!("tcpdump: /proc/net/packets: {}", e);
            process::exit(1);
        }
    };

    let mut displayed = 0u32;

    for line in content.lines() {
        if let Some(max) = count
            && displayed >= max {
                break;
            }

        // Parse text-based packet info.
        // Expected format: "PROTO SRC_IP:PORT > DST_IP:PORT FLAGS LEN TS"
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() < 5 {
            continue;
        }

        let proto = fields[0];
        let _src = fields[1];
        let _dst_raw = if fields.len() > 3 { fields[3] } else { "" };

        // Apply protocol filter.
        if let Some(proto_num) = filter.protocol {
            let matches = match proto_num {
                PROTO_TCP => proto == "TCP",
                PROTO_UDP => proto == "UDP",
                PROTO_ICMP => proto == "ICMP",
                _ => false,
            };
            if !matches {
                continue;
            }
        }

        let ts_str = format_timestamp(clock_monotonic_ns(), opts.timestamp);
        println!("{}{}", ts_str, line);
        displayed += 1;
    }

    eprintln!("{} packets displayed", displayed);
}

// ============================================================================
// Read PCAP file
// ============================================================================

fn read_pcap(path: &str, count: Option<u32>, filter: &Filter, opts: &DisplayOpts) {
    let mut reader = match PcapReader::open(path) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("tcpdump: {}", e);
            process::exit(1);
        }
    };

    let mut displayed = 0u32;
    let mut prev_ts_ns = 0u64;

    while let Some((ts_sec, ts_usec, pkt_data)) = reader.next_packet() {
        if let Some(max) = count
            && displayed >= max {
                break;
            }

        let ts_ns = (ts_sec as u64) * 1_000_000_000 + (ts_usec as u64) * 1000;

        // Parse and filter.
        let eth = parse_ethernet(pkt_data);
        let (ip_hdr, sport, dport) = if let Some(ref eth) = eth {
            if eth.ethertype == ETHER_IPV4 && pkt_data.len() > 14 {
                let ip = parse_ipv4(&pkt_data[14..]);
                let (sp, dp) = if let Some(ref ip) = ip {
                    let ip_hdr_len = (ip.ihl as usize) * 4;
                    if pkt_data.len() > 14 + ip_hdr_len + 4 {
                        let transport = &pkt_data[14 + ip_hdr_len..];
                        match ip.protocol {
                            PROTO_TCP | PROTO_UDP => {
                                (read_u16_be(transport, 0), read_u16_be(transport, 2))
                            }
                            _ => (0, 0),
                        }
                    } else {
                        (0, 0)
                    }
                } else {
                    (0, 0)
                };
                (ip, sp, dp)
            } else {
                (None, 0, 0)
            }
        } else {
            (None, 0, 0)
        };

        if let Some(ref eth_hdr) = eth
            && !filter.matches(eth_hdr, ip_hdr.as_ref(), sport, dport) {
                continue;
            }

        display_packet(pkt_data, opts, ts_ns, prev_ts_ns);
        prev_ts_ns = ts_ns;
        displayed += 1;
    }

    eprintln!();
    eprintln!("reading from file {}, link-type EN10MB (Ethernet)", path);
    eprintln!("{} packets read", displayed);
}

// ============================================================================
// Filter expression parser
// ============================================================================

fn parse_ipv4_addr(s: &str) -> Option<u32> {
    let parts: Vec<&str> = s.split('.').collect();
    if parts.len() != 4 {
        return None;
    }
    let a: u8 = parts[0].parse().ok()?;
    let b: u8 = parts[1].parse().ok()?;
    let c: u8 = parts[2].parse().ok()?;
    let d: u8 = parts[3].parse().ok()?;
    Some(u32::from_be_bytes([a, b, c, d]))
}

fn parse_filter(args: &[String]) -> Filter {
    let mut filter = Filter::default();
    let mut idx = 0;

    while idx < args.len() {
        let arg = args[idx].as_str();
        match arg {
            "tcp" => filter.protocol = Some(PROTO_TCP),
            "udp" => filter.protocol = Some(PROTO_UDP),
            "icmp" => filter.protocol = Some(PROTO_ICMP),
            "arp" => filter.arp_only = true,
            "host" => {
                idx += 1;
                if idx < args.len() {
                    filter.host = parse_ipv4_addr(&args[idx]);
                }
            }
            "src" => {
                idx += 1;
                if idx < args.len() {
                    if args[idx] == "host" {
                        idx += 1;
                        if idx < args.len() {
                            filter.src_host = parse_ipv4_addr(&args[idx]);
                        }
                    } else if args[idx] == "port" {
                        idx += 1;
                        if idx < args.len() {
                            filter.src_port = args[idx].parse().ok();
                        }
                    } else {
                        filter.src_host = parse_ipv4_addr(&args[idx]);
                    }
                }
            }
            "dst" => {
                idx += 1;
                if idx < args.len() {
                    if args[idx] == "host" {
                        idx += 1;
                        if idx < args.len() {
                            filter.dst_host = parse_ipv4_addr(&args[idx]);
                        }
                    } else if args[idx] == "port" {
                        idx += 1;
                        if idx < args.len() {
                            filter.dst_port = args[idx].parse().ok();
                        }
                    } else {
                        filter.dst_host = parse_ipv4_addr(&args[idx]);
                    }
                }
            }
            "port" => {
                idx += 1;
                if idx < args.len() {
                    filter.port = args[idx].parse().ok();
                }
            }
            "and" | "or" | "not" => {
                // Simple conjunction — we just skip boolean operators for now.
            }
            _ => {
                // Might be an IP address or port number.
                if let Some(ip) = parse_ipv4_addr(arg) {
                    if filter.host.is_none() {
                        filter.host = Some(ip);
                    }
                } else if let Ok(port) = arg.parse::<u16>()
                    && filter.port.is_none() {
                        filter.port = Some(port);
                    }
            }
        }
        idx += 1;
    }

    filter
}

// ============================================================================
// CLI
// ============================================================================

fn print_usage() {
    println!("OurOS Network Packet Analyzer v0.1.0");
    println!();
    println!("Capture and analyze network packets.");
    println!();
    println!("USAGE:");
    println!("  tcpdump [options] [filter expression]");
    println!();
    println!("OPTIONS:");
    println!("  -i IFACE       Listen on interface (default: any)");
    println!("  -c COUNT       Stop after N packets");
    println!("  -n             Numeric output (no DNS resolution)");
    println!("  -v/-vv/-vvv    Increase verbosity");
    println!("  -t             No timestamp");
    println!("  -tt            Unix epoch timestamp");
    println!("  -ttt           Delta timestamp");
    println!("  -X             Hex dump packets");
    println!("  -s SNAPLEN     Snapshot length (default: 262144)");
    println!("  -w FILE        Write packets to pcap file");
    println!("  -r FILE        Read packets from pcap file");
    println!("  -h, --help     Show this help");
    println!();
    println!("FILTER EXPRESSIONS:");
    println!("  tcp / udp / icmp / arp    Protocol filter");
    println!("  host IP                   Source or destination IP");
    println!("  src IP / dst IP           Source or destination only");
    println!("  port N                    Source or destination port");
    println!("  src port N / dst port N   Source or destination port only");
    println!();
    println!("EXAMPLES:");
    println!("  tcpdump -i eth0 -n");
    println!("  tcpdump -c 100 tcp port 80");
    println!("  tcpdump -w capture.pcap host 10.0.2.15");
    println!("  tcpdump -r capture.pcap -X");
}

fn main() {
    let args: Vec<String> = env::args().collect();

    let mut iface = "any".to_string();
    let mut count: Option<u32> = None;
    let mut numeric = false;
    let mut verbose: u8 = 0;
    let mut hex_dump = false;
    let mut timestamp = TimestampMode::Default;
    let mut snaplen: u32 = 262144;
    let mut write_path: Option<String> = None;
    let mut read_path: Option<String> = None;
    let mut filter_args: Vec<String> = Vec::new();

    let mut idx = 1;
    while idx < args.len() {
        match args[idx].as_str() {
            "-h" | "--help" | "help" => {
                print_usage();
                return;
            }
            "--version" => {
                println!("tcpdump (OurOS) 0.1.0");
                return;
            }
            "-i" => {
                idx += 1;
                if idx < args.len() {
                    iface = args[idx].clone();
                }
            }
            "-c" => {
                idx += 1;
                if idx < args.len() {
                    count = args[idx].parse().ok();
                }
            }
            "-n" => numeric = true,
            "-v" => verbose = 1,
            "-vv" => verbose = 2,
            "-vvv" => verbose = 3,
            "-t" => timestamp = TimestampMode::None,
            "-tt" => timestamp = TimestampMode::Epoch,
            "-ttt" => timestamp = TimestampMode::Delta,
            "-X" | "-x" | "-xx" => hex_dump = true,
            "-s" => {
                idx += 1;
                if idx < args.len() {
                    snaplen = args[idx].parse().unwrap_or(262144);
                }
            }
            "-w" => {
                idx += 1;
                if idx < args.len() {
                    write_path = Some(args[idx].clone());
                }
            }
            "-r" => {
                idx += 1;
                if idx < args.len() {
                    read_path = Some(args[idx].clone());
                }
            }
            other => {
                filter_args.push(other.to_string());
            }
        }
        idx += 1;
    }

    let filter = parse_filter(&filter_args);
    let opts = DisplayOpts {
        verbose,
        numeric,
        hex_dump,
        timestamp,
        snaplen,
    };

    if let Some(ref path) = read_path {
        read_pcap(path, count, &filter, &opts);
    } else {
        capture_live(&iface, count, &filter, &opts, write_path.as_deref());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_ipv4_addr() {
        assert_eq!(parse_ipv4_addr("10.0.2.15"), Some(0x0A00020F));
        assert_eq!(parse_ipv4_addr("0.0.0.0"), Some(0));
        assert_eq!(parse_ipv4_addr("bad"), None);
    }

    #[test]
    fn test_format_ip() {
        assert_eq!(format_ip(0x0A00020F), "10.0.2.15");
        assert_eq!(format_ip(0xC0A80101), "192.168.1.1");
    }

    #[test]
    fn test_format_mac() {
        assert_eq!(
            format_mac(&[0x52, 0x54, 0x00, 0x12, 0x34, 0x56]),
            "52:54:00:12:34:56"
        );
    }

    #[test]
    fn test_tcp_flags_string() {
        assert_eq!(tcp_flags_string(TCP_SYN), "[S]");
        assert_eq!(tcp_flags_string(TCP_SYN | TCP_ACK), "[S.]");
        assert_eq!(tcp_flags_string(TCP_FIN | TCP_ACK), "[F.]");
        assert_eq!(tcp_flags_string(TCP_RST), "[R]");
        assert_eq!(tcp_flags_string(TCP_ACK | TCP_PSH), "[P.]");
    }

    #[test]
    fn test_parse_ethernet() {
        let mut pkt = vec![0u8; 64];
        // Dst MAC
        pkt[0..6].copy_from_slice(&[0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]);
        // Src MAC
        pkt[6..12].copy_from_slice(&[0x52, 0x54, 0x00, 0x12, 0x34, 0x56]);
        // Ethertype: IPv4
        pkt[12] = 0x08;
        pkt[13] = 0x00;

        let eth = parse_ethernet(&pkt).unwrap();
        assert_eq!(eth.ethertype, ETHER_IPV4);
        assert_eq!(eth.dst_mac, [0xFF; 6]);
    }

    #[test]
    fn test_parse_filter() {
        let args: Vec<String> = vec!["tcp".to_string(), "port".to_string(), "80".to_string()];
        let f = parse_filter(&args);
        assert_eq!(f.protocol, Some(PROTO_TCP));
        assert_eq!(f.port, Some(80));
    }

    #[test]
    fn test_icmp_type_name() {
        assert_eq!(icmp_type_name(8), "echo request");
        assert_eq!(icmp_type_name(0), "echo reply");
        assert_eq!(icmp_type_name(3), "destination unreachable");
    }

    #[test]
    fn test_read_u16_be() {
        let data = [0x08, 0x00, 0x45, 0x00];
        assert_eq!(read_u16_be(&data, 0), 0x0800);
        assert_eq!(read_u16_be(&data, 2), 0x4500);
    }
}
