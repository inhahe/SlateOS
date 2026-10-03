//! Linux's IPv4/IPv6 **multicast socket options**, translated for the
//! daemon's [`OP_UDP_SETOPT`](crate::ring::OP_UDP_SETOPT) and
//! [`OP_UDP_GETOPT`](crate::ring::OP_UDP_GETOPT).
//!
//! The kernel copies a `setsockopt(2)` call's `optval` in and asks
//! [`parse_set`] what the daemon should be told; a `getsockopt(2)` asks
//! [`get_option`] what to read and [`encode_get`] how Linux would write it
//! back. Every errno is the one Linux 6.x gives the same call
//! (`net/ipv4/ip_sockglue.c`, `net/ipv6/ipv6_sockglue.c`), including where
//! the two families differ: an IPv4 leave of a non-multicast address is
//! `EADDRNOTAVAIL` (Linux simply does not find it) where IPv6's is `EINVAL`.
//!
//! The split with the daemon: what only the ABI knows -- the option numbers,
//! the struct layouts, Linux's `-1` meaning "the default", the interface
//! *indices* libc's `if_nametoindex` hands out -- is settled here. What only
//! the daemon can judge -- whether an IPv4 interface *address* is the host's,
//! whether the socket is in a group already, whether a table is full -- is
//! left to it, and comes back as an `ERR_*` completion.
//!
//! One deliberate divergence: Linux lets a TCP socket set
//! `IP_MULTICAST_LOOP` and `IPV6_MULTICAST_LOOP`, which it then never uses.
//! Here a stream socket is refused them (`ENOPROTOOPT`), so that the value a
//! stream socket's `getsockopt` reports -- always the default, from
//! [`Get::Fixed`] -- is never a lie about a setting it was given.

use crate::ring;

/// `IPPROTO_IP` (`SOL_IP`): the level of the IPv4 options.
pub const SOL_IP: i32 = 0;
/// `IPPROTO_IPV6` (`SOL_IPV6`): the level of the IPv6 options.
pub const SOL_IPV6: i32 = 41;

/// `IP_MULTICAST_TTL`: the TTL of the socket's IPv4 multicast sends.
pub const IP_MULTICAST_TTL: i32 = 33;
/// `IP_MULTICAST_LOOP`: whether its IPv4 multicast sends loop back.
pub const IP_MULTICAST_LOOP: i32 = 34;
/// `IP_ADD_MEMBERSHIP`: join an IPv4 group (`struct ip_mreq`/`ip_mreqn`).
pub const IP_ADD_MEMBERSHIP: i32 = 35;
/// `IP_DROP_MEMBERSHIP`: leave an IPv4 group.
pub const IP_DROP_MEMBERSHIP: i32 = 36;
/// `IPV6_MULTICAST_HOPS`: the hop limit of its IPv6 multicast sends.
pub const IPV6_MULTICAST_HOPS: i32 = 18;
/// `IPV6_MULTICAST_LOOP`: whether its IPv6 multicast sends loop back.
pub const IPV6_MULTICAST_LOOP: i32 = 19;
/// `IPV6_JOIN_GROUP`, also spelled `IPV6_ADD_MEMBERSHIP` (`struct ipv6_mreq`).
pub const IPV6_JOIN_GROUP: i32 = 20;
/// `IPV6_LEAVE_GROUP`, also spelled `IPV6_DROP_MEMBERSHIP`.
pub const IPV6_LEAVE_GROUP: i32 = 21;

/// The index of the one network interface, `eth0`, as libc's
/// `if_nametoindex` numbers it (`posix/src/socket.rs`, `INTERFACES`: `lo` 1,
/// `eth0` 2). A membership naming any other index has no device to be on.
pub const NIC_IFINDEX: u32 = 2;

/// The most of `optval` [`parse_set`] reads: a `struct ipv6_mreq`.
pub const MAX_OPTVAL: usize = 20;

/// Linux `ENODEV`.
pub const ENODEV: i32 = 19;
/// Linux `EFAULT`.
pub const EFAULT: i32 = 14;
/// Linux `EINVAL`.
pub const EINVAL: i32 = 22;
/// Linux `EPROTO`.
pub const EPROTO: i32 = 71;
/// Linux `ENOPROTOOPT`.
pub const ENOPROTOOPT: i32 = 92;
/// Linux `EOPNOTSUPP`.
pub const EOPNOTSUPP: i32 = 95;
/// Linux `EADDRNOTAVAIL`.
pub const EADDRNOTAVAIL: i32 = 99;

