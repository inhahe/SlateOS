//! ALSA control-device ABI definitions — the `/dev/snd/controlC0` interface
//! that ALSA-lib opens first to enumerate the sound card and to read/write
//! mixer controls (roadmap §5.1 / §5095).
//!
//! ## Why this exists
//!
//! Before a Linux audio client touches a PCM stream it opens the card's
//! **control device** (`/dev/snd/controlC0`) and drives it through
//! `ioctl(SNDRV_CTL_IOCTL_*)`: `CARD_INFO` to identify the card, then
//! `ELEM_LIST` / `ELEM_INFO` / `ELEM_READ` / `ELEM_WRITE` to enumerate and
//! manipulate the mixer's control elements (master volume, mute, …).
//! `snd_ctl_open()` — called by `alsamixer`, `amixer`, PulseAudio's ALSA
//! backend, and `snd_card_load()` — fails outright without this device, so
//! providing it is what makes the card *visible* to the Linux audio stack.
//!
//! This module is the **pure ABI layer**, mirroring [`crate::audio_alsa`] for
//! the PCM side: the control ioctl request-number encoding, the byte-exact
//! `#[repr(C)]` payload structs from `include/uapi/sound/asound.h`, and the
//! element type/interface/access constants.  It contains no device nodes, no
//! per-fd state, and no `unsafe` — the devfs node and the ioctl dispatch land
//! in the follow-up wiring commits.  Keeping the ABI surface pure and
//! exhaustively self-tested means the wiring builds on a verified,
//! byte-accurate foundation.
//!
//! ## ABI accuracy
//!
//! The struct layouts and constants below are fixed by `asound.h` on a 64-bit
//! target (`long` / pointer = 8 bytes) and must stay byte-identical, because
//! the struct-carrying ioctls encode `sizeof(struct)` in their request number —
//! a one-byte layout error makes real ALSA-lib's ioctl number miss ours.  Each
//! struct's size and each ioctl's encoding is asserted against its known Linux
//! value in [`self_test`].

#![allow(dead_code)] // ABI surface; consumers land in follow-up wiring commits.

use crate::serial_println;

// ---------------------------------------------------------------------------
// ioctl request-number encoding (same scheme as the PCM side, magic 'U')
// ---------------------------------------------------------------------------

const IOC_NRBITS: u32 = 8;
const IOC_TYPEBITS: u32 = 8;
const IOC_SIZEBITS: u32 = 14;

const IOC_NRSHIFT: u32 = 0;
const IOC_TYPESHIFT: u32 = IOC_NRSHIFT + IOC_NRBITS; // 8
const IOC_SIZESHIFT: u32 = IOC_TYPESHIFT + IOC_TYPEBITS; // 16
const IOC_DIRSHIFT: u32 = IOC_SIZESHIFT + IOC_SIZEBITS; // 30

const IOC_NONE: u32 = 0;
const IOC_WRITE: u32 = 1;
const IOC_READ: u32 = 2;

/// The ALSA control ioctl "magic" letter (`'U'`).
const SNDRV_CTL_IOCTL_MAGIC: u32 = 0x55; // b'U'

/// Encode an ioctl request number from its `(dir, type, nr, size)` tuple,
/// matching Linux's `_IOC(dir, type, nr, size)` macro.  Pure bit-twiddling, so
/// it stays `const` and clippy-clean.
const fn ioc(dir: u32, ty: u32, nr: u32, size: u32) -> u32 {
    (dir << IOC_DIRSHIFT)
        | (ty << IOC_TYPESHIFT)
        | (nr << IOC_NRSHIFT)
        | ((size & ((1 << IOC_SIZEBITS) - 1)) << IOC_SIZESHIFT)
}

/// `_IOR('U', nr, int)` — kernel returns a 4-byte int.
const fn ior_int(nr: u32) -> u32 {
    ioc(IOC_READ, SNDRV_CTL_IOCTL_MAGIC, nr, 4)
}

/// The ALSA control protocol version we advertise (`SNDRV_CTL_VERSION`).
///
/// `SNDRV_PROTOCOL_VERSION(2, 0, 9)` = `(2 << 16) | (0 << 8) | 9`.  Encoded as
/// a literal to stay clippy-clean (no arithmetic in a `const`).
pub const SNDRV_CTL_VERSION: u32 = 0x0002_0009;

