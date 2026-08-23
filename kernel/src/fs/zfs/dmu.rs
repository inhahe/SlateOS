//! The Data Management Unit: turning a block pointer into bytes, and an
//! object plus a block id into bytes.
//!
//! Two layers live here and it is worth keeping them apart.
//!
//! [`Reader::read_block`] resolves *one* block pointer: pick a copy, read
//! `psize` bytes off the device, prove the checksum, decompress to `lsize`.
//! Everything above it in the driver — object sets, ZAPs, files — is
//! ultimately a sequence of calls to it.
//!
//! [`Reader::read_object_block`] resolves *one block of an object*, which
//! means walking the dnode's indirect tree. A dnode holds up to three block
//! pointers inline; when an object outgrows them, ZFS inserts levels of
//! indirect blocks, each a dense array of 128-byte block pointers. The number
//! of levels is stored, not derived, so a reader never has to guess.
//!
//! # Holes are answers
//!
//! A zero block pointer is a hole, and a hole reads as zeroes. This is not an
//! error path bolted on for sparse files — the object *set* dnode of a
//! freshly created filesystem is mostly holes, and treating one as corruption
//! would fail every mount. Both layers return zeroes for a hole and say so.
//!
//! # Ditto blocks
//!
//! Metadata is written to two or three DVAs. `read_block` tries them in order
//! and only fails when every copy fails, which is the entire value of the
//! redundancy: a bad sector under copy 0 costs a retry, not the pool.
//!
//! # Gang blocks
//!
//! When a pool is too fragmented to find one contiguous run for a block, ZFS
//! writes a *gang block*: the DVA points at a small header holding block
//! pointers to the pieces, and the pieces concatenate — in header order, each
//! contributing its own `psize` — to the bytes the caller asked for. The gang
//! bit in the DVA is what says the offset leads to a header rather than to
//! data. Pieces may themselves be gang blocks, so the header is a tree.
//!
//! Two things about the header are easy to get wrong and worth stating here.
//! It is *self*-checksummed — it carries a `zio_eck_t` trailer verified
//! against its own address rather than against the parent's `blk_cksum` — and
//! the address used is always **DVA 0's**, even when reading copy 1 or 2, so
//! that every ditto copy of a header hashes identically. And the parent's
//! `blk_cksum` is never checked against the assembled bytes: each piece is
//! verified against its own pointer, which is what covers the data.

use alloc::vec;
use alloc::vec::Vec;

use crate::error::{KernelError, KernelResult};
use crate::fs::blocksrc::{SectorSource, read_bytes};

use super::raw::{
    BLKPTR_LEN, BlkPtr, DMU_OT_NONE, DNODE_CORE_LEN, DNODE_LEN, Dva, SPA_MAXBLOCKSIZE,
    ZIO_CHECKSUM_GANG_HEADER, parse_blkptr, read_u8, read_u16, read_u64,
};
use super::zio::{self, ZEC_LEN, ZioCksum};

/// `log2(BLKPTR_LEN)`: how many bits of a block id one level of indirection
/// consumes, before the indirect block's own size is taken into account.
const SPA_BLKPTRSHIFT: u32 = 7;

/// `SPA_OLD_GANGBLOCKSIZE`: the gang header size every pool used before the
/// `dynamic_gang_header` feature, and still the size on any pool without it.
/// Three block pointers, 88 bytes of padding, then the 40-byte trailer.
const SPA_OLD_GANGBLOCKSIZE: usize = 512;

/// Largest gang header this driver will attempt. A gang header occupies one
/// minimum allocation on its vdev — `1 << ashift` — and the label parser caps
/// `ashift` at 16, so nothing larger can be legitimate.
const GANG_HEADER_MAX: usize = 1 << 16;

/// How far a gang tree may nest before the read is refused.
///
/// A cap is not optional: a header whose child pointer addresses the header
/// itself is a cycle that appends no bytes, so nothing else would ever stop
/// the recursion, and a kernel stack is not a place to discover that. Six
/// levels is far past anything real — even the three-pointer old-style header
/// spans 729 pieces at that depth, which for the 16 MiB maximum block means
/// pieces of 22 KiB. A pool fragmented worse than that is failing allocations,
/// not writing deeper gang trees.
const MAX_GANG_DEPTH: u32 = 6;