/// The socket an option is set on or read from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Socket {
    /// Created `AF_INET6`. It takes the IPv4 options as well, as on Linux; an
    /// `AF_INET` socket takes no IPv6 ones.
    pub v6: bool,
    /// A stream (TCP) socket, which has no multicast to configure.
    pub stream: bool,
}

/// What a `setsockopt` asks of the daemon.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Set {
    /// A scalar option and its value, for
    /// [`Sqe::pack_udp_opt`](crate::ring::Sqe::pack_udp_opt).
    Scalar {
        /// The `UDP_OPT_*` option.
        option: u16,
        /// Its value, 0-255.
        value: u32,
    },
    /// A join or leave, with the data window that names the group: the first
    /// `len` bytes of `window`.
    Group {
        /// `UDP_OPT_MCAST_JOIN4` ... `UDP_OPT_MCAST_LEAVE6`.
        option: u16,
        /// `[group:4][interface address:4]`, or the 16-byte IPv6 group.
        window: [u8; 16],
        /// [`ring::UDP_MREQ4_LEN`] or [`ring::UDP_MREQ6_LEN`].
        len: usize,
    },
}

/// What a `getsockopt` reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Get {
    /// The value of this `UDP_OPT_*` option, from the daemon -- or, for a
    /// socket the daemon holds nothing for yet, [`default_value`].
    Ask(u16),
    /// A value that cannot have changed: a stream socket's, which may set
    /// none of these.
    Fixed(i32),
}

/// Whether `(level, optname)` is one of the options this module handles. The
/// kernel sends only these through [`parse_set`] and [`get_option`], and
/// keeps its own answer for every other option.
#[must_use]
pub fn is_multicast_option(level: i32, optname: i32) -> bool {
    match level {
        SOL_IP => matches!(
            optname,
            IP_MULTICAST_TTL | IP_MULTICAST_LOOP | IP_ADD_MEMBERSHIP | IP_DROP_MEMBERSHIP
        ),
        SOL_IPV6 => matches!(
            optname,
            IPV6_MULTICAST_HOPS | IPV6_MULTICAST_LOOP | IPV6_JOIN_GROUP | IPV6_LEAVE_GROUP
        ),
        _ => false,
    }
}

/// The value a scalar option has on a socket no one has set it on: Linux's
/// defaults, the same as `netproto::mcast::SocketGroups::new` (the daemon
/// checks the two agree at compile time). `None` for a join or leave, which
/// has no value.
#[must_use]
pub const fn default_value(option: u16) -> Option<i32> {
    match option {
        ring::UDP_OPT_MCAST_TTL
        | ring::UDP_OPT_MCAST_LOOP4
        | ring::UDP_OPT_MCAST_HOPS
        | ring::UDP_OPT_MCAST_LOOP6 => Some(1),
        _ => None,
    }
}

fn is_multicast4(addr: [u8; 4]) -> bool {
    addr[0] & 0xF0 == 0xE0
}

fn is_multicast6(addr: &[u8; 16]) -> bool {
    addr[0] == 0xFF
}

/// `N` bytes of `optval` from `at`, or `EFAULT` when the caller copied in
/// fewer than its own `optlen` said were there.
fn bytes<const N: usize>(optval: &[u8], at: usize) -> Result<[u8; N], i32> {
    optval
        .get(at..at.saturating_add(N))
        .and_then(|b| <[u8; N]>::try_from(b).ok())
        .ok_or(EFAULT)
}

/// The integer an IPv4 option's `optval` holds, read as Linux reads it: an
/// `int` when `optlen` has room for one, else a single unsigned byte; `None`
/// when `optlen` is 0, which every caller refuses.
fn ip_int(optval: &[u8], optlen: usize) -> Result<Option<i32>, i32> {
    Ok(if optlen >= 4 {
        Some(i32::from_ne_bytes(bytes(optval, 0)?))
    } else if optlen >= 1 {
        Some(i32::from(bytes::<1>(optval, 0)?[0]))
    } else {
        None
    })
}

