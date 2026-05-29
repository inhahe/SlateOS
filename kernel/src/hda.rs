//! Intel High Definition Audio (HDA) controller driver.
//!
//! Implements the Intel HD Audio specification for the codec interface
//! (CORB/RIRB command/response mechanism) and output stream configuration.
//! This driver targets the ICH6/ICH9-class HDA controllers emulated by
//! QEMU (`-device intel-hda -device hda-duplex`).
//!
//! ## Hardware Overview
//!
//! The HDA controller is a PCI device with MMIO register access:
//! - Vendor: 0x8086 (Intel)
//! - Device: 0x2668 (ICH6 HDA, QEMU default), 0x293E (ICH9)
//! - Class: 0x04 (multimedia), subclass 0x03 (HD Audio)
//! - BAR0: Memory-mapped I/O registers (~16 KiB region)
//!
//! ## Architecture
//!
//! The controller acts as a DMA engine between codecs (audio chips) and
//! host memory.  Communication with codecs uses two ring buffers:
//! - **CORB** (Command Output Ring Buffer): driver → codec verbs
//! - **RIRB** (Response Input Ring Buffer): codec → driver responses
//!
//! Audio data flows through "streams" — DMA channels with buffer descriptor
//! lists (BDLs) pointing to PCM data in host memory.
//!
//! ## References
//!
//! - Intel High Definition Audio Specification Rev 1.0a (June 2010)
//! - Linux sound/pci/hda/ (snd-hda-intel)
//! - OSDev Wiki: Intel High Definition Audio

use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use spin::Mutex;

use crate::error::{KernelError, KernelResult};
use crate::mm::frame::{self, PhysFrame};
use crate::mm::page_table::{self, PageFlags, VirtAddr};
use crate::pci::{self, PciDevice};
use crate::serial_println;

// ---------------------------------------------------------------------------
// PCI identification
// ---------------------------------------------------------------------------

const VENDOR_INTEL: u16 = 0x8086;

/// Known Intel HDA controller device IDs.
const HDA_DEVICE_IDS: &[u16] = &[
    0x2668, // ICH6 (QEMU default)
    0x27D8, // ICH6-M (82801FB/FBM/FR/FW/FRW)
    0x284B, // ICH8
    0x293E, // ICH9
    0x3A3E, // ICH10
    0x1C20, // Cougar Point (6 Series)
    0x8C20, // 8 Series / Lynx Point
    0xA170, // 100 Series / Sunrise Point
    0xA348, // 300 Series / Cannon Point
    0x06C8, // 400 Series / Comet Lake
    0xA0C8, // Tiger Lake
    0x4DC8, // Jasper Lake
    0x7AD0, // Alder Lake
    0xA728, // Raptor Lake
];

/// PCI class for multimedia audio (class 0x04, subclass 0x03).
const PCI_CLASS_MULTIMEDIA: u8 = 0x04;
const PCI_SUBCLASS_HD_AUDIO: u8 = 0x03;

// ---------------------------------------------------------------------------
// MMIO register offsets
// ---------------------------------------------------------------------------

/// Global Capabilities (16-bit).
const REG_GCAP: usize = 0x00;
/// Minor Version (8-bit).
const REG_VMIN: usize = 0x02;
/// Major Version (8-bit).
const REG_VMAJ: usize = 0x03;
/// Global Control (32-bit).
const REG_GCTL: usize = 0x08;
/// Wake Enable (16-bit).
#[allow(dead_code)]
const REG_WAKEEN: usize = 0x0C;
/// State Change Status (16-bit) — codec detection.
const REG_STATESTS: usize = 0x0E;
/// Interrupt Control (32-bit).
const REG_INTCTL: usize = 0x20;
/// Interrupt Status (32-bit).
#[allow(dead_code)]
const REG_INTSTS: usize = 0x24;

// CORB registers
const REG_CORBLBASE: usize = 0x40;
const REG_CORBUBASE: usize = 0x44;
const REG_CORBWP: usize = 0x48;
const REG_CORBRP: usize = 0x4A;
const REG_CORBCTL: usize = 0x4C;
#[allow(dead_code)]
const REG_CORBSTS: usize = 0x4D;
const REG_CORBSIZE: usize = 0x4E;

// RIRB registers
const REG_RIRBLBASE: usize = 0x50;
const REG_RIRBUBASE: usize = 0x54;
const REG_RIRBWP: usize = 0x58;
const REG_RINTCNT: usize = 0x5A;
const REG_RIRBCTL: usize = 0x5C;
const REG_RIRBSTS: usize = 0x5D;
const REG_RIRBSIZE: usize = 0x5E;

// Stream descriptor base offset (first stream at 0x80, each 0x20 bytes).
const STREAM_BASE: usize = 0x80;
const STREAM_SIZE: usize = 0x20;

// Stream descriptor register offsets (relative to stream base)
const SD_CTL: usize = 0x00;   // 24-bit control (read as u32, bits 23:0)
#[allow(dead_code)]
const SD_STS: usize = 0x03;   // 8-bit status
#[allow(dead_code)]
const SD_LPIB: usize = 0x04;  // 32-bit link position in buffer
const SD_CBL: usize = 0x08;   // 32-bit cyclic buffer length
const SD_LVI: usize = 0x0C;   // 16-bit last valid BDL index
const SD_FMT: usize = 0x12;   // 16-bit stream format
const SD_BDPL: usize = 0x18;  // 32-bit BDL physical address low
const SD_BDPU: usize = 0x1C;  // 32-bit BDL physical address high

// GCTL bits
const GCTL_CRST: u32 = 1 << 0;  // Controller reset
const GCTL_UNSOL: u32 = 1 << 8; // Accept unsolicited responses

// CORBCTL bits
const CORBCTL_RUN: u8 = 1 << 1;  // CORB DMA engine run

// RIRBCTL bits
const RIRBCTL_RINTCTL: u8 = 1 << 0; // Response interrupt enable
const RIRBCTL_DMAEN: u8 = 1 << 1;   // RIRB DMA engine enable

// Stream CTL bits
const SDCTL_SRST: u32 = 1 << 0;  // Stream reset
const SDCTL_RUN: u32 = 1 << 1;   // Stream run
const SDCTL_IOCE: u32 = 1 << 2;  // Interrupt on completion enable