/// `BPE_PAYLOAD_SIZE`: bytes of payload an embedded block pointer can carry.
const BPE_PAYLOAD_SIZE: usize = 112;

/// `BP_EMBEDDED_TYPE_DATA`: the only embedded type that carries file/metadata
/// bytes. `REDACTED` marks a range removed by `zfs redact` and has no data.
const BP_EMBEDDED_TYPE_DATA: u64 = 0;

/// Byte ranges of an embedded block pointer that hold payload, in order.
/// Words 6 (properties) and 10 (logical birth txg) are the two holes.
const BPE_PAYLOAD_RANGES: [(usize, usize); 3] = [(0, 48), (56, 80), (88, 128)];

/// `DNODE_FLAG_SPILL_BLKPTR`: the dnode's last block pointer's worth of space
/// holds a spill block pointer rather than bonus buffer.
const DNODE_FLAG_SPILL_BLKPTR: u8 = 1 << 2;

/// A parsed `dnode_phys_t`: the metadata for one DMU object.
#[derive(Debug, Clone)]
pub struct Dnode {
    /// `DMU_OT_*` of the object's data.
    pub obj_type: u8,
    /// `log2` of the indirect block size.
    pub indblkshift: u8,
    /// Number of levels in the block tree; 1 means `blkptrs` point at data.
    pub nlevels: u8,
    /// `DMU_OT_*` of the bonus buffer.
    pub bonustype: u8,
    /// Data block size in bytes.
    pub datablksz: u64,
    /// Highest allocated block id. Note that a `maxblkid` of 0 with a hole at
    /// block 0 is an empty object, not a one-block object.
    pub maxblkid: u64,
    /// Bytes of disk space charged to this object.
    pub used: u64,
    /// The inline block pointers (up to three).
    pub blkptrs: Vec<BlkPtr>,
    /// The bonus buffer, which is where a ZPL znode or its System Attributes
    /// live.
    pub bonus: Vec<u8>,
    /// `dn_flags`.
    pub flags: u8,
    /// The spill block pointer, when `dn_flags` says one is present.
    ///
    /// System Attributes overflow here when they no longer fit in the bonus
    /// buffer — which happens as soon as a file carries a non-trivial ACL — so
    /// a reader that ignores this reports wrong sizes and modes on ordinary
    /// files rather than on exotic ones.
    pub spill: Option<BlkPtr>,
}

impl Dnode {
    /// Whether this dnode slot is allocated.
    #[must_use]
    pub const fn is_allocated(&self) -> bool {
        self.obj_type != DMU_OT_NONE
    }
}

