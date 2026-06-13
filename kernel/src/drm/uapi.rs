//! Linux DRM userspace-ABI (uAPI) definitions — the foundation of the
//! Linux graphics-compatibility shim (roadmap §5.1 / §5094).
//!
//! ## Why this exists
//!
//! Unmodified Linux graphics clients — Mesa, libdrm, the X.Org modesetting
//! driver, Wayland compositors (wlroots/Mutter/KWin), SDL/KMSDRM, and the
//! userspace half of the proprietary NVIDIA stack — all drive the display
//! the same way: they `open("/dev/dri/card0")` (or `renderD128`), then
//! issue a sequence of `ioctl(DRM_IOCTL_*)` calls to learn the driver
//! version and capabilities, enumerate KMS resources (connectors, CRTCs,
//! planes), allocate buffers, and program scanout.  Providing this
//! interface lets those clients drive the OuRoS native [`crate::drm`]
//! subsystem without modification, exactly as the ALSA shim
//! ([`crate::audio_alsa`]) lets Linux audio clients drive the software
//! mixer.
//!
//! This module is the **pure ABI layer**: the `'d'`-magic ioctl
//! request-number encoding, byte-exact `#[repr(C)]` mirrors of the core
//! `include/uapi/drm/drm.h` payload structs, the capability tag constants,
//! and the small amount of pure policy (which client caps an atomic-capable
//! driver accepts).  It deliberately contains **no device nodes, no per-fd
//! state, and no `unsafe`** — those land in the follow-up commits that wire
//! `/dev/dri/*` into the VFS and route the DRM ioctls through here.  Keeping
//! the ABI surface pure and exhaustively self-tested means the wiring layers
//! build on a verified, byte-accurate foundation.
//!
//! ## ABI accuracy
//!
//! Every value below is fixed by the Linux UAPI header
//! `include/uapi/drm/drm.h` and must not be renumbered.  Struct-carrying
//! ioctls encode `sizeof(struct)` in their request number, so the
//! `#[repr(C)]` layouts must be byte-identical to Linux's on a 64-bit
//! target or real libdrm's computed ioctl number never matches ours.  Each
//! struct's size is asserted against its authoritative Linux value in
//! [`self_test`], and the ioctl numbers are derived from `size_of` (not
//! hand-typed) so they stay consistent with the layout, then checked
//! against their known Linux hex values.

// `dead_code` is allowed for the whole `drm` subsystem via the
// `#[allow(dead_code)]` on `pub mod uapi;` in `drm/mod.rs` (this is ABI
// surface whose consumers land in follow-up wiring commits).

use crate::serial_println;

// ---------------------------------------------------------------------------
// ioctl request-number encoding (Linux `include/uapi/asm-generic/ioctl.h`)
// ---------------------------------------------------------------------------

/// Number-field width (bits) — the per-driver command index.
const IOC_NRBITS: u32 = 8;
/// Type-field width (bits) — the driver "magic" letter.
const IOC_TYPEBITS: u32 = 8;
/// Size-field width (bits) — `sizeof` the argument struct.
const IOC_SIZEBITS: u32 = 14;

const IOC_NRSHIFT: u32 = 0;
const IOC_TYPESHIFT: u32 = IOC_NRSHIFT + IOC_NRBITS; // 8
const IOC_SIZESHIFT: u32 = IOC_TYPESHIFT + IOC_TYPEBITS; // 16
const IOC_DIRSHIFT: u32 = IOC_SIZESHIFT + IOC_SIZEBITS; // 30

/// Direction: no data transferred (`_IO`).
const IOC_NONE: u32 = 0;
/// Direction: userspace writes to the kernel (`_IOW`).
const IOC_WRITE: u32 = 1;
/// Direction: kernel writes to userspace (`_IOR`).
const IOC_READ: u32 = 2;

/// The DRM ioctl "magic" letter (`'d'`).  Every `DRM_IOCTL_*` uses it.
pub const DRM_IOCTL_BASE: u32 = 0x64; // b'd'