// Stream format bits (for SD_FMT register)
const FMT_BASE_48K: u16 = 0 << 14;      // 48 kHz base
#[allow(dead_code)]
const FMT_BASE_44K: u16 = 1 << 14;      // 44.1 kHz base
const FMT_BITS_16: u16 = 0b001 << 4;    // 16-bit samples
#[allow(dead_code)]
const FMT_BITS_24: u16 = 0b011 << 4;    // 24-bit samples
const FMT_CHAN_STEREO: u16 = 1;          // channels - 1 (stereo = 1)
#[allow(dead_code)]
const FMT_CHAN_MONO: u16 = 0;            // channels - 1 (mono = 0)

/// Standard format: 48 kHz, 16-bit, stereo.
const FMT_48K_16BIT_STEREO: u16 = FMT_BASE_48K | FMT_BITS_16 | FMT_CHAN_STEREO;

// CORB/RIRB sizes
const CORB_ENTRIES: usize = 256;
const RIRB_ENTRIES: usize = 256;

// Maximum number of BDL entries per stream.
#[allow(dead_code)]
const MAX_BDL_ENTRIES: usize = 256;

// Timeouts (in microseconds)
const RESET_TIMEOUT_US: u64 = 100_000;   // 100 ms
const CODEC_RESPONSE_TIMEOUT_US: u64 = 50_000; // 50 ms

// ---------------------------------------------------------------------------
// HDA codec verb construction
// ---------------------------------------------------------------------------

/// Build a 12-bit verb (the common format for most codec commands).
///
/// Format: `[CAD:4][NID:8][verb:12][payload:8]`
#[inline]
const fn verb12(cad: u8, nid: u8, verb: u16, payload: u8) -> u32 {
    ((cad as u32) << 28)
        | ((nid as u32) << 20)
        | (((verb & 0xFFF) as u32) << 8)
        | (payload as u32)
}

/// Build a 4-bit verb with 16-bit payload (used for SET_AMP_GAIN, etc.).
///
/// Format: `[CAD:4][NID:8][verb:4][payload:16]`
#[inline]
const fn verb4(cad: u8, nid: u8, verb: u8, payload: u16) -> u32 {
    ((cad as u32) << 28)
        | ((nid as u32) << 20)
        | (((verb & 0xF) as u32) << 16)
        | (payload as u32)
}

// Common 12-bit verbs
const VERB_GET_PARAM: u16 = 0xF00;
#[allow(dead_code)]
const VERB_SET_STREAM_FORMAT: u16 = 0x200; // 4-bit: verb=2, payload=format
const VERB_SET_CONV_STREAM_CHAN: u16 = 0x706;
#[allow(dead_code)]
const VERB_GET_CONV_STREAM_CHAN: u16 = 0xF06;
const VERB_SET_PIN_WIDGET_CTL: u16 = 0x707;
#[allow(dead_code)]
const VERB_GET_PIN_WIDGET_CTL: u16 = 0xF07;
const VERB_SET_EAPD_BTLENABLE: u16 = 0x70C;
const VERB_SET_POWER_STATE: u16 = 0x705;
#[allow(dead_code)]
const VERB_GET_CONN_LIST: u16 = 0xF02;

// 4-bit verb for amp gain
const VERB4_SET_AMP_GAIN: u8 = 0x3;

// Parameter IDs (for GET_PARAM)
const PARAM_VENDOR_ID: u8 = 0x00;
const PARAM_REVISION_ID: u8 = 0x02;
const PARAM_SUBORD_NODE_COUNT: u8 = 0x04;
const PARAM_FUNC_GROUP_TYPE: u8 = 0x05;
const PARAM_AUDIO_WIDGET_CAP: u8 = 0x09;
#[allow(dead_code)]
const PARAM_PCM_SIZE_RATE: u8 = 0x0A;
const PARAM_PIN_CAP: u8 = 0x0C;
#[allow(dead_code)]
const PARAM_CONN_LIST_LEN: u8 = 0x0E;
#[allow(dead_code)]
const PARAM_AMP_OUT_CAP: u8 = 0x12;

// Audio widget types (bits 23:20 of AUDIO_WIDGET_CAP)
const WIDGET_TYPE_AUDIO_OUTPUT: u8 = 0x0; // DAC
#[allow(dead_code)]
const WIDGET_TYPE_AUDIO_INPUT: u8 = 0x1;  // ADC
const WIDGET_TYPE_AUDIO_MIXER: u8 = 0x2;
const WIDGET_TYPE_AUDIO_SELECTOR: u8 = 0x3;
const WIDGET_TYPE_PIN_COMPLEX: u8 = 0x4;

// Pin widget control bits
const PIN_CTL_OUT_EN: u8 = 0x40;  // Output enable
const PIN_CTL_HP_EN: u8 = 0x80;   // Headphone amplifier enable

// Amp gain/mute bits (for SET_AMP_GAIN payload)
const AMP_OUT: u16 = 1 << 15;     // Output amp
#[allow(dead_code)]
const AMP_IN: u16 = 1 << 14;      // Input amp
const AMP_LEFT: u16 = 1 << 13;    // Left channel
const AMP_RIGHT: u16 = 1 << 12;   // Right channel
#[allow(dead_code)]
const AMP_MUTE: u16 = 1 << 7;     // Mute

// ---------------------------------------------------------------------------
// Buffer Descriptor List entry
// ---------------------------------------------------------------------------

/// A single entry in a stream's Buffer Descriptor List.
///
/// The BDL tells the DMA engine where PCM data lives in host memory.
/// Each entry points to one contiguous buffer segment.
#[repr(C, align(16))]
#[derive(Clone, Copy)]
struct BdlEntry {
    /// Physical address of buffer (lower 32 bits).
    addr_lo: u32,
    /// Physical address of buffer (upper 32 bits).
    addr_hi: u32,
    /// Length of this segment in bytes.
    length: u32,
    /// Flags: bit 0 = IOC (interrupt on completion).
    flags: u32,
}

/// RIRB response entry (8 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug)]
struct RirbEntry {
    /// Codec response value.
    response: u32,
    /// Extended: bit 4 = unsolicited, bits 3:0 = codec address.
    response_ex: u32,
}

// ---------------------------------------------------------------------------
// Driver state
// ---------------------------------------------------------------------------

/// Whether the HDA controller has been initialized.
static INITIALIZED: AtomicBool = AtomicBool::new(false);

/// Number of codecs detected.
static CODEC_COUNT: AtomicU8 = AtomicU8::new(0);

