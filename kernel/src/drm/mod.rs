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
pub mod atomic;
#[allow(dead_code)]
pub mod connector;
#[allow(dead_code)]
pub mod dumb_mmap;
#[allow(dead_code)]
pub mod crtc;
#[allow(dead_code)]
pub mod driver;
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
pub mod card_fd;
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
use self::mode::PixelFormat;
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
        };
        // One cursor state per CRTC.
        let cursor_count = crtcs.len();
        self.connectors = connectors;
        self.crtcs = crtcs;
        self.planes = planes;
        self.encoders = encoders;
        self.cursor_states = (0..cursor_count)
            .map(|_| CursorState::new())
            .collect();
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
        };
        let handle = gem.handle;
        self.gem_objects.push(gem);
        Ok(handle)
    }

    /// Free a GPU buffer object.
    pub fn gem_destroy(&mut self, handle: u32) -> KernelResult<()> {
        let idx = self.gem_objects.iter().position(|g| g.handle == handle)
            .ok_or(KernelError::NotFound)?;
        let gem = self.gem_objects.remove(idx);
        match &mut self.backend {
            DrmBackend::Limine(b) => b.gem_destroy(gem)?,
            DrmBackend::VirtioGpu(b) => b.gem_destroy(gem)?,
        }
        Ok(())
    }

    /// Get a kernel-virtual pointer to a GEM object's backing memory.
    pub fn gem_mmap(&self, handle: u32) -> KernelResult<*mut u8> {
        let gem = self.gem_objects.iter().find(|g| g.handle == handle)
            .ok_or(KernelError::NotFound)?;
        match &self.backend {
            DrmBackend::Limine(b) => b.gem_mmap(gem),
            DrmBackend::VirtioGpu(b) => b.gem_mmap(gem),
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
    pub fn fb_destroy(&mut self, fb_id: DrmObjectId) -> KernelResult<()> {
        let idx = self.framebuffers.iter().position(|f| f.id == fb_id)
            .ok_or(KernelError::NotFound)?;
        self.framebuffers.remove(idx);
        Ok(())
    }

    /// Look up a framebuffer by ID.
    #[must_use]
    pub fn fb_get(&self, fb_id: DrmObjectId) -> Option<&DrmFramebuffer> {
        self.framebuffers.iter().find(|f| f.id == fb_id)
    }

    // --- Display operations ---

    /// Page flip: swap the framebuffer on a CRTC's primary plane.
    pub fn page_flip(
        &mut self,
        crtc_id: DrmObjectId,
        fb_id: DrmObjectId,
    ) -> KernelResult<()> {
        // Validate both IDs exist.
        if !self.crtcs.iter().any(|c| c.id == crtc_id) {
            return Err(KernelError::NotFound);
        }
        let fb = self.framebuffers.iter().find(|f| f.id == fb_id)
            .ok_or(KernelError::NotFound)?;
        let gem = self.gem_objects.iter().find(|g| g.handle == fb.gem_handle)
            .ok_or(KernelError::NotFound)?;

        match &mut self.backend {
            DrmBackend::Limine(b) => b.page_flip(crtc_id, fb, gem),
            DrmBackend::VirtioGpu(b) => b.page_flip(crtc_id, fb, gem),
        }
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
        let fb = self.framebuffers.iter().find(|f| f.id == fb_id)
            .ok_or(KernelError::NotFound)?;
        let gem = self.gem_objects.iter().find(|g| g.handle == fb.gem_handle)
            .ok_or(KernelError::NotFound)?;

        match &mut self.backend {
            DrmBackend::Limine(b) => b.flush_region(fb, gem, x, y, w, h),
            DrmBackend::VirtioGpu(b) => b.flush_region(fb, gem, x, y, w, h),
        }
    }

    /// Get the current display dimensions (width, height) of the primary output.
    #[must_use]
    pub fn display_size(&self) -> (u32, u32) {
        self.connectors.first()
            .and_then(|c| c.modes.first())
            .map(|m| (m.hdisplay, m.vdisplay))
            .unwrap_or((0, 0))
    }

    /// Return the HHDM-mapped virtual addresses of a GEM object's backing frames.
    ///
    /// This allows callers to hold the addresses past the DRM lock scope
    /// and perform direct pixel writes without holding any DRM lock.
    /// Addresses remain valid as long as the GEM object is not destroyed.
    pub fn gem_frame_addrs(&self, handle: u32) -> KernelResult<Vec<u64>> {
        use crate::mm::page_table;

        let gem = self.gem_objects.iter().find(|g| g.handle == handle)
            .ok_or(KernelError::NotFound)?;
        let hhdm = page_table::hhdm().ok_or(KernelError::NotSupported)?;
        let addrs: Vec<u64> = gem.phys_frames.iter()
            .map(|pf| pf.addr() + hhdm)
            .collect();
        Ok(addrs)
    }

    /// Get the pitch (bytes per row) of a GEM object.
    pub fn gem_pitch(&self, handle: u32) -> KernelResult<u32> {
        let gem = self.gem_objects.iter().find(|g| g.handle == handle)
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
    pub fn gem_phys_addrs(&self, handle: u32) -> KernelResult<Vec<u64>> {
        let gem = self.gem_objects.iter().find(|g| g.handle == handle)
            .ok_or(KernelError::NotFound)?;
        Ok(gem.phys_frames.iter().map(|pf| pf.addr()).collect())
    }

    /// Get the total byte size of a GEM object's allocation.
    pub fn gem_size(&self, handle: u32) -> KernelResult<usize> {
        let gem = self.gem_objects.iter().find(|g| g.handle == handle)
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
        let crtc_idx = self.crtcs.iter().position(|c| c.id == crtc_id)
            .ok_or(KernelError::NotFound)?;

        // Validate GEM handle if non-zero.
        if gem_handle != 0 && !self.gem_objects.iter().any(|g| g.handle == gem_handle) {
            return Err(KernelError::NotFound);
        }

        let cs = self.cursor_states.get_mut(crtc_idx)
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
    pub fn cursor_move(
        &mut self,
        crtc_id: DrmObjectId,
        x: i32,
        y: i32,
    ) -> KernelResult<()> {
        let crtc_idx = self.crtcs.iter().position(|c| c.id == crtc_id)
            .ok_or(KernelError::NotFound)?;

        let cs = self.cursor_states.get_mut(crtc_idx)
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
    let device = reg.devices.get(index)
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
    let device = reg.devices.get_mut(index)
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
    let device = reg.devices.get(idx)
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
    let device = reg.devices.get_mut(idx)
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

    // Enable hotplug detection now that all backends are registered.
    hotplug::enable();

    serial_println!("[drm] DRM subsystem initialized ({} devices)", device_count());
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
        let crtc_id = dev.first_crtc_id()
            .ok_or(KernelError::InternalError)?;

        // Cursor should start invisible.
        let cs = dev.cursor_state(crtc_id)
            .ok_or(KernelError::InternalError)?;
        if cs.visible {
            serial_println!("[drm]   FAIL: cursor visible at init");
            return Err(KernelError::InternalError);
        }

        // Create a small GEM buffer for cursor.
        let handle = dev.gem_create(64, 64, PixelFormat::Argb8888)?;

        // Set cursor.
        dev.cursor_set(crtc_id, handle, 64, 64, 0, 0)?;
        let cs = dev.cursor_state(crtc_id)
            .ok_or(KernelError::InternalError)?;
        if !cs.visible || cs.gem_handle != handle {
            serial_println!("[drm]   FAIL: cursor_set didn't work");
            dev.gem_destroy(handle)?;
            return Err(KernelError::InternalError);
        }

        // Move cursor.
        dev.cursor_move(crtc_id, 100, 200)?;
        let cs = dev.cursor_state(crtc_id)
            .ok_or(KernelError::InternalError)?;
        if cs.x != 100 || cs.y != 200 {
            serial_println!("[drm]   FAIL: cursor_move didn't update position");
            dev.gem_destroy(handle)?;
            return Err(KernelError::InternalError);
        }

        // Hide cursor.
        dev.cursor_set(crtc_id, 0, 0, 0, 0, 0)?;
        let cs = dev.cursor_state(crtc_id)
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

    serial_println!("[drm] Self-test PASSED");
    Ok(())
}