/// Parse the `dnode_phys_t` at `off` within `buf`.
///
/// # Errors
///
/// - [`KernelError::InvalidArgument`] if the slot does not fit, or the header
///   describes a geometry that cannot exist (zero data block size, more than
///   three inline block pointers, an indirect block smaller than one pointer).
pub fn parse_dnode(buf: &[u8], off: usize) -> KernelResult<Dnode> {
    let end = off
        .checked_add(DNODE_LEN)
        .ok_or(KernelError::InvalidArgument)?;
    if end > buf.len() {
        return Err(KernelError::InvalidArgument);
    }

    let obj_type = read_u8(buf, off)?;
    let indblkshift = read_u8(buf, off.wrapping_add(1))?;
    let nlevels = read_u8(buf, off.wrapping_add(2))?;
    let nblkptr = read_u8(buf, off.wrapping_add(3))?;
    let bonustype = read_u8(buf, off.wrapping_add(4))?;
    let datablkszsec = read_u16(buf, off.wrapping_add(8))?;
    let bonuslen = read_u16(buf, off.wrapping_add(10))?;
    let extra_slots = read_u8(buf, off.wrapping_add(12))?;
    let maxblkid = read_u64(buf, off.wrapping_add(16))?;
    let used = read_u64(buf, off.wrapping_add(24))?;

    if obj_type == DMU_OT_NONE {
        // An unallocated slot's remaining fields are meaningless; validating
        // them would reject perfectly normal holes in a dnode array.
        return Ok(Dnode {
            obj_type,
            indblkshift,
            nlevels,
            bonustype,
            datablksz: 0,
            maxblkid: 0,
            used: 0,
            blkptrs: Vec::new(),
            bonus: Vec::new(),
            flags: 0,
            spill: None,
        });
    }

    if nblkptr == 0 || usize::from(nblkptr) > 3 {
        return Err(KernelError::InvalidArgument);
    }
    if nlevels == 0 {
        return Err(KernelError::InvalidArgument);
    }
    if datablkszsec == 0 {
        return Err(KernelError::InvalidArgument);
    }
    if nlevels > 1 && u32::from(indblkshift) < SPA_BLKPTRSHIFT {
        // An indirect block smaller than one block pointer would make the
        // fan-out zero and the descent below never terminate.
        return Err(KernelError::InvalidArgument);
    }

    let datablksz = u64::from(datablkszsec).wrapping_mul(512);
    if datablksz > SPA_MAXBLOCKSIZE {
        return Err(KernelError::InvalidArgument);
    }

    let mut blkptrs = Vec::with_capacity(usize::from(nblkptr));
    for i in 0..usize::from(nblkptr) {
        let bp_off = off
            .checked_add(DNODE_CORE_LEN)
            .and_then(|b| i.checked_mul(BLKPTR_LEN).and_then(|d| b.checked_add(d)))
            .ok_or(KernelError::InvalidArgument)?;
        blkptrs.push(parse_blkptr(buf, bp_off)?);
    }

    // The bonus buffer starts after the inline block pointers and runs to the
    // end of the dnode's slots. `dn_extra_slots` widens the dnode for a long
    // bonus (large System Attribute sets), so the available room depends on
    // it and a `bonuslen` claiming more than that is corrupt.
    let bonus_start = off
        .checked_add(DNODE_CORE_LEN)
        .and_then(|b| {
            usize::from(nblkptr)
                .checked_mul(BLKPTR_LEN)
                .and_then(|d| b.checked_add(d))
        })
        .ok_or(KernelError::InvalidArgument)?;
    let total_len = usize::from(extra_slots)
        .checked_add(1)
        .and_then(|n| n.checked_mul(DNODE_LEN))
        .ok_or(KernelError::InvalidArgument)?;
    let slot_end = off
        .checked_add(total_len)
        .ok_or(KernelError::InvalidArgument)?;
    let room = slot_end.saturating_sub(bonus_start);
    let want = usize::from(bonuslen).min(room);
    let bonus = buf
        .get(bonus_start..bonus_start.saturating_add(want))
        .unwrap_or(&[])
        .to_vec();

    // `DN_SPILL_BLKPTR`: the last block pointer's worth of the dnode's slots,
    // which is why it is found from the end rather than from `nblkptr`.
    let flags = read_u8(buf, off.wrapping_add(7))?;
    let spill = if flags & DNODE_FLAG_SPILL_BLKPTR == 0 {
        None
    } else {
        let sp_off = slot_end
            .checked_sub(BLKPTR_LEN)
            .ok_or(KernelError::InvalidArgument)?;
        let bp = parse_blkptr(buf, sp_off)?;
        if bp.is_hole() { None } else { Some(bp) }
    };

    Ok(Dnode {
        obj_type,
        indblkshift,
        nlevels,
        bonustype,
        datablksz,
        maxblkid,
        used,
        blkptrs,
        bonus,
        flags,
        spill,
    })
}

/// Reads blocks out of a single-vdev pool.
pub struct Reader<'a> {
    src: &'a dyn SectorSource,
}

impl<'a> Reader<'a> {
    /// Wrap a sector source.
    #[must_use]
    pub const fn new(src: &'a dyn SectorSource) -> Self {
        Self { src }
    }