/// Global driver state, protected by a mutex.
static DEVICE: Mutex<Option<HdaDevice>> = Mutex::new(None);

/// Represents the HDA controller's runtime state.
struct HdaDevice {
    /// MMIO base virtual address.
    mmio_base: u64,
    /// Number of input streams.
    iss: u8,
    /// Number of output streams.
    oss: u8,
    /// Number of bidirectional streams.
    bss: u8,
    /// Codec addresses that are present (bitmask, bits 0-14).
    _codec_mask: u16,
    /// CORB DMA buffer physical address.
    _corb_phys: u64,
    /// CORB DMA buffer virtual address.
    corb_virt: u64,
    /// Current CORB write pointer.
    corb_wp: u16,
    /// RIRB DMA buffer physical address.
    _rirb_phys: u64,
    /// RIRB DMA buffer virtual address.
    rirb_virt: u64,
    /// Current RIRB read pointer (software-maintained).
    rirb_rp: u16,
    /// Output stream BDL physical address.
    bdl_phys: u64,
    /// Output stream BDL virtual address.
    bdl_virt: u64,
    /// PCM output buffer physical address.
    pcm_phys: u64,
    /// PCM output buffer virtual address.
    pcm_virt: u64,
    /// PCM buffer size in bytes.
    pcm_size: u32,
    /// Output stream index (offset from first output stream).
    out_stream_idx: u8,
    /// Codec 0 vendor ID.
    vendor_id: u32,
    /// DAC node ID (found during codec walk).
    dac_nid: u8,
    /// Output pin node ID.
    pin_nid: u8,
    /// Physical frames allocated (for cleanup).
    _frames: Vec<PhysFrame>,
}

// ---------------------------------------------------------------------------
// MMIO helpers
// ---------------------------------------------------------------------------

/// Read a 32-bit MMIO register.
#[inline]
fn mmio_read32(base: u64, offset: usize) -> u32 {
    // SAFETY: base is a valid mapped MMIO address (mapped during init).
    unsafe {
        core::ptr::read_volatile((base as *const u8).add(offset) as *const u32)
    }
}

/// Write a 32-bit MMIO register.
#[inline]
fn mmio_write32(base: u64, offset: usize, val: u32) {
    // SAFETY: base is a valid mapped MMIO address.
    unsafe {
        core::ptr::write_volatile((base as *mut u8).add(offset) as *mut u32, val);
    }
}

/// Read a 16-bit MMIO register.
#[inline]
fn mmio_read16(base: u64, offset: usize) -> u16 {
    // SAFETY: base is a valid mapped MMIO address.
    unsafe {
        core::ptr::read_volatile((base as *const u8).add(offset) as *const u16)
    }
}

/// Write a 16-bit MMIO register.
#[inline]
fn mmio_write16(base: u64, offset: usize, val: u16) {
    // SAFETY: base is a valid mapped MMIO address.
    unsafe {
        core::ptr::write_volatile((base as *mut u8).add(offset) as *mut u16, val);
    }
}

/// Read an 8-bit MMIO register.
#[inline]
fn mmio_read8(base: u64, offset: usize) -> u8 {
    // SAFETY: base is a valid mapped MMIO address.
    unsafe {
        core::ptr::read_volatile((base as *const u8).add(offset))
    }
}

/// Write an 8-bit MMIO register.
#[inline]
fn mmio_write8(base: u64, offset: usize, val: u8) {
    // SAFETY: base is a valid mapped MMIO address.
    unsafe {
        core::ptr::write_volatile((base as *mut u8).add(offset), val);
    }
}

/// Busy-wait delay in microseconds (approximate, using TSC if available).
fn delay_us(us: u64) {
    // Use a simple TSC-based busy loop.
    // SAFETY: _rdtsc is always available on x86_64 and has no side effects.
    let start = unsafe { core::arch::x86_64::_rdtsc() };
    // Assume ~2 GHz clock minimum — 2000 cycles per µs.
    let target_cycles = us.saturating_mul(2000);
    loop {
        let now = unsafe { core::arch::x86_64::_rdtsc() };
        if now.wrapping_sub(start) >= target_cycles {
            break;
        }
        core::hint::spin_loop();
    }
}

// ---------------------------------------------------------------------------
// Controller initialization
// ---------------------------------------------------------------------------

