//! The evdev wire format, and nothing else.
//!
//! No file descriptors, no `unsafe`, no system calls — so every byte offset and
//! every table in here is compiled and tested on the machine this tree is
//! written on, which is not the machine the kernel runs on. That split is the
//! same one [`drm::uapi`](crate::present::drm::uapi) makes, for the same
//! reason: every bug this layer can have is a *protocol* bug (a field at the
//! wrong offset, a keycode mapped to the wrong physical key, a scroll direction
//! inverted), and a protocol bug behind `#[cfg(target_os = "linux")]` is a
//! protocol bug nobody can see.
//!
//! ## The record
//!
//! A read from `/dev/input/eventN` returns whole `struct input_event`s:
//!
//! ```text
//! offset  size  field
//!      0     8  time.tv_sec   (i64)
//!      8     8  time.tv_usec  (i64)
//!     16     2  type          (u16)  EV_KEY, EV_REL, EV_MSC, EV_SYN
//!     18     2  code          (u16)  KEY_A, REL_X, MSC_SCAN, SYN_REPORT
//!     20     4  value         (i32)  0 up / 1 down, or a signed delta
//! ```
//!
//! 24 bytes, no padding, little-endian on every target this runs on. A
//! coherent group of records is terminated by one `EV_SYN`/`SYN_REPORT`: a
//! mouse's `REL_X`, `REL_Y` and button transitions arrive together and are one
//! movement, not three.
//!
//! ## Scancodes, keycodes, and which one the compositor wants
//!
//! These are two different numbering schemes and the compositor uses the older
//! one. [`keymap`](crate::keymap) is a **scan code set 1** table — `0x1E` is
//! `A`, and an extended key carries its `0xE0` prefix in the high byte so that
//! `0xE04B` (Left arrow) stays distinct from `0x4B` (keypad 4). evdev speaks
//! **Linux keycodes** instead, in which those two keys are 105 and 75.
//!
//! Getting from one to the other has two routes, and this module provides both
//! because neither is sufficient alone:
//!
//! * **[`set1_for_keycode`]**, the inverse of the kernel's
//!   `evdev::set1_to_keycode` / `set1_extended_to_keycode`. This is the primary
//!   route, because a Linux keycode means the same physical key on every device
//!   — that is the entire point of the keycode layer.
//! * **`EV_MSC`/`MSC_SCAN`**, the raw code the device sent, which the SlateOS
//!   kernel emits alongside every key event already in exactly the compositor's
//!   convention — `0xE000 | code` for an extended key
//!   (`kernel/src/keyboard.rs::publish_evdev`). This is the *fallback*, used
//!   only for a keycode the table does not name.
//!
//! The order is deliberately the opposite of what the wire suggests. `MSC_SCAN`
//! looks like the more authoritative answer — it is what the hardware actually
//! sent rather than a reconstruction of it — but it is only a set-1 code when
//! the device is a PS/2 device. On a USB HID keyboard Linux reports the **HID
//! usage** there instead (`0x0007_0000 | usage`), and reading that as a set-1
//! code names an entirely different key. The keycode table has no such
//! ambiguity, so it comes first and `MSC_SCAN` catches only what it misses.

/// Bytes in one `struct input_event`.
pub const EVENT_SIZE: usize = 24;

// --- Event types (`EV_*`) ---

/// Synchronisation: packet boundaries and dropped-event notices.
pub const EV_SYN: u16 = 0x00;
/// A key or button changed state.
pub const EV_KEY: u16 = 0x01;
/// A relative axis moved.
pub const EV_REL: u16 = 0x02;
/// Miscellaneous; the only one that matters here is [`MSC_SCAN`].
pub const EV_MSC: u16 = 0x04;

// --- `EV_SYN` codes ---

/// End of a coherent packet.
pub const SYN_REPORT: u16 = 0;
/// The reader fell behind and was lapped; state must be re-synchronised.
pub const SYN_DROPPED: u16 = 3;

// --- `EV_REL` codes ---