/// Translate `setsockopt(level, optname, optval, optlen)` on `sock`.
///
/// `optval` holds the first `min(optlen, MAX_OPTVAL)` bytes of the caller's
/// buffer; `optlen` is the length the caller passed. Only the options
/// [`is_multicast_option`] names are handled: any other is `ENOPROTOOPT`.
///
/// # Errors
///
/// A positive Linux errno: `EINVAL` (too short, out of range, not a
/// multicast group to join), `EPROTO` (a membership on a stream socket),
/// `ENODEV` (joining on an interface index that is not the NIC's),
/// `EADDRNOTAVAIL` (leaving on one), `ENOPROTOOPT` (an IPv6 option on an
/// `AF_INET` socket, or an option a stream socket does not take), `EFAULT`
/// (`optval` shorter than its `optlen`).
pub fn parse_set(
    level: i32,
    optname: i32,
    optval: &[u8],
    optlen: usize,
    sock: Socket,
) -> Result<Set, i32> {
    match level {
        SOL_IP => set_ipv4(optname, optval, optlen, sock),
        // The level is refused before the option is looked at.
        SOL_IPV6 if !sock.v6 => Err(ENOPROTOOPT),
        SOL_IPV6 => set_ipv6(optname, optval, optlen, sock),
        _ => Err(ENOPROTOOPT),
    }
}

/// [`parse_set`] at `SOL_IP`.
fn set_ipv4(optname: i32, optval: &[u8], optlen: usize, sock: Socket) -> Result<Set, i32> {
    match optname {
        IP_MULTICAST_TTL => {
            if sock.stream {
                return Err(EINVAL);
            }
            let val = ip_int(optval, optlen)?.ok_or(EINVAL)?;
            // -1 is Linux's "the default".
            let val = if val == -1 { 1 } else { val };
            let value = u8::try_from(val).map_err(|_| EINVAL)?;
            Ok(Set::Scalar {
                option: ring::UDP_OPT_MCAST_TTL,
                value: u32::from(value),
            })
        }
        IP_MULTICAST_LOOP => {
            if sock.stream {
                return Err(ENOPROTOOPT);
            }
            let val = ip_int(optval, optlen)?.ok_or(EINVAL)?;
            Ok(Set::Scalar {
                option: ring::UDP_OPT_MCAST_LOOP4,
                value: u32::from(val != 0),
            })
        }
        IP_ADD_MEMBERSHIP | IP_DROP_MEMBERSHIP => {
            let join = optname == IP_ADD_MEMBERSHIP;
            if sock.stream {
                return Err(EPROTO);
            }
            // struct ip_mreq is { group, interface address }; struct
            // ip_mreqn adds an interface index, which wins over the address.
            if optlen < 8 {
                return Err(EINVAL);
            }
            let group: [u8; 4] = bytes(optval, 0)?;
            let mut addr: [u8; 4] = bytes(optval, 4)?;
            let ifindex = if optlen >= 12 {
                i32::from_ne_bytes(bytes(optval, 8)?)
            } else {
                0
            };
            if join && !is_multicast4(group) {
                return Err(EINVAL);
            }
            if ifindex != 0 {
                if u32::try_from(ifindex).ok() != Some(NIC_IFINDEX) {
                    return Err(if join { ENODEV } else { EADDRNOTAVAIL });
                }
                addr = [0; 4];
            }
            let mut window = [0u8; 16];
            window[..4].copy_from_slice(&group);
            window[4..8].copy_from_slice(&addr);
            Ok(Set::Group {
                option: if join {
                    ring::UDP_OPT_MCAST_JOIN4
                } else {
                    ring::UDP_OPT_MCAST_LEAVE4
                },
                window,
                len: ring::UDP_MREQ4_LEN,
            })
        }
        _ => Err(ENOPROTOOPT),
    }
}

/// [`parse_set`] at `SOL_IPV6`, on an `AF_INET6` socket.
fn set_ipv6(optname: i32, optval: &[u8], optlen: usize, sock: Socket) -> Result<Set, i32> {
    match optname {
        IPV6_MULTICAST_HOPS => {
            if sock.stream {
                return Err(ENOPROTOOPT);
            }
            if optlen < 4 {
                return Err(EINVAL);
            }
            let val = i32::from_ne_bytes(bytes(optval, 0)?);
            if !(-1..=255).contains(&val) {
                return Err(EINVAL);
            }
            let value = u32::try_from(if val == -1 { 1 } else { val }).map_err(|_| EINVAL)?;
            Ok(Set::Scalar {
                option: ring::UDP_OPT_MCAST_HOPS,
                value,
            })
        }
        IPV6_MULTICAST_LOOP => {
            if sock.stream {
                return Err(ENOPROTOOPT);
            }
            if optlen < 4 {
                return Err(EINVAL);
            }
            // Unlike IPv4's, only 0 and 1 are accepted.
            let value = match i32::from_ne_bytes(bytes(optval, 0)?) {
                0 => 0,
                1 => 1,
                _ => return Err(EINVAL),
            };
            Ok(Set::Scalar {
                option: ring::UDP_OPT_MCAST_LOOP6,
                value,
            })
        }
        IPV6_JOIN_GROUP | IPV6_LEAVE_GROUP => {
            let join = optname == IPV6_JOIN_GROUP;
            // struct ipv6_mreq is { group, interface index }. Linux checks
            // the length before the socket's type, here as there.
            if optlen < 20 {
                return Err(EINVAL);
            }
            if sock.stream {
                return Err(EPROTO);
            }
            let group: [u8; 16] = bytes(optval, 0)?;
            let ifindex = u32::from_ne_bytes(bytes(optval, 16)?);
            if !is_multicast6(&group) {
                return Err(EINVAL);
            }
            if ifindex != 0 && ifindex != NIC_IFINDEX {
                return Err(if join { ENODEV } else { EADDRNOTAVAIL });
            }
            Ok(Set::Group {
                option: if join {
                    ring::UDP_OPT_MCAST_JOIN6
                } else {
                    ring::UDP_OPT_MCAST_LEAVE6
                },
                window: group,
                len: ring::UDP_MREQ6_LEN,
            })
        }
        _ => Err(ENOPROTOOPT),
    }
}