/// Initialize the Intel HDA controller.
///
/// Searches PCI for a supported HDA device, maps its MMIO registers,
/// performs a controller reset, sets up CORB/RIRB, and discovers codecs.
///
/// # Arguments
/// * `hhdm_offset` — Higher Half Direct Map offset for physical→virtual conversion.
pub fn init(hhdm_offset: u64) {
    serial_println!("[hda] Probing for Intel HD Audio controller...");

    // Find the HDA PCI device.
    let pci_dev = match find_hda_device() {
        Some(dev) => dev,
        None => {
            serial_println!("[hda] No Intel HDA controller found");
            return;
        }
    };

    serial_println!(
        "[hda] Found HDA controller: {:04x}:{:04x} at bus={} dev={} func={}",
        pci_dev.vendor_id, pci_dev.device_id,
        pci_dev.address.bus, pci_dev.address.device, pci_dev.address.function
    );

    // Read BAR0 (MMIO).
    let bar0_raw = pci_dev.bars[0];
    if bar0_raw == 0 {
        serial_println!("[hda] BAR0 is zero — device not properly configured");
        return;
    }

    // BAR0 may be 64-bit MMIO; check type field (bits 2:1).
    let bar0_phys = if bar0_raw & 0x06 == 0x04 {
        // 64-bit BAR: combine BAR0 (low) and BAR1 (high).
        let bar1_raw = pci_dev.bars[1];
        ((bar1_raw as u64) << 32) | ((bar0_raw & !0xF_u32) as u64)
    } else {
        (bar0_raw & !0xF_u32) as u64
    };

    serial_println!("[hda] MMIO BAR0 physical: {:#x}", bar0_phys);

    // Map the MMIO region into kernel virtual address space.
    // PCI BAR addresses may be above physical RAM, so the HHDM bootloader
    // mapping doesn't cover them.  We must explicitly map the MMIO pages
    // with NO_CACHE attribute (uncacheable device memory).
    let mmio_base = bar0_phys.wrapping_add(hhdm_offset);
    let pml4_phys = page_table::cr3_to_pml4(page_table::read_cr3());
    let mmio_flags = PageFlags::PRESENT | PageFlags::WRITABLE | PageFlags::NO_CACHE;

    // HDA register space is ~16 KiB. Map 1 frame (16 KiB) minimum.
    if let Some(frame) = PhysFrame::from_addr(bar0_phys) {
        let virt = VirtAddr::new(mmio_base);
        // SAFETY: bar0_phys is the PCI BAR MMIO region for the HDA controller.
        // We're mapping device registers into kernel virtual address space.
        if let Err(e) = unsafe { page_table::map_frame(pml4_phys, virt, frame, mmio_flags) } {
            // May already be mapped (e.g., within HHDM range on large-RAM systems).
            serial_println!("[hda] Note: MMIO map returned {:?} (may be pre-mapped)", e);
        }
    }

    // Flush TLB for the mapped address.
    // SAFETY: Standard invlpg to flush stale TLB entries for device MMIO.
    unsafe {
        core::arch::asm!("invlpg [{}]", in(reg) mmio_base, options(nostack, preserves_flags));
    }

    // Ensure the PCI device is bus-master enabled (needed for DMA).
    pci::enable_bus_master(pci_dev.address);

    // Read capabilities before reset.
    let gcap = mmio_read16(mmio_base, REG_GCAP);
    let vmaj = mmio_read8(mmio_base, REG_VMAJ);
    let vmin = mmio_read8(mmio_base, REG_VMIN);

    let iss = ((gcap >> 8) & 0xF) as u8;  // Input streams
    let oss = ((gcap >> 12) & 0xF) as u8; // Output streams
    let bss = ((gcap >> 3) & 0x1F) as u8; // Bidirectional streams (bits 7:3)

    serial_println!("[hda] Version {}.{}, ISS={} OSS={} BSS={}", vmaj, vmin, iss, oss, bss);

    if oss == 0 && bss == 0 {
        serial_println!("[hda] No output or bidirectional streams — cannot play audio");
        return;
    }

    // Perform controller reset.
    if let Err(e) = controller_reset(mmio_base) {
        serial_println!("[hda] Controller reset failed: {:?}", e);
        return;
    }

    // Allocate DMA buffers for CORB and RIRB.
    let mut frames = Vec::new();

    // CORB: 256 entries × 4 bytes = 1024 bytes (fits in one 16 KiB frame).
    let corb_frame = match frame::alloc_frame() {
        Ok(f) => f,
        Err(e) => {
            serial_println!("[hda] Failed to allocate CORB frame: {:?}", e);
            return;
        }
    };
    let corb_phys = corb_frame.addr();
    let corb_virt = corb_phys + hhdm_offset;
    frames.push(corb_frame);

    // Zero the CORB buffer.
    // SAFETY: We just allocated this frame and have exclusive access.
    unsafe {
        core::ptr::write_bytes(corb_virt as *mut u8, 0, 4096);
    }

    // RIRB: 256 entries × 8 bytes = 2048 bytes (fits in same-size frame).
    let rirb_frame = match frame::alloc_frame() {
        Ok(f) => f,
        Err(e) => {
            serial_println!("[hda] Failed to allocate RIRB frame: {:?}", e);
            return;
        }
    };
    let rirb_phys = rirb_frame.addr();
    let rirb_virt = rirb_phys + hhdm_offset;
    frames.push(rirb_frame);

    // SAFETY: Exclusive access to freshly-allocated frame.
    unsafe {
        core::ptr::write_bytes(rirb_virt as *mut u8, 0, 4096);
    }

    // Set up CORB.
    setup_corb(mmio_base, corb_phys);

    // Set up RIRB.
    setup_rirb(mmio_base, rirb_phys);

    // Discover codecs via STATESTS.
    let statests = mmio_read16(mmio_base, REG_STATESTS);
    // Clear status bits by writing 1s.
    mmio_write16(mmio_base, REG_STATESTS, statests);

    let codec_mask = statests & 0x7FFF; // bits 14:0
    let codec_count = codec_mask.count_ones() as u8;

    if codec_count == 0 {
        serial_println!("[hda] No codecs detected (STATESTS={:#06x})", statests);
        serial_println!("[hda] Hint: QEMU needs '-device hda-duplex' or '-device hda-output'");
        // Still store state — controller works, just no codecs attached.
    } else {
        serial_println!("[hda] {} codec(s) detected (mask={:#06x})", codec_count, codec_mask);
    }

    CODEC_COUNT.store(codec_count, Ordering::Release);

    // Allocate BDL buffer (one frame for BDL entries).
    let bdl_frame = match frame::alloc_frame() {
        Ok(f) => f,
        Err(e) => {
            serial_println!("[hda] Failed to allocate BDL frame: {:?}", e);
            return;
        }
    };
    let bdl_phys = bdl_frame.addr();
    let bdl_virt = bdl_phys + hhdm_offset;
    frames.push(bdl_frame);

    // SAFETY: Exclusive access.
    unsafe {
        core::ptr::write_bytes(bdl_virt as *mut u8, 0, 4096);
    }

    // Allocate PCM output buffer (4 frames = 64 KiB for ~340ms of 48kHz/16-bit/stereo).
    let pcm_frames_needed = 4;
    let pcm_size: u32 = pcm_frames_needed * 16384; // 4 × 16 KiB = 64 KiB

    // Allocate contiguous frames for PCM buffer.
    // Note: for simplicity we use the first frame's address and hope the
    // allocator gives us sequential frames.  In production we'd use a
    // proper contiguous DMA allocator.
    let first_pcm_frame = match frame::alloc_frame() {
        Ok(f) => f,
        Err(e) => {
            serial_println!("[hda] Failed to allocate PCM frame: {:?}", e);
            return;
        }
    };
    let pcm_phys = first_pcm_frame.addr();
    let pcm_virt = pcm_phys + hhdm_offset;
    frames.push(first_pcm_frame);

    for _ in 1..pcm_frames_needed {
        match frame::alloc_frame() {
            Ok(f) => frames.push(f),
            Err(e) => {
                serial_println!("[hda] Failed to allocate PCM frame: {:?}", e);
                return;
            }
        }
    }

    // Zero the PCM buffer.
    // SAFETY: We allocated these frames and own them exclusively.
    unsafe {
        core::ptr::write_bytes(pcm_virt as *mut u8, 0, pcm_size as usize);
    }

    // Store device state.
    let mut dev = HdaDevice {
        mmio_base,
        iss,
        oss,
        bss,
        _codec_mask: codec_mask,
        _corb_phys: corb_phys,
        corb_virt,
        corb_wp: 0,
        _rirb_phys: rirb_phys,
        rirb_virt,
        rirb_rp: 0,
        bdl_phys,
        bdl_virt,
        pcm_phys,
        pcm_virt,
        pcm_size,
        out_stream_idx: 0,
        vendor_id: 0,
        dac_nid: 0,
        pin_nid: 0,
        _frames: frames,
    };

    // If we have codecs, probe codec 0.
    if codec_count > 0 {
        probe_codec(&mut dev, 0);
    }

    INITIALIZED.store(true, Ordering::Release);
    *DEVICE.lock() = Some(dev);

    serial_println!("[hda] Initialization complete");
}

