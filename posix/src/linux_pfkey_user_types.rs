//! `<linux/pfkeyv2.h>` — PF_KEY v2 socket ABI for IPsec key management.
//!
//! `PF_KEY` is the RFC 2367 socket family used by `racoon`,
//! `strongSwan`'s starter, and `setkey(8)` to install IPsec SA/SP
//! entries into the kernel. The kernel speaks the SADB v2 message
//! format on top of an `SOCK_RAW` socket in the `PF_KEY` family.

// ---------------------------------------------------------------------------
// Address family and version
// ---------------------------------------------------------------------------

pub const AF_KEY: u32 = 15;
pub const PF_KEY: u32 = AF_KEY;
pub const PF_KEY_V2: u32 = 2;

// ---------------------------------------------------------------------------
// SADB message types (`sadb_msg.sadb_msg_type`)
// ---------------------------------------------------------------------------

pub const SADB_RESERVED: u32 = 0;
pub const SADB_GETSPI: u32 = 1;
pub const SADB_UPDATE: u32 = 2;
pub const SADB_ADD: u32 = 3;
pub const SADB_DELETE: u32 = 4;
pub const SADB_GET: u32 = 5;
pub const SADB_ACQUIRE: u32 = 6;
pub const SADB_REGISTER: u32 = 7;
pub const SADB_EXPIRE: u32 = 8;
pub const SADB_FLUSH: u32 = 9;
pub const SADB_DUMP: u32 = 10;
pub const SADB_X_PROMISC: u32 = 11;
pub const SADB_X_PCHANGE: u32 = 12;
pub const SADB_X_SPDUPDATE: u32 = 13;
pub const SADB_X_SPDADD: u32 = 14;
pub const SADB_X_SPDDELETE: u32 = 15;
pub const SADB_X_SPDGET: u32 = 16;
pub const SADB_X_SPDACQUIRE: u32 = 17;
pub const SADB_X_SPDDUMP: u32 = 18;
pub const SADB_X_SPDFLUSH: u32 = 19;
pub const SADB_X_SPDSETIDX: u32 = 20;
pub const SADB_X_SPDEXPIRE: u32 = 21;
pub const SADB_X_SPDDELETE2: u32 = 22;
pub const SADB_X_NAT_T_NEW_MAPPING: u32 = 23;
pub const SADB_X_MIGRATE: u32 = 24;
pub const SADB_MAX: u32 = 24;

// ---------------------------------------------------------------------------
// SA states (`sadb_sa.sadb_sa_state`)
// ---------------------------------------------------------------------------

pub const SADB_SASTATE_LARVAL: u32 = 0;
pub const SADB_SASTATE_MATURE: u32 = 1;
pub const SADB_SASTATE_DYING: u32 = 2;
pub const SADB_SASTATE_DEAD: u32 = 3;
pub const SADB_SASTATE_MAX: u32 = 3;

// ---------------------------------------------------------------------------
// SA types (the IPsec protocol the SA carries)
// ---------------------------------------------------------------------------

pub const SADB_SATYPE_UNSPEC: u32 = 0;
pub const SADB_SATYPE_AH: u32 = 2;
pub const SADB_SATYPE_ESP: u32 = 3;
pub const SADB_SATYPE_RSVP: u32 = 5;
pub const SADB_SATYPE_OSPFV2: u32 = 6;
pub const SADB_SATYPE_RIPV2: u32 = 7;
pub const SADB_SATYPE_MIP: u32 = 8;
pub const SADB_X_SATYPE_IPCOMP: u32 = 9;
pub const SADB_SATYPE_MAX: u32 = 9;

// ---------------------------------------------------------------------------
// Authentication algorithms (`sadb_sa_auth`)
// ---------------------------------------------------------------------------

pub const SADB_AALG_NONE: u32 = 0;
pub const SADB_AALG_MD5HMAC: u32 = 2;
pub const SADB_AALG_SHA1HMAC: u32 = 3;
pub const SADB_X_AALG_SHA2_256HMAC: u32 = 5;
pub const SADB_X_AALG_SHA2_384HMAC: u32 = 6;
pub const SADB_X_AALG_SHA2_512HMAC: u32 = 7;
pub const SADB_X_AALG_RIPEMD160HMAC: u32 = 8;
pub const SADB_X_AALG_AES_XCBC_MAC: u32 = 9;
pub const SADB_X_AALG_SM3_256HMAC: u32 = 10;
pub const SADB_X_AALG_NULL: u32 = 251;
pub const SADB_AALG_MAX: u32 = 251;

// ---------------------------------------------------------------------------
// Encryption algorithms (`sadb_sa_encrypt`)
// ---------------------------------------------------------------------------