/// Encode an ioctl request number from its `(dir, type, nr, size)` tuple,
/// matching Linux's `_IOC(dir, type, nr, size)` macro.
///
/// Pure bit-twiddling — no arithmetic, so it is `const` and clippy-clean.
/// `size` is masked to [`IOC_SIZEBITS`]; every DRM payload here is well
/// under the 16383-byte limit (checked in [`self_test`]).
const fn ioc(dir: u32, ty: u32, nr: u32, size: u32) -> u32 {
    (dir << IOC_DIRSHIFT)
        | (ty << IOC_TYPESHIFT)
        | (nr << IOC_NRSHIFT)
        | ((size & ((1 << IOC_SIZEBITS) - 1)) << IOC_SIZESHIFT)
}

/// Size of a DRM payload struct as a `u32` for ioctl-number encoding.
///
/// Every struct here is well under the 14-bit `_IOC` size field (max
/// 16383 bytes), so the cast cannot truncate; the bound is checked in
/// [`self_test`].
#[allow(clippy::cast_possible_truncation)]
const fn struct_size<T>() -> u32 {
    core::mem::size_of::<T>() as u32
}

/// `DRM_IOWR('d', nr, struct)` — bidirectional struct payload.
const fn iowr<T>(nr: u32) -> u32 {
    ioc(IOC_READ | IOC_WRITE, DRM_IOCTL_BASE, nr, struct_size::<T>())
}
/// `DRM_IOW('d', nr, struct)` — userspace → kernel struct payload.
const fn iow<T>(nr: u32) -> u32 {
    ioc(IOC_WRITE, DRM_IOCTL_BASE, nr, struct_size::<T>())
}
/// `DRM_IOR('d', nr, struct)` — kernel → userspace struct payload.
const fn ior<T>(nr: u32) -> u32 {
    ioc(IOC_READ, DRM_IOCTL_BASE, nr, struct_size::<T>())
}

// ---------------------------------------------------------------------------
// Byte-exact `#[repr(C)]` mirrors of the core `drm.h` payload structs
// ---------------------------------------------------------------------------
//
// On our 64-bit-only target `__kernel_size_t` and `char __user *` are both
// 8 bytes, `int`/`__u32` are 4 bytes.  Field order and padding must match
// Linux exactly so libdrm's `sizeof`-derived ioctl number lands on ours.

/// `drm_magic_t` — an authentication magic token (`__u32`).
pub type DrmMagic = u32;

/// `struct drm_version` (DRM_IOCTL_VERSION) — driver name/date/description
/// plus version numbers.  64 bytes on a 64-bit target: three `int`s
/// (12 bytes) + 4 bytes padding to align the first `__kernel_size_t`, then
/// three `(size_t len, char *buf)` pairs (48 bytes).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct DrmVersion {
    /// Driver major version (kernel writes).
    pub version_major: i32,
    /// Driver minor version (kernel writes).
    pub version_minor: i32,
    /// Driver patch level (kernel writes).
    pub version_patchlevel: i32,
    /// In: capacity of `name`; out: actual length the kernel wrote/needs.
    pub name_len: u64,
    /// Userspace buffer for the driver name (`char __user *`).
    pub name: u64,
    /// In/out length for `date`.
    pub date_len: u64,
    /// Userspace buffer for the driver date (`char __user *`).
    pub date: u64,
    /// In/out length for `desc`.
    pub desc_len: u64,
    /// Userspace buffer for the driver description (`char __user *`).
    pub desc: u64,
}

/// `struct drm_unique` (GET_UNIQUE / SET_UNIQUE) — the device's unique bus
/// identifier string.  16 bytes: `(size_t len, char *buf)`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct DrmUnique {
    /// In/out length of the `unique` buffer.
    pub unique_len: u64,
    /// Userspace buffer for the unique string (`char __user *`).
    pub unique: u64,
}

/// `struct drm_auth` (GET_MAGIC / AUTH_MAGIC) — a single magic token.
/// 4 bytes.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct DrmAuth {
    /// The authentication magic.
    pub magic: DrmMagic,
}

/// `struct drm_get_cap` (GET_CAP) — query a device capability.  16 bytes:
/// `__u64 capability` (in) + `__u64 value` (out).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct DrmGetCap {
    /// One of the `DRM_CAP_*` tags.
    pub capability: u64,
    /// Kernel-written capability value.
    pub value: u64,
}