    /// Resolve one block pointer to `lsize` logical bytes.
    ///
    /// # Errors
    ///
    /// - [`KernelError::NotSupported`] for an encrypted block, or an
    ///   unimplemented checksum/compression algorithm.
    /// - [`KernelError::InvalidArgument`] for a block larger than
    ///   [`SPA_MAXBLOCKSIZE`], or a gang tree whose pieces do not add up to
    ///   the block.
    /// - [`KernelError::NotSupported`] for a DVA on a vdev we do not have, or
    ///   a gang tree nested past [`MAX_GANG_DEPTH`].
    /// - [`KernelError::IoError`] if every copy failed to read or verify.
    pub fn read_block(&self, bp: &BlkPtr) -> KernelResult<Vec<u8>> {
        if bp.embedded {
            return read_embedded(bp);
        }
        if bp.is_hole() {
            let lsize = usize::try_from(bp.lsize).map_err(|_| KernelError::InvalidArgument)?;
            return Ok(vec![0u8; lsize]);
        }
        if bp.encrypted {
            // A read that "succeeded" against ciphertext would return
            // plausible garbage; the caller must be told the key is missing.
            return Err(KernelError::NotSupported);
        }
        // The byte order bit is only meaningful on a non-hole pointer, which
        // is why it is checked here and not in the parser.
        if !bp.little_endian {
            return Err(KernelError::NotSupported);
        }
        if bp.lsize == 0 || bp.lsize > SPA_MAXBLOCKSIZE || bp.psize > SPA_MAXBLOCKSIZE {
            return Err(KernelError::InvalidArgument);
        }
        if !zio::compression_supported(bp.compress) {
            return Err(KernelError::NotSupported);
        }

        let lsize = usize::try_from(bp.lsize).map_err(|_| KernelError::InvalidArgument)?;
        let psize = usize::try_from(bp.psize).map_err(|_| KernelError::InvalidArgument)?;

        let mut last_err = KernelError::IoError;
        for dva in &bp.dvas {
            if dva.is_empty() {
                continue;
            }
            let raw = match self.read_copy(bp, dva, psize, 0) {
                Ok(v) => v,
                Err(e) => {
                    last_err = e;
                    continue;
                }
            };
            match zio::decompress(bp.compress, &raw, lsize) {
                Ok(v) => return Ok(v),
                Err(e) => last_err = e,
            }
        }
        Err(last_err)
    }

    /// Read `bp`'s `psize` bytes as stored — verified, but not decompressed —
    /// from whichever copy answers first.
    ///
    /// This is [`Self::read_block`] without the decompression step and without
    /// the caller-facing validation, and it exists because a gang piece needs
    /// exactly that: its own ditto copies tried in turn, its own checksum
    /// proved, and its stored bytes handed back to be concatenated. A gang
    /// piece is never compressed on its own — only the block as a whole is —
    /// so there is nothing to decompress here.
    fn read_raw(&self, bp: &BlkPtr, depth: u32) -> KernelResult<Vec<u8>> {
        if bp.embedded || bp.encrypted || !bp.little_endian {
            // ZFS does not put an embedded pointer in a gang header — an
            // embedded block needs no allocation, so it cannot be the thing a
            // failed allocation was split into. Refusing is therefore not a
            // gap in coverage; it is declining to invent a meaning for a shape
            // the writer never produces.
            return Err(KernelError::NotSupported);
        }
        if bp.psize == 0 || bp.psize > SPA_MAXBLOCKSIZE {
            return Err(KernelError::InvalidArgument);
        }
        let psize = usize::try_from(bp.psize).map_err(|_| KernelError::InvalidArgument)?;

        let mut last_err = KernelError::IoError;
        for dva in &bp.dvas {
            if dva.is_empty() {
                continue;
            }
            match self.read_copy(bp, dva, psize, depth) {
                Ok(v) => return Ok(v),
                Err(e) => last_err = e,
            }
        }
        Err(last_err)
    }

    /// Read one named copy of `bp`: `psize` verified bytes, gang or not.
    fn read_copy(&self, bp: &BlkPtr, dva: &Dva, psize: usize, depth: u32) -> KernelResult<Vec<u8>> {
        if dva.vdev != 0 {
            return Err(KernelError::NotSupported);
        }
        let phys = dva.physical_offset()?;
        if dva.gang {
            return self.read_gang(bp, phys, dva.asize, psize, depth);
        }
        let raw = read_bytes(self.src, phys, psize).map_err(|_| KernelError::IoError)?;
        zio::verify(bp.checksum, &raw, &bp.cksum)?;
        Ok(raw)
    }

