//! `<sound/asound.h>` — ALSA control element type and access constants.
//!
//! ALSA controls represent mixer knobs, switches, and enumerations
//! that configure audio routing and levels. Each control element
//! has a type, an access mask, and belongs to a numbered interface.
//! Applications use ioctls on the control device to read/write them.

// ---------------------------------------------------------------------------
// Control element types
// ---------------------------------------------------------------------------

/// Boolean (on/off).
pub const SNDRV_CTL_ELEM_TYPE_BOOLEAN: u32 = 1;
/// Integer (range with step).
pub const SNDRV_CTL_ELEM_TYPE_INTEGER: u32 = 2;
/// Enumerated (named choices).
pub const SNDRV_CTL_ELEM_TYPE_ENUMERATED: u32 = 3;
/// Byte array.
pub const SNDRV_CTL_ELEM_TYPE_BYTES: u32 = 4;
/// IEC 958 (S/PDIF) data.
pub const SNDRV_CTL_ELEM_TYPE_IEC958: u32 = 5;
/// 64-bit integer.
pub const SNDRV_CTL_ELEM_TYPE_INTEGER64: u32 = 6;

// ---------------------------------------------------------------------------
// Control element access flags
// ---------------------------------------------------------------------------

/// Element is readable.
pub const SNDRV_CTL_ELEM_ACCESS_READ: u32 = 1 << 0;
/// Element is writable.
pub const SNDRV_CTL_ELEM_ACCESS_WRITE: u32 = 1 << 1;
/// Element value is volatile (may change without notification).
pub const SNDRV_CTL_ELEM_ACCESS_VOLATILE: u32 = 1 << 2;
/// TLV (Type/Length/Value) read supported.
pub const SNDRV_CTL_ELEM_ACCESS_TLV_READ: u32 = 1 << 4;
/// TLV write supported.
pub const SNDRV_CTL_ELEM_ACCESS_TLV_WRITE: u32 = 1 << 5;
/// TLV command supported.
pub const SNDRV_CTL_ELEM_ACCESS_TLV_COMMAND: u32 = 1 << 6;
/// Element is inactive (grayed out in mixer).
pub const SNDRV_CTL_ELEM_ACCESS_INACTIVE: u32 = 1 << 8;
/// Element is locked (cannot write).
pub const SNDRV_CTL_ELEM_ACCESS_LOCK: u32 = 1 << 9;
/// Element owner died.
pub const SNDRV_CTL_ELEM_ACCESS_OWNER: u32 = 1 << 10;
/// Read+Write combined.
pub const SNDRV_CTL_ELEM_ACCESS_READWRITE: u32 = (1 << 0) | (1 << 1);

// ---------------------------------------------------------------------------
// Control interface IDs
// ---------------------------------------------------------------------------

/// Card-level interface.
pub const SNDRV_CTL_ELEM_IFACE_CARD: u32 = 0;
/// Hardware-dependent interface.
pub const SNDRV_CTL_ELEM_IFACE_HWDEP: u32 = 1;
/// Mixer interface.
pub const SNDRV_CTL_ELEM_IFACE_MIXER: u32 = 2;
/// PCM interface.
pub const SNDRV_CTL_ELEM_IFACE_PCM: u32 = 3;
/// Raw MIDI interface.
pub const SNDRV_CTL_ELEM_IFACE_RAWMIDI: u32 = 4;
/// Timer interface.
pub const SNDRV_CTL_ELEM_IFACE_TIMER: u32 = 5;
/// Sequencer interface.
pub const SNDRV_CTL_ELEM_IFACE_SEQUENCER: u32 = 6;

// ---------------------------------------------------------------------------
// Control event mask bits
// ---------------------------------------------------------------------------

/// Element value changed.
pub const SNDRV_CTL_EVENT_MASK_VALUE: u32 = 1 << 0;
/// Element info changed.
pub const SNDRV_CTL_EVENT_MASK_INFO: u32 = 1 << 1;
/// Element added.
pub const SNDRV_CTL_EVENT_MASK_ADD: u32 = 1 << 2;
/// TLV changed.
pub const SNDRV_CTL_EVENT_MASK_TLV: u32 = 1 << 3;
/// Element removed.
pub const SNDRV_CTL_EVENT_MASK_REMOVE: u32 = 0xFFFF_FFFF;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_element_types_distinct() {
        let types = [
            SNDRV_CTL_ELEM_TYPE_BOOLEAN,
            SNDRV_CTL_ELEM_TYPE_INTEGER,
            SNDRV_CTL_ELEM_TYPE_ENUMERATED,
            SNDRV_CTL_ELEM_TYPE_BYTES,
            SNDRV_CTL_ELEM_TYPE_IEC958,
            SNDRV_CTL_ELEM_TYPE_INTEGER64,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_access_flags_no_overlap() {
        let flags = [
            SNDRV_CTL_ELEM_ACCESS_READ,
            SNDRV_CTL_ELEM_ACCESS_WRITE,
            SNDRV_CTL_ELEM_ACCESS_VOLATILE,
            SNDRV_CTL_ELEM_ACCESS_TLV_READ,
            SNDRV_CTL_ELEM_ACCESS_TLV_WRITE,
            SNDRV_CTL_ELEM_ACCESS_TLV_COMMAND,
            SNDRV_CTL_ELEM_ACCESS_INACTIVE,
            SNDRV_CTL_ELEM_ACCESS_LOCK,
            SNDRV_CTL_ELEM_ACCESS_OWNER,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_readwrite_combines() {
        assert_eq!(
            SNDRV_CTL_ELEM_ACCESS_READWRITE,
            SNDRV_CTL_ELEM_ACCESS_READ | SNDRV_CTL_ELEM_ACCESS_WRITE
        );
    }

    #[test]
    fn test_interface_ids_distinct() {
        let ifaces = [
            SNDRV_CTL_ELEM_IFACE_CARD,
            SNDRV_CTL_ELEM_IFACE_HWDEP,
            SNDRV_CTL_ELEM_IFACE_MIXER,
            SNDRV_CTL_ELEM_IFACE_PCM,
            SNDRV_CTL_ELEM_IFACE_RAWMIDI,
            SNDRV_CTL_ELEM_IFACE_TIMER,
            SNDRV_CTL_ELEM_IFACE_SEQUENCER,
        ];
        for i in 0..ifaces.len() {
            for j in (i + 1)..ifaces.len() {
                assert_ne!(ifaces[i], ifaces[j]);
            }
        }
    }
}
