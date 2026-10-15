//! `swapon` and `swapoff` by path: what both ABIs' calls share.
//!
//! [`crate::mm::swap`] keeps the areas and knows nothing of paths; this
//! resolves one. A path names either a block device -- a `/dev` node, the
//! whole device or (once there are partition nodes) a partition: Linux's
//! `partition` -- or a regular file, whose filesystem gives the sectors it
//! occupies and refuses to change it while it is swap
//! ([`crate::fs::vfs::FileSystem::swapfile_claim`]): Linux's `file`. One
//! device is neither: `/dev/zram0` names the compressed tier
//! ([`swap::MEMORY_TIER_DEVICE`]), which these turn off and on itself.
//!
//! Root's authority is the caller's to check (`CAP_SYS_ADMIN` on Linux; uid 0
//! here, as for mounting): these take a path that is already the kernel's.

use alloc::string::String;
use alloc::vec::Vec;

use crate::error::{KernelError, KernelResult};
use crate::fs::path::Path;
use crate::fs::{FileId, SwapFileClaim};
use crate::mm::swap::{self, AreaExtent, AreaKind, AreaRequest, AreaTarget};
use crate::sched::kmutex::KMutex;

/// `swapon`'s `SWAP_FLAG_PREFER`: the low 15 bits are the priority.
pub const SWAP_FLAG_PREFER: u32 = 0x8000;
/// `swapon`'s priority bits.
pub const SWAP_FLAG_PRIO_MASK: u32 = 0x7FFF;
/// Every flag `swapon` knows (Linux's `SWAP_FLAGS_VALID`): the priority,
/// `SWAP_FLAG_PREFER`, and the three discard flags (accepted; slots are not
/// discarded yet).
pub const SWAP_FLAGS_VALID: u32 = 0x7_FFFF;

/// The swap files that are swap now: each one's claim, held from its
/// `swapon` to its `swapoff`. Dropping a claim lets the file go
/// ([`SwapFileClaim`]), so one is dropped only after its area is gone. A
/// sleeping lock, held across a file's activation; taken before
/// `swap::SWAP_ADMIN`, never under it.
static CLAIMS: KMutex<Vec<SwapFileClaim>> = KMutex::new(Vec::new());

/// The block device a resolved path names, if it is a `/dev` node of one,
/// and its size in sectors.
fn device_of(path: &Path) -> Option<(String, u64)> {
    let rest = path.as_bytes().strip_prefix(b"/dev/")?;
    if rest.is_empty() || rest.contains(&b'/') {
        return None;
    }
    let name = core::str::from_utf8(rest).ok()?;
    let info = crate::blkdev::info(name)?;
    Some((String::from(name), info.sector_count))
}

/// Turn swap on at the resolved path `path` with `swapon`'s `flags`.
///
/// # Errors
///
/// `InvalidArgument` for flags outside [`SWAP_FLAGS_VALID`], a path that is
/// neither a block device nor a regular file, a file with holes, or an area
/// with no swap header ([`swap::activate`]); `NotSupported` for a file on a
/// filesystem that cannot give its sectors; `DeviceBusy` when it is swap
/// already; the lookup's own errors; and [`swap::activate`]'s.
pub fn swap_on(path: &Path, flags: u32) -> KernelResult<swap::ActiveArea> {
    if flags & !SWAP_FLAGS_VALID != 0 {
        return Err(KernelError::InvalidArgument);
    }
    let priority = (flags & SWAP_FLAG_PREFER != 0)
        .then(|| i32::try_from(flags & SWAP_FLAG_PRIO_MASK).unwrap_or(0));
    let Some((device, sectors)) = device_of(path) else {
        return swap_on_file(path, priority);
    };
    if device == swap::MEMORY_TIER_DEVICE {
        return swap::enable_memory_tier(priority);
    }
    swap::activate(AreaRequest {
        path: path.as_bytes().to_vec(),
        kind: AreaKind::Device,
        device,
        extents: alloc::vec![AreaExtent {
            area_sector: 0,
            dev_sector: 0,
            sectors,
        }],
        priority,
    })
}

