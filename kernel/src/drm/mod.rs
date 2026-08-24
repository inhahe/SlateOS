//! DRM/KMS (Direct Rendering Manager / Kernel Mode Setting) subsystem.
//!
//! Abstracts display hardware behind a clean driver interface, modeled
//! after Linux's DRM subsystem but simplified for our microkernel.
//!
//! ## Architecture
//!
//! The DRM subsystem provides:
//!
//! - **Connectors**: physical or virtual display outputs (HDMI, DP, virtio)
//! - **CRTCs**: scanout engines that read framebuffers and drive connectors
//! - **Planes**: layers composited by the CRTC (primary, cursor, overlay)
//! - **Encoders**: signal conversion between CRTCs and connectors
//! - **Framebuffers**: pixel buffers backed by GEM buffer objects
//! - **GEM objects**: GPU memory allocations (system RAM for now, VRAM later)
//! - **Atomic modesetting**: all state changes in one atomic commit
//!
//! ## Backends
//!
//! - `LimineBackend`: wraps the bootloader-provided framebuffer (always available)
//! - `VirtioGpuBackend`: wraps the virtio-gpu paravirtualized driver (QEMU/KVM)
//! - Future: `AmdGpuBackend`, `IntelBackend` for real hardware
//!
//! ## Design Decisions
//!
//! - **Enum dispatch, not dyn Trait**: the compositor calls DRM at display
//!   refresh rate (< 2ms budget at 4K/144Hz).  Enum dispatch avoids vtable
//!   indirection and branch misprediction.
//! - **Atomic-only**: no legacy per-object KMS API.  All state changes go
//!   through `atomic_commit()`.
//! - **GEM-style, not TTM**: simple per-driver buffer management.  VRAM
//!   migration (TTM's main feature) belongs in userspace drivers.
//!
//! ## References
//!
//! - Linux `drivers/gpu/drm/drm_*` — DRM core
//! - Linux `include/uapi/drm/drm.h`, `drm_mode.h` — userspace ABI
//! - Wayland protocol spec (for understanding compositor needs)

// The DRM subsystem is built out for completeness against the design spec
// (atomic modesetting API, EDID parsing, hotplug events, GEM buffer
// management, plane/encoder/connector enumeration).  Many helpers and
// fields are exposed for the userspace compositor / hardware backends to
// consume, but the compositor implementation hasn't wired up every API
// path yet.  Silence dead_code across the subsystem so legitimate API
// surface doesn't generate noise.
#![allow(dead_code)]

#[allow(dead_code)]
pub mod ati;
#[allow(dead_code)]
pub mod atomic;
#[allow(dead_code)]
pub mod card_fd;
#[allow(dead_code)]
pub mod connector;
#[allow(dead_code)]
pub mod crtc;
#[allow(dead_code)]
pub mod driver;
#[allow(dead_code)]
pub mod dumb_mmap;
#[allow(dead_code)]
pub mod edid;
#[allow(dead_code)]
pub mod encoder;
#[allow(dead_code)]
pub mod framebuffer;
#[allow(dead_code)]
pub mod gem;
#[allow(dead_code)]
pub mod hotplug;
#[allow(dead_code)]
pub mod mode;
#[allow(dead_code)]
pub mod plane;
#[allow(dead_code)]
pub mod property;
#[allow(dead_code)]
pub mod syscall;
#[allow(dead_code)]
pub mod uapi;
pub mod virtgpu_uapi;

extern crate alloc;

use alloc::boxed::Box;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};

use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};
use crate::serial_println;

use self::atomic::CursorState;
use self::connector::DrmConnector;
use self::crtc::DrmCrtc;
use self::encoder::DrmEncoder;
use self::framebuffer::DrmFramebuffer;
use self::gem::GemObject;
use self::mode::{DrmMode, PixelFormat};
use self::plane::DrmPlane;

// ---------------------------------------------------------------------------
// Object IDs
// ---------------------------------------------------------------------------

/// Opaque DRM object identifier, unique within a DRM device.
///
/// Every DRM object (connector, CRTC, plane, encoder, framebuffer, GEM
/// buffer) gets a unique ID allocated from a per-device monotonic counter.
/// Mirrors Linux `drm_mode_object.id`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DrmObjectId(u32);

impl DrmObjectId {
    /// Create an object ID from a raw value (for driver backends).
    #[must_use]
    pub const fn new(raw: u32) -> Self {
        Self(raw)
    }

    /// Get the raw numeric value.
    #[must_use]
    pub const fn raw(self) -> u32 {
        self.0
    }
}

impl core::fmt::Display for DrmObjectId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.0)
    }
}

// ---------------------------------------------------------------------------
// DRM Device
// ---------------------------------------------------------------------------

/// Maximum number of registered DRM devices.
const MAX_DEVICES: usize = 8;

/// A DRM device — one GPU or display controller.
///
/// Holds the driver backend and all DRM objects (connectors, CRTCs,
/// planes, encoders, framebuffers, GEM buffers).
pub struct DrmDevice {
    /// Device index (0, 1, 2, ...).
    pub index: usize,
    /// Human-readable name.
    pub name: &'static str,
    /// Next object ID to allocate.
    next_object_id: AtomicU32,
    /// The driver backend (enum dispatch for hot path).
    backend: DrmBackend,
    /// Known connectors.
    connectors: Vec<DrmConnector>,
    /// Known CRTCs.
    crtcs: Vec<DrmCrtc>,
    /// Known planes.
    planes: Vec<DrmPlane>,
    /// Known encoders.
    encoders: Vec<DrmEncoder>,
    /// Active framebuffer objects.
    framebuffers: Vec<DrmFramebuffer>,
    /// Active GEM buffer objects.
    gem_objects: Vec<GemObject>,
    /// Per-CRTC cursor state.
    ///
    /// Indexed in parallel with `crtcs` — `cursor_states[i]` is the
    /// cursor for `crtcs[i]`.  Populated in `enumerate()` alongside
    /// the CRTC list.
    cursor_states: Vec<CursorState>,
}

/// Backend enum — avoids `dyn Trait` overhead on the hot path.
///
/// See module-level docs for why enum dispatch is preferred here.
pub enum DrmBackend {
    /// Bootloader-provided framebuffer (always available).
    Limine(driver::LimineBackend),
    /// virtio-gpu paravirtualized driver (QEMU/KVM).
    VirtioGpu(driver::VirtioGpuBackend),
    /// Legacy ATI/AMD display block — R100 and Rage 128.
    ///
    /// The only variant whose buffers are not in system RAM: its GEM objects
    /// live in the card's own video memory, so its `gem_destroy` must not go
    /// anywhere near the buddy allocator. See [`ati::backend::AtiBackend`].
    Ati(ati::backend::AtiBackend),
}

impl DrmDevice {
    /// Create a new DRM device with the given backend.
    fn new(index: usize, name: &'static str, backend: DrmBackend) -> Self {
        Self {
            index,
            name,
            // Start IDs at 1 (0 is reserved / means "none").
            next_object_id: AtomicU32::new(1),
            backend,
            connectors: Vec::new(),
            crtcs: Vec::new(),
            planes: Vec::new(),
            encoders: Vec::new(),
            framebuffers: Vec::new(),
            gem_objects: Vec::new(),
            cursor_states: Vec::new(),
        }
    }

    /// Allocate a fresh object ID.
    pub fn alloc_id(&self) -> DrmObjectId {
        DrmObjectId(self.next_object_id.fetch_add(1, Ordering::Relaxed))
    }

