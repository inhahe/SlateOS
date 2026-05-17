//! `<linux/nvme-tcp.h>` — NVMe over TCP transport constants.
//!
//! NVMe/TCP (NVM Express over TCP) allows access to NVMe storage
//! devices over a TCP/IP network without requiring RDMA hardware.
//! It is part of the NVMe-oF (over Fabrics) family of transports.

// ---------------------------------------------------------------------------
// PDU types (Protocol Data Unit)
// ---------------------------------------------------------------------------

/// IC (Initialize Connection) request.
pub const NVME_TCP_PDU_ICREQ: u8 = 0x00;
/// IC response.
pub const NVME_TCP_PDU_ICRESP: u8 = 0x01;
/// H2C (Host to Controller) terminate.
pub const NVME_TCP_PDU_H2C_TERM: u8 = 0x02;
/// C2H (Controller to Host) terminate.
pub const NVME_TCP_PDU_C2H_TERM: u8 = 0x03;
/// Command capsule (host → controller).
pub const NVME_TCP_PDU_CMD: u8 = 0x04;
/// Response capsule (controller → host).
pub const NVME_TCP_PDU_RSP: u8 = 0x05;
/// H2C data transfer.
pub const NVME_TCP_PDU_H2C_DATA: u8 = 0x06;
/// C2H data transfer.
pub const NVME_TCP_PDU_C2H_DATA: u8 = 0x07;
/// R2T (Ready to Transfer).
pub const NVME_TCP_PDU_R2T: u8 = 0x09;

// ---------------------------------------------------------------------------
// PDU header flags
// ---------------------------------------------------------------------------

/// Header digest present.
pub const NVME_TCP_F_HDGST: u8 = 1 << 0;
/// Data digest present.
pub const NVME_TCP_F_DDGST: u8 = 1 << 1;
/// Last PDU in a data sequence.
pub const NVME_TCP_F_DATA_LAST: u8 = 1 << 2;
/// Success flag (C2H data).
pub const NVME_TCP_F_DATA_SUCCESS: u8 = 1 << 3;

// ---------------------------------------------------------------------------
// Digest types
// ---------------------------------------------------------------------------

/// CRC-32C digest.
pub const NVME_TCP_DIGEST_CRC32C: u8 = 0x01;
/// No digest.
pub const NVME_TCP_DIGEST_NONE: u8 = 0x00;

// ---------------------------------------------------------------------------
// Header/data lengths
// ---------------------------------------------------------------------------

/// IC request PDU header length.
pub const NVME_TCP_ICREQ_HDR_LEN: u16 = 128;
/// Common PDU header length.
pub const NVME_TCP_HDR_LEN: u16 = 8;
/// Digest length (CRC-32C = 4 bytes).
pub const NVME_TCP_DIGEST_LEN: u16 = 4;

// ---------------------------------------------------------------------------
// Connection parameters
// ---------------------------------------------------------------------------

/// Default NVMe/TCP port.
pub const NVME_TCP_DISC_PORT: u16 = 8009;
/// Maximum PDU data length (per spec).
pub const NVME_TCP_MAX_PDU_DATA: u32 = 0x00100000;
/// Minimum MAXH2CDATA (per spec).
pub const NVME_TCP_MIN_MAXH2CDATA: u32 = 4096;

// ---------------------------------------------------------------------------
// Fatal error codes (terminate reasons)
// ---------------------------------------------------------------------------

/// Invalid PDU header digest.
pub const NVME_TCP_TERM_HDGST_ERR: u16 = 0x01;
/// Invalid PDU data digest.
pub const NVME_TCP_TERM_DDGST_ERR: u16 = 0x02;
/// Unsupported parameter.
pub const NVME_TCP_TERM_UNSUPPORTED: u16 = 0x03;
/// Invalid data offset.
pub const NVME_TCP_TERM_DATA_OFFSET: u16 = 0x04;
/// R2T limit exceeded.
pub const NVME_TCP_TERM_R2T_LIMIT: u16 = 0x05;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pdu_types_distinct() {
        let types = [
            NVME_TCP_PDU_ICREQ, NVME_TCP_PDU_ICRESP,
            NVME_TCP_PDU_H2C_TERM, NVME_TCP_PDU_C2H_TERM,
            NVME_TCP_PDU_CMD, NVME_TCP_PDU_RSP,
            NVME_TCP_PDU_H2C_DATA, NVME_TCP_PDU_C2H_DATA,
            NVME_TCP_PDU_R2T,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_pdu_flags_no_overlap() {
        let flags = [
            NVME_TCP_F_HDGST, NVME_TCP_F_DDGST,
            NVME_TCP_F_DATA_LAST, NVME_TCP_F_DATA_SUCCESS,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_digest_types_distinct() {
        assert_ne!(NVME_TCP_DIGEST_CRC32C, NVME_TCP_DIGEST_NONE);
    }

    #[test]
    fn test_term_reasons_distinct() {
        let reasons = [
            NVME_TCP_TERM_HDGST_ERR, NVME_TCP_TERM_DDGST_ERR,
            NVME_TCP_TERM_UNSUPPORTED, NVME_TCP_TERM_DATA_OFFSET,
            NVME_TCP_TERM_R2T_LIMIT,
        ];
        for i in 0..reasons.len() {
            for j in (i + 1)..reasons.len() {
                assert_ne!(reasons[i], reasons[j]);
            }
        }
    }

    #[test]
    fn test_default_port() {
        assert_eq!(NVME_TCP_DISC_PORT, 8009);
    }
}