/// `struct drm_set_client_cap` (SET_CLIENT_CAP) — opt into a client-side
/// behaviour change.  16 bytes: `__u64 capability` + `__u64 value`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct DrmSetClientCap {
    /// One of the `DRM_CLIENT_CAP_*` tags.
    pub capability: u64,
    /// Requested value (typically 0/1).
    pub value: u64,
}

/// `struct drm_set_version` (SET_VERSION) — negotiate the DRM
/// interface/driver version with the kernel.  16 bytes: four `int`s.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct DrmSetVersion {
    /// Desired DRM interface major (in); kernel's actual (out).
    pub drm_di_major: i32,
    /// Desired DRM interface minor.
    pub drm_di_minor: i32,
    /// Desired driver-specific major.
    pub drm_dd_major: i32,
    /// Desired driver-specific minor.
    pub drm_dd_minor: i32,
}

// ---------------------------------------------------------------------------
// Core (non-KMS) DRM ioctl request numbers
// ---------------------------------------------------------------------------
//
// Derived from the struct layouts above so they can never drift; the known
// Linux hex values are asserted in `self_test`.

/// `DRM_IOCTL_VERSION` — query the driver name/date/desc + version.
pub const DRM_IOCTL_VERSION: u32 = iowr::<DrmVersion>(0x00);
/// `DRM_IOCTL_GET_UNIQUE` — query the device's unique bus-id string.
pub const DRM_IOCTL_GET_UNIQUE: u32 = iowr::<DrmUnique>(0x01);
/// `DRM_IOCTL_GET_MAGIC` — obtain an auth magic for this fd.
pub const DRM_IOCTL_GET_MAGIC: u32 = ior::<DrmAuth>(0x02);
/// `DRM_IOCTL_SET_VERSION` — negotiate the interface/driver version.
pub const DRM_IOCTL_SET_VERSION: u32 = iowr::<DrmSetVersion>(0x07);
/// `DRM_IOCTL_GET_CAP` — query a device capability value.
pub const DRM_IOCTL_GET_CAP: u32 = iowr::<DrmGetCap>(0x0c);
/// `DRM_IOCTL_SET_CLIENT_CAP` — opt into a client-side behaviour change.
pub const DRM_IOCTL_SET_CLIENT_CAP: u32 = iow::<DrmSetClientCap>(0x0d);
/// `DRM_IOCTL_SET_UNIQUE` — (legacy) set the unique bus-id string.
pub const DRM_IOCTL_SET_UNIQUE: u32 = iow::<DrmUnique>(0x10);
/// `DRM_IOCTL_AUTH_MAGIC` — authenticate a previously-issued magic.
pub const DRM_IOCTL_AUTH_MAGIC: u32 = iow::<DrmAuth>(0x11);

// ---------------------------------------------------------------------------
// Device capability tags (`DRM_CAP_*`, argument to GET_CAP)
// ---------------------------------------------------------------------------

/// Driver supports the dumb-buffer (KMS scanout) allocation API.
pub const DRM_CAP_DUMB_BUFFER: u64 = 0x1;
/// `DRM_IOCTL_WAIT_VBLANK` supports a high-CRTC index field.
pub const DRM_CAP_VBLANK_HIGH_CRTC: u64 = 0x2;
/// Preferred bit depth for dumb buffers.
pub const DRM_CAP_DUMB_PREFERRED_DEPTH: u64 = 0x3;
/// Whether a shadow buffer is preferred for dumb-buffer scanout.
pub const DRM_CAP_DUMB_PREFER_SHADOW: u64 = 0x4;
/// PRIME (dma-buf) import/export support (bitmask of the two below).
pub const DRM_CAP_PRIME: u64 = 0x5;
/// Vblank/event timestamps use `CLOCK_MONOTONIC`.
pub const DRM_CAP_TIMESTAMP_MONOTONIC: u64 = 0x6;
/// Driver supports asynchronous (tear-allowed) page flips.
pub const DRM_CAP_ASYNC_PAGE_FLIP: u64 = 0x7;
/// Recommended/maximum hardware cursor plane width.
pub const DRM_CAP_CURSOR_WIDTH: u64 = 0x8;
/// Recommended/maximum hardware cursor plane height.
pub const DRM_CAP_CURSOR_HEIGHT: u64 = 0x9;
/// `ADDFB2` accepts per-plane format modifiers.
pub const DRM_CAP_ADDFB2_MODIFIERS: u64 = 0x10;
/// Page-flip can target a specific vblank sequence.
pub const DRM_CAP_PAGE_FLIP_TARGET: u64 = 0x11;
/// The CRTC id is reported in the vblank event.
pub const DRM_CAP_CRTC_IN_VBLANK_EVENT: u64 = 0x12;
/// Driver supports sync objects.
pub const DRM_CAP_SYNCOBJ: u64 = 0x13;
/// Driver supports timeline sync objects.
pub const DRM_CAP_SYNCOBJ_TIMELINE: u64 = 0x14;

