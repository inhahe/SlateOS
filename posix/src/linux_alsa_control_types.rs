//! `<sound/asound.h>` (control subset) — ALSA control element constants.
//!
//! ALSA controls are the mixer elements exposed to userspace: volume
//! sliders, mute switches, enum selectors (input source), etc. Each
//! sound card has a control device (/dev/snd/controlC0) that provides
//! access to all controls. Controls have a type (boolean, integer,
//! enum), access flags (read/write/volatile), and numeric IDs for
//! efficient lookup.

// ---------------------------------------------------------------------------
// Control element types
// ---------------------------------------------------------------------------

/// Boolean control (on/off switch).
pub const SNDRV_CTL_ELEM_TYPE_BOOLEAN: u32 = 1;
/// Integer control (volume, gain).
pub const SNDRV_CTL_ELEM_TYPE_INTEGER: u32 = 2;
/// Enumerated control (input source selector).
pub const SNDRV_CTL_ELEM_TYPE_ENUMERATED: u32 = 3;
/// Byte array control (arbitrary data).
pub const SNDRV_CTL_ELEM_TYPE_BYTES: u32 = 4;
/// IEC 958 (S/PDIF) status bits.
pub const SNDRV_CTL_ELEM_TYPE_IEC958: u32 = 5;
/// 64-bit integer control.
pub const SNDRV_CTL_ELEM_TYPE_INTEGER64: u32 = 6;

// ---------------------------------------------------------------------------
// Control element interfaces
// ---------------------------------------------------------------------------

/// Card-level control.
pub const SNDRV_CTL_ELEM_IFACE_CARD: u32 = 0;
/// HWDEP (hardware-dependent) control.
pub const SNDRV_CTL_ELEM_IFACE_HWDEP: u32 = 1;
/// Mixer control.
pub const SNDRV_CTL_ELEM_IFACE_MIXER: u32 = 2;
/// PCM control.
pub const SNDRV_CTL_ELEM_IFACE_PCM: u32 = 3;
/// Raw MIDI control.
pub const SNDRV_CTL_ELEM_IFACE_RAWMIDI: u32 = 4;
/// Timer control.
pub const SNDRV_CTL_ELEM_IFACE_TIMER: u32 = 5;
/// Sequencer control.
pub const SNDRV_CTL_ELEM_IFACE_SEQUENCER: u32 = 6;

// ---------------------------------------------------------------------------
// Control access flags
// ---------------------------------------------------------------------------

/// Control is readable.
pub const SNDRV_CTL_ELEM_ACCESS_READ: u32 = 0x01;
/// Control is writable.
pub const SNDRV_CTL_ELEM_ACCESS_WRITE: u32 = 0x02;
/// Control value is volatile (changes without notification).
pub const SNDRV_CTL_ELEM_ACCESS_VOLATILE: u32 = 0x04;
/// Control supports TLV (Type-Length-Value) data.
pub const SNDRV_CTL_ELEM_ACCESS_TLV_READ: u32 = 0x08;
/// Control TLV is writable.
pub const SNDRV_CTL_ELEM_ACCESS_TLV_WRITE: u32 = 0x10;
/// Control is inactive (grayed out in mixer).
pub const SNDRV_CTL_ELEM_ACCESS_INACTIVE: u32 = 0x100;
/// Control is locked (cannot be changed).
pub const SNDRV_CTL_ELEM_ACCESS_LOCK: u32 = 0x200;
/// Control belongs to the owner process.
pub const SNDRV_CTL_ELEM_ACCESS_OWNER: u32 = 0x400;

// ---------------------------------------------------------------------------
// Control event masks
// ---------------------------------------------------------------------------

/// Control value changed.
pub const SNDRV_CTL_EVENT_MASK_VALUE: u32 = 0x01;
/// Control info changed (range, type).
pub const SNDRV_CTL_EVENT_MASK_INFO: u32 = 0x02;
/// Control was added.
pub const SNDRV_CTL_EVENT_MASK_ADD: u32 = 0x04;
/// Control TLV changed.
pub const SNDRV_CTL_EVENT_MASK_TLV: u32 = 0x08;
/// Control was removed.
pub const SNDRV_CTL_EVENT_MASK_REMOVE: u32 = 0xFFFF_FFFF;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_types_distinct() {
        let types = [
            SNDRV_CTL_ELEM_TYPE_BOOLEAN, SNDRV_CTL_ELEM_TYPE_INTEGER,
            SNDRV_CTL_ELEM_TYPE_ENUMERATED, SNDRV_CTL_ELEM_TYPE_BYTES,
            SNDRV_CTL_ELEM_TYPE_IEC958, SNDRV_CTL_ELEM_TYPE_INTEGER64,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_ifaces_distinct() {
        let ifaces = [
            SNDRV_CTL_ELEM_IFACE_CARD, SNDRV_CTL_ELEM_IFACE_HWDEP,
            SNDRV_CTL_ELEM_IFACE_MIXER, SNDRV_CTL_ELEM_IFACE_PCM,
            SNDRV_CTL_ELEM_IFACE_RAWMIDI, SNDRV_CTL_ELEM_IFACE_TIMER,
            SNDRV_CTL_ELEM_IFACE_SEQUENCER,
        ];
        for i in 0..ifaces.len() {
            for j in (i + 1)..ifaces.len() {
                assert_ne!(ifaces[i], ifaces[j]);
            }
        }
    }

    #[test]
    fn test_access_flags() {
        // Read and write should not overlap
        assert_eq!(SNDRV_CTL_ELEM_ACCESS_READ & SNDRV_CTL_ELEM_ACCESS_WRITE, 0);
        assert!(SNDRV_CTL_ELEM_ACCESS_READ.is_power_of_two());
        assert!(SNDRV_CTL_ELEM_ACCESS_WRITE.is_power_of_two());
    }
}