// ---------------------------------------------------------------------------
// Control-element interface / type / access constants
// ---------------------------------------------------------------------------

/// `SNDRV_CTL_ELEM_IFACE_CARD` — a global card control.
pub const SNDRV_CTL_ELEM_IFACE_CARD: i32 = 0;
/// `SNDRV_CTL_ELEM_IFACE_HWDEP` — a hardware-dependent device control.
pub const SNDRV_CTL_ELEM_IFACE_HWDEP: i32 = 1;
/// `SNDRV_CTL_ELEM_IFACE_MIXER` — a mixer control (where volume/mute live).
pub const SNDRV_CTL_ELEM_IFACE_MIXER: i32 = 2;
/// `SNDRV_CTL_ELEM_IFACE_PCM` — a PCM control.
pub const SNDRV_CTL_ELEM_IFACE_PCM: i32 = 3;

/// `SNDRV_CTL_ELEM_TYPE_NONE`.
pub const SNDRV_CTL_ELEM_TYPE_NONE: i32 = 0;
/// `SNDRV_CTL_ELEM_TYPE_BOOLEAN` — an on/off control (e.g. mute switch).
pub const SNDRV_CTL_ELEM_TYPE_BOOLEAN: i32 = 1;
/// `SNDRV_CTL_ELEM_TYPE_INTEGER` — a ranged integer control (e.g. volume).
pub const SNDRV_CTL_ELEM_TYPE_INTEGER: i32 = 2;
/// `SNDRV_CTL_ELEM_TYPE_ENUMERATED` — a one-of-N control.
pub const SNDRV_CTL_ELEM_TYPE_ENUMERATED: i32 = 3;
/// `SNDRV_CTL_ELEM_TYPE_BYTES` — an opaque byte array.
pub const SNDRV_CTL_ELEM_TYPE_BYTES: i32 = 4;
/// `SNDRV_CTL_ELEM_TYPE_INTEGER64` — a ranged 64-bit-integer control.
pub const SNDRV_CTL_ELEM_TYPE_INTEGER64: i32 = 6;

/// `SNDRV_CTL_ELEM_ACCESS_READ` — the element value can be read.
pub const SNDRV_CTL_ELEM_ACCESS_READ: u32 = 1 << 0;
/// `SNDRV_CTL_ELEM_ACCESS_WRITE` — the element value can be written.
pub const SNDRV_CTL_ELEM_ACCESS_WRITE: u32 = 1 << 1;
/// `SNDRV_CTL_ELEM_ACCESS_VOLATILE` — value may change without notification.
pub const SNDRV_CTL_ELEM_ACCESS_VOLATILE: u32 = 1 << 2;
/// Convenience: a readable + writable element.
pub const SNDRV_CTL_ELEM_ACCESS_READWRITE: u32 =
    SNDRV_CTL_ELEM_ACCESS_READ | SNDRV_CTL_ELEM_ACCESS_WRITE;

// ---------------------------------------------------------------------------
// Byte-exact `#[repr(C)]` mirrors of the `asound.h` control payload structs
// ---------------------------------------------------------------------------

/// `struct snd_ctl_card_info` — card identification for `CARD_INFO`
/// (376 bytes).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct SndCtlCardInfo {
    /// Card number.
    pub card: i32,
    /// Reserved (was `type`); must be zero.
    pub pad: i32,
    /// User-selectable card ID string.
    pub id: [u8; 16],
    /// Driver name.
    pub driver: [u8; 16],
    /// Short card name.
    pub name: [u8; 32],
    /// Long card name (name + info text).
    pub longname: [u8; 80],
    /// Reserved (was mixer ID); must be zero.
    pub reserved_: [u8; 16],
    /// Visual mixer identification.
    pub mixername: [u8; 80],
    /// Card components / fine identification, space-delimited.
    pub components: [u8; 128],
}

/// `struct snd_ctl_elem_id` — identifies one control element (64 bytes).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct SndCtlElemId {
    /// Numeric identifier; zero = invalid.
    pub numid: u32,
    /// Interface (`SNDRV_CTL_ELEM_IFACE_*`).
    pub iface: i32,
    /// Device/client number.
    pub device: u32,
    /// Subdevice (substream) number.
    pub subdevice: u32,
    /// ASCII name of the element.
    pub name: [u8; 44],
    /// Index of the element within its name group.
    pub index: u32,
}