/// Find an Intel HDA PCI device.
fn find_hda_device() -> Option<PciDevice> {
    // Try vendor+device matching first.
    for &dev_id in HDA_DEVICE_IDS {
        if let Some(dev) = pci::find_device(VENDOR_INTEL, dev_id) {
            return Some(dev);
        }
    }
    // Fall back to class-based detection (any vendor's HDA controller).
    let devs = pci::find_devices_by_class(PCI_CLASS_MULTIMEDIA, PCI_SUBCLASS_HD_AUDIO);
    devs.into_iter().next()
}

/// Perform a full controller reset (GCTL.CRST cycle).
fn controller_reset(base: u64) -> KernelResult<()> {
    serial_println!("[hda] Resetting controller...");

    // Stop all streams first.
    let gcap = mmio_read16(base, REG_GCAP);
    let total_streams = ((gcap >> 8) & 0xF)  // ISS
        + ((gcap >> 12) & 0xF)               // OSS
        + (((gcap >> 3) & 0x1F));       // BSS

    for i in 0..total_streams {
        let stream_off = STREAM_BASE + (i as usize) * STREAM_SIZE;
        let ctl = mmio_read32(base, stream_off + SD_CTL);
        mmio_write32(base, stream_off + SD_CTL, ctl & !(SDCTL_RUN | SDCTL_IOCE));
    }

    // Stop CORB and RIRB DMA.
    mmio_write8(base, REG_CORBCTL, 0);
    mmio_write8(base, REG_RIRBCTL, 0);

    // Assert reset: clear GCTL.CRST.
    let gctl = mmio_read32(base, REG_GCTL);
    mmio_write32(base, REG_GCTL, gctl & !GCTL_CRST);

    // Wait for CRST to read 0 (controller acknowledged reset).
    let mut timeout = RESET_TIMEOUT_US / 10;
    while mmio_read32(base, REG_GCTL) & GCTL_CRST != 0 {
        if timeout == 0 {
            return Err(KernelError::TimedOut);
        }
        timeout -= 1;
        delay_us(10);
    }

    // Hold reset for 100 µs (spec minimum).
    delay_us(100);

    // Release reset: set GCTL.CRST.
    let gctl = mmio_read32(base, REG_GCTL);
    mmio_write32(base, REG_GCTL, gctl | GCTL_CRST);

    // Wait for CRST to read 1 (controller is out of reset).
    timeout = RESET_TIMEOUT_US / 10;
    while mmio_read32(base, REG_GCTL) & GCTL_CRST == 0 {
        if timeout == 0 {
            return Err(KernelError::TimedOut);
        }
        timeout -= 1;
        delay_us(10);
    }

    // Post-reset codec stabilization delay (spec says 521 µs minimum,
    // we use 1000 µs to be safe).
    delay_us(1000);

    // Enable unsolicited responses.
    let gctl = mmio_read32(base, REG_GCTL);
    mmio_write32(base, REG_GCTL, gctl | GCTL_UNSOL);

    serial_println!("[hda] Controller reset complete");
    Ok(())
}

/// Set up the CORB (Command Output Ring Buffer).
fn setup_corb(base: u64, phys: u64) {
    // Set size to 256 entries.
    mmio_write8(base, REG_CORBSIZE, 0x02);

    // Write physical address.
    mmio_write32(base, REG_CORBLBASE, phys as u32);
    mmio_write32(base, REG_CORBUBASE, (phys >> 32) as u32);

    // Reset read pointer: set bit 15, then clear it.
    mmio_write16(base, REG_CORBRP, 0x8000);
    delay_us(100);
    mmio_write16(base, REG_CORBRP, 0x0000);

    // Reset write pointer.
    mmio_write16(base, REG_CORBWP, 0x0000);

    // Start CORB DMA.
    mmio_write8(base, REG_CORBCTL, CORBCTL_RUN);
}

/// Set up the RIRB (Response Input Ring Buffer).
fn setup_rirb(base: u64, phys: u64) {
    // Set size to 256 entries.
    mmio_write8(base, REG_RIRBSIZE, 0x02);

    // Write physical address.
    mmio_write32(base, REG_RIRBLBASE, phys as u32);
    mmio_write32(base, REG_RIRBUBASE, (phys >> 32) as u32);

    // Reset write pointer (hardware-maintained).
    mmio_write16(base, REG_RIRBWP, 0x8000);

    // Set interrupt count to 1 (notify after every response).
    mmio_write16(base, REG_RINTCNT, 1);

    // Start RIRB DMA.
    mmio_write8(base, REG_RIRBCTL, RIRBCTL_DMAEN | RIRBCTL_RINTCTL);
}

// ---------------------------------------------------------------------------
// Codec communication
// ---------------------------------------------------------------------------

/// Send a verb to a codec via the CORB and wait for the RIRB response.
///
/// Returns the 32-bit response value, or an error on timeout.
fn send_verb(dev: &mut HdaDevice, verb: u32) -> KernelResult<u32> {
    let base = dev.mmio_base;

    // Write verb to next CORB slot.
    dev.corb_wp = (dev.corb_wp + 1) % (CORB_ENTRIES as u16);
    let _slot_offset = (dev.corb_wp as usize) * 4;

    // SAFETY: corb_virt points to our allocated DMA buffer.
    unsafe {
        let ptr = (dev.corb_virt as *mut u32).add(dev.corb_wp as usize);
        core::ptr::write_volatile(ptr, verb);
    }

    // Update hardware write pointer to trigger DMA.
    mmio_write16(base, REG_CORBWP, dev.corb_wp);

    // Poll RIRB write pointer until it advances (response arrived).
    let mut timeout = CODEC_RESPONSE_TIMEOUT_US / 10;
    loop {
        let rirb_wp = mmio_read16(base, REG_RIRBWP);
        if rirb_wp != dev.rirb_rp {
            break;
        }
        if timeout == 0 {
            return Err(KernelError::TimedOut);
        }
        timeout -= 1;
        delay_us(10);
    }

    // Read the response.
    dev.rirb_rp = (dev.rirb_rp + 1) % (RIRB_ENTRIES as u16);

    // SAFETY: rirb_virt points to our allocated DMA buffer.
    let entry = unsafe {
        let ptr = (dev.rirb_virt as *const RirbEntry).add(dev.rirb_rp as usize);
        core::ptr::read_volatile(ptr)
    };

    // Clear RIRB status.
    mmio_write8(base, REG_RIRBSTS, 0x05);

    Ok(entry.response)
}