/// Horizontal pointer motion, positive right.
pub const REL_X: u16 = 0x00;
/// Vertical pointer motion, positive **down** — screen order, not maths order.
pub const REL_Y: u16 = 0x01;
/// Horizontal scroll, positive right.
pub const REL_HWHEEL: u16 = 0x06;
/// Vertical scroll in notches, positive **up** — the opposite sense to
/// [`REL_Y`], which is a genuine asymmetry in the Linux ABI and not a mistake
/// here.
pub const REL_WHEEL: u16 = 0x08;

// --- `EV_MSC` codes ---

/// The raw hardware scan code behind an [`EV_KEY`].
pub const MSC_SCAN: u16 = 0x04;

// --- `EV_KEY` codes for buttons ---

/// Primary mouse button.
pub const BTN_LEFT: u16 = 0x110;
/// Secondary mouse button.
pub const BTN_RIGHT: u16 = 0x111;
/// Wheel-click button.
pub const BTN_MIDDLE: u16 = 0x112;
/// The "back" thumb button.
pub const BTN_SIDE: u16 = 0x113;
/// The "forward" thumb button.
pub const BTN_EXTRA: u16 = 0x114;

/// The lowest `BTN_*` code. Everything below this is a keyboard key.
pub const BTN_MISC: u16 = 0x100;

// --- Key-event values ---

/// Key released.
pub const KEY_VALUE_UP: i32 = 0;
/// Key pressed.
pub const KEY_VALUE_DOWN: i32 = 1;
/// Hardware autorepeat. The SlateOS kernel never generates this — it has no
/// repeat timer and says so by failing `EVIOCGREP` — but a Linux host driving
/// this same code does, and a repeat delivered as a *second* down without an up
/// would otherwise leave [`super::EvdevInput`] believing two keys are held.
pub const KEY_VALUE_REPEAT: i32 = 2;

/// One `struct input_event`, decoded.
///
/// The timestamp is kept even though nothing reads it today: it is the only
/// record of *when the hardware said so* rather than when the compositor got
/// round to looking, and discarding it here would make it unrecoverable later
/// without another pass over the wire format.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Record {
    /// Seconds of the event's timestamp.
    pub sec: i64,
    /// Microseconds of the event's timestamp.
    pub usec: i64,
    /// `EV_*`.
    pub kind: u16,
    /// The code within that type — a `KEY_*`, `REL_*`, `MSC_*` or `SYN_*`.
    pub code: u16,
    /// A key transition (0/1/2), or a signed relative delta.
    pub value: i32,
}

impl Record {
    /// Decode one record from the first [`EVENT_SIZE`] bytes of `bytes`.
    ///
    /// Returns `None` for a short slice rather than panicking or reading past
    /// the end: a short read is the kernel misbehaving, and a display server
    /// must not be brought down by its input device.
    #[must_use]
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        let sec = i64::from_le_bytes(bytes.get(0..8)?.try_into().ok()?);
        let usec = i64::from_le_bytes(bytes.get(8..16)?.try_into().ok()?);
        let kind = u16::from_le_bytes(bytes.get(16..18)?.try_into().ok()?);
        let code = u16::from_le_bytes(bytes.get(18..20)?.try_into().ok()?);
        let value = i32::from_le_bytes(bytes.get(20..24)?.try_into().ok()?);
        Some(Self {
            sec,
            usec,
            kind,
            code,
            value,
        })
    }

    /// Encode this record the way the kernel writes it.
    ///
    /// The compositor never sends events to a device, so this exists for the
    /// tests — which is precisely the point of it existing *here*, beside the
    /// decoder: a test that built its input with its own hand-rolled byte
    /// layout would agree with a broken decoder as readily as with a correct
    /// one.
    #[must_use]
    pub fn encode(&self) -> [u8; EVENT_SIZE] {
        let mut out = [0u8; EVENT_SIZE];
        out[0..8].copy_from_slice(&self.sec.to_le_bytes());
        out[8..16].copy_from_slice(&self.usec.to_le_bytes());
        out[16..18].copy_from_slice(&self.kind.to_le_bytes());
        out[18..20].copy_from_slice(&self.code.to_le_bytes());
        out[20..24].copy_from_slice(&self.value.to_le_bytes());
        out
    }

    /// A record with a zero timestamp, for tests and for building a synthetic
    /// stream.
    #[must_use]
    pub const fn new(kind: u16, code: u16, value: i32) -> Self {
        Self {
            sec: 0,
            usec: 0,
            kind,
            code,
            value,
        }
    }
}

