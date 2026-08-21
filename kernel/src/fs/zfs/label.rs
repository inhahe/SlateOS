//! Vdev labels and uberblock selection: finding the newest consistent view of
//! a pool.
//!
//! # The four labels
//!
//! Every device in a pool carries four identical 256 KiB labels — two at the
//! front, two at the back — so that a whole-device overwrite of either end
//! still leaves a readable copy. The front pair is written first and the back
//! pair second (with a flush between), which is what makes a torn label
//! update survivable rather than fatal.
//!
//! This driver reads the front pair. The back pair sits at
//! `device_size - 512 KiB` and `device_size - 256 KiB`, and a
//! [`SectorSource`](crate::fs::blocksrc::SectorSource) does not report a size
//! — it is a byte-range reader over whatever the caller opened. Reading the
//! back labels would mean either adding a size to that trait for one
//! filesystem's benefit, or guessing at a size and reading past the end of a
//! partition. Neither is worth it for a read-only mount whose failure mode is
//! "refuses to mount a volume whose first 512 KiB is damaged" — a volume that
//! damaged has bigger problems, and `zpool import` on a real ZFS host is the
//! right tool for it.
//!
//! # Choosing an uberblock
//!
//! The tail 128 KiB of each label is an array of uberblocks, written
//! round-robin one per transaction group. There is no "current" pointer: the
//! current uberblock is *the valid one with the highest `txg`*, which is what
//! makes a ZFS pool consistent after a crash without a journal replay. A slot
//! whose txg is lower is simply an older, still-self-consistent view.
//!
//! Slot size is `1 << max(ashift, 10)`, so a 4 KiB-sector pool has 32
//! uberblocks per label rather than 128. Getting that wrong does not fail
//! loudly — it reads uberblocks at the wrong stride and finds fewer of them —
//! which is why `ashift` is taken from the label's own nvlist rather than
//! assumed.

use alloc::vec::Vec;

use crate::error::{KernelError, KernelResult};
use crate::fs::blocksrc::{SectorSource, read_bytes};

use super::nvlist::NvList;
use super::raw::ZIO_CHECKSUM_LABEL;
use super::raw::{
    BlkPtr, UBERBLOCK_MAGIC, UBERBLOCK_MAGIC_SWAPPED, UBERBLOCK_SHIFT, VDEV_LABEL_NVLIST_OFFSET,
    VDEV_LABEL_NVLIST_SIZE, VDEV_LABEL_SIZE, VDEV_LABEL_UBERBLOCK_OFFSET,
    VDEV_LABEL_UBERBLOCK_SIZE, parse_blkptr, read_u64,
};
use super::zio::{ZEC_LEN, ZioCksum, verify_embedded};

/// Highest SPA version this driver will mount.
///
/// Version 5000 is "feature flags": everything past 28 is negotiated per
/// feature rather than by a monotonic number. We accept it and then check the
/// individual features we care about, which is the only way to mount a modern
/// pool at all.
pub const SPA_VERSION_FEATURES: u64 = 5000;

/// Pool is in use by a live system — the ordinary case.
pub const POOL_STATE_ACTIVE: u64 = 0;
/// Pool was cleanly exported. Its on-disk state is consistent and complete,
/// so a read-only mount is legitimate; `zpool import` accepts it unprompted.
pub const POOL_STATE_EXPORTED: u64 = 1;
/// Pool was destroyed. The labels survive — `zpool destroy` only rewrites the
/// state field — but nothing else about the pool is promised.
pub const POOL_STATE_DESTROYED: u64 = 2;
/// Device is a hot spare, held in reserve by one or more pools.
pub const POOL_STATE_SPARE: u64 = 3;
/// Device is an L2ARC second-level cache.
pub const POOL_STATE_L2CACHE: u64 = 4;

/// Offset of `ub_rootbp` within an uberblock.
const UB_ROOTBP_OFFSET: usize = 40;

