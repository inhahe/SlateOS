//! `<shadow.h>` — Shadow password file constants.
//!
//! The shadow password file `/etc/shadow` stores encrypted
//! passwords and password aging information. These constants
//! define field limits, hash algorithm identifiers, and
//! special field values.

// ---------------------------------------------------------------------------
// Password hash algorithm identifiers (prefix in $id$salt$hash)
// ---------------------------------------------------------------------------

/// MD5 hash ($1$).
pub const SHADOW_MD5: u8 = 1;
/// Blowfish hash ($2a$, $2b$, $2y$).
pub const SHADOW_BLOWFISH: u8 = 2;
/// SHA-256 hash ($5$).
pub const SHADOW_SHA256: u8 = 5;
/// SHA-512 hash ($6$).
pub const SHADOW_SHA512: u8 = 6;
/// yescrypt hash ($y$).
pub const SHADOW_YESCRYPT: u8 = 7;

// ---------------------------------------------------------------------------
// Special field values
// ---------------------------------------------------------------------------

/// Password field: account is locked (prefix '!').
pub const SHADOW_LOCKED_PREFIX: u8 = b'!';
/// Password field: no password set (empty or '*').
pub const SHADOW_NO_PASSWORD: u8 = b'*';
/// Aging field: no expiration (value -1 or empty).
pub const SHADOW_NO_EXPIRE: i64 = -1;
/// Aging field: password change required on next login (value 0).
pub const SHADOW_CHANGE_REQUIRED: i64 = 0;

// ---------------------------------------------------------------------------
// Password aging field limits
// ---------------------------------------------------------------------------

/// Minimum days between password changes.
pub const SHADOW_MIN_DAYS_DEFAULT: u32 = 0;
/// Maximum days a password is valid.
pub const SHADOW_MAX_DAYS_DEFAULT: u32 = 99999;
/// Warning days before password expires.
pub const SHADOW_WARN_DAYS_DEFAULT: u32 = 7;

// ---------------------------------------------------------------------------
// Shadow file separator
// ---------------------------------------------------------------------------

/// Field separator in /etc/shadow (colon).
pub const SHADOW_SEPARATOR: u8 = b':';

// ---------------------------------------------------------------------------
// Hash length limits
// ---------------------------------------------------------------------------

/// Maximum hash output length (SHA-512 base64).
pub const SHADOW_HASH_MAX: u32 = 106;
/// Maximum salt length.
pub const SHADOW_SALT_MAX: u32 = 16;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_ids_distinct() {
        let ids = [
            SHADOW_MD5,
            SHADOW_BLOWFISH,
            SHADOW_SHA256,
            SHADOW_SHA512,
            SHADOW_YESCRYPT,
        ];
        for i in 0..ids.len() {
            for j in (i + 1)..ids.len() {
                assert_ne!(ids[i], ids[j]);
            }
        }
    }

    #[test]
    fn test_no_expire() {
        assert_eq!(SHADOW_NO_EXPIRE, -1);
    }

    #[test]
    fn test_change_required() {
        assert_eq!(SHADOW_CHANGE_REQUIRED, 0);
    }

    #[test]
    fn test_aging_defaults() {
        assert_eq!(SHADOW_MIN_DAYS_DEFAULT, 0);
        assert_eq!(SHADOW_MAX_DAYS_DEFAULT, 99999);
        assert_eq!(SHADOW_WARN_DAYS_DEFAULT, 7);
    }

    #[test]
    fn test_separator() {
        assert_eq!(SHADOW_SEPARATOR, b':');
    }

    #[test]
    fn test_locked_prefix() {
        assert_eq!(SHADOW_LOCKED_PREFIX, b'!');
    }
}