// ---------------------------------------------------------------------------
// Keycode → scan code set 1
// ---------------------------------------------------------------------------

/// Translate a Linux keycode to the scan-code-set-1 code
/// [`keymap`](crate::keymap) expects, or `None` if there is no such key.
///
/// This is the exact inverse of the kernel's `evdev::set1_to_keycode` and
/// `set1_extended_to_keycode` (`kernel/src/evdev.rs`), and is kept an inverse
/// by [`tests::the_table_is_the_kernels_table_backwards`].
///
/// Two things make it more than a lookup:
///
/// * **The unextended range is the identity**, because Linux assigned keycodes
///   1..=0x58 to be the set-1 codes. That is a fact about the ABI, not a
///   coincidence this code may lean on quietly, so it is written out.
/// * **The extended range is not**, because the `0xE0`-prefixed keys were added
///   to the PC keyboard later and Linux numbered them from 96 upwards. Left
///   arrow is keycode 105 and scan code `0xE04B`; keypad 4 is keycode 75 and
///   scan code `0x4B`. Collapsing those two would move the caret every time
///   someone typed a 4 on the numeric pad.
///
/// A keycode with no set-1 equivalent — a media key on a USB device, anything
/// above the PS/2 range — returns `None`, and [`super::EvdevInput`] then reports
/// the raw `MSC_SCAN` if the device sent one and drops the event if it did not.
/// Guessing would be worse than silence: a fabricated scan code is a keypress
/// the user did not make.
#[must_use]
pub const fn set1_for_keycode(keycode: u16) -> Option<u32> {
    // The unextended range, where the keycode *is* the scan code.
    if keycode >= 1 && keycode <= 0x58 {
        return Some(keycode as u32);
    }
    // The extended range. `0xE000 | code` is the convention `keymap` uses and
    // the one the kernel already puts in `MSC_SCAN`, so the two agree by
    // construction.
    let extended: u32 = match keycode {
        96 => 0x1C,  // KEY_KPENTER
        97 => 0x1D,  // KEY_RIGHTCTRL
        98 => 0x35,  // KEY_KPSLASH
        99 => 0x37,  // KEY_SYSRQ (Print Screen)
        100 => 0x38, // KEY_RIGHTALT
        102 => 0x47, // KEY_HOME
        103 => 0x48, // KEY_UP
        104 => 0x49, // KEY_PAGEUP
        105 => 0x4B, // KEY_LEFT
        106 => 0x4D, // KEY_RIGHT
        107 => 0x4F, // KEY_END
        108 => 0x50, // KEY_DOWN
        109 => 0x51, // KEY_PAGEDOWN
        110 => 0x52, // KEY_INSERT
        111 => 0x53, // KEY_DELETE
        113 => 0x20, // KEY_MUTE
        114 => 0x2E, // KEY_VOLUMEDOWN
        115 => 0x30, // KEY_VOLUMEUP
        116 => 0x5E, // KEY_POWER
        119 => 0x46, // KEY_PAUSE
        125 => 0x5B, // KEY_LEFTMETA
        126 => 0x5C, // KEY_RIGHTMETA
        127 => 0x5D, // KEY_COMPOSE (Menu)
        128 => 0x68, // KEY_STOP
        140 => 0x21, // KEY_CALC
        142 => 0x5F, // KEY_SLEEP
        143 => 0x63, // KEY_WAKEUP
        150 => 0x32, // KEY_WWW
        155 => 0x6C, // KEY_MAIL
        156 => 0x66, // KEY_BOOKMARKS
        157 => 0x6B, // KEY_COMPUTER
        158 => 0x6A, // KEY_BACK
        159 => 0x69, // KEY_FORWARD
        161 => 0x6D, // KEY_MEDIA
        163 => 0x19, // KEY_NEXTSONG
        164 => 0x22, // KEY_PLAYPAUSE
        165 => 0x10, // KEY_PREVIOUSSONG
        166 => 0x24, // KEY_STOPCD
        173 => 0x67, // KEY_REFRESH
        217 => 0x65, // KEY_SEARCH
        _ => return None,
    };
    Some(0xE000 | extended)
}

