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

/// Extract the type ("magic") field from an encoded ioctl request number.
///
/// Used by the syscall dispatch layer to confirm a request really targets
/// the DRM driver ([`DRM_IOCTL_BASE`]) before trying to interpret it.
#[must_use]
pub const fn ioc_type(cmd: u32) -> u32 {
    (cmd >> IOC_TYPESHIFT) & ((1 << IOC_TYPEBITS) - 1)
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

// ===========================================================================
// KMS (kernel mode-setting) ABI — `include/uapi/drm/drm_mode.h`
// ===========================================================================
//
// These are the structs and ioctls a modesetting client (X.Org
// modesetting, a Wayland compositor, SDL/KMSDRM, plymouth) uses to
// enumerate display resources and program scanout.  Same `'d'` magic,
// same byte-exact-layout requirement: each `sizeof` is encoded in the
// ioctl request number and asserted in `self_test`.

/// Length of the fixed `name` field in `drm_mode_modeinfo`.
pub const DRM_DISPLAY_MODE_LEN: usize = 32;
/// Length of the fixed `name` field in `drm_mode_get_property`.
pub const DRM_PROP_NAME_LEN: usize = 32;

// --- Connection status (drm_mode_get_connector.connection) ----------------

/// A display is connected to this connector.
pub const DRM_MODE_CONNECTED: u32 = 1;
/// Nothing is connected to this connector.
pub const DRM_MODE_DISCONNECTED: u32 = 2;
/// Connection state could not be determined.
pub const DRM_MODE_UNKNOWNCONNECTION: u32 = 3;

// --- Subpixel order (drm_mode_get_connector.subpixel) ---------------------

/// Subpixel order is unknown.
pub const DRM_MODE_SUBPIXEL_UNKNOWN: u32 = 1;
/// Horizontal RGB subpixel order.
pub const DRM_MODE_SUBPIXEL_HORIZONTAL_RGB: u32 = 2;
/// Horizontal BGR subpixel order.
pub const DRM_MODE_SUBPIXEL_HORIZONTAL_BGR: u32 = 3;
/// Vertical RGB subpixel order.
pub const DRM_MODE_SUBPIXEL_VERTICAL_RGB: u32 = 4;
/// Vertical BGR subpixel order.
pub const DRM_MODE_SUBPIXEL_VERTICAL_BGR: u32 = 5;
/// No subpixel structure (e.g. projector).
pub const DRM_MODE_SUBPIXEL_NONE: u32 = 6;

// --- Connector types (drm_mode_get_connector.connector_type) --------------

/// Unknown / virtual connector type.
pub const DRM_MODE_CONNECTOR_UNKNOWN: u32 = 0;
/// VGA (DE-15) analog connector.
pub const DRM_MODE_CONNECTOR_VGA: u32 = 1;
/// HDMI type A connector.
pub const DRM_MODE_CONNECTOR_HDMIA: u32 = 11;
/// Virtual connector (e.g. virtio-gpu / vkms).
pub const DRM_MODE_CONNECTOR_VIRTUAL: u32 = 15;
/// DisplayPort connector.
pub const DRM_MODE_CONNECTOR_DISPLAYPORT: u32 = 10;

// --- Encoder types (drm_mode_get_encoder.encoder_type) --------------------

/// No / unknown encoder.
pub const DRM_MODE_ENCODER_NONE: u32 = 0;
/// TMDS encoder (DVI/HDMI).
pub const DRM_MODE_ENCODER_TMDS: u32 = 2;
/// Virtual encoder (virtio-gpu / vkms).
pub const DRM_MODE_ENCODER_VIRTUAL: u32 = 5;

// --- Mode type/flags (drm_mode_modeinfo.type / .flags) --------------------

/// This mode is the connector's preferred (native) mode.
pub const DRM_MODE_TYPE_PREFERRED: u32 = 1 << 3;
/// This mode is driver-supplied (not a user-added modeline).
pub const DRM_MODE_TYPE_DRIVER: u32 = 1 << 6;
/// Positive horizontal sync polarity.
pub const DRM_MODE_FLAG_PHSYNC: u32 = 1 << 0;
/// Positive vertical sync polarity.
pub const DRM_MODE_FLAG_PVSYNC: u32 = 1 << 2;

// --- KMS payload structs --------------------------------------------------

/// `struct drm_mode_modeinfo` — one display mode (timing) descriptor.
/// 68 bytes: `clock` + 10 `__u16` timing fields + `vrefresh`/`flags`/`type`
/// + a 32-byte name.  No padding (max field alignment is 4).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct DrmModeModeinfo {
    /// Pixel clock in kHz.
    pub clock: u32,
    /// Horizontal active pixels.
    pub hdisplay: u16,
    /// Horizontal sync start.
    pub hsync_start: u16,
    /// Horizontal sync end.
    pub hsync_end: u16,
    /// Horizontal total.
    pub htotal: u16,
    /// Horizontal skew.
    pub hskew: u16,
    /// Vertical active lines.
    pub vdisplay: u16,
    /// Vertical sync start.
    pub vsync_start: u16,
    /// Vertical sync end.
    pub vsync_end: u16,
    /// Vertical total.
    pub vtotal: u16,
    /// Vertical scan.
    pub vscan: u16,
    /// Refresh rate in Hz.
    pub vrefresh: u32,
    /// `DRM_MODE_FLAG_*` bits.
    pub flags: u32,
    /// `DRM_MODE_TYPE_*` bits.
    pub type_: u32,
    /// NUL-padded human-readable mode name.
    pub name: [u8; DRM_DISPLAY_MODE_LEN],
}

