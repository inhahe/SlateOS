//! util-linux 2.39.3's libfdisk, ported: the library under `fdisk`,
//! `sfdisk` and `cfdisk`.
//!
//! # What it is
//!
//! A [`Context`] is bound to a device (or an image file). Assigning it
//! discovers the device's topology and geometry, reads its first sector,
//! checks it for signatures a new table would hide, and probes the five
//! label drivers -- GPT, DOS (MBR), BSD, SGI and Sun -- in that order; the
//! first that recognises the device becomes the current label. Every
//! operation then works on that driver's **in-memory** copy of the table:
//! adding, deleting, resizing and retyping partitions, toggling flags,
//! creating a new table, verifying one. Only [`Context::write_disklabel`]
//! writes anything to the device.
//!
//! [`Partition`] and [`Table`] are the label-independent view: a partition
//! read out of the table, a template for a new one, or a line of an sfdisk
//! script. Changing one changes nothing on the label.
//!
//! What the library cannot decide it asks: a partition number, a first and
//! last sector, yes or no, a menu choice. Questions go through the
//! [`Ask`] callback a program sets with [`Context::set_ask`], and so do the
//! library's informational messages and warnings (`fdisk_info`,
//! `fdisk_warn`, `fdisk_warnx`), so that the program owns every byte that
//! reaches the terminal.
//!
//! # How the C maps to Rust
//!
//! The port follows the C file by file (`context.c` is [`context`],
//! `dos.c` is [`dos`], ...), function by function, keeping upstream's
//! names (`fdisk_add_partition` is [`Context::add_partition`]) and its
//! return conventions: most functions return an `i32` that is 0 on success,
//! a negative errno on failure, and for some a positive status (1: "not
//! found", "nothing to do", "out of range"), because callers branch on all
//! three.
//!
//! * Upstream's reference-counted objects are plain values: a
//!   [`Partition`] is cloned where C takes a reference, and a [`Table`]
//!   owns its partitions. Where C compares two partition pointers, the port
//!   compares positions in the table.
//! * A label driver is a [`Label`] -- the fields `struct fdisk_label`
//!   shares -- holding its private data in [`LabelData`]. The operations
//!   C reaches through `lb->op` are dispatched on the label's type in
//!   [`label`].
//! * A nested context (the BSD label inside a DOS partition) **owns** its
//!   parent: [`Context::new_nested`] consumes the parent and
//!   [`Context::into_parent`] gives it back, which is the life cycle
//!   `fdisk` gives them. Parent and child share the device and the
//!   in-memory first sector, as in C.
//! * Unsigned arithmetic wraps as C's does (`wrapping_*`), and a division
//!   C would trap on (by a zero sector size) yields 0 instead
//!   ([`utils::div`]).
//! * Strings that come from the device or the user -- partition names,
//!   type strings, paths -- are bytes, not `str`.
//!
//! # Not ported
//!
//! The debug masks (`LIBFDISK_DEBUG`): they write to stderr only when an
//! environment variable asks, and nothing measures them. The translations:
//! every message is upstream's English, as Ubuntu prints it under `LC_ALL=C`.

pub mod alignment;
pub mod ask;
pub mod bsd;
pub mod context;
pub mod dos;
pub mod field;
pub mod gpt;
mod gpt_parttypes;
pub mod item;
pub mod label;
pub mod partition;
pub mod parttype;
pub mod script;
pub mod sgi;
pub mod sun;
pub mod table;
pub mod utils;
pub mod uuid;
pub mod wipe;

pub use ask::{Ask, AskCallback, AskData, AskType};
pub use context::Context;
pub use field::Field;
pub use item::LabelItem;
pub use label::{Label, LabelData};
pub use partition::Partition;
pub use parttype::{PartType, Shortcut};
pub use table::Table;

/// `fdisk_sector_t`: an LBA address or a count of sectors.
pub type Sector = u64;

