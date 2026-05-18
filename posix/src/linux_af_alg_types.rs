//! `<linux/if_alg.h>` — AF_ALG socket interface constants.
//!
//! AF_ALG provides userspace access to the kernel crypto API via
//! sockets. A process opens an AF_ALG socket, binds it to an
//! algorithm type+name, then accepts connections to get operational
//! file descriptors for encrypt/decrypt/hash operations.

// ---------------------------------------------------------------------------
// AF_ALG socket option levels
// ---------------------------------------------------------------------------

/// Socket option level for ALG sockets.
pub const SOL_ALG: u32 = 279;

// ---------------------------------------------------------------------------
// ALG socket options (setsockopt/getsockopt)
// ---------------------------------------------------------------------------

/// Set algorithm key.
pub const ALG_SET_KEY: u32 = 1;
/// Set IV (initialization vector).
pub const ALG_SET_IV: u32 = 2;
/// Set operation type (encrypt/decrypt).
pub const ALG_SET_OP: u32 = 3;
/// Set AEAD associated data length.
pub const ALG_SET_AEAD_ASSOCLEN: u32 = 4;
/// Set AEAD authentication tag length.
pub const ALG_SET_AEAD_AUTHSIZE: u32 = 5;
/// Set DH/ECDH parameters.
pub const ALG_SET_DH_PARAMETERS: u32 = 6;

// ---------------------------------------------------------------------------
// ALG operation types (ALG_SET_OP values)
// ---------------------------------------------------------------------------

/// Encrypt operation.
pub const ALG_OP_ENCRYPT: u32 = 0;
/// Decrypt operation.
pub const ALG_OP_DECRYPT: u32 = 1;
/// Sign operation (akcipher).
pub const ALG_OP_SIGN: u32 = 2;
/// Verify operation (akcipher).
pub const ALG_OP_VERIFY: u32 = 3;

// ---------------------------------------------------------------------------
// ALG cmsg types (used with sendmsg)
// ---------------------------------------------------------------------------

/// Control message type: set IV.
pub const ALG_CMSG_IV: u32 = 1;
/// Control message type: set operation.
pub const ALG_CMSG_OP: u32 = 2;
/// Control message type: set AEAD assoclen.
pub const ALG_CMSG_AEAD_ASSOCLEN: u32 = 3;

// ---------------------------------------------------------------------------
// AF_ALG family constant
// ---------------------------------------------------------------------------

/// AF_ALG protocol family number.
pub const AF_ALG: u32 = 38;

// ---------------------------------------------------------------------------
// Algorithm type string indices (for struct sockaddr_alg.salg_type)
// ---------------------------------------------------------------------------

/// Maximum algorithm type name length.
pub const ALG_MAX_TYPE_LEN: u32 = 14;
/// Maximum algorithm name length.
pub const ALG_MAX_NAME_LEN: u32 = 64;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_socket_options_distinct() {
        let opts = [
            ALG_SET_KEY, ALG_SET_IV, ALG_SET_OP,
            ALG_SET_AEAD_ASSOCLEN, ALG_SET_AEAD_AUTHSIZE,
            ALG_SET_DH_PARAMETERS,
        ];
        for i in 0..opts.len() {
            for j in (i + 1)..opts.len() {
                assert_ne!(opts[i], opts[j]);
            }
        }
    }

    #[test]
    fn test_operations_distinct() {
        let ops = [
            ALG_OP_ENCRYPT, ALG_OP_DECRYPT,
            ALG_OP_SIGN, ALG_OP_VERIFY,
        ];
        for i in 0..ops.len() {
            for j in (i + 1)..ops.len() {
                assert_ne!(ops[i], ops[j]);
            }
        }
    }

    #[test]
    fn test_cmsg_types_distinct() {
        assert_ne!(ALG_CMSG_IV, ALG_CMSG_OP);
        assert_ne!(ALG_CMSG_OP, ALG_CMSG_AEAD_ASSOCLEN);
        assert_ne!(ALG_CMSG_IV, ALG_CMSG_AEAD_ASSOCLEN);
    }

    #[test]
    fn test_af_alg_value() {
        assert_eq!(AF_ALG, 38);
    }

    #[test]
    fn test_name_limits() {
        assert!(ALG_MAX_TYPE_LEN > 0);
        assert!(ALG_MAX_NAME_LEN > ALG_MAX_TYPE_LEN);
    }
}