    /// Get the driver backend name.
    #[must_use]
    pub fn driver_name(&self) -> &'static str {
        match &self.backend {
            DrmBackend::Limine(b) => b.name(),
            DrmBackend::VirtioGpu(b) => b.name(),
            DrmBackend::Ati(b) => b.name(),
        }
    }

    /// Enumerate display hardware and populate object lists.
    ///
    /// Called once after device creation.  The backend queries the
    /// hardware for available connectors, CRTCs, planes, and encoders.
    pub fn enumerate(&mut self) -> KernelResult<()> {
        // Extract a closure that captures only the ID allocator, not all
        // of self — this avoids the borrow conflict with &mut self.backend.
        let id_alloc = &self.next_object_id;
        let alloc_fn = || DrmObjectId(id_alloc.fetch_add(1, Ordering::Relaxed));

        let (connectors, crtcs, planes, encoders) = match &mut self.backend {
            DrmBackend::Limine(b) => b.enumerate(&alloc_fn)?,
            DrmBackend::VirtioGpu(b) => b.enumerate(&alloc_fn)?,
            DrmBackend::Ati(b) => b.enumerate(&alloc_fn)?,
        };
        // One cursor state per CRTC.
        let cursor_count = crtcs.len();
        self.connectors = connectors;
        self.crtcs = crtcs;
        self.planes = planes;
        self.encoders = encoders;
        self.cursor_states = (0..cursor_count).map(|_| CursorState::new()).collect();
        Ok(())
    }

    // --- Accessors ---

    /// All connectors.
    #[must_use]
    pub fn connectors(&self) -> &[DrmConnector] {
        &self.connectors
    }

    /// All CRTCs.
    #[must_use]
    pub fn crtcs(&self) -> &[DrmCrtc] {
        &self.crtcs
    }

    /// All planes.
    #[must_use]
    pub fn planes(&self) -> &[DrmPlane] {
        &self.planes
    }

    /// All encoders.
    #[must_use]
    pub fn encoders(&self) -> &[DrmEncoder] {
        &self.encoders
    }

    /// All active framebuffers.
    #[must_use]
    pub fn framebuffers(&self) -> &[DrmFramebuffer] {
        &self.framebuffers
    }

    // --- GEM operations ---

    /// Allocate a GPU buffer object.
    pub fn gem_create(
        &mut self,
        width: u32,
        height: u32,
        format: PixelFormat,
    ) -> KernelResult<u32> {
        let id_alloc = &self.next_object_id;
        let alloc_fn = || DrmObjectId(id_alloc.fetch_add(1, Ordering::Relaxed));

        let gem = match &mut self.backend {
            DrmBackend::Limine(b) => b.gem_create(&alloc_fn, width, height, format)?,
            DrmBackend::VirtioGpu(b) => b.gem_create(&alloc_fn, width, height, format)?,
            DrmBackend::Ati(b) => b.gem_create(&alloc_fn, width, height, format)?,
        };
        let handle = gem.handle;
        self.gem_objects.push(gem);
        Ok(handle)
    }

    /// Free a GPU buffer object.
    pub fn gem_destroy(&mut self, handle: u32) -> KernelResult<()> {
        let idx = self
            .gem_objects
            .iter()
            .position(|g| g.handle == handle)
            .ok_or(KernelError::NotFound)?;
        let gem = self.gem_objects.remove(idx);
        match &mut self.backend {
            DrmBackend::Limine(b) => b.gem_destroy(gem)?,
            DrmBackend::VirtioGpu(b) => b.gem_destroy(gem)?,
            DrmBackend::Ati(b) => b.gem_destroy(gem)?,
        }
        Ok(())
    }

    /// Get a kernel-virtual pointer to a GEM object's backing memory.
    pub fn gem_mmap(&self, handle: u32) -> KernelResult<*mut u8> {
        let gem = self
            .gem_objects
            .iter()
            .find(|g| g.handle == handle)
            .ok_or(KernelError::NotFound)?;
        match &self.backend {
            DrmBackend::Limine(b) => b.gem_mmap(gem),
            DrmBackend::VirtioGpu(b) => b.gem_mmap(gem),
            DrmBackend::Ati(b) => b.gem_mmap(gem),
        }
    }

    // --- Framebuffer operations ---

    /// Create a framebuffer object from a GEM handle.
    pub fn fb_create(
        &mut self,
        gem_handle: u32,
        width: u32,
        height: u32,
        pitch: u32,
        format: PixelFormat,
    ) -> KernelResult<DrmObjectId> {
        // Verify the GEM handle exists.
        if !self.gem_objects.iter().any(|g| g.handle == gem_handle) {
            return Err(KernelError::NotFound);
        }
        let id = self.alloc_id();
        let fb = DrmFramebuffer {
            id,
            gem_handle,
            width,
            height,
            pitch,
            format,
            offset: 0,
        };
        self.framebuffers.push(fb);
        Ok(id)
    }

    /// Destroy a framebuffer object.
    ///
    /// Any plane still naming it is unbound. Linux's `drm_framebuffer_remove`
    /// goes further and disables the CRTC as well; this does not, because the
    /// display engine has *not* stopped reading that memory and saying it has
    /// would be the same class of lie this call is here to avoid. Unbinding the
    /// plane is the part that is true: the framebuffer object is gone, so a
    /// `GETPLANE` reporting its id would be reporting an id that resolves to
    /// nothing — and worse, one a later `fb_create` can reuse, at which point
    /// the plane would appear bound to an unrelated buffer.
    ///
    /// # Errors
    ///
    /// `NotFound` if no framebuffer has this id.
    pub fn fb_destroy(&mut self, fb_id: DrmObjectId) -> KernelResult<()> {
        let idx = self
            .framebuffers
            .iter()
            .position(|f| f.id == fb_id)
            .ok_or(KernelError::NotFound)?;
        self.framebuffers.remove(idx);
        for p in &mut self.planes {
            if p.fb == Some(fb_id) {
                p.fb = None;
            }
        }
        Ok(())
    }

    /// Look up a framebuffer by ID.
    #[must_use]
    pub fn fb_get(&self, fb_id: DrmObjectId) -> Option<&DrmFramebuffer> {
        self.framebuffers.iter().find(|f| f.id == fb_id)
    }

    // --- Display operations ---

    /// Configure a CRTC: program `mode`, drive it out of `connectors`, and
    /// scan `fb_id` out of its primary plane — or, with `mode: None`, turn it
    /// off.
    ///
    /// This is the kernel side of `DRM_IOCTL_MODE_SETCRTC`, and it is the only
    /// path that changes a display timing. Until 2026-08-21 there was none, and
    /// [`Self::page_flip`] carried an implicit mode-set on one of the three
    /// backends; see `design-decisions.md` §270 for why that was removed rather
    /// than generalised.
    ///
    /// ## What is validated, and why each check is here rather than in a driver
    ///
    /// * **The mode must be one the connector advertises.** Not "close to" one:
    ///   a CRTC programmed with a timing that merely resembles what the monitor
    ///   expects produces no picture, and the symptom is indistinguishable from
    ///   a hang. Checking here means a backend cannot be reached with a mode its
    ///   own enumeration never offered.
    /// * **The stored mode is the kernel's copy, never the caller's.**
    ///   `DrmModeCrtc` round-trips through userspace, and
    ///   `drm_mode_to_uapi` writes zeros for `hsync_*`, `vsync_*`, `hskew`,
    ///   `vscan` and `flags` — so a client that reads a mode out of `GETCONNECTOR`
    ///   and hands it straight back is *not* returning what it was given. Storing
    ///   the caller's struct would silently zero the blanking intervals of every
    ///   mode ever set.
    /// * **The connector must actually be able to drive this CRTC**, through one
    ///   of its encoders' `possible_crtcs` masks. Otherwise a client can wire a
    ///   panel to a CRTC that is not routed to it and get a black screen with a
    ///   successful return code.
    /// * **The framebuffer must cover the mode at the requested origin.** Linux
    ///   makes exactly this check (`Invalid fb size`), and it is what stops the
    ///   display engine reading past the end of the buffer.
    ///
    /// ## Errors
    ///
    /// * `NotFound` — no such CRTC, framebuffer, GEM object or connector.
    /// * `InvalidArgument` — a disable that also names a framebuffer or
    ///   connectors; an enable that names neither; a mode no listed connector
    ///   advertises; a connector that cannot reach this CRTC; a framebuffer too
    ///   small for the mode.
    /// * Whatever the backend's `set_mode`/`disable_crtc` returns —
    ///   `NotSupported` from a backend that cannot retime at all, `NotFound`
    ///   from one that has no timing for the requested size.
    ///
    /// The object model is updated **only after** the backend reports success,
    /// so a failed mode-set leaves `GETCRTC` reporting what is genuinely still
    /// being scanned out rather than what was asked for.
    pub fn set_crtc(
        &mut self,
        crtc_id: DrmObjectId,
        fb_id: Option<DrmObjectId>,
        x: u32,
        y: u32,
        connectors: &[DrmObjectId],
        mode: Option<&DrmMode>,
    ) -> KernelResult<()> {
        let crtc_idx = self
            .crtcs
            .iter()
            .position(|c| c.id == crtc_id)
            .ok_or(KernelError::NotFound)?;
        let crtc_index_bit = self
            .crtcs
            .get(crtc_idx)
            .map_or(0u32, |c| 1u32.checked_shl(c.index).unwrap_or(0));
        let primary_plane = self
            .crtcs
            .get(crtc_idx)
            .ok_or(KernelError::NotFound)?
            .primary_plane;

        let Some(want) = mode else {
            // Disable. Linux requires `fb_id == 0` and `count_connectors == 0`
            // to accompany `mode_valid == 0`; a request that turns the CRTC off
            // *and* names a framebuffer is self-contradictory, and guessing
            // which half was meant is how a compositor shutting down ends up
            // with a CRTC still scanning a buffer it has just freed.
            if fb_id.is_some() || !connectors.is_empty() {
                return Err(KernelError::InvalidArgument);
            }
            match &mut self.backend {
                DrmBackend::Limine(b) => b.disable_crtc(crtc_id),
                DrmBackend::VirtioGpu(b) => b.disable_crtc(crtc_id),
                DrmBackend::Ati(b) => b.disable_crtc(crtc_id),
            }?;
            if let Some(c) = self.crtcs.get_mut(crtc_idx) {
                c.active = false;
                c.mode = None;
            }
            if let Some(p) = self.planes.iter_mut().find(|p| p.id == primary_plane) {
                p.fb = None;
            }
            self.unbind_crtc_routing(crtc_id);
            return Ok(());
        };

        // Enable. A mode with no framebuffer would leave the CRTC timed and
        // fetching from wherever it last pointed, and a mode with no connector
        // drives nothing — both are `EINVAL` in Linux and both are refused here.
        let fb_id = fb_id.ok_or(KernelError::InvalidArgument)?;
        if connectors.is_empty() {
            return Err(KernelError::InvalidArgument);
        }

        // Resolve every connector before anything is programmed, and take the
        // kernel's own copy of the matched mode from the first of them.
        //
        // The encoder each connector will be routed through is chosen here too,
        // but not *recorded* until the backend has succeeded — the routing is
        // part of what `GETCONNECTOR` and `GETENCODER` report, and a failed
        // mode-set must not leave them describing a path no signal takes.
        let mut kernel_mode: Option<DrmMode> = None;
        let mut routing: Vec<(DrmObjectId, DrmObjectId)> = Vec::new();
        for &conn_id in connectors {
            let conn = self
                .connectors
                .iter()
                .find(|c| c.id == conn_id)
                .ok_or(KernelError::NotFound)?;
            // Routable to this CRTC through at least one of its encoders? Take
            // the first that is: the connectors here each have one hardware
            // path to a given CRTC, so "first" and "only" coincide, and if that
            // ever stops being true the choice becomes a real one rather than a
            // tie-break — at which point it belongs to the caller, via the
            // atomic path's explicit connector→encoder binding.
            let enc_id = conn
                .possible_encoders
                .iter()
                .copied()
                .find(|eid| {
                    self.encoders
                        .iter()
                        .any(|e| e.id == *eid && (e.possible_crtcs & crtc_index_bit) != 0)
                })
                .ok_or(KernelError::InvalidArgument)?;
            routing.push((conn_id, enc_id));
            // Matched on size, and on refresh only when the caller stated one.
            // Linux's own `drm_mode_equal` ignores `vrefresh` entirely — it is
            // a derived convenience field, not part of the timing — and a
            // client that built its request from scratch rather than from
            // `GETCONNECTOR` will legitimately leave it 0. Treating 0 as "any"
            // accepts that client; treating a *stated* refresh as binding stops
            // a request for 60 Hz being silently served at 75.
            let matched = conn
                .modes
                .iter()
                .find(|m| {
                    m.hdisplay == want.hdisplay
                        && m.vdisplay == want.vdisplay
                        && (want.vrefresh == 0 || m.vrefresh == want.vrefresh)
                })
                .ok_or(KernelError::InvalidArgument)?;
            if kernel_mode.is_none() {
                kernel_mode = Some(*matched);
            }
        }
        let kernel_mode = kernel_mode.ok_or(KernelError::InvalidArgument)?;

        let fb = self
            .framebuffers
            .iter()
            .find(|f| f.id == fb_id)
            .ok_or(KernelError::NotFound)?;
        // `x`/`y` are the origin *within* the framebuffer that lands at the top
        // left of the display, so the buffer must extend a full mode past them.
        // Checked arithmetic: a caller-supplied origin near `u32::MAX` would
        // otherwise wrap and turn an absurd request into a passing one.
        let need_w = kernel_mode
            .hdisplay
            .checked_add(x)
            .ok_or(KernelError::InvalidArgument)?;
        let need_h = kernel_mode
            .vdisplay
            .checked_add(y)
            .ok_or(KernelError::InvalidArgument)?;
        if fb.width < need_w || fb.height < need_h {
            return Err(KernelError::InvalidArgument);
        }
        let gem = self
            .gem_objects
            .iter()
            .find(|g| g.handle == fb.gem_handle)
            .ok_or(KernelError::NotFound)?;

        match &mut self.backend {
            DrmBackend::Limine(b) => b.set_mode(crtc_id, &kernel_mode, fb, gem),
            DrmBackend::VirtioGpu(b) => b.set_mode(crtc_id, &kernel_mode, fb, gem),
            DrmBackend::Ati(b) => b.set_mode(crtc_id, &kernel_mode, fb, gem),
        }?;

        if let Some(c) = self.crtcs.get_mut(crtc_idx) {
            c.active = true;
            c.mode = Some(kernel_mode);
        }
        // Drop whatever used to be routed here before recording the new set, so
        // a `SETCRTC` that re-points a CRTC at a different connector does not
        // leave the old one still claiming to drive it.
        self.unbind_crtc_routing(crtc_id);
        for (conn_id, enc_id) in routing {
            if let Some(c) = self.connectors.iter_mut().find(|c| c.id == conn_id) {
                c.current_encoder = Some(enc_id);
            }
            if let Some(e) = self.encoders.iter_mut().find(|e| e.id == enc_id) {
                e.crtc = Some(crtc_id);
            }
        }
        if let Some(p) = self.planes.iter_mut().find(|p| p.id == primary_plane) {
            p.fb = Some(fb_id);
            p.crtc = Some(crtc_id);
            p.src_x = x;
            p.src_y = y;
            p.src_w = kernel_mode.hdisplay;
            p.src_h = kernel_mode.vdisplay;
            p.dst_x = 0;
            p.dst_y = 0;
            p.dst_w = kernel_mode.hdisplay;
            p.dst_h = kernel_mode.vdisplay;
        }
        Ok(())
    }

    /// Detach every encoder currently routed to `crtc_id`, and every connector
    /// feeding one of them.
    ///
    /// This is the bookkeeping half of turning a CRTC off or re-pointing it.
    /// It exists because `GETCONNECTOR`'s `encoder_id` and `GETENCODER`'s
    /// `crtc_id` are how a client discovers what is driving what: an encoder
    /// left naming a CRTC that no longer scans it out is the same category of
    /// falsehood as a `crtc.mode` that was never updated, and it misleads in a
    /// worse direction — a client reading it concludes the display is already
    /// configured and skips the `SETCRTC` that would have lit it.
    ///
    /// Connectors are cleared by *following the encoders*, not by matching on
    /// the connector's own list, because `possible_encoders` says what could be
    /// routed and `current_encoder` says what is. Only the second is a claim
    /// about the present.
    fn unbind_crtc_routing(&mut self, crtc_id: DrmObjectId) {
        let mut detached: Vec<DrmObjectId> = Vec::new();
        for e in &mut self.encoders {
            if e.crtc == Some(crtc_id) {
                e.crtc = None;
                detached.push(e.id);
            }
        }
        for c in &mut self.connectors {
            if c.current_encoder.is_some_and(|eid| detached.contains(&eid)) {
                c.current_encoder = None;
            }
        }
    }

    /// Page flip: swap the framebuffer on a CRTC's primary plane, without
    /// changing the mode.
    ///
    /// The framebuffer must be **exactly** the size of the CRTC's programmed
    /// mode, and the CRTC must have one. Both refusals were added on
    /// 2026-08-21 at lane C's request; before that this checked only that the
    /// three objects existed, and the three backends each invented a different
    /// meaning for a mismatch — ATI silently performed a full mode-set,
    /// virtio-gpu silently cropped to the top-left, and Limine silently cropped
    /// too, while `GETCRTC` afterwards reported the boot mode on all three
    /// because nothing ever wrote `crtc.mode`. The same sequence of ioctls
    /// therefore changed the resolution on one machine and cropped the image on
    /// another, with no way for the client to tell which had happened. Use
    /// [`Self::set_crtc`] to change size. See `design-decisions.md` §270.
    ///
    /// # Errors
    ///
    /// * `NotFound` — no such CRTC, framebuffer, or backing GEM object.
    /// * `InvalidArgument` — the CRTC is in no mode (never configured, or
    ///   disabled), or `fb` is not the size of the mode it is in.
    /// * Whatever the backend's `page_flip` returns.
    pub fn page_flip(&mut self, crtc_id: DrmObjectId, fb_id: DrmObjectId) -> KernelResult<()> {
        let crtc = self
            .crtcs
            .iter()
            .find(|c| c.id == crtc_id)
            .ok_or(KernelError::NotFound)?;
        // A CRTC with no mode is not scanning anything out, so there is no
        // "swap the front buffer" for this call to mean.
        let mode = crtc.mode.ok_or(KernelError::InvalidArgument)?;
        let fb = self
            .framebuffers
            .iter()
            .find(|f| f.id == fb_id)
            .ok_or(KernelError::NotFound)?;
        if fb.width != mode.hdisplay || fb.height != mode.vdisplay {
            return Err(KernelError::InvalidArgument);
        }
        let gem = self
            .gem_objects
            .iter()
            .find(|g| g.handle == fb.gem_handle)
            .ok_or(KernelError::NotFound)?;

        let primary_plane = crtc.primary_plane;
        match &mut self.backend {
            DrmBackend::Limine(b) => b.page_flip(crtc_id, fb, gem),
            DrmBackend::VirtioGpu(b) => b.page_flip(crtc_id, fb, gem),
            DrmBackend::Ati(b) => b.page_flip(crtc_id, fb, gem),
        }?;
        // Record what is now on screen. Without this the primary plane reports
        // `fb_id = 0` forever — only `atomic.rs` ever wrote this field — so a
        // client that asks which buffer is being scanned out is told "none"
        // while looking at it.
        if let Some(p) = self.planes.iter_mut().find(|p| p.id == primary_plane) {
            p.fb = Some(fb_id);
        }
        Ok(())
    }

    /// Flush a dirty region of a framebuffer to the display.
    ///
    /// For paravirtualized GPUs (virtio-gpu), this triggers a host-side
    /// transfer.  For direct-scanout hardware, this is typically a no-op.
    pub fn flush_region(
        &mut self,
        fb_id: DrmObjectId,
        x: u32,
        y: u32,
        w: u32,
        h: u32,
    ) -> KernelResult<()> {
        let fb = self
            .framebuffers
            .iter()
            .find(|f| f.id == fb_id)
            .ok_or(KernelError::NotFound)?;
        let gem = self
            .gem_objects
            .iter()
            .find(|g| g.handle == fb.gem_handle)
            .ok_or(KernelError::NotFound)?;

        match &mut self.backend {
            DrmBackend::Limine(b) => b.flush_region(fb, gem, x, y, w, h),
            DrmBackend::VirtioGpu(b) => b.flush_region(fb, gem, x, y, w, h),
            DrmBackend::Ati(b) => b.flush_region(fb, gem, x, y, w, h),
        }
    }

    /// The mode a connector should be driven at absent any client preference:
    /// the one it flags `PREFERRED`, or its first if none is flagged.
    ///
    /// "First" is not a sensible default on its own. The ATI backend orders its
    /// list by area and flags the *last* entry, so taking the first would drive
    /// a 1920x1080-capable card at 640x480 — which is why every caller that
    /// wants "the right mode" must go through this rather than `modes.first()`.
    fn preferred_mode(conn: &DrmConnector) -> Option<&DrmMode> {
        conn.modes
            .iter()
            .find(|m| m.flags == mode::DrmModeFlags::PREFERRED)
            .or_else(|| conn.modes.first())
    }

    /// Get the current display dimensions (width, height) of the primary output.
    ///
    /// The CRTC's programmed mode when it has one, so a caller that allocates a
    /// buffer this size can page-flip it — [`Self::page_flip`] requires an exact
    /// match. Falls back to the first connector's preferred mode when no CRTC is
    /// configured yet, which is the size [`Self::ensure_crtc_configured`] would
    /// program.
    #[must_use]
    pub fn display_size(&self) -> (u32, u32) {
        if let Some(m) = self.crtcs.first().and_then(|c| c.mode) {
            return (m.hdisplay, m.vdisplay);
        }
        self.connectors
            .first()
            .and_then(Self::preferred_mode)
            .map_or((0, 0), |m| (m.hdisplay, m.vdisplay))
    }

    /// Bring an unconfigured CRTC up at its connector's preferred mode, scanning
    /// out `fb_id`.
    ///
    /// A no-op if the CRTC already has a mode — this is "make sure it is on",
    /// not "reconfigure it", so it is safe to call before every flip and safe to
    /// call from more than one buffer's constructor.
    ///
    /// This exists because [`Self::page_flip`] stopped carrying an implicit
    /// mode-set on 2026-08-21 (see `design-decisions.md` §270). Two of the three
    /// backends enumerate a CRTC that is already live — the bootloader or the
    /// hypervisor timed it — but the ATI one cannot: it enumerates
    /// `active: false, mode: None`, because claiming otherwise would tell a
    /// compositor a flip is all that is needed when the CRTC has never been
    /// timed at all. Something has to do the first mode-set, and in-kernel that
    /// something is this.
    ///
    /// # Errors
    ///
    /// `NotFound` if the CRTC, or any connector routed to it, does not exist;
    /// otherwise propagates [`Self::set_crtc`].
    pub fn ensure_crtc_configured(
        &mut self,
        crtc_id: DrmObjectId,
        fb_id: DrmObjectId,
    ) -> KernelResult<()> {
        let crtc = self
            .crtcs
            .iter()
            .find(|c| c.id == crtc_id)
            .ok_or(KernelError::NotFound)?;
        if crtc.mode.is_some() {
            return Ok(());
        }
        let crtc_bit = 1u32.checked_shl(crtc.index).unwrap_or(0);
        // The first connector that can actually be routed here. Picking one that
        // cannot would be refused by `set_crtc`, correctly but uselessly.
        let (conn_id, want) = self
            .connectors
            .iter()
            .filter(|c| {
                c.possible_encoders.iter().any(|eid| {
                    self.encoders
                        .iter()
                        .any(|e| e.id == *eid && (e.possible_crtcs & crtc_bit) != 0)
                })
            })
            .find_map(|c| Self::preferred_mode(c).map(|m| (c.id, *m)))
            .ok_or(KernelError::NotFound)?;
        self.set_crtc(crtc_id, Some(fb_id), 0, 0, &[conn_id], Some(&want))
    }

    /// Return the HHDM-mapped virtual addresses of a GEM object's backing frames.
    ///
    /// This allows callers to hold the addresses past the DRM lock scope
    /// and perform direct pixel writes without holding any DRM lock.
    /// Addresses remain valid as long as the GEM object is not destroyed.
    ///
    /// # Errors
    ///
    /// `NotFound` if the handle is unknown; `NotSupported` if the object lives
    /// in a card's video memory, which the HHDM does not cover — ask
    /// [`Self::gem_mmap`] instead, which routes to the driver holding the
    /// aperture.
    pub fn gem_frame_addrs(&self, handle: u32) -> KernelResult<Vec<u64>> {
        use crate::mm::page_table;

        let gem = self
            .gem_objects
            .iter()
            .find(|g| g.handle == handle)
            .ok_or(KernelError::NotFound)?;
        let hhdm = page_table::hhdm().ok_or(KernelError::NotSupported)?;
        // A frame address the HHDM cannot reach would be a physical address
        // above the direct map, which on this kernel means the frame allocator
        // handed out something outside RAM. Reported rather than wrapped: a
        // wrapped address is a pointer into low memory that writes will corrupt.
        gem.ram_frames()?
            .iter()
            .map(|pf| {
                pf.addr()
                    .checked_add(hhdm)
                    .ok_or(KernelError::InvalidAddress)
            })
            .collect()
    }

    /// Get the pitch (bytes per row) of a GEM object.
    pub fn gem_pitch(&self, handle: u32) -> KernelResult<u32> {
        let gem = self
            .gem_objects
            .iter()
            .find(|g| g.handle == handle)
            .ok_or(KernelError::NotFound)?;
        Ok(gem.pitch)
    }

    /// Return the *physical* base addresses of a GEM object's backing frames,
    /// in scanout order.
    ///
    /// Unlike [`Self::gem_frame_addrs`] (which returns HHDM-mapped *virtual*
    /// addresses for in-kernel pixel writes), this returns raw physical frame
    /// addresses so the Linux `mmap` shim can reconstruct [`crate::mm::frame::
    /// PhysFrame`]s and map the buffer into a user process.  Each address is
    /// 16 KiB-frame-aligned.  Addresses remain valid as long as the GEM object
    /// is not destroyed.
    ///
    /// # Errors
    ///
    /// `NotFound` if the handle is unknown; `NotSupported` if the object lives
    /// in video memory. Its bytes do have physical addresses — they are behind
    /// a PCI BAR — but handing them to the `mmap` shim would map device memory
    /// into a process with the write-back caching an ordinary page gets, and
    /// pixels written into a cache line are pixels the scanout engine never
    /// sees.
    pub fn gem_phys_addrs(&self, handle: u32) -> KernelResult<Vec<u64>> {
        let gem = self
            .gem_objects
            .iter()
            .find(|g| g.handle == handle)
            .ok_or(KernelError::NotFound)?;
        Ok(gem.ram_frames()?.iter().map(|pf| pf.addr()).collect())
    }

    /// Get the total byte size of a GEM object's allocation.
    pub fn gem_size(&self, handle: u32) -> KernelResult<usize> {
        let gem = self
            .gem_objects
            .iter()
            .find(|g| g.handle == handle)
            .ok_or(KernelError::NotFound)?;
        Ok(gem.size)
    }

    /// Look up the first CRTC's object ID.
    #[must_use]
    pub fn first_crtc_id(&self) -> Option<DrmObjectId> {
        self.crtcs.first().map(|c| c.id)
    }

    // --- Mutable accessors (for atomic commit) ---

    /// Mutable reference to a CRTC by ID.
    pub fn crtc_mut(&mut self, id: DrmObjectId) -> Option<&mut DrmCrtc> {
        self.crtcs.iter_mut().find(|c| c.id == id)
    }

    /// Mutable reference to a plane by ID.
    pub fn plane_mut(&mut self, id: DrmObjectId) -> Option<&mut DrmPlane> {
        self.planes.iter_mut().find(|p| p.id == id)
    }

    /// Mutable reference to a connector by ID.
    pub fn connector_mut(&mut self, id: DrmObjectId) -> Option<&mut DrmConnector> {
        self.connectors.iter_mut().find(|c| c.id == id)
    }

    /// Mutable reference to an encoder by ID.
    pub fn encoder_mut(&mut self, id: DrmObjectId) -> Option<&mut DrmEncoder> {
        self.encoders.iter_mut().find(|e| e.id == id)
    }

    // --- Cursor operations ---

    /// Set the cursor image for a CRTC.
    ///
    /// `gem_handle` is a GEM buffer containing the ARGB cursor pixels
    /// (typically 64×64).  Pass `gem_handle = 0` to hide the cursor.
    ///
    /// Cursor updates are separate from atomic commit because cursor
    /// moves happen at mouse input frequency (1000 Hz+), far too fast
    /// for the atomic commit path.
    pub fn cursor_set(
        &mut self,
        crtc_id: DrmObjectId,
        gem_handle: u32,
        width: u32,
        height: u32,
        hot_x: u32,
        hot_y: u32,
    ) -> KernelResult<()> {
        let crtc_idx = self
            .crtcs
            .iter()
            .position(|c| c.id == crtc_id)
            .ok_or(KernelError::NotFound)?;

        // Validate GEM handle if non-zero.
        if gem_handle != 0 && !self.gem_objects.iter().any(|g| g.handle == gem_handle) {
            return Err(KernelError::NotFound);
        }

        let cs = self
            .cursor_states
            .get_mut(crtc_idx)
            .ok_or(KernelError::NotFound)?;
        cs.gem_handle = gem_handle;
        cs.width = width;
        cs.height = height;
        cs.hot_x = hot_x;
        cs.hot_y = hot_y;
        cs.visible = gem_handle != 0;

        Ok(())
    }

    /// Move the cursor position for a CRTC.
    ///
    /// This is the hottest path in the cursor subsystem — called on
    /// every mouse movement event.  No locks beyond the device lock.
    pub fn cursor_move(&mut self, crtc_id: DrmObjectId, x: i32, y: i32) -> KernelResult<()> {
        let crtc_idx = self
            .crtcs
            .iter()
            .position(|c| c.id == crtc_id)
            .ok_or(KernelError::NotFound)?;

        let cs = self
            .cursor_states
            .get_mut(crtc_idx)
            .ok_or(KernelError::NotFound)?;
        cs.x = x;
        cs.y = y;

        Ok(())
    }

    /// Get the cursor state for a CRTC.
    #[must_use]
    pub fn cursor_state(&self, crtc_id: DrmObjectId) -> Option<&CursorState> {
        let idx = self.crtcs.iter().position(|c| c.id == crtc_id)?;
        self.cursor_states.get(idx)
    }
}