/// PRIME flag: driver can import a dma-buf.
pub const DRM_PRIME_CAP_IMPORT: u64 = 0x1;
/// PRIME flag: driver can export a dma-buf.
pub const DRM_PRIME_CAP_EXPORT: u64 = 0x2;

// ---------------------------------------------------------------------------
// Client capability tags (`DRM_CLIENT_CAP_*`, argument to SET_CLIENT_CAP)
// ---------------------------------------------------------------------------

/// Expose stereo 3D modes to this client.
pub const DRM_CLIENT_CAP_STEREO_3D: u64 = 1;
/// Expose *all* planes (primary + cursor + overlay), not just overlays.
pub const DRM_CLIENT_CAP_UNIVERSAL_PLANES: u64 = 2;
/// Expose atomic-modesetting properties and accept atomic commits.
pub const DRM_CLIENT_CAP_ATOMIC: u64 = 3;
/// Expose the aspect-ratio bits in mode flags.
pub const DRM_CLIENT_CAP_ASPECT_RATIO: u64 = 4;
/// Expose writeback connectors to this client.
pub const DRM_CLIENT_CAP_WRITEBACK_CONNECTORS: u64 = 5;
/// Expose the cursor-plane hotspot properties (for paravirt cursors).
pub const DRM_CLIENT_CAP_CURSOR_PLANE_HOTSPOT: u64 = 6;

// ---------------------------------------------------------------------------
// Driver identity reported through DRM_IOCTL_VERSION
// ---------------------------------------------------------------------------

/// Driver name reported by `DRM_IOCTL_VERSION` (matches the module name a
/// libdrm client would look for).  No trailing NUL — the dispatch layer
/// copies exactly `min(name_len, len)` bytes like the Linux kernel.
pub const DRIVER_NAME: &[u8] = b"ouros";
/// Driver "date" string (build/ABI date), `DRM_IOCTL_VERSION`.
pub const DRIVER_DATE: &[u8] = b"20260613";
/// Human-readable driver description, `DRM_IOCTL_VERSION`.
pub const DRIVER_DESC: &[u8] = b"OuRoS virtual display (KMS compatibility)";
/// Unique bus-id string reported by `DRM_IOCTL_GET_UNIQUE`.
pub const DRIVER_UNIQUE: &[u8] = b"ouros-drm";

/// Driver version major reported by `DRM_IOCTL_VERSION`.
pub const DRIVER_VERSION_MAJOR: i32 = 1;
/// Driver version minor.
pub const DRIVER_VERSION_MINOR: i32 = 0;
/// Driver patch level.
pub const DRIVER_VERSION_PATCHLEVEL: i32 = 0;

// ---------------------------------------------------------------------------
// Pure ABI policy
// ---------------------------------------------------------------------------

