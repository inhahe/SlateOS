//! ZFS System Attributes: the self-describing replacement for `znode_phys_t`.
//!
//! Up to ZPL version 4, a file's metadata was a fixed C struct in the dnode's
//! bonus buffer — mode at byte 72, size at byte 80, and so on. From version 5
//! the bonus holds a *System Attribute* set instead: a small header naming a
//! **layout**, followed by the attribute values packed end to end with no
//! padding and no field names. The layout says which attributes are present
//! and in what order; a second table says how wide each attribute is. Both
//! tables live on disk, in ZAPs reachable from the filesystem's master node.
//!
//! # Why this reads the tables instead of hardcoding the offsets
//!
//! Every bootloader that reads ZFS — FreeBSD's `zfsimpl.c`, GRUB's — skips
//! the tables and hardcodes the offsets of the default ZPL layout: mode at 0,
//! size at 8, gen at 16, uid at 24, gid at 32, parent at 40. That is correct
//! for a file created by a stock `zfs create` and wrong for anything else. A
//! file with an ACL, an xattr, or a project id gets a different layout, and
//! the hardcoded reader then reports another attribute's bytes as the size.
//! It is a silent wrong answer, not an error — exactly the failure mode this
//! project's rules forbid — and it is only tolerable in a bootloader because
//! a bootloader reads a handful of files it also created.
//!
//! Reading the registry costs one ZAP walk at mount. After that a layout
//! lookup is one more ZAP lookup per distinct layout, and layouts are shared
//! across every file in the dataset.
//!
//! # Layout of the bonus buffer
//!
//! ```text
//! sa_hdr_phys_t:
//!   0  u32  sa_magic       0x2f505a
//!   4  u16  sa_layout_info  bits 0..9 = header size >> 3, bits 10.. = layout
//!   6  u16  sa_lengths[]    one entry per variable-length attribute
//!  ...  padding to sa_hdr_size
//! then the attribute values, in layout order.
//! ```
//!
//! A short-circuit the format allows: when the layout has no variable-length
//! attributes the header is exactly 8 bytes and `sa_lengths` is empty.
//!
//! # Byte order
//!
//! Attribute values are stored in the pool's native order — unlike ZAP array
//! integers, which are always big-endian. This driver is little-endian only,
//! so the `bswap` column of the registry is read and checked but never acted
//! on: a pool needing a swap was already rejected at the label.

use alloc::vec::Vec;

use crate::error::{KernelError, KernelResult};

use super::dmu::{Dnode, Reader};
use super::raw::{bf64_get, read_u16, read_u32, read_u64};
use super::zap;

/// `SA_MAGIC`, at the head of every System Attribute bonus buffer.
pub const SA_MAGIC: u32 = 0x002f_505a;

/// Key in the filesystem's master node ZAP naming the SA master node object.
pub const SA_ATTRS_KEY: &[u8] = b"SA_ATTRS";
/// Key in the SA master node ZAP naming the attribute registry ZAP.
pub const SA_REGISTRY_KEY: &[u8] = b"REGISTRY";
/// Key in the SA master node ZAP naming the layout table ZAP.
pub const SA_LAYOUTS_KEY: &[u8] = b"LAYOUTS";

/// An SA set larger than this is refused rather than assembled. The bonus
/// buffer plus one spill block is the whole of an attribute set; a spill
/// block is at most `SPA_MAXBLOCKSIZE`, so this is generous by a wide margin
/// and exists only to bound a corrupt `sa_lengths` entry.
const MAX_SA_BYTES: usize = 1 << 21;

/// Attributes cannot number more than this in one layout. The registry is a
/// small fixed set (22 ZPL attributes as of ZPL 5) plus room to grow.
const MAX_LAYOUT_ATTRS: usize = 256;

// ---------------------------------------------------------------------------
// The registry
// ---------------------------------------------------------------------------

/// One row of the on-disk attribute registry.
#[derive(Debug, Clone)]
pub struct SaAttr {
    /// The attribute's name, e.g. `ZPL_SIZE`.
    pub name: Vec<u8>,
    /// Its number, which is what a layout lists.
    pub num: u16,
    /// Its width in bytes, or 0 for a variable-length attribute whose length
    /// is carried per-object in `sa_lengths`.
    pub length: u16,
    /// Its byteswap class. Non-zero only matters on a foreign-endian pool,
    /// which this driver does not mount.
    pub bswap: u8,
}

/// The decoded attribute registry, plus enough to reach the layout table.
#[derive(Debug, Clone)]
pub struct SaRegistry {
    attrs: Vec<SaAttr>,
}

