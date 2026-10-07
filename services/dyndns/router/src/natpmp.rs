//! NAT-PMP, the NAT Port Mapping Protocol (RFC 6886): a router's internet
//! address, and port mappings on it, asked in UDP datagrams sent to the
//! router's port 5351.
//!
//! Only the datagrams are here -- built and read -- and the timing RFC 6886
//! asks of a client. Sending them is the caller's ([`crate::Net`]).
//!
//! ```text
//! external address   request  [0, 0]
//!                    answer   [0, 128, result:2, epoch:4, address:4]
//! map UDP (1) / TCP (2)
//!                    request  [0, op, 0, 0, internal:2, external:2, lifetime:4]
//!                    answer   [0, 128+op, result:2, epoch:4, internal:2, external:2, lifetime:4]
//! ```
//!
//! Every number is big-endian. `epoch` is the router's "seconds since start
//! of epoch": the time since its mapping table was last emptied, which is how
//! a client learns that the router restarted and forgot its mappings
//! ([`router_restarted`]).

use core::net::Ipv4Addr;
use core::time::Duration;

use crate::Protocol;

/// The router's NAT-PMP port.
pub const PORT: u16 = 5351;

/// The protocol version a request carries, and its answer: 0.
const VERSION: u8 = 0;

/// Ask for the router's internet address.
const OP_EXTERNAL_ADDRESS: u8 = 0;
/// Map a UDP port.
const OP_MAP_UDP: u8 = 1;
/// Map a TCP port.
const OP_MAP_TCP: u8 = 2;

/// Set in an answer's opcode: the answer to the request with the rest of it.
const ANSWER: u8 = 128;

/// The lifetime RFC 6886 §3.3 recommends a client ask for, in seconds: two
/// hours. A mapping is asked for again when half of what the router granted
/// has passed.
pub const LIFETIME: u32 = 7200;

/// How many times a request is sent before the router is taken not to be
/// answering. RFC 6886 §3.1 asks for nine sends, the last waited on for 64
/// seconds -- over two minutes in all -- before a client concludes that a
/// router does not speak NAT-PMP. Four sends, 3.75 seconds, is what this
/// crate waits: a router on the same network that has not answered by then
/// is not going to, and the service asks again at its next check rather than
/// stopping everything else for two minutes.
pub const SENDS: u32 = 4;

/// How long to wait for an answer after send number `send` (0 for the
/// first): 250 ms, doubled after each send (RFC 6886 §3.1).
#[must_use]
pub fn wait_after(send: u32) -> Duration {
    Duration::from_millis(250u64 << send.min(8))
}

/// A request for the router's internet address.
#[must_use]
pub const fn external_address_request() -> [u8; 2] {
    [VERSION, OP_EXTERNAL_ADDRESS]
}

/// The opcode that maps `protocol`.
const fn map_opcode(protocol: Protocol) -> u8 {
    match protocol {
        Protocol::Udp => OP_MAP_UDP,
        Protocol::Tcp => OP_MAP_TCP,
    }
}

/// A request to map `external_port` on the router to `internal_port` here,
/// for `lifetime` seconds. The external port is a suggestion: a router may
/// grant another (RFC 6886 §3.3), and the answer says which.
#[must_use]
pub const fn map_request(
    protocol: Protocol,
    internal_port: u16,
    external_port: u16,
    lifetime: u32,
) -> [u8; 12] {
    let [i0, i1] = internal_port.to_be_bytes();
    let [e0, e1] = external_port.to_be_bytes();
    let [l0, l1, l2, l3] = lifetime.to_be_bytes();
    [
        VERSION,
        map_opcode(protocol),
        0,
        0,
        i0,
        i1,
        e0,
        e1,
        l0,
        l1,
        l2,
        l3,
    ]
}

/// A request to remove the mapping of `internal_port` (RFC 6886 §3.4): a map
/// request with lifetime 0 and external port 0.
#[must_use]
pub const fn unmap_request(protocol: Protocol, internal_port: u16) -> [u8; 12] {
    map_request(protocol, internal_port, 0, 0)
}

