# F → A — fuzz `netproto` (a harness, ready to drop in) and the ELF loader (its parser in a crate of its own first)

**From:** Lane F. **To:** Lane A (`netproto`, `kernel/src/proc/elf.rs`).
**Filed:** 2026-10-12, on the operator's instruction. **Status:** open.

**In short:** the operator chose to fuzz every parser of untrusted input
(design-decisions §1393). Two of them are yours: the network stack's
packet parsers, which read whatever arrives on the wire, and the kernel's
ELF loader, which reads every program SlateOS runs. `scripts/fuzz.py` (lane
F's) runs any crate's harness in WSL with cargo-fuzz.

## 1. `netproto`: the harness below, ready to drop in

It was run through `scripts/fuzz.py` for two minutes a target: `frame` ran
33 million inputs, `dns` 4.6 million and `dhcp` 3.2 million, with no
finding. Put the files in `netproto/fuzz/`, commit them, and run a round
whenever a parser changes:

    python scripts/fuzz.py run netproto/fuzz 1200

The three targets:
- `frame` reads an Ethernet frame all the way down: ARP; IPv4 and IPv6;
  ICMP, IGMP, ICMPv6's neighbour discovery and MLD; TCP and UDP; and DNS
  and DHCP over UDP. Each layer's checksum is made valid before the layer
  is read, so the fuzzer reaches what lies behind it rather than stopping
  at a sum it would almost never get right by chance.
- `dns` exercises every name read from every offset, which is where
  compression pointers could loop or overrun.
- `dhcp` exercises every option.

`netproto` forbids `unsafe`, so nothing runs under the address sanitizer.

Mutation testing (design-decisions §1393) of netproto's last change,
8c3e25af6, found one test missing. `write_echo_reply` (`icmpv6.rs:353`)
still passes every test with its `<` made `<=`. Nothing checks an echo
reply written into a buffer of exactly the reply's size, which must
succeed. The other mutant it left, `|` for `^` in `answer_solicitation`
(`icmpv6.rs:331`), cannot matter: the flags it joins never share a bit.

## 2. The ELF loader: its parser needs to be a crate of its own first

`kernel/src/proc/elf.rs` cannot be fuzzed where it is: it is part of the
kernel crate, which does not build for the host. The parsing half depends
on the kernel for only its error value (`KernelError::InvalidExecutable`).
That half is:
- `ElfFile::parse`, the header and program-header checks;
- `program_header`, `loadable_segments` and its iterator;
- `raw_segment_bytes` and `segment_data`;
- `interp_path`, `detect_linux_abi`, `has_slateos_marker` and the note
  search;
- `image_end`, with its bias arithmetic.

Asked:
- Move that half into a `no_std` crate (say `elfparse`, with
  `#![forbid(unsafe_code)]`) with its own error type. The kernel maps that
  error to `InvalidExecutable` and keeps the mapping and loading (page
  tables, frames).
- Give the crate a harness at `elfparse/fuzz/` in the shape of section 1's
  (`harness.toml`, `fuzz_targets/`, `seeds.py`). One target, `elf`: parse
  the input, walk every program header and loadable segment, read the
  interpreter path and the notes, and compute `image_end` at a few biases,
  including ones near `u64::MAX`.
- Seed it from the ELF files the tree builds: the test ELFs `elf.rs`'s own
  builders make (copied out by a test, or by `seeds.py`), and a handful of
  real programs from the rootfs.

## The files for section 1

`harness.toml`:

```toml
# netproto's cargo-fuzz harness, run by `scripts/fuzz.py run netproto/fuzz`.
# Installed as ~/fuzz/netproto-fuzz/tree/fuzz/Cargo.toml in WSL; the `path`s
# below are relative to that place. Kept under this name so that nothing in the
# real workspace mistakes this directory for a package. netproto has no
# `unsafe` code, so its targets run without the address sanitizer. Seeds:
# `seeds.py`.
[package]
name = "netproto-fuzz"
version = "0.0.0"
publish = false
edition = "2021"

[package.metadata]
cargo-fuzz = true

[dependencies]
libfuzzer-sys = "0.4"
netproto = { path = "../netproto" }

[workspace]
members = ["."]

[profile.release]
debug = 1

[[bin]]
name = "frame"
path = "fuzz_targets/frame.rs"
test = false
doc = false
bench = false

[[bin]]
name = "dns"
path = "fuzz_targets/dns.rs"
test = false
doc = false
bench = false

[[bin]]
name = "dhcp"
path = "fuzz_targets/dhcp.rs"
test = false
doc = false
bench = false
```

`seeds.py`:

```python
"""Seeds for netproto's targets: one well-formed packet of each kind the
parsers read, so that the fuzzer starts from inside each layer. Checksums are
left zero; the `frame` target makes them valid before each layer is read.
Called by `scripts/fuzz.py` as `seed(src, corpus)`."""
import os
import struct

MAC_A = bytes([2, 0, 0, 0, 0, 1])
MAC_B = bytes([2, 0, 0, 0, 0, 2])
IP_A = bytes([10, 0, 2, 15])
IP_B = bytes([10, 0, 2, 3])
IP6_A = bytes.fromhex("fe800000000000000000000000000001")
IP6_B = bytes.fromhex("ff0200000000000000000001ff000002")


def ether(ethertype, payload, dst=MAC_B):
    return dst + MAC_A + struct.pack(">H", ethertype) + payload


def ipv4(proto, payload):
    return struct.pack(">BBHHHBBH4s4s", 0x45, 0, 20 + len(payload), 1, 0x4000, 64, proto, 0,
                       IP_A, IP_B) + payload


def ipv6(nh, payload):
    return struct.pack(">IHBB16s16s", 6 << 28, len(payload), nh, 255, IP6_A, IP6_B) + payload


def udp(sport, dport, payload):
    return struct.pack(">HHHH", sport, dport, 8 + len(payload), 0) + payload


def name(n):
    return b"".join(bytes([len(p)]) + p for p in n.encode().split(b".")) + b"\0"


DNS_QUERY = struct.pack(">HHHHHH", 0x1234, 0x0100, 1, 0, 0, 0) + name("example.com") + \
    struct.pack(">HH", 1, 1)
DNS_ANSWER = struct.pack(">HHHHHH", 0x1234, 0x8180, 1, 2, 0, 0) + name("example.com") + \
    struct.pack(">HH", 1, 1) + \
    b"\xc0\x0c" + struct.pack(">HHIH", 1, 1, 300, 4) + bytes([93, 184, 216, 34]) + \
    b"\xc0\x0c" + struct.pack(">HHIH", 28, 1, 300, 16) + bytes(range(16))
DNS_PTR = struct.pack(">HHHHHH", 7, 0x8180, 1, 1, 0, 0) + name("3.2.0.10.in-addr.arpa") + \
    struct.pack(">HH", 12, 1) + b"\xc0\x0c" + struct.pack(">HHIH", 12, 1, 60, 13) + \
    name("gateway.lan")[:13]
DHCP_OFFER = struct.pack(">BBBBIHH4s4s4s4s16s64s128s", 2, 1, 6, 0, 0xdeadbeef, 0, 0x8000,
                         bytes(4), IP_A, IP_B, bytes(4), MAC_A + bytes(10), bytes(64),
                         bytes(128)) + bytes([99, 130, 83, 99]) + \
    bytes([53, 1, 2, 1, 4, 255, 255, 255, 0, 3, 4]) + IP_B + bytes([6, 4]) + IP_B + \
    bytes([51, 4, 0, 0, 14, 16, 54, 4]) + IP_B + bytes([255])
TCP_SYN = struct.pack(">HHIIBBHHH", 40000, 80, 1, 0, 6 << 4, 0x02, 64240, 0, 0) + \
    bytes([2, 4, 5, 180, 1, 3, 3, 7])
ICMP_ECHO = struct.pack(">BBHHH", 8, 0, 0, 1, 1) + b"ping"
IGMP_QUERY = struct.pack(">BBH4s", 0x11, 100, 0, bytes(4))
NEIGHBOR_SOLICIT = struct.pack(">BBHI16s", 135, 0, 0, 0, IP6_A) + bytes([1, 1]) + MAC_A
MLD_QUERY = struct.pack(">BBHHH16s", 130, 0, 0, 1000, 0, bytes(16))
ARP_REQUEST = struct.pack(">HHBBH6s4s6s4s", 1, 0x0800, 6, 4, 1, MAC_A, IP_A, bytes(6), IP_B)


def seed(src, corpus):
    frames = {
        "arp": ether(0x0806, ARP_REQUEST, dst=b"\xff" * 6),
        "dns-query": ether(0x0800, ipv4(17, udp(5353, 53, DNS_QUERY))),
        "dns-answer": ether(0x0800, ipv4(17, udp(53, 5353, DNS_ANSWER))),
        "dhcp-offer": ether(0x0800, ipv4(17, udp(67, 68, DHCP_OFFER))),
        "tcp-syn": ether(0x0800, ipv4(6, TCP_SYN)),
        "icmp-echo": ether(0x0800, ipv4(1, ICMP_ECHO)),
        "igmp-query": ether(0x0800, ipv4(2, IGMP_QUERY)),
        "ns": ether(0x86DD, ipv6(58, NEIGHBOR_SOLICIT)),
        "mld-query": ether(0x86DD, ipv6(58, MLD_QUERY)),
        "udp6-dns": ether(0x86DD, ipv6(17, udp(5353, 53, DNS_QUERY))),
        "tcp6-syn": ether(0x86DD, ipv6(6, TCP_SYN)),
    }
    for label, data in frames.items():
        open(os.path.join(corpus("frame"), label), "wb").write(data)
    for label, data in (("query", DNS_QUERY), ("answer", DNS_ANSWER), ("ptr", DNS_PTR)):
        open(os.path.join(corpus("dns"), label), "wb").write(data)
    open(os.path.join(corpus("dhcp"), "offer"), "wb").write(DHCP_OFFER)
```

`fuzz_targets/dhcp.rs`:

```rust
//! A DHCP message from the network, and every option it might carry.
#![no_main]

use libfuzzer_sys::fuzz_target;
use netproto::dhcp;

fuzz_target!(|data: &[u8]| {
    if let Some(m) = dhcp::Message::parse(data) {
        let _ = (m.msg_type(), m.subnet_mask(), m.router(), m.dns(), m.server_id());
        for code in 0..=255u8 {
            let _ = m.option(code);
        }
    }
});
```

`fuzz_targets/dns.rs`:

```rust
//! A DNS message from the network: its header, its answers, the first
//! address and name it gives, and every name read from every offset -- the
//! compression pointers `read_name` follows are where a loop or an overrun
//! would hide.
#![no_main]

use libfuzzer_sys::fuzz_target;
use netproto::dns;

fuzz_target!(|data: &[u8]| {
    if let Some(m) = dns::Message::parse(data) {
        let _ = (m.id, m.flags, m.qdcount, m.ancount, m.is_response(), m.truncated(), m.rcode());
        for a in m.answers() {
            let _ = (a.atype, a.class, a.ttl, a.rdata.len());
        }
        let (mut v4, mut v6, mut name) = ([0u8; 4], [0u8; 16], [0u8; 256]);
        let _ = (m.first_ipv4(&mut v4), m.first_ipv6(&mut v6), m.first_ptr(&mut name));
    }
    let mut out = [0u8; 256];
    for start in 0..data.len().min(512) {
        let _ = dns::read_name(data, start, &mut out);
    }
});
```

`fuzz_targets/frame.rs`:

```rust
//! An Ethernet frame, read all the way down: ARP, IPv4 and IPv6, and under
//! them ICMP, IGMP, ICMPv6's neighbour discovery and MLD, TCP and UDP, and
//! DNS and DHCP in UDP. Each checksum is made valid before the layer is read,
//! so that the fuzzer reaches what lies behind it rather than stopping at a
//! sum it would almost never get right by chance.
#![no_main]

use libfuzzer_sys::fuzz_target;
use netproto::{arp, checksum, dhcp, dns, ethernet, icmp, icmpv6, igmp, ipv4, ipv6, mld, tcp, udp};

/// Write into `seg[at..at + 2]` the checksum that makes `seg` verify against
/// the pseudo-header sum `sum`.
fn fix(seg: &mut [u8], at: usize, sum: u32) {
    if seg.len() >= at + 2 {
        seg[at] = 0;
        seg[at + 1] = 0;
        let c = checksum::internet_continue(sum, seg);
        seg[at..at + 2].copy_from_slice(&c.to_be_bytes());
    }
}

fn application(payload: &[u8]) {
    if let Some(m) = dns::Message::parse(payload) {
        let _ = (m.is_response(), m.truncated(), m.rcode());
        for a in m.answers() {
            let _ = (a.atype, a.class, a.ttl, a.rdata.len());
        }
        let (mut v4, mut v6, mut name) = ([0u8; 4], [0u8; 16], [0u8; 256]);
        let _ = (m.first_ipv4(&mut v4), m.first_ipv6(&mut v6), m.first_ptr(&mut name));
    }
    if let Some(m) = dhcp::Message::parse(payload) {
        let _ = (m.msg_type(), m.subnet_mask(), m.router(), m.dns(), m.server_id());
    }
}

fn udp_v4(p: &ipv4::Packet<'_>) {
    let mut seg = p.payload.to_vec();
    if seg.len() >= 8 {
        let len = usize::from(u16::from_be_bytes([seg[4], seg[5]]));
        if (8..=seg.len()).contains(&len) {
            let sum = ipv4::pseudo_header_sum(&p.src, &p.dst, len as u16, ipv4::PROTO_UDP);
            fix(&mut seg[..len], 6, sum);
        }
    }
    if let Some(d) = udp::Datagram::parse(&seg, &p.src, &p.dst) {
        application(d.payload);
    }
}

fn ipv4_packet(buf: &[u8]) {
    // The header's own checksum, over the header alone.
    let mut buf = buf.to_vec();
    if let Some(&b0) = buf.first() {
        let header_len = usize::from(b0 & 0x0f) * 4;
        if header_len >= 20 && buf.len() >= header_len {
            fix(&mut buf[..header_len], 10, 0);
        }
    }
    let Some(p) = ipv4::Packet::parse(&buf) else { return };
    let _ = (p.dont_fragment(), p.is_fragment());
    match p.protocol {
        ipv4::PROTO_ICMP => {
            // ICMP's checksum covers the message alone, as IGMP's does.
            let mut msg = p.payload.to_vec();
            fix(&mut msg, 2, 0);
            if let Some(e) = icmp::Echo::parse(&msg) {
                let mut out = [0u8; 2048];
                let _ = icmp::reply_to(&mut out, &e);
            }
        }
        igmp::PROTO_IGMP => {
            let mut msg = p.payload.to_vec();
            fix(&mut msg, 2, 0);
            if let Some(m) = igmp::parse(&msg) {
                let _ = m.is_general_query();
            }
        }
        ipv4::PROTO_TCP => {
            let mut seg = p.payload.to_vec();
            let sum = ipv4::pseudo_header_sum(&p.src, &p.dst, seg.len() as u16, ipv4::PROTO_TCP);
            fix(&mut seg, 16, sum);
            if let Some(s) = tcp::Segment::parse(&seg, &p.src, &p.dst) {
                let _ = (s.has_flag(tcp::FLAG_SYN), s.options.len(), s.payload.len());
            }
        }
        ipv4::PROTO_UDP => udp_v4(&p),
        _ => {}
    }
}

fn ipv6_packet(buf: &[u8]) {
    let Some(p) = ipv6::Packet::parse(buf) else { return };
    let mut seg = p.payload.to_vec();
    match p.next_header {
        6 => {
            let sum = ipv6::pseudo_header_sum(&p.src, &p.dst, seg.len() as u32, 6);
            fix(&mut seg, 16, sum);
            let _ = tcp::Segment::parse_v6(&seg, &p.src, &p.dst);
        }
        17 => {
            if seg.len() >= 8 {
                let len = usize::from(u16::from_be_bytes([seg[4], seg[5]]));
                if (8..=seg.len()).contains(&len) {
                    let sum = ipv6::pseudo_header_sum(&p.src, &p.dst, len as u32, 17);
                    fix(&mut seg[..len], 6, sum);
                }
            }
            if let Some(d) = udp::Datagram::parse_v6(&seg, &p.src, &p.dst) {
                application(d.payload);
            }
        }
        58 => {
            if seg.len() >= 4 {
                seg[2] = 0;
                seg[3] = 0;
                let c = icmpv6::checksum(&p.src, &p.dst, &seg);
                seg[2..4].copy_from_slice(&c.to_be_bytes());
            }
            let _ = icmpv6::parse_neighbor_solicitation(&seg, &p.src, &p.dst);
            let _ = icmpv6::parse_neighbor_advertisement(&seg, &p.src, &p.dst);
            let _ = icmpv6::is_echo_request(&seg, &p.src, &p.dst);
            let _ = mld::parse(&seg, &p.src, &p.dst);
        }
        _ => {}
    }
}

fuzz_target!(|data: &[u8]| {
    let Some(frame) = ethernet::Frame::parse(data) else { return };
    let _ = (frame.is_broadcast(), frame.is_multicast());
    match frame.ethertype {
        ethernet::ETHERTYPE_ARP => {
            if let Some(a) = arp::Packet::parse(frame.payload) {
                let _ = arp::reply_to(&a, &[2, 0, 0, 0, 0, 1]);
            }
        }
        0x0800 => ipv4_packet(frame.payload),
        0x86dd => ipv6_packet(frame.payload),
        _ => {}
    }
});
```

## Replies
