//! `<linux/key-type.h>` — Kernel key type and state constants.
//!
//! Each key in the keyring subsystem has a type determining how its
//! payload is interpreted and managed. Built-in types include "user"
//! (arbitrary blob), "keyring" (container), "logon" (non-readable
//! by userspace), and "encrypted" (kernel-managed encryption).

// ---------------------------------------------------------------------------
// Key states
// ---------------------------------------------------------------------------

/// Key is valid and usable.
pub const KEY_IS_POSITIVE: u32 = 0;
/// Key has been negatively instantiated (lookup failed).
pub const KEY_IS_NEGATIVE: u32 = 1;
/// Key has expired (past its timeout).
pub const KEY_IS_EXPIRED: u32 = 2;
/// Key has been revoked.
pub const KEY_IS_REVOKED: u32 = 3;
/// Key is uninstantiated (awaiting payload).
pub const KEY_IS_UNINSTANTIATED: u32 = 4;

// ---------------------------------------------------------------------------
// Key flags (internal kernel flags)
// ---------------------------------------------------------------------------

/// Key has been instantiated.
pub const KEY_FLAG_INSTANTIATED: u32 = 1 << 0;
/// Key is dead (garbage collection pending).
pub const KEY_FLAG_DEAD: u32 = 1 << 1;
/// Key is revoked.
pub const KEY_FLAG_REVOKED: u32 = 1 << 2;
/// Key is negatively instantiated.
pub const KEY_FLAG_NEGATIVE: u32 = 1 << 3;
/// Key has root-only access.
pub const KEY_FLAG_ROOT_CAN_CLEAR: u32 = 1 << 4;
/// Key is invalidated.
pub const KEY_FLAG_INVALIDATED: u32 = 1 << 5;
/// Key built-in (compiled into kernel).
pub const KEY_FLAG_BUILTIN: u32 = 1 << 6;
/// Key cannot be overridden by userspace.
pub const KEY_FLAG_KEEP: u32 = 1 << 7;

// ---------------------------------------------------------------------------
// Key type indices (for type matching)
// ---------------------------------------------------------------------------

/// Type: user (arbitrary userspace data).
pub const KEY_TYPE_USER: u32 = 0;
/// Type: logon (like user, but not readable by userspace).
pub const KEY_TYPE_LOGON: u32 = 1;
/// Type: keyring (container for other keys).
pub const KEY_TYPE_KEYRING: u32 = 2;
/// Type: big_key (large payload, may be stored in shmem/tmpfs).
pub const KEY_TYPE_BIG_KEY: u32 = 3;
/// Type: encrypted (kernel-managed encrypted blob).
pub const KEY_TYPE_ENCRYPTED: u32 = 4;
/// Type: trusted (sealed by TPM).
pub const KEY_TYPE_TRUSTED: u32 = 5;
/// Type: asymmetric (X.509 certificate or public key).
pub const KEY_TYPE_ASYMMETRIC: u32 = 6;
/// Type: dns_resolver (DNS lookup result caching).
pub const KEY_TYPE_DNS_RESOLVER: u32 = 7;

// ---------------------------------------------------------------------------
// Key size limits
// ---------------------------------------------------------------------------

/// Maximum key description length.
pub const KEY_MAX_DESC_SIZE: u32 = 4096;
/// Maximum payload size for "user" type keys.
pub const KEY_MAX_PAYLOAD_SIZE: u32 = 32767;
/// Maximum size before big_key uses shmem backing.
pub const KEY_BIG_KEY_THRESHOLD: u32 = 1024;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_key_states_distinct() {
        let states = [
            KEY_IS_POSITIVE,
            KEY_IS_NEGATIVE,
            KEY_IS_EXPIRED,
            KEY_IS_REVOKED,
            KEY_IS_UNINSTANTIATED,
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_key_flags_no_overlap() {
        let flags = [
            KEY_FLAG_INSTANTIATED,
            KEY_FLAG_DEAD,
            KEY_FLAG_REVOKED,
            KEY_FLAG_NEGATIVE,
            KEY_FLAG_ROOT_CAN_CLEAR,
            KEY_FLAG_INVALIDATED,
            KEY_FLAG_BUILTIN,
            KEY_FLAG_KEEP,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_key_types_distinct() {
        let types = [
            KEY_TYPE_USER,
            KEY_TYPE_LOGON,
            KEY_TYPE_KEYRING,
            KEY_TYPE_BIG_KEY,
            KEY_TYPE_ENCRYPTED,
            KEY_TYPE_TRUSTED,
            KEY_TYPE_ASYMMETRIC,
            KEY_TYPE_DNS_RESOLVER,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_size_limits() {
        assert!(KEY_MAX_DESC_SIZE > 0);
        assert!(KEY_MAX_PAYLOAD_SIZE > KEY_BIG_KEY_THRESHOLD);
    }
}