/// Whether the shim accepts a given `DRM_CLIENT_CAP_*` opt-in.
///
/// We are an atomic-capable, universal-planes driver, so we accept the
/// modesetting caps a modern compositor needs.  We reject caps we cannot
/// honestly honour (stereo 3D, writeback connectors) so clients fall back
/// rather than assume a feature that does not exist — mirroring how the
/// Linux DRM core returns `-EINVAL` for unsupported client caps.
#[must_use]
pub const fn client_cap_supported(cap: u64) -> bool {
    matches!(
        cap,
        DRM_CLIENT_CAP_UNIVERSAL_PLANES
            | DRM_CLIENT_CAP_ATOMIC
            | DRM_CLIENT_CAP_ASPECT_RATIO
    )
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Exhaustively verify the DRM uAPI ABI surface: struct sizes against
/// Linux's `drm.h`, ioctl request numbers against their known hex values,
/// and the client-cap policy.  Run at boot so an accidental layout or
/// encoding regression is caught immediately rather than as a silent
/// ioctl-number mismatch when a real client connects.
///
/// # Errors
/// Returns [`crate::error::KernelError::InternalError`] if any ABI constant
/// or struct layout does not match its expected value.
pub fn self_test() -> crate::error::KernelResult<()> {
    use crate::error::KernelError;
    use core::mem::size_of;

    macro_rules! check {
        ($cond:expr, $($arg:tt)*) => {
            if !($cond) {
                serial_println!("[drm-uapi]   FAIL: {}", format_args!($($arg)*));
                return Err(KernelError::InternalError);
            }
        };
    }

    // --- byte-exact struct layouts vs Linux drm.h (64-bit) ---------------
    check!(
        size_of::<DrmVersion>() == 64,
        "drm_version size {}",
        size_of::<DrmVersion>()
    );
    check!(
        size_of::<DrmUnique>() == 16,
        "drm_unique size {}",
        size_of::<DrmUnique>()
    );
    check!(size_of::<DrmAuth>() == 4, "drm_auth size {}", size_of::<DrmAuth>());
    check!(
        size_of::<DrmGetCap>() == 16,
        "drm_get_cap size {}",
        size_of::<DrmGetCap>()
    );
    check!(
        size_of::<DrmSetClientCap>() == 16,
        "drm_set_client_cap size {}",
        size_of::<DrmSetClientCap>()
    );
    check!(
        size_of::<DrmSetVersion>() == 16,
        "drm_set_version size {}",
        size_of::<DrmSetVersion>()
    );

    // --- ioctl encodings against known Linux hex values ------------------
    check!(
        DRM_IOCTL_VERSION == 0xC040_6400,
        "DRM_IOCTL_VERSION enc {:#x}",
        DRM_IOCTL_VERSION
    );
    check!(
        DRM_IOCTL_GET_UNIQUE == 0xC010_6401,
        "DRM_IOCTL_GET_UNIQUE enc {:#x}",
        DRM_IOCTL_GET_UNIQUE
    );
    check!(
        DRM_IOCTL_GET_MAGIC == 0x8004_6402,
        "DRM_IOCTL_GET_MAGIC enc {:#x}",
        DRM_IOCTL_GET_MAGIC
    );
    check!(
        DRM_IOCTL_SET_VERSION == 0xC010_6407,
        "DRM_IOCTL_SET_VERSION enc {:#x}",
        DRM_IOCTL_SET_VERSION
    );
    check!(
        DRM_IOCTL_GET_CAP == 0xC010_640C,
        "DRM_IOCTL_GET_CAP enc {:#x}",
        DRM_IOCTL_GET_CAP
    );
    check!(
        DRM_IOCTL_SET_CLIENT_CAP == 0x4010_640D,
        "DRM_IOCTL_SET_CLIENT_CAP enc {:#x}",
        DRM_IOCTL_SET_CLIENT_CAP
    );
    check!(
        DRM_IOCTL_SET_UNIQUE == 0x4010_6410,
        "DRM_IOCTL_SET_UNIQUE enc {:#x}",
        DRM_IOCTL_SET_UNIQUE
    );
    check!(
        DRM_IOCTL_AUTH_MAGIC == 0x4004_6411,
        "DRM_IOCTL_AUTH_MAGIC enc {:#x}",
        DRM_IOCTL_AUTH_MAGIC
    );

    // --- client-cap acceptance policy ------------------------------------
    check!(
        client_cap_supported(DRM_CLIENT_CAP_UNIVERSAL_PLANES),
        "universal-planes cap should be accepted"
    );
    check!(
        client_cap_supported(DRM_CLIENT_CAP_ATOMIC),
        "atomic cap should be accepted"
    );
    check!(
        client_cap_supported(DRM_CLIENT_CAP_ASPECT_RATIO),
        "aspect-ratio cap should be accepted"
    );
    check!(
        !client_cap_supported(DRM_CLIENT_CAP_STEREO_3D),
        "stereo-3d cap should be rejected (unsupported)"
    );
    check!(
        !client_cap_supported(DRM_CLIENT_CAP_WRITEBACK_CONNECTORS),
        "writeback-connectors cap should be rejected (no writeback connectors)"
    );

    serial_println!("[drm-uapi] DRM uAPI ABI self-test PASSED");
    Ok(())
}
