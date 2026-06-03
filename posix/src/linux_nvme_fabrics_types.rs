//! `<linux/nvme.h>` — NVMe-over-Fabrics (NVMe-oF) command constants.
//!
//! NVMe-oF extends the NVMe command set across network transports
//! (RDMA, TCP, FC). The kernel's nvme-fabrics driver uses the
//! constants below for the Fabrics command set, discovery log pages,
//! and the property registers exposed over the network transport.

// ---------------------------------------------------------------------------
// Fabrics command opcodes (NVME_FABRICS_TYPE_*)
// ---------------------------------------------------------------------------

/// Fabrics command opcode (Admin opcode 0x7f means "Fabrics command";
/// the type byte selects the actual command below).
pub const NVME_FABRICS_OPCODE: u8 = 0x7f;

/// Property set (write to a controller register).
pub const NVME_FABRICS_TYPE_PROPERTY_SET: u8 = 0x00;
/// Connect (establish a fabrics queue).
pub const NVME_FABRICS_TYPE_CONNECT: u8 = 0x01;
/// Property get (read a controller register).
pub const NVME_FABRICS_TYPE_PROPERTY_GET: u8 = 0x04;
/// Authentication send.
pub const NVME_FABRICS_TYPE_AUTH_SEND: u8 = 0x05;
/// Authentication receive.
pub const NVME_FABRICS_TYPE_AUTH_RECV: u8 = 0x06;
/// Disconnect (tear down a fabrics queue).
pub const NVME_FABRICS_TYPE_DISCONNECT: u8 = 0x08;

// ---------------------------------------------------------------------------
// Discovery log entry — TRTYPE (transport type)
// ---------------------------------------------------------------------------

/// Discovery log entry refers to an RDMA transport.
pub const NVMF_TRTYPE_RDMA: u8 = 1;
/// Fibre Channel transport.
pub const NVMF_TRTYPE_FC: u8 = 2;
/// TCP transport.
pub const NVMF_TRTYPE_TCP: u8 = 3;
/// Intra-host (loop / loopback) transport.
pub const NVMF_TRTYPE_LOOP: u8 = 254;

// ---------------------------------------------------------------------------
// Discovery log entry — ADRFAM (address family)
// ---------------------------------------------------------------------------

/// IPv4 address family.
pub const NVMF_ADDR_FAMILY_IP4: u8 = 1;
/// IPv6 address family.
pub const NVMF_ADDR_FAMILY_IP6: u8 = 2;
/// Infiniband.
pub const NVMF_ADDR_FAMILY_IB: u8 = 3;
/// Fibre Channel.
pub const NVMF_ADDR_FAMILY_FC: u8 = 4;
/// Loop/intra-host.
pub const NVMF_ADDR_FAMILY_LOOP: u8 = 254;

// ---------------------------------------------------------------------------
// Discovery log entry — SUBTYPE
// ---------------------------------------------------------------------------

/// Referral to another discovery subsystem.
pub const NVME_NQN_DISC: u8 = 1;
/// NVM (block storage) subsystem.
pub const NVME_NQN_NVME: u8 = 2;
/// Discovery subsystem capable of current-discovery (NVMe TP 8010).
pub const NVME_NQN_CURR: u8 = 3;

// ---------------------------------------------------------------------------
// Property register offsets (4-byte addresses inside the fabrics
// controller property space)
// ---------------------------------------------------------------------------

/// Controller capabilities (CAP, 64-bit).
pub const NVME_REG_CAP: u32 = 0x0000;
/// Version (VS).
pub const NVME_REG_VS: u32 = 0x0008;
/// Controller configuration (CC).
pub const NVME_REG_CC: u32 = 0x0014;
/// Controller status (CSTS).
pub const NVME_REG_CSTS: u32 = 0x001c;
/// Reset register (NSSR).
pub const NVME_REG_NSSR: u32 = 0x0020;

// ---------------------------------------------------------------------------
// NQN size limits
// ---------------------------------------------------------------------------

/// Maximum NQN length (NVMe spec — 223 bytes plus NUL terminator).
pub const NVMF_NQN_FIELD_LEN: u32 = 256;
/// Standard discovery-service NQN string.
pub const NVME_DISC_SUBSYS_NAME: &str =
    "nqn.2014-08.org.nvmexpress.discovery";

