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
//! holder. Its holder's exit closes it with the rest of its held handles
//! (`pcb::exit_close_fds`), before the address space its BARs and arenas are
//! mapped in is released. Interrupts are MSI, one vector, taken or waited
//! for through the handle (`msi` delivers them); MSI-X, INTx and readiness
//! in `SYS_WAIT_MULTIPLE` are not here yet (roadmap.md 2.1).

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
const CFG_HEADER_TYPE: u8 = 0x0E;
/// Bits of the command register.
const CMD_MEMORY_SPACE: u16 = 1 << 1;
/// Capability ids whose registers the kernel manages.
const CAP_MSI: u8 = 0x05;
const CAP_MSIX: u8 = 0x11;
const CAP_PM: u8 = 0x01;
const CAP_PCIE: u8 = 0x10;
const CAP_AF: u8 = 0x13;
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
    /// The BAR's physical base: an entry is cleared only while it maps it.
    phys: u64,
    /// The region reserved for it, in whole frames.
    span: u64,
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
    guards: Guards,
    /// DMA arenas mapped in the domain.
    arenas: Vec<Arena>,
    /// Its MSI, once set up: the vector its messages arrive on.
    msi: Option<crate::msi::MsiConfig>,
    /// An MSI set-up in progress (made outside the table's lock).
    msi_busy: bool,
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
/// Functions being opened: claimed under [`DEVICES`]'s lock (taken first),
/// made outside it, so a second open of the same function is refused while
/// the first is still sizing it.
static OPENING: Mutex<Vec<PciAddress>> = Mutex::new(Vec::new());
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
    // A bridge's header (type 1) has two BARs and its own windows where a
    // device's has six: what is sized and guarded here is a device's.
    if pci::config_read8(addr.bus, addr.device, addr.function, CFG_HEADER_TYPE) & 0x7F != 0 {
        return Err(KernelError::NotSupported);
    }
    // Claimed under the table's lock, then made outside it: sizing the BARs,
    // reading the capabilities and confining the device are dozens of port
    // accesses and an IOMMU invalidation, too long to hold a spinlock across.
    {
        let devices = DEVICES.lock();
        let mut opening = OPENING.lock();
        if pci::bound_driver(addr).is_some()
            || devices.iter().any(|d| d.addr == addr)
            || opening.contains(&addr)
        {
            return Err(KernelError::DeviceBusy);
        }
        opening.push(addr);
    }
    let made = (|| {
        // Sized now, before its driver starts: sizing writes all-ones to each
        // BAR, which a running driver would see.
        let bars = size_memory_bars(addr);
        let guards = Guards::of(addr);
        let domain = confine(addr)?;
        pci::bind_driver(addr, USERSPACE_DRIVER);
        Ok(Device {
            raw: NEXT_RAW.fetch_add(1, core::sync::atomic::Ordering::Relaxed),
            addr,
            owner,
            bars,
            mapped: Vec::new(),
            domain,
            guards,
            arenas: Vec::new(),
            msi: None,
            msi_busy: false,
        })
    })();
    let mut devices = DEVICES.lock();
    OPENING.lock().retain(|&a| a != addr);
    let device = made?;
    let raw = device.raw;
    devices.push(device);
    Ok(raw)
}

/// Release the device `raw` names. A handle that names nothing (already
/// released) does nothing.
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

