//! `<netinet/sctp.h>` — Stream Control Transmission Protocol ABI.
//!
//! SCTP is the message-oriented, multi-streamed, multi-homed transport
//! defined by RFC 4960. It's the carrier for SS7/Diameter on telecom
//! infrastructure (USP/AAA, IMS), and Linux exposes it as
//! `SOCK_SEQPACKET` over `IPPROTO_SCTP`.

// ---------------------------------------------------------------------------
// Address / protocol family
// ---------------------------------------------------------------------------

pub const IPPROTO_SCTP: u32 = 132;
pub const SOL_SCTP: u32 = 132;

// ---------------------------------------------------------------------------
// SCTP chunk types (`SCTP_CID_*`)
// ---------------------------------------------------------------------------

pub const SCTP_CID_DATA: u8 = 0x00;
pub const SCTP_CID_INIT: u8 = 0x01;
pub const SCTP_CID_INIT_ACK: u8 = 0x02;
pub const SCTP_CID_SACK: u8 = 0x03;
pub const SCTP_CID_HEARTBEAT: u8 = 0x04;
pub const SCTP_CID_HEARTBEAT_ACK: u8 = 0x05;
pub const SCTP_CID_ABORT: u8 = 0x06;
pub const SCTP_CID_SHUTDOWN: u8 = 0x07;
pub const SCTP_CID_SHUTDOWN_ACK: u8 = 0x08;
pub const SCTP_CID_ERROR: u8 = 0x09;
pub const SCTP_CID_COOKIE_ECHO: u8 = 0x0A;
pub const SCTP_CID_COOKIE_ACK: u8 = 0x0B;
pub const SCTP_CID_ECN_ECNE: u8 = 0x0C;
pub const SCTP_CID_ECN_CWR: u8 = 0x0D;
pub const SCTP_CID_SHUTDOWN_COMPLETE: u8 = 0x0E;
pub const SCTP_CID_AUTH: u8 = 0x0F;
pub const SCTP_CID_I_DATA: u8 = 0x40;
pub const SCTP_CID_ASCONF_ACK: u8 = 0x80;
pub const SCTP_CID_RECONF: u8 = 0x82;
pub const SCTP_CID_PAD: u8 = 0x84;
pub const SCTP_CID_FWD_TSN: u8 = 0xC0;
pub const SCTP_CID_ASCONF: u8 = 0xC1;
pub const SCTP_CID_I_FWD_TSN: u8 = 0xC2;

// ---------------------------------------------------------------------------
// Socket options (`SCTP_*`)
// ---------------------------------------------------------------------------

pub const SCTP_RTOINFO: u32 = 0;
pub const SCTP_ASSOCINFO: u32 = 1;
pub const SCTP_INITMSG: u32 = 2;
pub const SCTP_NODELAY: u32 = 3;
pub const SCTP_AUTOCLOSE: u32 = 4;
pub const SCTP_SET_PEER_PRIMARY_ADDR: u32 = 5;
pub const SCTP_PRIMARY_ADDR: u32 = 6;
pub const SCTP_ADAPTATION_LAYER: u32 = 7;
pub const SCTP_DISABLE_FRAGMENTS: u32 = 8;
pub const SCTP_PEER_ADDR_PARAMS: u32 = 9;
pub const SCTP_DEFAULT_SEND_PARAM: u32 = 10;
pub const SCTP_EVENTS: u32 = 11;
pub const SCTP_I_WANT_MAPPED_V4_ADDR: u32 = 12;
pub const SCTP_MAXSEG: u32 = 13;
pub const SCTP_STATUS: u32 = 14;
pub const SCTP_GET_PEER_ADDR_INFO: u32 = 15;
pub const SCTP_DELAYED_SACK: u32 = 16;
pub const SCTP_CONTEXT: u32 = 17;
pub const SCTP_FRAGMENT_INTERLEAVE: u32 = 18;
pub const SCTP_PARTIAL_DELIVERY_POINT: u32 = 19;
pub const SCTP_MAX_BURST: u32 = 20;

// ---------------------------------------------------------------------------
// Association limits / defaults
// ---------------------------------------------------------------------------

