//! util-linux 2.39.3's libblkid: the low-level prober that reads a device
//! and says what is on it -- which filesystem or RAID member, with what
//! label and UUID; which partition table, with which partitions; and the
//! device's I/O topology.
//!
//! # How it is organised (as upstream is)
//!
//! A [`Probe`] is bound to a device or file (and optionally a window of it,
//! an offset and a size). Three *chains* run against it, in order:
//!
//! * **superblocks** (on by default): about eighty probers, from LUKS and
//!   the RAID formats through every filesystem libblkid knows, each matched
//!   first by its magic bytes and then confirmed by its own function;
//! * **topology** (off by default): the I/O limits of a block device;
//! * **partitions** (off by default): DOS, GPT, Mac, BSD and the rest,
//!   nested tables (a BSD label inside a DOS partition) included.
//!
//! Results are `NAME=value` pairs ([`Value`]) -- `TYPE`, `LABEL`, `UUID`,
//! `PTTYPE`, `PART_ENTRY_NAME` -- and, for partitions and topology, a
//! binary interface too ([`Probe::partitions`], [`Probe::topology`]).
//! [`Probe::do_safeprobe`] runs each enabled chain once and refuses an
//! ambiguous answer (two filesystems that cannot coexist);
//! [`Probe::do_probe`] steps through every match one at a time, which is how
//! `wipefs` finds every signature on a device; [`Probe::do_fullprobe`]
//! gathers everything without the ambiguity check.
//!
//! Above the probe sit libblkid's other two layers: the device cache
//! ([`cache`], `/run/blkid/blkid.tab` -- the devices seen and the tags each
//! carries, re-probed when stale) and tag evaluation ([`evaluate`] --
//! `LABEL=root` to a device, through udev's links and then the cache).
//!
//! # Fidelity
//!
//! The port keeps upstream's behaviour where it shows: the order probers run
//! in, which of them are tolerant of company, what each reads and in what
//! size (a device too short for a read is a different answer from a read
//! error), the exact bytes of every value, `errno` as upstream leaves it.
//! Upstream's global `errno` is [`Probe::errno`], set where libblkid sets it
//! and read where libblkid reads it. `scripts/blkid-diff.sh` compares every
//! value, byte for byte, with the system's libblkid on the util-linux test
//! corpus and on images made for the purpose.

/// `lib/blkdev.c` -- util-linux's shared block-device helpers, which its
/// programs use as well as libblkid: public so a port (`blockdev`) calls the
/// one copy rather than carrying its own request numbers.
pub mod blkdev;
pub mod cache;
pub mod devno;
pub mod encode;
pub mod evaluate;
pub mod partitions;
mod probe;
pub mod superblocks;
pub mod topology;

#[cfg(test)]
mod tests;

pub use blkdev::{get_sector_size as blkdev_get_sector_size, get_size as blkdev_get_size};
pub use probe::{Buf, ChainId, IdInfo, IdMag, Probe, Value, open_nonblock, parse_tag_string};

/// `BLKID_PROBE_OK`.
pub const PROBE_OK: i32 = 0;
/// `BLKID_PROBE_NONE`: nothing found.
pub const PROBE_NONE: i32 = 1;
/// `BLKID_PROBE_ERROR`.
pub const PROBE_ERROR: i32 = -1;
/// `BLKID_PROBE_AMBIGUOUS`: more than one thing found where one was
/// expected.
pub const PROBE_AMBIGUOUS: i32 = -2;