impl SaRegistry {
    /// Decode the registry ZAP.
    ///
    /// # Errors
    ///
    /// Propagates the ZAP walk. An entry whose encoded number exceeds `u16`
    /// is skipped rather than failing the mount: an unknown attribute this
    /// driver never asks for should not stop it reading the ones it does.
    pub fn read(reader: &Reader<'_>, registry_dn: &Dnode) -> KernelResult<Self> {
        let raw = zap::entries(reader, registry_dn)?;
        let mut attrs = Vec::with_capacity(raw.len());
        for ent in raw {
            // ATTR_BSWAP bits 0..7, ATTR_LENGTH bits 8..23, ATTR_NUM bits 24..39.
            let bswap = bf64_get(ent.value, 0, 8);
            let length = bf64_get(ent.value, 8, 16);
            let num = bf64_get(ent.value, 24, 16);
            let (Ok(num), Ok(length), Ok(bswap)) = (
                u16::try_from(num),
                u16::try_from(length),
                u8::try_from(bswap),
            ) else {
                continue;
            };
            attrs.push(SaAttr {
                name: ent.name,
                num,
                length,
                bswap,
            });
        }
        Ok(Self { attrs })
    }

    /// Build a registry directly from a list of attributes.
    ///
    /// Only [`super::tests`] uses this: the layout-resolution checks need a
    /// registry with known contents but have no [`Reader`] to read a ZAP
    /// through, and the alternative — exposing `attrs` — would let any caller
    /// mutate a registry after it had been used to resolve a layout.
    #[must_use]
    pub(super) const fn from_attrs(attrs: Vec<SaAttr>) -> Self {
        Self { attrs }
    }

    /// The number registered for `name`.
    #[must_use]
    pub fn num_of(&self, name: &[u8]) -> Option<u16> {
        self.attrs.iter().find(|a| a.name == name).map(|a| a.num)
    }

    /// The declared width of attribute `num`; 0 means variable-length.
    #[must_use]
    pub fn length_of(&self, num: u16) -> Option<u16> {
        self.attrs.iter().find(|a| a.num == num).map(|a| a.length)
    }

    /// Whether the registry is empty, which means the SA master node was
    /// unreadable and no attribute can be resolved.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.attrs.is_empty()
    }
}

/// Read layout `num` from the layout ZAP.
///
/// Layout keys are the layout number in decimal, and the value is an array of
/// `u16` attribute numbers — the one place in this driver that needs a ZAP
/// value that is not a single object number.
///
/// # Errors
///
/// - [`KernelError::NotFound`] if the layout is not registered, which means
///   the object's SA header names a layout the dataset does not define.
/// - [`KernelError::CorruptedData`] for a value that is not a `u16` array, or
///   one longer than [`MAX_LAYOUT_ATTRS`].
pub fn read_layout(reader: &Reader<'_>, layouts_dn: &Dnode, num: u16) -> KernelResult<Vec<u16>> {
    let key = decimal_key(u64::from(num));
    let arr = zap::lookup_array(reader, layouts_dn, &key)?;
    if arr.intlen != 2 {
        return Err(KernelError::CorruptedData);
    }
    if usize::from(arr.numints) > MAX_LAYOUT_ATTRS {
        return Err(KernelError::CorruptedData);
    }
    Ok(arr.as_u16s())
}

/// Format `v` as its decimal digits, which is how layout numbers are keyed.
pub(super) fn decimal_key(v: u64) -> Vec<u8> {
    if v == 0 {
        return alloc::vec![b'0'];
    }
    let mut digits = Vec::new();
    let mut n = v;
    while n > 0 {
        // `n % 10` is 0..=9, so the cast is exact.
        let d = (n % 10) as u8;
        digits.push(b'0'.wrapping_add(d));
        n /= 10;
    }
    digits.reverse();
    digits
}

// ---------------------------------------------------------------------------
// The header and the value map
// ---------------------------------------------------------------------------

/// The fixed part of `sa_hdr_phys_t`.
#[derive(Debug, Clone, Copy)]
pub struct SaHeader {
    /// Bytes of header before the first attribute value.
    pub hdrsize: usize,
    /// The layout this object uses.
    pub layout_num: u16,
}

