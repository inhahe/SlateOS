//! Device handles: how a userspace driver owns and reaches its PCI device
//! (design-decisions 1579).
//!
//! A driver process opens one PCI function and holds a handle to it; every
//! later operation names the handle, never an address. This module is the
//! object behind the handle -- ownership of the function, its BARs as sized
//! when it was opened, the configuration-space policy, and the BAR mappings
//! made in its owner -- and what happens on the last close: the device stops
//! doing DMA first, then everything mapped for it is taken back.
//!
//! The system calls are `SYS_PCI_DEVICE_*` (`syscall::handlers`); a handle is a
//! held handle of `ResourceType::PciDevice`, never inherited, so it has one
//! holder. Its holder's exit releases it at the zombie transition
//! ([`release_owned_by`]), before the address space its BARs are mapped in
//! is torn down. Not here yet (roadmap.md 2.1): DMA arenas in the device's
//! IOMMU domain, and interrupts.

use crate::error::{KernelError, KernelResult};
use crate::iommu_remap::{self, Caller, DmaPerms};
use crate::pci::{self, PciAddress};
use crate::proc::pcb::ProcessId;
use crate::sync::Mutex;
use alloc::vec::Vec;

/// PCI configuration-space offsets this module reads or guards.
const CFG_COMMAND: u8 = 0x04;
const CFG_BAR0: u8 = 0x10;
const CFG_ROM: u8 = 0x30;
/// Bits of the command register.
const CMD_MEMORY_SPACE: u16 = 1 << 1;
/// Capability ids whose registers the kernel manages.
const CAP_MSI: u8 = 0x05;
const CAP_MSIX: u8 = 0x11;
/// The span of an MSI capability at its largest (64-bit, per-vector
/// masking) and of an MSI-X capability: what a write may not touch.
const CAP_MSI_LEN: u8 = 24;
const CAP_MSIX_LEN: u8 = 12;
/// The hardware page BARs are mapped in: a BAR smaller than this shares its
/// page with other registers, perhaps another device's, and is not mapped.
const HW_PAGE: u64 = 4096;

/// A memory BAR, as sized when its device was opened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemoryBar {
    /// Physical base address.
    pub base: u64,
    /// Size in bytes (a power of two).
    pub size: u64,
    /// Prefetchable: may be mapped write-combining.
    pub prefetch: bool,
}

/// One BAR mapped into the owner: removed when the device is released.
#[derive(Debug, Clone, Copy)]
struct MappedBar {
    user_virt: u64,
    pages: u64,
}

/// What an open device is.
struct Device {
    /// The raw handle value the owner's handle table refers to.
    raw: u64,
    addr: PciAddress,
    owner: ProcessId,
    /// Its memory BARs by index; `None` for an unimplemented, I/O-port or
    /// upper-half-of-64-bit register.
    bars: [Option<MemoryBar>; 6],
    mapped: Vec<MappedBar>,
    /// The IOMMU domain the device's DMA is confined to, made at open; `None`
    /// without an active IOMMU, when nothing confines it.
    domain: Option<iommu_remap::DomainId>,
    /// DMA arenas mapped in the domain.
    arenas: Vec<Arena>,
}

/// A DMA arena: memory the device may read and write, mapped in its owner
/// and in the device's domain. The device's address for it is its physical
/// address (the domain maps each arena at its own address).
#[derive(Debug, Clone, Copy)]
struct Arena {
    user_virt: u64,
    phys: u64,
}

static DEVICES: Mutex<Vec<Device>> = Mutex::new(Vec::new());
/// The next raw handle value; never reused, so a stale handle names nothing.
static NEXT_RAW: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(1);

/// The name a function opened by a process is recorded under in
/// `pci::bound_driver`, so the kernel's own drivers and a second open both
/// see it as taken.
const USERSPACE_DRIVER: &str = "userspace driver";