pub const SADB_EALG_NONE: u32 = 0;
pub const SADB_EALG_DESCBC: u32 = 2;
pub const SADB_EALG_3DESCBC: u32 = 3;
pub const SADB_X_EALG_CASTCBC: u32 = 6;
pub const SADB_X_EALG_BLOWFISHCBC: u32 = 7;
pub const SADB_EALG_NULL: u32 = 11;
pub const SADB_X_EALG_AESCBC: u32 = 12;
pub const SADB_X_EALG_AESCTR: u32 = 13;
pub const SADB_X_EALG_AES_CCM_ICV8: u32 = 14;
pub const SADB_X_EALG_AES_CCM_ICV12: u32 = 15;
pub const SADB_X_EALG_AES_CCM_ICV16: u32 = 16;
pub const SADB_X_EALG_AES_GCM_ICV8: u32 = 18;
pub const SADB_X_EALG_AES_GCM_ICV12: u32 = 19;
pub const SADB_X_EALG_AES_GCM_ICV16: u32 = 20;
pub const SADB_X_EALG_CAMELLIACBC: u32 = 22;
pub const SADB_X_EALG_NULL_AES_GMAC: u32 = 23;
pub const SADB_X_EALG_SM4CBC: u32 = 24;
pub const SADB_EALG_MAX: u32 = 253;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_af_pf_alias_and_version() {
        // PF_KEY is just AF_KEY by another name, and we speak v2.
        assert_eq!(AF_KEY, 15);
        assert_eq!(PF_KEY, AF_KEY);
        assert_eq!(PF_KEY_V2, 2);
    }

    #[test]
    fn test_sadb_msg_types_dense_0_to_24() {
        let m = [
            SADB_RESERVED,
            SADB_GETSPI,
            SADB_UPDATE,
            SADB_ADD,
            SADB_DELETE,
            SADB_GET,
            SADB_ACQUIRE,
            SADB_REGISTER,
            SADB_EXPIRE,
            SADB_FLUSH,
            SADB_DUMP,
            SADB_X_PROMISC,
            SADB_X_PCHANGE,
            SADB_X_SPDUPDATE,
            SADB_X_SPDADD,
            SADB_X_SPDDELETE,
            SADB_X_SPDGET,
            SADB_X_SPDACQUIRE,
            SADB_X_SPDDUMP,
            SADB_X_SPDFLUSH,
            SADB_X_SPDSETIDX,
            SADB_X_SPDEXPIRE,
            SADB_X_SPDDELETE2,
            SADB_X_NAT_T_NEW_MAPPING,
            SADB_X_MIGRATE,
        ];
        for (i, &v) in m.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
        assert_eq!(SADB_MAX, SADB_X_MIGRATE);
    }

    #[test]
    fn test_sa_states_dense_0_to_3() {
        let s = [
            SADB_SASTATE_LARVAL,
            SADB_SASTATE_MATURE,
            SADB_SASTATE_DYING,
            SADB_SASTATE_DEAD,
        ];
        for (i, &v) in s.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
        assert_eq!(SADB_SASTATE_MAX, 3);
    }

    #[test]
    fn test_satype_anchors_match_rfc2367() {
        // RFC 2367 fixes AH = 2 and ESP = 3.
        assert_eq!(SADB_SATYPE_AH, 2);
        assert_eq!(SADB_SATYPE_ESP, 3);
        assert_eq!(SADB_SATYPE_UNSPEC, 0);
        assert_eq!(SADB_X_SATYPE_IPCOMP, 9);
        assert_eq!(SADB_SATYPE_MAX, 9);
    }

    #[test]
    fn test_aalg_anchors() {
        // RFC 2367 fixes the HMAC-MD5 / HMAC-SHA1 numbers.
        assert_eq!(SADB_AALG_MD5HMAC, 2);
        assert_eq!(SADB_AALG_SHA1HMAC, 3);
        // NULL auth uses 251 — IANA-private high value.
        assert_eq!(SADB_X_AALG_NULL, 251);
        assert_eq!(SADB_AALG_MAX, 251);
    }

    #[test]
    fn test_ealg_aes_gcm_icv_progression() {
        // The three GCM ICV-length variants are consecutive integers.
        let g = [
            SADB_X_EALG_AES_GCM_ICV8,
            SADB_X_EALG_AES_GCM_ICV12,
            SADB_X_EALG_AES_GCM_ICV16,
        ];
        for win in g.windows(2) {
            assert_eq!(win[1], win[0] + 1);
        }
        // CBC and CCM lookups too.
        assert_eq!(SADB_EALG_DESCBC, 2);
        assert_eq!(SADB_EALG_3DESCBC, 3);
        assert_eq!(SADB_EALG_NULL, 11);
        assert_eq!(SADB_X_EALG_AESCBC, 12);
    }
}