// ---------------------------------------------------------------------------
// Global device registry
// ---------------------------------------------------------------------------

/// Global registry of DRM devices.
static DEVICES: Mutex<DeviceRegistry> = Mutex::new(DeviceRegistry::new());

struct DeviceRegistry {
    devices: [Option<Box<DrmDevice>>; MAX_DEVICES],
    count: usize,
    /// Index of the primary display device.
    primary: usize,
}

impl DeviceRegistry {
    const fn new() -> Self {
        Self {
            devices: [const { None }; MAX_DEVICES],
            count: 0,
            primary: 0,
        }
    }
}

/// Register a new DRM device.
///
/// Returns the device index on success.
pub fn register_device(name: &'static str, backend: DrmBackend) -> KernelResult<usize> {
    let mut reg = DEVICES.lock();
    if reg.count >= MAX_DEVICES {
        return Err(KernelError::OutOfMemory);
    }
    let index = reg.count;
    let mut device = DrmDevice::new(index, name, backend);
    device.enumerate()?;
    serial_println!(
        "[drm] Registered device {} ({}, {} connectors, {} CRTCs)",
        index,
        device.driver_name(),
        device.connectors().len(),
        device.crtcs().len(),
    );
    reg.devices[index] = Some(Box::new(device));
    reg.count = reg.count.saturating_add(1);
    Ok(index)
}