/// `struct snd_ctl_elem_list` — enumerate control element IDs (`ELEM_LIST`,
/// 80 bytes).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct SndCtlElemList {
    /// W: first element index to return.
    pub offset: u32,
    /// W: number of element IDs the caller's buffer can hold.
    pub space: u32,
    /// R: number of element IDs actually written.
    pub used: u32,
    /// R: total number of elements on the card.
    pub count: u32,
    /// W: user pointer to a `snd_ctl_elem_id[space]` array (as an address).
    pub pids: u64,
    /// Reserved, must be zero.
    pub reserved: [u8; 50],
}

/// `struct snd_ctl_elem_info` — describe one control element (`ELEM_INFO`,
/// 272 bytes).
///
/// The `value` union is modelled by its integer arm (`min`/`max`/`step`)
/// followed by reserved padding to the union's full 128 bytes; the leading
/// `i64` forces the same 8-byte alignment (and thus the 4-byte pad after
/// `owner`) that the C union's `long` members impose, so the layout is
/// byte-exact.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct SndCtlElemInfo {
    /// W: element ID.
    pub id: SndCtlElemId,
    /// R: value type (`SNDRV_CTL_ELEM_TYPE_*`).
    pub r#type: i32,
    /// R: access bitmask (`SNDRV_CTL_ELEM_ACCESS_*`).
    pub access: u32,
    /// Count of values in the element (1 for master volume/mute).
    pub count: u32,
    /// Owning PID (0 = unowned).
    pub owner: i32,
    /// `value.integer.min` (union arm; 8-aligned).
    pub value_integer_min: i64,
    /// `value.integer.max`.
    pub value_integer_max: i64,
    /// `value.integer.step` (0 = continuous).
    pub value_integer_step: i64,
    /// Remainder of the 128-byte `value` union.
    pub value_reserved: [u8; 104],
    /// `dimen` union (dimensions); unused, zero.
    pub dimen: [u16; 4],
    /// Reserved, must be zero.
    pub reserved: [u8; 56],
}

/// `struct snd_ctl_elem_value` — read/write a control element's value
/// (`ELEM_READ` / `ELEM_WRITE`, 1224 bytes).
///
/// The `value` union is modelled by its integer arm (`long value[128]`); the
/// `i64` array forces the union's 8-byte alignment, reproducing the 4-byte pad
/// after the `indirect` bitfield word exactly.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct SndCtlElemValue {
    /// W: element ID.
    pub id: SndCtlElemId,
    /// `unsigned int indirect:1` bitfield container (obsolete; we treat bit0 as
    /// the indirect flag, the rest as zero).
    pub indirect: u32,
    /// `value.integer.value[128]` (and the rest of the union — BOOLEAN and
    /// ENUMERATED arms alias the same `long`/`int` storage).
    pub value_integer: [i64; 128],
    /// Reserved, must be zero.
    pub reserved: [u8; 128],
}

/// Size of a control payload struct as a `u32` for ioctl-number encoding.
///
/// Every struct here is well under the 14-bit `_IOC` size field, so the cast
/// cannot truncate; the bound is checked in [`self_test`].
#[allow(clippy::cast_possible_truncation)]
const fn struct_size<T>() -> u32 {
    core::mem::size_of::<T>() as u32
}

// --- Control ioctls -------------------------------------------------------

/// `PVERSION` — query the control protocol version (`int` out).
pub const SNDRV_CTL_IOCTL_PVERSION: u32 = ior_int(0x00);
/// `CARD_INFO` — query card identification (`_IOR`).
pub const SNDRV_CTL_IOCTL_CARD_INFO: u32 =
    ioc(IOC_READ, SNDRV_CTL_IOCTL_MAGIC, 0x01, struct_size::<SndCtlCardInfo>());