/// Open `addr` for `owner`, returning the raw handle.
///
/// The caller has checked the capability. Refused with `NotFound` when no
/// function answers there, and `DeviceBusy` when a kernel driver or another
/// open has it.
pub fn open(owner: ProcessId, addr: PciAddress) -> KernelResult<u64> {
    if pci::config_read16(addr.bus, addr.device, addr.function, 0x00) == 0xFFFF {
        return Err(KernelError::NotFound);
    }
    let mut devices = DEVICES.lock();
    if pci::bound_driver(addr).is_some() || devices.iter().any(|d| d.addr == addr) {
        return Err(KernelError::DeviceBusy);
    }
    // Sized now, before its driver starts: sizing writes all-ones to each
    // BAR, which a running driver would see.
    let bars = size_memory_bars(addr);
    let domain = confine(addr)?;
    let raw = NEXT_RAW.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
    pci::bind_driver(addr, USERSPACE_DRIVER);
    devices.push(Device {
        raw,
        addr,
        owner,
        bars,
        mapped: Vec::new(),
        domain,
        arenas: Vec::new(),
    });
    Ok(raw)
}

/// Release the device `raw` names. A handle already released -- by its
/// holder's exit, before the reap that closes its handles -- names nothing,
/// and closing it does nothing.
pub fn close(raw: u64) {
    let released = {
        let mut devices = DEVICES.lock();
        let Some(index) = devices.iter().position(|d| d.raw == raw) else {
            return;
        };
        devices.swap_remove(index)
    };
    release(released);
}

/// Release every device `owner` holds. Called as it becomes a zombie, while
/// its page table still exists: the BARs mapped in it must be unmapped
/// before the address space is torn down, which would otherwise free them as
/// RAM.
pub fn release_owned_by(owner: ProcessId) {
    let released: Vec<Device> = {
        let mut devices = DEVICES.lock();
        let (mine, others) = core::mem::take(&mut *devices)
            .into_iter()
            .partition(|d| d.owner == owner);
        *devices = others;
        mine
    };
    for device in released {
        release(device);
    }
}

/// Take back everything an open device was given, in the order that is safe
/// while it may still be running: bus mastering off first, so it stops
/// reading and writing memory, then its mappings.
fn release(device: Device) {
    pci::disable_bus_master(device.addr);
    let pml4 = crate::proc::pcb::get_pml4(device.owner).filter(|&p| p != 0);
    if let Some(domain) = device.domain {
        // Detached and destroyed before any arena is freed: the device can
        // reach none of them from here on, whatever it was doing.
        let (b, d, f) = (device.addr.bus, device.addr.device, device.addr.function);
        if let Err(e) = iommu_remap::detach_device_as(Caller::Kernel, domain, b, d, f)
            .and_then(|()| iommu_remap::destroy_domain_as(Caller::Kernel, domain))
        {
            crate::serial_println!(
                "[devhandle] {:?}: its IOMMU domain could not be released: {:?}",
                device.addr,
                e
            );
        }
    }
    if let Some(pml4) = pml4 {
        // The arenas' tracking entries are the DMA module's: freed through it,
        // they leave its table too. Without the owner's page table (a process
        // destroyed outright) its address space's teardown frees them.
        for arena in &device.arenas {
            if let Err(e) = crate::mm::dma::free_for_user(pml4, arena.user_virt) {
                crate::serial_println!(
                    "[devhandle] {:?}: an arena could not be freed: {:?}",
                    device.addr,
                    e
                );
            }
        }
        for bar in &device.mapped {
            unmap_pages(pml4, bar.user_virt, bar.pages);
        }
    }
    pci::unbind_driver(device.addr, USERSPACE_DRIVER);
}