/// `struct drm_mode_card_res` (GETRESOURCES) — the device's KMS resource
/// counts and id arrays.  64 bytes: four `__u64` array pointers + eight
/// `__u32` counts/extents.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct DrmModeCardRes {
    /// Userspace array to receive framebuffer ids.
    pub fb_id_ptr: u64,
    /// Userspace array to receive CRTC ids.
    pub crtc_id_ptr: u64,
    /// Userspace array to receive connector ids.
    pub connector_id_ptr: u64,
    /// Userspace array to receive encoder ids.
    pub encoder_id_ptr: u64,
    /// In: caller's array capacity; out: actual count (framebuffers).
    pub count_fbs: u32,
    /// In/out count of CRTCs.
    pub count_crtcs: u32,
    /// In/out count of connectors.
    pub count_connectors: u32,
    /// In/out count of encoders.
    pub count_encoders: u32,
    /// Minimum framebuffer width.
    pub min_width: u32,
    /// Maximum framebuffer width.
    pub max_width: u32,
    /// Minimum framebuffer height.
    pub min_height: u32,
    /// Maximum framebuffer height.
    pub max_height: u32,
}

/// `struct drm_mode_crtc` (GET/SETCRTC) — a CRTC's scanout configuration.
/// 104 bytes: connector-id array pointer/count, ids/position/gamma, plus
/// an embedded `drm_mode_modeinfo`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct DrmModeCrtc {
    /// Userspace array of connector ids to drive (SETCRTC).
    pub set_connectors_ptr: u64,
    /// Number of entries in `set_connectors_ptr`.
    pub count_connectors: u32,
    /// CRTC object id.
    pub crtc_id: u32,
    /// Framebuffer bound to this CRTC (0 = none).
    pub fb_id: u32,
    /// Scanout x offset within the framebuffer.
    pub x: u32,
    /// Scanout y offset within the framebuffer.
    pub y: u32,
    /// Gamma LUT size.
    pub gamma_size: u32,
    /// Whether `mode` is valid (1) or the CRTC is off (0).
    pub mode_valid: u32,
    /// Current/desired display mode.
    pub mode: DrmModeModeinfo,
}

/// `struct drm_mode_get_encoder` (GETENCODER). 20 bytes.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct DrmModeGetEncoder {
    /// Encoder object id (in).
    pub encoder_id: u32,
    /// `DRM_MODE_ENCODER_*` type (out).
    pub encoder_type: u32,
    /// CRTC currently bound to this encoder (out).
    pub crtc_id: u32,
    /// Bitmask of CRTCs this encoder can drive.
    pub possible_crtcs: u32,
    /// Bitmask of encoders that can be cloned with this one.
    pub possible_clones: u32,
}