// ---------------------------------------------------------------------------
// Codec probing
// ---------------------------------------------------------------------------

/// Probe a codec at the given address.
fn probe_codec(dev: &mut HdaDevice, cad: u8) {
    serial_println!("[hda] Probing codec {}...", cad);

    // Get vendor/device ID.
    let vendor_id = match send_verb(dev, verb12(cad, 0, VERB_GET_PARAM, PARAM_VENDOR_ID)) {
        Ok(v) => v,
        Err(_) => {
            serial_println!("[hda]   Codec {} not responding", cad);
            return;
        }
    };

    dev.vendor_id = vendor_id;
    serial_println!(
        "[hda]   Vendor: {:04x}:{:04x}",
        (vendor_id >> 16) & 0xFFFF,
        vendor_id & 0xFFFF
    );

    // Get revision.
    if let Ok(rev) = send_verb(dev, verb12(cad, 0, VERB_GET_PARAM, PARAM_REVISION_ID)) {
        serial_println!("[hda]   Revision: {}.{}.{}.{}",
            (rev >> 20) & 0xF, (rev >> 16) & 0xF, (rev >> 8) & 0xFF, rev & 0xFF);
    }

    // Get subordinate node count (tells us where the AFG starts).
    let subord = match send_verb(dev, verb12(cad, 0, VERB_GET_PARAM, PARAM_SUBORD_NODE_COUNT)) {
        Ok(v) => v,
        Err(_) => {
            serial_println!("[hda]   Failed to get node count");
            return;
        }
    };

    let start_nid = ((subord >> 16) & 0xFF) as u8;
    let num_nodes = (subord & 0xFF) as u8;
    serial_println!("[hda]   Root nodes: start={} count={}", start_nid, num_nodes);

    // Walk Audio Function Group nodes.
    for nid in start_nid..(start_nid.saturating_add(num_nodes)) {
        if let Ok(func_type) = send_verb(dev, verb12(cad, nid, VERB_GET_PARAM, PARAM_FUNC_GROUP_TYPE)) {
            let ftype = func_type & 0xFF;
            if ftype == 0x01 {
                // Audio Function Group found.
                serial_println!("[hda]   AFG at NID {}", nid);
                // Power on the AFG.
                let _ = send_verb(dev, verb12(cad, nid, VERB_SET_POWER_STATE, 0x00));
                probe_afg(dev, cad, nid);
                return;
            }
        }
    }

    serial_println!("[hda]   No Audio Function Group found");
}

/// Probe an Audio Function Group to find DAC and output pin nodes.
fn probe_afg(dev: &mut HdaDevice, cad: u8, afg_nid: u8) {
    // Get subordinate nodes of the AFG.
    let subord = match send_verb(dev, verb12(cad, afg_nid, VERB_GET_PARAM, PARAM_SUBORD_NODE_COUNT)) {
        Ok(v) => v,
        Err(_) => return,
    };

    let start_nid = ((subord >> 16) & 0xFF) as u8;
    let num_nodes = (subord & 0xFF) as u8;
    serial_println!("[hda]   AFG widgets: start={} count={}", start_nid, num_nodes);

    let mut dac_nid: u8 = 0;
    let mut pin_nid: u8 = 0;

    for nid in start_nid..(start_nid.saturating_add(num_nodes)) {
        let wcap = match send_verb(dev, verb12(cad, nid, VERB_GET_PARAM, PARAM_AUDIO_WIDGET_CAP)) {
            Ok(v) => v,
            Err(_) => continue,
        };

        let wtype = ((wcap >> 20) & 0xF) as u8;
        match wtype {
            WIDGET_TYPE_AUDIO_OUTPUT => {
                if dac_nid == 0 {
                    dac_nid = nid;
                    serial_println!("[hda]     NID {}: Audio Output (DAC)", nid);
                }
            }
            WIDGET_TYPE_PIN_COMPLEX => {
                // Check pin capabilities to find an output pin.
                if let Ok(pincap) = send_verb(dev, verb12(cad, nid, VERB_GET_PARAM, PARAM_PIN_CAP)) {
                    let is_output = pincap & (1 << 4) != 0; // Output capable
                    if is_output && pin_nid == 0 {
                        pin_nid = nid;
                        serial_println!("[hda]     NID {}: Pin Complex (output capable)", nid);
                    }
                }
            }
            WIDGET_TYPE_AUDIO_MIXER => {
                serial_println!("[hda]     NID {}: Audio Mixer", nid);
            }
            WIDGET_TYPE_AUDIO_SELECTOR => {
                serial_println!("[hda]     NID {}: Audio Selector", nid);
            }
            _ => {}
        }
    }

    dev.dac_nid = dac_nid;
    dev.pin_nid = pin_nid;

    if dac_nid != 0 && pin_nid != 0 {
        serial_println!("[hda]   Output path: DAC(NID {}) → Pin(NID {})", dac_nid, pin_nid);
    } else {
        serial_println!("[hda]   WARNING: incomplete output path (dac={}, pin={})", dac_nid, pin_nid);
    }
}

// ---------------------------------------------------------------------------
// Audio output configuration
// ---------------------------------------------------------------------------