    /// Reassemble the gang block whose header sits at `phys`, into exactly
    /// `psize` bytes.
    fn read_gang(
        &self,
        bp: &BlkPtr,
        phys: u64,
        asize: u64,
        psize: usize,
        depth: u32,
    ) -> KernelResult<Vec<u8>> {
        if depth >= MAX_GANG_DEPTH {
            return Err(KernelError::NotSupported);
        }
        let verifier = gang_verifier(bp);
        let gbh = self.read_gang_header(phys, asize, &verifier)?;

        // The trailer is the last 40 bytes; everything before it that is a
        // whole block pointer is one, and the remainder is padding.
        let nblkptrs = gbh.len().saturating_sub(ZEC_LEN) / BLKPTR_LEN;
        let mut out: Vec<u8> = Vec::new();
        for g in 0..nblkptrs {
            let off = g
                .checked_mul(BLKPTR_LEN)
                .ok_or(KernelError::InvalidArgument)?;
            let gbp = parse_blkptr(&gbh, off)?;
            if gbp.is_hole() {
                // A header allocated more pointer slots than the split needed.
                // Unlike a hole in a file this contributes nothing at all — not
                // even zeroes — because the pieces are defined by concatenation
                // and an unused slot is not a piece.
                continue;
            }
            let part = self.read_raw(&gbp, depth.wrapping_add(1))?;
            let total = out
                .len()
                .checked_add(part.len())
                .ok_or(KernelError::InvalidArgument)?;
            if total > psize {
                return Err(KernelError::InvalidArgument);
            }
            out.extend_from_slice(&part);
        }
        if out.len() != psize {
            // The pieces must account for the whole block. A short tree means
            // the header and the pointer disagree about how much was written,
            // and padding the difference with zeroes would hand the caller
            // invented bytes that then pass no further check — the parent's
            // checksum does not cover assembled gang data.
            return Err(KernelError::InvalidArgument);
        }
        Ok(out)
    }

    /// Read and verify a gang header, resolving how long it is.
    ///
    /// A header used to be 512 bytes always. The `dynamic_gang_header` feature
    /// made it one minimum allocation on its vdev instead, which on an
    /// `ashift=12` pool is 4096. This driver does not read `features_for_read`,
    /// so it does what ZFS itself does in `zio_checksum_error`: try the larger
    /// size the allocation admits, and fall back to 512 if that does not
    /// verify. The self-checksum is what decides, so a wrong guess cannot be
    /// mistaken for a right one — it fails the trailer magic or the hash.
    fn read_gang_header(
        &self,
        phys: u64,
        asize: u64,
        verifier: &ZioCksum,
    ) -> KernelResult<Vec<u8>> {
        let big = usize::try_from(asize)
            .ok()
            .filter(|&n| n > SPA_OLD_GANGBLOCKSIZE && n <= GANG_HEADER_MAX && n % BLKPTR_LEN == 0);
        if let Some(len) = big
            && let Ok(buf) = read_bytes(self.src, phys, len)
            && zio::verify_embedded(ZIO_CHECKSUM_GANG_HEADER, &buf, verifier).is_ok()
        {
            return Ok(buf);
        }
        let buf =
            read_bytes(self.src, phys, SPA_OLD_GANGBLOCKSIZE).map_err(|_| KernelError::IoError)?;
        zio::verify_embedded(ZIO_CHECKSUM_GANG_HEADER, &buf, verifier)?;
        Ok(buf)
    }

