//! `<linux/pfkeyv2.h>` — PF_KEY v2 (IPsec key management) constants.
//!
//! PF_KEY is the socket-based API for managing IPsec Security
//! Associations (SAs) and Security Policies (SPs) in the kernel.
//! IKE daemons (strongSwan, Libreswan, racoon) use PF_KEY to
//! install negotiated keys and configure IPsec tunnel/transport
//! mode. While largely superseded by XFRM netlink for new code,
//! PF_KEY remains supported for compatibility with BSD-derived
//! IPsec implementations.

// ---------------------------------------------------------------------------
// PF_KEY message types (SADB_*)
// ---------------------------------------------------------------------------

/// Reserved (not used).
pub const SADB_RESERVED: u32 = 0;
/// Get SA (Security Association).
pub const SADB_GETSPI: u32 = 1;
/// Update SA.
pub const SADB_UPDATE: u32 = 2;
/// Add SA.
pub const SADB_ADD: u32 = 3;
/// Delete SA.
pub const SADB_DELETE: u32 = 4;
/// Get SA by SPI.
pub const SADB_GET: u32 = 5;
/// Acquire — request SA negotiation.
pub const SADB_ACQUIRE: u32 = 6;
/// Register for ACQUIRE notifications.
pub const SADB_REGISTER: u32 = 7;
/// SA expired.
pub const SADB_EXPIRE: u32 = 8;
/// Flush all SAs.
pub const SADB_FLUSH: u32 = 9;
/// Dump all SAs.
pub const SADB_DUMP: u32 = 10;
/// Add security policy.
pub const SADB_X_SPDADD: u32 = 15;
/// Delete security policy.
pub const SADB_X_SPDDELETE: u32 = 16;
/// Get security policy.
pub const SADB_X_SPDGET: u32 = 17;
/// Dump all security policies.
pub const SADB_X_SPDDUMP: u32 = 19;
/// Flush all security policies.
pub const SADB_X_SPDFLUSH: u32 = 20;
/// NAT-T new mapping notification.
pub const SADB_X_NAT_T_NEW_MAPPING: u32 = 24;
/// Migrate SA.
pub const SADB_X_MIGRATE: u32 = 25;

// ---------------------------------------------------------------------------
// SA types (SADB_SATYPE_*)
// ---------------------------------------------------------------------------

/// Unspecified SA type.
pub const SADB_SATYPE_UNSPEC: u32 = 0;
/// AH (Authentication Header).
pub const SADB_SATYPE_AH: u32 = 2;
/// ESP (Encapsulating Security Payload).
pub const SADB_SATYPE_ESP: u32 = 3;

// ---------------------------------------------------------------------------
// SA states
// ---------------------------------------------------------------------------

/// SA is larval (SPI reserved, no keys yet).
pub const SADB_SASTATE_LARVAL: u32 = 0;
/// SA is mature (has keys, usable).
pub const SADB_SASTATE_MATURE: u32 = 1;
/// SA is dying (soft lifetime expired).
pub const SADB_SASTATE_DYING: u32 = 2;
/// SA is dead (hard lifetime expired).
pub const SADB_SASTATE_DEAD: u32 = 3;

// ---------------------------------------------------------------------------
// Extension types (SADB_EXT_*)
// ---------------------------------------------------------------------------

/// SA extension.
pub const SADB_EXT_SA: u32 = 1;
/// Lifetime current.
pub const SADB_EXT_LIFETIME_CURRENT: u32 = 2;
/// Lifetime hard.
pub const SADB_EXT_LIFETIME_HARD: u32 = 3;
/// Lifetime soft.
pub const SADB_EXT_LIFETIME_SOFT: u32 = 4;
/// Source address.
pub const SADB_EXT_ADDRESS_SRC: u32 = 5;
/// Destination address.
pub const SADB_EXT_ADDRESS_DST: u32 = 6;
/// Encryption key.
pub const SADB_EXT_KEY_AUTH: u32 = 8;
/// Authentication key.
pub const SADB_EXT_KEY_ENCRYPT: u32 = 9;
/// Proposal (for ACQUIRE).
pub const SADB_EXT_PROPOSAL: u32 = 12;
/// Supported algorithms (auth).
pub const SADB_EXT_SUPPORTED_AUTH: u32 = 13;
/// Supported algorithms (encrypt).
pub const SADB_EXT_SUPPORTED_ENCRYPT: u32 = 14;
/// Security policy.
pub const SADB_X_EXT_POLICY: u32 = 18;
/// NAT-T type.
pub const SADB_X_EXT_NAT_T_TYPE: u32 = 19;
/// NAT-T source port.
pub const SADB_X_EXT_NAT_T_SPORT: u32 = 20;
/// NAT-T destination port.
pub const SADB_X_EXT_NAT_T_DPORT: u32 = 21;
/// NAT-T original address.
pub const SADB_X_EXT_NAT_T_OA: u32 = 22;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_msg_types_distinct() {
        let msgs = [
            SADB_RESERVED, SADB_GETSPI, SADB_UPDATE, SADB_ADD,
            SADB_DELETE, SADB_GET, SADB_ACQUIRE, SADB_REGISTER,
            SADB_EXPIRE, SADB_FLUSH, SADB_DUMP,
            SADB_X_SPDADD, SADB_X_SPDDELETE, SADB_X_SPDGET,
            SADB_X_SPDDUMP, SADB_X_SPDFLUSH,
            SADB_X_NAT_T_NEW_MAPPING, SADB_X_MIGRATE,
        ];
        for i in 0..msgs.len() {
            for j in (i + 1)..msgs.len() {
                assert_ne!(msgs[i], msgs[j]);
            }
        }
    }

    #[test]
    fn test_sa_types_distinct() {
        let types = [SADB_SATYPE_UNSPEC, SADB_SATYPE_AH, SADB_SATYPE_ESP];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_sa_states_distinct() {
        let states = [
            SADB_SASTATE_LARVAL, SADB_SASTATE_MATURE,
            SADB_SASTATE_DYING, SADB_SASTATE_DEAD,
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_extensions_distinct() {
        let exts = [
            SADB_EXT_SA, SADB_EXT_LIFETIME_CURRENT,
            SADB_EXT_LIFETIME_HARD, SADB_EXT_LIFETIME_SOFT,
            SADB_EXT_ADDRESS_SRC, SADB_EXT_ADDRESS_DST,
            SADB_EXT_KEY_AUTH, SADB_EXT_KEY_ENCRYPT,
            SADB_EXT_PROPOSAL, SADB_EXT_SUPPORTED_AUTH,
            SADB_EXT_SUPPORTED_ENCRYPT, SADB_X_EXT_POLICY,
            SADB_X_EXT_NAT_T_TYPE, SADB_X_EXT_NAT_T_SPORT,
            SADB_X_EXT_NAT_T_DPORT, SADB_X_EXT_NAT_T_OA,
        ];
        for i in 0..exts.len() {
            for j in (i + 1)..exts.len() {
                assert_ne!(exts[i], exts[j]);
            }
        }
    }
}