/// Run a closure with a reference to a DRM device.
pub fn with_device<F, R>(index: usize, f: F) -> KernelResult<R>
where
    F: FnOnce(&DrmDevice) -> KernelResult<R>,
{
    let reg = DEVICES.lock();
    let device = reg
        .devices
        .get(index)
        .and_then(|d| d.as_ref())
        .ok_or(KernelError::NotFound)?;
    f(device)
}

/// Run a closure with a mutable reference to a DRM device.
pub fn with_device_mut<F, R>(index: usize, f: F) -> KernelResult<R>
where
    F: FnOnce(&mut DrmDevice) -> KernelResult<R>,
{
    let mut reg = DEVICES.lock();
    let device = reg
        .devices
        .get_mut(index)
        .and_then(|d| d.as_mut())
        .ok_or(KernelError::NotFound)?;
    f(device)
}

/// Run a closure with the primary DRM device.
pub fn with_primary<F, R>(f: F) -> KernelResult<R>
where
    F: FnOnce(&DrmDevice) -> KernelResult<R>,
{
    let reg = DEVICES.lock();
    let idx = reg.primary;
    let device = reg
        .devices
        .get(idx)
        .and_then(|d| d.as_ref())
        .ok_or(KernelError::NotFound)?;
    f(device)
}

