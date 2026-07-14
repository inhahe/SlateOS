//! Linux virtio-gpu DRM userspace-ABI (uAPI) definitions — the foundation of
//! GPU-accelerated rendering (roadmap §3.2 "Vulkan loader and basic GPU command
//! submission"; operator-approved Q15 option C).
//!
//! ## Why this exists
//!
//! The DRM/KMS shim ([`crate::drm::uapi`]) covers the *display* half of the
//! Linux graphics stack — enumerate connectors/CRTCs/planes, allocate dumb
//! buffers, program scanout. It is driver-agnostic (the core `drm.h` ioctls).
//! GPU *rendering* — the part Mesa's virgl OpenGL driver and its Venus Vulkan
//! driver need — goes through the **driver-specific** ioctls defined in
//! `include/uapi/drm/virtgpu_drm.h`: create a 3D rendering context, allocate
//! 3D resources (textures/buffers), transfer pixel data host⇄guest, and
//! **submit command buffers** (`EXECBUFFER`) that the host GPU executes via
//! virglrenderer. Providing this interface on our `renderD128` render node lets
//! an unmodified Mesa virgl/venus stack drive the native
//! [`crate::virtio::gpu`] device, exactly as the KMS shim lets X.Org/Wayland
//! drive scanout.
//!
//! This module is the **pure ABI layer**, mirroring the structure of
//! [`crate::drm::uapi`]: the `'d'`-magic ioctl request-number encoding (shared
//! with the core DRM ioctls), byte-exact `#[repr(C)]` mirrors of every
//! `virtgpu_drm.h` payload struct, and the `VIRTGPU_PARAM_*` / capset / blob
//! constants. It deliberately contains **no device state, no command
//! submission, and no `unsafe`** — those land in follow-up commits that route
//! the virtio-gpu render ioctls through here into the driver. Keeping the ABI
//! surface pure and exhaustively self-tested means the wiring builds on a
//! verified, byte-accurate foundation.
//!
//! ## ABI accuracy
//!
//! Every value below is fixed by the Linux UAPI header
//! `include/uapi/drm/virtgpu_drm.h` and must not be renumbered. Struct-carrying
//! ioctls encode `sizeof(struct)` in their request number, so the `#[repr(C)]`
//! layouts must be byte-identical to Linux's on a 64-bit target or real Mesa's
//! computed ioctl number never matches ours. Each struct's size is asserted
//! against its authoritative Linux value in [`self_test`], and the ioctl
//! numbers are derived from `size_of` (not hand-typed) so they stay consistent
//! with the layout, then checked against their known Linux hex values.

// This is ABI surface whose consumers land in follow-up wiring commits.
#![allow(dead_code)]

use crate::serial_println;

// ---------------------------------------------------------------------------
// ioctl request-number encoding
// ---------------------------------------------------------------------------
//
// virtio-gpu shares the DRM `'d'` magic and the generic `_IOC` encoding with
// the core DRM ioctls; we re-derive the small const helpers here (rather than
// exporting them from `uapi`) so this module is self-contained ABI surface.

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

/// Direction: userspace writes to the kernel (`_IOW`).
const IOC_WRITE: u32 = 1;
/// Direction: kernel writes to userspace (`_IOR`).
const IOC_READ: u32 = 2;

/// The DRM ioctl "magic" letter (`'d'`).
pub const DRM_IOCTL_BASE: u32 = 0x64; // b'd'

/// Base command index for driver-specific ioctls (`DRM_COMMAND_BASE`).
///
/// Core DRM ioctls occupy `0x00..0x40`; each driver's private ioctls start at
/// `0x40`. virtio-gpu's commands are `0x40 + DRM_VIRTGPU_*`.
pub const DRM_COMMAND_BASE: u32 = 0x40;

/// Encode an ioctl request number from `(dir, type, nr, size)`, matching
/// Linux's `_IOC(dir, type, nr, size)`.  Pure bit-twiddling — `const` and
/// clippy-clean; `size` is masked to [`IOC_SIZEBITS`].
const fn ioc(dir: u32, ty: u32, nr: u32, size: u32) -> u32 {
    (dir << IOC_DIRSHIFT)
        | (ty << IOC_TYPESHIFT)
        | (nr << IOC_NRSHIFT)
        | ((size & ((1 << IOC_SIZEBITS) - 1)) << IOC_SIZESHIFT)
}