/// The IOMMU domain a newly opened device's DMA is confined to: made, and
/// the device attached, when an IOMMU is active; `None` when none is, and the
/// log says the device is not confined. A device the IOMMU cannot be given
/// alone is refused rather than opened unconfined.
fn confine(addr: PciAddress) -> KernelResult<Option<iommu_remap::DomainId>> {
    if !iommu_remap::is_active() {
        crate::serial_println!(
            "[devhandle] {:02x}:{:02x}.{}: no active IOMMU, so its DMA is not confined",
            addr.bus,
            addr.device,
            addr.function
        );
        return Ok(None);
    }
    let domain = iommu_remap::create_domain_as(Caller::Kernel)?;
    if let Err(e) =
        iommu_remap::attach_device_as(Caller::Kernel, domain, addr.bus, addr.device, addr.function)
    {
        // Nothing is mapped in it yet; its own failure to go leaves nothing
        // reachable, and the refusal is what the caller needs to see.
        let _ = iommu_remap::destroy_domain_as(Caller::Kernel, domain);
        return Err(e);
    }
    Ok(Some(domain))
}

/// Allocate a DMA arena of at least `size` bytes for the device `raw` names,
/// owned by `pid`: mapped in `pid` and in the device's domain. Returns (the
/// address `pid` sees, the address the device must be given).
pub fn dma_alloc(raw: u64, pid: ProcessId, size: u64) -> KernelResult<(u64, u64)> {
    use crate::mm::dma::{self, DmaConstraint};

    let pml4 = crate::proc::pcb::get_pml4(pid)
        .filter(|&p| p != 0)
        .ok_or(KernelError::NoSuchProcess)?;
    let domain = with_owned(raw, pid, |d| Ok(d.domain))?;
    let size = usize::try_from(size).map_err(|_| KernelError::InvalidArgument)?;
    let (user_virt, phys, size) = dma::alloc_for_user(pml4, size, DmaConstraint::None)?;
    if let Some(domain) = domain
        && let Err(e) = iommu_remap::map_dma_as(
            Caller::Kernel,
            domain,
            phys,
            phys,
            size,
            DmaPerms::READ_WRITE,
        )
    {
        // Never given to the device: freeing it needs nothing revoked.
        let _ = dma::free_for_user(pml4, user_virt);
        return Err(e);
    }
    dma::mark_device(pml4, user_virt, raw);
    let recorded = with_owned(raw, pid, |d| {
        d.arenas.push(Arena { user_virt, phys });
        Ok(())
    });
    if let Err(e) = recorded {
        // Closed while it was being made: free it through the tracker, which
        // revokes the (now gone) device first.
        let _ = dma::free_for_user(pml4, user_virt);
        return Err(e);
    }
    Ok((user_virt, phys))
}

/// The device `raw` names stops reaching the arena at `phys` (`size` bytes),
/// which is about to be freed: it is unmapped from the device's domain, so a
/// late access faults in the IOMMU rather than landing in freed memory.
/// Called by the DMA module before it frees any buffer marked for a device; a
/// device already released, or an arena already revoked, has nothing to do.
pub fn revoke_arena(raw: u64, phys: u64, size: usize) {
    let mut devices = DEVICES.lock();
    let Some(device) = devices.iter_mut().find(|d| d.raw == raw) else {
        return;
    };
    let Some(index) = device.arenas.iter().position(|a| a.phys == phys) else {
        return;
    };
    device.arenas.swap_remove(index);
    if let Some(domain) = device.domain
        && let Err(e) = iommu_remap::unmap_dma_as(Caller::Kernel, domain, phys, size)
    {
        crate::serial_println!(
            "[devhandle] {:?}: an arena could not be revoked from its domain: {:?}",
            device.addr,
            e
        );
    }
}

/// The IOMMU domain of the device `raw` names, if `pid` owns it.
pub fn domain_of(raw: u64, pid: ProcessId) -> KernelResult<Option<iommu_remap::DomainId>> {
    with_owned(raw, pid, |d| Ok(d.domain))
}