/// [`swap_on`] for a regular file: claimed for as long as it is swap, and
/// let go again when it does not become swap.
fn swap_on_file(path: &Path, priority: Option<i32>) -> KernelResult<swap::ActiveArea> {
    let claim = crate::fs::Vfs::swapfile_claim_resolved(path)?;
    let FileId { fs_id, ino } = claim.id();
    let request = AreaRequest {
        path: path.as_bytes().to_vec(),
        kind: AreaKind::File { fs_id, ino },
        device: claim.file().device.clone(),
        extents: claim.file().extents.clone(),
        priority,
    };
    // Held across the activation, with room for the claim made first: once
    // the area exists, nothing can fail before the claim is kept. (Not swap
    // after all: the claim is dropped on the way out, and the filesystem may
    // change the file again.)
    let mut claims = CLAIMS.lock();
    claims
        .try_reserve(1)
        .map_err(|_| KernelError::OutOfMemory)?;
    let area = swap::activate(request)?;
    claims.push(claim);
    Ok(area)
}

/// Turn swap off at the resolved path `path`: the device it names, or the
/// file -- by identity, so any name for it will do.
///
/// # Errors
///
/// `InvalidArgument` when it is not a swap area; the lookup's errors; and
/// [`swap::deactivate`]'s.
pub fn swap_off(path: &Path) -> KernelResult<()> {
    let target = match device_of(path) {
        Some((device, _)) if device == swap::MEMORY_TIER_DEVICE => AreaTarget::Memory,
        Some((device, _)) => AreaTarget::Device(device),
        None => {
            let id = crate::fs::Vfs::file_identity_resolved(path)?
                .ok_or(KernelError::InvalidArgument)?;
            AreaTarget::File {
                fs_id: id.fs_id,
                ino: id.ino,
            }
        }
    };
    let removed = swap::deactivate(&target)?;
    if let AreaKind::File { fs_id, ino } = removed.kind {
        let claim = {
            let mut claims = CLAIMS.lock();
            claims
                .iter()
                .position(|c| c.id() == FileId { fs_id, ino })
                .map(|i| claims.swap_remove(i))
        };
        // Dropped here, after the area went: the file is let go.
        drop(claim);
    }
    Ok(())
}