/// `FDISK_INIT_UNDEF` for a 64-bit field: "not set". Zero is a valid start,
/// size and partition number, so "unset" is all ones, as in C.
pub const UNDEF: u64 = u64::MAX;

/// Whether `x` is [`UNDEF`] (`FDISK_IS_UNDEF`).
#[must_use]
pub const fn is_undef(x: u64) -> bool {
    x == UNDEF
}

/// The errno values libfdisk returns, negated. Linux's numbers, which are
/// what `strerror` describes and what the programs compare against.
pub mod errno {
    /// No such file or directory.
    pub const ENOENT: i32 = 2;
    /// Input/output error.
    pub const EIO: i32 = 5;
    /// Bad file descriptor.
    pub const EBADF: i32 = 9;
    /// Cannot allocate memory.
    pub const ENOMEM: i32 = 12;
    /// Device or resource busy.
    pub const EBUSY: i32 = 16;
    /// Invalid argument.
    pub const EINVAL: i32 = 22;
    /// No space left on device.
    pub const ENOSPC: i32 = 28;
    /// Numerical result out of range.
    pub const ERANGE: i32 = 34;
    /// Function not implemented.
    pub const ENOSYS: i32 = 38;
    /// Operation not supported.
    pub const ENOTSUP: i32 = 95;
    /// Operation canceled.
    pub const ECANCELED: i32 = 125;
}

/// `enum fdisk_labeltype`: which driver a label is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LabelType {
    /// MBR.
    Dos = 1 << 1,
    /// Sun.
    Sun = 1 << 2,
    /// SGI.
    Sgi = 1 << 3,
    /// BSD.
    Bsd = 1 << 4,
    /// UEFI GPT.
    Gpt = 1 << 5,
}

/// `struct fdisk_geometry`: legacy cylinders/heads/sectors.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Geometry {
    /// Heads per cylinder.
    pub heads: u32,
    /// Sectors per track.
    pub sectors: Sector,
    /// Cylinders.
    pub cylinders: Sector,
}

/// `FDISK_PLURAL` and `FDISK_SINGULAR`, for [`Context::get_unit`].
pub const PLURAL: i32 = 0;
/// See [`PLURAL`].
pub const SINGULAR: i32 = 1;

/// `FDISK_SIZEUNIT_HUMAN`: sizes as `100M`.
pub const SIZEUNIT_HUMAN: i32 = 0;
/// `FDISK_SIZEUNIT_BYTES`: sizes in bytes.
pub const SIZEUNIT_BYTES: i32 = 1;

/// `FDISK_ALIGN_UP`: align to the next grain boundary.
pub const ALIGN_UP: i32 = 1;
/// `FDISK_ALIGN_DOWN`: align to the previous grain boundary.
pub const ALIGN_DOWN: i32 = 2;
/// `FDISK_ALIGN_NEAREST`: align to the nearer boundary.
pub const ALIGN_NEAREST: i32 = 3;

/// `FDISK_GPT_NPARTITIONS_DEFAULT`: entries in a new GPT's array.
pub const GPT_NPARTITIONS_DEFAULT: u32 = 128;