/// Size of a payload struct as a `u32` for ioctl-number encoding.  Every
/// struct here is well under the 14-bit `_IOC` size field; the bound is checked
/// in [`self_test`].
#[allow(clippy::cast_possible_truncation)]
const fn struct_size<T>() -> u32 {
    core::mem::size_of::<T>() as u32
}

/// Extract the type ("magic") field from an encoded ioctl request number.
#[must_use]
pub const fn ioc_type(cmd: u32) -> u32 {
    (cmd >> IOC_TYPESHIFT) & ((1 << IOC_TYPEBITS) - 1)
}

/// Extract the command index (`nr`) field from an encoded ioctl number.
#[must_use]
pub const fn ioc_nr(cmd: u32) -> u32 {
    (cmd >> IOC_NRSHIFT) & ((1 << IOC_NRBITS) - 1)
}

/// `DRM_IOWR('d', DRM_COMMAND_BASE + nr, struct)` — bidirectional payload.
const fn iowr<T>(cmd: u32) -> u32 {
    ioc(
        IOC_READ | IOC_WRITE,
        DRM_IOCTL_BASE,
        DRM_COMMAND_BASE + cmd,
        struct_size::<T>(),
    )
}
/// `DRM_IOW('d', DRM_COMMAND_BASE + nr, struct)` — userspace → kernel payload.
const fn iow<T>(cmd: u32) -> u32 {
    ioc(
        IOC_WRITE,
        DRM_IOCTL_BASE,
        DRM_COMMAND_BASE + cmd,
        struct_size::<T>(),
    )
}

// ---------------------------------------------------------------------------
// Driver-specific command indices (`DRM_VIRTGPU_*`, virtgpu_drm.h)
// ---------------------------------------------------------------------------

/// Map a buffer object into the process for CPU access.
pub const DRM_VIRTGPU_MAP: u32 = 0x01;
/// Submit a 3D command buffer for host execution.
pub const DRM_VIRTGPU_EXECBUFFER: u32 = 0x02;
/// Query a device parameter (3D features, blob support, …).
pub const DRM_VIRTGPU_GETPARAM: u32 = 0x03;
/// Create a 3D resource (texture/buffer).
pub const DRM_VIRTGPU_RESOURCE_CREATE: u32 = 0x04;
/// Query a resource's host handle / size.
pub const DRM_VIRTGPU_RESOURCE_INFO: u32 = 0x05;
/// Transfer pixel/vertex data host → guest.
pub const DRM_VIRTGPU_TRANSFER_FROM_HOST: u32 = 0x06;
/// Transfer pixel/vertex data guest → host.
pub const DRM_VIRTGPU_TRANSFER_TO_HOST: u32 = 0x07;
/// Wait for a buffer object to become idle.
pub const DRM_VIRTGPU_WAIT: u32 = 0x08;
/// Query a capability set (virgl/venus feature capset blob).
pub const DRM_VIRTGPU_GET_CAPS: u32 = 0x09;
/// Create a blob resource (guest/host/host3d memory).
pub const DRM_VIRTGPU_RESOURCE_CREATE_BLOB: u32 = 0x0a;
/// Initialise a rendering context with explicit parameters (context-init).
pub const DRM_VIRTGPU_CONTEXT_INIT: u32 = 0x0b;

// ---------------------------------------------------------------------------
// Byte-exact `#[repr(C)]` mirrors of the virtgpu_drm.h payload structs
// ---------------------------------------------------------------------------
//
// On our 64-bit-only target `__u64`/pointer are 8 bytes, `__u32`/`__s32` are
// 4 bytes.  Field order and padding must match Linux exactly.

/// `struct drm_virtgpu_map` — request a fake mmap offset for a buffer object.
/// 16 bytes.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct DrmVirtgpuMap {
    /// Returned fake offset to pass to `mmap(2)`.
    pub offset: u64,
    /// Buffer-object (GEM) handle to map.
    pub handle: u32,
    /// Padding to 8-byte alignment (must be 0).
    pub pad: u32,
}