/// Run a closure with the primary DRM device (mutable).
pub fn with_primary_mut<F, R>(f: F) -> KernelResult<R>
where
    F: FnOnce(&mut DrmDevice) -> KernelResult<R>,
{
    let mut reg = DEVICES.lock();
    let idx = reg.primary;
    let device = reg
        .devices
        .get_mut(idx)
        .and_then(|d| d.as_mut())
        .ok_or(KernelError::NotFound)?;
    f(device)
}

/// Number of registered DRM devices.
#[must_use]
pub fn device_count() -> usize {
    DEVICES.lock().count
}

/// Index of the primary display device — the GPU that owns the active scanout.
///
/// When a hardware GPU (virtio-gpu) is present it is promoted to primary over
/// a fallback dumb framebuffer (limine-fb), so this is the device a
/// `/dev/dri/card0` / `renderD128` client should be bound to (matching Linux,
/// where the primary and render nodes are two faces of the *same* GPU).  Zero
/// when no device is registered (callers must gate on [`device_count`]).
#[must_use]
pub fn primary_device() -> usize {
    DEVICES.lock().primary
}

// ---------------------------------------------------------------------------
// Initialization
// ---------------------------------------------------------------------------

/// Initialize the DRM subsystem.
///
/// Creates a Limine backend for the bootloader framebuffer (always
/// available) and optionally a virtio-gpu backend if the device is
/// present.
pub fn init() {
    serial_println!("[drm] Initializing DRM subsystem...");

    // Always register the Limine framebuffer backend (it's how we
    // get a display before any GPU driver loads).
    let limine = driver::LimineBackend::new();
    if let Err(e) = register_device("limine-fb", DrmBackend::Limine(limine)) {
        serial_println!("[drm] WARNING: failed to register Limine backend: {:?}", e);
    }

    // Try to register a virtio-gpu backend if the device exists.
    if let Some(virtio) = driver::VirtioGpuBackend::probe() {
        match register_device("virtio-gpu", DrmBackend::VirtioGpu(virtio)) {
            Ok(idx) => {
                // Prefer virtio-gpu over Limine if available (it supports
                // mode switching, page flipping, etc.).
                DEVICES.lock().primary = idx;
                serial_println!("[drm] virtio-gpu set as primary display");
            }
            Err(e) => {
                serial_println!("[drm] WARNING: failed to register virtio-gpu: {:?}", e);
            }
        }
    }

    // Probe for a legacy ATI display device, check its register map against the
    // hardware, exercise the display, and register it as a DRM device.
    //
    // It is registered but *not* promoted to primary, even though it is a real
    // GPU and virtio-gpu is a paravirtual one. Primary means "the device the
    // console and compositor bind to", and on the machine this actually boots
    // the ATI card is an unused second head while virtio-gpu drives the screen
    // the operator is looking at. Promoting it would move the display onto a
    // driver with no cursor plane, no gamma, and a mode list capped by 16 MiB
    // of aperture — a downgrade dressed as an upgrade.
    if let Some(ati) = ati::probe_hardware() {
        match register_device("ati", DrmBackend::Ati(ati)) {
            Ok(idx) => serial_println!("[drm] ATI display block registered as device {}", idx),
            Err(e) => serial_println!("[drm] WARNING: failed to register ATI backend: {:?}", e),
        }
    }

    // Enable hotplug detection now that all backends are registered.
    hotplug::enable();

    serial_println!(
        "[drm] DRM subsystem initialized ({} devices)",
        device_count()
    );
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Run DRM subsystem self-tests.
pub fn self_test() -> KernelResult<()> {
    serial_println!("[drm] Running self-test...");

    // 1. At least one device is registered.
    let count = device_count();
    if count == 0 {
        serial_println!("[drm]   FAIL: no devices registered");
        return Err(KernelError::InternalError);
    }
    serial_println!("[drm]   Devices registered: {} — OK", count);

    // 2. Primary device has at least one connector and one CRTC.
    with_primary(|dev| {
        if dev.connectors().is_empty() {
            serial_println!("[drm]   FAIL: primary has no connectors");
            return Err(KernelError::InternalError);
        }
        if dev.crtcs().is_empty() {
            serial_println!("[drm]   FAIL: primary has no CRTCs");
            return Err(KernelError::InternalError);
        }
        serial_println!(
            "[drm]   Primary: {} ({} connectors, {} CRTCs, {} planes) — OK",
            dev.driver_name(),
            dev.connectors().len(),
            dev.crtcs().len(),
            dev.planes().len(),
        );
        Ok(())
    })?;

    // 3. GEM create/mmap/destroy round-trip.
    with_primary_mut(|dev| {
        let handle = dev.gem_create(64, 64, PixelFormat::Xrgb8888)?;
        let ptr = dev.gem_mmap(handle)?;
        if ptr.is_null() {
            serial_println!("[drm]   FAIL: gem_mmap returned null");
            return Err(KernelError::InternalError);
        }
        // Write and read a test pattern.
        // SAFETY: ptr points to a freshly-allocated GEM buffer of
        // at least 64*64*4 = 16384 bytes.
        unsafe {
            ptr.write(0xDE);
            let val = ptr.read();
            if val != 0xDE {
                serial_println!("[drm]   FAIL: GEM read-back mismatch");
                return Err(KernelError::InternalError);
            }
        }
        dev.gem_destroy(handle)?;
        serial_println!("[drm]   GEM create/mmap/destroy: OK");
        Ok(())
    })?;

    // 4. Framebuffer create/destroy.
    with_primary_mut(|dev| {
        let handle = dev.gem_create(128, 128, PixelFormat::Xrgb8888)?;
        let fb_id = dev.fb_create(handle, 128, 128, 512, PixelFormat::Xrgb8888)?;
        let fb = dev.fb_get(fb_id);
        if fb.is_none() {
            serial_println!("[drm]   FAIL: fb_get returned None");
            dev.gem_destroy(handle)?;
            return Err(KernelError::InternalError);
        }
        dev.fb_destroy(fb_id)?;
        dev.gem_destroy(handle)?;
        serial_println!("[drm]   Framebuffer create/destroy: OK");
        Ok(())
    })?;

    // 5. Display size query.
    with_primary(|dev| {
        let (w, h) = dev.display_size();
        if w == 0 || h == 0 {
            serial_println!("[drm]   FAIL: display_size returned 0x0");
            return Err(KernelError::InternalError);
        }
        serial_println!("[drm]   Display size: {}x{} — OK", w, h);
        Ok(())
    })?;

    // 6. PixelFormat conversion.
    mode::self_test()?;

    // 6b. Fake-offset allocator for dumb-buffer mmap.
    dumb_mmap::self_test()?;

    // 7. EDID parser.
    edid::self_test()?;

    // 8. Hotplug detection framework.
    hotplug::self_test()?;

    // 9. Atomic modesetting.
    atomic::self_test()?;

    // 10. Cursor operations.
    with_primary_mut(|dev| {
        let crtc_id = dev.first_crtc_id().ok_or(KernelError::InternalError)?;

        // Cursor should start invisible.
        let cs = dev
            .cursor_state(crtc_id)
            .ok_or(KernelError::InternalError)?;
        if cs.visible {
            serial_println!("[drm]   FAIL: cursor visible at init");
            return Err(KernelError::InternalError);
        }

        // Create a small GEM buffer for cursor.
        let handle = dev.gem_create(64, 64, PixelFormat::Argb8888)?;

        // Set cursor.
        dev.cursor_set(crtc_id, handle, 64, 64, 0, 0)?;
        let cs = dev
            .cursor_state(crtc_id)
            .ok_or(KernelError::InternalError)?;
        if !cs.visible || cs.gem_handle != handle {
            serial_println!("[drm]   FAIL: cursor_set didn't work");
            dev.gem_destroy(handle)?;
            return Err(KernelError::InternalError);
        }

        // Move cursor.
        dev.cursor_move(crtc_id, 100, 200)?;
        let cs = dev
            .cursor_state(crtc_id)
            .ok_or(KernelError::InternalError)?;
        if cs.x != 100 || cs.y != 200 {
            serial_println!("[drm]   FAIL: cursor_move didn't update position");
            dev.gem_destroy(handle)?;
            return Err(KernelError::InternalError);
        }

        // Hide cursor.
        dev.cursor_set(crtc_id, 0, 0, 0, 0, 0)?;
        let cs = dev
            .cursor_state(crtc_id)
            .ok_or(KernelError::InternalError)?;
        if cs.visible {
            serial_println!("[drm]   FAIL: cursor still visible after hide");
            dev.gem_destroy(handle)?;
            return Err(KernelError::InternalError);
        }

        dev.gem_destroy(handle)?;
        serial_println!("[drm]   Cursor operations: OK");
        Ok(())
    })?;

    // 11. Mode-set and page-flip discipline.
    //
    // This is the regression test for the request lane C filed on 2026-08-21:
    // `page_flip` used to accept a framebuffer of any size and let each backend
    // invent a meaning for the mismatch — ATI silently mode-set, virtio-gpu
    // silently cropped — so the same ioctl sequence changed the resolution on
    // one machine and cropped the picture on another, with `GETCRTC` reporting
    // the boot mode on both. Every assertion below fails if that behaviour is
    // reintroduced.
    //
    // It performs a *real* mode-set on the primary display, which on a
    // paravirtual backend means the console's pixels are replaced by a blank
    // test buffer for the rest of boot. That is deliberate: an untested
    // mode-set success path is how a driver first gets debugged on a black
    // screen. The serial log is the boot contract, not the framebuffer.
    with_primary_mut(|dev| {
        let crtc_id = dev.first_crtc_id().ok_or(KernelError::InternalError)?;
        let conn_id = dev
            .connectors()
            .first()
            .map(|c| c.id)
            .ok_or(KernelError::InternalError)?;
        let (w, h) = dev.display_size();
        let cur_mode = dev
            .connectors()
            .first()
            .and_then(|c| c.modes.iter().find(|m| m.hdisplay == w && m.vdisplay == h))
            .copied()
            .ok_or(KernelError::InternalError)?;

        // A correctly-sized buffer, and a deliberately wrong-sized one.
        let good_gem = dev.gem_create(w, h, PixelFormat::Xrgb8888)?;
        let good_pitch = dev.gem_pitch(good_gem)?;
        let good_fb = dev.fb_create(good_gem, w, h, good_pitch, PixelFormat::Xrgb8888)?;
        let (bw, bh) = (w.saturating_sub(16).max(16), h.saturating_sub(16).max(16));
        let bad_gem = dev.gem_create(bw, bh, PixelFormat::Xrgb8888)?;
        let bad_pitch = dev.gem_pitch(bad_gem)?;
        let bad_fb = dev.fb_create(bad_gem, bw, bh, bad_pitch, PixelFormat::Xrgb8888)?;

        // Everything from here on must clean up, so failures are collected
        // rather than returned early: leaking video memory out of a self-test
        // would make the *next* test fail for an unrelated reason.
        let mut fail: Option<&'static str> = None;
        fn check(fail: &mut Option<&'static str>, cond: bool, what: &'static str) {
            if !cond && fail.is_none() {
                serial_println!("[drm]   FAIL: {}", what);
                *fail = Some(what);
            }
        }

        // (a) A mode nobody advertises is refused.
        let bogus = mode::DrmMode::from_resolution(w.saturating_add(1), h.saturating_add(1), 60);
        check(
            &mut fail,
            matches!(
                dev.set_crtc(crtc_id, Some(good_fb), 0, 0, &[conn_id], Some(&bogus)),
                Err(KernelError::InvalidArgument)
            ),
            "set_crtc accepted a mode the connector does not advertise",
        );

        // (b) An enable with no framebuffer, and one with no connector, are
        //     both refused — a timed CRTC fetching from nowhere is not a state
        //     to leave the hardware in.
        check(
            &mut fail,
            matches!(
                dev.set_crtc(crtc_id, None, 0, 0, &[conn_id], Some(&cur_mode)),
                Err(KernelError::InvalidArgument)
            ),
            "set_crtc accepted an enable with no framebuffer",
        );
        check(
            &mut fail,
            matches!(
                dev.set_crtc(crtc_id, Some(good_fb), 0, 0, &[], Some(&cur_mode)),
                Err(KernelError::InvalidArgument)
            ),
            "set_crtc accepted an enable with no connectors",
        );

        // (c) A disable that also names a framebuffer is self-contradictory.
        check(
            &mut fail,
            matches!(
                dev.set_crtc(crtc_id, Some(good_fb), 0, 0, &[], None),
                Err(KernelError::InvalidArgument)
            ),
            "set_crtc accepted a disable that also named a framebuffer",
        );

        // (d) A framebuffer too small for the mode is refused, even at the
        //     right origin — this is Linux's "Invalid fb size".
        check(
            &mut fail,
            matches!(
                dev.set_crtc(crtc_id, Some(bad_fb), 0, 0, &[conn_id], Some(&cur_mode)),
                Err(KernelError::InvalidArgument)
            ),
            "set_crtc accepted a framebuffer smaller than the mode",
        );

        // (e) An unknown connector is ENOENT, not EINVAL.
        let ghost = DrmObjectId::new(u32::MAX);
        check(
            &mut fail,
            matches!(
                dev.set_crtc(crtc_id, Some(good_fb), 0, 0, &[ghost], Some(&cur_mode)),
                Err(KernelError::NotFound)
            ),
            "set_crtc did not report an unknown connector as NotFound",
        );

        // (f) The real thing. After it, the object model must describe what is
        //     actually on screen — this is the half that used to be missing
        //     entirely, since nothing ever wrote `crtc.mode` or (outside the
        //     atomic path) `plane.fb`.
        if fail.is_none() {
            if let Err(e) = dev.set_crtc(crtc_id, Some(good_fb), 0, 0, &[conn_id], Some(&cur_mode))
            {
                serial_println!("[drm]   FAIL: set_crtc refused a valid configuration: {e:?}");
                fail = Some("set_crtc refused a valid configuration");
            }
        }
        let crtc = dev
            .crtcs()
            .iter()
            .find(|c| c.id == crtc_id)
            .map(|c| (c.active, c.mode, c.primary_plane));
        check(
            &mut fail,
            crtc.is_some_and(|(active, m, _)| {
                active && m.is_some_and(|m| m.hdisplay == w && m.vdisplay == h)
            }),
            "GETCRTC would not report the mode that was just programmed",
        );
        let primary = crtc.map(|(_, _, p)| p);
        check(
            &mut fail,
            primary.is_some_and(|pid| {
                dev.planes()
                    .iter()
                    .any(|p| p.id == pid && p.fb == Some(good_fb))
            }),
            "the primary plane does not name the framebuffer that was just bound",
        );
        // The routing is the other half of what `GETCONNECTOR`/`GETENCODER`
        // report, and it is what tells a client the display is already
        // configured. It must name this CRTC, not merely be non-null.
        check(
            &mut fail,
            dev.connectors()
                .iter()
                .find(|c| c.id == conn_id)
                .and_then(|c| c.current_encoder)
                .is_some_and(|eid| {
                    dev.encoders()
                        .iter()
                        .any(|e| e.id == eid && e.crtc == Some(crtc_id))
                }),
            "the connector is not routed to the CRTC that was just programmed",
        );

        // (g) A flip of the right size works; one of the wrong size does not.
        //     This pair is the whole of lane C's Ask 2.
        if fail.is_none() {
            if let Err(e) = dev.page_flip(crtc_id, good_fb) {
                serial_println!("[drm]   FAIL: page_flip refused a matching framebuffer: {e:?}");
                fail = Some("page_flip refused a matching framebuffer");
            }
        }
        check(
            &mut fail,
            matches!(
                dev.page_flip(crtc_id, bad_fb),
                Err(KernelError::InvalidArgument)
            ),
            "page_flip accepted a framebuffer whose size differs from the mode",
        );

        // (h) Turning the CRTC off is a success path, not an error one — a
        //     compositor shutting down cleanly is its normal user — and a flip
        //     afterwards is refused because there is no mode to flip within.
        if fail.is_none() {
            if let Err(e) = dev.set_crtc(crtc_id, None, 0, 0, &[], None) {
                serial_println!("[drm]   FAIL: set_crtc refused a disable: {e:?}");
                fail = Some("set_crtc refused a disable");
            }
        }
        check(
            &mut fail,
            dev.crtcs()
                .iter()
                .any(|c| c.id == crtc_id && !c.active && c.mode.is_none()),
            "the CRTC still reports a mode after being turned off",
        );
        check(
            &mut fail,
            matches!(
                dev.page_flip(crtc_id, good_fb),
                Err(KernelError::InvalidArgument)
            ),
            "page_flip succeeded on a CRTC that is in no mode",
        );
        check(
            &mut fail,
            dev.encoders().iter().all(|e| e.crtc != Some(crtc_id))
                && dev
                    .connectors()
                    .iter()
                    .find(|c| c.id == conn_id)
                    .is_some_and(|c| c.current_encoder.is_none()),
            "an encoder still names the CRTC after it was turned off",
        );

        // (i) Tear down in the one order that is correct on every backend: the
        //     CRTC is already off from (h), so nothing is scanning these
        //     buffers out when they are freed.
        //
        //     It is tempting to re-arm the mode first so the test does not
        //     leave the screen dark — but that would point the CRTC at
        //     `good_gem` and then free it, and a driver that can really
        //     retime refuses exactly that (`AtiBackend::gem_destroy` returns
        //     `DeviceBusy`). Freeing VRAM that the display engine is still
        //     reading is the bug that check exists to catch; a self-test must
        //     not be the thing that trips it.
        //
        //     The CRTC is therefore left off. That is a state the system
        //     already knows how to leave: the compositor calls
        //     `ensure_crtc_configured` before its first flip, which re-times
        //     the CRTC against its own buffer. On both backends that can be
        //     primary today (`limine-fb`, `virtio-gpu`) `disable_crtc` is a
        //     no-op anyway, so the console keeps its pixels either way.
        dev.fb_destroy(bad_fb)?;
        dev.gem_destroy(bad_gem)?;
        dev.fb_destroy(good_fb)?;
        dev.gem_destroy(good_gem)?;

        if fail.is_some() {
            return Err(KernelError::InternalError);
        }
        serial_println!("[drm]   Mode-set and page-flip discipline ({w}x{h}): OK");
        Ok(())
    })?;

    // 12. ATI/AMD legacy register + timing arithmetic.
    //
    // Pure arithmetic, so it runs on every boot regardless of what display
    // hardware is present — the machine need not have an ATI device in it.
    ati::self_test()?;

    serial_println!("[drm] Self-test PASSED");
    Ok(())
}