/// The memory BARs of the device `raw` names, if `pid` owns it.
pub fn bars(raw: u64, pid: ProcessId) -> KernelResult<[Option<MemoryBar>; 6]> {
    with_owned(raw, pid, |d| Ok(d.bars))
}

/// Read `width` bytes (1, 2 or 4, naturally aligned) of configuration space
/// at `offset`.
pub fn config_read(raw: u64, pid: ProcessId, offset: u16, width: u8) -> KernelResult<u32> {
    let addr = with_owned(raw, pid, |d| Ok(d.addr))?;
    let offset = config_offset(offset, width)?;
    let (b, d, f) = (addr.bus, addr.device, addr.function);
    Ok(match width {
        1 => u32::from(pci::config_read8(b, d, f, offset)),
        2 => u32::from(pci::config_read16(b, d, f, offset)),
        _ => pci::config_read32(b, d, f, offset),
    })
}

/// Write `width` bytes of configuration space at `offset`. The registers the
/// kernel manages -- the BARs, the expansion ROM, and the MSI and MSI-X
/// capabilities -- are refused with `PermissionDenied`.
pub fn config_write(
    raw: u64,
    pid: ProcessId,
    offset: u16,
    width: u8,
    value: u32,
) -> KernelResult<()> {
    let addr = with_owned(raw, pid, |d| Ok(d.addr))?;
    let offset = config_offset(offset, width)?;
    if config_write_refused(addr, offset, width) {
        return Err(KernelError::PermissionDenied);
    }
    let (b, d, f) = (addr.bus, addr.device, addr.function);
    #[allow(clippy::cast_possible_truncation)]
    match width {
        1 => pci::config_write8(b, d, f, offset, value as u8),
        2 => pci::config_write16(b, d, f, offset, value as u16),
        _ => pci::config_write32(b, d, f, offset, value),
    }
    Ok(())
}

/// Map memory BAR `index` of the device `raw` names into its owner `pid`,
/// uncached, and return (address, size). Refused with `InvalidArgument` for
/// a BAR that is not memory, and `NotSupported` for one smaller than a 4 KiB
/// page or not aligned to one, which cannot be mapped without exposing the
/// registers beside it.
pub fn map_bar(raw: u64, pid: ProcessId, index: usize) -> KernelResult<(u64, u64)> {
    use crate::mm::page_table::{self, PageFlags, VirtAddr};

    let pml4 = crate::proc::pcb::get_pml4(pid)
        .filter(|&p| p != 0)
        .ok_or(KernelError::NoSuchProcess)?;
    let bar = with_owned(raw, pid, |d| {
        d.bars
            .get(index)
            .copied()
            .flatten()
            .ok_or(KernelError::InvalidArgument)
    })?;
    if bar.size < HW_PAGE || !bar.base.is_multiple_of(HW_PAGE) {
        return Err(KernelError::NotSupported);
    }
    let pages = bar.size / HW_PAGE;
    let user_virt = crate::syscall::handlers::mmap_alloc_vaddr(bar.size);
    if user_virt == 0 {
        return Err(KernelError::OutOfMemory);
    }
    let flags = PageFlags::PRESENT
        | PageFlags::WRITABLE
        | PageFlags::USER_ACCESSIBLE
        | PageFlags::NO_CACHE
        | PageFlags::NO_EXECUTE
        | PageFlags::SHARED;
    for page in 0..pages {
        let offset = page.saturating_mul(HW_PAGE);
        // SAFETY: `pml4` is the owner's own table; the address is in the
        // device mmap region, where nothing else maps; the physical page is
        // the device's own BAR, which the allocator does not own.
        let mapped = unsafe {
            page_table::map_4k_if_absent(
                pml4,
                VirtAddr::new(user_virt.saturating_add(offset)),
                bar.base.saturating_add(offset),
                flags,
            )
        };
        if let Err(e) = mapped {
            unmap_pages(pml4, user_virt, page);
            return Err(e);
        }
    }
    let recorded = with_owned(raw, pid, |d| {
        d.mapped.push(MappedBar { user_virt, pages });
        Ok(())
    });
    if let Err(e) = recorded {
        // Closed while it was being mapped: nothing would ever unmap it.
        unmap_pages(pml4, user_virt, pages);
        return Err(e);
    }
    Ok((user_virt, bar.size))
}

