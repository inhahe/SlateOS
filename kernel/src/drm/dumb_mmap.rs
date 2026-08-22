//! Fake mmap-offset allocator for DRM dumb buffers (`DRM_IOCTL_MODE_MAP_DUMB`).
//!
//! On Linux, a graphics client does not `mmap` a GEM/dumb buffer directly by
//! its handle.  Instead it asks the kernel for a *fake mmap offset*
//! (`DRM_IOCTL_MODE_MAP_DUMB` → `drm_mode_map_dumb.offset`), and then calls
//! `mmap(/dev/dri/cardN, …, offset)` with that token.  The DRM core's
//! `drm_vma_offset_manager` keeps a per-device map from "fake offset" → buffer
//! object; the `mmap` handler resolves the token back to the buffer and maps
//! its pages into the process.  The offset is purely a lookup key — it bears no
//! relation to any real file position.
//!
//! This module is the Slate OS analogue of `drm_vma_offset_manager`: a global
//! table mapping a 16 KiB-aligned fake offset to the `(device index, GEM
//! handle)` pair it stands for.  [`crate::syscall::linux`]'s `sys_mmap`
//! intercepts an `mmap` on a `HandleKind::DrmCard` fd, looks the offset up
//! here, and maps the GEM object's backing frames into the caller.
//!
//! ## Idempotency and lifetime
//!
//! [`offset_for`] is idempotent: a buffer that is mapped, then queried again,
//! gets the *same* offset (matching Linux, where `drm_vma_offset_add` is a
//! no-op once a node already has an offset).  [`forget`] drops a buffer's
//! offset when the dumb buffer is destroyed (`DRM_IOCTL_MODE_DESTROY_DUMB`), so
//! a later `MAP_DUMB` of a freshly-allocated handle with the same numeric value
//! gets a fresh offset rather than aliasing the dead one.
//!
//! The table only records the *token → buffer* association; it owns no
//! physical memory and takes no reference on the GEM object.  The buffer's
//! frame lifetime is governed by the GEM refcount (see
//! [`crate::syscall::linux`]'s dumb-buffer mmap path, which `ref_inc`s each
//! frame it maps so process teardown's refcounted `free_frame` balances the
//! reference rather than double-freeing the buffer).
//!
//! ## Two kinds of mappable object
//!
//! `DRM_IOCTL_VIRTGPU_MAP` needs exactly the same token → object → frames
//! machinery for virtio-gpu render resources, so those share this table via
//! [`Mappable::VirtgpuResource`] rather than getting a second offset space —
//! two token spaces would either have to be kept disjoint by hand or be
//! disambiguated by the caller, and both are ways to eventually map the wrong
//! buffer.  They are a *distinct* object kind rather than GEM objects for the
//! reason given on that variant.

use alloc::collections::BTreeMap;
use core::sync::atomic::{AtomicU64, Ordering};

use crate::sync::PreemptSpinMutex as Mutex;

use crate::mm::frame::FRAME_SIZE;

/// The object a fake offset stands for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mappable {
    /// A GEM buffer object on `device` — the dumb-buffer path
    /// (`DRM_IOCTL_MODE_MAP_DUMB`).
    Gem {
        /// DRM device index (in [`crate::drm`]'s registry).
        device: usize,
        /// GEM handle within that device.
        gem_handle: u32,
    },
    /// A virtio-gpu render-node resource (`DRM_IOCTL_VIRTGPU_MAP`).
    ///
    /// Deliberately *not* a GEM object.  A virtio 2D resource's guest backing
    /// must be exactly `width * 4` bytes per row, because
    /// `VIRTIO_GPU_CMD_TRANSFER_TO_HOST_2D` carries no stride and the host
    /// assumes that layout — whereas [`crate::drm::gem::GemObject::alloc_2d`]
    /// pads the pitch out to 64 bytes.  Making render resources GEM objects
    /// would therefore skew every image whose width is not a multiple of 16,
    /// and would do so silently.
    VirtgpuResource {
        /// DRM device index the resource belongs to.
        device: usize,
        /// Device-scoped virtio-gpu resource id.
        resource_id: u32,
    },
}

/// Global fake-offset → object table.  Leaf lock: no other lock is taken
/// while it is held.
static TABLE: Mutex<BTreeMap<u64, Mappable>> = Mutex::new(BTreeMap::new());

/// Base of the fake-offset token space.
///
/// Chosen well above any plausible real file offset and 16 KiB-aligned.  The
/// value is opaque to clients — they only ever round-trip it back through
/// `mmap` — so its only requirements are alignment and uniqueness.
const FAKE_OFFSET_BASE: u64 = 0x1_0000_0000;

/// Next fake offset to hand out (bump allocator, 16 KiB-aligned steps).
static NEXT_OFFSET: AtomicU64 = AtomicU64::new(FAKE_OFFSET_BASE);

/// Round a byte size up to a whole number of 16 KiB frames.
///
/// Returns `None` only on overflow, which cannot happen for any real buffer
/// size but is handled rather than panicked.
fn frames_round_up(size: u64) -> Option<u64> {
    let fs = FRAME_SIZE as u64;
    size.checked_add(fs.wrapping_sub(1))
        .map(|v| v & !(fs.wrapping_sub(1)))
}