/// Default number of outgoing streams per association.
pub const SCTP_DEFAULT_OSTREAMS: u32 = 10;
/// Maximum incoming streams the kernel allocates if peer requests more.
pub const SCTP_DEFAULT_MAX_INSTREAMS: u32 = 65535;
/// RFC 4960 minimum cookie life.
pub const SCTP_RTO_INITIAL_MS: u32 = 3000;
pub const SCTP_RTO_MIN_MS: u32 = 1000;
pub const SCTP_RTO_MAX_MS: u32 = 60_000;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ip_proto_and_sol_match() {
        // IPPROTO_SCTP == SOL_SCTP == 132 (IANA assigned).
        assert_eq!(IPPROTO_SCTP, 132);
        assert_eq!(SOL_SCTP, 132);
        assert_eq!(IPPROTO_SCTP, SOL_SCTP);
    }

    #[test]
    fn test_core_chunk_types_dense_0_to_15() {
        let c = [
            SCTP_CID_DATA,
            SCTP_CID_INIT,
            SCTP_CID_INIT_ACK,
            SCTP_CID_SACK,
            SCTP_CID_HEARTBEAT,
            SCTP_CID_HEARTBEAT_ACK,
            SCTP_CID_ABORT,
            SCTP_CID_SHUTDOWN,
            SCTP_CID_SHUTDOWN_ACK,
            SCTP_CID_ERROR,
            SCTP_CID_COOKIE_ECHO,
            SCTP_CID_COOKIE_ACK,
            SCTP_CID_ECN_ECNE,
            SCTP_CID_ECN_CWR,
            SCTP_CID_SHUTDOWN_COMPLETE,
            SCTP_CID_AUTH,
        ];
        for (i, &v) in c.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }

    #[test]
    fn test_extended_chunk_types_distinct() {
        // Extension chunks have the top action bits set per RFC 4960 §3.2.
        let e = [
            SCTP_CID_I_DATA,
            SCTP_CID_ASCONF_ACK,
            SCTP_CID_RECONF,
            SCTP_CID_PAD,
            SCTP_CID_FWD_TSN,
            SCTP_CID_ASCONF,
            SCTP_CID_I_FWD_TSN,
        ];
        for a in 0..e.len() {
            for b in (a + 1)..e.len() {
                assert_ne!(e[a], e[b]);
            }
        }
        // I_DATA is the modern (RFC 8260) chunk for interleaved streams.
        assert_eq!(SCTP_CID_I_DATA, 0x40);
    }

    #[test]
    fn test_sockopts_dense_0_to_20() {
        let o = [
            SCTP_RTOINFO,
            SCTP_ASSOCINFO,
            SCTP_INITMSG,
            SCTP_NODELAY,
            SCTP_AUTOCLOSE,
            SCTP_SET_PEER_PRIMARY_ADDR,
            SCTP_PRIMARY_ADDR,
            SCTP_ADAPTATION_LAYER,
            SCTP_DISABLE_FRAGMENTS,
            SCTP_PEER_ADDR_PARAMS,
            SCTP_DEFAULT_SEND_PARAM,
            SCTP_EVENTS,
            SCTP_I_WANT_MAPPED_V4_ADDR,
            SCTP_MAXSEG,
            SCTP_STATUS,
            SCTP_GET_PEER_ADDR_INFO,
            SCTP_DELAYED_SACK,
            SCTP_CONTEXT,
            SCTP_FRAGMENT_INTERLEAVE,
            SCTP_PARTIAL_DELIVERY_POINT,
            SCTP_MAX_BURST,
        ];
        for (i, &v) in o.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }

    #[test]
    fn test_rto_bounds_consistent() {
        // RFC 4960 says RTO.Min ≤ RTO.Initial ≤ RTO.Max.
        assert!(SCTP_RTO_MIN_MS <= SCTP_RTO_INITIAL_MS);
        assert!(SCTP_RTO_INITIAL_MS <= SCTP_RTO_MAX_MS);
        // Reference defaults from the RFC.
        assert_eq!(SCTP_RTO_INITIAL_MS, 3000);
        assert_eq!(SCTP_RTO_MIN_MS, 1000);
        assert_eq!(SCTP_RTO_MAX_MS, 60_000);
    }

    #[test]
    fn test_stream_default_and_max() {
        // 10 outgoing streams is the historical Linux default.
        assert_eq!(SCTP_DEFAULT_OSTREAMS, 10);
        // 65535 is the SCTP wire-protocol maximum (16-bit stream id).
        assert_eq!(SCTP_DEFAULT_MAX_INSTREAMS, 65535);
    }
}