/// Parse the SA header at the head of a bonus buffer.
///
/// # Errors
///
/// - [`KernelError::InvalidArgument`] if the magic is wrong — the caller
///   should then treat the bonus as a legacy `znode_phys_t`.
/// - [`KernelError::CorruptedData`] if the declared header size does not fit.
pub fn parse_header(buf: &[u8]) -> KernelResult<SaHeader> {
    if read_u32(buf, 0)? != SA_MAGIC {
        return Err(KernelError::InvalidArgument);
    }
    let info = read_u16(buf, 4)?;
    // SA_HDR_SIZE: bits 0..9, scaled by 8. SA_HDR_LAYOUT_NUM: the rest.
    let hdrsize = usize::from(info & 0x03ff).wrapping_mul(8);
    let layout_num = info >> 10;
    if hdrsize < 8 || hdrsize > buf.len() {
        return Err(KernelError::CorruptedData);
    }
    Ok(SaHeader {
        hdrsize,
        layout_num,
    })
}

/// Where each attribute of one object's SA set lives within its value bytes.
#[derive(Debug, Clone)]
pub struct SaMap {
    /// `(attribute number, offset, length)`, in layout order.
    entries: Vec<(u16, usize, usize)>,
}

impl SaMap {
    /// Resolve a layout against a header into per-attribute offsets.
    ///
    /// Fixed-width attributes take their width from the registry; the widths
    /// of the variable-length ones are read from `sa_lengths[]` in the order
    /// they appear in the layout. That ordering is the whole contract between
    /// the two tables, and getting it wrong shifts every later attribute.
    ///
    /// # Errors
    ///
    /// - [`KernelError::CorruptedData`] if a layout names an attribute the
    ///   registry does not define, if `sa_lengths` runs out, or if the total
    ///   exceeds [`MAX_SA_BYTES`].
    pub fn resolve(
        hdr: &SaHeader,
        header_bytes: &[u8],
        layout: &[u16],
        reg: &SaRegistry,
    ) -> KernelResult<Self> {
        let mut entries = Vec::with_capacity(layout.len());
        let mut off = 0usize;
        let mut var_idx = 0usize;
        for &num in layout {
            let declared = reg.length_of(num).ok_or(KernelError::CorruptedData)?;
            let len = if declared == 0 {
                // sa_lengths[] starts at byte 6 and must stay inside the header.
                let at = 6usize
                    .checked_add(var_idx.checked_mul(2).ok_or(KernelError::CorruptedData)?)
                    .ok_or(KernelError::CorruptedData)?;
                if at.checked_add(2).ok_or(KernelError::CorruptedData)? > hdr.hdrsize {
                    return Err(KernelError::CorruptedData);
                }
                var_idx = var_idx.wrapping_add(1);
                usize::from(read_u16(header_bytes, at)?)
            } else {
                usize::from(declared)
            };
            let end = off.checked_add(len).ok_or(KernelError::CorruptedData)?;
            if end > MAX_SA_BYTES {
                return Err(KernelError::CorruptedData);
            }
            entries.push((num, off, len));
            off = end;
        }
        Ok(Self { entries })
    }

    /// The raw bytes of attribute `num` within `values`.
    #[must_use]
    pub fn get<'a>(&self, values: &'a [u8], num: u16) -> Option<&'a [u8]> {
        let &(_, off, len) = self.entries.iter().find(|e| e.0 == num)?;
        values.get(off..off.checked_add(len)?)
    }

    /// Attribute `num` as a little-endian unsigned integer of its own width.
    ///
    /// Widths of 1, 2, 4 and 8 are accepted; anything else is not a scalar and
    /// returns `None` rather than a truncation.
    #[must_use]
    pub fn get_uint(&self, values: &[u8], num: u16) -> Option<u64> {
        let bytes = self.get(values, num)?;
        match bytes.len() {
            1 => bytes.first().map(|&b| u64::from(b)),
            2 => read_u16(bytes, 0).ok().map(u64::from),
            4 => read_u32(bytes, 0).ok().map(u64::from),
            8 => read_u64(bytes, 0).ok(),
            _ => None,
        }
    }

    /// Attribute `num` read as the first `u64` of a timestamp pair.
    ///
    /// A ZPL time attribute is two 64-bit values, seconds then nanoseconds.
    #[must_use]
    pub fn get_time_secs(&self, values: &[u8], num: u16) -> Option<u64> {
        let bytes = self.get(values, num)?;
        if bytes.len() < 8 {
            return None;
        }
        read_u64(bytes, 0).ok()
    }

    /// Whether any attribute was resolved.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// One SA buffer: its values with the header stripped, and the map into them.
#[derive(Debug, Clone)]
pub struct SaPart {
    /// The attribute values, in layout order.
    pub values: Vec<u8>,
    /// Where each one starts.
    pub map: SaMap,
}