    /// Resolve block `blkid` of the object described by `dn`.
    ///
    /// Returns a zero-filled block for a hole, which is how a sparse file's
    /// unwritten middle reads.
    ///
    /// # Errors
    ///
    /// As [`Self::read_block`], plus [`KernelError::InvalidArgument`] if the
    /// dnode's declared level count and block id do not fit together.
    pub fn read_object_block(&self, dn: &Dnode, blkid: u64) -> KernelResult<Vec<u8>> {
        let datablksz = usize::try_from(dn.datablksz).map_err(|_| KernelError::InvalidArgument)?;
        if !dn.is_allocated() {
            return Err(KernelError::InvalidArgument);
        }

        // Bits of block id consumed per level of indirection: an indirect
        // block of `1 << indblkshift` bytes holds `1 << (indblkshift - 7)`
        // pointers.
        let epbs = u32::from(dn.indblkshift)
            .checked_sub(SPA_BLKPTRSHIFT)
            .ok_or(KernelError::InvalidArgument)?;
        let top_level = u32::from(dn.nlevels)
            .checked_sub(1)
            .ok_or(KernelError::InvalidArgument)?;

        let top_shift = top_level
            .checked_mul(epbs)
            .ok_or(KernelError::InvalidArgument)?;
        if top_shift >= 64 {
            return Err(KernelError::InvalidArgument);
        }
        let top_idx =
            usize::try_from(blkid >> top_shift).map_err(|_| KernelError::InvalidArgument)?;

        let Some(mut bp) = dn.blkptrs.get(top_idx).copied() else {
            // Past the end of the inline pointer array is past the end of the
            // object; a hole, not a failure.
            return Ok(vec![0u8; datablksz]);
        };

        let mut level = top_level;
        while level > 0 {
            if bp.is_hole() {
                return Ok(vec![0u8; datablksz]);
            }
            let indirect = self.read_block(&bp)?;
            level = level.wrapping_sub(1);
            let shift = level
                .checked_mul(epbs)
                .ok_or(KernelError::InvalidArgument)?;
            if shift >= 64 {
                return Err(KernelError::InvalidArgument);
            }
            let mask = if epbs >= 64 {
                u64::MAX
            } else {
                (1u64 << epbs).wrapping_sub(1)
            };
            let idx = usize::try_from((blkid >> shift) & mask)
                .map_err(|_| KernelError::InvalidArgument)?;
            let bp_off = idx
                .checked_mul(BLKPTR_LEN)
                .ok_or(KernelError::InvalidArgument)?;
            if bp_off.saturating_add(BLKPTR_LEN) > indirect.len() {
                return Ok(vec![0u8; datablksz]);
            }
            bp = parse_blkptr(&indirect, bp_off)?;
        }

        if bp.is_hole() {
            return Ok(vec![0u8; datablksz]);
        }
        self.read_block(&bp)
    }

    /// Read `len` bytes of an object starting at byte `offset`, stitching
    /// across block boundaries and treating holes as zeroes.
    ///
    /// # Errors
    ///
    /// As [`Self::read_object_block`].
    pub fn read_object_range(
        &self,
        dn: &Dnode,
        offset: u64,
        len: usize,
        size_limit: u64,
    ) -> KernelResult<Vec<u8>> {
        if len == 0 || offset >= size_limit {
            return Ok(Vec::new());
        }
        let avail = size_limit.saturating_sub(offset);
        let want = usize::try_from(avail.min(len as u64)).unwrap_or(len);

        let blksz = dn.datablksz;
        if blksz == 0 {
            return Err(KernelError::InvalidArgument);
        }

        let mut out = Vec::with_capacity(want);
        let mut pos = offset;
        while out.len() < want {
            let blkid = pos.checked_div(blksz).ok_or(KernelError::InvalidArgument)?;
            let within = usize::try_from(pos.checked_rem(blksz).unwrap_or(0))
                .map_err(|_| KernelError::InvalidArgument)?;
            let block = self.read_object_block(dn, blkid)?;
            let take = want
                .saturating_sub(out.len())
                .min(block.len().saturating_sub(within));
            if take == 0 {
                // A short block at the tail of the object. Anything further is
                // past the end; stop rather than spin.
                break;
            }
            out.extend_from_slice(
                block
                    .get(within..within.saturating_add(take))
                    .unwrap_or(&[]),
            );
            pos = pos.saturating_add(take as u64);
        }
        Ok(out)
    }

    /// Read the whole of an object into one buffer.
    ///
    /// Used for ZAP blocks and short metadata objects, never for file data —
    /// a caller with a file reads ranges.
    ///
    /// # Errors
    ///
    /// As [`Self::read_object_block`].
    pub fn read_object_all(&self, dn: &Dnode, size: u64) -> KernelResult<Vec<u8>> {
        let len = usize::try_from(size).map_err(|_| KernelError::FileTooLarge)?;
        self.read_object_range(dn, 0, len, size)
    }
}