/// Run `f` on the device `raw` names if `pid` owns it: `InvalidHandle` when
/// it names nothing, `PermissionDenied` when it is another process's.
fn with_owned<T>(
    raw: u64,
    pid: ProcessId,
    f: impl FnOnce(&mut Device) -> KernelResult<T>,
) -> KernelResult<T> {
    let mut devices = DEVICES.lock();
    let device = devices
        .iter_mut()
        .find(|d| d.raw == raw)
        .ok_or(KernelError::InvalidHandle)?;
    if device.owner != pid {
        return Err(KernelError::PermissionDenied);
    }
    f(device)
}

/// A configuration-space access's offset, checked: within the 256 bytes the
/// legacy mechanism reaches, and naturally aligned for its width.
fn config_offset(offset: u16, width: u8) -> KernelResult<u8> {
    if !matches!(width, 1 | 2 | 4) || !offset.is_multiple_of(u16::from(width)) {
        return Err(KernelError::InvalidArgument);
    }
    u8::try_from(offset).map_err(|_| KernelError::InvalidArgument)
}

/// Whether a write of `width` bytes at `offset` touches a register the
/// kernel manages.
fn config_write_refused(addr: PciAddress, offset: u8, width: u8) -> bool {
    let start = u16::from(offset);
    let end = start.saturating_add(u16::from(width));
    let overlaps = |from: u16, len: u16| start < from.saturating_add(len) && from < end;
    if overlaps(u16::from(CFG_BAR0), 24) || overlaps(u16::from(CFG_ROM), 4) {
        return true;
    }
    pci::walk_capabilities(addr).iter().any(|cap| match cap.id {
        CAP_MSI => overlaps(u16::from(cap.offset), u16::from(CAP_MSI_LEN)),
        CAP_MSIX => overlaps(u16::from(cap.offset), u16::from(CAP_MSIX_LEN)),
        _ => false,
    })
}

/// Size each memory BAR of `addr` by the standard probe: memory decoding off,
/// all-ones written, the read-back's low set bit is the size, then the
/// original value and the command register restored.
fn size_memory_bars(addr: PciAddress) -> [Option<MemoryBar>; 6] {
    let (b, d, f) = (addr.bus, addr.device, addr.function);
    let mut bars = [None; 6];
    let command = pci::config_read16(b, d, f, CFG_COMMAND);
    pci::config_write16(b, d, f, CFG_COMMAND, command & !CMD_MEMORY_SPACE);
    let mut index: usize = 0;
    while index < 6 {
        let offset =
            CFG_BAR0.saturating_add(u8::try_from(index.saturating_mul(4)).unwrap_or(u8::MAX));
        let low = pci::config_read32(b, d, f, offset);
        let is_io = low & 1 != 0;
        let is_64 = !is_io && (low >> 1) & 0b11 == 0b10;
        // A memory BAR reading 0 is unassigned (a 64-bit one never reads 0:
        // its type bits are set), so there is nothing to map either way.
        if is_io || low == 0 {
            index = index.saturating_add(1);
            continue;
        }
        pci::config_write32(b, d, f, offset, u32::MAX);
        let size_low = pci::config_read32(b, d, f, offset) & !0xF;
        pci::config_write32(b, d, f, offset, low);
        let (base, mask) = if is_64 && index < 5 {
            let high_offset = offset.saturating_add(4);
            let high = pci::config_read32(b, d, f, high_offset);
            pci::config_write32(b, d, f, high_offset, u32::MAX);
            let size_high = pci::config_read32(b, d, f, high_offset);
            pci::config_write32(b, d, f, high_offset, high);
            (
                u64::from(high) << 32 | u64::from(low & !0xF),
                u64::from(size_high) << 32 | u64::from(size_low),
            )
        } else {
            (
                u64::from(low & !0xF),
                u64::from(size_low) | 0xFFFF_FFFF_0000_0000,
            )
        };
        let size = (!mask).wrapping_add(1);
        if let Some(slot) = bars.get_mut(index)
            && base != 0
            && size.is_power_of_two()
        {
            *slot = Some(MemoryBar {
                base,
                size,
                prefetch: low & 0b1000 != 0,
            });
        }
        index = index.saturating_add(if is_64 { 2 } else { 1 });
    }
    pci::config_write16(b, d, f, CFG_COMMAND, command);
    bars
}