/// Why a router refused a request: an answer's result code (RFC 6886 §3.5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// 1: it does not speak this version of the protocol.
    UnsupportedVersion,
    /// 2: it can map ports, but will not -- usually because its owner turned
    /// the feature off.
    NotAuthorized,
    /// 3: it has no internet connection of its own to map to (it has no
    /// address from its provider yet, say).
    NetworkFailure,
    /// 4: it cannot make any more mappings now.
    OutOfResources,
    /// 5: it does not know the request.
    UnsupportedOpcode,
    /// A code RFC 6886 does not define.
    Other(u16),
}

impl Refusal {
    /// The refusal a result code names; `None` for 0, which is success.
    #[must_use]
    pub const fn from_code(code: u16) -> Option<Self> {
        Some(match code {
            0 => return None,
            1 => Self::UnsupportedVersion,
            2 => Self::NotAuthorized,
            3 => Self::NetworkFailure,
            4 => Self::OutOfResources,
            5 => Self::UnsupportedOpcode,
            other => Self::Other(other),
        })
    }

    /// The result code.
    #[must_use]
    pub const fn code(self) -> u16 {
        match self {
            Self::UnsupportedVersion => 1,
            Self::NotAuthorized => 2,
            Self::NetworkFailure => 3,
            Self::OutOfResources => 4,
            Self::UnsupportedOpcode => 5,
            Self::Other(code) => code,
        }
    }

    /// What it means, for a person: a clause that follows "the router".
    #[must_use]
    pub fn meaning(self) -> String {
        match self {
            Self::UnsupportedVersion => "does not speak this version of NAT-PMP".to_owned(),
            Self::NotAuthorized => {
                "can map ports but will not: its owner may have turned the feature off".to_owned()
            }
            Self::NetworkFailure => {
                "has no internet connection of its own to map to (it may still be waiting for an \
                 address from its provider)"
                    .to_owned()
            }
            Self::OutOfResources => "cannot make any more mappings now".to_owned(),
            Self::UnsupportedOpcode => "does not know the request".to_owned(),
            Self::Other(code) => format!("refused with NAT-PMP result code {code}"),
        }
    }
}

/// A router's refusal, with the epoch its answer carried.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Refused {
    /// Why.
    pub refusal: Refusal,
    /// The answer's seconds since start of epoch.
    pub epoch: u32,
}

/// The router's internet address.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExternalAddress {
    /// The answer's seconds since start of epoch.
    pub epoch: u32,
    /// The address.
    pub address: Ipv4Addr,
}

/// A mapping the router made (or, for an unmap request, removed).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Granted {
    /// The answer's seconds since start of epoch.
    pub epoch: u32,
    /// The port here.
    pub internal_port: u16,
    /// The port on the router's internet side: the one asked for, or
    /// another the router chose.
    pub external_port: u16,
    /// How long the mapping lasts, in seconds: what was asked for, or less.
    pub lifetime: u32,
}

/// An answer's result code and epoch, if `bytes` is an answer to opcode `op`.
fn header(bytes: &[u8], op: u8) -> Option<(u16, u32)> {
    let [version, opcode, r0, r1, e0, e1, e2, e3, ..] = *bytes else {
        return None;
    };
    (version == VERSION && opcode == (ANSWER | op)).then(|| {
        (
            u16::from_be_bytes([r0, r1]),
            u32::from_be_bytes([e0, e1, e2, e3]),
        )
    })
}

/// Read an answer to [`external_address_request`]: `None` if `bytes` is not
/// one, to be ignored while waiting on.
///
/// A refusal may come as the eight bytes of the header alone; an address
/// needs all twelve.
#[must_use]
pub fn read_external_address(bytes: &[u8]) -> Option<Result<ExternalAddress, Refused>> {
    let (code, epoch) = header(bytes, OP_EXTERNAL_ADDRESS)?;
    if let Some(refusal) = Refusal::from_code(code) {
        return Some(Err(Refused { refusal, epoch }));
    }
    let [a, b, c, d] = *bytes.get(8..12)? else {
        return None;
    };
    Some(Ok(ExternalAddress {
        epoch,
        address: Ipv4Addr::new(a, b, c, d),
    }))
}