// ---------------------------------------------------------------------------
// Connect/disconnect status (low byte of SCT/SC field, type "Generic
// Command Status")
// ---------------------------------------------------------------------------

/// Connect command — invalid parameter in connect data.
pub const NVME_SC_CONNECT_INVALID_PARAM: u16 = 0x0480;
/// Connect — connection already in use.
pub const NVME_SC_CONNECT_RESTART_DISC: u16 = 0x0481;
/// Connect — invalid host.
pub const NVME_SC_CONNECT_INVALID_HOST: u16 = 0x0482;
/// Discover restart required.
pub const NVME_SC_DISCOVERY_RESTART: u16 = 0x048a;
/// Authentication required.
pub const NVME_SC_AUTH_REQUIRED: u16 = 0x048b;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fabrics_types_distinct() {
        let t = [
            NVME_FABRICS_TYPE_PROPERTY_SET,
            NVME_FABRICS_TYPE_CONNECT,
            NVME_FABRICS_TYPE_PROPERTY_GET,
            NVME_FABRICS_TYPE_AUTH_SEND,
            NVME_FABRICS_TYPE_AUTH_RECV,
            NVME_FABRICS_TYPE_DISCONNECT,
        ];
        for i in 0..t.len() {
            for j in (i + 1)..t.len() {
                assert_ne!(t[i], t[j]);
            }
        }
        assert_eq!(NVME_FABRICS_OPCODE, 0x7f);
    }

    #[test]
    fn test_trtypes_distinct() {
        let tr = [
            NVMF_TRTYPE_RDMA,
            NVMF_TRTYPE_FC,
            NVMF_TRTYPE_TCP,
            NVMF_TRTYPE_LOOP,
        ];
        for i in 0..tr.len() {
            for j in (i + 1)..tr.len() {
                assert_ne!(tr[i], tr[j]);
            }
        }
    }

    #[test]
    fn test_address_families_distinct() {
        let af = [
            NVMF_ADDR_FAMILY_IP4,
            NVMF_ADDR_FAMILY_IP6,
            NVMF_ADDR_FAMILY_IB,
            NVMF_ADDR_FAMILY_FC,
            NVMF_ADDR_FAMILY_LOOP,
        ];
        for i in 0..af.len() {
            for j in (i + 1)..af.len() {
                assert_ne!(af[i], af[j]);
            }
        }
    }

    #[test]
    fn test_subtypes_distinct() {
        let st = [NVME_NQN_DISC, NVME_NQN_NVME, NVME_NQN_CURR];
        for i in 0..st.len() {
            for j in (i + 1)..st.len() {
                assert_ne!(st[i], st[j]);
            }
        }
    }

    #[test]
    fn test_register_offsets_distinct_aligned() {
        let regs =
            [NVME_REG_CAP, NVME_REG_VS, NVME_REG_CC, NVME_REG_CSTS, NVME_REG_NSSR];
        for i in 0..regs.len() {
            for j in (i + 1)..regs.len() {
                assert_ne!(regs[i], regs[j]);
            }
            // All property addresses are 4-byte aligned in the NVMe spec.
            assert_eq!(regs[i] % 4, 0);
        }
    }

    #[test]
    fn test_nqn_len_and_disc_string() {
        assert_eq!(NVMF_NQN_FIELD_LEN, 256);
        // Discovery NQN must fit within the field (including NUL).
        assert!(NVME_DISC_SUBSYS_NAME.len() < NVMF_NQN_FIELD_LEN as usize);
        assert!(NVME_DISC_SUBSYS_NAME.starts_with("nqn."));
    }

    #[test]
    fn test_status_codes_distinct() {
        let s = [
            NVME_SC_CONNECT_INVALID_PARAM,
            NVME_SC_CONNECT_RESTART_DISC,
            NVME_SC_CONNECT_INVALID_HOST,
            NVME_SC_DISCOVERY_RESTART,
            NVME_SC_AUTH_REQUIRED,
        ];
        for i in 0..s.len() {
            for j in (i + 1)..s.len() {
                assert_ne!(s[i], s[j]);
            }
            // All connect-family status codes have SCT (status code type)
            // bits 8..11 = 4 (Command Specific Status).
            assert_eq!((s[i] >> 8) & 0xf, 4);
        }
    }
}