/// The compressed tier off and on by its path, `/dev/zram0`, as `swapoff -a`
/// and `swapon` see it: listed under that path; `/dev/zram0` reads as a swap
/// header for the tier's size and refuses a write; `swapoff` takes the tier
/// away, its pages back in memory, and a second is refused; `swapon` puts it
/// back, at the priority asked, and a second is refused; `swapon` with no
/// priority gives 100.
///
/// # Errors
///
/// `InternalError` naming what failed.
pub fn self_test_memory_tier() -> KernelResult<()> {
    crate::serial_println!("[swapctl] Running compressed-tier self-test...");
    let path = Path::new("/dev/zram0");
    let listed = |path: &[u8]| {
        swap::list_devices()
            .into_iter()
            .find(|d| d.path == path)
            .map(|d| d.priority)
    };
    let result = (|| -> Result<(), &'static str> {
        let before = listed(path.as_bytes()).ok_or("the tier is not listed as /dev/zram0")?;
        let info = crate::blkdev::info(swap::MEMORY_TIER_DEVICE).ok_or("no /dev/zram0")?;
        let mut first = alloc::vec![0u8; 16_384];
        for (i, sector) in first.chunks_mut(512).enumerate() {
            let mut buf = [0u8; 512];
            crate::blkdev::with_device(swap::MEMORY_TIER_DEVICE, |d| {
                d.read_sector(u64::try_from(i).unwrap_or(0), &mut buf)
            })
            .ok_or("no /dev/zram0")?
            .map_err(|_| "/dev/zram0 could not be read")?;
            sector.copy_from_slice(&buf);
        }
        let header = swap::parse_header(&first).map_err(|_| "/dev/zram0 has no swap header")?;
        let pages = info.sector_count.checked_div(32).unwrap_or(0);
        if u64::from(header.last_page).saturating_add(1) != pages || !info.read_only {
            return Err("/dev/zram0's header or size is not the tier's");
        }
        let refused_write = crate::blkdev::with_device(swap::MEMORY_TIER_DEVICE, |d| {
            d.write_sector(0, &[0u8; 512])
        }) == Some(Err(KernelError::ReadOnlyFilesystem));
        if !refused_write {
            return Err("/dev/zram0 took a write");
        }
        swap_off(path).map_err(|_| "swapoff /dev/zram0 failed")?;
        if listed(path.as_bytes()).is_some() {
            return Err("the tier is still listed after swapoff");
        }
        if swap_off(path).err() != Some(KernelError::InvalidArgument) {
            return Err("a second swapoff was not refused");
        }
        let area = swap_on(path, SWAP_FLAG_PREFER | 77).map_err(|_| "swapon /dev/zram0 failed")?;
        if area.priority != 77 || listed(path.as_bytes()) != Some(77) {
            return Err("swapon /dev/zram0 did not give the priority asked");
        }
        if swap_on(path, 0).err() != Some(KernelError::DeviceBusy) {
            return Err("a second swapon was not refused");
        }
        swap_off(path).map_err(|_| "the second swapoff failed")?;
        let area = swap_on(path, 0).map_err(|_| "swapon /dev/zram0 with no priority failed")?;
        if area.priority != 100 || before != 100 {
            return Err("the tier's own priority is not 100");
        }
        Ok(())
    })();
    // Whatever happened, the tier on again, as the boot left it.
    let _ = swap_on(path, 0); // DeviceBusy is the usual answer: it is on.
    match result {
        Ok(()) => {
            crate::serial_println!(
                "[swapctl]   /dev/zram0: listed, a swap header for its size, read-only, off and \
                 on again by its path, at a priority asked or 100: OK"
            );
            crate::serial_println!("[swapctl] Compressed-tier self-test PASSED");
            Ok(())
        }
        Err(what) => {
            crate::serial_println!("[swapctl]   FAIL: {}", what);
            Err(KernelError::InternalError)
        }
    }
}