/// `enum fdisk_fieldtype`: what a column of a partition listing shows.
pub mod fieldtype {
    /// Unspecified.
    pub const NONE: i32 = 0;
    /// Partition device name.
    pub const DEVICE: i32 = 1;
    /// Start offset.
    pub const START: i32 = 2;
    /// End offset.
    pub const END: i32 = 3;
    /// Number of sectors.
    pub const SECTORS: i32 = 4;
    /// Number of cylinders (deprecated).
    pub const CYLINDERS: i32 = 5;
    /// Partition size.
    pub const SIZE: i32 = 6;
    /// Partition type.
    pub const TYPE: i32 = 7;
    /// Partition type ID.
    pub const TYPEID: i32 = 8;
    /// Partition attributes (GPT).
    pub const ATTR: i32 = 9;
    /// Boot flag.
    pub const BOOT: i32 = 10;
    /// Boot area size (BSD).
    pub const BSIZE: i32 = 11;
    /// Cylinders per group (BSD).
    pub const CPG: i32 = 12;
    /// End C/H/S (MBR).
    pub const EADDR: i32 = 13;
    /// Fragment size (BSD).
    pub const FSIZE: i32 = 14;
    /// Partition label or name.
    pub const NAME: i32 = 15;
    /// Start C/H/S (MBR).
    pub const SADDR: i32 = 16;
    /// Partition UUID (GPT).
    pub const UUID: i32 = 17;
    /// Filesystem UUID.
    pub const FSUUID: i32 = 18;
    /// Filesystem label.
    pub const FSLABEL: i32 = 19;
    /// Filesystem type.
    pub const FSTYPE: i32 = 20;
    /// `FDISK_NFIELDS`: the count.
    pub const NFIELDS: i32 = 21;
}

/// `FDISK_LABELITEM_ID`: the disk identifier, for every label.
pub const LABELITEM_ID: i32 = 0;
/// `__FDISK_NLABELITEMS`: generic items are 0..8, label-specific ones after.
pub const NLABELITEMS: i32 = 8;

/// `FDISK_LABEL_FL_REQUIRE_GEOMETRY`: the label needs C/H/S.
pub const LABEL_FL_REQUIRE_GEOMETRY: i32 = 1 << 2;
/// `FDISK_LABEL_FL_INCHARS_PARTNO`: partitions are `a`, `b`, ... (BSD).
pub const LABEL_FL_INCHARS_PARTNO: i32 = 1 << 3;

/// `FDISK_MOVE_NONE`: a script's start is not relative.
pub const MOVE_NONE: i32 = 0;
/// `FDISK_MOVE_DOWN`: `-N`, or `-` alone, moves the start down.
pub const MOVE_DOWN: i32 = -1;
/// `FDISK_MOVE_UP`: `+N` moves the start up.
pub const MOVE_UP: i32 = 1;

/// `FDISK_RESIZE_NONE`: a script's size is not relative.
pub const RESIZE_NONE: i32 = 0;
/// `FDISK_RESIZE_REDUCE`: `-N` shrinks.
pub const RESIZE_REDUCE: i32 = -1;
/// `FDISK_RESIZE_ENLARGE`: `+N`, or `+` alone, grows.
pub const RESIZE_ENLARGE: i32 = 1;

/// `FDISK_DIFF_*`: how a partition changed between two tables.
pub mod diff {
    /// The same.
    pub const UNCHANGED: i32 = 0;
    /// Gone.
    pub const REMOVED: i32 = 1;
    /// New.
    pub const ADDED: i32 = 2;
    /// Another start.
    pub const MOVED: i32 = 3;
    /// Another size.
    pub const RESIZED: i32 = 4;
}

/// `DOS_FLAG_ACTIVE`: the MBR boot flag, for
/// [`Context::toggle_partition_flag`].
pub const DOS_FLAG_ACTIVE: u64 = 1;
/// `SGI_FLAG_BOOT`.
pub const SGI_FLAG_BOOT: u64 = 1;
/// `SGI_FLAG_SWAP`.
pub const SGI_FLAG_SWAP: u64 = 2;
/// `GPT_FLAG_REQUIRED`: a system partition, to be preserved as is.
pub const GPT_FLAG_REQUIRED: u64 = 1;
/// `GPT_FLAG_NOBLOCK`: firmware should not read the partition.
pub const GPT_FLAG_NOBLOCK: u64 = 2;
/// `GPT_FLAG_LEGACYBOOT`: boot it the legacy way.
pub const GPT_FLAG_LEGACYBOOT: u64 = 3;
/// `GPT_FLAG_GUIDSPECIFIC`: ask which of bits 48..63 to toggle.
pub const GPT_FLAG_GUIDSPECIFIC: u64 = 4;

#[cfg(test)]
mod tests;