/// Read an answer to a [`map_request`] or [`unmap_request`] for `protocol`'s
/// `internal_port`: `None` if `bytes` is not one -- including an answer about
/// another port, which is a late answer to an earlier request -- to be
/// ignored while waiting on.
#[must_use]
pub fn read_map(
    bytes: &[u8],
    protocol: Protocol,
    internal_port: u16,
) -> Option<Result<Granted, Refused>> {
    let (code, epoch) = header(bytes, map_opcode(protocol))?;
    let rest = bytes.get(8..16);
    if let Some(refusal) = Refusal::from_code(code) {
        // The port is echoed in a refusal too; one that names another port
        // is not about this request. Eight bytes alone are taken as they
        // are: there is nothing in them to tell.
        if let Some(&[i0, i1, ..]) = rest
            && u16::from_be_bytes([i0, i1]) != internal_port
        {
            return None;
        }
        return Some(Err(Refused { refusal, epoch }));
    }
    let [i0, i1, x0, x1, l0, l1, l2, l3] = *rest? else {
        return None;
    };
    let internal = u16::from_be_bytes([i0, i1]);
    (internal == internal_port).then_some(Ok(Granted {
        epoch,
        internal_port: internal,
        external_port: u16::from_be_bytes([x0, x1]),
        lifetime: u32::from_be_bytes([l0, l1, l2, l3]),
    }))
}