/// A swap file end to end on the ext4 mounted at `/mnt`, when there is one:
/// a 2 MiB file made with a swap header and a pattern 1 MiB in is turned on;
/// its area reads the pattern back where the file's blocks are; while it is
/// swap the file refuses a write, a truncation, removal and renaming, and a
/// second `swapon`, and its filesystem an unmount; `swapoff` turns it off,
/// after which the file can be written and removed again.
///
/// # Errors
///
/// `InternalError` naming what failed.
pub fn self_test_file() -> KernelResult<()> {
    const PATH: &str = "/mnt/.swaptest-file";
    const MOVED: &str = "/mnt/.swaptest-moved";
    const AREA: usize = 2 << 20;
    const PATTERN_AT: usize = 1 << 20;
    const SLOT: usize = 16_384;

    crate::serial_println!("[swapctl] Running swap-file self-test...");
    let on_ext4 = crate::fs::Vfs::mounts()
        .iter()
        .any(|(path, kind)| path.as_path() == Path::new("/mnt") && kind == "ext4");
    if !on_ext4 {
        crate::serial_println!("[swapctl]   SKIP: no ext4 at /mnt");
        return Ok(());
    }

    // The file: a swap header for 128 pages of 16 KiB, and a pattern at 1 MiB.
    let mut data = alloc::vec![0u8; AREA];
    swap::make_header(&mut data, SLOT, 127, &[]);
    for (i, b) in data.iter_mut().skip(PATTERN_AT).take(SLOT).enumerate() {
        *b = u8::try_from(i % 251).unwrap_or(0);
    }
    let pattern: alloc::vec::Vec<u8> = data
        .get(PATTERN_AT..PATTERN_AT + SLOT)
        .map(<[u8]>::to_vec)
        .unwrap_or_default();
    // Whatever a failed run before left: gone first (it is not swap: this
    // boot turned nothing on).
    let _ = crate::fs::Vfs::remove(PATH); // Absent is the usual answer.
    crate::fs::Vfs::write_file(PATH, &data)?;
    drop(data);

    let result = (|| -> Result<(), &'static str> {
        let path = crate::fs::Vfs::resolve_path(PATH).map_err(|_| "resolve")?;
        let area = swap_on(&path, SWAP_FLAG_PREFER | SWAP_FLAG_PRIO_MASK)
            .map_err(|_| "swapon of a 2 MiB swap file failed")?;
        if area.pages != 127 {
            return Err("128 slots less the header's should hold 127 pages");
        }
        let id = crate::fs::Vfs::file_identity_resolved(&path)
            .ok()
            .flatten()
            .ok_or("no identity")?;
        let target = AreaTarget::File {
            fs_id: id.fs_id,
            ino: id.ino,
        };
        let mut back = alloc::vec![0u8; SLOT];
        swap::area_read(&target, PATTERN_AT as u64, &mut back).map_err(|_| "area read")?;
        if back != pattern {
            return Err("the area does not read the file's blocks where ext4 put them");
        }
        let listed = swap::list_devices()
            .into_iter()
            .any(|d| d.path == path.as_bytes() && d.device_type == "file");
        if !listed {
            return Err("the file is not listed as a file area");
        }
        let refusals = [
            (
                "a write was taken",
                crate::fs::Vfs::write_at(PATH, 0, b"x") == Err(KernelError::DeviceBusy),
            ),
            (
                "a truncation was taken",
                crate::fs::Vfs::truncate(PATH, 0) == Err(KernelError::DeviceBusy),
            ),
            (
                "the file was removed",
                crate::fs::Vfs::remove(PATH) == Err(KernelError::NotPermitted),
            ),
            (
                "the file was renamed",
                crate::fs::Vfs::rename(PATH, MOVED) == Err(KernelError::NotPermitted),
            ),
            (
                "a second swapon was taken",
                swap_on(&path, 0).err() == Some(KernelError::DeviceBusy),
            ),
            (
                "the filesystem under it was unmounted",
                crate::fs::Vfs::unmount("/mnt") == Err(KernelError::DeviceBusy),
            ),
        ];
        if let Some((what, _)) = refusals.iter().find(|(_, ok)| !ok) {
            return Err(*what);
        }
        swap_off(&path).map_err(|_| "swapoff failed")?;
        if swap_off(&path).err() != Some(KernelError::InvalidArgument) {
            return Err("a second swapoff was not refused");
        }
        crate::fs::Vfs::write_at(PATH, 0, b"x").map_err(|_| "the file stayed unwritable")?;
        Ok(())
    })();

    // Whatever happened: off, then gone. Each may fail for having been done
    // already, which is the success case.
    if let Ok(path) = crate::fs::Vfs::resolve_path(PATH) {
        let _ = swap_off(&path);
    }
    let removed = crate::fs::Vfs::remove(PATH);

    match (result, removed) {
        (Ok(()), Ok(())) => {
            crate::serial_println!(
                "[swapctl]   swap file on ext4: on, read where ext4 put it, refused a write, a \
                 truncation, removal, renaming, a second swapon and an unmount, off, writable \
                 and removable again: OK"
            );
            crate::serial_println!("[swapctl] Swap-file self-test PASSED");
            Ok(())
        }
        (Err(what), _) => {
            crate::serial_println!("[swapctl]   FAIL: {}", what);
            Err(KernelError::InternalError)
        }
        (Ok(()), Err(e)) => {
            crate::serial_println!(
                "[swapctl]   FAIL: the file could not be removed after: {:?}",
                e
            );
            Err(KernelError::InternalError)
        }
    }
}
