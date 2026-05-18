//! `<linux/pfkeyv2.h>` — SA database message types and constants.
//!
//! PF_KEY v2 protocol constants covering SADB message types,
//! SA types, extension types, and identity types.

// ---------------------------------------------------------------------------
// SADB message types (SADB_*)
// ---------------------------------------------------------------------------

/// Reserved.
pub const SADB_RESERVED: u8 = 0;
/// Get SA.
pub const SADB_GET: u8 = 1;
/// Add SA.
pub const SADB_ADD: u8 = 2;
/// Delete SA.
pub const SADB_DELETE: u8 = 3;
/// Update SA.
pub const SADB_UPDATE: u8 = 4;
/// Dump all SAs.
pub const SADB_DUMP: u8 = 5;
/// Flush all SAs.
pub const SADB_FLUSH: u8 = 6;
/// Acquire SA.
pub const SADB_ACQUIRE: u8 = 7;
/// Register PF_KEY socket.
pub const SADB_REGISTER: u8 = 8;
/// SA expired.
pub const SADB_EXPIRE: u8 = 9;
/// Maximum base message type.
pub const SADB_MAX: u8 = 9;

// ---------------------------------------------------------------------------
// SADB extension types (SADB_EXT_*)
// ---------------------------------------------------------------------------

/// Reserved extension.
pub const SADB_EXT_RESERVED: u16 = 0;
/// SA extension.
pub const SADB_EXT_SA: u16 = 1;
/// Current lifetime.
pub const SADB_EXT_LIFETIME_CURRENT: u16 = 2;
/// Hard lifetime.
pub const SADB_EXT_LIFETIME_HARD: u16 = 3;
/// Soft lifetime.
pub const SADB_EXT_LIFETIME_SOFT: u16 = 4;
/// Source address.
pub const SADB_EXT_ADDRESS_SRC: u16 = 5;
/// Destination address.
pub const SADB_EXT_ADDRESS_DST: u16 = 6;
/// Proxy address.
pub const SADB_EXT_ADDRESS_PROXY: u16 = 7;
/// Authentication key.
pub const SADB_EXT_KEY_AUTH: u16 = 8;
/// Encryption key.
pub const SADB_EXT_KEY_ENCRYPT: u16 = 9;
/// Source identity.
pub const SADB_EXT_IDENTITY_SRC: u16 = 10;
/// Destination identity.
pub const SADB_EXT_IDENTITY_DST: u16 = 11;
/// Sensitivity.
pub const SADB_EXT_SENSITIVITY: u16 = 12;
/// Proposal.
pub const SADB_EXT_PROPOSAL: u16 = 13;
/// Supported auth algorithms.
pub const SADB_EXT_SUPPORTED_AUTH: u16 = 14;
/// Supported encrypt algorithms.
pub const SADB_EXT_SUPPORTED_ENCRYPT: u16 = 15;

// ---------------------------------------------------------------------------
// SADB SA types (SADB_SATYPE_*)
// ---------------------------------------------------------------------------

/// Unspec SA type.
pub const SADB_SATYPE_UNSPEC: u8 = 0;
/// AH (Authentication Header).
pub const SADB_SATYPE_AH: u8 = 2;
/// ESP (Encapsulating Security Payload).
pub const SADB_SATYPE_ESP: u8 = 3;
/// IPCOMP (IP Compression).
pub const SADB_X_SATYPE_IPCOMP: u8 = 9;

// ---------------------------------------------------------------------------
// SADB SA state (SADB_SASTATE_*)
// ---------------------------------------------------------------------------

/// Larval state.
pub const SADB_SASTATE_LARVAL: u8 = 0;
/// Mature state.
pub const SADB_SASTATE_MATURE: u8 = 1;
/// Dying state.
pub const SADB_SASTATE_DYING: u8 = 2;
/// Dead state.
pub const SADB_SASTATE_DEAD: u8 = 3;

// ---------------------------------------------------------------------------
// SADB identity types
// ---------------------------------------------------------------------------

/// Reserved identity type.
pub const SADB_IDENTTYPE_RESERVED: u16 = 0;
/// Prefix identity.
pub const SADB_IDENTTYPE_PREFIX: u16 = 1;
/// FQDN identity.
pub const SADB_IDENTTYPE_FQDN: u16 = 2;
/// User FQDN identity.
pub const SADB_IDENTTYPE_USERFQDN: u16 = 3;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_msg_types_distinct() {
        let types: [u8; 10] = [
            SADB_RESERVED, SADB_GET, SADB_ADD, SADB_DELETE,
            SADB_UPDATE, SADB_DUMP, SADB_FLUSH, SADB_ACQUIRE,
            SADB_REGISTER, SADB_EXPIRE,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_msg_max() {
        assert_eq!(SADB_MAX, SADB_EXPIRE);
    }

    #[test]
    fn test_ext_types_distinct() {
        let exts: [u16; 16] = [
            SADB_EXT_RESERVED, SADB_EXT_SA,
            SADB_EXT_LIFETIME_CURRENT, SADB_EXT_LIFETIME_HARD,
            SADB_EXT_LIFETIME_SOFT, SADB_EXT_ADDRESS_SRC,
            SADB_EXT_ADDRESS_DST, SADB_EXT_ADDRESS_PROXY,
            SADB_EXT_KEY_AUTH, SADB_EXT_KEY_ENCRYPT,
            SADB_EXT_IDENTITY_SRC, SADB_EXT_IDENTITY_DST,
            SADB_EXT_SENSITIVITY, SADB_EXT_PROPOSAL,
            SADB_EXT_SUPPORTED_AUTH, SADB_EXT_SUPPORTED_ENCRYPT,
        ];
        for i in 0..exts.len() {
            for j in (i + 1)..exts.len() {
                assert_ne!(exts[i], exts[j]);
            }
        }
    }

    #[test]
    fn test_sa_types_distinct() {
        let types: [u8; 4] = [
            SADB_SATYPE_UNSPEC, SADB_SATYPE_AH,
            SADB_SATYPE_ESP, SADB_X_SATYPE_IPCOMP,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_sa_states_sequential() {
        assert_eq!(SADB_SASTATE_LARVAL, 0);
        assert_eq!(SADB_SASTATE_MATURE, 1);
        assert_eq!(SADB_SASTATE_DYING, 2);
        assert_eq!(SADB_SASTATE_DEAD, 3);
    }

    #[test]
    fn test_ident_types_sequential() {
        assert_eq!(SADB_IDENTTYPE_RESERVED, 0);
        assert_eq!(SADB_IDENTTYPE_PREFIX, 1);
        assert_eq!(SADB_IDENTTYPE_FQDN, 2);
        assert_eq!(SADB_IDENTTYPE_USERFQDN, 3);
    }
}