/// `struct drm_mode_get_connector` (GETCONNECTOR). 80 bytes: four `__u64`
/// array pointers + twelve `__u32` fields (the last is explicit padding).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct DrmModeGetConnector {
    /// Userspace array to receive encoder ids.
    pub encoders_ptr: u64,
    /// Userspace array to receive `drm_mode_modeinfo` entries.
    pub modes_ptr: u64,
    /// Userspace array to receive property ids.
    pub props_ptr: u64,
    /// Userspace array to receive property values.
    pub prop_values_ptr: u64,
    /// In/out count of modes.
    pub count_modes: u32,
    /// In/out count of properties.
    pub count_props: u32,
    /// In/out count of encoders.
    pub count_encoders: u32,
    /// Currently-bound encoder id (out).
    pub encoder_id: u32,
    /// Connector object id (in).
    pub connector_id: u32,
    /// `DRM_MODE_CONNECTOR_*` type (out).
    pub connector_type: u32,
    /// Per-type connector instance number (out).
    pub connector_type_id: u32,
    /// `DRM_MODE_CONNECTED` / `DISCONNECTED` / `UNKNOWNCONNECTION`.
    pub connection: u32,
    /// Physical width in millimetres.
    pub mm_width: u32,
    /// Physical height in millimetres.
    pub mm_height: u32,
    /// `DRM_MODE_SUBPIXEL_*` order.
    pub subpixel: u32,
    /// Explicit tail padding (matches Linux's `pad`).
    pub pad: u32,
}

/// `struct drm_mode_get_plane` (GETPLANE). 32 bytes.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct DrmModeGetPlane {
    /// Plane object id (in).
    pub plane_id: u32,
    /// CRTC currently using this plane (out).
    pub crtc_id: u32,
    /// Framebuffer currently on this plane (out).
    pub fb_id: u32,
    /// Bitmask of CRTCs this plane can attach to.
    pub possible_crtcs: u32,
    /// Gamma LUT size.
    pub gamma_size: u32,
    /// In/out count of supported pixel formats.
    pub count_format_types: u32,
    /// Userspace array to receive supported FourCC formats.
    pub format_type_ptr: u64,
}

/// `struct drm_mode_get_plane_res` (GETPLANERESOURCES). 16 bytes
/// (4 bytes tail padding after the `__u32` count).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct DrmModeGetPlaneRes {
    /// Userspace array to receive plane ids.
    pub plane_id_ptr: u64,
    /// In/out count of planes.
    pub count_planes: u32,
}

/// `struct drm_mode_fb_cmd` (ADDFB, legacy single-plane). 28 bytes.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct DrmModeFbCmd {
    /// Framebuffer object id (out).
    pub fb_id: u32,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Row stride in bytes.
    pub pitch: u32,
    /// Bits per pixel.
    pub bpp: u32,
    /// Colour depth.
    pub depth: u32,
    /// Backing GEM/dumb handle.
    pub handle: u32,
}

/// `struct drm_mode_fb_cmd2` (ADDFB2, multi-plane + FourCC + modifiers).
/// 104 bytes (4 bytes padding before the `__u64 modifier[4]`).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct DrmModeFbCmd2 {
    /// Framebuffer object id (out).
    pub fb_id: u32,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// FourCC pixel format (`DRM_FORMAT_*`).
    pub pixel_format: u32,
    /// `DRM_MODE_FB_*` flags.
    pub flags: u32,
    /// Per-plane backing handles.
    pub handles: [u32; 4],
    /// Per-plane row strides.
    pub pitches: [u32; 4],
    /// Per-plane byte offsets.
    pub offsets: [u32; 4],
    /// Per-plane format modifiers.
    pub modifier: [u64; 4],
}

/// `struct drm_mode_create_dumb` (CREATE_DUMB). 32 bytes.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct DrmModeCreateDumb {
    /// Requested height in pixels.
    pub height: u32,
    /// Requested width in pixels.
    pub width: u32,
    /// Bits per pixel.
    pub bpp: u32,
    /// Allocation flags (must be 0).
    pub flags: u32,
    /// Allocated buffer handle (out).
    pub handle: u32,
    /// Row stride in bytes (out).
    pub pitch: u32,
    /// Total allocation size in bytes (out).
    pub size: u64,
}

/// `struct drm_mode_map_dumb` (MAP_DUMB). 16 bytes.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct DrmModeMapDumb {
    /// Dumb buffer handle (in).
    pub handle: u32,
    /// Padding.
    pub pad: u32,
    /// Fake mmap offset to pass to `mmap(/dev/dri/card0)` (out).
    pub offset: u64,
}