/// Whether a `EV_KEY` code names a mouse button rather than a keyboard key.
///
/// The two share one event type and one code space in evdev, split at
/// [`BTN_MISC`]. A compositor that did not check would route `BTN_LEFT`
/// (0x110) through the keymap, where it is not a key at all.
#[must_use]
pub const fn is_button(code: u16) -> bool {
    code >= BTN_MISC
}

// ---------------------------------------------------------------------------
// `EVIOC*` request numbers
// ---------------------------------------------------------------------------

/// `_IOC_READ` — the kernel writes, userspace reads.
pub const IOC_READ: u32 = 2;

/// The `'E'` in every `EVIOC*` request.
const EVDEV_IOC_MAGIC: u32 = b'E' as u32;

/// `EVIOCGKEY` — which keys and buttons are held right now.
pub const EVIOC_NR_GKEY: u32 = 0x18;

/// `EVIOCGNAME` — the device's human-readable name.
pub const EVIOC_NR_GNAME: u32 = 0x06;

/// Encode an `EVIOC*` request number the way userspace's `_IOC` macro does.
///
/// `size` is part of the request number in the Linux ioctl ABI, which is why
/// `EVIOCGKEY` is not a constant: the length of the bitmap the caller is
/// willing to receive is encoded *in the request*, and a mismatch is how a
/// kernel tells a stale client from a current one.
#[must_use]
pub const fn ioc(dir: u32, nr: u32, size: u32) -> u32 {
    ((dir & 0x3) << 30) | ((size & 0x3FFF) << 16) | (EVDEV_IOC_MAGIC << 8) | (nr & 0xFF)
}

/// Whether bit `index` is set in a little-endian bitmap of the kind
/// `EVIOCGKEY` returns.
///
/// Out of range is `false` rather than an error: the kernel returns however
/// many bytes it has, and a client asking about a key beyond them is asking
/// about a key the device cannot report, whose honest answer is "not held".
#[must_use]
pub fn bit_set(bitmap: &[u8], index: u16) -> bool {
    let byte = usize::from(index) / 8;
    let bit = index % 8;
    bitmap.get(byte).is_some_and(|b| (b >> bit) & 1 == 1)
}

