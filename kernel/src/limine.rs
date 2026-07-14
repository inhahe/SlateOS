//! Minimal Limine boot protocol bindings.
//!
//! Hand-written against the Limine protocol specification rather than
//! depending on the `limine` crate (which requires nightly Rust).
//! This gives us stable-Rust compatibility and full control.
//!
//! ## Protocol Overview
//!
//! The kernel declares static request objects in `.requests` linker
//! sections, bracketed by start/end markers.  Each request has:
//! - A 4×`u64` ID (first two are common magic, last two are feature-specific)
//! - A revision number
//! - A pointer to a response struct (filled by the bootloader)
//!
//! The bootloader scans the `.requests` section, fills in the response
//! pointers, and jumps to the kernel entry point.
//!
//! ## Reference
//!
//! <https://github.com/limine-bootloader/limine/blob/v8.x/PROTOCOL.md>

use core::ptr;

// ---------------------------------------------------------------------------
// Magic values
// ---------------------------------------------------------------------------

/// Common magic prefix shared by all Limine requests.
const COMMON_MAGIC: [u64; 2] = [0xc7b1_dd30_df4c_8b88, 0x0a82_e883_a194_f07b];

// ---------------------------------------------------------------------------
// Base revision
// ---------------------------------------------------------------------------

/// Protocol base revision marker.
///
/// Placed in the `.requests` section.  The bootloader writes the actual
/// supported revision into `revision` (replacing our requested value).
/// If the bootloader doesn't support our revision, it leaves the field
/// unchanged — so we detect support by checking if the value changed
/// from a sentinel.
#[repr(C)]
pub struct BaseRevision {
    magic0: u64,
    magic1: u64,
    /// Set to the requested revision. The bootloader overwrites this
    /// with the actual revision if supported, or leaves it unchanged.
    revision: u64,
}

// SAFETY: BaseRevision is a plain data struct with no interior mutability
// once the bootloader is done writing to it (which happens before we run).
unsafe impl Sync for BaseRevision {}

impl BaseRevision {
    /// Create a base revision request for the given protocol revision.
    #[must_use]
    pub const fn with_revision(rev: u64) -> Self {
        Self {
            magic0: 0xf956_2b2d_5c95_a6c8,
            magic1: 0x6a7b_3849_4453_6bdc,
            revision: rev,
        }
    }

    /// Check if the bootloader supports our requested revision.
    ///
    /// The bootloader sets `revision` to 0 if it supports our revision.
    /// If it doesn't touch it, the original value remains.
    #[must_use]
    pub fn is_supported(&self) -> bool {
        // The bootloader sets this to 0 to indicate support.
        // SAFETY: volatile read because the bootloader writes this before
        // we read it, and we don't want the compiler to optimize it away.
        unsafe { ptr::read_volatile(&raw const self.revision) == 0 }
    }
}

// ---------------------------------------------------------------------------
// Requests start/end markers
// ---------------------------------------------------------------------------

/// Marks the beginning of the requests section.
#[repr(C)]
pub struct RequestsStartMarker {
    magic: [u64; 4],
}

// SAFETY: Constant data, no interior mutability.
unsafe impl Sync for RequestsStartMarker {}

impl RequestsStartMarker {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            magic: [
                0xf6b8_f4b3_9de7_d1ae,
                0xfab9_1a69_40fc_b9cf,
                0x785c_6ed0_15d3_e316,
                0x181e_920a_7852_b9d9,
            ],
        }
    }
}

/// Marks the end of the requests section.
#[repr(C)]
pub struct RequestsEndMarker {
    magic: [u64; 2],
}

// SAFETY: Constant data, no interior mutability.
unsafe impl Sync for RequestsEndMarker {}

impl RequestsEndMarker {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            magic: [0xadc0_e053_1bb1_0d03, 0x9572_709f_3176_4c62],
        }
    }
}

// ---------------------------------------------------------------------------
// Generic request structure
// ---------------------------------------------------------------------------

/// A Limine protocol request.
///
/// `R` is the response type.  The bootloader fills `response` with a
/// pointer to the response struct if it understands the request.
#[repr(C)]
pub struct LimineRequest<R> {
    /// 4×`u64` request ID: `[common_magic_0, common_magic_1, feature_id_0, feature_id_1]`.
    id: [u64; 4],
    /// Protocol revision for this feature.
    revision: u64,
    /// Pointer to response.  Null until the bootloader fills it in.
    response: *const R,
}