/// `VIRTGPU_EXECBUF_*` flags for [`DrmVirtgpuExecbuffer::flags`].
pub const VIRTGPU_EXECBUF_FENCE_FD_IN: u32 = 0x01;
pub const VIRTGPU_EXECBUF_FENCE_FD_OUT: u32 = 0x02;
pub const VIRTGPU_EXECBUF_RING_IDX: u32 = 0x04;
/// Mask of all defined execbuffer flag bits (for validation).
pub const VIRTGPU_EXECBUF_FLAGS: u32 =
    VIRTGPU_EXECBUF_FENCE_FD_IN | VIRTGPU_EXECBUF_FENCE_FD_OUT | VIRTGPU_EXECBUF_RING_IDX;

/// `struct drm_virtgpu_execbuffer` (DRM_IOCTL_VIRTGPU_EXECBUFFER) — submit a
/// command buffer plus its referenced buffer objects and sync objects.
/// 64 bytes.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct DrmVirtgpuExecbuffer {
    /// `VIRTGPU_EXECBUF_*` flags.
    pub flags: u32,
    /// Size of the command buffer in bytes.
    pub size: u32,
    /// Userspace pointer to the command buffer (`__u64` void*).
    pub command: u64,
    /// Userspace pointer to an array of referenced BO handles.
    pub bo_handles: u64,
    /// Number of entries in `bo_handles`.
    pub num_bo_handles: u32,
    /// In/out fence fd (see `FENCE_FD_IN`/`FENCE_FD_OUT`).
    pub fence_fd: i32,
    /// Command-ring index (see `RING_IDX`).
    pub ring_idx: u32,
    /// Size of a single syncobj descriptor.
    pub syncobj_stride: u32,
    /// Number of input syncobjs.
    pub num_in_syncobjs: u32,
    /// Number of output syncobjs.
    pub num_out_syncobjs: u32,
    /// Userspace pointer to input syncobj descriptors.
    pub in_syncobjs: u64,
    /// Userspace pointer to output syncobj descriptors.
    pub out_syncobjs: u64,
}

// `VIRTGPU_PARAM_*` ids for `DRM_IOCTL_VIRTGPU_GETPARAM`.
/// Device supports 3D (virgl) rendering.
pub const VIRTGPU_PARAM_3D_FEATURES: u64 = 1;
/// The capset-query ABI fix is present.
pub const VIRTGPU_PARAM_CAPSET_QUERY_FIX: u64 = 2;
/// Device supports blob resources.
pub const VIRTGPU_PARAM_RESOURCE_BLOB: u64 = 3;
/// Device supports host-visible blob memory.
pub const VIRTGPU_PARAM_HOST_VISIBLE: u64 = 4;
/// Device supports cross-device sharing (dma-buf).
pub const VIRTGPU_PARAM_CROSS_DEVICE: u64 = 5;
/// Device supports explicit context init (multiple context types).
pub const VIRTGPU_PARAM_CONTEXT_INIT: u64 = 6;
/// Bitmask of capset ids the device supports.
pub const VIRTGPU_PARAM_SUPPORTED_CAPSET_IDS: u64 = 7;
/// Device supports an explicit debug name on context init.
pub const VIRTGPU_PARAM_EXPLICIT_DEBUG_NAME: u64 = 8;

/// `struct drm_virtgpu_getparam` — query one `VIRTGPU_PARAM_*` value.
/// 16 bytes.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct DrmVirtgpuGetparam {
    /// `VIRTGPU_PARAM_*` id being queried.
    pub param: u64,
    /// Userspace pointer to a `__u64` that receives the value.
    pub value: u64,
}

/// `struct drm_virtgpu_resource_create` — allocate a 3D resource.
/// 56 bytes (14 × `__u32`).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct DrmVirtgpuResourceCreate {
    /// Gallium `pipe_texture_target`.
    pub target: u32,
    /// Gallium `pipe_format`.
    pub format: u32,
    /// Bind flags (`VIRGL_BIND_*`).
    pub bind: u32,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Depth (3D textures).
    pub depth: u32,
    /// Array size (array textures).
    pub array_size: u32,
    /// Last mip level.
    pub last_level: u32,
    /// Sample count (MSAA).
    pub nr_samples: u32,
    /// Creation flags.
    pub flags: u32,
    /// Existing BO handle to bind (0 = allocate a new one).
    pub bo_handle: u32,
    /// Returned host resource handle.
    pub res_handle: u32,
    /// Size of the underlying BO in bytes.
    pub size: u32,
    /// Returned row stride in bytes.
    pub stride: u32,
}