/// Clear `pages` 4 KiB entries from `user_virt` in `pml4`, and flush them.
fn unmap_pages(pml4: u64, user_virt: u64, pages: u64) {
    use crate::mm::page_table::{self, VirtAddr};
    for page in 0..pages {
        let va = user_virt.saturating_add(page.saturating_mul(HW_PAGE));
        // SAFETY: the page was mapped by `map_bar` into this table; the frame
        // behind it is device memory, which clearing the entry does not free.
        // An entry already absent leaves nothing to clear.
        let _ = unsafe { page_table::unmap_4k(pml4, VirtAddr::new(va)) };
    }
    crate::tlb::flush_range(user_virt, u32::try_from(pages).unwrap_or(u32::MAX));
}

/// QEMU's `edu` device, which the boot test attaches for this test: a 1 MiB
/// memory BAR and an MSI capability, and no kernel driver.
const EDU_VENDOR: u16 = 0x1234;
const EDU_DEVICE: u16 = 0x11E8;

/// Open, configure, map and close QEMU's `edu` device as a scratch process,
/// and check every refusal on the way: a second open, another process's
/// access, writes to the BARs and the MSI capability. Skipped, and recorded
/// as skipped, where there is no `edu` (anywhere but the boot test).
pub fn self_test() -> KernelResult<()> {
    use crate::mm::page_table::{self, VirtAddr};
    use crate::proc::pcb;
    use crate::serial_println;

    let mut skips = crate::fs::selftest::Skips::new();
    let Some(edu) = pci::find_device(EDU_VENDOR, EDU_DEVICE) else {
        skips.record("edu device", "no QEMU edu device attached");
        skips.report("[devhandle]");
        return Ok(());
    };
    let addr = edu.address;
    let owner = pcb::create("devhandle-owner", 0);
    let stranger = pcb::create("devhandle-stranger", 0);
    let result = self_test_on(addr, owner, stranger);
    pcb::destroy(owner);
    pcb::destroy(stranger);
    let confined = match result {
        Ok(confined) => confined,
        Err(msg) => {
            serial_println!("[devhandle]   FAIL: {}", msg);
            return Err(KernelError::InternalError);
        }
    };
    if !confined {
        skips.record("IOMMU confinement", "no active IOMMU");
    }
    serial_println!("[devhandle]   open, configure, map, DMA arenas and close edu: OK");
    skips.report("[devhandle]");

    fn self_test_on(
        addr: PciAddress,
        owner: ProcessId,
        stranger: ProcessId,
    ) -> Result<bool, &'static str> {
        let pml4 = pcb::get_pml4(owner)
            .filter(|&p| p != 0)
            .ok_or("the scratch process has no address space")?;
        let raw = open(owner, addr).map_err(|_| "edu could not be opened")?;
        let checks = (|| {
            if open(stranger, addr) != Err(KernelError::DeviceBusy) {
                return Err("a second open of an open device was not refused");
            }
            if bars(raw, stranger).is_ok() || config_read(raw, stranger, 0, 2).is_ok() {
                return Err("another process reached the device through the owner's handle");
            }
            let bar0 = bars(raw, owner).map_err(|_| "the owner could not read its BARs")?[0]
                .ok_or("edu's BAR0 was not found as a memory BAR")?;
            if bar0.size != 1 << 20 {
                return Err("edu's BAR0 was not sized 1 MiB");
            }
            if config_read(raw, owner, 0, 2) != Ok(u32::from(EDU_VENDOR)) {
                return Err("configuration space did not read edu's vendor id");
            }
            if config_write(raw, owner, u16::from(CFG_BAR0), 4, 0)
                != Err(KernelError::PermissionDenied)
            {
                return Err("a write to BAR0 was not refused");
            }
            let msi = pci::find_capability(addr, CAP_MSI).ok_or("edu has no MSI capability")?;
            if config_write(raw, owner, u16::from(msi.offset).saturating_add(4), 4, 0)
                != Err(KernelError::PermissionDenied)
            {
                return Err("a write into the MSI capability was not refused");
            }
            // Bus mastering on, through the owner's own write -- the command
            // register is the driver's to set -- so that the release below
            // has something to turn off.
            let command = config_read(raw, owner, u16::from(CFG_COMMAND), 2)
                .map_err(|_| "the command register could not be read")?;
            if config_write(raw, owner, u16::from(CFG_COMMAND), 2, command | 1 << 2).is_err() {
                return Err("the owner's write to the command register was refused");
            }
            let (va, size) = map_bar(raw, owner, 0).map_err(|_| "BAR0 could not be mapped")?;
            let last = size.saturating_sub(HW_PAGE);
            let at =
                |offset: u64| page_table::translate(pml4, VirtAddr::new(va.saturating_add(offset)));
            if at(0) != Some(bar0.base) || at(last) != Some(bar0.base.saturating_add(last)) {
                return Err("the mapping does not reach BAR0's own pages");
            }
            if at(size).is_some() {
                return Err("the mapping runs past BAR0");
            }
            // A DMA arena: where the owner sees it is its device address,
            // and the device's domain reaches it until it is freed.
            let (arena, device_addr) =
                dma_alloc(raw, owner, 0x4000).map_err(|_| "a DMA arena could not be made")?;
            if page_table::translate(pml4, VirtAddr::new(arena)) != Some(device_addr) {
                return Err("the arena is not at its device address");
            }
            let Some(domain) = domain_of(raw, owner).map_err(|_| "no domain answer")? else {
                return Ok((va, None));
            };
            if iommu_remap::translate_bus(domain, device_addr) != Some(device_addr) {
                return Err("the device's domain does not reach its arena");
            }
            crate::mm::dma::free_for_user(pml4, arena)
                .map_err(|_| "the arena could not be freed")?;
            if iommu_remap::translate_bus(domain, device_addr).is_some() {
                return Err("a freed arena is still reachable by the device");
            }
            // A second, left for the close to take back.
            let (_kept, kept_addr) =
                dma_alloc(raw, owner, 0x4000).map_err(|_| "a second arena could not be made")?;
            Ok((va, Some((domain, kept_addr))))
        })();
        let (va, kept) = match checks {
            Ok(found) => found,
            Err(msg) => {
                close(raw);
                return Err(msg);
            }
        };
        close(raw);
        if pci::bound_driver(addr).is_some() {
            return Err("the function is still bound after its last close");
        }
        if page_table::translate(pml4, VirtAddr::new(va)).is_some() {
            return Err("BAR0 is still mapped after the last close");
        }
        if let Some((domain, kept_addr)) = kept
            && iommu_remap::translate_bus(domain, kept_addr).is_some()
        {
            return Err("an arena is still reachable by the device after the last close");
        }
        let command = pci::config_read16(addr.bus, addr.device, addr.function, CFG_COMMAND);
        if command & (1 << 2) != 0 {
            return Err("bus mastering is still on after the last close");
        }
        Ok(kept.is_some())
    }
    Ok(())
}