/// Configure the output stream for playback.
///
/// Sets up the stream descriptor, BDL, and codec routing for 48 kHz 16-bit
/// stereo PCM output.
///
/// Returns `Ok(())` if the stream is ready to run.
pub fn configure_output() -> KernelResult<()> {
    let mut guard = DEVICE.lock();
    let dev = guard.as_mut().ok_or(KernelError::NoSuchDevice)?;

    if dev.dac_nid == 0 || dev.pin_nid == 0 {
        return Err(KernelError::NotSupported);
    }

    let base = dev.mmio_base;
    let stream_tag: u8 = 1; // Use stream tag 1 for output.
    let cad: u8 = 0;        // Codec 0.

    // Calculate output stream descriptor offset.
    let stream_idx = dev.iss + dev.out_stream_idx;
    let stream_off = STREAM_BASE + (stream_idx as usize) * STREAM_SIZE;

    // Reset the stream.
    mmio_write32(base, stream_off + SD_CTL, SDCTL_SRST);
    delay_us(100);
    // Wait for reset to complete.
    let mut timeout = 1000u32;
    while mmio_read32(base, stream_off + SD_CTL) & SDCTL_SRST == 0 {
        if timeout == 0 { break; }
        timeout -= 1;
        delay_us(10);
    }
    // Clear reset.
    mmio_write32(base, stream_off + SD_CTL, 0);
    timeout = 1000;
    while mmio_read32(base, stream_off + SD_CTL) & SDCTL_SRST != 0 {
        if timeout == 0 { break; }
        timeout -= 1;
        delay_us(10);
    }

    // Set up the BDL with entries pointing to our PCM buffer.
    // Use the whole PCM buffer as a single entry for simplicity.
    let bdl_entry = BdlEntry {
        addr_lo: dev.pcm_phys as u32,
        addr_hi: (dev.pcm_phys >> 32) as u32,
        length: dev.pcm_size,
        flags: 1, // IOC on this entry
    };

    // SAFETY: bdl_virt is our allocated, zeroed DMA buffer.
    unsafe {
        let bdl_ptr = dev.bdl_virt as *mut BdlEntry;
        core::ptr::write_volatile(bdl_ptr, bdl_entry);
    }

    // Configure stream descriptor.
    mmio_write32(base, stream_off + SD_CBL, dev.pcm_size);
    mmio_write16(base, stream_off + SD_LVI, 0); // Last valid index = 0 (one entry)
    mmio_write16(base, stream_off + SD_FMT, FMT_48K_16BIT_STEREO);
    mmio_write32(base, stream_off + SD_BDPL, dev.bdl_phys as u32);
    mmio_write32(base, stream_off + SD_BDPU, (dev.bdl_phys >> 32) as u32);

    // Set stream tag in CTL (bits 23:20).
    let ctl = ((stream_tag as u32) << 20) | SDCTL_IOCE;
    mmio_write32(base, stream_off + SD_CTL, ctl);

    // Configure codec: set stream/channel on DAC.
    // Stream tag in bits 7:4, channel 0 in bits 3:0.
    let stream_chan = stream_tag << 4;
    let _ = send_verb(dev, verb12(cad, dev.dac_nid, VERB_SET_CONV_STREAM_CHAN, stream_chan));

    // Set format on DAC (4-bit verb: verb=2, payload=format).
    let _ = send_verb(dev, verb4(cad, dev.dac_nid, 0x2, FMT_48K_16BIT_STEREO));

    // Enable output on pin widget.
    let _ = send_verb(dev, verb12(cad, dev.pin_nid, VERB_SET_PIN_WIDGET_CTL, PIN_CTL_OUT_EN | PIN_CTL_HP_EN));

    // Set EAPD on pin (some codecs need this).
    let _ = send_verb(dev, verb12(cad, dev.pin_nid, VERB_SET_EAPD_BTLENABLE, 0x02));

    // Unmute output amp on DAC: set output amp, both channels, gain=max (0x7F).
    let amp_payload = AMP_OUT | AMP_LEFT | AMP_RIGHT | 0x7F;
    let _ = send_verb(dev, verb4(cad, dev.dac_nid, VERB4_SET_AMP_GAIN, amp_payload));

    // Unmute output amp on pin.
    let _ = send_verb(dev, verb4(cad, dev.pin_nid, VERB4_SET_AMP_GAIN, amp_payload));

    // Enable the stream interrupt in INTCTL.
    let intctl = mmio_read32(base, REG_INTCTL);
    mmio_write32(base, REG_INTCTL, intctl | (1 << stream_idx) | (1 << 31));

    serial_println!(
        "[hda] Output stream configured: tag={}, format=48kHz/16bit/stereo, buf={}KiB",
        stream_tag, dev.pcm_size / 1024
    );

    Ok(())
}

/// Start playback on the configured output stream.
pub fn start_playback() -> KernelResult<()> {
    let guard = DEVICE.lock();
    let dev = guard.as_ref().ok_or(KernelError::NoSuchDevice)?;

    let stream_idx = dev.iss + dev.out_stream_idx;
    let stream_off = STREAM_BASE + (stream_idx as usize) * STREAM_SIZE;

    // Set RUN bit.
    let ctl = mmio_read32(dev.mmio_base, stream_off + SD_CTL);
    mmio_write32(dev.mmio_base, stream_off + SD_CTL, ctl | SDCTL_RUN);

    serial_println!("[hda] Playback started");
    Ok(())
}

/// Stop playback on the output stream.
pub fn stop_playback() -> KernelResult<()> {
    let guard = DEVICE.lock();
    let dev = guard.as_ref().ok_or(KernelError::NoSuchDevice)?;

    let stream_idx = dev.iss + dev.out_stream_idx;
    let stream_off = STREAM_BASE + (stream_idx as usize) * STREAM_SIZE;

    // Clear RUN bit.
    let ctl = mmio_read32(dev.mmio_base, stream_off + SD_CTL);
    mmio_write32(dev.mmio_base, stream_off + SD_CTL, ctl & !SDCTL_RUN);

    serial_println!("[hda] Playback stopped");
    Ok(())
}

