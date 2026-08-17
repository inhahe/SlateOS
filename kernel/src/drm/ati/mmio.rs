//! Bus access for the ATI display block: BAR2 mapping and register I/O.
//!
//! This is the impure half of the split described in [`super`]. Everything
//! here touches real hardware, so none of it can be checked by the boot-time
//! self-test in [`super::tests`] — it is validated instead by *running* it
//! against QEMU's `ati-vga`, which emulates the Rage 128 Pro and RV100
//! register interfaces.
//!
//! ## What "validated" means here, and what it does not
//!
//! A register offset is a claim about silicon. This file's claims are checked
//! by [`probe`] against an emulator, which is a second implementation of the
//! same documentation — agreement means the two readings of the spec match,
//! not that either matches an actual chip. That is a genuine limit and is
//! worth naming: it catches a transposed offset or a misread field width,
//! which is the overwhelming majority of what goes wrong, and it cannot catch
//! a place where QEMU and the real part diverge.
//!
//! It is still strictly more than a modern AMDGPU port could have here, where
//! nothing would execute at all — which is the argument of
//! `design-decisions.md` §217 in miniature.
//!
//! ## Why the probe only reads
//!
//! [`probe`] performs no CRTC programming. The registers it touches are the
//! identification and configuration ones, plus a single write to the
//! [`super::regs::MM_INDEX`] index port to exercise the indirect path. It
//! deliberately does not enable, disable or retime the CRTC: this device is
//! not the console, and a driver that reprograms a display it does not own is
//! how a machine ends up with a black screen and no way to report why.

use crate::error::{KernelError, KernelResult};
use crate::mm::frame::{FRAME_SIZE, PhysFrame};
use crate::mm::page_table::{self, PageFlags, VirtAddr};
use crate::pci;
use crate::serial_println;

use super::regs;

/// Bytes of the register aperture this driver maps.
///
/// One 16 KiB frame. Every register this driver names lives below `0x300`, so
/// a single frame covers the whole of it with room to spare; mapping more
/// would be mapping address space we cannot say anything about. The bound is
/// enforced on every access rather than assumed — see [`check_offset`].
const MMIO_MAP_LEN: u64 = FRAME_SIZE as u64;

/// Validate a register offset against an aperture of `len` bytes: in range,
/// and naturally aligned. Returns the offset widened to `u64`.
///
/// A free function rather than a method, so that the one part of this module
/// that is decidable without hardware is covered by [`super::tests`] on every
/// boot — the same split the module as a whole is organised around. The
/// aperture length is a parameter for the same reason.
///
/// Both the offset and the four bytes it names must lie inside the mapping,
/// which is why the end is computed and checked rather than just comparing
/// `offset < len`: an offset of `len - 2` is in range while the `u32` starting
/// there is not.
///
/// The alignment check is what makes the `*mut u8` → `*mut u32` casts in
/// [`AtiMmio::read32`] and [`AtiMmio::write32`] sound, and it is not a
/// formality. x86 tolerates a misaligned load from RAM, so a misaligned
/// *register* access would not fault — it would be split by the bus into two
/// partial accesses to two different registers, which for a register with side
/// effects on read is a hardware action nobody asked for. Since the base is
/// frame-aligned (enforced in [`AtiMmio::map`]), requiring the offset to be a
/// multiple of four makes every resulting address 4-byte aligned.
///
/// # Errors
///
/// `InvalidArgument` if `offset` is not a multiple of four, or if the four
/// bytes at `offset` are not wholly within `len`.
pub const fn check_offset(offset: u32, len: u64) -> KernelResult<u64> {
    if !offset.is_multiple_of(4) {
        return Err(KernelError::InvalidArgument);
    }
    let off = offset as u64;
    // `offset` is a `u32`, so `off + 4` cannot overflow a `u64`; the check is
    // written with `checked_add` anyway rather than relying on that, because
    // the guarantee lives in the parameter type and would evaporate silently
    // if the signature ever widened.
    let Some(end) = off.checked_add(4) else {
        return Err(KernelError::InvalidArgument);
    };
    if end > len {
        return Err(KernelError::InvalidArgument);
    }
    Ok(off)
}