impl SaPart {
    /// Decode one SA buffer — a bonus buffer or a spill block.
    fn parse(
        reader: &Reader<'_>,
        buf: &[u8],
        layouts_dn: &Dnode,
        reg: &SaRegistry,
    ) -> KernelResult<Self> {
        let hdr = parse_header(buf)?;
        let layout = read_layout(reader, layouts_dn, hdr.layout_num)?;
        let map = SaMap::resolve(&hdr, buf, &layout, reg)?;
        let values = buf
            .get(hdr.hdrsize..)
            .ok_or(KernelError::CorruptedData)?
            .to_vec();
        if values.len() > MAX_SA_BYTES {
            return Err(KernelError::CorruptedData);
        }
        Ok(Self { values, map })
    }
}

/// One object's System Attributes, across however many buffers hold them.
///
/// The bonus buffer and the spill block are *independent* attribute sets:
/// each has its own `sa_hdr_phys_t` naming its own layout, and an attribute
/// lives in exactly one of them. Treating the spill as a continuation of the
/// bonus — concatenating their bytes and resolving one layout across the
/// join — reads every spilled attribute at the wrong offset, which is why
/// they are kept as separate parts here and searched in turn.
#[derive(Debug, Clone, Default)]
pub struct SaObject {
    parts: Vec<SaPart>,
}

impl SaObject {
    /// Assemble an object's attributes from its dnode.
    ///
    /// A spilled attribute set is not an edge case: any file carrying an ACL
    /// has one, so a reader that ignores the spill block gets wrong answers
    /// on ordinary files.
    ///
    /// # Errors
    ///
    /// - [`KernelError::InvalidArgument`] if neither buffer holds an SA header
    ///   — the object is a legacy `znode_phys_t` and the caller should read it
    ///   as one.
    /// - Otherwise as [`SaMap::resolve`] and the layout lookup.
    pub fn read(
        reader: &Reader<'_>,
        dn: &Dnode,
        layouts_dn: &Dnode,
        reg: &SaRegistry,
    ) -> KernelResult<Self> {
        let mut parts = Vec::with_capacity(2);
        // A zero-length bonus is normal when the whole set spilled, so the
        // bonus failing to parse is only fatal if the spill fails too.
        if let Ok(part) = SaPart::parse(reader, &dn.bonus, layouts_dn, reg) {
            parts.push(part);
        }
        if let Some(bp) = dn.spill.as_ref() {
            let spill = reader.read_block(bp)?;
            parts.push(SaPart::parse(reader, &spill, layouts_dn, reg)?);
        }
        if parts.is_empty() {
            return Err(KernelError::InvalidArgument);
        }
        Ok(Self { parts })
    }

    /// Attribute `num` as an unsigned integer.
    #[must_use]
    pub fn uint(&self, num: u16) -> Option<u64> {
        self.parts
            .iter()
            .find_map(|p| p.map.get_uint(&p.values, num))
    }

    /// Attribute `num`'s raw bytes.
    #[must_use]
    pub fn bytes(&self, num: u16) -> Option<&[u8]> {
        self.parts.iter().find_map(|p| p.map.get(&p.values, num))
    }

    /// Attribute `num` as the seconds field of a timestamp.
    #[must_use]
    pub fn time_secs(&self, num: u16) -> Option<u64> {
        self.parts
            .iter()
            .find_map(|p| p.map.get_time_secs(&p.values, num))
    }
}

// ---------------------------------------------------------------------------
// Legacy znode_phys_t
// ---------------------------------------------------------------------------

/// Field offsets in a `znode_phys_t`, for datasets at ZPL version 4 or below.
///
/// Kept as named constants rather than a struct because the layout is fixed
/// and reading it field-by-field avoids any reinterpretation of raw bytes.
pub mod znode {
    /// `zp_atime[2]`.
    pub const ATIME: usize = 0;
    /// `zp_mtime[2]`.
    pub const MTIME: usize = 16;
    /// `zp_ctime[2]`.
    pub const CTIME: usize = 32;
    /// `zp_crtime[2]`.
    pub const CRTIME: usize = 48;
    /// `zp_gen`.
    pub const GEN: usize = 64;
    /// `zp_mode`.
    pub const MODE: usize = 72;
    /// `zp_size`.
    pub const SIZE: usize = 80;
    /// `zp_parent`.
    pub const PARENT: usize = 88;
    /// `zp_links`.
    pub const LINKS: usize = 96;
    /// `zp_xattr`.
    pub const XATTR: usize = 104;
    /// `zp_rdev`.
    pub const RDEV: usize = 112;
    /// `zp_flags`.
    pub const FLAGS: usize = 120;
    /// `zp_uid`.
    pub const UID: usize = 128;
    /// `zp_gid`.
    pub const GID: usize = 136;
}