/// A parsed uberblock — the root of everything else in the pool.
#[derive(Debug, Clone, Copy)]
pub struct Uberblock {
    /// SPA version, or [`SPA_VERSION_FEATURES`].
    pub version: u64,
    /// Transaction group. The highest valid one wins.
    pub txg: u64,
    /// Sum of every vdev GUID, used by `zpool import` to spot a missing
    /// device. Recorded for diagnostics; a single-vdev mount cannot act on it.
    pub guid_sum: u64,
    /// Seconds since the epoch at which this txg was committed.
    pub timestamp: u64,
    /// Block pointer to the meta object set.
    pub rootbp: BlkPtr,
    /// Byte offset in the device this copy was read from, for diagnostics.
    pub offset: u64,
}

/// What a label told us about the pool, beyond the uberblock.
#[derive(Debug, Clone)]
pub struct PoolConfig {
    /// Pool name.
    pub name: Vec<u8>,
    /// Pool GUID.
    pub guid: u64,
    /// `log2` of the vdev's sector size. Also sets the uberblock stride.
    pub ashift: u32,
    /// Top-level vdev type: `disk`, `file`, `mirror`, `raidz`…
    pub vdev_type: Vec<u8>,
    /// txg the config was written at.
    pub txg: u64,
    /// SPA version.
    pub version: u64,
}

/// Byte offset of label `l` (0 or 1) at the front of a device.
#[must_use]
pub const fn front_label_offset(l: u64) -> u64 {
    l.saturating_mul(VDEV_LABEL_SIZE)
}

/// Read and parse the pool configuration from the first readable front label.
///
/// A `disk`, `file` or `mirror` top-level vdev is accepted: in each case the
/// one device in hand is a complete copy of that vdev's address space.
///
/// # Errors
///
/// - [`KernelError::InvalidArgument`] if neither front label holds a decodable
///   nvlist, or if the config contradicts itself — a `mirror` with no
///   `children`, a `disk`/`file` that has them, or an `ashift` outside 9..=16.
/// - [`KernelError::NotSupported`] if the pool is not `ACTIVE` or `EXPORTED`
///   (a hot spare, an L2ARC cache device, or a destroyed pool), if its SPA
///   version is above [`SPA_VERSION_FEATURES`], if the top-level vdev is any
///   type but `disk`/`file`/`mirror` (raidz and draid interleave parity across
///   devices; `replacing` and `spare` have a child that is mid-resilver and so
///   not yet a replica), if this device is a separate intent log, or if it
///   holds any top-level vdev but the first — this driver reads one device and
///   cannot reconstruct a stripe it cannot see.
pub fn read_config(src: &dyn SectorSource) -> KernelResult<PoolConfig> {
    let mut last_err = KernelError::InvalidArgument;
    for l in 0..2u64 {
        let off = front_label_offset(l).saturating_add(VDEV_LABEL_NVLIST_OFFSET);
        let raw = match read_bytes(src, off, VDEV_LABEL_NVLIST_SIZE) {
            Ok(v) => v,
            Err(e) => {
                last_err = e;
                continue;
            }
        };
        match parse_config(&raw) {
            Ok(cfg) => return Ok(cfg),
            Err(e) => last_err = e,
        }
    }
    Err(last_err)
}