/// `struct drm_virtgpu_resource_info` — query a resource's host handle+size.
/// 16 bytes.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct DrmVirtgpuResourceInfo {
    /// BO handle to query.
    pub bo_handle: u32,
    /// Returned host resource handle.
    pub res_handle: u32,
    /// Returned size in bytes.
    pub size: u32,
    /// Returned blob-memory type (0 for classic resources).
    pub blob_mem: u32,
}

/// `struct drm_virtgpu_3d_box` — a sub-region of a resource for transfers.
/// 24 bytes (6 × `__u32`).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct DrmVirtgpu3dBox {
    pub x: u32,
    pub y: u32,
    pub z: u32,
    pub w: u32,
    pub h: u32,
    pub d: u32,
}

/// `struct drm_virtgpu_3d_transfer_to_host` — copy guest data → host resource.
/// 44 bytes.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct DrmVirtgpu3dTransferToHost {
    /// BO handle (source).
    pub bo_handle: u32,
    /// Destination sub-box in the host resource.
    pub r#box: DrmVirtgpu3dBox,
    /// Mip level.
    pub level: u32,
    /// Byte offset into the BO.
    pub offset: u32,
    /// Row stride in bytes.
    pub stride: u32,
    /// Layer (array/3D) stride in bytes.
    pub layer_stride: u32,
}

/// `struct drm_virtgpu_3d_transfer_from_host` — copy host resource → guest.
/// 44 bytes (identical layout to the to-host form).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct DrmVirtgpu3dTransferFromHost {
    /// BO handle (destination).
    pub bo_handle: u32,
    /// Source sub-box in the host resource.
    pub r#box: DrmVirtgpu3dBox,
    /// Mip level.
    pub level: u32,
    /// Byte offset into the BO.
    pub offset: u32,
    /// Row stride in bytes.
    pub stride: u32,
    /// Layer (array/3D) stride in bytes.
    pub layer_stride: u32,
}

/// `VIRTGPU_WAIT_NOWAIT` — return `-EBUSY` immediately if the BO is busy.
pub const VIRTGPU_WAIT_NOWAIT: u32 = 1;

/// `struct drm_virtgpu_3d_wait` — wait for a BO to become idle. 8 bytes.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct DrmVirtgpu3dWait {
    /// BO handle to wait on (0 is invalid).
    pub handle: u32,
    /// `VIRTGPU_WAIT_*` flags.
    pub flags: u32,
}

/// `struct drm_virtgpu_get_caps` — fetch a capability-set blob. 24 bytes.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct DrmVirtgpuGetCaps {
    /// Capset id (`VIRTGPU_DRM_CAPSET_*`).
    pub cap_set_id: u32,
    /// Capset version.
    pub cap_set_ver: u32,
    /// Userspace pointer to the caps-blob destination buffer.
    pub addr: u64,
    /// Size of the destination buffer in bytes.
    pub size: u32,
    /// Padding to 8-byte alignment (must be 0).
    pub pad: u32,
}

// Known capset ids (`VIRTGPU_DRM_CAPSET_*`).
/// virgl (OpenGL / Gallium) capset, version 1.
pub const VIRTGPU_DRM_CAPSET_VIRGL: u32 = 1;
/// virgl2 (OpenGL / Gallium) capset, version 2.
pub const VIRTGPU_DRM_CAPSET_VIRGL2: u32 = 2;
/// gfxstream (Vulkan) capset.
pub const VIRTGPU_DRM_CAPSET_GFXSTREAM_VULKAN: u32 = 3;
/// venus (Vulkan) capset.
pub const VIRTGPU_DRM_CAPSET_VENUS: u32 = 4;
/// cross-domain (Wayland proxy) capset.
pub const VIRTGPU_DRM_CAPSET_CROSS_DOMAIN: u32 = 5;
/// drm native-context capset.
pub const VIRTGPU_DRM_CAPSET_DRM: u32 = 6;