/// `struct drm_mode_destroy_dumb` (DESTROY_DUMB). 4 bytes.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct DrmModeDestroyDumb {
    /// Dumb buffer handle to free.
    pub handle: u32,
}

/// `struct drm_mode_crtc_page_flip` (PAGE_FLIP). 24 bytes.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct DrmModeCrtcPageFlip {
    /// CRTC to flip.
    pub crtc_id: u32,
    /// New framebuffer to scan out.
    pub fb_id: u32,
    /// `DRM_MODE_PAGE_FLIP_*` flags.
    pub flags: u32,
    /// Reserved (must be 0).
    pub reserved: u32,
    /// Opaque cookie returned in the flip-complete event.
    pub user_data: u64,
}

/// `struct drm_mode_cursor` (CURSOR). 28 bytes.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct DrmModeCursor {
    /// `DRM_MODE_CURSOR_BO` / `DRM_MODE_CURSOR_MOVE`.
    pub flags: u32,
    /// Target CRTC.
    pub crtc_id: u32,
    /// Cursor x position.
    pub x: i32,
    /// Cursor y position.
    pub y: i32,
    /// Cursor image width.
    pub width: u32,
    /// Cursor image height.
    pub height: u32,
    /// Cursor image GEM handle (0 = hide).
    pub handle: u32,
}

/// `struct drm_mode_cursor2` (CURSOR2 — adds hotspot). 36 bytes.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct DrmModeCursor2 {
    /// `DRM_MODE_CURSOR_BO` / `DRM_MODE_CURSOR_MOVE`.
    pub flags: u32,
    /// Target CRTC.
    pub crtc_id: u32,
    /// Cursor x position.
    pub x: i32,
    /// Cursor y position.
    pub y: i32,
    /// Cursor image width.
    pub width: u32,
    /// Cursor image height.
    pub height: u32,
    /// Cursor image GEM handle (0 = hide).
    pub handle: u32,
    /// Hotspot x offset.
    pub hot_x: i32,
    /// Hotspot y offset.
    pub hot_y: i32,
}

/// `struct drm_mode_get_property` (GETPROPERTY). 64 bytes.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct DrmModeGetProperty {
    /// Userspace array to receive enum/range values.
    pub values_ptr: u64,
    /// Userspace array to receive enum-name blobs.
    pub enum_blob_ptr: u64,
    /// Property object id (in).
    pub prop_id: u32,
    /// `DRM_MODE_PROP_*` flags (out).
    pub flags: u32,
    /// NUL-padded property name (out).
    pub name: [u8; DRM_PROP_NAME_LEN],
    /// In/out count of values.
    pub count_values: u32,
    /// In/out count of enum blobs.
    pub count_enum_blobs: u32,
}

/// `struct drm_mode_obj_get_properties` (OBJ_GETPROPERTIES). 32 bytes
/// (4 bytes tail padding after the three `__u32`s).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct DrmModeObjGetProperties {
    /// Userspace array to receive property ids.
    pub props_ptr: u64,
    /// Userspace array to receive property values.
    pub prop_values_ptr: u64,
    /// In/out count of properties.
    pub count_props: u32,
    /// Object id being queried.
    pub obj_id: u32,
    /// `DRM_MODE_OBJECT_*` type of `obj_id`.
    pub obj_type: u32,
}

/// `struct drm_mode_atomic` (ATOMIC). 56 bytes.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct DrmModeAtomic {
    /// `DRM_MODE_ATOMIC_*` flags.
    pub flags: u32,
    /// Number of objects in the commit.
    pub count_objs: u32,
    /// Userspace array of object ids.
    pub objs_ptr: u64,
    /// Userspace array of per-object property counts.
    pub count_props_ptr: u64,
    /// Userspace flat array of property ids.
    pub props_ptr: u64,
    /// Userspace flat array of property values.
    pub prop_values_ptr: u64,
    /// Reserved (must be 0).
    pub reserved: u64,
    /// Opaque cookie returned in the page-flip event.
    pub user_data: u64,
}

// --- KMS ioctl request numbers --------------------------------------------
//
// Derived from the layouts above; the known Linux hex values are asserted
// in `self_test`.  All are `DRM_IOWR` (bidirectional) per drm_mode.h.