/// `BLKID_SUBLKS_LABEL`: read LABEL.
pub const SUBLKS_LABEL: u32 = 1 << 1;
/// `BLKID_SUBLKS_LABELRAW`: define LABEL_RAW.
pub const SUBLKS_LABELRAW: u32 = 1 << 2;
/// `BLKID_SUBLKS_UUID`: read UUID.
pub const SUBLKS_UUID: u32 = 1 << 3;
/// `BLKID_SUBLKS_UUIDRAW`: define UUID_RAW.
pub const SUBLKS_UUIDRAW: u32 = 1 << 4;
/// `BLKID_SUBLKS_TYPE`: define TYPE.
pub const SUBLKS_TYPE: u32 = 1 << 5;
/// `BLKID_SUBLKS_SECTYPE`: define SEC_TYPE.
pub const SUBLKS_SECTYPE: u32 = 1 << 6;
/// `BLKID_SUBLKS_USAGE`: define USAGE.
pub const SUBLKS_USAGE: u32 = 1 << 7;
/// `BLKID_SUBLKS_VERSION`: define VERSION.
pub const SUBLKS_VERSION: u32 = 1 << 8;
/// `BLKID_SUBLKS_MAGIC`: define SBMAGIC and SBMAGIC_OFFSET.
pub const SUBLKS_MAGIC: u32 = 1 << 9;
/// `BLKID_SUBLKS_BADCSUM`: accept a bad checksum, and say so (SBBADCSUM).
pub const SUBLKS_BADCSUM: u32 = 1 << 10;
/// `BLKID_SUBLKS_FSINFO`: define FSSIZE, FSLASTBLOCK, FSBLOCKSIZE,
/// ENDIANNESS.
pub const SUBLKS_FSINFO: u32 = 1 << 11;
/// `BLKID_SUBLKS_DEFAULT`.
pub const SUBLKS_DEFAULT: u32 = SUBLKS_LABEL | SUBLKS_UUID | SUBLKS_TYPE | SUBLKS_SECTYPE;

/// `BLKID_PARTS_FORCE_GPT`: GPT even without a protective MBR.
pub const PARTS_FORCE_GPT: u32 = 1 << 1;
/// `BLKID_PARTS_ENTRY_DETAILS`: define PART_ENTRY_* for a partition.
pub const PARTS_ENTRY_DETAILS: u32 = 1 << 2;
/// `BLKID_PARTS_MAGIC`: define PTMAGIC and PTMAGIC_OFFSET.
pub const PARTS_MAGIC: u32 = 1 << 3;

/// `BLKID_FLTR_NOTIN`: probe for everything not named.
pub const FLTR_NOTIN: u32 = 1;
/// `BLKID_FLTR_ONLYIN`: probe only for what is named.
pub const FLTR_ONLYIN: u32 = 2;

/// `BLKID_USAGE_FILESYSTEM`.
pub const USAGE_FILESYSTEM: u32 = 1 << 1;
/// `BLKID_USAGE_RAID`.
pub const USAGE_RAID: u32 = 1 << 2;
/// `BLKID_USAGE_CRYPTO`.
pub const USAGE_CRYPTO: u32 = 1 << 3;
/// `BLKID_USAGE_OTHER`.
pub const USAGE_OTHER: u32 = 1 << 4;

/// `BLKID_IDINFO_TOLERANT`: may share a device with another signature.
pub const IDINFO_TOLERANT: u32 = 1 << 1;

/// `DEFAULT_SECTOR_SIZE`.
pub const DEFAULT_SECTOR_SIZE: u32 = 512;
/// `UUID_STR_LEN`: 36 characters and the NUL.
pub const UUID_STR_LEN: usize = 37;

/// `S_IFMT`.
pub const S_IFMT: u32 = 0o170_000;
/// `S_IFREG`.
pub const S_IFREG: u32 = 0o100_000;
/// `S_IFBLK`.
pub const S_IFBLK: u32 = 0o060_000;
/// `S_IFCHR`.
pub const S_IFCHR: u32 = 0o020_000;
/// `S_IFDIR`.
pub const S_IFDIR: u32 = 0o040_000;

/// `EIO`.
pub const EIO: i32 = 5;
/// `ENOMEM`.
pub const ENOMEM: i32 = 12;
/// `EINVAL`.
pub const EINVAL: i32 = 22;
/// `ERANGE`.
pub const ERANGE: i32 = 34;

/// `enum BLKID_ENDIANNESS`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Endianness {
    /// `BLKID_ENDIANNESS_LITTLE`.
    Little,
    /// `BLKID_ENDIANNESS_BIG`.
    Big,
}

/// The `errno` of an I/O error.
#[must_use]
pub fn errno_of(e: &std::io::Error) -> i32 {
    e.raw_os_error().unwrap_or(EIO)
}