// `VIRTGPU_BLOB_*` constants for [`DrmVirtgpuResourceCreateBlob`].
/// Guest-side memory only.
pub const VIRTGPU_BLOB_MEM_GUEST: u32 = 0x0001;
/// Host-3D memory.
pub const VIRTGPU_BLOB_MEM_HOST3D: u32 = 0x0002;
/// Host-3D memory backed by guest pages.
pub const VIRTGPU_BLOB_MEM_HOST3D_GUEST: u32 = 0x0003;
/// Blob is CPU-mappable.
pub const VIRTGPU_BLOB_FLAG_USE_MAPPABLE: u32 = 0x0001;
/// Blob is shareable (exportable as dma-buf).
pub const VIRTGPU_BLOB_FLAG_USE_SHAREABLE: u32 = 0x0002;
/// Blob is usable cross-device.
pub const VIRTGPU_BLOB_FLAG_USE_CROSS_DEVICE: u32 = 0x0004;

/// `struct drm_virtgpu_resource_create_blob` — allocate a blob resource.
/// 48 bytes.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct DrmVirtgpuResourceCreateBlob {
    /// `VIRTGPU_BLOB_MEM_*` memory type.
    pub blob_mem: u32,
    /// `VIRTGPU_BLOB_FLAG_*` usage flags.
    pub blob_flags: u32,
    /// Returned BO handle.
    pub bo_handle: u32,
    /// Returned host resource handle.
    pub res_handle: u32,
    /// Blob size in bytes.
    pub size: u64,
    /// Opaque host blob id (host3d).
    pub blob_id: u64,
    /// Userspace pointer to a creation command buffer (host3d).
    pub cmd: u64,
    /// Size of the creation command buffer.
    pub cmd_size: u32,
    /// Padding to 8-byte alignment (must be 0).
    pub pad: u32,
}

/// `struct drm_virtgpu_context_set_param` — one `(param, value)` pair for
/// context init. 16 bytes.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct DrmVirtgpuContextSetParam {
    /// `VIRTGPU_CONTEXT_PARAM_*` id.
    pub param: u64,
    /// Parameter value.
    pub value: u64,
}

// `VIRTGPU_CONTEXT_PARAM_*` ids.
/// Selects the context's capset (virgl/venus/…).
pub const VIRTGPU_CONTEXT_PARAM_CAPSET_ID: u64 = 0x0001;
/// Sets the number of command rings.
pub const VIRTGPU_CONTEXT_PARAM_NUM_RINGS: u64 = 0x0002;
/// Bitmask limiting which rings may carry poll-able fences.
pub const VIRTGPU_CONTEXT_PARAM_POLL_RINGS_MASK: u64 = 0x0003;
/// Sets an explicit debug name for the context.
pub const VIRTGPU_CONTEXT_PARAM_DEBUG_NAME: u64 = 0x0004;

/// `struct drm_virtgpu_context_init` — create a context from a param array.
/// 16 bytes.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct DrmVirtgpuContextInit {
    /// Number of `DrmVirtgpuContextSetParam` entries in `ctx_set_params`.
    pub num_params: u32,
    /// Padding to 8-byte alignment (must be 0).
    pub pad: u32,
    /// Userspace pointer to the param array.
    pub ctx_set_params: u64,
}

// ---------------------------------------------------------------------------
// Encoded ioctl request numbers (`DRM_IOCTL_VIRTGPU_*`)
// ---------------------------------------------------------------------------

pub const DRM_IOCTL_VIRTGPU_MAP: u32 = iowr::<DrmVirtgpuMap>(DRM_VIRTGPU_MAP);
pub const DRM_IOCTL_VIRTGPU_EXECBUFFER: u32 = iow::<DrmVirtgpuExecbuffer>(DRM_VIRTGPU_EXECBUFFER);
pub const DRM_IOCTL_VIRTGPU_GETPARAM: u32 = iowr::<DrmVirtgpuGetparam>(DRM_VIRTGPU_GETPARAM);
pub const DRM_IOCTL_VIRTGPU_RESOURCE_CREATE: u32 =
    iowr::<DrmVirtgpuResourceCreate>(DRM_VIRTGPU_RESOURCE_CREATE);
pub const DRM_IOCTL_VIRTGPU_RESOURCE_INFO: u32 =
    iowr::<DrmVirtgpuResourceInfo>(DRM_VIRTGPU_RESOURCE_INFO);
pub const DRM_IOCTL_VIRTGPU_TRANSFER_FROM_HOST: u32 =
    iowr::<DrmVirtgpu3dTransferFromHost>(DRM_VIRTGPU_TRANSFER_FROM_HOST);