/// Translate `getsockopt(level, optname)` on `sock`.
///
/// # Errors
///
/// `EOPNOTSUPP` for an IPv6 option on an `AF_INET` socket (Linux's
/// `do_ip_getsockopt` answers so, where its `setsockopt` says
/// `ENOPROTOOPT`); `ENOPROTOOPT` for the joins and leaves, which have
/// nothing to read, and for every option this module does not handle.
pub fn get_option(level: i32, optname: i32, sock: Socket) -> Result<Get, i32> {
    let option = match (level, optname) {
        (SOL_IP, IP_MULTICAST_TTL) => ring::UDP_OPT_MCAST_TTL,
        (SOL_IP, IP_MULTICAST_LOOP) => ring::UDP_OPT_MCAST_LOOP4,
        (SOL_IPV6, _) if !sock.v6 => return Err(EOPNOTSUPP),
        (SOL_IPV6, IPV6_MULTICAST_HOPS) => ring::UDP_OPT_MCAST_HOPS,
        (SOL_IPV6, IPV6_MULTICAST_LOOP) => ring::UDP_OPT_MCAST_LOOP6,
        _ => return Err(ENOPROTOOPT),
    };
    if sock.stream {
        // A stream socket can set none of them, so its value is the default.
        return default_value(option).map(Get::Fixed).ok_or(ENOPROTOOPT);
    }
    Ok(Get::Ask(option))
}

/// The bytes a `getsockopt` writes for `value`, given the `*optlen` the
/// caller passed (`room`), and how many of them: Linux's rules, which
/// differ by level. At `SOL_IP` (`ipv4`) a buffer shorter than an `int`
/// gets the value as one byte when it fits in one, and a negative `room` is
/// `EINVAL`; at `SOL_IPV6` the length is `min(4, room)` taken unsigned, so a
/// negative `room` writes all four.
///
/// # Errors
///
/// `EINVAL` for a negative `room` at `SOL_IP`.
pub fn encode_get(value: i32, room: i32, ipv4: bool) -> Result<([u8; 4], usize), i32> {
    if ipv4 {
        if room < 0 {
            return Err(EINVAL);
        }
        if room < 4
            && room > 0
            && let Ok(byte) = u8::try_from(value)
        {
            return Ok(([byte, 0, 0, 0], 1));
        }
        let n = usize::try_from(room).map_or(4, |r| r.min(4));
        return Ok((value.to_ne_bytes(), n));
    }
    let n = usize::try_from(room).map_or(4, |r| r.min(4));
    Ok((value.to_ne_bytes(), n))
}

#[cfg(test)]
mod tests {
    use super::*;

    const UDP4: Socket = Socket {
        v6: false,
        stream: false,
    };
    const UDP6: Socket = Socket {
        v6: true,
        stream: false,
    };
    const TCP4: Socket = Socket {
        v6: false,
        stream: true,
    };
    const TCP6: Socket = Socket {
        v6: true,
        stream: true,
    };

    fn int(v: i32) -> [u8; 4] {
        v.to_ne_bytes()
    }

    fn set(level: i32, optname: i32, optval: &[u8], sock: Socket) -> Result<Set, i32> {
        parse_set(level, optname, optval, optval.len(), sock)
    }