/// Whether the router lost its mappings between two answers, by RFC 6886
/// §3.6's test: `earlier_epoch` was read at `earlier_at` and `epoch` at `at`,
/// both by this host's clock in seconds. A router's epoch counts up one a
/// second; one that is more than two seconds behind seven-eighths of the
/// time this host saw pass -- the allowance for two clocks that disagree --
/// has started again, and every mapping must be asked for again.
#[must_use]
pub fn router_restarted(earlier_epoch: u32, earlier_at: u64, epoch: u32, at: u64) -> bool {
    let elapsed = at.saturating_sub(earlier_at);
    let expected = u64::from(earlier_epoch).saturating_add(elapsed.saturating_mul(7) / 8);
    u64::from(epoch).saturating_add(2) < expected
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_are_rfc_6886s_bytes() {
        assert_eq!(external_address_request(), [0, 0]);
        // §3.3's layout: version, opcode, reserved, internal, external,
        // lifetime -- all big-endian.
        assert_eq!(
            map_request(Protocol::Tcp, 22, 2222, 7200),
            [0, 2, 0, 0, 0, 22, 0x08, 0xae, 0, 0, 0x1c, 0x20]
        );
        assert_eq!(
            map_request(Protocol::Udp, 0x1234, 0x5678, 0x0102_0304),
            [0, 1, 0, 0, 0x12, 0x34, 0x56, 0x78, 1, 2, 3, 4]
        );
        // §3.4: removal is lifetime 0 and external port 0.
        assert_eq!(
            unmap_request(Protocol::Udp, 51413),
            [0, 1, 0, 0, 0xc8, 0xd5, 0, 0, 0, 0, 0, 0]
        );
    }

    #[test]
    fn an_address_answer_is_read() {
        let answer = [0, 128, 0, 0, 0, 0, 0x01, 0x00, 203, 0, 113, 7];
        assert_eq!(
            read_external_address(&answer),
            Some(Ok(ExternalAddress {
                epoch: 256,
                address: Ipv4Addr::new(203, 0, 113, 7),
            }))
        );
        // A longer datagram is read for its first twelve bytes.
        let mut long = answer.to_vec();
        long.extend_from_slice(&[9, 9]);
        assert!(matches!(read_external_address(&long), Some(Ok(_))));
    }

    #[test]
    fn a_refusal_is_read_with_or_without_the_rest() {
        let refused = Some(Err(Refused {
            refusal: Refusal::NetworkFailure,
            epoch: 5,
        }));
        assert_eq!(
            read_external_address(&[0, 128, 0, 3, 0, 0, 0, 5, 0, 0, 0, 0]),
            refused
        );
        assert_eq!(read_external_address(&[0, 128, 0, 3, 0, 0, 0, 5]), refused);
    }

    #[test]
    fn what_is_not_an_answer_is_ignored() {
        // Too short, the wrong version, the request itself, an answer to a
        // map request, and a success too short to hold its address.
        for bytes in [
            &[][..],
            &[0, 128, 0, 0, 0, 0, 0][..],
            &[1, 128, 0, 0, 0, 0, 0, 0, 1, 2, 3, 4][..],
            &[0, 0, 0, 0, 0, 0, 0, 0, 1, 2, 3, 4][..],
            &[0, 129, 0, 0, 0, 0, 0, 0, 1, 2, 3, 4][..],
            &[0, 128, 0, 0, 0, 0, 0, 0, 1, 2, 3][..],
        ] {
            assert_eq!(read_external_address(bytes), None, "{bytes:?}");
        }
    }

    #[test]
    fn a_map_answer_is_read_for_its_own_port_only() {
        let answer = [
            0, 130, 0, 0, 0, 0, 0, 9, 0, 22, 0x08, 0xae, 0, 0, 0x0e, 0x10,
        ];
        assert_eq!(
            read_map(&answer, Protocol::Tcp, 22),
            Some(Ok(Granted {
                epoch: 9,
                internal_port: 22,
                external_port: 2222,
                lifetime: 3600,
            }))
        );
        // The same answer is not one about UDP, nor about another port.
        assert_eq!(read_map(&answer, Protocol::Udp, 22), None);
        assert_eq!(read_map(&answer, Protocol::Tcp, 80), None);
        // Nor is an answer cut short.
        assert_eq!(
            read_map(answer.get(..15).unwrap_or(&[]), Protocol::Tcp, 22),
            None
        );
    }

    #[test]
    fn a_map_refusal_names_its_port_or_nothing() {
        let about_22 = [0, 130, 0, 2, 0, 0, 0, 9, 0, 22, 0, 0, 0, 0, 0, 0];
        let refused = Some(Err(Refused {
            refusal: Refusal::NotAuthorized,
            epoch: 9,
        }));
        assert_eq!(read_map(&about_22, Protocol::Tcp, 22), refused);
        assert_eq!(read_map(&about_22, Protocol::Tcp, 23), None);
        assert_eq!(
            read_map(about_22.get(..8).unwrap_or(&[]), Protocol::Tcp, 23),
            refused
        );
    }

    #[test]
    fn result_codes_round_trip_and_mean_something() {
        assert_eq!(Refusal::from_code(0), None);
        for code in 1..=7 {
            let refusal = Refusal::from_code(code);
            assert_eq!(refusal.map(Refusal::code), Some(code));
            assert!(refusal.is_some_and(|r| !r.meaning().is_empty()));
        }
        assert_eq!(Refusal::from_code(6), Some(Refusal::Other(6)));
    }

    #[test]
    fn the_waits_double_from_a_quarter_second() {
        let waits: Vec<u128> = (0..SENDS).map(|s| wait_after(s).as_millis()).collect();
        assert_eq!(waits, [250, 500, 1000, 2000]);
        // RFC 6886's ninth and last wait is 64 seconds, and none is longer.
        assert_eq!(wait_after(8), Duration::from_secs(64));
        assert_eq!(wait_after(20), Duration::from_secs(64));
    }

    #[test]
    fn a_restart_is_an_epoch_that_fell_behind_the_clock() {
        // 100 s later the epoch should read about 1000 + 87.5.
        assert!(!router_restarted(1000, 50, 1100, 150));
        assert!(
            !router_restarted(1000, 50, 1086, 150),
            "within the allowance"
        );
        assert!(router_restarted(1000, 50, 1084, 150));
        assert!(router_restarted(1000, 50, 3, 150), "started again from 0");
        // A clock that went back is no evidence either way.
        assert!(!router_restarted(1000, 150, 1000, 50));
    }
}