// SAFETY: The response pointer is written by the bootloader before we run,
// and we only read it afterward.  No mutation after boot.
unsafe impl<R> Sync for LimineRequest<R> {}

impl<R> LimineRequest<R> {
    /// Create a new request with the given feature-specific ID.
    const fn new(feature_id: [u64; 2]) -> Self {
        Self {
            id: [COMMON_MAGIC[0], COMMON_MAGIC[1], feature_id[0], feature_id[1]],
            revision: 0,
            response: ptr::null(),
        }
    }

    /// Get the response, if the bootloader provided one.
    ///
    /// Returns `None` if the bootloader didn't understand this request.
    pub fn response(&self) -> Option<&'static R> {
        // SAFETY: volatile read because the bootloader writes the pointer
        // before we execute.  If non-null, it points to a valid response
        // struct that lives for the entire kernel lifetime.
        let ptr = unsafe { ptr::read_volatile(&raw const self.response) };
        if ptr.is_null() {
            None
        } else {
            // SAFETY: The bootloader guarantees the pointer is valid and
            // the response struct is in memory that won't be reclaimed
            // (it's in bootloader-reclaimable or reserved memory that we
            // haven't freed yet).
            Some(unsafe { &*ptr })
        }
    }
}

// ---------------------------------------------------------------------------
// Memory map
// ---------------------------------------------------------------------------

/// Memory map entry types.
pub mod memmap_type {
    pub const USABLE: u64 = 0;
    pub const RESERVED: u64 = 1;
    pub const ACPI_RECLAIMABLE: u64 = 2;
    pub const ACPI_NVS: u64 = 3;
    pub const BAD_MEMORY: u64 = 4;
    pub const BOOTLOADER_RECLAIMABLE: u64 = 5;
    pub const EXECUTABLE_AND_MODULES: u64 = 6;
    pub const FRAMEBUFFER: u64 = 7;
}

/// A single memory map entry.
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct MemmapEntry {
    pub base: u64,
    pub length: u64,
    pub type_: u64,
}

/// Memory map response from the bootloader.
#[repr(C)]
pub struct MemmapResponse {
    pub revision: u64,
    entry_count: u64,
    entries_ptr: *const *const MemmapEntry,
}

impl MemmapResponse {
    /// Get the memory map entries as a slice.
    #[allow(clippy::transmute_ptr_to_ref)]
    pub fn entries(&self) -> &[&MemmapEntry] {
        if self.entries_ptr.is_null() || self.entry_count == 0 {
            return &[];
        }
        // SAFETY: The bootloader guarantees entry_count entries at
        // entries_ptr, each pointing to a valid MemmapEntry.
        // *const *const MemmapEntry → &[*const MemmapEntry] → &[&MemmapEntry]
        // This transmute is safe because *const T and &T have the same
        // layout, and the bootloader guarantees all pointers are valid
        // and the referenced data lives for the entire kernel lifetime.
        //
        // cast_possible_truncation: This is a 64-bit target (x86_64-unknown-none)
        // so u64 → usize is lossless.
        #[allow(clippy::cast_possible_truncation)]
        unsafe {
            core::slice::from_raw_parts(
                self.entries_ptr.cast::<&MemmapEntry>(),
                self.entry_count as usize,
            )
        }
    }
}

/// Create a memory map request.
impl LimineRequest<MemmapResponse> {
    pub const MEMMAP: Self = Self::new([0x67cf_3d9d_378a_806f, 0xe304_acdf_c50c_3c62]);
}

// ---------------------------------------------------------------------------
// HHDM (Higher Half Direct Map)
// ---------------------------------------------------------------------------

/// HHDM response — provides the virtual address offset for the direct map.
#[repr(C)]
pub struct HhdmResponse {
    pub revision: u64,
    /// Virtual address = `offset + physical_address`.
    pub offset: u64,
}

impl LimineRequest<HhdmResponse> {
    pub const HHDM: Self = Self::new([0x48dc_f1cb_8ad2_b852, 0x6398_4e95_9a98_244b]);
}

// ---------------------------------------------------------------------------
// Framebuffer
// ---------------------------------------------------------------------------