/// `DRM_IOCTL_MODE_GETRESOURCES` — enumerate fb/crtc/connector/encoder ids.
pub const DRM_IOCTL_MODE_GETRESOURCES: u32 = iowr::<DrmModeCardRes>(0xA0);
/// `DRM_IOCTL_MODE_GETCRTC` — read a CRTC's configuration.
pub const DRM_IOCTL_MODE_GETCRTC: u32 = iowr::<DrmModeCrtc>(0xA1);
/// `DRM_IOCTL_MODE_SETCRTC` — program a CRTC (modeset).
pub const DRM_IOCTL_MODE_SETCRTC: u32 = iowr::<DrmModeCrtc>(0xA2);
/// `DRM_IOCTL_MODE_CURSOR` — set/move the hardware cursor.
pub const DRM_IOCTL_MODE_CURSOR: u32 = iowr::<DrmModeCursor>(0xA3);
/// `DRM_IOCTL_MODE_GETENCODER` — read an encoder's configuration.
pub const DRM_IOCTL_MODE_GETENCODER: u32 = iowr::<DrmModeGetEncoder>(0xA6);
/// `DRM_IOCTL_MODE_GETCONNECTOR` — read a connector + its modes/props.
pub const DRM_IOCTL_MODE_GETCONNECTOR: u32 = iowr::<DrmModeGetConnector>(0xA7);
/// `DRM_IOCTL_MODE_GETPROPERTY` — read a property's metadata.
pub const DRM_IOCTL_MODE_GETPROPERTY: u32 = iowr::<DrmModeGetProperty>(0xAA);
/// `DRM_IOCTL_MODE_ADDFB` — create a framebuffer (legacy single-plane).
pub const DRM_IOCTL_MODE_ADDFB: u32 = iowr::<DrmModeFbCmd>(0xAE);
/// `DRM_IOCTL_MODE_RMFB` — destroy a framebuffer (payload: `unsigned int`).
pub const DRM_IOCTL_MODE_RMFB: u32 = ioc(IOC_READ | IOC_WRITE, DRM_IOCTL_BASE, 0xAF, 4);
/// `DRM_IOCTL_MODE_PAGE_FLIP` — async framebuffer swap on a CRTC.
pub const DRM_IOCTL_MODE_PAGE_FLIP: u32 = iowr::<DrmModeCrtcPageFlip>(0xB0);
/// `DRM_IOCTL_MODE_CREATE_DUMB` — allocate a dumb scanout buffer.
pub const DRM_IOCTL_MODE_CREATE_DUMB: u32 = iowr::<DrmModeCreateDumb>(0xB2);
/// `DRM_IOCTL_MODE_MAP_DUMB` — get an mmap offset for a dumb buffer.
pub const DRM_IOCTL_MODE_MAP_DUMB: u32 = iowr::<DrmModeMapDumb>(0xB3);
/// `DRM_IOCTL_MODE_DESTROY_DUMB` — free a dumb buffer.
pub const DRM_IOCTL_MODE_DESTROY_DUMB: u32 = iowr::<DrmModeDestroyDumb>(0xB4);
/// `DRM_IOCTL_MODE_GETPLANERESOURCES` — enumerate plane ids.
pub const DRM_IOCTL_MODE_GETPLANERESOURCES: u32 = iowr::<DrmModeGetPlaneRes>(0xB5);
/// `DRM_IOCTL_MODE_GETPLANE` — read a plane's configuration.
pub const DRM_IOCTL_MODE_GETPLANE: u32 = iowr::<DrmModeGetPlane>(0xB6);
/// `DRM_IOCTL_MODE_ADDFB2` — create a framebuffer (multi-plane + FourCC).
pub const DRM_IOCTL_MODE_ADDFB2: u32 = iowr::<DrmModeFbCmd2>(0xB8);
/// `DRM_IOCTL_MODE_OBJ_GETPROPERTIES` — read an object's property set.
pub const DRM_IOCTL_MODE_OBJ_GETPROPERTIES: u32 = iowr::<DrmModeObjGetProperties>(0xB9);
/// `DRM_IOCTL_MODE_CURSOR2` — set/move the cursor with a hotspot.
pub const DRM_IOCTL_MODE_CURSOR2: u32 = iowr::<DrmModeCursor2>(0xBB);
/// `DRM_IOCTL_MODE_ATOMIC` — atomic modeset/page-flip commit.
pub const DRM_IOCTL_MODE_ATOMIC: u32 = iowr::<DrmModeAtomic>(0xBC);