/// `ELEM_LIST` — enumerate control element IDs (`_IOWR`).
pub const SNDRV_CTL_IOCTL_ELEM_LIST: u32 = ioc(
    IOC_READ | IOC_WRITE,
    SNDRV_CTL_IOCTL_MAGIC,
    0x10,
    struct_size::<SndCtlElemList>(),
);
/// `ELEM_INFO` — describe one control element (`_IOWR`).
pub const SNDRV_CTL_IOCTL_ELEM_INFO: u32 = ioc(
    IOC_READ | IOC_WRITE,
    SNDRV_CTL_IOCTL_MAGIC,
    0x11,
    struct_size::<SndCtlElemInfo>(),
);
/// `ELEM_READ` — read a control element's value (`_IOWR`).
pub const SNDRV_CTL_IOCTL_ELEM_READ: u32 = ioc(
    IOC_READ | IOC_WRITE,
    SNDRV_CTL_IOCTL_MAGIC,
    0x12,
    struct_size::<SndCtlElemValue>(),
);
/// `ELEM_WRITE` — write a control element's value (`_IOWR`).
pub const SNDRV_CTL_IOCTL_ELEM_WRITE: u32 = ioc(
    IOC_READ | IOC_WRITE,
    SNDRV_CTL_IOCTL_MAGIC,
    0x13,
    struct_size::<SndCtlElemValue>(),
);

// ---------------------------------------------------------------------------
// Our card's control-element model
// ---------------------------------------------------------------------------
//
// The SlateOS virtual card exposes the two controls every mixer UI expects: a
// "Master Playback Volume" integer (0..100, matching audio_mixer's scale) and
// a "Master Playback Switch" boolean (the mute toggle, inverted: 1 = unmuted).
// The numids are stable 1-based identifiers, as Linux assigns them.

/// `numid` of the master playback volume control.
pub const NUMID_MASTER_VOLUME: u32 = 1;
/// `numid` of the master playback switch (unmute) control.
pub const NUMID_MASTER_SWITCH: u32 = 2;
/// Total number of control elements the card exposes.
pub const ELEM_COUNT: u32 = 2;

/// The maximum value of the master volume control (matches the
/// `audio_mixer` 0..=100 percentage scale).
pub const MASTER_VOLUME_MAX: i64 = 100;

/// ASCII name of the master playback volume control, as ALSA-lib expects it.
pub const MASTER_VOLUME_NAME: &[u8] = b"Master Playback Volume";
/// ASCII name of the master playback switch (unmute) control.
pub const MASTER_SWITCH_NAME: &[u8] = b"Master Playback Switch";

/// Compare a NUL-padded fixed-size control-element name field against a
/// desired ASCII name, matching up to the field's first NUL.
fn name_field_matches(field: &[u8; 44], want: &[u8]) -> bool {
    let used = field.iter().position(|&b| b == 0).unwrap_or(field.len());
    field.get(..used) == Some(want)
}

/// Resolve a caller-supplied [`SndCtlElemId`] to one of our element numids
/// ([`NUMID_MASTER_VOLUME`] or [`NUMID_MASTER_SWITCH`]), or `0` if it matches
/// no element on the card.
///
/// ALSA identifies an element either by its `numid` (when non-zero) or, when
/// `numid == 0`, by the `iface + name + index` tuple (plus device/subdevice,
/// which are always 0 for our card-global mixer controls).  We honour both
/// forms so that clients that cache a numid and clients that look up by name
/// both resolve correctly.
#[must_use]
pub fn resolve_numid(id: &SndCtlElemId) -> u32 {
    // numid form: accept only the two we expose.
    if id.numid == NUMID_MASTER_VOLUME || id.numid == NUMID_MASTER_SWITCH {
        return id.numid;
    }
    if id.numid != 0 {
        return 0; // a non-zero numid we don't recognise
    }
    // name form: must be a card-global mixer control at index 0.
    if id.iface != SNDRV_CTL_ELEM_IFACE_MIXER
        || id.index != 0
        || id.device != 0
        || id.subdevice != 0
    {
        return 0;
    }
    if name_field_matches(&id.name, MASTER_VOLUME_NAME) {
        NUMID_MASTER_VOLUME
    } else if name_field_matches(&id.name, MASTER_SWITCH_NAME) {
        NUMID_MASTER_SWITCH
    } else {
        0
    }
}

/// Build the canonical [`SndCtlElemId`] for one of our numids.  For an unknown
/// numid the returned id carries only that numid (name empty), which a caller
/// can treat as "no such element".
#[must_use]
pub fn elem_id_for(numid: u32) -> SndCtlElemId {
    let mut id = SndCtlElemId {
        numid,
        iface: SNDRV_CTL_ELEM_IFACE_MIXER,
        device: 0,
        subdevice: 0,
        name: [0u8; 44],
        index: 0,
    };
    let name: &[u8] = match numid {
        NUMID_MASTER_VOLUME => MASTER_VOLUME_NAME,
        NUMID_MASTER_SWITCH => MASTER_SWITCH_NAME,
        _ => &[],
    };
    let n = name.len().min(id.name.len().saturating_sub(1));
    if let (Some(d), Some(s)) = (id.name.get_mut(..n), name.get(..n)) {
        d.copy_from_slice(s);
    }
    id
}