/// Return the fake mmap offset for a mappable object, allocating one on first
/// use.
///
/// Idempotent: an object that already has an offset gets the same one back.
/// `size` is the object's byte size; the offset token space is advanced by the
/// frame-rounded size so distinct objects occupy distinct token ranges (purely
/// cosmetic — lookups key on the exact base offset).
///
/// Returns `None` only if the token space is exhausted or the size overflows
/// on rounding — neither occurs in practice, but both are handled without
/// panicking.
#[must_use]
pub fn offset_for(want: Mappable, size: u64) -> Option<u64> {
    let mut table = TABLE.lock();
    // Idempotent: reuse an existing offset for this object.
    if let Some((&off, _)) = table.iter().find(|(_, b)| **b == want) {
        return Some(off);
    }
    let step = frames_round_up(size)?.max(FRAME_SIZE as u64);
    // Reserve [off, off+step) in the token space.  `fetch_add` is atomic, so
    // concurrent callers never get the same base; we hold the table lock only
    // to keep the insert consistent with the reservation.
    let off = NEXT_OFFSET.fetch_add(step, Ordering::Relaxed);
    // Overflow / wrap guard: if the reservation wrapped, refuse (the `?`
    // short-circuits to `None`).
    off.checked_add(step)?;
    table.insert(off, want);
    Some(off)
}

/// Resolve a fake offset to the object it stands for.
///
/// Returns `None` if the offset was never handed out (or has been forgotten),
/// which the `mmap` path maps to `EINVAL` exactly as Linux does for an offset
/// with no matching `drm_vma_offset_node`.
#[must_use]
pub fn lookup(offset: u64) -> Option<Mappable> {
    TABLE.lock().get(&offset).copied()
}

/// Drop any fake offset registered for `want`.
///
/// Called when the object is destroyed so its token can't resolve to freed
/// memory, and so a recycled GEM handle or resource id gets a fresh offset
/// rather than inheriting the dead one.  An object with no registered offset
/// is silently ignored.
pub fn forget(want: Mappable) {
    let mut table = TABLE.lock();
    // Collect-then-remove: at most one offset maps to a given buffer, but scan
    // defensively in case a future path ever registers more than one.
    let stale: alloc::vec::Vec<u64> = table
        .iter()
        .filter(|(_, b)| **b == want)
        .map(|(&off, _)| off)
        .collect();
    for off in stale {
        table.remove(&off);
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Boot-time self-test of the fake-offset allocator.
///
/// Verifies allocation, 16 KiB alignment, idempotency, distinctness across
/// buffers, resolution, and forgetting.  Leaves the table as it found it
/// (modulo the monotonic offset counter, which is immaterial).
///
/// # Errors
///
/// Returns [`crate::error::KernelError::InternalError`] on the first failed
/// invariant.
pub fn self_test() -> crate::error::KernelResult<()> {
    use crate::error::KernelError;
    macro_rules! check {
        ($cond:expr, $msg:expr) => {
            if !($cond) {
                crate::serial_println!("[drm_dumb_mmap] SELF-TEST FAILED: {}", $msg);
                return Err(KernelError::InternalError);
            }
        };
    }

    // Use a device index unlikely to collide with a real entry; the table is
    // keyed by offset and only filtered by (device, handle), so any values
    // round-trip correctly regardless of whether the device exists.
    let dev = 7;
    let buf_a = Mappable::Gem {
        device: dev,
        gem_handle: 100,
    };
    let buf_b = Mappable::Gem {
        device: dev,
        gem_handle: 200,
    };
    // Same device, same numeric id, different *kind* — must not alias.
    let res_a = Mappable::VirtgpuResource {
        device: dev,
        resource_id: 100,
    };
    let off_a = offset_for(buf_a, 64 * 1024).ok_or(KernelError::InternalError)?;
    let off_b = offset_for(buf_b, 16 * 1024).ok_or(KernelError::InternalError)?;
    let off_r = offset_for(res_a, 16 * 1024).ok_or(KernelError::InternalError)?;

    check!(
        off_r != off_a && off_r != off_b,
        "a virtgpu resource never aliases a GEM handle with the same number"
    );
    check!(
        lookup(off_r) == Some(res_a),
        "virtgpu offset resolves to the resource"
    );

    check!(
        off_a.is_multiple_of(FRAME_SIZE as u64),
        "offset A is frame-aligned"
    );
    check!(
        off_b.is_multiple_of(FRAME_SIZE as u64),
        "offset B is frame-aligned"
    );
    check!(off_a != off_b, "distinct buffers get distinct offsets");

    // Idempotent: same buffer → same offset.
    let off_a2 = offset_for(buf_a, 64 * 1024).ok_or(KernelError::InternalError)?;
    check!(off_a2 == off_a, "offset_for is idempotent per buffer");

    // Resolution.
    check!(lookup(off_a) == Some(buf_a), "offset A resolves to buffer 100");
    check!(lookup(off_b) == Some(buf_b), "offset B resolves to buffer 200");
    check!(
        lookup(0xDEAD_0000).is_none(),
        "unknown offset resolves to None"
    );

    // Forget drops the mapping.
    forget(buf_a);
    check!(
        lookup(off_a).is_none(),
        "forgotten offset no longer resolves"
    );
    check!(lookup(off_b) == Some(buf_b), "forget is buffer-scoped");
    check!(
        lookup(off_r) == Some(res_a),
        "forgetting a GEM handle leaves the same-numbered resource alone"
    );

    // A re-mapped handle gets a *fresh* offset (not the forgotten one).
    let off_a3 = offset_for(buf_a, 64 * 1024).ok_or(KernelError::InternalError)?;
    check!(off_a3 != off_a, "re-mapped handle gets a fresh offset");

    // Clean up the test entries.
    forget(buf_a);
    forget(buf_b);
    forget(res_a);
    check!(lookup(off_a3).is_none(), "cleanup removed buffer 100");
    check!(lookup(off_b).is_none(), "cleanup removed buffer 200");
    check!(lookup(off_r).is_none(), "cleanup removed the resource");

    crate::serial_println!("[drm_dumb_mmap] fake-offset allocator self-test PASSED");
    Ok(())
}
