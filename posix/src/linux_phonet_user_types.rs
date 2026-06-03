//! `<linux/phonet.h>` — `AF_PHONET` socket ABI.
//!
//! Phonet is Nokia's ISI bus protocol — used to talk to modems and
//! cellular radios on N-series and N9 devices. Linux added the
//! socket family in 2.6.31 so that `pnatd` and Maemo/MeeGo
//! telephony stacks could speak ISI without going through a vendor
//! daemon.

// ---------------------------------------------------------------------------
// Address family
// ---------------------------------------------------------------------------

pub const AF_PHONET: u32 = 35;
pub const PF_PHONET: u32 = AF_PHONET;

// ---------------------------------------------------------------------------
// Sockaddr layout fields
// ---------------------------------------------------------------------------

/// `sockaddr_pn.spn_family` — always `AF_PHONET`.
pub const SPN_FAMILY_OFFSET: u32 = 0;
/// Length of the Phonet address (`pn_dev` + `pn_obj`).
pub const PN_ADDR_LEN: u32 = 2;

// ---------------------------------------------------------------------------
// Socket protocols
// ---------------------------------------------------------------------------

pub const PN_PROTO_TRANSPORT: u32 = 0;
pub const PN_PROTO_PHONET: u32 = 1;
pub const PN_PROTO_PIPE: u32 = 2;
pub const PHONET_NPROTO: u32 = 3;

// ---------------------------------------------------------------------------
// `SOL_PNPIPE` socket options
// ---------------------------------------------------------------------------

pub const PNPIPE_ENCAP: u32 = 1;
pub const PNPIPE_IFINDEX: u32 = 2;
pub const PNPIPE_HANDLE: u32 = 3;
pub const PNPIPE_INITSTATE: u32 = 4;

pub const PNPIPE_ENCAP_NONE: u32 = 0;
pub const PNPIPE_ENCAP_IP: u32 = 1;

// ---------------------------------------------------------------------------
// ISI resource ids commonly addressed on the bus
// ---------------------------------------------------------------------------

pub const PN_DEV_PC: u8 = 0x10;
pub const PN_DEV_HOST: u8 = 0x00;
pub const PN_DEV_SOS: u8 = 0xFE;

/// Phonet pipe identifiers must fall inside this two-byte range.
pub const PN_PIPE_INVALID_HANDLE: u8 = 0xFF;

// ---------------------------------------------------------------------------
// Netlink ABI for setting Phonet addresses
// ---------------------------------------------------------------------------

pub const PHONET_NLATTR_UNSPEC: u32 = 0;
pub const PHONET_NLATTR_PND_OBJECT: u32 = 1;
pub const PHONET_NLATTR_PND_REMOTE_OBJECT: u32 = 2;
pub const PHONET_NLATTR_PND_REMOTE_DEV: u32 = 3;
pub const PHONET_NLATTR_PND_LOCAL_DEV: u32 = 4;
pub const PHONET_NLATTR_PND_TYPE: u32 = 5;
pub const PHONET_NLATTR_PND_FLAGS: u32 = 6;
pub const PHONET_NLATTR_PND_RXQ: u32 = 7;
pub const PHONET_NLATTR_PND_TXQ: u32 = 8;
pub const PHONET_NLATTR_MAX: u32 = 8;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_af_phonet_is_35() {
        // Phonet got address family 35 when Nokia upstreamed it in 2.6.31.
        assert_eq!(AF_PHONET, 35);
        assert_eq!(PF_PHONET, AF_PHONET);
    }

    #[test]
    fn test_protos_dense_0_to_2() {
        let p = [PN_PROTO_TRANSPORT, PN_PROTO_PHONET, PN_PROTO_PIPE];
        for (i, &v) in p.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
        assert_eq!(PHONET_NPROTO, 3);
    }

    #[test]
    fn test_pnpipe_sockopts_dense_1_to_4() {
        let o = [PNPIPE_ENCAP, PNPIPE_IFINDEX, PNPIPE_HANDLE, PNPIPE_INITSTATE];
        for (i, &v) in o.iter().enumerate() {
            assert_eq!(v as usize, i + 1);
        }
    }

    #[test]
    fn test_pnpipe_encap_modes() {
        assert_eq!(PNPIPE_ENCAP_NONE, 0);
        assert_eq!(PNPIPE_ENCAP_IP, 1);
    }

    #[test]
    fn test_device_ids_distinct() {
        // PC host vs SoS (the modem) — must be unique.
        assert_ne!(PN_DEV_PC, PN_DEV_HOST);
        assert_ne!(PN_DEV_PC, PN_DEV_SOS);
        assert_ne!(PN_DEV_HOST, PN_DEV_SOS);
        assert_eq!(PN_DEV_HOST, 0x00);
        assert_eq!(PN_DEV_SOS, 0xFE);
        // Invalid pipe handle sentinel.
        assert_eq!(PN_PIPE_INVALID_HANDLE, 0xFF);
    }

    #[test]
    fn test_nlattr_dense_0_to_8() {
        let a = [
            PHONET_NLATTR_UNSPEC,
            PHONET_NLATTR_PND_OBJECT,
            PHONET_NLATTR_PND_REMOTE_OBJECT,
            PHONET_NLATTR_PND_REMOTE_DEV,
            PHONET_NLATTR_PND_LOCAL_DEV,
            PHONET_NLATTR_PND_TYPE,
            PHONET_NLATTR_PND_FLAGS,
            PHONET_NLATTR_PND_RXQ,
            PHONET_NLATTR_PND_TXQ,
        ];
        for (i, &v) in a.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
        assert_eq!(PHONET_NLATTR_MAX, 8);
    }

    #[test]
    fn test_addr_layout_matches_2_byte_record() {
        assert_eq!(SPN_FAMILY_OFFSET, 0);
        assert_eq!(PN_ADDR_LEN, 2);
    }
}