/// Take back everything an open device was given, in the order that is safe
/// while it may still be running: bus mastering off first, so it stops
/// reading and writing memory, then its mappings.
fn release(device: Device) {
    pci::disable_bus_master(device.addr);
    // Bus mastering is off, so the device sends no more messages. Its
    // vector's waiter is woken and its count dropped while the vector is
    // still this device's, then MSI is turned off and the vector freed.
    if let Some(mut config) = device.msi {
        crate::msi::release_delivery(config.allocated_vector, device.raw);
        crate::msi::disable_msi(&mut config);
    }
    let pml4 = crate::proc::pcb::get_pml4(device.owner).filter(|&p| p != 0);
    // Detached and destroyed before any arena is freed: the device can reach
    // none of them from here on, whatever it was doing.
    let mut reachable = false;
    if let Some(domain) = device.domain {
        let (b, d, f) = (device.addr.bus, device.addr.device, device.addr.function);
        if let Err(e) = iommu_remap::detach_device_as(Caller::Kernel, domain, b, d, f)
            .and_then(|()| iommu_remap::destroy_domain_as(Caller::Kernel, domain))
        {
            crate::serial_println!(
                "[devhandle] {:?}: its IOMMU domain could not be released: {:?}",
                device.addr,
                e
            );
            reachable = true;
        }
    }
    if let Some(pml4) = pml4 {
        // The arenas' tracking entries are the DMA module's: freed through it,
        // they leave its table too. Without the owner's page table (a process
        // destroyed outright) its address space's teardown frees them. One
        // the device may still reach -- its domain would not let go -- is
        // abandoned, not freed: its frames given out again would be the
        // device's to write.
        for arena in &device.arenas {
            if reachable {
                crate::mm::dma::abandon_for_user(pml4, arena.user_virt);
                continue;
            }
            if let Err(e) = crate::mm::dma::free_for_user(pml4, arena.user_virt) {
                crate::serial_println!(
                    "[devhandle] {:?}: an arena could not be freed: {:?}",
                    device.addr,
                    e
                );
            }
        }
        for bar in &device.mapped {
            unmap_bar(pml4, device.owner, bar);
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
/// owned by `pid`, within the addresses the device can reach (`constraint`):
/// mapped in `pid` and in the device's domain. Returns (the address `pid`
/// sees, the address the device must be given).
pub fn dma_alloc(
    raw: u64,
    pid: ProcessId,
    size: u64,
    constraint: crate::mm::dma::DmaConstraint,
) -> KernelResult<(u64, u64)> {
    use crate::mm::dma;

    /// Why the arena did not reach the device.
    enum NotGiven {
        /// Freed already, by another thread of the owner: nothing to undo.
        Freed,
        /// Anything else: the arena is still tracked, and is freed here.
        Error(KernelError),
    }

    let pml4 = crate::proc::pcb::get_pml4(pid)
        .filter(|&p| p != 0)
        .ok_or(KernelError::NoSuchProcess)?;
    with_owned(raw, pid, |_| Ok(()))?;
    let size = usize::try_from(size).map_err(|_| KernelError::InvalidArgument)?;
    let (user_virt, phys, size) = dma::alloc_for_user(pml4, size, constraint)?;
    // Given to the device under the device table's lock: a close cannot
    // release the device -- and its domain id be reused by another device --
    // between finding the domain and mapping into it, and a free racing this
    // call revokes through `revoke_arena`, which waits for the lock and then
    // finds the arena recorded.
    let given = {
        let mut devices = DEVICES.lock();
        match devices.iter_mut().find(|d| d.raw == raw && d.owner == pid) {
            None => Err(NotGiven::Error(KernelError::InvalidHandle)),
            Some(device) => {
                if !dma::mark_device(pml4, user_virt, raw) {
                    Err(NotGiven::Freed)
                } else if let Some(domain) = device.domain
                    && let Err(e) = iommu_remap::map_dma_as(
                        Caller::Kernel,
                        domain,
                        phys,
                        phys,
                        size,
                        DmaPerms::READ_WRITE,
                    )
                {
                    // A failure part-way leaves the pages before it mapped:
                    // taken out of the domain before the arena is freed. The
                    // pages that never were mapped make this report an error,
                    // which is expected and says nothing more.
                    let _ = iommu_remap::unmap_dma_as(Caller::Kernel, domain, phys, size);
                    Err(NotGiven::Error(e))
                } else {
                    device.arenas.push(Arena { user_virt, phys });
                    Ok(())
                }
            }
        }
    };
    match given {
        Ok(()) => Ok((user_virt, phys)),
        Err(NotGiven::Freed) => Err(KernelError::InvalidArgument),
        Err(NotGiven::Error(e)) => {
            // Not given to the device (or taken back out of its domain), so
            // freeing it needs nothing revoked; the free's own failure would
            // only mean it was freed meanwhile.
            let _ = dma::free_for_user(pml4, user_virt);
            Err(e)
        }
    }
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

/// Set up the device's MSI, owned by `pid`: one vector, its messages sent to
/// the CPU this runs on (online by definition, read from its own local APIC;
/// a waiter on another CPU is woken from there). `NotSupported` before the
/// local APIC is up and for a device without an MSI capability,
/// `AlreadyExists` once it is set up (or being set up), `ResourceExhausted`
/// with no vector free.
pub fn irq_setup(raw: u64, pid: ProcessId) -> KernelResult<()> {
    // No local APIC yet (early boot): no message could be delivered.
    if !crate::apic::is_ready() {
        return Err(KernelError::NotSupported);
    }
    // Reserved under the table's lock, made outside it: the capability walk
    // and the programming are port accesses and a log line, too long to hold
    // a spinlock across.
    let addr = {
        let mut devices = DEVICES.lock();
        let device = devices
            .iter_mut()
            .find(|d| d.raw == raw)
            .ok_or(KernelError::InvalidHandle)?;
        if device.owner != pid {
            return Err(KernelError::PermissionDenied);
        }
        if device.msi.is_some() || device.msi_busy {
            return Err(KernelError::AlreadyExists);
        }
        device.msi_busy = true;
        device.addr
    };
    let made = (|| {
        let (b, d, f) = (addr.bus, addr.device, addr.function);
        let mut config =
            crate::msi::find_msi_capability(b, d, f).ok_or(KernelError::NotSupported)?;
        crate::msi::enable_msi(&mut config, crate::apic::read_id())
            .ok_or(KernelError::ResourceExhausted)?;
        if !crate::msi::claim_delivery(config.allocated_vector, raw) {
            crate::msi::disable_msi(&mut config);
            return Err(KernelError::InternalError);
        }
        Ok(config)
    })();
    let unwanted = {
        let mut devices = DEVICES.lock();
        match devices.iter_mut().find(|d| d.raw == raw) {
            Some(device) => {
                device.msi_busy = false;
                device.msi = Some(made?);
                return Ok(());
            }
            // Closed while it was being made: undone below, without the lock.
            None => made.ok(),
        }
    };
    if let Some(mut config) = unwanted {
        crate::msi::release_delivery(config.allocated_vector, raw);
        crate::msi::disable_msi(&mut config);
    }
    Err(KernelError::InvalidHandle)
}

/// The device's interrupts since the last take, owned by `pid`: taken at
/// once when `block` is false (0 when none came), else waited for until at
/// least one has. Several tasks may wait, here or in `SYS_WAIT_MULTIPLE`
/// ([`watch_irq`]): every interrupt wakes them all, and whoever takes the
/// count has it -- the others look again and wait on.
/// `InvalidArgument` before [`irq_setup`]; `InvalidHandle` when the device
/// is closed, waiting or not; `Interrupted` when a signal or a freeze comes
/// for `pid` while it waits.
pub fn irq_wait(raw: u64, pid: ProcessId, block: bool) -> KernelResult<u64> {
    let vector = with_owned(raw, pid, |d| {
        d.msi
            .as_ref()
            .map(|c| c.allocated_vector)
            .ok_or(KernelError::InvalidArgument)
    })?;
    if !block {
        return crate::msi::take(vector, raw).ok_or(KernelError::InvalidHandle);
    }
    let task = crate::sched::current_task_id();
    crate::msi::register_waiter(vector, raw, task)?;
    // A signal for the process wakes the tasks registered here, as it does
    // a sleeping nanosleep.
    crate::proc::signal::register_signalfd_waiter(pid, task, 0);
    let taken = loop {
        match crate::msi::take(vector, raw) {
            None => break Err(KernelError::InvalidHandle),
            Some(count) if count > 0 => break Ok(count),
            Some(_) => {}
        }
        if crate::ipc::waiters::deliverable_signal_pending(pid)
            || crate::proc::freezer::interrupt_pending(pid)
        {
            break Err(KernelError::Interrupted);
        }
        // A wake landing before the park leaves a token the park consumes,
        // so an interrupt between the take above and here is not lost.
        crate::sched::block_current_on(crate::wchan::Wait::new(
            crate::wchan::WaitChannel::Io,
            u64::from(vector),
        ));
    };
    crate::proc::signal::deregister_signalfd_waiter(pid, task);
    crate::msi::unregister_waiter(vector, task);
    taken
}

/// The MSI vector of the device `raw` names, if it is open and its MSI is
/// set up.
fn vector_of(raw: u64) -> KernelResult<u8> {
    DEVICES
        .lock()
        .iter()
        .find(|d| d.raw == raw)
        .ok_or(KernelError::InvalidHandle)?
        .msi
        .as_ref()
        .map(|c| c.allocated_vector)
        .ok_or(KernelError::InvalidArgument)
}

/// For `SYS_WAIT_MULTIPLE`: how many of the device's interrupts are counted
/// and not yet taken, left where they are. `InvalidArgument` when its MSI is
/// not set up -- it cannot become ready -- and `InvalidHandle` when the
/// device is closed. The caller has checked that its process holds `raw`.
pub fn irq_readiness(raw: u64) -> KernelResult<u64> {
    let vector = vector_of(raw)?;
    crate::msi::pending(vector, raw).ok_or(KernelError::InvalidHandle)
}

/// For `SYS_WAIT_MULTIPLE`: `task` is woken by the device's next interrupt,
/// and by its close. Nothing when there is nothing to wait for -- the device
/// closed, or no MSI set up -- which the readiness test reports instead.
pub fn watch_irq(raw: u64, task: crate::sched::task::TaskId) {
    if let Ok(vector) = vector_of(raw) {
        // Refused only when the device was closed in between, which wakes
        // its waiters anyway: there is nothing left to wait for.
        let _closed = crate::msi::register_waiter(vector, raw, task);
    }
}

/// Undo [`watch_irq`]. A device closed since has let its waiters go already.
pub fn unwatch_irq(raw: u64, task: crate::sched::task::TaskId) {
    if let Ok(vector) = vector_of(raw) {
        crate::msi::unregister_waiter(vector, task);
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
    let offset = config_offset(offset, width)?;
    let (addr, refused) = with_owned(raw, pid, |d| {
        Ok((d.addr, d.guards.refuse(offset, width, value)))
    })?;
    if refused {
        return Err(KernelError::PermissionDenied);
    }
    let (b, d, f) = (addr.bus, addr.device, addr.function);
    #[allow(clippy::cast_possible_truncation)]
    match width {
        1 => pci::config_write8(b, d, f, offset, value as u8),
        2 => pci::config_write16_native(b, d, f, offset, value as u16),
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
    use crate::mm::vma::VmaKind;
    use crate::proc::pcb::{self, ForkAdvice};

    let pml4 = pcb::get_pml4(pid)
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
    // A region of its own in the owner's address space, in whole frames, as
    // every region is: reused once the device is released, and not given to
    // a forked child, which holds no handle.
    let frame = crate::mm::frame::FRAME_SIZE as u64;
    let span = bar.size.div_ceil(frame).saturating_mul(frame);
    let flags = PageFlags::PRESENT
        | PageFlags::WRITABLE
        | PageFlags::USER_ACCESSIBLE
        | PageFlags::NO_CACHE
        | PageFlags::NO_EXECUTE
        | PageFlags::FOREIGN;
    let user_virt =
        crate::syscall::handlers::alloc_user_mmap_reserve(pid, span, VmaKind::Fixed, flags);
    if user_virt == 0 {
        return Err(KernelError::OutOfMemory);
    }
    let mapped = MappedBar {
        user_virt,
        pages,
        phys: bar.base,
        span,
    };
    let placed = pcb::set_fork_policy(
        pid,
        user_virt,
        user_virt.saturating_add(span),
        ForkAdvice::DontCopy,
    )
    .map(|_| ())
    .and_then(|()| {
        for page in 0..pages {
            let offset = page.saturating_mul(HW_PAGE);
            // SAFETY: `pml4` is the owner's own table; the region was
            // just reserved for this mapping; the physical page is the
            // device's own BAR, which the allocator does not own.
            let done = unsafe {
                page_table::map_4k_if_absent(
                    pml4,
                    VirtAddr::new(user_virt.saturating_add(offset)),
                    bar.base.saturating_add(offset),
                    flags,
                )
            };
            // A page already there is not the BAR: the region is not
            // what it was reserved as, and nothing is left half-mapped.
            if !matches!(done, Ok(true)) {
                return Err(done.err().unwrap_or(KernelError::AlreadyExists));
            }
        }
        with_owned(raw, pid, |d| {
            d.mapped.push(mapped);
            Ok(())
        })
    });
    if let Err(e) = placed {
        unmap_bar(pml4, pid, &mapped);
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

/// The configuration registers a driver may not write, read once when its
/// device is opened (the capability list is the hardware's and does not
/// change): the BARs and the ROM by position, the MSI and MSI-X
/// capabilities, the power state, and a Function Level Reset -- the last two
/// reset the function behind the kernel's back, after which its BARs read 0
/// while the kernel still has them recorded as sized at open.
#[derive(Debug, Clone, Default)]
struct Guards {
    /// Byte ranges refused outright: (start, length).
    ranges: Vec<(u16, u16)>,
    /// Bits that reset the function when written as 1, as (byte, mask): the
    /// PCIe Device Control register's Initiate Function Level Reset, and the
    /// Advanced Features capability's. A write setting one is refused; any
    /// other write to those registers (payload sizes, relaxed ordering, ...)
    /// is the driver's.
    resets: Vec<(u16, u8)>,
}

impl Guards {
    fn of(addr: PciAddress) -> Self {
        let mut ranges = alloc::vec![(u16::from(CFG_BAR0), 24), (u16::from(CFG_ROM), 4)];
        let mut resets = Vec::new();
        for cap in pci::walk_capabilities(addr) {
            let at = u16::from(cap.offset);
            match cap.id {
                CAP_MSI => ranges.push((at, u16::from(CAP_MSI_LEN))),
                CAP_MSIX => ranges.push((at, u16::from(CAP_MSIX_LEN))),
                // PMCSR's low byte: the power state.
                CAP_PM => ranges.push((at.saturating_add(4), 1)),
                // Device Control's high byte, bit 7 (bit 15 of the word).
                CAP_PCIE => resets.push((at.saturating_add(9), 0x80)),
                // AF Control, bit 0.
                CAP_AF => resets.push((at.saturating_add(4), 0x01)),
                _ => {}
            }
        }
        Self { ranges, resets }
    }

    /// Whether writing `value`, `width` bytes at `offset`, is refused.
    fn refuse(&self, offset: u8, width: u8, value: u32) -> bool {
        let start = u16::from(offset);
        let end = start.saturating_add(u16::from(width));
        let overlaps = |from: u16, len: u16| start < from.saturating_add(len) && from < end;
        if self.ranges.iter().any(|&(from, len)| overlaps(from, len)) {
            return true;
        }
        self.resets.iter().any(|&(byte, mask)| {
            overlaps(byte, 1)
                && (value >> (u32::from(byte.saturating_sub(start)).saturating_mul(8)))
                    & u32::from(mask)
                    != 0
        })
    }
}

/// Size each memory BAR of `addr` by the standard probe: memory decoding off,
/// all-ones written, the read-back's low set bit is the size, then the
/// original value and the command register restored.
fn size_memory_bars(addr: PciAddress) -> [Option<MemoryBar>; 6] {
    let (b, d, f) = (addr.bus, addr.device, addr.function);
    let mut bars = [None; 6];
    let command = pci::config_read16(b, d, f, CFG_COMMAND);
    pci::config_write16_native(b, d, f, CFG_COMMAND, command & !CMD_MEMORY_SPACE);
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
    pci::config_write16_native(b, d, f, CFG_COMMAND, command);
    bars
}

/// Take a BAR mapping out of its owner: each entry that still maps the
/// BAR's own page is cleared (one that no longer does is someone else's
/// now, and is left alone), the range flushed, and the region removed so
/// the address space can reuse it.
fn unmap_bar(pml4: u64, owner: ProcessId, bar: &MappedBar) {
    use crate::mm::page_table::{self, VirtAddr};
    for page in 0..bar.pages {
        let offset = page.saturating_mul(HW_PAGE);
        let va = VirtAddr::new(bar.user_virt.saturating_add(offset));
        if page_table::translate(pml4, va) != Some(bar.phys.saturating_add(offset)) {
            continue;
        }
        // SAFETY: the entry maps the BAR's own page, mapped by `map_bar` into
        // this table; the frame behind it is device memory, which clearing
        // the entry does not free. Its result is the address just checked.
        let _ = unsafe { page_table::unmap_4k(pml4, va) };
    }
    let end = bar.user_virt.saturating_add(bar.span);
    crate::syscall::handlers::mmap_flush_range(bar.user_virt, end);
    // Only while it is still the region reserved for this BAR: one the owner
    // has since unmapped, and perhaps filled with something of its own, is
    // left alone.
    crate::proc::pcb::remove_foreign_region(owner, bar.user_virt, end);
}

/// The blocking-wait tests' worker: waits on the device its statics name,
/// as a driver's interrupt thread would -- in [`irq_wait`], or with
/// `SYS_WAIT_MULTIPLE`'s machinery ([`WAITER_MULTI`]) -- and leaves the
/// outcome behind.
static WAITER_RAW: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
static WAITER_OWNER: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
static WAITER_RESULT: core::sync::atomic::AtomicI64 = core::sync::atomic::AtomicI64::new(WAITING);
/// Wait as `SYS_WAIT_MULTIPLE` does on a device handle, not in `irq_wait`.
static WAITER_MULTI: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);
/// How long the worker's wait lasted, in nanoseconds.
static WAITER_WAITED: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
/// [`WAITER_RESULT`] while the worker has not answered.
const WAITING: i64 = i64::MIN;
/// How long a multiwait worker waits at most: five times the second its
/// wake must come within, so a wake that never comes is not mistaken for a
/// slow one.
const MULTIWAIT_TIMEOUT_NS: u64 = 5_000_000_000;

extern "C" fn irq_waiter(_arg: u64) {
    use core::sync::atomic::Ordering;
    let raw = WAITER_RAW.load(Ordering::SeqCst);
    let owner = WAITER_OWNER.load(Ordering::SeqCst);
    let started = crate::hrtimer::now_ns();
    let outcome = if WAITER_MULTI.load(Ordering::SeqCst) {
        crate::ipc::multiwait::wait_multiple(
            &[crate::ipc::multiwait::WaitTarget::Device(raw)],
            Some(MULTIWAIT_TIMEOUT_NS),
            || usize::from(irq_readiness(raw).is_ok_and(|count| count > 0)),
        )
        .map(|ready| u64::try_from(ready).unwrap_or(u64::MAX))
    } else {
        irq_wait(raw, owner, true)
    };
    let answer = match outcome {
        Ok(count) => i64::try_from(count).unwrap_or(i64::MAX),
        Err(e) => i64::from(e.code()),
    };
    WAITER_WAITED.store(
        crate::hrtimer::now_ns().saturating_sub(started),
        Ordering::SeqCst,
    );
    WAITER_RESULT.store(answer, Ordering::SeqCst);
}

/// Start the worker on `raw` -- waiting in `SYS_WAIT_MULTIPLE`'s way when
/// `multi` -- and return once it is registered for `vector` and blocked
/// (`false` if it never got there). Blocked, not only registered: an
/// interrupt landing between the two would be found by its scan with no
/// wake, and the wake is what is under test.
fn start_waiter(raw: u64, owner: ProcessId, vector: u8, multi: bool) -> bool {
    use core::sync::atomic::Ordering;
    WAITER_RAW.store(raw, Ordering::SeqCst);
    WAITER_OWNER.store(owner, Ordering::SeqCst);
    WAITER_MULTI.store(multi, Ordering::SeqCst);
    WAITER_RESULT.store(WAITING, Ordering::SeqCst);
    let Ok(task) = crate::sched::spawn(b"devhandle-irq", 16, irq_waiter, 0, 0) else {
        return false;
    };
    yield_until(|| {
        crate::msi::waiter_count(vector) > 0
            && crate::sched::task_state(task) == Some(crate::sched::task::TaskState::Blocked)
    })
}

/// The worker's answer, once it has one.
fn waiter_answer() -> Option<i64> {
    use core::sync::atomic::Ordering;
    let mut answer = None;
    let _done = yield_until(|| {
        let value = WAITER_RESULT.load(Ordering::SeqCst);
        answer = (value != WAITING).then_some(value);
        answer.is_some()
    });
    answer
}

/// Yield until `ready`, for at most a second (or a bounded number of
/// yields, should the clock not run).
fn yield_until(mut ready: impl FnMut() -> bool) -> bool {
    let deadline = crate::hpet::elapsed_ns().saturating_add(1_000_000_000);
    for _ in 0..200_000 {
        if ready() {
            return true;
        }
        if crate::hpet::elapsed_ns() > deadline {
            break;
        }
        crate::sched::yield_now();
    }
    ready()
}

/// QEMU's `edu` device, which the boot test attaches for this test: a 1 MiB
/// memory BAR and an MSI capability, and no kernel driver.
const EDU_VENDOR: u16 = 0x1234;
const EDU_DEVICE: u16 = 0x11E8;
/// edu's interrupt registers: writing a bit raises it (an MSI when MSI is
/// on), writing it to the other acknowledges it.
const EDU_IRQ_RAISE: u64 = 0x60;
const EDU_IRQ_ACK: u64 = 0x64;

/// Open, configure, map and close QEMU's `edu` device as a scratch process,
/// and check every refusal on the way: a second open, another process's
/// access, writes to the BARs and the MSI capability. Skipped, and recorded
/// as skipped, where there is no `edu` (anywhere but the boot test).
pub fn self_test() -> KernelResult<()> {
    use crate::mm::page_table::{self, VirtAddr};
    use crate::proc::pcb;
    use crate::serial_println;

    // MSI delivery itself, before anything uses it for a device.
    crate::msi::self_test_delivery()?;

    // The write guards, on a made-up layout: BARs at 0x10, the PCIe Device
    // Control register at 0x48 (its high byte, holding FLR, at 0x49), and an
    // Advanced Features control byte at 0x64 (FLR in bit 0).
    let guards = Guards {
        ranges: alloc::vec![(0x10, 24)],
        resets: alloc::vec![(0x49, 0x80), (0x64, 0x01)],
    };
    for (offset, width, value, refused) in [
        (0x48, 2, 0x8000, true),
        (0x48, 2, 0x0010, false),
        (0x48, 4, 0x0000_8000, true),
        (0x49, 1, 0x80, true),
        (0x49, 1, 0x10, false),
        (0x10, 4, 0, true),
        (0x24, 4, 0, true),
        (0x0C, 4, 0, false),
        (0x28, 4, 0, false),
        (0x64, 1, 0x01, true),
        (0x64, 1, 0x02, false),
        (0x64, 4, 0x0000_0001, true),
    ] {
        if guards.refuse(offset, width, value) != refused {
            serial_println!(
                "[devhandle]   FAIL: a {}-byte write of {:#x} at {:#x} was {} refused",
                width,
                value,
                offset,
                if refused { "not" } else { "wrongly" }
            );
            return Err(KernelError::InternalError);
        }
    }

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
    serial_println!(
        "[devhandle]   open, configure, map, MSI (taken; waited for in irq_wait and as SYS_WAIT_MULTIPLE waits, woken by the interrupt), DMA arenas and close edu: OK"
    );
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
            // An MSI: edu sends one when its interrupt-raise register is
            // written, and it is taken once on the vector set up for it.
            irq_setup(raw, owner).map_err(|_| "MSI could not be set up")?;
            if irq_setup(raw, owner) != Err(KernelError::AlreadyExists) {
                return Err("a second MSI set-up was not refused");
            }
            let vector = with_owned(raw, owner, |d| {
                d.msi
                    .as_ref()
                    .map(|c| c.allocated_vector)
                    .ok_or(KernelError::InvalidArgument)
            })
            .map_err(|_| "no vector recorded")?;
            // SAFETY: BAR0 is edu's register window, which nothing else in
            // the kernel maps or drives.
            let regs = unsafe { page_table::map_mmio(bar0.base, HW_PAGE) }
                .map_err(|_| "BAR0 could not be mapped for the kernel")?;
            let reg = |offset: u64| regs.saturating_add(offset) as *mut u32;
            // SAFETY: two of edu's 32-bit registers, inside the page just
            // mapped: acknowledge everything, then raise interrupt bit 0.
            unsafe {
                reg(EDU_IRQ_ACK).write_volatile(u32::MAX);
                reg(EDU_IRQ_RAISE).write_volatile(1);
            }
            let deadline = crate::hpet::elapsed_ns().saturating_add(100_000_000);
            let mut taken = 0;
            while taken == 0 && crate::hpet::elapsed_ns() < deadline {
                taken = irq_wait(raw, owner, false).unwrap_or(0);
                core::hint::spin_loop();
            }
            // SAFETY: as above; the raised bit is acknowledged.
            unsafe { reg(EDU_IRQ_ACK).write_volatile(1) };
            if taken != 1 {
                return Err("edu's MSI was not taken exactly once");
            }
            if irq_wait(raw, owner, false) != Ok(0) {
                return Err("one MSI was taken twice");
            }
            // Parked in a blocking wait, a waiter is woken by the device's
            // interrupt and takes it.
            if !start_waiter(raw, owner, vector, false) {
                return Err("the blocking waiter never parked");
            }
            // SAFETY: as above.
            unsafe { reg(EDU_IRQ_RAISE).write_volatile(1) };
            let woken = waiter_answer();
            // SAFETY: as above.
            unsafe { reg(EDU_IRQ_ACK).write_volatile(1) };
            if woken != Some(1) {
                return Err("a parked waiter was not woken by the device's interrupt");
            }
            // Parked as SYS_WAIT_MULTIPLE parks on a device handle, a waiter
            // is woken by the device's interrupt -- within a second, not
            // found when its five-second timeout comes -- and leaves the
            // interrupt for the take.
            if !start_waiter(raw, owner, vector, true) {
                return Err("the multiwait waiter never parked");
            }
            // SAFETY: as above.
            unsafe { reg(EDU_IRQ_RAISE).write_volatile(1) };
            let ready = waiter_answer();
            let waited = WAITER_WAITED.load(core::sync::atomic::Ordering::SeqCst);
            // SAFETY: as above.
            unsafe { reg(EDU_IRQ_ACK).write_volatile(1) };
            if ready != Some(1) || waited >= 1_000_000_000 {
                return Err(
                    "a waiter in SYS_WAIT_MULTIPLE was not woken by the device's interrupt",
                );
            }
            if irq_wait(raw, owner, false) != Ok(1) {
                return Err("the interrupt a multiwait waiter saw was not left for the take");
            }
            // A DMA arena: where the owner sees it is its device address,
            // and the device's domain reaches it until it is freed.
            let (arena, device_addr) =
                dma_alloc(raw, owner, 0x4000, crate::mm::dma::DmaConstraint::None)
                    .map_err(|_| "a DMA arena could not be made")?;
            if page_table::translate(pml4, VirtAddr::new(arena)) != Some(device_addr) {
                return Err("the arena is not at its device address");
            }
            let Some(domain) = domain_of(raw, owner).map_err(|_| "no domain answer")? else {
                return Ok((va, None, vector));
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
            let (kept, kept_addr) =
                dma_alloc(raw, owner, 0x4000, crate::mm::dma::DmaConstraint::Below4G)
                    .map_err(|_| "a second arena could not be made")?;
            // A fork's child gets neither the BAR nor the arena
            // (`PageFlags::FOREIGN`): it holds no handle and no tracker
            // entry. Cloned with no range excluded, so the flag alone is
            // what keeps them out.
            // SAFETY: the owner is a scratch process no thread runs, so its
            // tables are quiescent for the clone.
            let child = unsafe { crate::mm::cow::clone_address_space_cow(pml4, &[]) }
                .map_err(|_| "the owner's address space could not be cloned")?;
            let given = page_table::translate(child, VirtAddr::new(va)).is_some()
                || page_table::translate(child, VirtAddr::new(kept)).is_some();
            // SAFETY: the child table was just made, and no CPU runs on it.
            unsafe { page_table::destroy_user_address_space(child) };
            if given {
                return Err("a forked child got the device's registers or its arena");
            }
            Ok((va, Some((domain, kept_addr)), vector))
        })();
        let (va, kept, vector) = match checks {
            Ok(found) => found,
            Err(msg) => {
                close(raw);
                return Err(msg);
            }
        };
        // A waiter parked when the device closes is woken, to find it gone.
        let parked = start_waiter(raw, owner, vector, false);
        close(raw);
        let woken = waiter_answer();
        if !parked {
            return Err("the second blocking waiter never parked");
        }
        if woken != Some(i64::from(KernelError::InvalidHandle.code())) {
            return Err("a waiter parked at close was not woken with InvalidHandle");
        }
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
        if crate::msi::is_allocated(vector) {
            return Err("the device's MSI vector is still allocated after the last close");
        }
        Ok(kept.is_some())
    }
    Ok(())
}

/// The probe driver `services/devprobe`, embedded: the device interface's
/// first ring-3 user. MIT, like the kernel.
static DEVPROBE_ELF: &[u8] =
    include_bytes!("../../services/devprobe/target/x86_64-unknown-none/release/devprobe");

// The probe answers a refused open with `devprobe::PERMISSION_DENIED`, and
// tells the capability check from every other refusal by it.
#[allow(clippy::cast_lossless)] // `i64::from` is not const.
const _: () = assert!(devprobe::PERMISSION_DENIED == KernelError::PermissionDenied.code() as i64);
// The kind the probe names its device by in `SYS_WAIT_MULTIPLE`.
const _: () = assert!(devprobe::KIND_PCI_DEVICE == crate::cap::ResourceType::PciDevice as u32);

/// The boot test's card for the probe: an unbound 82574 on bus 0 -- the only
/// bus the probe scans -- whose receive address 0 holds
/// `devprobe::EXPECTED_MAC`, the address `scripts/boot-test.sh` gives it. A
/// real 82574 has another address, so one on the same machine, spare or one
/// the e1000 driver failed to take, is never handed to the probe.
fn probe_card() -> Option<pci::PciDevice> {
    pci::find_all_devices(devprobe::INTEL, devprobe::E1000E_82574)
        .into_iter()
        .filter(|d| d.address.bus == 0 && pci::bound_driver(d.address).is_none())
        .find(|d| card_mac(d) == Some(devprobe::EXPECTED_MAC))
}

/// The MAC address in `dev`'s receive address 0, read through its BAR0, if
/// that is a memory BAR and the address is marked valid. A card not decoding
/// its BAR reads all ones, which is no valid address.
fn card_mac(dev: &pci::PciDevice) -> Option<[u8; 6]> {
    let phys = pci::bar_mmio_addr64(dev, 0)?;
    // SAFETY: BAR0 of an 82574 is its register block, device memory of at
    // least `MIN_BAR0` bytes, mapped uncached as the e1000 driver maps it.
    let base = unsafe { crate::mm::page_table::map_mmio(phys, devprobe::MIN_BAR0).ok()? };
    let read = |offset: usize| {
        let at = base.checked_add(u64::try_from(offset).ok()?)?;
        // SAFETY: inside the mapping just made: both registers read here
        // are 4-aligned and well inside the first `MIN_BAR0` bytes.
        Some(unsafe { (at as *const u32).read_volatile() })
    };
    devprobe::mac_from_receive_address(read(devprobe::RAL0)?, read(devprobe::RAH0)?)
}

/// The device interface from ring 3: the probe driver (`services/devprobe`)
/// is spawned holding a `PciDevice` capability for exactly one function --
/// the boot test's card for it ([`probe_card`]) -- and must open only that
/// one, with every other refused by the capability check; read its ids;
/// turn on decoding and bus mastering; map BAR0 and read the MAC through it;
/// make a DMA arena; set up its MSI; find the device not ready in
/// `SYS_WAIT_MULTIPLE`, have the card raise an interrupt, wait for it there
/// and take it; and exit with `devprobe::Exit::Ok`. Its
/// exit, which closes nothing itself, must release the device: unbound, bus
/// mastering off, MSI off and its vector back in the pool. Skipped, and
/// recorded so, where there is no such card.
pub fn self_test_ring3() -> KernelResult<()> {
    use crate::cap::{ResourceType, Rights};
    use crate::proc::{pcb, spawn};
    use crate::serial_println;

    let mut skips = crate::fs::selftest::Skips::new();
    let Some(card) = probe_card() else {
        skips.record(
            "ring-3 probe driver",
            "no unbound 82574 on bus 0 with the boot test's MAC address",
        );
        skips.report("[devhandle]");
        return Ok(());
    };
    let a = card.address;
    let caps = [(
        ResourceType::PciDevice,
        crate::cap::pci_device_id(a.bus, a.device, a.function),
        Rights::WRITE,
    )];
    let options = spawn::SpawnOptions {
        name: "devprobe",
        parent: 0,
        priority: crate::sched::task::DEFAULT_PRIORITY,
        capabilities: &caps,
        fd_map: &[],
        argv: &[],
        envp: &[],
        exe_path: None,
        cwd: None,
        uid_gid: None,
    };
    let vectors_before = crate::msi::stats().vectors_used;
    let probe = match spawn::spawn_process(DEVPROBE_ELF, &options) {
        Ok(p) => p,
        Err(e) => {
            serial_println!(
                "[devhandle]   FAIL: the probe driver could not be spawned: {:?}",
                e
            );
            return Err(KernelError::InternalError);
        }
    };
    let deadline = crate::hrtimer::now_ns().saturating_add(10_000_000_000);
    let finished = spawn::wait_for_fixture(probe.pid, deadline, 0);
    let exit_code = pcb::exit_code(probe.pid);
    // Released by its exit: the held handles close as its thread ends, which
    // may be just after it is seen to have ended.
    let released = yield_until(|| pci::bound_driver(a).is_none());
    spawn::teardown_fixture(probe.pid, probe.task_id);
    if !finished {
        serial_println!("[devhandle]   FAIL: the probe driver did not exit within 10 s");
        return Err(KernelError::TimedOut);
    }
    let exit = exit_code.and_then(|c| devprobe::Exit::from_code(i64::from(c)));
    if exit != Some(devprobe::Exit::Ok) {
        // A code that is no step's is a death: 128 and up, a fault's signal.
        serial_println!(
            "[devhandle]   FAIL: the probe driver exited with {:?}: {:?} (devprobe::Exit; None is no step's code, a fault)",
            exit_code,
            exit
        );
        return Err(KernelError::InternalError);
    }
    if !released {
        serial_println!(
            "[devhandle]   FAIL: the device is still bound after the probe driver's exit"
        );
        return Err(KernelError::InternalError);
    }
    // What the release must undo of what the probe turned on: bus mastering
    // (step 3), and MSI with its vector (step 5).
    let command = pci::config_read16(a.bus, a.device, a.function, CFG_COMMAND);
    if command & (1 << 2) != 0 {
        serial_println!(
            "[devhandle]   FAIL: bus mastering is still on after the probe driver's exit"
        );
        return Err(KernelError::InternalError);
    }
    let msi_on = crate::msi::find_msi_capability(a.bus, a.device, a.function).is_some_and(|cap| {
        pci::config_read16(a.bus, a.device, a.function, cap.cap_offset.wrapping_add(2)) & 1 != 0
    });
    if msi_on {
        serial_println!("[devhandle]   FAIL: MSI is still on after the probe driver's exit");
        return Err(KernelError::InternalError);
    }
    if crate::msi::stats().vectors_used != vectors_before {
        serial_println!(
            "[devhandle]   FAIL: the probe driver's MSI vector was not given back at its exit"
        );
        return Err(KernelError::InternalError);
    }
    serial_println!(
        "[devhandle]   ring-3 probe driver: OK (only its granted function opened, every other refused by the capability check; ids, MAC, DMA arena, MSI; an interrupt waited for in SYS_WAIT_MULTIPLE and taken; released at exit, bus mastering and MSI off)"
    );
    Ok(())
}