pub const DRM_IOCTL_VIRTGPU_TRANSFER_TO_HOST: u32 =
    iowr::<DrmVirtgpu3dTransferToHost>(DRM_VIRTGPU_TRANSFER_TO_HOST);
pub const DRM_IOCTL_VIRTGPU_WAIT: u32 = iowr::<DrmVirtgpu3dWait>(DRM_VIRTGPU_WAIT);
pub const DRM_IOCTL_VIRTGPU_GET_CAPS: u32 = iowr::<DrmVirtgpuGetCaps>(DRM_VIRTGPU_GET_CAPS);
pub const DRM_IOCTL_VIRTGPU_RESOURCE_CREATE_BLOB: u32 =
    iowr::<DrmVirtgpuResourceCreateBlob>(DRM_VIRTGPU_RESOURCE_CREATE_BLOB);
pub const DRM_IOCTL_VIRTGPU_CONTEXT_INIT: u32 =
    iowr::<DrmVirtgpuContextInit>(DRM_VIRTGPU_CONTEXT_INIT);

// ---------------------------------------------------------------------------
// Pure `GETPARAM` policy
// ---------------------------------------------------------------------------

/// The value our virtio-gpu render node reports for a `VIRTGPU_PARAM_*` query,
/// or `None` for an unknown parameter (the ioctl returns `EINVAL`).
///
/// This is the *pure policy* the `GETPARAM` handler consults. Per the Q18
/// resolution (design-decisions §59, operator option B) it reports **honestly**:
/// the plain virtio-gpu device our headless CI exposes advertises **no**
/// `VIRTIO_GPU_F_VIRGL` (observed feature bits `0x30000002` — EDID only), so
/// `3D_FEATURES` is `0` and every other capability that presupposes a working
/// render/blob path is `0` too. `CAPSET_QUERY_FIX` stays `1` because that flag
/// describes the *capset-query ABI shape* (which we implement correctly — it
/// simply returns "no capsets"), not a 3D capability. Advertising a feature we
/// cannot service would make Mesa take a path the driver can't complete; honest
/// zeros make it fall back cleanly. When a virgl-capable backend + the Mesa port
/// land (deferred half of Q18) these flip to reflect real capability.
#[must_use]
pub fn param_value(param: u64) -> Option<u64> {
    match param {
        // Honest no-3D reporting until a virgl backend exists (§59).
        VIRTGPU_PARAM_3D_FEATURES => Some(0),
        // The capset-query ioctl behaves per-ABI (returns "no capsets"); this
        // flag is about that ABI shape, not 3D, so it is truthfully 1.
        VIRTGPU_PARAM_CAPSET_QUERY_FIX => Some(1),
        VIRTGPU_PARAM_RESOURCE_BLOB => Some(0),
        VIRTGPU_PARAM_HOST_VISIBLE => Some(0),
        VIRTGPU_PARAM_CROSS_DEVICE => Some(0),
        VIRTGPU_PARAM_CONTEXT_INIT => Some(0),
        VIRTGPU_PARAM_SUPPORTED_CAPSET_IDS => Some(0),
        VIRTGPU_PARAM_EXPLICIT_DEBUG_NAME => Some(0),
        _ => None,
    }
}

/// Number of capability sets the render node advertises via `GET_CAPS`.
///
/// Zero until a virgl/venus backend exists — the honest counterpart to
/// [`param_value`]'s `3D_FEATURES = 0`. A `GET_CAPS` for any capset id therefore
/// fails with `EINVAL` (matching Linux's virtio-gpu, which rejects a capset
/// query when `num_capsets == 0` or the id is unknown).
pub const NUM_CAPSETS: u32 = 0;

/// Whether the render node can service the given capset id. Always `false`
/// today (see [`NUM_CAPSETS`]); provided so the ioctl handler expresses intent
/// rather than hard-coding `false`.
#[must_use]
pub const fn capset_supported(_cap_set_id: u32) -> bool {
    // No capsets advertised until a virgl backend lands (§59).
    false
}

// ---------------------------------------------------------------------------
// Self-test — verify the ABI surface byte-for-byte against Linux
// ---------------------------------------------------------------------------