/// Decode a label's nvlist region into a [`PoolConfig`].
///
/// # Errors
///
/// As [`read_config`].
pub fn parse_config(raw: &[u8]) -> KernelResult<PoolConfig> {
    let nv = NvList::new(raw)?;

    let name = nv
        .get_str(b"name")
        .ok_or(KernelError::InvalidArgument)?
        .to_vec();
    let guid = nv
        .get_u64(b"pool_guid")
        .ok_or(KernelError::InvalidArgument)?;
    let txg = nv.get_u64(b"txg").unwrap_or(0);
    let version = nv.get_u64(b"version").unwrap_or(0);

    // `SPA_VERSION_FEATURES` is documented as the highest version we mount, so
    // compare against it rather than leaving the constant a claim nothing
    // enforces. Nothing above 5000 exists today — 28 was the last numbered
    // version before feature flags, and feature flags were designed so the
    // number would never need to rise again — which is exactly why this is
    // worth writing down now: if it ever does rise, the pool that carries it
    // has a layout we have never seen, and refusing it by name beats parsing
    // it by accident.
    //
    // No lower bound. Old pools are not rejected on suspicion; if some
    // version-6 layout difference does break us it will surface as a real
    // parse failure, which is more informative than a blanket refusal we
    // never tested.
    if version > SPA_VERSION_FEATURES {
        return Err(KernelError::NotSupported);
    }

    // A pool's state is recorded in its labels, and three of the values name a
    // device that has a complete, entirely valid label and no pool data behind
    // it. A hot spare and an L2ARC cache device are both *members* of a pool
    // while holding none of its object tree: the spare is unwritten reserve
    // capacity, the cache is an evictable copy of blocks whose home is
    // elsewhere. A destroyed pool is the same disk it was a moment before
    // `zpool destroy` — that command rewrites the state field and little else,
    // which is what makes `zpool import -D` able to undo it — but nothing
    // promises the object tree is still coherent, and silently mounting a pool
    // the administrator deleted is the wrong default whatever its condition.
    //
    // Without this, all three reach `find_uberblock`. A spare or cache device
    // has no uberblock array worth the name and fails as `InvalidArgument`
    // ("not a ZFS device"), which is false — it is very much a ZFS device. A
    // destroyed pool usually still has its uberblocks and mounts, which is
    // worse than a wrong error message.
    //
    // `EXPORTED` is accepted alongside `ACTIVE`: a clean export leaves the
    // on-disk state consistent by definition, and read-only is all this driver
    // does. That matches `zpool import`, which takes an exported pool without
    // argument and demands `-D` only for a destroyed one.
    //
    // Absent means `ACTIVE`. The field is always written in practice, and
    // defaulting the other way would turn any label key we merely failed to
    // parse into a refusal to mount a healthy pool.
    let state = nv.get_u64(b"state").unwrap_or(POOL_STATE_ACTIVE);
    if state != POOL_STATE_ACTIVE && state != POOL_STATE_EXPORTED {
        return Err(KernelError::NotSupported);
    }

    let vdev_tree = nv
        .nested(b"vdev_tree")
        .ok_or(KernelError::InvalidArgument)?;

    let vdev_type = vdev_tree
        .get_str(b"type")
        .ok_or(KernelError::InvalidArgument)?
        .to_vec();

    // One `SectorSource` is one device, so the question every vdev type has to
    // answer is: *is the device in hand a complete copy of this vdev's address
    // space?* This is a whitelist rather than a raidz blacklist because the
    // answer is "no" for every type not named here, including ones ZFS may add
    // later, and a blacklist would silently admit those.
    //
    // - `disk` / `file`: the vdev *is* the device. Trivially yes.
    // - `mirror`: every leg holds byte-identical content at identical offsets
    //   — that is what makes a mirror a mirror, and it is why ZFS itself is
    //   free to service a read from whichever leg is idle. A DVA names the
    //   top-level vdev and an offset within it, so it resolves to the same
    //   physical offset on the device in hand as on any other leg. Yes.
    // - `raidz` / `draid`: a leg holds interleaved data and parity columns, so
    //   the bytes at a DVA's offset on one device are a *stripe fragment*, not
    //   the record. Reconstruction needs every column, and we have one. No.
    // - `replacing` / `spare`: these look exactly like a mirror — a parent with
    //   replica children — and are the dangerous case precisely because of
    //   that. One child is mid-resilver and therefore *not yet* a complete
    //   copy. We cannot tell which of the children is the device in hand, so
    //   accepting these would be a coin flip on whether the reads are real. No.
    //
    // The per-block checksum is the backstop if any of this reasoning is wrong:
    // a misread block fails verification and surfaces as `IoError` rather than
    // as wrong data. It is a backstop, not the argument.
    let is_mirror = vdev_type == b"mirror";
    if vdev_type != b"disk" && vdev_type != b"file" && !is_mirror {
        return Err(KernelError::NotSupported);
    }

    // A leaf vdev with children is not a shape ZFS produces; a mirror without
    // them is not a mirror. Either way the config contradicts itself, so both
    // are `InvalidArgument`: a self-contradictory label is a parse problem, not
    // a layout we decline to support, and reporting `NotSupported` would send
    // whoever reads it looking for a missing feature instead of at the label.
    //
    // Note this branch means something narrower than it did before the vdev
    // type whitelist above existed. It used to be the mirror/raidz detector —
    // "has children" was how a parent vdev was recognised, and `NotSupported`
    // was the right answer. Type is now checked first, so the only thing that
    // reaches here is a `disk`/`file` claiming children, which is malformed.
    let children = vdev_tree.nested_array_len(b"children");
    if is_mirror {
        if children.unwrap_or(0) == 0 {
            return Err(KernelError::InvalidArgument);
        }
    } else if children.is_some() {
        return Err(KernelError::InvalidArgument);
    }

    // `ashift` is written on the *top-level* vdev — for a single disk that is
    // the disk itself, for a mirror it is the mirror — so this reads it from
    // the right place in both cases. The fallback into the first child covers
    // a label that recorded it only on the leaves: `nested` returns the first
    // element of a `children` array, which is all that is needed, since every
    // leg of a mirror shares one ashift by construction.
    //
    // Getting this wrong is not silent. The uberblock stride derives from
    // `ashift`, and an uberblock is checksummed against the byte offset it was
    // written at, so a wrong stride finds no valid uberblock and the mount
    // fails — rather than reading the pool at the wrong granularity.
    let ashift = vdev_tree
        .get_u64(b"ashift")
        .or_else(|| {
            vdev_tree
                .nested(b"children")
                .and_then(|first| first.get_u64(b"ashift"))
        })
        .unwrap_or(u64::from(UBERBLOCK_SHIFT));
    let ashift = u32::try_from(ashift).map_err(|_| KernelError::InvalidArgument)?;
    if !(9..=16).contains(&ashift) {
        // Outside this range the pool is not something ZFS creates, and the
        // uberblock stride derived from it would be nonsense.
        return Err(KernelError::InvalidArgument);
    }

    // A separate intent-log (SLOG) device is a full member of the pool and
    // carries a complete, valid label — same `name`, same `pool_guid`, its own
    // uberblock array kept current with the pool's transaction groups. Nothing
    // above this line distinguishes it from the data disk, so without this
    // check it is accepted, and the failure surfaces several layers later as a
    // checksum mismatch on the meta object set: `IoError`, which reads as "this
    // pool is corrupt" when in truth the pool is fine and we opened the wrong
    // device of it. The log holds recently-committed writes awaiting replay,
    // never the object tree the DVAs point into.
    if vdev_tree.get_u64(b"is_log").unwrap_or(0) != 0 {
        return Err(KernelError::NotSupported);
    }

    // `id` is this top-level vdev's index in the pool. Only vdev 0 is
    // addressable here — `dmu::Reader::read_block` refuses any DVA whose vdev
    // is not 0 — so on a device holding vdev 1 or later *every* readable DVA
    // names storage that is somewhere else, and the mount is doomed before it
    // starts. Catching it here names the real problem (this is the wrong
    // device of a multi-vdev pool) at the point where the answer is still in
    // hand, instead of at the first block read.
    //
    // A stripe's second disk says `type: disk` with no children, exactly as a
    // single-disk pool's label does, so `id` is the only field in the config
    // that tells the two apart.
    //
    // This does not make every striped pool refuse to mount, and is not meant
    // to: opening the *first* disk of a stripe still succeeds here, because
    // vdev 0 genuinely is the device in hand. That mount then fails at the
    // first block that lives on another vdev, which `read_block` reports as
    // `NotSupported` after trying the block pointer's other copies. Detecting
    // the whole-pool shape is impossible from one label — the pool-wide vdev
    // tree lives in the MOS config object, which is itself reached through a
    // DVA — so the honest boundary is per-device, and that is what this is.
    //
    // Absent means 0 — the field has always been written, and defaulting the
    // other way would newly refuse any pool whose label we merely failed to
    // parse a key from.
    if vdev_tree.get_u64(b"id").unwrap_or(0) != 0 {
        return Err(KernelError::NotSupported);
    }

    Ok(PoolConfig {
        name,
        guid,
        ashift,
        vdev_type,
        txg,
        version,
    })
}