#[cfg(test)]
mod tests {
    // A test that indexes out of range should fail loudly and point at the
    // line that did it — that is the diagnosis. The defensive lints exist to
    // keep panics out of code that runs on a user's data, which this is not.
    #![allow(
        clippy::indexing_slicing,
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::arithmetic_side_effects
    )]

    use super::{
        BTN_LEFT, EV_KEY, EV_REL, EVENT_SIZE, EVIOC_NR_GKEY, EVIOC_NR_GNAME, IOC_READ, REL_X,
        Record, bit_set, ioc, is_button, set1_for_keycode,
    };
    use crate::keymap::key_for_scancode;
    use guitk::event::Key;

    #[test]
    fn a_record_survives_the_round_trip_through_its_own_bytes() {
        let original = Record {
            sec: 1_724_500_000,
            usec: 123_456,
            kind: EV_KEY,
            code: 30,
            value: 1,
        };
        let bytes = original.encode();
        assert_eq!(bytes.len(), EVENT_SIZE);
        assert_eq!(Record::decode(&bytes), Some(original));
    }

    #[test]
    fn every_field_is_at_the_offset_the_abi_puts_it_at() {
        // Not a round trip: a decoder and an encoder that are wrong in the same
        // way round-trip perfectly. These are the offsets from
        // `struct input_event`, asserted against bytes written by hand.
        let mut bytes = [0u8; EVENT_SIZE];
        bytes[0] = 0x07; // tv_sec low byte
        bytes[8] = 0x09; // tv_usec low byte
        bytes[16] = 0x02; // type = EV_REL
        bytes[18] = 0x01; // code = REL_Y
        bytes[20] = 0xFF; // value = -1, sign-extended
        bytes[21] = 0xFF;
        bytes[22] = 0xFF;
        bytes[23] = 0xFF;
        let record = Record::decode(&bytes).unwrap();
        assert_eq!(record.sec, 7);
        assert_eq!(record.usec, 9);
        assert_eq!(record.kind, EV_REL);
        assert_eq!(record.code, 1);
        assert_eq!(
            record.value, -1,
            "value is a signed 32-bit quantity; a mouse moving left or up \
             depends on it"
        );
    }

    #[test]
    fn a_short_slice_decodes_to_nothing_rather_than_reading_past_it() {
        for len in 0..EVENT_SIZE {
            assert_eq!(
                Record::decode(&[0u8; EVENT_SIZE][..len]),
                None,
                "{len} bytes is not a record"
            );
        }
        assert!(Record::decode(&[0u8; EVENT_SIZE]).is_some());
    }

    #[test]
    fn the_unextended_keycode_range_is_the_identity() {
        // Linux numbered these to *be* the set-1 codes. Spot-checked at both
        // ends and at a letter, because an off-by-one at either boundary would
        // be a whole key.
        assert_eq!(set1_for_keycode(1), Some(0x01)); // Escape
        assert_eq!(set1_for_keycode(30), Some(0x1E)); // A
        assert_eq!(set1_for_keycode(0x58), Some(0x58)); // F12
        assert_eq!(set1_for_keycode(0), None, "keycode 0 is 'no key'");
    }

    #[test]
    fn an_extended_key_keeps_the_prefix_that_distinguishes_it_from_the_keypad() {
        // The whole reason the extended half of the table exists, asserted
        // through the *real* keymap rather than on the number — because the
        // number is only interesting if it names the right key.
        let left = set1_for_keycode(105).unwrap();
        assert_eq!(left, 0xE04B);
        assert_eq!(key_for_scancode(left), Key::Left);

        // Keycode 75 is keypad 4: the same low byte, no prefix, a different
        // key. An extended bit dropped here would move the caret every time
        // someone typed a 4 on the numeric pad.
        let keypad_four = set1_for_keycode(75).unwrap();
        assert_eq!(keypad_four, 0x4B);
        assert_ne!(key_for_scancode(keypad_four), Key::Left);
    }

    #[test]
    fn the_modifiers_and_the_navigation_block_all_reach_the_keymap() {
        // These are the keys a text editor cares about most, and the ones an
        // incomplete table silently loses. Each is checked to the *named* key,
        // so a transposed pair in the table fails here rather than in a bug
        // report about Home going to the end of the line.
        for (keycode, expected) in [
            (103u16, Key::Up),
            (108, Key::Down),
            (105, Key::Left),
            (106, Key::Right),
            (102, Key::Home),
            (107, Key::End),
            (104, Key::PageUp),
            (109, Key::PageDown),
            (110, Key::Insert),
            (111, Key::Delete),
            (97, Key::RightCtrl),
            (100, Key::RightAlt),
            (125, Key::LeftSuper),
            (126, Key::RightSuper),
        ] {
            let scancode = set1_for_keycode(keycode).expect("keycode {keycode} has a scan code");
            assert_eq!(
                key_for_scancode(scancode),
                expected,
                "keycode {keycode} -> scancode {scancode:#06X}"
            );
        }
    }

    #[test]
    fn the_table_is_the_kernels_table_backwards() {
        // Every pair in `kernel/src/evdev.rs::set1_extended_to_keycode`,
        // written the other way round. Transcribed rather than computed on
        // purpose: this test's job is to be an independent copy of the kernel's
        // table, so that a typo in `set1_for_keycode` has to be made twice to
        // go unnoticed.
        const KERNEL_TABLE: &[(u8, u16)] = &[
            (0x10, 165),
            (0x19, 163),
            (0x1C, 96),
            (0x1D, 97),
            (0x20, 113),
            (0x21, 140),
            (0x22, 164),
            (0x24, 166),
            (0x2E, 114),
            (0x30, 115),
            (0x32, 150),
            (0x35, 98),
            (0x37, 99),
            (0x38, 100),
            (0x46, 119),
            (0x47, 102),
            (0x48, 103),
            (0x49, 104),
            (0x4B, 105),
            (0x4D, 106),
            (0x4F, 107),
            (0x50, 108),
            (0x51, 109),
            (0x52, 110),
            (0x53, 111),
            (0x5B, 125),
            (0x5C, 126),
            (0x5D, 127),
            (0x5E, 116),
            (0x5F, 142),
            (0x63, 143),
            (0x65, 217),
            (0x66, 156),
            (0x67, 173),
            (0x68, 128),
            (0x69, 159),
            (0x6A, 158),
            (0x6B, 157),
            (0x6C, 155),
            (0x6D, 161),
        ];
        for &(scancode, keycode) in KERNEL_TABLE {
            assert_eq!(
                set1_for_keycode(keycode),
                Some(0xE000 | u32::from(scancode)),
                "keycode {keycode} should come back as scan code E0-{scancode:02X}"
            );
        }
        // …and nothing outside it invents an extended code. Every keycode above
        // the unextended range that is not in the table must be `None`.
        for keycode in 0x59u16..=300 {
            let known = KERNEL_TABLE.iter().any(|&(_, k)| k == keycode);
            assert_eq!(
                set1_for_keycode(keycode).is_some(),
                known,
                "keycode {keycode} is {} in the kernel's table",
                if known { "present" } else { "absent" }
            );
        }
    }

    #[test]
    fn keyboard_keys_and_mouse_buttons_are_told_apart() {
        assert!(!is_button(30), "keycode 30 is A");
        assert!(!is_button(0x58), "keycode 0x58 is F12");
        assert!(is_button(BTN_LEFT));
        assert!(is_button(0x100), "BTN_MISC is the first button");
        assert_eq!(
            set1_for_keycode(BTN_LEFT),
            None,
            "a button must not be mistaken for a key with a scan code"
        );
    }

    #[test]
    fn an_ioctl_number_is_the_one_a_real_client_would_send() {
        // Checked against the literals a Linux header expands to, which is what
        // makes this a test of the encoding rather than a restatement of it.
        // EVIOCGNAME(64) is _IOC(_IOC_READ, 'E', 0x06, 64) = 0x8040_4506.
        assert_eq!(ioc(IOC_READ, EVIOC_NR_GNAME, 64), 0x8040_4506);
        // EVIOCGKEY(96) = _IOC(_IOC_READ, 'E', 0x18, 96) = 0x8060_4518.
        assert_eq!(ioc(IOC_READ, EVIOC_NR_GKEY, 96), 0x8060_4518);
    }

    #[test]
    fn a_key_bitmap_is_read_bit_by_bit_and_never_past_its_end() {
        // Bit 30 (`A`) set, nothing else: byte 3, bit 6.
        let mut bitmap = [0u8; 8];
        bitmap[3] = 1 << 6;
        assert!(bit_set(&bitmap, 30));
        assert!(!bit_set(&bitmap, 29));
        assert!(!bit_set(&bitmap, 31));
        assert!(
            !bit_set(&bitmap, 1000),
            "a key beyond the bitmap is not held, and asking must not panic"
        );
        assert!(!bit_set(&[], 0));
    }

    #[test]
    fn a_relative_axis_record_is_recognisable_as_one() {
        let record = Record::new(EV_REL, REL_X, -7);
        let decoded = Record::decode(&record.encode()).unwrap();
        assert_eq!(decoded.kind, EV_REL);
        assert_eq!(decoded.code, REL_X);
        assert_eq!(decoded.value, -7);
    }
}
