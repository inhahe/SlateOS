//! `<sound/asound.h>` — ALSA control element types and access flags.
//!
//! The "control" interface is how userspace inspects and adjusts mixer
//! controls, switches, and metadata exposed by an ALSA card. Every
//! control has a type, an interface category, and a set of access bits.

// ---------------------------------------------------------------------------
// Control element types (`snd_ctl_elem_type_t`)
// ---------------------------------------------------------------------------

pub const SNDRV_CTL_ELEM_TYPE_NONE: u32 = 0;
pub const SNDRV_CTL_ELEM_TYPE_BOOLEAN: u32 = 1;
pub const SNDRV_CTL_ELEM_TYPE_INTEGER: u32 = 2;
pub const SNDRV_CTL_ELEM_TYPE_ENUMERATED: u32 = 3;
pub const SNDRV_CTL_ELEM_TYPE_BYTES: u32 = 4;
pub const SNDRV_CTL_ELEM_TYPE_IEC958: u32 = 5;
pub const SNDRV_CTL_ELEM_TYPE_INTEGER64: u32 = 6;

// ---------------------------------------------------------------------------
// Control interface categories (`snd_ctl_elem_iface_t`)
// ---------------------------------------------------------------------------

pub const SNDRV_CTL_ELEM_IFACE_CARD: u32 = 0;
pub const SNDRV_CTL_ELEM_IFACE_HWDEP: u32 = 1;
pub const SNDRV_CTL_ELEM_IFACE_MIXER: u32 = 2;
pub const SNDRV_CTL_ELEM_IFACE_PCM: u32 = 3;
pub const SNDRV_CTL_ELEM_IFACE_RAWMIDI: u32 = 4;
pub const SNDRV_CTL_ELEM_IFACE_TIMER: u32 = 5;
pub const SNDRV_CTL_ELEM_IFACE_SEQUENCER: u32 = 6;

// ---------------------------------------------------------------------------
// Access bits (`SNDRV_CTL_ELEM_ACCESS_*`) — flags, OR-combined
// ---------------------------------------------------------------------------

pub const SNDRV_CTL_ELEM_ACCESS_READ: u32 = 1 << 0;
pub const SNDRV_CTL_ELEM_ACCESS_WRITE: u32 = 1 << 1;
pub const SNDRV_CTL_ELEM_ACCESS_VOLATILE: u32 = 1 << 2;
pub const SNDRV_CTL_ELEM_ACCESS_TIMESTAMP: u32 = 1 << 3;
pub const SNDRV_CTL_ELEM_ACCESS_TLV_READ: u32 = 1 << 4;
pub const SNDRV_CTL_ELEM_ACCESS_TLV_WRITE: u32 = 1 << 5;
pub const SNDRV_CTL_ELEM_ACCESS_TLV_COMMAND: u32 = 1 << 6;
pub const SNDRV_CTL_ELEM_ACCESS_INACTIVE: u32 = 1 << 8;
pub const SNDRV_CTL_ELEM_ACCESS_LOCK: u32 = 1 << 9;
pub const SNDRV_CTL_ELEM_ACCESS_OWNER: u32 = 1 << 10;
pub const SNDRV_CTL_ELEM_ACCESS_TLV_CALLBACK: u32 = 1 << 28;
pub const SNDRV_CTL_ELEM_ACCESS_USER: u32 = 1 << 29;

/// `READ | WRITE` — most controls are bidirectional.
pub const SNDRV_CTL_ELEM_ACCESS_READWRITE: u32 =
    SNDRV_CTL_ELEM_ACCESS_READ | SNDRV_CTL_ELEM_ACCESS_WRITE;

// ---------------------------------------------------------------------------
// Element-ID limits
// ---------------------------------------------------------------------------

/// Maximum length of the human-readable control name (`name` field).
pub const SNDRV_CTL_ELEM_ID_NAME_MAXLEN: usize = 44;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_elem_types_dense_0_to_6() {
        let t = [
            SNDRV_CTL_ELEM_TYPE_NONE,
            SNDRV_CTL_ELEM_TYPE_BOOLEAN,
            SNDRV_CTL_ELEM_TYPE_INTEGER,
            SNDRV_CTL_ELEM_TYPE_ENUMERATED,
            SNDRV_CTL_ELEM_TYPE_BYTES,
            SNDRV_CTL_ELEM_TYPE_IEC958,
            SNDRV_CTL_ELEM_TYPE_INTEGER64,
        ];
        for (i, &v) in t.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }

    #[test]
    fn test_iface_categories_dense_0_to_6() {
        let i = [
            SNDRV_CTL_ELEM_IFACE_CARD,
            SNDRV_CTL_ELEM_IFACE_HWDEP,
            SNDRV_CTL_ELEM_IFACE_MIXER,
            SNDRV_CTL_ELEM_IFACE_PCM,
            SNDRV_CTL_ELEM_IFACE_RAWMIDI,
            SNDRV_CTL_ELEM_IFACE_TIMER,
            SNDRV_CTL_ELEM_IFACE_SEQUENCER,
        ];
        for (idx, &v) in i.iter().enumerate() {
            assert_eq!(v as usize, idx);
        }
    }

    #[test]
    fn test_access_bits_each_a_power_of_two() {
        let bits = [
            SNDRV_CTL_ELEM_ACCESS_READ,
            SNDRV_CTL_ELEM_ACCESS_WRITE,
            SNDRV_CTL_ELEM_ACCESS_VOLATILE,
            SNDRV_CTL_ELEM_ACCESS_TIMESTAMP,
            SNDRV_CTL_ELEM_ACCESS_TLV_READ,
            SNDRV_CTL_ELEM_ACCESS_TLV_WRITE,
            SNDRV_CTL_ELEM_ACCESS_TLV_COMMAND,
            SNDRV_CTL_ELEM_ACCESS_INACTIVE,
            SNDRV_CTL_ELEM_ACCESS_LOCK,
            SNDRV_CTL_ELEM_ACCESS_OWNER,
            SNDRV_CTL_ELEM_ACCESS_TLV_CALLBACK,
            SNDRV_CTL_ELEM_ACCESS_USER,
        ];
        for v in bits {
            assert!(v.is_power_of_two());
        }
    }

    #[test]
    fn test_access_readwrite_combines_low_two_bits() {
        assert_eq!(SNDRV_CTL_ELEM_ACCESS_READWRITE, 0b11);
        assert_ne!(
            SNDRV_CTL_ELEM_ACCESS_READ & SNDRV_CTL_ELEM_ACCESS_WRITE,
            SNDRV_CTL_ELEM_ACCESS_READ
        );
    }

    #[test]
    fn test_user_callback_bits_in_high_range() {
        // High bits (28, 29) are reserved for runtime metadata,
        // disjoint from the low protocol bits.
        assert!(SNDRV_CTL_ELEM_ACCESS_TLV_CALLBACK >= 1 << 28);
        assert!(SNDRV_CTL_ELEM_ACCESS_USER >= 1 << 28);
        assert_eq!(SNDRV_CTL_ELEM_ACCESS_USER & 0xFF, 0);
    }

    #[test]
    fn test_name_maxlen_is_44() {
        // The 44-byte ID name length is fixed ABI.
        assert_eq!(SNDRV_CTL_ELEM_ID_NAME_MAXLEN, 44);
    }
}