/// Scan both front labels' uberblock arrays and return the valid uberblock
/// with the highest transaction group.
///
/// # Errors
///
/// - [`KernelError::NotSupported`] if the only uberblocks found are
///   byte-swapped, i.e. the pool was created on a big-endian host.
/// - [`KernelError::InvalidArgument`] if no uberblock with the right magic
///   was found at all, which means this is not a ZFS device (or the front of
///   it has been overwritten).
pub fn find_uberblock(src: &dyn SectorSource, ashift: u32) -> KernelResult<Uberblock> {
    let slot_shift = ashift.max(UBERBLOCK_SHIFT);
    if slot_shift >= 32 {
        return Err(KernelError::InvalidArgument);
    }
    let slot_size = 1u64 << slot_shift;
    let slots = VDEV_LABEL_UBERBLOCK_SIZE
        .checked_div(slot_size)
        .ok_or(KernelError::InvalidArgument)?;
    let slot_len = usize::try_from(slot_size).map_err(|_| KernelError::InvalidArgument)?;

    let mut best: Option<Uberblock> = None;
    let mut saw_swapped = false;

    for l in 0..2u64 {
        let array_off = front_label_offset(l).saturating_add(VDEV_LABEL_UBERBLOCK_OFFSET);
        // One read per label rather than one per slot: 128 KiB through the
        // block cache is a handful of requests, whereas 128 separate 1 KiB
        // reads is 128 round trips to answer a question asked once at mount.
        let Ok(array) = read_bytes(
            src,
            array_off,
            usize::try_from(VDEV_LABEL_UBERBLOCK_SIZE).unwrap_or(0),
        ) else {
            continue;
        };

        for i in 0..slots {
            let start = match usize::try_from(i.saturating_mul(slot_size)) {
                Ok(v) => v,
                Err(_) => continue,
            };
            let Some(slot) = array.get(start..start.saturating_add(slot_len)) else {
                continue;
            };
            let Ok(magic) = read_u64(slot, 0) else {
                continue;
            };
            if magic == UBERBLOCK_MAGIC_SWAPPED {
                saw_swapped = true;
                continue;
            }
            if magic != UBERBLOCK_MAGIC {
                continue;
            }

            let Ok(ub) = parse_uberblock(slot, array_off.saturating_add(start as u64)) else {
                continue;
            };
            // A txg of 0 is an unwritten slot; a rootbp that points nowhere
            // is one too, and both would otherwise beat a real uberblock if
            // the array happened to be zeroed.
            if ub.txg == 0 || ub.rootbp.is_hole() {
                continue;
            }
            // The uberblock is self-checksummed against its own byte offset,
            // which is what proves we computed the slot stride correctly: a
            // wrong stride lands mid-slot and fails here rather than handing
            // back a half-uberblock that parses.
            if verify_uberblock(slot, ub.offset).is_err() {
                continue;
            }
            if best.is_none_or(|b| ub.txg > b.txg) {
                best = Some(ub);
            }
        }
    }

    best.ok_or(if saw_swapped {
        // Naming this specifically matters: "not a ZFS pool" would send
        // whoever reads it looking for the wrong problem entirely.
        KernelError::NotSupported
    } else {
        KernelError::InvalidArgument
    })
}

/// Parse one uberblock slot.
///
/// # Errors
///
/// [`KernelError::InvalidArgument`] if the slot is shorter than an uberblock.
pub fn parse_uberblock(slot: &[u8], offset: u64) -> KernelResult<Uberblock> {
    Ok(Uberblock {
        version: read_u64(slot, 8)?,
        txg: read_u64(slot, 16)?,
        guid_sum: read_u64(slot, 24)?,
        timestamp: read_u64(slot, 32)?,
        rootbp: parse_blkptr(slot, UB_ROOTBP_OFFSET)?,
        offset,
    })
}

/// Verify an uberblock slot's embedded SHA-256 against its byte offset.
///
/// # Errors
///
/// Propagates [`verify_embedded`]: [`KernelError::InvalidArgument`] if the
/// trailer magic is absent, [`KernelError::IoError`] on a mismatch.
pub fn verify_uberblock(slot: &[u8], offset: u64) -> KernelResult<()> {
    if slot.len() < ZEC_LEN {
        return Err(KernelError::InvalidArgument);
    }
    let verifier: ZioCksum = [offset, 0, 0, 0];
    verify_embedded(ZIO_CHECKSUM_LABEL, slot, &verifier)
}
