//! `<linux/keyctl.h>` extras — keyring search-restriction interface.
//!
//! Linux 4.12 added the "restrict_keyring" interface that lets the
//! caller install a kernel-side hook deciding which keys may be added
//! to a keyring. `evmctl`, IMA, the kernel module signer, and the
//! cryptsetup integrity pipeline all set restrictions through these
//! string constants.

// ---------------------------------------------------------------------------
// Standard restriction "method" strings (passed verbatim to keyctl(2))
// ---------------------------------------------------------------------------

/// Restrict by asymmetric-key chain to a trusted source.
pub const KEYCTL_RESTRICT_BUILTIN_TRUSTED: &str = "builtin_trusted";
/// Trust the secondary trusted keyring as well.
pub const KEYCTL_RESTRICT_SECONDARY_TRUSTED: &str = "builtin_and_secondary_trusted";
/// Restrict by a specified key in a specified keyring.
pub const KEYCTL_RESTRICT_KEY_OR_KEYRING: &str = "key_or_keyring";

// ---------------------------------------------------------------------------
// Standard key types accepted by `add_key(2)`
// ---------------------------------------------------------------------------

pub const KEY_TYPE_KEYRING: &str = "keyring";
pub const KEY_TYPE_USER: &str = "user";
pub const KEY_TYPE_LOGON: &str = "logon";
pub const KEY_TYPE_BIG_KEY: &str = "big_key";
pub const KEY_TYPE_ENCRYPTED: &str = "encrypted";
pub const KEY_TYPE_TRUSTED: &str = "trusted";
pub const KEY_TYPE_ASYMMETRIC: &str = "asymmetric";
pub const KEY_TYPE_DNS_RESOLVER: &str = "dns_resolver";
pub const KEY_TYPE_RXRPC: &str = "rxrpc";

// ---------------------------------------------------------------------------
// Watch_queue notification IDs (Linux 5.8+)
// ---------------------------------------------------------------------------

pub const NOTIFY_KEY_INSTANTIATED: u32 = 1;
pub const NOTIFY_KEY_UPDATED: u32 = 2;
pub const NOTIFY_KEY_LINKED: u32 = 3;
pub const NOTIFY_KEY_UNLINKED: u32 = 4;
pub const NOTIFY_KEY_CLEARED: u32 = 5;
pub const NOTIFY_KEY_REVOKED: u32 = 6;
pub const NOTIFY_KEY_INVALIDATED: u32 = 7;
pub const NOTIFY_KEY_SETATTR: u32 = 8;

// ---------------------------------------------------------------------------
// Sizes
// ---------------------------------------------------------------------------

/// `keyring_index_key.desc_len` — max key description length.
pub const KEY_MAX_DESC_SIZE: usize = 4096;
/// Max payload bytes for "user" and "logon" key types.
pub const KEY_MAX_PAYLOAD_SIZE: usize = 32_767;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_restriction_methods_unique() {
        let m = [
            KEYCTL_RESTRICT_BUILTIN_TRUSTED,
            KEYCTL_RESTRICT_SECONDARY_TRUSTED,
            KEYCTL_RESTRICT_KEY_OR_KEYRING,
        ];
        for i in 0..m.len() {
            for j in (i + 1)..m.len() {
                assert_ne!(m[i], m[j]);
            }
        }
        // None of the names contain spaces — the kernel parser is strict.
        for x in m {
            assert!(!x.contains(' '));
            assert!(!x.is_empty());
        }
    }

    #[test]
    fn test_key_types_lowercase_no_spaces() {
        let t = [
            KEY_TYPE_KEYRING,
            KEY_TYPE_USER,
            KEY_TYPE_LOGON,
            KEY_TYPE_BIG_KEY,
            KEY_TYPE_ENCRYPTED,
            KEY_TYPE_TRUSTED,
            KEY_TYPE_ASYMMETRIC,
            KEY_TYPE_DNS_RESOLVER,
            KEY_TYPE_RXRPC,
        ];
        for x in t {
            // Kernel type names are lowercase ASCII identifiers with optional _.
            for b in x.as_bytes() {
                assert!(b.is_ascii_lowercase() || *b == b'_');
            }
        }
    }

    #[test]
    fn test_notify_codes_dense_1_to_8() {
        let n = [
            NOTIFY_KEY_INSTANTIATED,
            NOTIFY_KEY_UPDATED,
            NOTIFY_KEY_LINKED,
            NOTIFY_KEY_UNLINKED,
            NOTIFY_KEY_CLEARED,
            NOTIFY_KEY_REVOKED,
            NOTIFY_KEY_INVALIDATED,
            NOTIFY_KEY_SETATTR,
        ];
        for (i, &v) in n.iter().enumerate() {
            assert_eq!(v as usize, i + 1);
        }
    }

    #[test]
    fn test_size_constants_sane() {
        // 4 KiB cap on description size matches a page on common arches.
        assert_eq!(KEY_MAX_DESC_SIZE, 4096);
        // Payload cap is 32 KiB - 1 (fits in s16).
        assert_eq!(KEY_MAX_PAYLOAD_SIZE, 0x7FFF);
        // Payload limit dwarfs the description limit.
        assert!(KEY_MAX_PAYLOAD_SIZE > KEY_MAX_DESC_SIZE);
    }
}