/// The verifier a gang header is checksummed against.
///
/// `zio_checksum_gang_verifier`. A gang header cannot be checksummed the
/// ordinary way, because the pointer that would hold its checksum is the same
/// pointer whose contents the header describes — ZFS would have to know the
/// header's hash before it had finished writing the header. So the header
/// carries its own hash in a trailer, and what goes into the hash in place of
/// that field is the header's *address*: vdev, offset, and the txg it was
/// written in. Forging a header therefore means placing it at exactly the
/// address the parent pointer names, in the txg the parent records.
///
/// Two details are easy to get wrong and both matter:
///
/// - The address comes from **DVA 0** (`BP_IDENTITY`) whichever copy is being
///   read. That is deliberate: it makes all ditto copies of a header
///   byte-identical, so any of them satisfies the parent.
/// - The offset is the raw DVA offset, *not* [`Dva::physical_offset`]. The
///   label/boot reserve is a property of the device, not of the block's
///   identity.
fn gang_verifier(bp: &BlkPtr) -> ZioCksum {
    let identity = bp.dvas.first().copied().unwrap_or_default();
    // `BP_GET_PHYSICAL_BIRTH`: the physical birth word is zero when it equals
    // the logical one, which is the common case, so a zero there means "read
    // the logical txg" rather than "txg zero".
    let txg = if bp.phys_birth == 0 {
        bp.birth
    } else {
        bp.phys_birth
    };
    [u64::from(identity.vdev), identity.offset, txg, 0]
}

/// Reconstruct the payload of an embedded block pointer.
///
/// The payload is stored in the pointer's own 128 bytes, in the three ranges
/// left over once the properties word and the logical birth txg are accounted
/// for. It is used for objects small enough that a separate allocation would
/// cost more than it saves — most often a short symlink target or a tiny
/// file's contents.
fn read_embedded(bp: &BlkPtr) -> KernelResult<Vec<u8>> {
    // The embedded type is stored where a non-embedded pointer keeps the
    // checksum algorithm.
    if u64::from(bp.checksum) != BP_EMBEDDED_TYPE_DATA {
        return Err(KernelError::NotSupported);
    }
    let lsize = usize::try_from(bp.lsize).map_err(|_| KernelError::InvalidArgument)?;
    let psize = usize::try_from(bp.psize).map_err(|_| KernelError::InvalidArgument)?;
    if psize > BPE_PAYLOAD_SIZE || lsize > BPE_PAYLOAD_SIZE {
        return Err(KernelError::InvalidArgument);
    }

    // Re-serialise the pointer so the payload ranges can be sliced out of it.
    // Keeping `BlkPtr` a parsed struct rather than a borrowed byte range is
    // worth this one reconstruction: every other consumer wants the fields.
    let mut words = [0u8; BLKPTR_LEN];
    for (i, dva) in bp.dvas.iter().enumerate() {
        // The DVA words *are* payload here; `parse_dva` shifted and masked
        // them, so the original bytes have to be rebuilt from the same
        // definitions in reverse.
        let w0 = (u64::from(dva.vdev) << 32) | ((dva.asize >> 9) & 0x00ff_ffff);
        let w1 = ((dva.offset >> 9) & 0x7fff_ffff_ffff_ffff) | (u64::from(dva.gang) << 63);
        let base = i.wrapping_mul(16);
        if let Some(dst) = words.get_mut(base..base.wrapping_add(8)) {
            dst.copy_from_slice(&w0.to_le_bytes());
        }
        if let Some(dst) = words.get_mut(base.wrapping_add(8)..base.wrapping_add(16)) {
            dst.copy_from_slice(&w1.to_le_bytes());
        }
    }
    if let Some(dst) = words.get_mut(56..64) {
        dst.copy_from_slice(&bp.phys_birth.to_le_bytes());
    }
    if let Some(dst) = words.get_mut(64..72) {
        dst.copy_from_slice(&bp.fill.to_le_bytes());
    }
    for (i, w) in bp.cksum.iter().enumerate() {
        let base = 96usize.wrapping_add(i.wrapping_mul(8));
        if let Some(dst) = words.get_mut(base..base.wrapping_add(8)) {
            dst.copy_from_slice(&w.to_le_bytes());
        }
    }

    let mut payload = Vec::with_capacity(BPE_PAYLOAD_SIZE);
    for (start, end) in BPE_PAYLOAD_RANGES {
        payload.extend_from_slice(words.get(start..end).unwrap_or(&[]));
    }
    payload.truncate(psize);
    zio::decompress(bp.compress, &payload, lsize)
}