/// A single framebuffer descriptor.
#[repr(C)]
pub struct Framebuffer {
    pub address: *mut u8,
    pub width: u64,
    pub height: u64,
    pub pitch: u64,
    pub bpp: u16,
    pub memory_model: u8,
    pub red_mask_size: u8,
    pub red_mask_shift: u8,
    pub green_mask_size: u8,
    pub green_mask_shift: u8,
    pub blue_mask_size: u8,
    pub blue_mask_shift: u8,
}

/// Framebuffer response from the bootloader.
#[repr(C)]
pub struct FramebufferResponse {
    pub revision: u64,
    framebuffer_count: u64,
    framebuffers_ptr: *const *const Framebuffer,
}

impl FramebufferResponse {
    /// Get the framebuffers as a slice.
    #[allow(clippy::transmute_ptr_to_ref)]
    pub fn framebuffers(&self) -> &[&Framebuffer] {
        if self.framebuffers_ptr.is_null() || self.framebuffer_count == 0 {
            return &[];
        }
        // SAFETY: Same guarantees as MemmapResponse::entries — the bootloader
        // provides valid pointers to Framebuffer structs that live for the
        // entire kernel lifetime. *const T and &T have identical layout.
        //
        // cast_possible_truncation: 64-bit target, u64 → usize is lossless.
        #[allow(clippy::cast_possible_truncation)]
        unsafe {
            core::slice::from_raw_parts(
                self.framebuffers_ptr.cast::<&Framebuffer>(),
                self.framebuffer_count as usize,
            )
        }
    }
}

impl LimineRequest<FramebufferResponse> {
    pub const FRAMEBUFFER: Self = Self::new([0x9d58_27dc_d881_dd75, 0xa314_8604_f6fa_b11b]);
}

// ---------------------------------------------------------------------------
// RSDP (Root System Description Pointer)
// ---------------------------------------------------------------------------

/// RSDP response from the bootloader.
///
/// Contains a virtual address pointing to the ACPI RSDP structure.
/// The RSDP is in HHDM-mapped memory and remains valid for the
/// entire kernel lifetime.
#[repr(C)]
pub struct RsdpResponse {
    pub revision: u64,
    /// Virtual address of the RSDP (already mapped via HHDM).
    pub address: u64,
}

impl LimineRequest<RsdpResponse> {
    /// Limine RSDP feature request ID.
    ///
    /// Reference: Limine Protocol v8.x, RSDP Feature.
    pub const RSDP: Self = Self::new([0x71ba_7686_3cc5_5f63, 0xb264_4a48_c516_a487]);
}

// ---------------------------------------------------------------------------
// Kernel File (for symbol table access)
// ---------------------------------------------------------------------------

/// A Limine file descriptor.
#[repr(C)]
pub struct LimineFile {
    pub revision: u64,
    /// Virtual address of the file contents (in HHDM).
    pub address: *const u8,
    /// Size in bytes.
    pub size: u64,
    /// Null-terminated path string.
    pub path: *const u8,
    /// Null-terminated cmdline string.
    pub cmdline: *const u8,
    /// Media type.
    pub media_type: u32,
    _unused: u32,
    /// TFTP info / partition info.
    pub tftp_ip: u32,
    pub tftp_port: u32,
    pub partition_index: u32,
    pub mbr_disk_id: u32,
    pub gpt_disk_uuid: [u8; 16],
    pub gpt_part_uuid: [u8; 16],
    pub part_uuid: [u8; 16],
}

/// Kernel File response — provides access to the raw kernel ELF binary.
#[repr(C)]
pub struct KernelFileResponse {
    pub revision: u64,
    /// Pointer to the kernel file descriptor.
    pub kernel_file: *const LimineFile,
}

impl LimineRequest<KernelFileResponse> {
    /// Limine Kernel File feature request ID.
    // NOTE: the second feature-id word MUST be 0x31eb_5d1c_5ff2_3b69, not the
    // 0x31eb_5d10_c871_c930 value used previously — that earlier value was a
    // typo that never matched Limine's Kernel/Executable-File feature magic, so
    // the response was silently always null (breaking both the boot cmdline and
    // kernel-file symbolization). Matches LIMINE_{KERNEL,EXECUTABLE}_FILE_REQUEST
    // in limine.h (Limine 8.7.0).
    pub const KERNEL_FILE: Self = Self::new([0xad97_e90e_83f1_ed67, 0x31eb_5d1c_5ff2_3b69]);
}
