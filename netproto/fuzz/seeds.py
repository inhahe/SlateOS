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