/// Populate a [`SndCtlElemInfo`] describing the control with `numid`, mirroring
/// Linux's `snd_ctl_*_info` helpers: the master volume is a mono INTEGER on
/// `0..=MASTER_VOLUME_MAX`, the master switch a mono BOOLEAN on `0..=1`; both
/// are read-write with a single value.  Returns `false` (leaving `info`
/// untouched) for an unknown numid.
#[must_use]
pub fn fill_elem_info(numid: u32, info: &mut SndCtlElemInfo) -> bool {
    let (ty, max) = match numid {
        NUMID_MASTER_VOLUME => (SNDRV_CTL_ELEM_TYPE_INTEGER, MASTER_VOLUME_MAX),
        NUMID_MASTER_SWITCH => (SNDRV_CTL_ELEM_TYPE_BOOLEAN, 1),
        _ => return false,
    };
    info.id = elem_id_for(numid);
    info.r#type = ty;
    info.access = SNDRV_CTL_ELEM_ACCESS_READWRITE;
    info.count = 1;
    info.owner = 0;
    info.value_integer_min = 0;
    info.value_integer_max = max;
    info.value_integer_step = 0;
    true
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Exhaustive boot self-test for the control ABI layer.  Returns `Err` after
/// printing a `FAIL` line so the kernel boot self-test can surface a regression
/// without panicking.
///
/// # Errors
/// Returns [`crate::error::KernelError::InternalError`] if any ABI constant,
/// struct size, or ioctl encoding does not match its known Linux value.
pub fn self_test() -> crate::error::KernelResult<()> {
    use crate::error::KernelError;
    use core::mem::size_of;

    macro_rules! check {
        ($cond:expr, $($arg:tt)*) => {
            if !($cond) {
                serial_println!("[alsactl] FAIL: {}", format_args!($($arg)*));
                return Err(KernelError::InternalError);
            }
        };
    }

    // --- byte-exact struct layouts vs Linux asound.h (64-bit) ------------
    check!(
        size_of::<SndCtlCardInfo>() == 376,
        "snd_ctl_card_info size {}",
        size_of::<SndCtlCardInfo>()
    );
    check!(
        size_of::<SndCtlElemId>() == 64,
        "snd_ctl_elem_id size {}",
        size_of::<SndCtlElemId>()
    );
    check!(
        size_of::<SndCtlElemList>() == 80,
        "snd_ctl_elem_list size {}",
        size_of::<SndCtlElemList>()
    );
    check!(
        size_of::<SndCtlElemInfo>() == 272,
        "snd_ctl_elem_info size {}",
        size_of::<SndCtlElemInfo>()
    );
    check!(
        size_of::<SndCtlElemValue>() == 1224,
        "snd_ctl_elem_value size {}",
        size_of::<SndCtlElemValue>()
    );

    // The integer value arm must sit at the union's start (offset 80 in
    // elem_info, 72 in elem_value) — i.e. correctly 8-aligned after the
    // preceding scalar fields.
    check!(
        core::mem::align_of::<SndCtlElemInfo>() == 8,
        "elem_info must be 8-aligned"
    );
    check!(
        core::mem::align_of::<SndCtlElemValue>() == 8,
        "elem_value must be 8-aligned"
    );

    // --- ioctl encodings against known Linux hex values ------------------
    check!(
        SNDRV_CTL_IOCTL_PVERSION == 0x8004_5500,
        "PVERSION enc {:#x}",
        SNDRV_CTL_IOCTL_PVERSION
    );
    check!(
        SNDRV_CTL_IOCTL_CARD_INFO == 0x8178_5501,
        "CARD_INFO enc {:#x}",
        SNDRV_CTL_IOCTL_CARD_INFO
    );
    check!(
        SNDRV_CTL_IOCTL_ELEM_LIST == 0xC050_5510,
        "ELEM_LIST enc {:#x}",
        SNDRV_CTL_IOCTL_ELEM_LIST
    );
    check!(
        SNDRV_CTL_IOCTL_ELEM_INFO == 0xC110_5511,
        "ELEM_INFO enc {:#x}",
        SNDRV_CTL_IOCTL_ELEM_INFO
    );
    check!(
        SNDRV_CTL_IOCTL_ELEM_READ == 0xC4C8_5512,
        "ELEM_READ enc {:#x}",
        SNDRV_CTL_IOCTL_ELEM_READ
    );
    check!(
        SNDRV_CTL_IOCTL_ELEM_WRITE == 0xC4C8_5513,
        "ELEM_WRITE enc {:#x}",
        SNDRV_CTL_IOCTL_ELEM_WRITE
    );

    // --- version + interface/type/access constants ----------------------
    check!(SNDRV_CTL_VERSION == 0x0002_0009, "version literal wrong");
    check!(SNDRV_CTL_ELEM_IFACE_MIXER == 2, "IFACE_MIXER");
    check!(SNDRV_CTL_ELEM_TYPE_BOOLEAN == 1, "TYPE_BOOLEAN");
    check!(SNDRV_CTL_ELEM_TYPE_INTEGER == 2, "TYPE_INTEGER");
    check!(
        SNDRV_CTL_ELEM_ACCESS_READWRITE == 3,
        "ACCESS_READWRITE = READ|WRITE"
    );

    // --- card's element model -------------------------------------------
    check!(ELEM_COUNT == 2, "two control elements");
    check!(NUMID_MASTER_VOLUME == 1 && NUMID_MASTER_SWITCH == 2, "numids");
    check!(MASTER_VOLUME_MAX == 100, "volume scale matches mixer");

    // --- element id resolution ------------------------------------------
    // numid form resolves directly.
    let mut probe = elem_id_for(NUMID_MASTER_VOLUME);
    check!(resolve_numid(&probe) == NUMID_MASTER_VOLUME, "resolve vol by numid");
    check!(probe.iface == SNDRV_CTL_ELEM_IFACE_MIXER, "vol iface is MIXER");
    check!(
        name_field_matches(&probe.name, MASTER_VOLUME_NAME),
        "vol name populated"
    );
    check!(
        resolve_numid(&elem_id_for(NUMID_MASTER_SWITCH)) == NUMID_MASTER_SWITCH,
        "resolve switch by numid"
    );
    // An unknown numid resolves to 0.
    probe.numid = 99;
    probe.name = [0u8; 44];
    check!(resolve_numid(&probe) == 0, "unknown numid -> 0");
    // name form: numid 0 + iface MIXER + matching name resolves.
    probe.numid = 0;
    probe.iface = SNDRV_CTL_ELEM_IFACE_MIXER;
    let n = MASTER_SWITCH_NAME.len().min(probe.name.len().saturating_sub(1));
    if let (Some(d), Some(s)) = (probe.name.get_mut(..n), MASTER_SWITCH_NAME.get(..n)) {
        d.copy_from_slice(s);
    }
    check!(resolve_numid(&probe) == NUMID_MASTER_SWITCH, "resolve switch by name");
    // wrong iface with the right name does not resolve.
    probe.iface = SNDRV_CTL_ELEM_IFACE_PCM;
    check!(resolve_numid(&probe) == 0, "name form requires MIXER iface");

    // --- element info population ----------------------------------------
    // SAFETY: SndCtlElemInfo is a plain `#[repr(C)]` integer/byte aggregate, so
    // an all-zero value is a valid initialised value.
    let mut info: SndCtlElemInfo = unsafe { core::mem::zeroed() };
    check!(fill_elem_info(NUMID_MASTER_VOLUME, &mut info), "fill vol info");
    check!(info.r#type == SNDRV_CTL_ELEM_TYPE_INTEGER, "vol is INTEGER");
    check!(info.count == 1, "vol count 1");
    check!(info.access == SNDRV_CTL_ELEM_ACCESS_READWRITE, "vol rw");
    check!(
        info.value_integer_min == 0 && info.value_integer_max == MASTER_VOLUME_MAX,
        "vol range 0..max"
    );
    check!(fill_elem_info(NUMID_MASTER_SWITCH, &mut info), "fill switch info");
    check!(info.r#type == SNDRV_CTL_ELEM_TYPE_BOOLEAN, "switch is BOOLEAN");
    check!(
        info.value_integer_min == 0 && info.value_integer_max == 1,
        "switch range 0..1"
    );
    check!(!fill_elem_info(99, &mut info), "unknown numid info -> false");

    serial_println!("[alsactl] ALSA control ABI self-test PASSED");
    Ok(())
}