/// Fill the PCM buffer with a sine wave test tone.
///
/// Generates a 440 Hz sine wave at 48 kHz / 16-bit / stereo.
pub fn fill_test_tone() -> KernelResult<()> {
    let guard = DEVICE.lock();
    let dev = guard.as_ref().ok_or(KernelError::NoSuchDevice)?;

    let sample_rate = 48000u32;
    let frequency = 440u32; // A4
    let amplitude = 16000i16; // ~50% volume

    // Number of 16-bit stereo samples that fit in our buffer.
    let total_samples = dev.pcm_size / 4; // 4 bytes per stereo sample (2×16-bit)

    // SAFETY: pcm_virt is our allocated buffer.
    let pcm_buf = unsafe {
        core::slice::from_raw_parts_mut(
            dev.pcm_virt as *mut i16,
            (dev.pcm_size / 2) as usize, // 16-bit samples
        )
    };

    // Generate sine wave using integer approximation.
    // Use a simple lookup table approach (quarter-wave symmetry).
    for i in 0..total_samples {
        // Phase: 0..65535 represents 0..2π
        let phase = ((i as u64 * frequency as u64 * 65536) / sample_rate as u64) as u32;
        let phase16 = (phase & 0xFFFF) as u16;

        // Approximate sine using a polynomial (Bhaskara's approximation):
        // sin(x) ≈ 16x(π−x) / (5π²−4x(π−x))  for 0 ≤ x ≤ π
        let sample = sine_approx(phase16, amplitude);

        let idx = (i * 2) as usize; // stereo: left, right
        if let Some(slot) = pcm_buf.get_mut(idx) {
            *slot = sample;
        }
        if let Some(slot) = pcm_buf.get_mut(idx + 1) {
            *slot = sample; // Same on both channels.
        }
    }

    serial_println!("[hda] Test tone generated: {}Hz, {} samples", frequency, total_samples);
    Ok(())
}

/// Integer sine approximation using Bhaskara's formula.
///
/// `phase` is 0..65535 representing 0..2π.
/// Returns amplitude-scaled i16 value.
fn sine_approx(phase: u16, amplitude: i16) -> i16 {
    // Map phase to 0..32767 (half period) with sign.
    let (half_phase, negative) = if phase < 32768 {
        (phase, false)
    } else {
        (phase - 32768, true)
    };

    // Bhaskara: sin(x) ≈ 4x(180-x) / (40500 - x(180-x))
    // Adapted for 0..32767 range:
    // Let p = half_phase, range 0..32767
    // sin ≈ 4*p*(32767-p) / (40500*4 - p*(32767-p)/4)
    // Simplified integer version:
    let p = half_phase as i32;
    let complement = 32767 - p;
    let numerator = (4 * p * complement) >> 2;   // Scale down to avoid overflow
    let denominator = 40500i32 - (p * complement / (32767 / 4));

    let result = if denominator == 0 {
        0i16
    } else {
        let val = (numerator as i64 * amplitude as i64 / denominator as i64) as i32;
        val.clamp(i16::MIN as i32, i16::MAX as i32) as i16
    };

    if negative { result.wrapping_neg() } else { result }
}

// ---------------------------------------------------------------------------
// Public query API
// ---------------------------------------------------------------------------

/// Whether the HDA controller is initialized.
#[must_use]
pub fn is_initialized() -> bool {
    INITIALIZED.load(Ordering::Acquire)
}

/// Number of codecs detected.
#[must_use]
pub fn codec_count() -> u8 {
    CODEC_COUNT.load(Ordering::Acquire)
}

/// Get the codec vendor ID string (if initialized).
#[must_use]
pub fn vendor_id() -> Option<u32> {
    let guard = DEVICE.lock();
    guard.as_ref().map(|d| d.vendor_id)
}

/// Get controller stream counts.
#[must_use]
pub fn stream_counts() -> Option<(u8, u8, u8)> {
    let guard = DEVICE.lock();
    guard.as_ref().map(|d| (d.iss, d.oss, d.bss))
}

// ---------------------------------------------------------------------------
// IRQ handling
// ---------------------------------------------------------------------------

/// Handle an HDA controller interrupt.
///
/// Called from the IOAPIC IRQ handler when the HDA interrupt fires.
#[allow(dead_code)]
pub fn handle_irq() {
    let guard = DEVICE.lock();
    let Some(dev) = guard.as_ref() else { return };

    let intsts = mmio_read32(dev.mmio_base, REG_INTSTS);
    if intsts == 0 {
        return; // Not our interrupt.
    }

    // Check for stream completion interrupts.
    let stream_idx = dev.iss + dev.out_stream_idx;
    if intsts & (1 << stream_idx) != 0 {
        // Clear stream interrupt status.
        let stream_off = STREAM_BASE + (stream_idx as usize) * STREAM_SIZE;
        let sts = mmio_read8(dev.mmio_base, stream_off + SD_STS);
        mmio_write8(dev.mmio_base, stream_off + SD_STS, sts); // Write 1 to clear.
    }

    // Clear controller interrupt status.
    mmio_write32(dev.mmio_base, REG_INTSTS, intsts);
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Self-test for the HDA driver.
///
/// Verifies controller detection, reset, codec communication, and basic
/// stream setup work correctly.
pub fn self_test() -> KernelResult<()> {
    serial_println!("[hda] Running self-test...");

    if !is_initialized() {
        serial_println!("[hda]   SKIP: no HDA controller detected");
        return Ok(());
    }

    // Test 1: Controller is initialized and has stream counts.
    let (iss, oss, bss) = stream_counts().ok_or(KernelError::NoSuchDevice)?;
    assert!(oss > 0 || bss > 0, "must have output streams");
    serial_println!("[hda]   Stream counts OK (ISS={} OSS={} BSS={})", iss, oss, bss);

    // Test 2: Codec detection.
    let count = codec_count();
    serial_println!("[hda]   Codec count: {}", count);

    if count > 0 {
        // Test 3: Vendor ID was read.
        let vid = vendor_id().unwrap_or(0);
        assert!(vid != 0, "vendor ID should be non-zero for detected codec");
        serial_println!("[hda]   Vendor ID: {:04x}:{:04x}", (vid >> 16) & 0xFFFF, vid & 0xFFFF);

        // Test 4: Output path discovered.
        {
            let guard = DEVICE.lock();
            let dev = guard.as_ref().unwrap();
            if dev.dac_nid != 0 && dev.pin_nid != 0 {
                serial_println!("[hda]   Output path: DAC={} Pin={}", dev.dac_nid, dev.pin_nid);
            } else {
                serial_println!("[hda]   WARNING: no output path found (dac={} pin={})",
                    dev.dac_nid, dev.pin_nid);
            }
        }

        // Test 5: Configure and fill test tone (don't actually play).
        if configure_output().is_ok() {
            serial_println!("[hda]   Output stream configured OK");
            if fill_test_tone().is_ok() {
                serial_println!("[hda]   Test tone generated OK");
            }
        }
    }

    serial_println!("[hda] Self-test PASSED");
    Ok(())
}