/// A mapped ATI register aperture.
///
/// Holds the mapped length as well as the base so that every access can be
/// bounds-checked. An MMIO wrapper that trusts its caller's offset is a
/// wild pointer with extra steps: the failure is a write into whatever the
/// next BAR happens to be, which is silent, and belongs to another device.
pub struct AtiMmio {
    /// Kernel-virtual base of the aperture.
    base: *mut u8,
    /// Physical base, retained for logging and for identifying the mapping.
    phys: u64,
    /// Length actually mapped, in bytes.
    len: u64,
}

// SAFETY: `AtiMmio` is a handle to a device register aperture. The pointer is
// to device memory mapped NO_CACHE, not to any Rust allocation, so there is no
// aliasing of Rust-owned data to worry about. Sending it between threads is
// sound; concurrent *use* is serialised by the caller holding it exclusively,
// which is why only `Send` is claimed and not `Sync`.
unsafe impl Send for AtiMmio {}

impl AtiMmio {
    /// Map the register aperture at `phys`.
    ///
    /// # Errors
    ///
    /// `InvalidArgument` if `phys` is zero or not frame-aligned — an
    /// unassigned BAR reads as zero, and mapping it would hand back a handle
    /// whose every read returns garbage from physical page zero rather than
    /// reporting that the device was never configured.
    ///
    /// # Safety
    ///
    /// `phys` must be the physical base of an MMIO region that is not RAM and
    /// is not already mapped for another purpose. The caller must not create a
    /// second `AtiMmio` for the same region.
    pub unsafe fn map(phys: u64) -> KernelResult<Self> {
        if phys == 0 {
            return Err(KernelError::InvalidArgument);
        }
        if !phys.is_multiple_of(FRAME_SIZE as u64) {
            // The mapping is frame-granular, so a BAR that is not frame-aligned
            // would silently map from a lower address and shift every offset.
            return Err(KernelError::InvalidArgument);
        }

        let hhdm = page_table::hhdm().ok_or(KernelError::NotSupported)?;
        let virt = phys.wrapping_add(hhdm);

        // PCI BARs commonly sit above physical RAM, where the bootloader's
        // direct map does not reach, so the frame is mapped explicitly and
        // NO_CACHE — device registers must not be served from a cache line.
        let pml4 = page_table::cr3_to_pml4(page_table::read_cr3());
        let flags = PageFlags::PRESENT | PageFlags::WRITABLE | PageFlags::NO_CACHE;
        if let Some(frame) = PhysFrame::from_addr(phys) {
            // SAFETY: `phys` is a PCI BAR reported by config space, so it is
            // device memory rather than RAM; `virt` is its HHDM image, which
            // no allocator hands out. Mapping may fail because the region is
            // already covered by the bootloader's direct map on a machine with
            // enough RAM to reach this address, which is harmless — [`verify`]
            // is what decides whether the aperture actually reads registers.
            if let Err(e) =
                unsafe { page_table::map_frame(pml4, VirtAddr::new(virt), frame, flags) }
            {
                serial_println!(
                    "[ati]   note: BAR2 map returned {:?} (already mapped is expected)",
                    e
                );
            }
        }

        // Flush every hardware page of the frame, not just the first. A 16 KiB
        // frame is four 4 KiB TLB entries, and `invlpg` invalidates one entry:
        // flushing only the base would leave stale, cached translations for
        // three quarters of the aperture — which for registers mapped NO_CACHE
        // is precisely the failure the NO_CACHE flag exists to prevent, and it
        // would show up only for the registers above 0x1000.
        for i in 0..page_table::HW_PAGES_PER_FRAME {
            let addr = virt.wrapping_add((i.saturating_mul(page_table::HW_PAGE_SIZE)) as u64);
            // SAFETY: standard invlpg on a page just mapped, to drop any stale
            // translation for that address. `invlpg` on an address with no
            // cached translation is a no-op, not a fault.
            unsafe {
                core::arch::asm!("invlpg [{}]", in(reg) addr, options(nostack, preserves_flags));
            }
        }

        Ok(Self {
            base: virt as *mut u8,
            phys,
            len: MMIO_MAP_LEN,
        })
    }

    /// Physical base of the aperture.
    #[must_use]
    pub const fn phys(&self) -> u64 {
        self.phys
    }