    #[allow(
        clippy::unnecessary_wraps,
        reason = "shaped like `set`'s result, so the two compare directly"
    )]
    fn scalar(option: u16, value: u32) -> Result<Set, i32> {
        Ok(Set::Scalar { option, value })
    }

    fn mreq(group: [u8; 4], addr: [u8; 4]) -> [u8; 8] {
        let mut m = [0u8; 8];
        m[..4].copy_from_slice(&group);
        m[4..].copy_from_slice(&addr);
        m
    }

    fn mreqn(group: [u8; 4], addr: [u8; 4], ifindex: i32) -> [u8; 12] {
        let mut m = [0u8; 12];
        m[..8].copy_from_slice(&mreq(group, addr));
        m[8..].copy_from_slice(&ifindex.to_ne_bytes());
        m
    }

    fn mreq6(group: [u8; 16], ifindex: u32) -> [u8; 20] {
        let mut m = [0u8; 20];
        m[..16].copy_from_slice(&group);
        m[16..].copy_from_slice(&ifindex.to_ne_bytes());
        m
    }

    const MDNS4: [u8; 4] = [224, 0, 0, 251];
    const MDNS6: [u8; 16] = [0xFF, 0x02, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0xFB];

    #[test]
    fn the_ipv4_ttl_takes_an_int_or_a_byte_and_minus_one_is_the_default() {
        let ttl = ring::UDP_OPT_MCAST_TTL;
        assert_eq!(set(SOL_IP, IP_MULTICAST_TTL, &int(32), UDP4), scalar(ttl, 32));
        assert_eq!(set(SOL_IP, IP_MULTICAST_TTL, &int(255), UDP4), scalar(ttl, 255));
        assert_eq!(set(SOL_IP, IP_MULTICAST_TTL, &int(0), UDP4), scalar(ttl, 0));
        assert_eq!(set(SOL_IP, IP_MULTICAST_TTL, &int(-1), UDP4), scalar(ttl, 1));
        assert_eq!(set(SOL_IP, IP_MULTICAST_TTL, &int(256), UDP4), Err(EINVAL));
        assert_eq!(set(SOL_IP, IP_MULTICAST_TTL, &int(-2), UDP4), Err(EINVAL));
        // A one-byte optval is an unsigned char: 200 stays 200.
        assert_eq!(set(SOL_IP, IP_MULTICAST_TTL, &[200], UDP4), scalar(ttl, 200));
        // Three bytes are still too few for an int, so the first is read.
        assert_eq!(set(SOL_IP, IP_MULTICAST_TTL, &[7, 9, 9], UDP4), scalar(ttl, 7));
        assert_eq!(set(SOL_IP, IP_MULTICAST_TTL, &[], UDP4), Err(EINVAL));
        // A stream socket has no multicast TTL.
        assert_eq!(set(SOL_IP, IP_MULTICAST_TTL, &int(2), TCP4), Err(EINVAL));
        // And an AF_INET6 socket takes the IPv4 option, as on Linux.
        assert_eq!(set(SOL_IP, IP_MULTICAST_TTL, &int(4), UDP6), scalar(ttl, 4));
    }

    #[test]
    fn the_ipv4_loop_is_any_nonzero() {
        let lp = ring::UDP_OPT_MCAST_LOOP4;
        assert_eq!(set(SOL_IP, IP_MULTICAST_LOOP, &int(0), UDP4), scalar(lp, 0));
        assert_eq!(set(SOL_IP, IP_MULTICAST_LOOP, &int(1), UDP4), scalar(lp, 1));
        assert_eq!(set(SOL_IP, IP_MULTICAST_LOOP, &int(-7), UDP4), scalar(lp, 1));
        assert_eq!(set(SOL_IP, IP_MULTICAST_LOOP, &[0], UDP4), scalar(lp, 0));
        assert_eq!(set(SOL_IP, IP_MULTICAST_LOOP, &[], UDP4), Err(EINVAL));
        assert_eq!(set(SOL_IP, IP_MULTICAST_LOOP, &int(1), TCP4), Err(ENOPROTOOPT));
    }

    #[test]
    fn an_ipv4_membership_reads_ip_mreq_and_ip_mreqn() {
        let join = ring::UDP_OPT_MCAST_JOIN4;
        let leave = ring::UDP_OPT_MCAST_LEAVE4;
        let window = |group: [u8; 4], addr: [u8; 4]| {
            let mut w = [0u8; 16];
            w[..8].copy_from_slice(&mreq(group, addr));
            w
        };
        let group = |option, w| {
            Ok(Set::Group {
                option,
                window: w,
                len: ring::UDP_MREQ4_LEN,
            })
        };
        // ip_mreq: the address goes to the daemon, which knows the host's.
        let m = mreq(MDNS4, [10, 0, 2, 15]);
        assert_eq!(
            set(SOL_IP, IP_ADD_MEMBERSHIP, &m, UDP4),
            group(join, window(MDNS4, [10, 0, 2, 15]))
        );
        assert_eq!(
            set(SOL_IP, IP_DROP_MEMBERSHIP, &m, UDP4),
            group(leave, window(MDNS4, [10, 0, 2, 15]))
        );
        // ip_mreqn: an index wins over the address, which is then "any".
        let m = mreqn(MDNS4, [10, 9, 9, 9], 2);
        assert_eq!(
            set(SOL_IP, IP_ADD_MEMBERSHIP, &m, UDP4),
            group(join, window(MDNS4, [0; 4]))
        );
        // Index 0 leaves the address to say.
        let m = mreqn(MDNS4, [10, 0, 2, 15], 0);
        assert_eq!(
            set(SOL_IP, IP_ADD_MEMBERSHIP, &m, UDP4),
            group(join, window(MDNS4, [10, 0, 2, 15]))
        );
        // Another index has no device: ENODEV to join, EADDRNOTAVAIL to
        // leave (Linux searches the socket's list and finds nothing).
        for bad in [1, 3, -1] {
            let m = mreqn(MDNS4, [0; 4], bad);
            assert_eq!(set(SOL_IP, IP_ADD_MEMBERSHIP, &m, UDP4), Err(ENODEV));
            assert_eq!(set(SOL_IP, IP_DROP_MEMBERSHIP, &m, UDP4), Err(EADDRNOTAVAIL));
        }
    }

    #[test]
    fn an_ipv4_membership_refusals_come_in_linux_order() {
        let unicast = mreq([10, 0, 0, 1], [0; 4]);
        // Joining a unicast address is EINVAL, before the index is looked at.
        assert_eq!(set(SOL_IP, IP_ADD_MEMBERSHIP, &unicast, UDP4), Err(EINVAL));
        assert_eq!(
            set(SOL_IP, IP_ADD_MEMBERSHIP, &mreqn([10, 0, 0, 1], [0; 4], 9), UDP4),
            Err(EINVAL)
        );
        // Leaving one is the daemon's EADDRNOTAVAIL: Linux does not check.
        assert!(matches!(
            set(SOL_IP, IP_DROP_MEMBERSHIP, &unicast, UDP4),
            Ok(Set::Group { .. })
        ));
        // Short of an ip_mreq.
        assert_eq!(set(SOL_IP, IP_ADD_MEMBERSHIP, &[224, 0, 0, 1, 0, 0, 0], UDP4), Err(EINVAL));
        // A stream socket is EPROTO, before even the length.
        assert_eq!(set(SOL_IP, IP_ADD_MEMBERSHIP, &[0; 3], TCP4), Err(EPROTO));
        assert_eq!(set(SOL_IP, IP_DROP_MEMBERSHIP, &mreq(MDNS4, [0; 4]), TCP6), Err(EPROTO));
    }

    #[test]
    fn the_ipv6_hop_limit_needs_an_int() {
        let hops = ring::UDP_OPT_MCAST_HOPS;
        assert_eq!(set(SOL_IPV6, IPV6_MULTICAST_HOPS, &int(5), UDP6), scalar(hops, 5));
        assert_eq!(set(SOL_IPV6, IPV6_MULTICAST_HOPS, &int(-1), UDP6), scalar(hops, 1));
        assert_eq!(set(SOL_IPV6, IPV6_MULTICAST_HOPS, &int(255), UDP6), scalar(hops, 255));
        assert_eq!(set(SOL_IPV6, IPV6_MULTICAST_HOPS, &int(256), UDP6), Err(EINVAL));
        assert_eq!(set(SOL_IPV6, IPV6_MULTICAST_HOPS, &int(-2), UDP6), Err(EINVAL));
        // No byte form at this level.
        assert_eq!(set(SOL_IPV6, IPV6_MULTICAST_HOPS, &[5], UDP6), Err(EINVAL));
        assert_eq!(set(SOL_IPV6, IPV6_MULTICAST_HOPS, &int(5), TCP6), Err(ENOPROTOOPT));
        // An AF_INET socket has no IPv6 options at all.
        assert_eq!(set(SOL_IPV6, IPV6_MULTICAST_HOPS, &int(5), UDP4), Err(ENOPROTOOPT));
    }

    #[test]
    fn the_ipv6_loop_is_zero_or_one_only() {
        let lp = ring::UDP_OPT_MCAST_LOOP6;
        assert_eq!(set(SOL_IPV6, IPV6_MULTICAST_LOOP, &int(0), UDP6), scalar(lp, 0));
        assert_eq!(set(SOL_IPV6, IPV6_MULTICAST_LOOP, &int(1), UDP6), scalar(lp, 1));
        assert_eq!(set(SOL_IPV6, IPV6_MULTICAST_LOOP, &int(2), UDP6), Err(EINVAL));
        assert_eq!(set(SOL_IPV6, IPV6_MULTICAST_LOOP, &int(-1), UDP6), Err(EINVAL));
        assert_eq!(set(SOL_IPV6, IPV6_MULTICAST_LOOP, &[1], UDP6), Err(EINVAL));
        assert_eq!(set(SOL_IPV6, IPV6_MULTICAST_LOOP, &int(1), TCP6), Err(ENOPROTOOPT));
    }

    #[test]
    fn an_ipv6_membership_reads_ipv6_mreq() {
        let join = |w| {
            Ok(Set::Group {
                option: ring::UDP_OPT_MCAST_JOIN6,
                window: w,
                len: ring::UDP_MREQ6_LEN,
            })
        };
        assert_eq!(set(SOL_IPV6, IPV6_JOIN_GROUP, &mreq6(MDNS6, 0), UDP6), join(MDNS6));
        assert_eq!(set(SOL_IPV6, IPV6_JOIN_GROUP, &mreq6(MDNS6, 2), UDP6), join(MDNS6));
        assert_eq!(
            set(SOL_IPV6, IPV6_LEAVE_GROUP, &mreq6(MDNS6, 0), UDP6),
            Ok(Set::Group {
                option: ring::UDP_OPT_MCAST_LEAVE6,
                window: MDNS6,
                len: ring::UDP_MREQ6_LEN,
            })
        );
        assert_eq!(set(SOL_IPV6, IPV6_JOIN_GROUP, &mreq6(MDNS6, 1), UDP6), Err(ENODEV));
        assert_eq!(
            set(SOL_IPV6, IPV6_LEAVE_GROUP, &mreq6(MDNS6, 7), UDP6),
            Err(EADDRNOTAVAIL)
        );
    }

    #[test]
    fn an_ipv6_membership_refusals_differ_from_ipv4s() {
        let unicast = {
            let mut a = [0u8; 16];
            a[0] = 0xFE;
            a[1] = 0x80;
            a[15] = 1;
            a
        };
        // IPv6 refuses a unicast address to leave as well as to join.
        assert_eq!(set(SOL_IPV6, IPV6_JOIN_GROUP, &mreq6(unicast, 0), UDP6), Err(EINVAL));
        assert_eq!(set(SOL_IPV6, IPV6_LEAVE_GROUP, &mreq6(unicast, 0), UDP6), Err(EINVAL));
        // A v4-mapped multicast address is not IPv6 multicast.
        let mut mapped = [0u8; 16];
        mapped[10] = 0xFF;
        mapped[11] = 0xFF;
        mapped[12..].copy_from_slice(&MDNS4);
        assert_eq!(set(SOL_IPV6, IPV6_JOIN_GROUP, &mreq6(mapped, 0), UDP6), Err(EINVAL));
        // Length is checked before the socket's type here, unlike IPv4.
        assert_eq!(set(SOL_IPV6, IPV6_JOIN_GROUP, &[0xFF; 19], TCP6), Err(EINVAL));
        assert_eq!(set(SOL_IPV6, IPV6_JOIN_GROUP, &mreq6(MDNS6, 0), TCP6), Err(EPROTO));
        assert_eq!(set(SOL_IPV6, IPV6_JOIN_GROUP, &mreq6(MDNS6, 0), UDP4), Err(ENOPROTOOPT));
    }

    #[test]
    fn a_long_optlen_reads_only_what_it_needs() {
        // The kernel copies at most MAX_OPTVAL bytes; a longer optlen is
        // fine, as Linux reads only the struct it expects.
        let m = mreq6(MDNS6, 0);
        assert!(matches!(
            parse_set(SOL_IPV6, IPV6_JOIN_GROUP, &m, 4096, UDP6),
            Ok(Set::Group { .. })
        ));
        assert_eq!(
            parse_set(SOL_IP, IP_MULTICAST_TTL, &int(3), 64, UDP4),
            scalar(ring::UDP_OPT_MCAST_TTL, 3)
        );
        // An optval shorter than its own optlen is the caller's fault.
        assert_eq!(parse_set(SOL_IP, IP_MULTICAST_TTL, &[1], 4, UDP4), Err(EFAULT));
    }

    #[test]
    fn other_options_are_not_ours() {
        assert!(!is_multicast_option(SOL_IP, 1)); // IP_TOS
        assert!(!is_multicast_option(SOL_IPV6, 26)); // IPV6_V6ONLY
        assert!(!is_multicast_option(1, IP_MULTICAST_TTL)); // SOL_SOCKET
        assert!(is_multicast_option(SOL_IP, IP_ADD_MEMBERSHIP));
        assert!(is_multicast_option(SOL_IPV6, IPV6_LEAVE_GROUP));
        assert_eq!(set(SOL_IP, 1, &int(0), UDP4), Err(ENOPROTOOPT));
        assert_eq!(get_option(SOL_IP, IP_ADD_MEMBERSHIP, UDP4), Err(ENOPROTOOPT));
        assert_eq!(get_option(SOL_IPV6, IPV6_JOIN_GROUP, UDP6), Err(ENOPROTOOPT));
    }

    #[test]
    fn reading_asks_the_daemon_except_on_a_stream_socket() {
        assert_eq!(
            get_option(SOL_IP, IP_MULTICAST_TTL, UDP4),
            Ok(Get::Ask(ring::UDP_OPT_MCAST_TTL))
        );
        assert_eq!(
            get_option(SOL_IP, IP_MULTICAST_LOOP, UDP6),
            Ok(Get::Ask(ring::UDP_OPT_MCAST_LOOP4))
        );
        assert_eq!(
            get_option(SOL_IPV6, IPV6_MULTICAST_HOPS, UDP6),
            Ok(Get::Ask(ring::UDP_OPT_MCAST_HOPS))
        );
        assert_eq!(
            get_option(SOL_IPV6, IPV6_MULTICAST_LOOP, UDP6),
            Ok(Get::Ask(ring::UDP_OPT_MCAST_LOOP6))
        );
        assert_eq!(get_option(SOL_IP, IP_MULTICAST_TTL, TCP4), Ok(Get::Fixed(1)));
        assert_eq!(get_option(SOL_IPV6, IPV6_MULTICAST_LOOP, TCP6), Ok(Get::Fixed(1)));
        // The IPv6 level on an AF_INET socket: EOPNOTSUPP to read.
        assert_eq!(get_option(SOL_IPV6, IPV6_MULTICAST_HOPS, UDP4), Err(EOPNOTSUPP));
        assert_eq!(get_option(SOL_IPV6, IPV6_MULTICAST_HOPS, TCP4), Err(EOPNOTSUPP));
    }

    #[test]
    fn every_scalar_has_a_default_and_no_membership_does() {
        for option in [
            ring::UDP_OPT_MCAST_TTL,
            ring::UDP_OPT_MCAST_LOOP4,
            ring::UDP_OPT_MCAST_HOPS,
            ring::UDP_OPT_MCAST_LOOP6,
        ] {
            assert_eq!(default_value(option), Some(1));
        }
        for option in [
            ring::UDP_OPT_MCAST_JOIN4,
            ring::UDP_OPT_MCAST_LEAVE4,
            ring::UDP_OPT_MCAST_JOIN6,
            ring::UDP_OPT_MCAST_LEAVE6,
            0,
        ] {
            assert_eq!(default_value(option), None);
        }
    }

    #[test]
    fn an_ipv4_value_shrinks_to_a_byte_for_a_short_buffer() {
        assert_eq!(encode_get(1, 4, true), Ok((int(1), 4)));
        assert_eq!(encode_get(1, 100, true), Ok((int(1), 4)));
        assert_eq!(encode_get(200, 1, true), Ok(([200, 0, 0, 0], 1)));
        assert_eq!(encode_get(64, 3, true), Ok(([64, 0, 0, 0], 1)));
        // Zero room writes nothing.
        assert_eq!(encode_get(1, 0, true), Ok((int(1), 0)));
        assert_eq!(encode_get(1, -1, true), Err(EINVAL));
        // A value that does not fit a byte is cut to the room instead.
        assert_eq!(encode_get(300, 2, true), Ok((int(300), 2)));
    }

    #[test]
    fn an_ipv6_value_is_cut_to_the_room_taken_unsigned() {
        assert_eq!(encode_get(1, 4, false), Ok((int(1), 4)));
        assert_eq!(encode_get(1, 2, false), Ok((int(1), 2)));
        assert_eq!(encode_get(1, 0, false), Ok((int(1), 0)));
        // A negative room is huge as Linux's unsigned min sees it.
        assert_eq!(encode_get(1, -1, false), Ok((int(1), 4)));
    }
}
