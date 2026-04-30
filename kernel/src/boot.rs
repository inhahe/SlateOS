//! Limine boot protocol integration.
//!
//! The Limine bootloader communicates with the kernel through a
//! request/response mechanism.  The kernel places static request
//! objects in a special linker section (`.requests`).  Before
//! transferring control, Limine scans these sections, fills in the
//! response pointers, and jumps to the kernel entry point.
//!
//! ## What Limine gives us
//!
//! - 64-bit long mode, paging enabled
//! - Identity mapping of physical memory (first N GiB)
//! - Higher Half Direct Map (HHDM) at a bootloader-chosen offset
//! - Kernel mapped at its ELF load address (higher half)
//! - GDT with flat segments (we replace it with our own)
//! - Interrupts disabled
//! - BSS zeroed
//! - Stack provided (we switch to our own later)
//!
//! ## References
//!
//! - Limine protocol spec: <https://github.com/limine-bootloader/limine/blob/trunk/PROTOCOL.md>

use crate::limine::{
    BaseRevision, FramebufferResponse, HhdmResponse, LimineRequest, MemmapEntry,
    MemmapResponse, RequestsEndMarker, RequestsStartMarker, memmap_type,
};
use crate::serial_println;

// ---------------------------------------------------------------------------
// Protocol markers and requests
// ---------------------------------------------------------------------------

/// Start-of-requests marker.  Must be the first item in `.requests_start_marker`.
#[used]
#[unsafe(link_section = ".requests_start_marker")]
static REQUESTS_START: RequestsStartMarker = RequestsStartMarker::new();

/// End-of-requests marker.  Must be the last item in `.requests_end_marker`.
#[used]
#[unsafe(link_section = ".requests_end_marker")]
static REQUESTS_END: RequestsEndMarker = RequestsEndMarker::new();

/// Protocol base revision.  Tells Limine which protocol version we speak.
#[used]
#[unsafe(link_section = ".requests")]
static BASE_REVISION: BaseRevision = BaseRevision::with_revision(3);

/// Request: physical memory map.
///
/// Returns a list of memory regions with their types (usable, reserved,
/// ACPI reclaimable, etc.).  This is the foundation for the physical
/// page allocator.
#[used]
#[unsafe(link_section = ".requests")]
static MEMORY_MAP_REQUEST: LimineRequest<MemmapResponse> = LimineRequest::MEMMAP;

/// Request: Higher Half Direct Map offset.
///
/// Limine maps all of physical memory at `HHDM_offset + phys_addr`.
/// We use this to convert physical addresses to virtual addresses
/// without setting up our own page tables for the direct map.
#[used]
#[unsafe(link_section = ".requests")]
static HHDM_REQUEST: LimineRequest<HhdmResponse> = LimineRequest::HHDM;

/// Request: framebuffer information.
///
/// We don't use a graphical framebuffer yet, but requesting it ensures
/// Limine sets one up.  This lets us display boot text on real hardware
/// where serial output isn't visible.
#[used]
#[unsafe(link_section = ".requests")]
static FRAMEBUFFER_REQUEST: LimineRequest<FramebufferResponse> = LimineRequest::FRAMEBUFFER;

// ---------------------------------------------------------------------------
// Public accessors
// ---------------------------------------------------------------------------

/// Framebuffer information from the bootloader (if available).
///
/// Contains the virtual address and geometry needed to initialize
/// the framebuffer text console.
pub struct FramebufferInfo {
    /// Virtual address of the framebuffer start (already mapped by Limine).
    pub address: u64,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Bytes per row (may include padding beyond visible width).
    pub pitch: u32,
    /// Bits per pixel (typically 32 for BGRA).
    pub bpp: u16,
}

/// Information extracted from the Limine boot protocol responses.
pub struct BootInfo {
    /// Offset for the Higher Half Direct Map.
    /// Virtual address = `hhdm_offset + physical_address`.
    pub hhdm_offset: u64,
    /// Physical memory map from the bootloader.
    ///
    /// Entries are sorted by base address and do not overlap.  The frame
    /// allocator uses this to discover usable physical memory.
    pub memory_map: &'static [&'static MemmapEntry],
    /// Framebuffer info for the text console (None if not available).
    pub framebuffer: Option<FramebufferInfo>,
}

/// Parse Limine responses and extract boot information.
///
/// Returns `None` if any critical response is missing (which means
/// the bootloader didn't understand our requests — should never happen
/// with a compatible Limine version).
// All arithmetic in this function is for display-only logging (KiB/MiB
// conversions, address range endpoints).  Overflow is handled via
// saturating_add where it matters; the divisions are by constants.
#[allow(clippy::arithmetic_side_effects)]
pub fn parse_boot_info() -> Option<BootInfo> {
    // Verify the bootloader understood our base revision.
    if !BASE_REVISION.is_supported() {
        serial_println!("[boot] ERROR: Limine base revision 3 not supported");
        return None;
    }

    // HHDM offset — needed for physical-to-virtual address translation.
    let hhdm_response = HHDM_REQUEST.response()?;
    let hhdm_offset = hhdm_response.offset;
    serial_println!("[boot] HHDM offset: {:#x}", hhdm_offset);

    // Memory map — log it and pass to the frame allocator.
    let mmap_response = MEMORY_MAP_REQUEST.response()?;
    let entries = mmap_response.entries();
    serial_println!("[boot] Memory map ({} entries):", entries.len());
    let mut total_usable: u64 = 0;
    for entry in entries {
        let kind = match entry.type_ {
            memmap_type::USABLE => {
                total_usable = total_usable.saturating_add(entry.length);
                "usable"
            }
            memmap_type::RESERVED => "reserved",
            memmap_type::ACPI_RECLAIMABLE => "ACPI reclaimable",
            memmap_type::ACPI_NVS => "ACPI NVS",
            memmap_type::BAD_MEMORY => "bad memory",
            memmap_type::BOOTLOADER_RECLAIMABLE => "bootloader reclaimable",
            memmap_type::EXECUTABLE_AND_MODULES => "kernel+modules",
            memmap_type::FRAMEBUFFER => "framebuffer",
            _ => "unknown",
        };
        serial_println!(
            "  [{:#012x} - {:#012x}] {} ({} KiB)",
            entry.base,
            entry.base.saturating_add(entry.length),
            kind,
            entry.length / 1024
        );
    }
    serial_println!(
        "[boot] Total usable memory: {} MiB",
        total_usable / (1024 * 1024)
    );

    // Framebuffer (optional — extract info for the text console).
    // Limine provides the framebuffer already mapped at a virtual address.
    // Dimensions are u64 in the Limine protocol but practically never
    // exceed u32; the truncation is intentional and safe.
    #[allow(clippy::cast_possible_truncation)]
    let framebuffer = FRAMEBUFFER_REQUEST.response().and_then(|fb_response| {
        fb_response.framebuffers().first().map(|fb| {
            serial_println!(
                "[boot] Framebuffer: {}x{} @ {:#x} (pitch={}, bpp={})",
                fb.width,
                fb.height,
                fb.address as u64,
                fb.pitch,
                fb.bpp
            );
            FramebufferInfo {
                address: fb.address as u64,
                width: fb.width as u32,
                height: fb.height as u32,
                pitch: fb.pitch as u32,
                bpp: fb.bpp,
            }
        })
    });

    Some(BootInfo {
        hhdm_offset,
        memory_map: entries,
        framebuffer,
    })
}