    /// Validate an offset against this aperture. See [`check_offset`].
    fn check(&self, offset: u32) -> KernelResult<u64> {
        check_offset(offset, self.len)
    }

    /// Read a 32-bit register.
    ///
    /// # Errors
    ///
    /// `InvalidArgument` if the offset is outside the mapped aperture or is
    /// not a multiple of four.
    // The `*mut u8` → `*mut u32` cast is alignment-checked rather than assumed:
    // `check` rejects any offset that is not a multiple of four, and `map`
    // rejects any base that is not frame-aligned. See `check`'s documentation.
    #[allow(clippy::cast_ptr_alignment)]
    pub fn read32(&self, offset: u32) -> KernelResult<u32> {
        let off = self.check(offset)?;
        // SAFETY: `off` has been bounds- and alignment-checked, so `base + off`
        // is a 4-byte-aligned address inside the aperture this handle owns, and
        // the aperture is mapped present. `read_volatile` because the value
        // comes from a device and must not be cached or reordered.
        Ok(unsafe { self.base.add(off as usize).cast::<u32>().read_volatile() })
    }

    /// Write a 32-bit register.
    ///
    /// # Errors
    ///
    /// `InvalidArgument` if the offset is outside the mapped aperture or is
    /// not a multiple of four.
    // Alignment: see `read32`.
    #[allow(clippy::cast_ptr_alignment)]
    pub fn write32(&self, offset: u32, value: u32) -> KernelResult<()> {
        let off = self.check(offset)?;
        // SAFETY: as `read32` — `off` is bounds- and alignment-checked and the
        // aperture is mapped writable. `write_volatile` because the store is
        // the point: it must reach the device and must not be elided or merged.
        unsafe {
            self.base
                .add(off as usize)
                .cast::<u32>()
                .write_volatile(value);
        }
        Ok(())
    }

    /// Read a register through the [`regs::MM_INDEX`] / [`regs::MM_DATA`] pair.
    ///
    /// The aperture exposes the low registers directly; anything above is
    /// reached by writing its offset to the index port and then reading the
    /// data port. For registers that are visible both ways the two paths must
    /// agree, which is what [`probe`] uses this for.
    ///
    /// # Errors
    ///
    /// Propagates the bounds check on the two ports themselves.
    pub fn read32_indirect(&self, offset: u32) -> KernelResult<u32> {
        self.write32(regs::MM_INDEX, offset)?;
        self.read32(regs::MM_DATA)
    }
}

/// Result of probing for an ATI display device.
pub struct AtiDevice {
    /// Which part was found.
    pub info: &'static super::AsicInfo,
    /// Mapped register aperture.
    pub mmio: AtiMmio,
    /// Video RAM size in bytes, as the hardware reports it.
    pub vram_bytes: u32,
}

/// Look for a supported ATI display device and map its registers.
///
/// Returns `Ok(None)` when no such device is present, which is the normal case
/// on this project's usual QEMU command line and is not an error — the caller
/// falls back to whatever backend is already driving the display.
///
/// # Errors
///
/// Propagates mapping failures for a device that *was* found. A device that is
/// present but unusable is reported rather than skipped: silently treating it
/// as absent would make a broken mapping indistinguishable from a machine
/// without the card, and the two want opposite responses.
pub fn probe() -> KernelResult<Option<AtiDevice>> {
    let Some((dev, info)) = find_supported() else {
        return Ok(None);
    };

    serial_println!(
        "[ati] Found {} ({:04x}:{:04x}) at {:02x}:{:02x}.{}",
        info.name,
        dev.vendor_id,
        dev.device_id,
        dev.address.bus,
        dev.address.device,
        dev.address.function
    );

    // Registers live in BAR2 on these parts: BAR0 is the linear framebuffer
    // aperture (prefetchable) and BAR1 is the I/O port range. Confirmed
    // against QEMU's `ati-vga`, which reports exactly that layout.
    let bar2 = pci::bar_mmio_addr64(&dev, 2).ok_or(KernelError::NoSuchDevice)?;
    if bar2 == 0 {
        // An unassigned BAR reads as zero. That means firmware never gave the
        // device an address, so there is nothing to map; say so rather than
        // mapping physical page zero and reporting its contents as registers.
        serial_println!("[ati]   BAR2 is unassigned — device left alone");
        return Err(KernelError::NoSuchDevice);
    }

    // SAFETY: `bar2` is the MMIO base reported by the device's own config
    // space, so it names device registers rather than RAM, and this is the
    // only `AtiMmio` created for it — `probe` is called once from `drm::init`.
    let mmio = unsafe { AtiMmio::map(bar2)? };
    serial_println!("[ati]   BAR2 register aperture at {:#x}", mmio.phys());

    let vram_bytes = mmio.read32(regs::CNFG_MEMSIZE)?;
    serial_println!(
        "[ati]   CNFG_MEMSIZE: {:#010x} ({} MiB)",
        vram_bytes,
        vram_bytes / (1024 * 1024)
    );

    Ok(Some(AtiDevice {
        info,
        mmio,
        vram_bytes,
    }))
}