/// A C string: `b` up to its first NUL.
#[must_use]
pub fn c_str(b: &[u8]) -> &[u8] {
    b.iter()
        .position(|&c| c == 0)
        .map_or(b, |n| b.get(..n).unwrap_or_default())
}

/// `blkid_unparse_uuid`: a DCE UUID's sixteen bytes as the 36-character
/// lowercase string.
#[must_use]
pub fn unparse_uuid(uuid: &[u8]) -> Vec<u8> {
    let b = |k: usize| uuid.get(k).copied().unwrap_or(0);
    let mut s = String::with_capacity(36);
    for k in 0..16 {
        if matches!(k, 4 | 6 | 8 | 10) {
            s.push('-');
        }
        s.push_str(&format!("{:02x}", b(k)));
    }
    s.into_bytes()
}

/// `memcpy(dst, src, sizeof dst)` from a slice that may be short: bytes it
/// lacks are left as they were. Every caller copies out of a buffer it read
/// at a size that covers the field, so nothing is ever left; the check is
/// here so that a miscounted offset cannot become a panic.
pub(crate) fn copy_into(dst: &mut [u8], src: &[u8]) {
    for (d, &s) in dst.iter_mut().zip(src) {
        *d = s;
    }
}

/// `blkid_uuid_is_empty(buf, len)`: all zeros.
#[must_use]
pub fn uuid_is_empty(buf: &[u8]) -> bool {
    buf.iter().all(|&b| b == 0)
}

/// Little- and big-endian readers over a byte slice. A read past the end
/// yields zeros: every caller reads inside a buffer it asked for by size,
/// so it never happens, and zeros are what the C struct would not have
/// had either.
pub trait Bytes {
    /// A byte.
    fn u8_at(&self, off: usize) -> u8;
    /// `le16_to_cpu`.
    fn le16(&self, off: usize) -> u16;
    /// `be16_to_cpu`.
    fn be16(&self, off: usize) -> u16;
    /// `le32_to_cpu`.
    fn le32(&self, off: usize) -> u32;
    /// `be32_to_cpu`.
    fn be32(&self, off: usize) -> u32;
    /// `le64_to_cpu`.
    fn le64(&self, off: usize) -> u64;
    /// `be64_to_cpu`.
    fn be64(&self, off: usize) -> u64;
    /// `len` bytes at `off`, as far as they go.
    fn span(&self, off: usize, len: usize) -> &[u8];
}

impl Bytes for [u8] {
    fn u8_at(&self, off: usize) -> u8 {
        self.get(off).copied().unwrap_or(0)
    }
    fn le16(&self, off: usize) -> u16 {
        u16::from_le_bytes([self.u8_at(off), self.u8_at(off.saturating_add(1))])
    }
    fn be16(&self, off: usize) -> u16 {
        u16::from_be_bytes([self.u8_at(off), self.u8_at(off.saturating_add(1))])
    }
    fn le32(&self, off: usize) -> u32 {
        let mut w = [0u8; 4];
        for (k, b) in w.iter_mut().enumerate() {
            *b = self.u8_at(off.saturating_add(k));
        }
        u32::from_le_bytes(w)
    }
    fn be32(&self, off: usize) -> u32 {
        let mut w = [0u8; 4];
        for (k, b) in w.iter_mut().enumerate() {
            *b = self.u8_at(off.saturating_add(k));
        }
        u32::from_be_bytes(w)
    }
    fn le64(&self, off: usize) -> u64 {
        let mut w = [0u8; 8];
        for (k, b) in w.iter_mut().enumerate() {
            *b = self.u8_at(off.saturating_add(k));
        }
        u64::from_le_bytes(w)
    }
    fn be64(&self, off: usize) -> u64 {
        let mut w = [0u8; 8];
        for (k, b) in w.iter_mut().enumerate() {
            *b = self.u8_at(off.saturating_add(k));
        }
        u64::from_be_bytes(w)
    }
    fn span(&self, off: usize, len: usize) -> &[u8] {
        let start = off.min(self.len());
        let end = off.saturating_add(len).min(self.len());
        self.get(start..end).unwrap_or_default()
    }
}
