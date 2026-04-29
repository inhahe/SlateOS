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
//! - `limine` crate: <https://docs.rs/limine>
//!
//! **Note:** The API calls below target `limine` crate 0.3.x.  If the
//! crate version changes, method names or module paths may need updating.
//! Check <https://docs.rs/limine> for the exact API of the version in use.

use limine::BaseRevision;
use limine::request::{
    FramebufferRequest, HhdmRequest, MemoryMapRequest,
    RequestsEndMarker, RequestsStartMarker,
};

// ---------------------------------------------------------------------------
// Protocol markers and requests
// ---------------------------------------------------------------------------

/// Start-of-requests marker.  Must be the first item in `.requests_start_marker`.
#[used]
#[link_section = ".requests_start_marker"]
static REQUESTS_START: RequestsStartMarker = RequestsStartMarker::new();

/// End-of-requests marker.  Must be the last item in `.requests_end_marker`.
#[used]
#[link_section = ".requests_end_marker"]
static REQUESTS_END: RequestsEndMarker = RequestsEndMarker::new();

/// Protocol base revision.  Tells Limine which protocol version we speak.
#[used]
#[link_section = ".requests"]
static BASE_REVISION: BaseRevision = BaseRevision::with_revision(3);

/// Request: physical memory map.
///
/// Returns a list of memory regions with their types (usable, reserved,
/// ACPI reclaimable, etc.).  This is the foundation for the physical
/// page allocator.
#[used]
#[link_section = ".requests"]
static MEMORY_MAP_REQUEST: MemoryMapRequest = MemoryMapRequest::new();

/// Request: Higher Half Direct Map offset.
///
/// Limine maps all of physical memory at `HHDM_offset + phys_addr`.
/// We use this to convert physical addresses to virtual addresses
/// without setting up our own page tables for the direct map.
#[used]
#[link_section = ".requests"]
static HHDM_REQUEST: HhdmRequest = HhdmRequest::new();

/// Request: framebuffer information.
///
/// We don't use a graphical framebuffer yet, but requesting it ensures
/// Limine sets one up.  This lets us display boot text on real hardware
/// where serial output isn't visible.
#[used]
#[link_section = ".requests"]
static FRAMEBUFFER_REQUEST: FramebufferRequest = FramebufferRequest::new();

// ---------------------------------------------------------------------------
// Public accessors
// ---------------------------------------------------------------------------

/// Information extracted from the Limine boot protocol responses.
pub struct BootInfo {
    /// Offset for the Higher Half Direct Map.
    /// Virtual address = `hhdm_offset + physical_address`.
    pub hhdm_offset: u64,
}

/// Parse Limine responses and extract boot information.
///
/// Returns `None` if any critical response is missing (which means
/// the bootloader didn't understand our requests — should never happen
/// with a compatible Limine version).
pub fn parse_boot_info() -> Option<BootInfo> {
    // Verify the bootloader understood our base revision.
    if !BASE_REVISION.is_supported() {
        serial_println!("[boot] ERROR: Limine base revision 3 not supported");
        return None;
    }

    // HHDM offset — needed for physical-to-virtual address translation.
    let hhdm_response = HHDM_REQUEST.get_response()?;
    let hhdm_offset = hhdm_response.offset();
    serial_println!("[boot] HHDM offset: {:#x}", hhdm_offset);

    // Memory map — log it and pass to the frame allocator.
    let mmap_response = MEMORY_MAP_REQUEST.get_response()?;
    serial_println!("[boot] Memory map ({} entries):", mmap_response.entries().len());
    let mut total_usable: u64 = 0;
    for entry in mmap_response.entries() {
        let kind = match entry.entry_type {
            limine::memory_map::EntryType::USABLE => {
                total_usable = total_usable.saturating_add(entry.length);
                "usable"
            }
            limine::memory_map::EntryType::RESERVED => "reserved",
            limine::memory_map::EntryType::ACPI_RECLAIMABLE => "ACPI reclaimable",
            limine::memory_map::EntryType::ACPI_NVS => "ACPI NVS",
            limine::memory_map::EntryType::BAD_MEMORY => "bad memory",
            limine::memory_map::EntryType::BOOTLOADER_RECLAIMABLE => "bootloader reclaimable",
            limine::memory_map::EntryType::KERNEL_AND_MODULES => "kernel+modules",
            limine::memory_map::EntryType::FRAMEBUFFER => "framebuffer",
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
    serial_println!("[boot] Total usable memory: {} MiB", total_usable / (1024 * 1024));

    // Framebuffer (optional — log if present).
    if let Some(fb_response) = FRAMEBUFFER_REQUEST.get_response() {
        if let Some(fb) = fb_response.framebuffers().next() {
            serial_println!(
                "[boot] Framebuffer: {}x{} @ {:#x} (pitch={}, bpp={})",
                fb.width(),
                fb.height(),
                fb.addr() as u64,
                fb.pitch(),
                fb.bpp()
            );
        }
    }

    Some(BootInfo { hhdm_offset })
}