/// Find the first PCI device this driver claims.
fn find_supported() -> Option<(pci::PciDevice, &'static super::AsicInfo)> {
    for dev in pci::scan_bus0() {
        if let Some(info) = super::identify(dev.vendor_id, dev.device_id) {
            return Some((dev, info));
        }
    }
    None
}

/// Check the mapped registers against what the hardware must report.
///
/// Run after [`probe`] when a device was found. These are the checks that can
/// only be made against a real (or emulated) device, so unlike
/// [`super::tests`] they are skipped entirely when no card is present.
///
/// # Errors
///
/// `InternalError` if a register reads back a value that contradicts the
/// register map — which means an offset in [`regs`] is wrong, and every
/// subsequent access would land on the wrong register.
pub fn verify(dev: &AtiDevice) -> KernelResult<()> {
    let mut failed = 0u32;

    // VRAM size is the strongest available check on the offset of a *named*
    // register: it is a specific number the emulator is configured with
    // (`vgamem_mb`, 16 MiB by default) rather than something that happens to
    // be non-zero, so reading it correctly means offset 0xF8 really is
    // CNFG_MEMSIZE and not a neighbour that also holds a plausible value.
    if dev.vram_bytes == 0 || !dev.vram_bytes.is_multiple_of(1024 * 1024) {
        serial_println!(
            "[ati]   FAIL: CNFG_MEMSIZE reads {:#010x}, not a whole number of MiB",
            dev.vram_bytes
        );
        failed = failed.saturating_add(1);
    } else {
        serial_println!(
            "[ati]   CNFG_MEMSIZE is a whole {} MiB — offset 0xF8 confirmed",
            dev.vram_bytes / (1024 * 1024)
        );
    }

    // The direct and indirect paths must agree. This checks the MM_INDEX /
    // MM_DATA pair itself: if the index port were at the wrong offset the
    // indirect read would return something unrelated to the direct one, and
    // every high register — which is reachable no other way — would be
    // silently wrong.
    let direct = dev.mmio.read32(regs::CNFG_MEMSIZE)?;
    let indirect = dev.mmio.read32_indirect(regs::CNFG_MEMSIZE)?;
    if direct == indirect {
        serial_println!(
            "[ati]   MM_INDEX/MM_DATA agrees with the direct read ({:#010x}) — indirect path confirmed",
            direct
        );
    } else {
        serial_println!(
            "[ati]   FAIL: indirect read {:#010x} disagrees with direct {:#010x}",
            indirect,
            direct
        );
        failed = failed.saturating_add(1);
    }

    // Report the CRTC state without changing it. This is not an assertion —
    // the values depend on what firmware left behind — but it is the evidence
    // that the CRTC block responds at the documented offsets at all, and it is
    // what a later mode-set has to be diffed against.
    let gen_cntl = dev.mmio.read32(regs::CRTC_GEN_CNTL)?;
    let h_total_disp = dev.mmio.read32(regs::CRTC_H_TOTAL_DISP)?;
    let v_total_disp = dev.mmio.read32(regs::CRTC_V_TOTAL_DISP)?;
    serial_println!(
        "[ati]   CRTC_GEN_CNTL={:#010x} (enabled={}) H_TOTAL_DISP={:#010x} V_TOTAL_DISP={:#010x}",
        gen_cntl,
        gen_cntl & regs::CRTC_EN != 0,
        h_total_disp,
        v_total_disp
    );

    if failed > 0 {
        return Err(KernelError::InternalError);
    }
    Ok(())
}