/// DRM object-type tag: CRTC (for OBJ_GETPROPERTIES / atomic).
pub const DRM_MODE_OBJECT_CRTC: u32 = 0xcccc_cccc;
/// DRM object-type tag: connector.
pub const DRM_MODE_OBJECT_CONNECTOR: u32 = 0xc0c0_c0c0;
/// DRM object-type tag: encoder.
pub const DRM_MODE_OBJECT_ENCODER: u32 = 0xe0e0_e0e0;
/// DRM object-type tag: plane.
pub const DRM_MODE_OBJECT_PLANE: u32 = 0xeeee_eeee;
/// DRM object-type tag: framebuffer.
pub const DRM_MODE_OBJECT_FB: u32 = 0xfbfb_fbfb;

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

    // --- KMS struct layouts vs Linux drm_mode.h (64-bit) -----------------
    check!(
        size_of::<DrmModeModeinfo>() == 68,
        "drm_mode_modeinfo size {}",
        size_of::<DrmModeModeinfo>()
    );
    check!(
        size_of::<DrmModeCardRes>() == 64,
        "drm_mode_card_res size {}",
        size_of::<DrmModeCardRes>()
    );
    check!(
        size_of::<DrmModeCrtc>() == 104,
        "drm_mode_crtc size {}",
        size_of::<DrmModeCrtc>()
    );
    check!(
        size_of::<DrmModeGetEncoder>() == 20,
        "drm_mode_get_encoder size {}",
        size_of::<DrmModeGetEncoder>()
    );
    check!(
        size_of::<DrmModeGetConnector>() == 80,
        "drm_mode_get_connector size {}",
        size_of::<DrmModeGetConnector>()
    );
    check!(
        size_of::<DrmModeGetPlane>() == 32,
        "drm_mode_get_plane size {}",
        size_of::<DrmModeGetPlane>()
    );
    check!(
        size_of::<DrmModeGetPlaneRes>() == 16,
        "drm_mode_get_plane_res size {}",
        size_of::<DrmModeGetPlaneRes>()
    );
    check!(
        size_of::<DrmModeFbCmd>() == 28,
        "drm_mode_fb_cmd size {}",
        size_of::<DrmModeFbCmd>()
    );
    check!(
        size_of::<DrmModeFbCmd2>() == 104,
        "drm_mode_fb_cmd2 size {}",
        size_of::<DrmModeFbCmd2>()
    );
    check!(
        size_of::<DrmModeCreateDumb>() == 32,
        "drm_mode_create_dumb size {}",
        size_of::<DrmModeCreateDumb>()
    );
    check!(
        size_of::<DrmModeMapDumb>() == 16,
        "drm_mode_map_dumb size {}",
        size_of::<DrmModeMapDumb>()
    );
    check!(
        size_of::<DrmModeDestroyDumb>() == 4,
        "drm_mode_destroy_dumb size {}",
        size_of::<DrmModeDestroyDumb>()
    );
    check!(
        size_of::<DrmModeCrtcPageFlip>() == 24,
        "drm_mode_crtc_page_flip size {}",
        size_of::<DrmModeCrtcPageFlip>()
    );
    check!(
        size_of::<DrmModeCursor>() == 28,
        "drm_mode_cursor size {}",
        size_of::<DrmModeCursor>()
    );
    check!(
        size_of::<DrmModeCursor2>() == 36,
        "drm_mode_cursor2 size {}",
        size_of::<DrmModeCursor2>()
    );
    check!(
        size_of::<DrmModeGetProperty>() == 64,
        "drm_mode_get_property size {}",
        size_of::<DrmModeGetProperty>()
    );
    check!(
        size_of::<DrmModeObjGetProperties>() == 32,
        "drm_mode_obj_get_properties size {}",
        size_of::<DrmModeObjGetProperties>()
    );
    check!(
        size_of::<DrmModeAtomic>() == 56,
        "drm_mode_atomic size {}",
        size_of::<DrmModeAtomic>()
    );

    // --- KMS ioctl encodings against known Linux hex values --------------
    check!(
        DRM_IOCTL_MODE_GETRESOURCES == 0xC040_64A0,
        "MODE_GETRESOURCES enc {:#x}",
        DRM_IOCTL_MODE_GETRESOURCES
    );
    check!(
        DRM_IOCTL_MODE_GETCRTC == 0xC068_64A1,
        "MODE_GETCRTC enc {:#x}",
        DRM_IOCTL_MODE_GETCRTC
    );
    check!(
        DRM_IOCTL_MODE_SETCRTC == 0xC068_64A2,
        "MODE_SETCRTC enc {:#x}",
        DRM_IOCTL_MODE_SETCRTC
    );
    check!(
        DRM_IOCTL_MODE_CURSOR == 0xC01C_64A3,
        "MODE_CURSOR enc {:#x}",
        DRM_IOCTL_MODE_CURSOR
    );
    check!(
        DRM_IOCTL_MODE_GETENCODER == 0xC014_64A6,
        "MODE_GETENCODER enc {:#x}",
        DRM_IOCTL_MODE_GETENCODER
    );
    check!(
        DRM_IOCTL_MODE_GETCONNECTOR == 0xC050_64A7,
        "MODE_GETCONNECTOR enc {:#x}",
        DRM_IOCTL_MODE_GETCONNECTOR
    );
    check!(
        DRM_IOCTL_MODE_GETPROPERTY == 0xC040_64AA,
        "MODE_GETPROPERTY enc {:#x}",
        DRM_IOCTL_MODE_GETPROPERTY
    );
    check!(
        DRM_IOCTL_MODE_ADDFB == 0xC01C_64AE,
        "MODE_ADDFB enc {:#x}",
        DRM_IOCTL_MODE_ADDFB
    );
    check!(
        DRM_IOCTL_MODE_RMFB == 0xC004_64AF,
        "MODE_RMFB enc {:#x}",
        DRM_IOCTL_MODE_RMFB
    );
    check!(
        DRM_IOCTL_MODE_PAGE_FLIP == 0xC018_64B0,
        "MODE_PAGE_FLIP enc {:#x}",
        DRM_IOCTL_MODE_PAGE_FLIP
    );
    check!(
        DRM_IOCTL_MODE_CREATE_DUMB == 0xC020_64B2,
        "MODE_CREATE_DUMB enc {:#x}",
        DRM_IOCTL_MODE_CREATE_DUMB
    );
    check!(
        DRM_IOCTL_MODE_MAP_DUMB == 0xC010_64B3,
        "MODE_MAP_DUMB enc {:#x}",
        DRM_IOCTL_MODE_MAP_DUMB
    );
    check!(
        DRM_IOCTL_MODE_DESTROY_DUMB == 0xC004_64B4,
        "MODE_DESTROY_DUMB enc {:#x}",
        DRM_IOCTL_MODE_DESTROY_DUMB
    );
    check!(
        DRM_IOCTL_MODE_GETPLANERESOURCES == 0xC010_64B5,
        "MODE_GETPLANERESOURCES enc {:#x}",
        DRM_IOCTL_MODE_GETPLANERESOURCES
    );
    check!(
        DRM_IOCTL_MODE_GETPLANE == 0xC020_64B6,
        "MODE_GETPLANE enc {:#x}",
        DRM_IOCTL_MODE_GETPLANE
    );
    check!(
        DRM_IOCTL_MODE_ADDFB2 == 0xC068_64B8,
        "MODE_ADDFB2 enc {:#x}",
        DRM_IOCTL_MODE_ADDFB2
    );
    check!(
        DRM_IOCTL_MODE_OBJ_GETPROPERTIES == 0xC020_64B9,
        "MODE_OBJ_GETPROPERTIES enc {:#x}",
        DRM_IOCTL_MODE_OBJ_GETPROPERTIES
    );
    check!(
        DRM_IOCTL_MODE_CURSOR2 == 0xC024_64BB,
        "MODE_CURSOR2 enc {:#x}",
        DRM_IOCTL_MODE_CURSOR2
    );
    check!(
        DRM_IOCTL_MODE_ATOMIC == 0xC038_64BC,
        "MODE_ATOMIC enc {:#x}",
        DRM_IOCTL_MODE_ATOMIC
    );

    serial_println!("[drm-uapi] DRM uAPI ABI self-test PASSED");
    Ok(())
}