/// Exhaustive ABI self-test: every struct size and ioctl number against its
/// authoritative Linux `virtgpu_drm.h` value, plus the `GETPARAM` policy.
///
/// # Errors
/// Returns [`crate::error::KernelError::InternalError`] on the first mismatch.
pub fn self_test() -> crate::error::KernelResult<()> {
    use crate::error::KernelError;
    use core::mem::size_of;

    macro_rules! check {
        ($cond:expr, $($arg:tt)*) => {
            if !($cond) {
                serial_println!("[virtgpu-uapi]   FAIL: {}", format_args!($($arg)*));
                return Err(KernelError::InternalError);
            }
        };
    }

    // --- byte-exact struct layouts vs Linux virtgpu_drm.h (64-bit) -------
    check!(size_of::<DrmVirtgpuMap>() == 16, "drm_virtgpu_map size {}", size_of::<DrmVirtgpuMap>());
    check!(
        size_of::<DrmVirtgpuExecbuffer>() == 64,
        "drm_virtgpu_execbuffer size {}",
        size_of::<DrmVirtgpuExecbuffer>()
    );
    check!(
        size_of::<DrmVirtgpuGetparam>() == 16,
        "drm_virtgpu_getparam size {}",
        size_of::<DrmVirtgpuGetparam>()
    );
    check!(
        size_of::<DrmVirtgpuResourceCreate>() == 56,
        "drm_virtgpu_resource_create size {}",
        size_of::<DrmVirtgpuResourceCreate>()
    );
    check!(
        size_of::<DrmVirtgpuResourceInfo>() == 16,
        "drm_virtgpu_resource_info size {}",
        size_of::<DrmVirtgpuResourceInfo>()
    );
    check!(
        size_of::<DrmVirtgpu3dBox>() == 24,
        "drm_virtgpu_3d_box size {}",
        size_of::<DrmVirtgpu3dBox>()
    );
    check!(
        size_of::<DrmVirtgpu3dTransferToHost>() == 44,
        "drm_virtgpu_3d_transfer_to_host size {}",
        size_of::<DrmVirtgpu3dTransferToHost>()
    );
    check!(
        size_of::<DrmVirtgpu3dTransferFromHost>() == 44,
        "drm_virtgpu_3d_transfer_from_host size {}",
        size_of::<DrmVirtgpu3dTransferFromHost>()
    );
    check!(
        size_of::<DrmVirtgpu3dWait>() == 8,
        "drm_virtgpu_3d_wait size {}",
        size_of::<DrmVirtgpu3dWait>()
    );
    check!(
        size_of::<DrmVirtgpuGetCaps>() == 24,
        "drm_virtgpu_get_caps size {}",
        size_of::<DrmVirtgpuGetCaps>()
    );
    check!(
        size_of::<DrmVirtgpuResourceCreateBlob>() == 48,
        "drm_virtgpu_resource_create_blob size {}",
        size_of::<DrmVirtgpuResourceCreateBlob>()
    );
    check!(
        size_of::<DrmVirtgpuContextSetParam>() == 16,
        "drm_virtgpu_context_set_param size {}",
        size_of::<DrmVirtgpuContextSetParam>()
    );
    check!(
        size_of::<DrmVirtgpuContextInit>() == 16,
        "drm_virtgpu_context_init size {}",
        size_of::<DrmVirtgpuContextInit>()
    );

    // Every payload must fit the 14-bit `_IOC` size field.
    check!(
        size_of::<DrmVirtgpuExecbuffer>() < (1 << IOC_SIZEBITS),
        "execbuffer exceeds _IOC size field"
    );

    // --- ioctl encodings against known Linux hex values ------------------
    check!(
        DRM_IOCTL_VIRTGPU_MAP == 0xC010_6441,
        "DRM_IOCTL_VIRTGPU_MAP enc {:#x}",
        DRM_IOCTL_VIRTGPU_MAP
    );
    check!(
        DRM_IOCTL_VIRTGPU_EXECBUFFER == 0x4040_6442,
        "DRM_IOCTL_VIRTGPU_EXECBUFFER enc {:#x}",
        DRM_IOCTL_VIRTGPU_EXECBUFFER
    );
    check!(
        DRM_IOCTL_VIRTGPU_GETPARAM == 0xC010_6443,
        "DRM_IOCTL_VIRTGPU_GETPARAM enc {:#x}",
        DRM_IOCTL_VIRTGPU_GETPARAM
    );
    check!(
        DRM_IOCTL_VIRTGPU_RESOURCE_CREATE == 0xC038_6444,
        "DRM_IOCTL_VIRTGPU_RESOURCE_CREATE enc {:#x}",
        DRM_IOCTL_VIRTGPU_RESOURCE_CREATE
    );
    check!(
        DRM_IOCTL_VIRTGPU_RESOURCE_INFO == 0xC010_6445,
        "DRM_IOCTL_VIRTGPU_RESOURCE_INFO enc {:#x}",
        DRM_IOCTL_VIRTGPU_RESOURCE_INFO
    );
    check!(
        DRM_IOCTL_VIRTGPU_TRANSFER_FROM_HOST == 0xC02C_6446,
        "DRM_IOCTL_VIRTGPU_TRANSFER_FROM_HOST enc {:#x}",
        DRM_IOCTL_VIRTGPU_TRANSFER_FROM_HOST
    );
    check!(
        DRM_IOCTL_VIRTGPU_TRANSFER_TO_HOST == 0xC02C_6447,
        "DRM_IOCTL_VIRTGPU_TRANSFER_TO_HOST enc {:#x}",
        DRM_IOCTL_VIRTGPU_TRANSFER_TO_HOST
    );
    check!(
        DRM_IOCTL_VIRTGPU_WAIT == 0xC008_6448,
        "DRM_IOCTL_VIRTGPU_WAIT enc {:#x}",
        DRM_IOCTL_VIRTGPU_WAIT
    );
    check!(
        DRM_IOCTL_VIRTGPU_GET_CAPS == 0xC018_6449,
        "DRM_IOCTL_VIRTGPU_GET_CAPS enc {:#x}",
        DRM_IOCTL_VIRTGPU_GET_CAPS
    );
    check!(
        DRM_IOCTL_VIRTGPU_RESOURCE_CREATE_BLOB == 0xC030_644A,
        "DRM_IOCTL_VIRTGPU_RESOURCE_CREATE_BLOB enc {:#x}",
        DRM_IOCTL_VIRTGPU_RESOURCE_CREATE_BLOB
    );
    check!(
        DRM_IOCTL_VIRTGPU_CONTEXT_INIT == 0xC010_644B,
        "DRM_IOCTL_VIRTGPU_CONTEXT_INIT enc {:#x}",
        DRM_IOCTL_VIRTGPU_CONTEXT_INIT
    );

    // The ioctl "magic" must be `'d'` and the nr must be in the driver range.
    check!(ioc_type(DRM_IOCTL_VIRTGPU_EXECBUFFER) == DRM_IOCTL_BASE, "execbuffer magic not 'd'");
    check!(
        ioc_nr(DRM_IOCTL_VIRTGPU_EXECBUFFER) == DRM_COMMAND_BASE + DRM_VIRTGPU_EXECBUFFER,
        "execbuffer nr {:#x}",
        ioc_nr(DRM_IOCTL_VIRTGPU_EXECBUFFER)
    );

    // --- GETPARAM policy (honest no-3D reporting, §59) -------------------
    check!(param_value(VIRTGPU_PARAM_3D_FEATURES) == Some(0), "3D_FEATURES must be 0 (no virgl)");
    check!(param_value(VIRTGPU_PARAM_CAPSET_QUERY_FIX) == Some(1), "CAPSET_QUERY_FIX param");
    check!(param_value(VIRTGPU_PARAM_RESOURCE_BLOB) == Some(0), "RESOURCE_BLOB param");
    check!(param_value(VIRTGPU_PARAM_CONTEXT_INIT) == Some(0), "CONTEXT_INIT param");
    check!(param_value(0xDEAD_BEEF).is_none(), "unknown param should be None");

    // --- GET_CAPS policy: no capsets advertised until a virgl backend ---
    check!(NUM_CAPSETS == 0, "NUM_CAPSETS must be 0 until virgl lands");
    check!(!capset_supported(VIRTGPU_DRM_CAPSET_VIRGL), "virgl capset must be unsupported");
    check!(!capset_supported(VIRTGPU_DRM_CAPSET_VENUS), "venus capset must be unsupported");

    serial_println!("[virtgpu-uapi] virtio-gpu DRM uAPI ABI self-test PASSED");
    Ok(())
}
