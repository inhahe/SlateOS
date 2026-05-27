//! Syscall handler implementations.
//!
//! Each handler is a function that takes [`SyscallArgs`] and returns a
//! [`SyscallResult`].  Handlers are referenced from the dispatch table
//! in [`super::dispatch`].
//!
//! ## Conventions
//!
//! - Handlers are named `sys_<operation>`.
//! - On success, `SyscallResult::value` is the return value (>= 0).
//! - On error, `SyscallResult::value` is the negative error code
//!   from [`KernelError`].
//! - Pointer arguments from userspace are validated via
//!   [`crate::mm::user`] before dereferencing.  Every buffer pointer
//!   is checked for user-space range and page table mapping.

// Syscall args are u64 (register-width).  On our x86_64 target,
// usize is 64 bits, so u64→usize casts cannot truncate.
#![allow(clippy::cast_possible_truncation)]

use crate::cap::ResourceType;
use crate::error::KernelError;
use crate::ipc::channel::{self, ChannelHandle, Message};
use crate::ipc::completion::{self, CpHandle, WaitSource};
use crate::ipc::eventfd::{self, EventFdHandle};
use crate::ipc::futex;
use crate::ipc::pipe::{self, PipeHandle};
use crate::ipc::service::{self, ServiceListenerHandle};
use crate::ipc::shm::{self, ShmHandle};
use crate::proc::pcb;
use crate::sched;
use crate::serial_println;

use super::dispatch::{SyscallArgs, SyscallResult};

// ---------------------------------------------------------------------------
// Channel creation flags
// ---------------------------------------------------------------------------

/// Create a synchronous (rendezvous) channel — no internal buffer.
///
/// Sends block until a receiver takes the message.  Used for
/// low-latency, L4-style IPC where the kernel copies directly
/// from sender to receiver.
pub const CHANNEL_FLAG_SYNC: u64 = 1;

// ---------------------------------------------------------------------------
// Capability enforcement helpers
// ---------------------------------------------------------------------------

/// Get the calling process's PID.
///
/// Returns `None` for bare kernel tasks (no owning process).
/// Used by [`require_cap`] and [`require_cap_type`] to identify
/// the caller.
fn caller_pid() -> Option<u64> {
    let task_id = sched::current_task_id();
    crate::proc::thread::owner_process(task_id)
}

/// Get the calling process's PML4 physical address.
fn caller_pml4() -> Option<u64> {
    let pid = caller_pid()?;
    let pml4 = crate::proc::pcb::get_pml4(pid)?;
    if pml4 != 0 { Some(pml4) } else { None }
}

/// Check that the calling process holds a capability for a specific
/// resource (type + ID + rights).
///
/// Kernel tasks (no owning process, or PID 0) bypass all checks —
/// the kernel has implicit authority over all resources.
///
/// # Errors
///
/// - `PermissionDenied` — the process lacks the required capability.
#[allow(dead_code)] // Will be used for per-resource checks (per-IRQ, per-port).
fn require_cap(
    resource_type: crate::cap::ResourceType,
    resource_id: u64,
    required_rights: crate::cap::Rights,
) -> Result<(), KernelError> {
    let pid = match caller_pid() {
        Some(pid) => pid,
        None => return Ok(()), // Bare kernel task — bypass.
    };
    if pid == 0 {
        return Ok(()); // Kernel process — implicit authority.
    }
    if crate::proc::pcb::has_capability_for(
        pid,
        resource_type,
        resource_id,
        required_rights,
    ) {
        Ok(())
    } else {
        Err(KernelError::PermissionDenied)
    }
}

/// Check that the calling process holds *any* capability of the given
/// type with the required rights (ignoring resource ID).
///
/// Used for resource types where the specific ID is not relevant to
/// the check — e.g., "does this process have filesystem access?"
/// or "can this process open network sockets?"
///
/// Kernel tasks (no owning process, or PID 0) bypass all checks.
///
/// # Errors
///
/// - `PermissionDenied` — the process lacks the required capability.
fn require_cap_type(
    resource_type: crate::cap::ResourceType,
    required_rights: crate::cap::Rights,
) -> Result<(), KernelError> {
    let pid = match caller_pid() {
        Some(pid) => pid,
        None => return Ok(()), // Bare kernel task — bypass.
    };
    if pid == 0 {
        return Ok(()); // Kernel process — implicit authority.
    }
    if crate::proc::pcb::has_capability_type(
        pid,
        resource_type,
        required_rights,
    ) {
        Ok(())
    } else {
        Err(KernelError::PermissionDenied)
    }
}

// ---------------------------------------------------------------------------
// Kernel-core handlers (0–199)
// ---------------------------------------------------------------------------

/// `SYS_YIELD` — yield the current task's time slice.
pub fn sys_yield(args: &SyscallArgs) -> SyscallResult {
    let _ = args;
    sched::yield_now();
    SyscallResult::ok(0)
}

/// `SYS_EXIT` — exit the current task.
///
/// Notifies the thread/process system before terminating.  If this
/// was the last thread in a process, the process becomes a zombie.
pub fn sys_exit(args: &SyscallArgs) -> SyscallResult {
    let exit_code = args.arg0 as i32;
    let task_id = sched::current_task_id();

    // Store exit code in the PCB before on_thread_exit transitions
    // the process to Zombie.  remove_thread() has a guard that only
    // sets exit_code=0 when None, so our explicit code wins.
    // Bare kernel tasks (no owning process) simply skip this.
    if let Some(pid) = crate::proc::thread::owner_process(task_id) {
        let _ = crate::proc::pcb::set_exit_code(pid, exit_code);
    }

    // Notify the thread system so the owning process can transition
    // to Zombie when its last thread exits.  For bare kernel tasks
    // (not owned by any process), this is a harmless no-op.
    crate::proc::thread::on_thread_exit(task_id);

    sched::task_exit();
    // Unreachable — task_exit never returns.
    SyscallResult::ok(0)
}

/// `SYS_TASK_ID` — get the current task's ID.
pub fn sys_task_id(args: &SyscallArgs) -> SyscallResult {
    let _ = args;
    #[allow(clippy::cast_possible_wrap)]
    let id = sched::current_task_id() as i64;
    SyscallResult::ok(id)
}

/// `SYS_DEBUG_PRINT` — print a byte string to serial (debug only).
///
/// # Safety contract
///
/// `arg0` must be a valid pointer to `arg1` bytes of readable memory.
/// For now (kernel-mode testing), we trust the pointer.  When
/// userspace is implemented, this must validate the pointer against
/// the caller's address space.
pub fn sys_debug_print(args: &SyscallArgs) -> SyscallResult {
    let ptr = args.arg0 as *const u8;
    let len = args.arg1 as usize;

    if ptr.is_null() || len == 0 {
        return SyscallResult::ok(0);
    }

    // Cap length to prevent excessive output.
    let safe_len = len.min(1024);

    // Validate the user buffer is in user space and mapped.
    if let Err(e) = crate::mm::user::validate_user_read(args.arg0, safe_len) {
        return SyscallResult::err(e);
    }

    // SAFETY: Buffer validated above — in user space and mapped.
    let bytes = unsafe { core::slice::from_raw_parts(ptr, safe_len) };

    // Print as UTF-8 if valid, otherwise as hex.
    if let Ok(s) = core::str::from_utf8(bytes) {
        serial_println!("[debug] {}", s);
    } else {
        serial_println!("[debug] <{} non-UTF8 bytes>", safe_len);
    }

    #[allow(clippy::cast_possible_wrap)]
    let written = safe_len as i64;
    SyscallResult::ok(written)
}

// ---------------------------------------------------------------------------
// IRQ management (30–39)
// ---------------------------------------------------------------------------

/// `SYS_IRQ_REGISTER` — claim an IRQ line for the calling task.
///
/// Registers the task for wakeup notifications and unmasks the IRQ
/// on the IOAPIC.  Only one task may be registered per IRQ.
pub fn sys_irq_register(args: &SyscallArgs) -> SyscallResult {
    let irq = args.arg0;

    if irq >= crate::ioapic::MAX_IRQ as u64 {
        return SyscallResult::err(KernelError::InvalidArgument);
    }

    // Capability check: caller must hold a DeviceIrq capability.
    // First try per-IRQ check (resource_id = IRQ number), fall back
    // to type-level check (any DeviceIrq cap with WRITE grants access
    // to all IRQs — for drivers with broad hardware access).
    if require_cap(
        crate::cap::ResourceType::DeviceIrq,
        irq,
        crate::cap::Rights::WRITE,
    ).is_err() {
        // Per-IRQ check failed — try type-level (any DeviceIrq cap).
        if let Err(e) = require_cap_type(
            crate::cap::ResourceType::DeviceIrq,
            crate::cap::Rights::WRITE,
        ) {
            return SyscallResult::err(e);
        }
    }

    let task_id = sched::current_task_id();

    #[allow(clippy::cast_possible_truncation)]
    let irq_u32 = irq as u32;
    #[allow(clippy::cast_possible_truncation)]
    let irq_u8 = irq as u8;

    crate::ioapic::irq_register_task(irq_u32, task_id);

    // SAFETY: The corresponding IDT entry (vector 33 + irq) has an
    // ISR stub registered during idt::init().
    unsafe {
        crate::ioapic::unmask_irq(irq_u8);
    }

    SyscallResult::ok(0)
}

/// `SYS_IRQ_WAIT` — block until an IRQ fires on a registered line.
///
/// If the pending counter is already > 0, consumes and returns
/// immediately.  Otherwise, blocks the calling task until the ISR
/// increments the counter and the deferred-wake mechanism wakes us.
pub fn sys_irq_wait(args: &SyscallArgs) -> SyscallResult {
    let irq = args.arg0;

    if irq >= crate::ioapic::MAX_IRQ as u64 {
        return SyscallResult::err(KernelError::InvalidArgument);
    }

    #[allow(clippy::cast_possible_truncation)]
    let irq_u32 = irq as u32;

    // Fast path: if interrupts are already pending, consume and return.
    let count = crate::ioapic::irq_consume(irq_u32);
    if count > 0 {
        #[allow(clippy::cast_possible_wrap)]
        return SyscallResult::ok(count as i64);
    }

    // Ensure the calling task is registered for this IRQ's wakeup.
    let task_id = sched::current_task_id();
    crate::ioapic::irq_register_task(irq_u32, task_id);

    // Slow path: block until IRQ fires.
    //
    // The ISR will increment the pending counter and attempt to wake
    // us immediately (via try_wake).  If that fails, the timer ISR's
    // deferred-wake scan will catch it within ~10 ms.
    sched::block_current();

    // We've been woken — consume the pending count.
    let count = crate::ioapic::irq_consume(irq_u32);
    // At least 1 interrupt must have fired to wake us.
    let result = if count > 0 { count } else { 1 };
    #[allow(clippy::cast_possible_wrap)]
    SyscallResult::ok(result as i64)
}

/// `SYS_IRQ_RELEASE` — release a previously registered IRQ line.
///
/// Masks the IRQ on the IOAPIC and unregisters the task.
pub fn sys_irq_release(args: &SyscallArgs) -> SyscallResult {
    let irq = args.arg0;

    if irq >= crate::ioapic::MAX_IRQ as u64 {
        return SyscallResult::err(KernelError::InvalidArgument);
    }

    #[allow(clippy::cast_possible_truncation)]
    let irq_u32 = irq as u32;
    #[allow(clippy::cast_possible_truncation)]
    let irq_u8 = irq as u8;

    crate::ioapic::mask_irq(irq_u8);
    crate::ioapic::irq_unregister_task(irq_u32);

    SyscallResult::ok(0)
}

// ---------------------------------------------------------------------------
// Port I/O (40–49)
// ---------------------------------------------------------------------------

/// `SYS_PORT_READ` — read a value from an I/O port.
///
/// `arg0`: port number (0–65535).
/// `arg1`: access width (1 = byte, 2 = word, 4 = dword).
///
/// Returns: the value read.
pub fn sys_port_read(args: &SyscallArgs) -> SyscallResult {
    let port = args.arg0;
    let width = args.arg1;

    if port > u64::from(u16::MAX) {
        return SyscallResult::err(KernelError::InvalidArgument);
    }

    // Capability check: per-port first, then type-level fallback.
    if require_cap(
        crate::cap::ResourceType::PortIo,
        port,
        crate::cap::Rights::READ,
    ).is_err() {
        if let Err(e) = require_cap_type(
            crate::cap::ResourceType::PortIo,
            crate::cap::Rights::READ,
        ) {
            return SyscallResult::err(e);
        }
    }

    #[allow(clippy::cast_possible_truncation)]
    let port_u16 = port as u16;

    // SAFETY: Capability-gated — only processes holding PortIo caps can
    // reach this point.  Port validity is the caller's responsibility.
    let value: u64 = match width {
        1 => u64::from(unsafe { crate::port::inb(port_u16) }),
        2 => u64::from(unsafe { crate::port::inw(port_u16) }),
        4 => u64::from(unsafe { crate::port::inl(port_u16) }),
        _ => return SyscallResult::err(KernelError::InvalidArgument),
    };

    #[allow(clippy::cast_possible_wrap)]
    SyscallResult::ok(value as i64)
}

/// `SYS_PORT_WRITE` — write a value to an I/O port.
///
/// `arg0`: port number (0–65535).
/// `arg1`: access width (1 = byte, 2 = word, 4 = dword).
/// `arg2`: value to write (low bits used per width).
///
/// Returns: 0 on success.
pub fn sys_port_write(args: &SyscallArgs) -> SyscallResult {
    let port = args.arg0;
    let width = args.arg1;
    let value = args.arg2;

    if port > u64::from(u16::MAX) {
        return SyscallResult::err(KernelError::InvalidArgument);
    }

    // Capability check: per-port first, then type-level fallback.
    if require_cap(
        crate::cap::ResourceType::PortIo,
        port,
        crate::cap::Rights::WRITE,
    ).is_err() {
        if let Err(e) = require_cap_type(
            crate::cap::ResourceType::PortIo,
            crate::cap::Rights::WRITE,
        ) {
            return SyscallResult::err(e);
        }
    }

    #[allow(clippy::cast_possible_truncation)]
    let port_u16 = port as u16;

    // SAFETY: Capability-gated — only processes holding PortIo caps can
    // reach this point.  Port validity is the caller's responsibility.
    match width {
        1 => unsafe { crate::port::outb(port_u16, value as u8) },
        2 => unsafe { crate::port::outw(port_u16, value as u16) },
        4 => unsafe { crate::port::outl(port_u16, value as u32) },
        _ => return SyscallResult::err(KernelError::InvalidArgument),
    }

    SyscallResult::ok(0)
}

// ---------------------------------------------------------------------------
// DMA / IOMMU syscalls (42–49)
// ---------------------------------------------------------------------------

/// `SYS_DMA_ALLOC` — allocate a DMA buffer mapped into user space.
///
/// Returns (value, value2) = (user_virt, phys_addr).
pub fn sys_dma_alloc(args: &SyscallArgs) -> SyscallResult {
    let size = args.arg0 as usize;
    let constraint_raw = args.arg1;

    if size == 0 {
        return SyscallResult::err(KernelError::InvalidArgument);
    }

    // Capability check: require PortIo capability (driver privilege).
    if let Err(e) = require_cap_type(
        crate::cap::ResourceType::PortIo,
        crate::cap::Rights::READ,
    ) {
        return SyscallResult::err(e);
    }

    let constraint = match constraint_raw {
        0 => crate::mm::dma::DmaConstraint::None,
        1 => crate::mm::dma::DmaConstraint::Below4G,
        2 => crate::mm::dma::DmaConstraint::Below16M,
        _ => return SyscallResult::err(KernelError::InvalidArgument),
    };

    // Get the calling process's PML4.
    let pml4 = match caller_pml4() {
        Some(p) => p,
        None => return SyscallResult::err(KernelError::InvalidArgument),
    };

    match crate::mm::dma::alloc_for_user(pml4, size, constraint) {
        Ok((user_virt, phys_addr, _actual_size)) => {
            SyscallResult::ok2(user_virt as i64, phys_addr as i64)
        }
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_DMA_FREE` — free a DMA buffer.
pub fn sys_dma_free(args: &SyscallArgs) -> SyscallResult {
    let user_virt = args.arg0;

    let pml4 = match caller_pml4() {
        Some(p) => p,
        None => return SyscallResult::err(KernelError::InvalidArgument),
    };

    match crate::mm::dma::free_for_user(pml4, user_virt) {
        Ok(()) => SyscallResult::ok(0),
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_DMA_DOMAIN_CREATE` — create an IOMMU domain.
pub fn sys_dma_domain_create(_args: &SyscallArgs) -> SyscallResult {
    // Capability check.
    if let Err(e) = require_cap_type(
        crate::cap::ResourceType::PortIo,
        crate::cap::Rights::WRITE,
    ) {
        return SyscallResult::err(e);
    }

    match crate::iommu_remap::create_domain() {
        Ok(id) => SyscallResult::ok(id as i64),
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_DMA_DOMAIN_DESTROY` — destroy an IOMMU domain.
pub fn sys_dma_domain_destroy(args: &SyscallArgs) -> SyscallResult {
    let domain_id = args.arg0 as u16;

    if let Err(e) = require_cap_type(
        crate::cap::ResourceType::PortIo,
        crate::cap::Rights::WRITE,
    ) {
        return SyscallResult::err(e);
    }

    match crate::iommu_remap::destroy_domain(domain_id) {
        Ok(()) => SyscallResult::ok(0),
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_DMA_MAP` — map physical address into IOMMU domain.
pub fn sys_dma_map(args: &SyscallArgs) -> SyscallResult {
    let domain_id = args.arg0 as u16;
    let bus_addr = args.arg1;
    let phys_addr = args.arg2;
    let size = args.arg3 as usize;
    let perms_raw = args.arg4;

    if let Err(e) = require_cap_type(
        crate::cap::ResourceType::PortIo,
        crate::cap::Rights::WRITE,
    ) {
        return SyscallResult::err(e);
    }

    let perms = match perms_raw {
        1 => crate::iommu_remap::DmaPerms::READ,
        2 => crate::iommu_remap::DmaPerms::WRITE,
        3 => crate::iommu_remap::DmaPerms::READ_WRITE,
        _ => return SyscallResult::err(KernelError::InvalidArgument),
    };

    match crate::iommu_remap::map_dma(domain_id, bus_addr, phys_addr, size, perms) {
        Ok(()) => SyscallResult::ok(0),
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_DMA_UNMAP` — unmap bus address range from IOMMU domain.
pub fn sys_dma_unmap(args: &SyscallArgs) -> SyscallResult {
    let domain_id = args.arg0 as u16;
    let bus_addr = args.arg1;
    let size = args.arg2 as usize;

    if let Err(e) = require_cap_type(
        crate::cap::ResourceType::PortIo,
        crate::cap::Rights::WRITE,
    ) {
        return SyscallResult::err(e);
    }

    match crate::iommu_remap::unmap_dma(domain_id, bus_addr, size) {
        Ok(()) => SyscallResult::ok(0),
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_DMA_ATTACH` — attach a PCI device to an IOMMU domain.
pub fn sys_dma_attach(args: &SyscallArgs) -> SyscallResult {
    let domain_id = args.arg0 as u16;
    let bus = args.arg1 as u8;
    let device = args.arg2 as u8;
    let function = args.arg3 as u8;

    if let Err(e) = require_cap_type(
        crate::cap::ResourceType::PortIo,
        crate::cap::Rights::WRITE,
    ) {
        return SyscallResult::err(e);
    }

    match crate::iommu_remap::attach_device(domain_id, bus, device, function) {
        Ok(()) => SyscallResult::ok(0),
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_DMA_DETACH` — detach a PCI device from an IOMMU domain.
pub fn sys_dma_detach(args: &SyscallArgs) -> SyscallResult {
    let domain_id = args.arg0 as u16;
    let bus = args.arg1 as u8;
    let device = args.arg2 as u8;
    let function = args.arg3 as u8;

    if let Err(e) = require_cap_type(
        crate::cap::ResourceType::PortIo,
        crate::cap::Rights::WRITE,
    ) {
        return SyscallResult::err(e);
    }

    match crate::iommu_remap::detach_device(domain_id, bus, device, function) {
        Ok(()) => SyscallResult::ok(0),
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_MMAP` — map memory into the calling process's address space.
///
/// Supports two modes:
/// - **Anonymous**: allocates physical frames and maps them (default).
/// - **MMIO**: maps a specific physical address range (`MAP_MMIO` flag).
///
/// `arg0`: virtual address hint (0 = kernel picks).
/// `arg1`: size in bytes (rounded up to frame boundary).
/// `arg2`: flags (`MAP_READ` | `MAP_WRITE` | `MAP_EXEC` | `MAP_NOCACHE` | `MAP_MMIO`).
/// `arg3`: physical address (only used with `MAP_MMIO`).
///
/// Returns: virtual address of the mapped region, or negative error.
pub fn sys_mmap(args: &SyscallArgs) -> SyscallResult {
    use crate::mm::frame::{PhysFrame, FRAME_SIZE};
    use crate::mm::page_table::{self, PageFlags, VirtAddr};
    use crate::proc::{pcb, thread};
    use super::number::{MAP_EXEC, MAP_LAZY, MAP_MMIO, MAP_NOCACHE, MAP_WRITE};

    let vaddr_hint = args.arg0;
    let size = args.arg1;
    let mut flags = args.arg2;
    let phys_addr = args.arg3;

    // If the system-wide default is lazy allocation and the caller
    // didn't explicitly specify MAP_LAZY or MAP_MMIO, apply lazy as
    // the default.  MMIO mappings are always committed (they must map
    // specific physical addresses).
    if flags & (MAP_LAZY | MAP_MMIO) == 0 {
        if crate::sysctl::get(crate::sysctl::PARAM_MM_LAZY_DEFAULT) == Some(1) {
            flags |= MAP_LAZY;
        }
    }

    // Validate size.
    if size == 0 {
        return SyscallResult::err(KernelError::InvalidArgument);
    }

    // Get the calling process's PML4.
    let task_id = sched::current_task_id();
    let pid = thread::owner_process(task_id).unwrap_or(0);
    let pml4_phys = match pcb::get_pml4(pid) {
        Some(pml4) if pml4 != 0 => pml4,
        _ => return SyscallResult::err(KernelError::NoSuchProcess),
    };

    // Round size up to frame boundary.
    #[allow(clippy::arithmetic_side_effects)]
    let frame_size = FRAME_SIZE as u64;
    #[allow(clippy::arithmetic_side_effects)]
    let size_aligned = (size.saturating_add(frame_size - 1)) & !(frame_size - 1);
    #[allow(clippy::arithmetic_side_effects)]
    let num_frames = (size_aligned / frame_size) as usize;

    if num_frames == 0 {
        return SyscallResult::err(KernelError::InvalidArgument);
    }

    // Build page flags from mmap flags.
    let mut page_flags = PageFlags::PRESENT | PageFlags::USER_ACCESSIBLE;
    if flags & MAP_WRITE != 0 {
        page_flags = page_flags | PageFlags::WRITABLE;
    }
    if flags & MAP_EXEC == 0 {
        page_flags = page_flags | PageFlags::NO_EXECUTE;
    }
    if flags & MAP_NOCACHE != 0 {
        page_flags = page_flags | PageFlags::NO_CACHE;
    }

    // Pick a virtual address.
    let base_vaddr = if vaddr_hint != 0 {
        // Caller specified an address — validate alignment.
        if !vaddr_hint.is_multiple_of(frame_size) {
            return SyscallResult::err(KernelError::BadAlignment);
        }
        vaddr_hint
    } else {
        // Kernel picks: use a simple bump allocator in the mmap region.
        // Range: 0x0000_0060_0000_0000 .. 0x0000_0070_0000_0000.
        mmap_alloc_vaddr(size_aligned)
    };

    if base_vaddr == 0 {
        return SyscallResult::err(KernelError::OutOfMemory);
    }

    if flags & MAP_MMIO != 0 {
        // MMIO mapping: map specific physical address.
        if !phys_addr.is_multiple_of(frame_size) {
            return SyscallResult::err(KernelError::BadAlignment);
        }

        for i in 0..num_frames {
            #[allow(clippy::arithmetic_side_effects)]
            let pa = phys_addr + (i as u64) * frame_size;
            #[allow(clippy::arithmetic_side_effects)]
            let va = base_vaddr + (i as u64) * frame_size;

            let phys = match PhysFrame::from_addr(pa) {
                Some(f) => f,
                None => {
                    // Rollback: unmap frames 0..i (don't free — device memory).
                    for j in 0..i {
                        #[allow(clippy::arithmetic_side_effects)]
                        let rv = base_vaddr + (j as u64) * frame_size;
                        // SAFETY: we mapped these frames successfully above.
                        let _ = unsafe { page_table::unmap_frame(pml4_phys, VirtAddr::new(rv)) };
                    }
                    return SyscallResult::err(KernelError::BadAlignment);
                }
            };

            // SAFETY: pml4_phys is valid, phys is a device MMIO address
            // (not managed by our allocator), virt is in user space.
            if let Err(e) = unsafe {
                page_table::map_frame(pml4_phys, VirtAddr::new(va), phys, page_flags)
            } {
                serial_println!(
                    "[mmap] MMIO map failed at va={:#x} pa={:#x}: {:?}",
                    va, pa, e
                );
                // Rollback: unmap frames 0..i (don't free — device memory).
                for j in 0..i {
                    #[allow(clippy::arithmetic_side_effects)]
                    let rv = base_vaddr + (j as u64) * frame_size;
                    // SAFETY: we mapped these frames successfully above.
                    let _ = unsafe { page_table::unmap_frame(pml4_phys, VirtAddr::new(rv)) };
                }
                return SyscallResult::err(e);
            }
        }

        serial_println!(
            "[mmap] MMIO mapped {:#x}..{:#x} → phys {:#x} ({} frames)",
            base_vaddr, base_vaddr + size_aligned, phys_addr, num_frames
        );
    } else if flags & MAP_LAZY != 0 {
        // Lazy (demand-paged) anonymous mapping: register a VMA but
        // don't allocate physical frames.  Frames are allocated on
        // first access via the page fault handler.
        //
        // This is the opt-in lazy path.  The default (without MAP_LAZY)
        // is committed allocation per the design spec.
        use crate::mm::vma::{Vma, VmaKind};

        // MAP_LAZY + MAP_MMIO makes no sense — MMIO regions must be
        // backed by specific physical addresses, not demand-paged.
        // (MAP_MMIO was already handled above, but defensive check.)
        let vma = Vma {
            start: base_vaddr,
            end: base_vaddr.saturating_add(size_aligned),
            kind: VmaKind::Anonymous,
            flags: page_flags,
        };

        if let Err(e) = pcb::add_vma(pid, vma) {
            serial_println!(
                "[mmap] Lazy VMA registration failed at {:#x}: {:?}",
                base_vaddr, e
            );
            return SyscallResult::err(e);
        }

        serial_println!(
            "[mmap] Lazy mapped {:#x}..{:#x} ({} frames, demand-paged)",
            base_vaddr, base_vaddr + size_aligned, num_frames
        );
    } else {
        // Committed anonymous mapping (default): allocate and map
        // fresh zeroed frames immediately.  map_committed_range handles
        // alloc + zero + map atomically with full rollback on partial failure.
        // SAFETY: pml4_phys is a valid page table for this process.
        if let Err(e) = unsafe {
            page_table::map_committed_range(
                pml4_phys,
                VirtAddr::new(base_vaddr),
                num_frames,
                page_flags,
            )
        } {
            serial_println!(
                "[mmap] Committed map failed at {:#x} ({} frames): {:?}",
                base_vaddr, num_frames, e
            );
            return SyscallResult::err(e);
        }

        serial_println!(
            "[mmap] Committed mapped {:#x}..{:#x} ({} frames)",
            base_vaddr, base_vaddr + size_aligned, num_frames
        );
    }

    #[allow(clippy::cast_possible_wrap)]
    SyscallResult::ok(base_vaddr as i64)
}

/// `SYS_MUNMAP` — unmap a region from the calling process's address space.
///
/// `arg0`: virtual address (must be frame-aligned).
/// `arg1`: size in bytes (rounded up to frame boundary).
///
/// For anonymous mappings, the physical frames are freed back to the
/// allocator.  For MMIO mappings, only the page table entries are
/// cleared (the physical memory belongs to the device).
///
/// Returns: 0 on success.
pub fn sys_munmap(args: &SyscallArgs) -> SyscallResult {
    use crate::mm::frame::{self, FRAME_SIZE};
    use crate::mm::page_table::{self, VirtAddr};
    use crate::proc::{pcb, thread};

    let vaddr = args.arg0;
    let size = args.arg1;

    if size == 0 {
        return SyscallResult::ok(0);
    }

    // Validate alignment.
    let frame_size = FRAME_SIZE as u64;
    if !vaddr.is_multiple_of(frame_size) {
        return SyscallResult::err(KernelError::BadAlignment);
    }

    // Get the calling process's PML4.
    let task_id = sched::current_task_id();
    let pid = thread::owner_process(task_id).unwrap_or(0);
    let pml4_phys = match pcb::get_pml4(pid) {
        Some(pml4) if pml4 != 0 => pml4,
        _ => return SyscallResult::err(KernelError::NoSuchProcess),
    };

    // Round size up.
    #[allow(clippy::arithmetic_side_effects)]
    let size_aligned = (size.saturating_add(frame_size - 1)) & !(frame_size - 1);
    #[allow(clippy::arithmetic_side_effects)]
    let num_frames = (size_aligned / frame_size) as usize;

    let mut unmapped = 0usize;

    for i in 0..num_frames {
        #[allow(clippy::arithmetic_side_effects)]
        let va = vaddr + (i as u64) * frame_size;

        // Unmap returns the physical frame that was mapped.
        // SAFETY: pml4_phys is valid, va was mapped by a previous mmap.
        match unsafe { page_table::unmap_frame(pml4_phys, VirtAddr::new(va)) } {
            Ok(phys) => {
                // Check if this is allocator-owned memory (not MMIO).
                // MMIO physical addresses are typically above the usable
                // RAM range.  A proper VMA tracker would record this;
                // for now, we check if the frame belongs to the allocator.
                if frame::is_allocator_owned(phys) {
                    // SAFETY: The frame was allocated by our allocator
                    // (verified by is_allocator_owned), the mapping was
                    // just removed so no references remain.
                    unsafe { let _ = frame::free_frame(phys); }
                }
                unmapped = unmapped.saturating_add(1);
            }
            Err(_) => {
                // Frame wasn't mapped — skip silently (idempotent).
            }
        }
    }

    // Also remove any per-process VMA that starts at this address.
    // This handles lazy-mapped regions created with MAP_LAZY.
    // If no VMA matches, this is a no-op (committed regions don't have VMAs).
    pcb::remove_vma(pid, vaddr);

    serial_println!(
        "[mmap] Unmapped {} frames at {:#x}..{:#x}",
        unmapped, vaddr, vaddr + size_aligned
    );

    SyscallResult::ok(0)
}

/// Simple bump allocator for mmap virtual addresses.
///
/// Allocates virtual addresses in the range
/// `0x0000_0060_0000_0000..0x0000_0070_0000_0000` (256 GiB region).
/// This is a temporary solution — a proper VMA (virtual memory area)
/// tracker will replace this.
pub(crate) fn mmap_alloc_vaddr(size: u64) -> u64 {
    use core::sync::atomic::{AtomicU64, Ordering};

    /// Base of the mmap region in user address space.
    const MMAP_BASE: u64 = 0x0000_0060_0000_0000;
    /// End of the mmap region (exclusive).
    const MMAP_END: u64 = 0x0000_0070_0000_0000;

    static NEXT_VADDR: AtomicU64 = AtomicU64::new(MMAP_BASE);

    let addr = NEXT_VADDR.fetch_add(size, Ordering::Relaxed);
    if addr.checked_add(size).is_none_or(|end| end > MMAP_END) {
        // Ran out of mmap space.
        return 0;
    }
    addr
}

// ---------------------------------------------------------------------------
// IPC handlers (200–399)
// ---------------------------------------------------------------------------

/// `SYS_CHANNEL_CREATE` — create a new IPC channel pair.
///
/// `arg0`: flags (bit 0 = sync/rendezvous mode).
///
/// Returns both handles: `value` = ep0, `value2` = ep1.
///
/// If `CHANNEL_FLAG_SYNC` (bit 0) is set, creates a synchronous
/// (rendezvous) channel with no internal buffer.  Sends block until
/// a receiver takes the message (L4/seL4-style synchronous IPC).
pub fn sys_channel_create(args: &SyscallArgs) -> SyscallResult {
    let flags = args.arg0;
    let sync = flags & CHANNEL_FLAG_SYNC != 0;

    let (ep0, ep1) = if sync {
        channel::create_sync()
    } else {
        channel::create()
    };

    // Register both endpoints for cleanup on process death.
    if let Some(pid) = caller_pid() {
        pcb::register_ipc_handle(pid, ResourceType::Channel, ep0.raw());
        pcb::register_ipc_handle(pid, ResourceType::Channel, ep1.raw());
    }

    // Pack handles into the two return registers.
    #[allow(clippy::cast_possible_wrap)]
    let r0 = ep0.raw() as i64;
    #[allow(clippy::cast_possible_wrap)]
    let r1 = ep1.raw() as i64;
    SyscallResult::ok2(r0, r1)
}

/// `SYS_CHANNEL_SEND` — send a message on a channel.
///
/// `arg0`: channel handle.
/// `arg1`: pointer to message data.
/// `arg2`: length of message data.
pub fn sys_channel_send(args: &SyscallArgs) -> SyscallResult {
    let handle = ChannelHandle::from_raw(args.arg0);
    let ptr = args.arg1 as *const u8;
    let len = args.arg2 as usize;

    if ptr.is_null() && len > 0 {
        return SyscallResult::err(KernelError::InvalidArgument);
    }

    // Validate user buffer.
    if len > 0 {
        if let Err(e) = crate::mm::user::validate_user_read(args.arg1, len) {
            return SyscallResult::err(e);
        }
    }

    // SAFETY: Buffer validated above — in user space and mapped.
    let data = if len == 0 {
        &[]
    } else {
        unsafe { core::slice::from_raw_parts(ptr, len) }
    };

    let msg = match Message::from_bytes(data) {
        Ok(m) => m,
        Err(e) => return SyscallResult::err(e),
    };

    match channel::send(handle, msg) {
        Ok(()) => SyscallResult::ok(0),
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_CHANNEL_RECV` — blocking receive on a channel.
///
/// `arg0`: channel handle.
/// `arg1`: pointer to receive buffer.
/// `arg2`: buffer capacity.
///
/// Returns: message length on success.
pub fn sys_channel_recv(args: &SyscallArgs) -> SyscallResult {
    let handle = ChannelHandle::from_raw(args.arg0);
    let buf_ptr = args.arg1 as *mut u8;
    let buf_cap = args.arg2 as usize;

    if buf_ptr.is_null() && buf_cap > 0 {
        return SyscallResult::err(KernelError::InvalidArgument);
    }

    // Validate user buffer is writable.
    if buf_cap > 0 {
        if let Err(e) = crate::mm::user::validate_user_write(args.arg1, buf_cap) {
            return SyscallResult::err(e);
        }
    }

    match channel::recv(handle) {
        Ok(msg) => {
            let data = msg.data();
            let copy_len = data.len().min(buf_cap);

            if copy_len > 0 {
                // SAFETY: Buffer validated above — in user space, mapped, writable.
                unsafe {
                    core::ptr::copy_nonoverlapping(
                        data.as_ptr(),
                        buf_ptr,
                        copy_len,
                    );
                }
            }

            #[allow(clippy::cast_possible_wrap)]
            let len = data.len() as i64;
            SyscallResult::ok(len)
        }
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_CHANNEL_TRY_RECV` — non-blocking receive on a channel.
///
/// `arg0`: channel handle.
/// `arg1`: pointer to receive buffer.
/// `arg2`: buffer capacity.
///
/// Returns: message length, 0 if empty, negative error code on failure.
pub fn sys_channel_try_recv(args: &SyscallArgs) -> SyscallResult {
    let handle = ChannelHandle::from_raw(args.arg0);
    let buf_ptr = args.arg1 as *mut u8;
    let buf_cap = args.arg2 as usize;

    if buf_ptr.is_null() && buf_cap > 0 {
        return SyscallResult::err(KernelError::InvalidArgument);
    }

    // Validate user buffer is writable.
    if buf_cap > 0 {
        if let Err(e) = crate::mm::user::validate_user_write(args.arg1, buf_cap) {
            return SyscallResult::err(e);
        }
    }

    match channel::try_recv(handle) {
        Ok(Some(msg)) => {
            let data = msg.data();
            let copy_len = data.len().min(buf_cap);

            if copy_len > 0 {
                // SAFETY: Validated above — buf_ptr is in user space, mapped, and writable.
                unsafe {
                    core::ptr::copy_nonoverlapping(
                        data.as_ptr(),
                        buf_ptr,
                        copy_len,
                    );
                }
            }

            #[allow(clippy::cast_possible_wrap)]
            let len = data.len() as i64;
            SyscallResult::ok(len)
        }
        Ok(None) => {
            // No message available — return 0 (not an error).
            SyscallResult::ok(0)
        }
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_CHANNEL_CLOSE` — close a channel endpoint.
///
/// `arg0`: channel handle.
pub fn sys_channel_close(args: &SyscallArgs) -> SyscallResult {
    let handle = ChannelHandle::from_raw(args.arg0);
    if let Some(pid) = caller_pid() {
        pcb::deregister_ipc_handle(pid, ResourceType::Channel, handle.raw());
    }
    channel::close(handle);
    SyscallResult::ok(0)
}

/// `SYS_CHANNEL_RECV_TIMEOUT` — receive with a deadline.
///
/// `arg0`: channel handle.
/// `arg1`: pointer to receive buffer.
/// `arg2`: buffer capacity.
/// `arg3`: timeout in nanoseconds (0 = non-blocking try).
///
/// Returns: message length on success, `TimedOut` if deadline expires.
pub fn sys_channel_recv_timeout(args: &SyscallArgs) -> SyscallResult {
    let handle = ChannelHandle::from_raw(args.arg0);
    let buf_ptr = args.arg1 as *mut u8;
    let buf_cap = args.arg2 as usize;
    let timeout_ns = args.arg3;

    if buf_ptr.is_null() && buf_cap > 0 {
        return SyscallResult::err(KernelError::InvalidArgument);
    }

    if buf_cap > 0 {
        if let Err(e) = crate::mm::user::validate_user_write(args.arg1, buf_cap) {
            return SyscallResult::err(e);
        }
    }

    match channel::recv_timeout(handle, timeout_ns) {
        Ok(msg) => {
            let data = msg.data();
            let copy_len = data.len().min(buf_cap);

            if copy_len > 0 {
                // SAFETY: Buffer validated above — in user space, mapped, writable.
                unsafe {
                    core::ptr::copy_nonoverlapping(
                        data.as_ptr(),
                        buf_ptr,
                        copy_len,
                    );
                }
            }

            #[allow(clippy::cast_possible_wrap)]
            let len = data.len() as i64;
            SyscallResult::ok(len)
        }
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_CHANNEL_SEND_TIMEOUT` — send with a deadline.
///
/// `arg0`: channel handle.
/// `arg1`: pointer to message data.
/// `arg2`: message data length.
/// `arg3`: timeout in nanoseconds (0 = return TimedOut if full).
///
/// Returns: 0 on success, `TimedOut` if deadline expires.
pub fn sys_channel_send_timeout(args: &SyscallArgs) -> SyscallResult {
    let handle = ChannelHandle::from_raw(args.arg0);
    let data_ptr = args.arg1 as *const u8;
    let data_len = args.arg2 as usize;
    let timeout_ns = args.arg3;

    if data_ptr.is_null() && data_len > 0 {
        return SyscallResult::err(KernelError::InvalidArgument);
    }

    if data_len > 0 {
        if let Err(e) = crate::mm::user::validate_user_read(args.arg1, data_len) {
            return SyscallResult::err(e);
        }
    }

    let data = if data_len == 0 {
        &[]
    } else {
        // SAFETY: Validated above — data_ptr is in user space, mapped, readable.
        unsafe { core::slice::from_raw_parts(data_ptr, data_len) }
    };

    let msg = match Message::from_bytes(data) {
        Ok(m) => m,
        Err(e) => return SyscallResult::err(e),
    };

    match channel::send_timeout(handle, msg, timeout_ns) {
        Ok(()) => SyscallResult::ok(0),
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_CHANNEL_SEND_BLOCKING` — send, blocking when queue is full.
///
/// `arg0`: channel handle.
/// `arg1`: pointer to message data.
/// `arg2`: message data length.
///
/// Returns: 0 on success.
pub fn sys_channel_send_blocking(args: &SyscallArgs) -> SyscallResult {
    let handle = ChannelHandle::from_raw(args.arg0);
    let data_ptr = args.arg1 as *const u8;
    let data_len = args.arg2 as usize;

    if data_ptr.is_null() && data_len > 0 {
        return SyscallResult::err(KernelError::InvalidArgument);
    }

    if data_len > 0 {
        if let Err(e) = crate::mm::user::validate_user_read(args.arg1, data_len) {
            return SyscallResult::err(e);
        }
    }

    let data = if data_len == 0 {
        &[]
    } else {
        // SAFETY: Validated above — data_ptr is in user space, mapped, readable.
        unsafe { core::slice::from_raw_parts(data_ptr, data_len) }
    };

    let msg = match Message::from_bytes(data) {
        Ok(m) => m,
        Err(e) => return SyscallResult::err(e),
    };

    match channel::send_blocking(handle, msg) {
        Ok(()) => SyscallResult::ok(0),
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_CHANNEL_SEND_CAPS` — send a message with capability transfer.
///
/// `arg0`: channel handle.
/// `arg1`: pointer to message data.
/// `arg2`: message data length.
/// `arg3`: pointer to array of u64 cap handles.
/// `arg4`: number of cap handles.
///
/// Returns: 0 on success.
pub fn sys_channel_send_caps(args: &SyscallArgs) -> SyscallResult {
    let handle = ChannelHandle::from_raw(args.arg0);
    let data_ptr = args.arg1 as *const u8;
    let data_len = args.arg2 as usize;
    let caps_ptr = args.arg3 as *const u64;
    let caps_count = args.arg4 as usize;

    // Validate data buffer.
    if data_ptr.is_null() && data_len > 0 {
        return SyscallResult::err(KernelError::InvalidArgument);
    }
    if data_len > 0 {
        if let Err(e) = crate::mm::user::validate_user_read(args.arg1, data_len) {
            return SyscallResult::err(e);
        }
    }

    // Validate caps array.
    if caps_ptr.is_null() && caps_count > 0 {
        return SyscallResult::err(KernelError::InvalidArgument);
    }
    if caps_count > 0 {
        let caps_bytes = caps_count.saturating_mul(8); // Each handle is u64 = 8 bytes.
        if let Err(e) = crate::mm::user::validate_user_read(args.arg3, caps_bytes) {
            return SyscallResult::err(e);
        }
    }

    let data = if data_len == 0 {
        &[]
    } else {
        // SAFETY: Validated above.
        unsafe { core::slice::from_raw_parts(data_ptr, data_len) }
    };

    let cap_handles = if caps_count == 0 {
        &[]
    } else {
        // SAFETY: Validated above.
        unsafe { core::slice::from_raw_parts(caps_ptr, caps_count) }
    };

    // Get sender PID.
    let sender_pid = match caller_pid() {
        Some(pid) => pid,
        None => 0, // Kernel task — no cap table management needed.
    };

    match channel::send_with_caps(handle, data, cap_handles, sender_pid) {
        Ok(()) => SyscallResult::ok(0),
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_CHANNEL_RECV_CAPS` — receive a message with capability transfer.
///
/// `arg0`: channel handle.
/// `arg1`: pointer to message receive buffer.
/// `arg2`: message buffer capacity.
/// `arg3`: pointer to output array for u64 cap handles.
/// `arg4`: capacity of the cap handle output array.
///
/// Returns (rax): message data length.
/// Returns (rdx): number of capability handles received.
pub fn sys_channel_recv_caps(args: &SyscallArgs) -> SyscallResult {
    let handle = ChannelHandle::from_raw(args.arg0);
    let buf_ptr = args.arg1 as *mut u8;
    let buf_cap = args.arg2 as usize;
    let caps_out_ptr = args.arg3 as *mut u64;
    let caps_out_cap = args.arg4 as usize;

    // Validate message buffer.
    if buf_ptr.is_null() && buf_cap > 0 {
        return SyscallResult::err(KernelError::InvalidArgument);
    }
    if buf_cap > 0 {
        if let Err(e) = crate::mm::user::validate_user_write(args.arg1, buf_cap) {
            return SyscallResult::err(e);
        }
    }

    // Validate caps output array.
    if caps_out_ptr.is_null() && caps_out_cap > 0 {
        return SyscallResult::err(KernelError::InvalidArgument);
    }
    if caps_out_cap > 0 {
        let caps_bytes = caps_out_cap.saturating_mul(8);
        if let Err(e) = crate::mm::user::validate_user_write(args.arg3, caps_bytes) {
            return SyscallResult::err(e);
        }
    }

    // Get receiver PID.
    let receiver_pid = caller_pid().unwrap_or_default();

    match channel::recv_with_caps(handle, receiver_pid) {
        Ok((data, new_handles)) => {
            // Copy message data to user buffer.
            let copy_len = data.len().min(buf_cap);
            if copy_len > 0 {
                // SAFETY: Buffer validated above.
                unsafe {
                    core::ptr::copy_nonoverlapping(
                        data.as_ptr(),
                        buf_ptr,
                        copy_len,
                    );
                }
            }

            // Copy cap handles to user output array.
            let caps_copy = new_handles.len().min(caps_out_cap);
            if caps_copy > 0 {
                // SAFETY: Buffer validated above.
                unsafe {
                    core::ptr::copy_nonoverlapping(
                        new_handles.as_ptr(),
                        caps_out_ptr,
                        caps_copy,
                    );
                }
            }

            #[allow(clippy::cast_possible_wrap)]
            let msg_len = data.len() as i64;
            #[allow(clippy::cast_possible_wrap)]
            let caps_received = caps_copy as i64;
            SyscallResult::ok2(msg_len, caps_received)
        }
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_FUTEX_WAIT` — block if `*addr == expected`.
///
/// `arg0`: pointer to 32-bit futex word (4-byte aligned).
/// `arg1`: expected value.
///
/// Returns: 1 if blocked and woken, 0 if value didn't match.
pub fn sys_futex_wait(args: &SyscallArgs) -> SyscallResult {
    let addr = args.arg0;
    let expected = args.arg1 as u32;

    // Futex word is 4 bytes — validate the pointer is in user space
    // and the page is mapped (futex_wait reads the value at addr).
    if let Err(e) = crate::mm::user::validate_user_read(addr, 4) {
        return SyscallResult::err(e);
    }

    match futex::futex_wait(addr, expected) {
        Ok(true) => SyscallResult::ok(1),
        Ok(false) => SyscallResult::ok(0),
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_FUTEX_WAKE` — wake up to `max_wake` waiters on a futex.
///
/// `arg0`: pointer to futex word.
/// `arg1`: maximum number of tasks to wake.
///
/// Returns: number of tasks actually woken.
pub fn sys_futex_wake(args: &SyscallArgs) -> SyscallResult {
    let addr = args.arg0;
    let max_wake = args.arg1 as u32;

    // The address is used as a wait queue key.  Validate it's in
    // user space to prevent kernel address confusion.
    if let Err(e) = crate::mm::user::validate_user_ptr(addr) {
        return SyscallResult::err(e);
    }

    let woken = futex::futex_wake(addr, max_wake);
    SyscallResult::ok(i64::from(woken))
}

/// `SYS_FUTEX_WAIT_TIMEOUT` — block on a futex with a deadline.
///
/// `arg0`: pointer to 32-bit futex word (4-byte aligned).
/// `arg1`: expected value.
/// `arg2`: timeout in nanoseconds (0 = non-blocking check).
///
/// Returns: 1 if blocked and woken, 0 if value didn't match, `TimedOut`.
pub fn sys_futex_wait_timeout(args: &SyscallArgs) -> SyscallResult {
    let addr = args.arg0;
    let expected = args.arg1 as u32;
    let timeout_ns = args.arg2;

    if let Err(e) = crate::mm::user::validate_user_read(addr, 4) {
        return SyscallResult::err(e);
    }

    match futex::futex_wait_timeout(addr, expected, timeout_ns) {
        Ok(true) => SyscallResult::ok(1),
        Ok(false) => SyscallResult::ok(0),
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_FUTEX_LOCK_PI` — lock a PI (Priority Inheritance) futex.
///
/// `arg0`: pointer to 32-bit futex word (4-byte aligned).
///
/// If uncontended, acquires immediately.  If contended, blocks and
/// boosts the lock holder's priority to the caller's level.
///
/// Returns: 0 on success.
pub fn sys_futex_lock_pi(args: &SyscallArgs) -> SyscallResult {
    let addr = args.arg0;
    // PI futex reads and writes the 4-byte futex word.
    if let Err(e) = crate::mm::user::validate_user_write(addr, 4) {
        return SyscallResult::err(e);
    }

    match futex::futex_lock_pi(addr) {
        Ok(()) => SyscallResult::ok(0),
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_FUTEX_UNLOCK_PI` — unlock a PI (Priority Inheritance) futex.
///
/// `arg0`: pointer to 32-bit futex word (4-byte aligned).
///
/// Releases the lock and transfers ownership to the highest-priority
/// waiter.  Restores the caller's inherited priority.
///
/// Returns: 0 on success.
pub fn sys_futex_unlock_pi(args: &SyscallArgs) -> SyscallResult {
    let addr = args.arg0;

    // PI futex reads and writes the 4-byte futex word.
    if let Err(e) = crate::mm::user::validate_user_write(addr, 4) {
        return SyscallResult::err(e);
    }

    match futex::futex_unlock_pi(addr) {
        Ok(()) => SyscallResult::ok(0),
        Err(e) => SyscallResult::err(e),
    }
}

// ---------------------------------------------------------------------------
// Pipe handlers (220–229)
// ---------------------------------------------------------------------------

/// `SYS_PIPE_CREATE` — create a one-way pipe.
///
/// Returns both handles: `value` = read end, `value2` = write end.
pub fn sys_pipe_create(args: &SyscallArgs) -> SyscallResult {
    let _ = args;
    let (read_handle, write_handle) = pipe::create();

    if let Some(pid) = caller_pid() {
        pcb::register_ipc_handle(pid, ResourceType::Pipe, read_handle.raw());
        pcb::register_ipc_handle(pid, ResourceType::Pipe, write_handle.raw());
    }

    #[allow(clippy::cast_possible_wrap)]
    let r0 = read_handle.raw() as i64;
    #[allow(clippy::cast_possible_wrap)]
    let r1 = write_handle.raw() as i64;
    SyscallResult::ok2(r0, r1)
}

/// `SYS_PIPE_WRITE` — write bytes to a pipe (blocking).
///
/// `arg0`: write-end pipe handle.
/// `arg1`: pointer to data buffer.
/// `arg2`: number of bytes to write.
///
/// Returns: number of bytes written.
pub fn sys_pipe_write(args: &SyscallArgs) -> SyscallResult {
    let handle = PipeHandle::from_raw(args.arg0);
    let ptr = args.arg1 as *const u8;
    let len = args.arg2 as usize;

    if ptr.is_null() && len > 0 {
        return SyscallResult::err(KernelError::InvalidArgument);
    }

    if len > 0 {
        if let Err(e) = crate::mm::user::validate_user_read(args.arg1, len) {
            return SyscallResult::err(e);
        }
    }

    let data = if len == 0 {
        &[]
    } else {
        // SAFETY: Buffer validated above — in user space and mapped.
        unsafe { core::slice::from_raw_parts(ptr, len) }
    };

    match pipe::write(handle, data) {
        Ok(n) => {
            #[allow(clippy::cast_possible_wrap)]
            let written = n as i64;
            SyscallResult::ok(written)
        }
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_PIPE_READ` — read bytes from a pipe (blocking).
///
/// `arg0`: read-end pipe handle.
/// `arg1`: pointer to receive buffer.
/// `arg2`: buffer capacity.
///
/// Returns: number of bytes read (0 = EOF).
pub fn sys_pipe_read(args: &SyscallArgs) -> SyscallResult {
    let handle = PipeHandle::from_raw(args.arg0);
    let buf_ptr = args.arg1 as *mut u8;
    let buf_cap = args.arg2 as usize;

    if buf_ptr.is_null() && buf_cap > 0 {
        return SyscallResult::err(KernelError::InvalidArgument);
    }

    if buf_cap > 0 {
        if let Err(e) = crate::mm::user::validate_user_write(args.arg1, buf_cap) {
            return SyscallResult::err(e);
        }
    }

    let buf = if buf_cap == 0 {
        &mut []
    } else {
        // SAFETY: Buffer validated above — in user space, mapped, writable.
        unsafe { core::slice::from_raw_parts_mut(buf_ptr, buf_cap) }
    };

    match pipe::read(handle, buf) {
        Ok(n) => {
            #[allow(clippy::cast_possible_wrap)]
            let read_bytes = n as i64;
            SyscallResult::ok(read_bytes)
        }
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_PIPE_TRY_WRITE` — non-blocking write to a pipe.
///
/// Same as `SYS_PIPE_WRITE` but returns `WouldBlock` if buffer is full.
pub fn sys_pipe_try_write(args: &SyscallArgs) -> SyscallResult {
    let handle = PipeHandle::from_raw(args.arg0);
    let ptr = args.arg1 as *const u8;
    let len = args.arg2 as usize;

    if ptr.is_null() && len > 0 {
        return SyscallResult::err(KernelError::InvalidArgument);
    }

    if len > 0 {
        if let Err(e) = crate::mm::user::validate_user_read(args.arg1, len) {
            return SyscallResult::err(e);
        }
    }

    let data = if len == 0 {
        &[]
    } else {
        // SAFETY: Validated above — ptr is in user space and mapped.
        unsafe { core::slice::from_raw_parts(ptr, len) }
    };

    match pipe::try_write(handle, data) {
        Ok(n) => {
            #[allow(clippy::cast_possible_wrap)]
            let written = n as i64;
            SyscallResult::ok(written)
        }
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_PIPE_TRY_READ` — non-blocking read from a pipe.
///
/// Same as `SYS_PIPE_READ` but returns `WouldBlock` if empty.
pub fn sys_pipe_try_read(args: &SyscallArgs) -> SyscallResult {
    let handle = PipeHandle::from_raw(args.arg0);
    let buf_ptr = args.arg1 as *mut u8;
    let buf_cap = args.arg2 as usize;

    if buf_ptr.is_null() && buf_cap > 0 {
        return SyscallResult::err(KernelError::InvalidArgument);
    }

    if buf_cap > 0 {
        if let Err(e) = crate::mm::user::validate_user_write(args.arg1, buf_cap) {
            return SyscallResult::err(e);
        }
    }

    let buf = if buf_cap == 0 {
        &mut []
    } else {
        // SAFETY: Validated above — buf_ptr is in user space, mapped, and writable.
        unsafe { core::slice::from_raw_parts_mut(buf_ptr, buf_cap) }
    };

    match pipe::try_read(handle, buf) {
        Ok(n) => {
            #[allow(clippy::cast_possible_wrap)]
            let read_bytes = n as i64;
            SyscallResult::ok(read_bytes)
        }
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_PIPE_CLOSE` — close a pipe handle.
///
/// `arg0`: pipe handle (either end).
pub fn sys_pipe_close(args: &SyscallArgs) -> SyscallResult {
    let handle = PipeHandle::from_raw(args.arg0);
    if let Some(pid) = caller_pid() {
        pcb::deregister_ipc_handle(pid, ResourceType::Pipe, handle.raw());
    }
    pipe::close(handle);
    SyscallResult::ok(0)
}

/// `SYS_PIPE_POLL` — query pipe readiness for poll/select.
///
/// `arg0`: pipe handle (either end).
///
/// Returns a bitmask:
/// - bit 0 (0x01): readable
/// - bit 2 (0x04): writable
/// - bit 4 (0x10): hangup (other end closed)
pub fn sys_pipe_poll(args: &SyscallArgs) -> SyscallResult {
    let handle = PipeHandle::from_raw(args.arg0);
    let flags = pipe::poll_status(handle);
    SyscallResult::ok(flags as i64)
}

/// `SYS_PIPE_READABLE_BYTES` — return bytes buffered in a pipe.
pub fn sys_pipe_readable_bytes(args: &SyscallArgs) -> SyscallResult {
    let handle = PipeHandle::from_raw(args.arg0);
    let bytes = pipe::readable_bytes(handle);
    SyscallResult::ok(bytes as i64)
}

/// `SYS_PIPE_READ_TIMEOUT` — read from a pipe with a deadline.
///
/// `arg0`: pipe handle (read end).
/// `arg1`: pointer to receive buffer.
/// `arg2`: buffer capacity.
/// `arg3`: timeout in nanoseconds (0 = non-blocking try).
///
/// Returns: bytes read, 0 if EOF, `TimedOut` if deadline expires.
pub fn sys_pipe_read_timeout(args: &SyscallArgs) -> SyscallResult {
    let handle = PipeHandle::from_raw(args.arg0);
    let buf_ptr = args.arg1 as *mut u8;
    let buf_cap = args.arg2 as usize;
    let timeout_ns = args.arg3;

    if buf_ptr.is_null() && buf_cap > 0 {
        return SyscallResult::err(KernelError::InvalidArgument);
    }

    if buf_cap > 0 {
        if let Err(e) = crate::mm::user::validate_user_write(args.arg1, buf_cap) {
            return SyscallResult::err(e);
        }
    }

    let buf = if buf_cap == 0 {
        &mut []
    } else {
        // SAFETY: Validated above — buf_ptr is in user space, mapped, and writable.
        unsafe { core::slice::from_raw_parts_mut(buf_ptr, buf_cap) }
    };

    match pipe::read_timeout(handle, buf, timeout_ns) {
        Ok(n) => {
            #[allow(clippy::cast_possible_wrap)]
            let read_bytes = n as i64;
            SyscallResult::ok(read_bytes)
        }
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_PIPE_WRITE_TIMEOUT` — write to a pipe with a deadline.
///
/// `arg0`: pipe handle (write end).
/// `arg1`: pointer to data buffer.
/// `arg2`: data length.
/// `arg3`: timeout in nanoseconds (0 = non-blocking try).
///
/// Returns: bytes written, `TimedOut` if deadline expires.
pub fn sys_pipe_write_timeout(args: &SyscallArgs) -> SyscallResult {
    let handle = PipeHandle::from_raw(args.arg0);
    let data_ptr = args.arg1 as *const u8;
    let data_len = args.arg2 as usize;
    let timeout_ns = args.arg3;

    if data_ptr.is_null() && data_len > 0 {
        return SyscallResult::err(KernelError::InvalidArgument);
    }

    if data_len > 0 {
        if let Err(e) = crate::mm::user::validate_user_read(args.arg1, data_len) {
            return SyscallResult::err(e);
        }
    }

    let data = if data_len == 0 {
        &[]
    } else {
        // SAFETY: Validated above — data_ptr is in user space, mapped, and readable.
        unsafe { core::slice::from_raw_parts(data_ptr, data_len) }
    };

    match pipe::write_timeout(handle, data, timeout_ns) {
        Ok(n) => {
            #[allow(clippy::cast_possible_wrap)]
            let written = n as i64;
            SyscallResult::ok(written)
        }
        Err(e) => SyscallResult::err(e),
    }
}

// ---------------------------------------------------------------------------
// Shared memory handlers (230–239)
// ---------------------------------------------------------------------------

/// `SYS_SHM_CREATE` — create a shared memory region.
///
/// `arg0`: requested size in bytes.
///
/// Returns: shared memory handle.
pub fn sys_shm_create(args: &SyscallArgs) -> SyscallResult {
    let size = args.arg0 as usize;

    match shm::create(size) {
        Ok(handle) => {
            if let Some(pid) = caller_pid() {
                pcb::register_ipc_handle(pid, ResourceType::SharedMemory, handle.raw());
            }
            #[allow(clippy::cast_possible_wrap)]
            let h = handle.raw() as i64;
            SyscallResult::ok(h)
        }
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_SHM_SIZE` — query the size of a shared memory region.
///
/// `arg0`: shared memory handle.
///
/// Returns: size in bytes.
pub fn sys_shm_size(args: &SyscallArgs) -> SyscallResult {
    let handle = ShmHandle::from_raw(args.arg0);

    match shm::size(handle) {
        Ok(sz) => {
            #[allow(clippy::cast_possible_wrap)]
            let s = sz as i64;
            SyscallResult::ok(s)
        }
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_SHM_CLOSE` — close a shared memory handle.
///
/// `arg0`: shared memory handle.
pub fn sys_shm_close(args: &SyscallArgs) -> SyscallResult {
    let handle = ShmHandle::from_raw(args.arg0);
    if let Some(pid) = caller_pid() {
        pcb::deregister_ipc_handle(pid, ResourceType::SharedMemory, handle.raw());
    }
    shm::close(handle);
    SyscallResult::ok(0)
}

// ---------------------------------------------------------------------------
// Eventfd handlers (240–249)
// ---------------------------------------------------------------------------

/// `SYS_EVENTFD_CREATE` — create a new eventfd counter.
///
/// `arg0`: initial counter value.
/// `arg1`: flags.  Bit 0 (`EVENTFD_SEMAPHORE_FLAG`) selects semaphore
///         mode: `read()` decrements the counter by 1 and returns 1
///         (matches Linux `EFD_SEMAPHORE`).  All other bits are
///         reserved and must be 0 — userspace handles `EFD_CLOEXEC`
///         and `EFD_NONBLOCK` itself via the fd-table layer.
///
/// Returns: eventfd handle, or `InvalidArgument` if reserved flag
/// bits are set.
pub fn sys_eventfd_create(args: &SyscallArgs) -> SyscallResult {
    const EVENTFD_SEMAPHORE_FLAG: u64 = 1;

    let initial = args.arg0;
    let flags = args.arg1;
    if flags & !EVENTFD_SEMAPHORE_FLAG != 0 {
        return SyscallResult::err(KernelError::InvalidArgument);
    }
    let semaphore = flags & EVENTFD_SEMAPHORE_FLAG != 0;
    let handle = eventfd::create_with_flags(initial, semaphore);

    if let Some(pid) = caller_pid() {
        pcb::register_ipc_handle(pid, ResourceType::EventFd, handle.raw());
    }

    #[allow(clippy::cast_possible_wrap)]
    let h = handle.raw() as i64;
    SyscallResult::ok(h)
}

/// `SYS_EVENTFD_WRITE` — signal an eventfd (add value to counter).
///
/// `arg0`: eventfd handle.
/// `arg1`: value to add.
///
/// Returns: 0 on success.
pub fn sys_eventfd_write(args: &SyscallArgs) -> SyscallResult {
    let handle = EventFdHandle::from_raw(args.arg0);
    let value = args.arg1;

    match eventfd::write(handle, value) {
        Ok(()) => SyscallResult::ok(0),
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_EVENTFD_READ` — consume the eventfd counter (blocking).
///
/// `arg0`: eventfd handle.
///
/// Returns: counter value (> 0).
pub fn sys_eventfd_read(args: &SyscallArgs) -> SyscallResult {
    let handle = EventFdHandle::from_raw(args.arg0);

    match eventfd::read(handle) {
        Ok(val) => {
            #[allow(clippy::cast_possible_wrap)]
            let v = val as i64;
            SyscallResult::ok(v)
        }
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_EVENTFD_TRY_READ` — non-blocking read on an eventfd.
///
/// `arg0`: eventfd handle.
///
/// Returns: counter value, or `WouldBlock` if counter is 0.
pub fn sys_eventfd_try_read(args: &SyscallArgs) -> SyscallResult {
    let handle = EventFdHandle::from_raw(args.arg0);

    match eventfd::try_read(handle) {
        Ok(val) => {
            #[allow(clippy::cast_possible_wrap)]
            let v = val as i64;
            SyscallResult::ok(v)
        }
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_EVENTFD_CLOSE` — close an eventfd handle.
///
/// `arg0`: eventfd handle.
pub fn sys_eventfd_close(args: &SyscallArgs) -> SyscallResult {
    let handle = EventFdHandle::from_raw(args.arg0);
    if let Some(pid) = caller_pid() {
        pcb::deregister_ipc_handle(pid, ResourceType::EventFd, handle.raw());
    }
    eventfd::close(handle);
    SyscallResult::ok(0)
}

/// `SYS_EVENTFD_READ_TIMEOUT` — read with a timeout (nanoseconds).
///
/// `arg0`: eventfd handle.
/// `arg1`: timeout in nanoseconds (0 = non-blocking try).
///
/// Returns: counter value, or `TimedOut` if deadline expires.
pub fn sys_eventfd_read_timeout(args: &SyscallArgs) -> SyscallResult {
    let handle = EventFdHandle::from_raw(args.arg0);
    let timeout_ns = args.arg1;

    match eventfd::read_timeout(handle, timeout_ns) {
        Ok(val) => {
            #[allow(clippy::cast_possible_wrap)]
            SyscallResult::ok(val as i64)
        }
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_EVENTFD_WRITE_TIMEOUT` — write with a timeout (nanoseconds).
///
/// `arg0`: eventfd handle.
/// `arg1`: value to add.
/// `arg2`: timeout in nanoseconds.
pub fn sys_eventfd_write_timeout(args: &SyscallArgs) -> SyscallResult {
    let handle = EventFdHandle::from_raw(args.arg0);
    let value = args.arg1;
    let timeout_ns = args.arg2;

    match eventfd::write_timeout(handle, value, timeout_ns) {
        Ok(()) => SyscallResult::ok(0),
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_EVENTFD_HAS_VALUE` — non-destructive readiness query.
///
/// `arg0`: eventfd handle.
///
/// Returns: 1 if the counter is > 0 (readable), 0 if the counter is 0.
/// Used by `poll`/`select`/`epoll` to determine readability without
/// consuming the eventfd value.
pub fn sys_eventfd_has_value(args: &SyscallArgs) -> SyscallResult {
    let handle = EventFdHandle::from_raw(args.arg0);
    SyscallResult::ok(i64::from(eventfd::has_value(handle)))
}

// ---------------------------------------------------------------------------
// Completion port handlers (250–259)
// ---------------------------------------------------------------------------

/// Decode a source type + handle from syscall args into a `WaitSource`.
///
/// Source types: 0=channel, 1=`pipe_read`, 2=`pipe_write`, 3=eventfd,
/// 4=`process_exit`.
fn decode_wait_source(source_type: u64, handle: u64) -> Option<WaitSource> {
    match source_type {
        0 => Some(WaitSource::Channel(handle)),
        1 => Some(WaitSource::PipeRead(handle)),
        2 => Some(WaitSource::PipeWrite(handle)),
        3 => Some(WaitSource::EventFd(handle)),
        4 => Some(WaitSource::ProcessExit(handle)),
        5 => Some(WaitSource::Timer(handle)),
        6 => Some(WaitSource::Semaphore(handle)),
        7 => Some(WaitSource::IoCompletion(handle)),
        _ => None,
    }
}

/// `SYS_CP_CREATE` — create a completion port.
///
/// Returns: completion port handle.
pub fn sys_cp_create(args: &SyscallArgs) -> SyscallResult {
    let _ = args;
    let handle = completion::create();

    if let Some(pid) = caller_pid() {
        pcb::register_ipc_handle(pid, ResourceType::CompletionPort, handle.raw());
    }

    #[allow(clippy::cast_possible_wrap)]
    let h = handle.raw() as i64;
    SyscallResult::ok(h)
}

/// `SYS_CP_REGISTER` — register a source with a completion port.
///
/// `arg0`: CP handle.
/// `arg1`: source type (0-3).
/// `arg2`: source handle.
/// `arg3`: `user_data`.
pub fn sys_cp_register(args: &SyscallArgs) -> SyscallResult {
    let cp = CpHandle::from_raw(args.arg0);

    let Some(source) = decode_wait_source(args.arg1, args.arg2) else {
        return SyscallResult::err(KernelError::InvalidArgument);
    };

    match completion::register(cp, source, args.arg3) {
        Ok(()) => SyscallResult::ok(0),
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_CP_UNREGISTER` — unregister a source from a completion port.
///
/// `arg0`: CP handle.
/// `arg1`: source type.
/// `arg2`: source handle.
pub fn sys_cp_unregister(args: &SyscallArgs) -> SyscallResult {
    let cp = CpHandle::from_raw(args.arg0);

    let Some(source) = decode_wait_source(args.arg1, args.arg2) else {
        return SyscallResult::err(KernelError::InvalidArgument);
    };

    match completion::unregister(cp, source) {
        Ok(()) => SyscallResult::ok(0),
        Err(e) => SyscallResult::err(e),
    }
}

/// Raw event structure for the syscall boundary.
///
/// Matches the layout expected by userspace.  Each event is 24 bytes:
/// `source_type` (u64) + `source_handle` (u64) + `user_data` (u64).
#[repr(C)]
struct CpEventRaw {
    source_type: u64,
    source_handle: u64,
    user_data: u64,
}

/// Encode a `CompletionEvent` into the raw format.
fn encode_event(event: &completion::CompletionEvent) -> CpEventRaw {
    let (source_type, source_handle) = match event.source {
        WaitSource::Channel(h) => (0u64, h),
        WaitSource::PipeRead(h) => (1, h),
        WaitSource::PipeWrite(h) => (2, h),
        WaitSource::EventFd(h) => (3, h),
        WaitSource::ProcessExit(h) => (4, h),
        WaitSource::Timer(h) => (5, h),
        WaitSource::Semaphore(h) => (6, h),
        WaitSource::IoCompletion(h) => (7, h),
    };
    CpEventRaw {
        source_type,
        source_handle,
        user_data: event.user_data,
    }
}

/// Write events to the userspace buffer and return the count.
///
/// # Safety
///
/// `buf_ptr` must be valid for `buf_cap` `CpEventRaw` entries.
unsafe fn write_events_to_buffer(
    events: &[completion::CompletionEvent],
    buf_ptr: *mut CpEventRaw,
    buf_cap: usize,
) -> usize {
    let count = events.len().min(buf_cap);
    for (i, event) in events.iter().take(count).enumerate() {
        let raw = encode_event(event);
        // SAFETY: buf_ptr is valid for buf_cap entries, i < count <= buf_cap.
        unsafe {
            buf_ptr.add(i).write(raw);
        }
    }
    count
}

/// `SYS_CP_WAIT` — blocking wait for events.
///
/// `arg0`: CP handle.
/// `arg1`: pointer to event buffer.
/// `arg2`: buffer capacity (max events).
pub fn sys_cp_wait(args: &SyscallArgs) -> SyscallResult {
    let cp = CpHandle::from_raw(args.arg0);
    let buf_ptr = args.arg1 as *mut CpEventRaw;
    let buf_cap = args.arg2 as usize;

    if buf_ptr.is_null() && buf_cap > 0 {
        return SyscallResult::err(KernelError::InvalidArgument);
    }

    if buf_cap > 0 {
        // Each CpEventRaw is 24 bytes (3 × u64).
        let byte_len = buf_cap.checked_mul(core::mem::size_of::<CpEventRaw>())
            .ok_or(KernelError::InvalidArgument);
        match byte_len {
            Ok(n) => {
                if let Err(e) = crate::mm::user::validate_user_write(args.arg1, n) {
                    return SyscallResult::err(e);
                }
            }
            Err(e) => return SyscallResult::err(e),
        }
    }

    match completion::wait(cp) {
        Ok(events) => {
            // SAFETY: Validated above — buf_ptr is in user space, mapped, and writable.
            let count = unsafe { write_events_to_buffer(&events, buf_ptr, buf_cap) };
            #[allow(clippy::cast_possible_wrap)]
            let n = count as i64;
            SyscallResult::ok(n)
        }
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_CP_TRY_WAIT` — non-blocking poll for events.
///
/// Same arguments as `SYS_CP_WAIT`.
pub fn sys_cp_try_wait(args: &SyscallArgs) -> SyscallResult {
    let cp = CpHandle::from_raw(args.arg0);
    let buf_ptr = args.arg1 as *mut CpEventRaw;
    let buf_cap = args.arg2 as usize;

    if buf_ptr.is_null() && buf_cap > 0 {
        return SyscallResult::err(KernelError::InvalidArgument);
    }

    if buf_cap > 0 {
        let byte_len = buf_cap.checked_mul(core::mem::size_of::<CpEventRaw>())
            .ok_or(KernelError::InvalidArgument);
        match byte_len {
            Ok(n) => {
                if let Err(e) = crate::mm::user::validate_user_write(args.arg1, n) {
                    return SyscallResult::err(e);
                }
            }
            Err(e) => return SyscallResult::err(e),
        }
    }

    match completion::try_wait(cp) {
        Ok(events) => {
            // SAFETY: Validated above — buf_ptr is in user space, mapped, and writable.
            let count = unsafe { write_events_to_buffer(&events, buf_ptr, buf_cap) };
            #[allow(clippy::cast_possible_wrap)]
            let n = count as i64;
            SyscallResult::ok(n)
        }
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_CP_CLOSE` — close a completion port.
///
/// `arg0`: CP handle.
pub fn sys_cp_close(args: &SyscallArgs) -> SyscallResult {
    let cp = CpHandle::from_raw(args.arg0);
    if let Some(pid) = caller_pid() {
        pcb::deregister_ipc_handle(pid, ResourceType::CompletionPort, cp.raw());
    }
    completion::close(cp);
    SyscallResult::ok(0)
}

/// `SYS_CP_NOTIFY` — manually post a notification to a completion port.
///
/// `arg0`: CP handle.
/// `arg1`: source type.
/// `arg2`: source handle.
pub fn sys_cp_notify(args: &SyscallArgs) -> SyscallResult {
    let cp = CpHandle::from_raw(args.arg0);

    let Some(source) = decode_wait_source(args.arg1, args.arg2) else {
        return SyscallResult::err(KernelError::InvalidArgument);
    };

    completion::notify(cp, source);
    SyscallResult::ok(0)
}

// ---------------------------------------------------------------------------
// Process syscalls (500–599)
// ---------------------------------------------------------------------------

/// `SYS_PROCESS_SPAWN` — spawn a new process from an ELF binary.
///
/// `arg0`: pointer to ELF data in memory.
/// `arg1`: ELF data length.
/// `arg2`: pointer to name string (UTF-8).
/// `arg3`: name length.
///
/// Returns: process ID on success, negative error on failure.
///
/// # Note
///
/// In the final implementation, `arg0`/`arg1` will be a path in the
/// filesystem, not raw ELF data.  The kernel will open the file, read
/// the ELF, and load it.  For now (no filesystem), we accept raw bytes.
pub fn sys_process_spawn(args: &SyscallArgs) -> SyscallResult {
    use crate::proc::spawn::{SpawnOptions, spawn_process};

    let elf_ptr = args.arg0 as usize;
    let elf_len = args.arg1 as usize;
    let name_ptr = args.arg2 as usize;
    let name_len = args.arg3 as usize;

    if elf_len == 0 {
        return SyscallResult::err(KernelError::InvalidArgument);
    }

    // Validate ELF data pointer.
    if let Err(e) = crate::mm::user::validate_user_read(args.arg0, elf_len) {
        return SyscallResult::err(e);
    }

    // Validate name pointer (if provided).
    if name_len > 0 && name_ptr != 0 {
        if let Err(e) = crate::mm::user::validate_user_read(args.arg2, name_len) {
            return SyscallResult::err(e);
        }
    }

    // SAFETY: Validated above — elf_ptr is in user space and mapped.
    let elf_data = unsafe {
        core::slice::from_raw_parts(elf_ptr as *const u8, elf_len)
    };

    // Read the name.
    let name = if name_len > 0 && name_ptr != 0 {
        // SAFETY: Validated above — name_ptr is in user space and mapped.
        let name_bytes = unsafe {
            core::slice::from_raw_parts(name_ptr as *const u8, name_len)
        };
        core::str::from_utf8(name_bytes).unwrap_or("unnamed")
    } else {
        "unnamed"
    };

    let options = SpawnOptions::new(name);

    match spawn_process(elf_data, &options) {
        Ok(result) => {
            #[allow(clippy::cast_possible_wrap)]
            SyscallResult::ok(result.pid as i64)
        }
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_PROCESS_SPAWN_EX` — spawn a new process with extended options.
///
/// Accepts a pointer to a `SpawnExArgs` struct that bundles all spawn
/// parameters: ELF data, process name, fd map, argv, and envp.
///
/// `arg0`: pointer to `SpawnExArgs` struct in user memory.
///
/// Returns: process ID on success, negative error on failure.
pub fn sys_process_spawn_ex(args: &SyscallArgs) -> SyscallResult {
    use crate::proc::spawn::{FdMapEntry, SpawnExArgs, SpawnOptions, spawn_process};

    let args_ptr = args.arg0 as usize;

    // Validate the args struct pointer.
    let struct_size = core::mem::size_of::<SpawnExArgs>();
    if args_ptr == 0 {
        return SyscallResult::err(KernelError::InvalidArgument);
    }
    if let Err(e) = crate::mm::user::validate_user_read(args.arg0, struct_size) {
        return SyscallResult::err(e);
    }

    // SAFETY: Validated above — args_ptr points to mapped user memory.
    let spawn_args: SpawnExArgs = unsafe { *(args_ptr as *const SpawnExArgs) };

    let elf_ptr = spawn_args.elf_ptr as usize;
    let elf_len = spawn_args.elf_len as usize;
    let name_ptr = spawn_args.name_ptr as usize;
    let name_len = spawn_args.name_len as usize;
    let fd_map_ptr = spawn_args.fd_map_ptr as usize;
    let fd_map_count = spawn_args.fd_map_count as usize;
    let argv_ptr = spawn_args.argv_ptr as usize;
    let argv_len = spawn_args.argv_len as usize;
    let argc = spawn_args.argc as usize;
    let envp_ptr = spawn_args.envp_ptr as usize;
    let envp_len = spawn_args.envp_len as usize;
    let envc = spawn_args.envc as usize;

    if elf_len == 0 {
        return SyscallResult::err(KernelError::InvalidArgument);
    }

    // Validate ELF data pointer.
    if let Err(e) = crate::mm::user::validate_user_read(spawn_args.elf_ptr, elf_len) {
        return SyscallResult::err(e);
    }

    // Validate name pointer (if provided).
    if name_len > 0 && name_ptr != 0 {
        if let Err(e) = crate::mm::user::validate_user_read(spawn_args.name_ptr, name_len) {
            return SyscallResult::err(e);
        }
    }

    // Validate fd map pointer (if provided).
    if fd_map_count > 256 {
        return SyscallResult::err(KernelError::InvalidArgument);
    }
    let fd_map_byte_len = fd_map_count
        .checked_mul(core::mem::size_of::<FdMapEntry>())
        .unwrap_or(usize::MAX);
    if fd_map_count > 0 && fd_map_ptr != 0 {
        if let Err(e) = crate::mm::user::validate_user_read(spawn_args.fd_map_ptr, fd_map_byte_len) {
            return SyscallResult::err(e);
        }
    }

    // Validate argv pointer (if provided).
    if argv_len > 256 * 1024 {
        return SyscallResult::err(KernelError::InvalidArgument);
    }
    if argv_len > 0 && argv_ptr != 0 {
        if let Err(e) = crate::mm::user::validate_user_read(spawn_args.argv_ptr, argv_len) {
            return SyscallResult::err(e);
        }
    }

    // Validate envp pointer (if provided).
    if envp_len > 256 * 1024 {
        return SyscallResult::err(KernelError::InvalidArgument);
    }
    if envp_len > 0 && envp_ptr != 0 {
        if let Err(e) = crate::mm::user::validate_user_read(spawn_args.envp_ptr, envp_len) {
            return SyscallResult::err(e);
        }
    }

    // SAFETY: Validated above — elf_ptr is in user space and mapped.
    let elf_data = unsafe {
        core::slice::from_raw_parts(elf_ptr as *const u8, elf_len)
    };

    // Read the name.
    let name = if name_len > 0 && name_ptr != 0 {
        let name_bytes = unsafe {
            core::slice::from_raw_parts(name_ptr as *const u8, name_len)
        };
        core::str::from_utf8(name_bytes).unwrap_or("unnamed")
    } else {
        "unnamed"
    };

    // Read the fd map entries.
    let fd_pairs: alloc::vec::Vec<(i32, u8, u64)> = if fd_map_count > 0 && fd_map_ptr != 0 {
        let entries = unsafe {
            core::slice::from_raw_parts(fd_map_ptr as *const FdMapEntry, fd_map_count)
        };
        entries.iter().map(|e| (e.fd, e.handle_type, e.handle)).collect()
    } else {
        alloc::vec::Vec::new()
    };

    // Parse packed argv strings (null-terminated, concatenated).
    let argv_slices: alloc::vec::Vec<&[u8]> = if argc > 0 && argv_len > 0 && argv_ptr != 0 {
        let data = unsafe {
            core::slice::from_raw_parts(argv_ptr as *const u8, argv_len)
        };
        parse_packed_strings(data, argc)
    } else {
        alloc::vec::Vec::new()
    };

    // Parse packed envp strings.
    let envp_slices: alloc::vec::Vec<&[u8]> = if envc > 0 && envp_len > 0 && envp_ptr != 0 {
        let data = unsafe {
            core::slice::from_raw_parts(envp_ptr as *const u8, envp_len)
        };
        parse_packed_strings(data, envc)
    } else {
        alloc::vec::Vec::new()
    };

    let options = SpawnOptions::new(name)
        .fd_map(&fd_pairs)
        .argv(&argv_slices)
        .envp(&envp_slices);

    match spawn_process(elf_data, &options) {
        Ok(result) => {
            #[allow(clippy::cast_possible_wrap)]
            SyscallResult::ok(result.pid as i64)
        }
        Err(e) => SyscallResult::err(e),
    }
}

/// Parse packed null-terminated strings from a data buffer.
///
/// Returns up to `max_count` strings, splitting on null bytes.
fn parse_packed_strings(data: &[u8], max_count: usize) -> alloc::vec::Vec<&[u8]> {
    let mut result = alloc::vec::Vec::with_capacity(max_count);
    let mut start = 0;
    for (i, &b) in data.iter().enumerate() {
        if b == 0 {
            result.push(&data[start..i]);
            start = i + 1;
            if result.len() >= max_count {
                break;
            }
        }
    }
    // If the last string isn't null-terminated, include it.
    if start < data.len() && result.len() < max_count {
        result.push(&data[start..]);
    }
    result
}

/// `SYS_PROCESS_GET_INITIAL_FDS` — retrieve inherited fd mappings.
///
/// Called by the child process during startup to discover which file
/// descriptors were inherited from the parent.
///
/// `arg0`: pointer to output buffer (array of `FdMapEntry`).
/// `arg1`: capacity of the output buffer (in entries).
///
/// Returns: number of entries written, or negative error.
/// Entries are consumed (one-shot) — subsequent calls return 0.
pub fn sys_process_get_initial_fds(args: &SyscallArgs) -> SyscallResult {
    use crate::proc::spawn::FdMapEntry;
    use crate::proc::pcb;

    let out_ptr = args.arg0 as usize;
    let out_cap = args.arg1 as usize;

    // Get the calling process's PID.
    let pid = match caller_pid() {
        Some(p) => p,
        None => return SyscallResult::ok(0), // Kernel task — no fds.
    };

    // Take the initial fds from the PCB (one-shot: clears them).
    let fds = pcb::take_initial_fds(pid);

    if fds.is_empty() {
        return SyscallResult::ok(0);
    }

    // Clamp to output capacity.
    let count = fds.len().min(out_cap);

    if count > 0 && out_ptr != 0 {
        let byte_len = count
            .checked_mul(core::mem::size_of::<FdMapEntry>())
            .unwrap_or(usize::MAX);

        if let Err(e) = crate::mm::user::validate_user_write(args.arg0, byte_len) {
            // Put the fds back — caller can retry with a valid buffer.
            pcb::set_initial_fds(pid, fds);
            return SyscallResult::err(e);
        }

        // SAFETY: Validated above — out_ptr is writable user memory.
        let out_slice = unsafe {
            core::slice::from_raw_parts_mut(out_ptr as *mut FdMapEntry, count)
        };

        for (i, &(fd, handle_type, handle)) in fds.iter().take(count).enumerate() {
            if let Some(entry) = out_slice.get_mut(i) {
                *entry = FdMapEntry {
                    fd,
                    handle_type,
                    _pad: [0; 3],
                    handle,
                };
            }
        }

        // If we couldn't deliver all entries (output buffer too small),
        // put the remaining ones back.
        if count < fds.len() {
            let remaining: alloc::vec::Vec<(i32, u8, u64)> = fds[count..].to_vec();
            pcb::set_initial_fds(pid, remaining);
        }
    }

    #[allow(clippy::cast_possible_wrap)]
    SyscallResult::ok(count as i64)
}

/// `SYS_PROCESS_GET_ARGS` — retrieve initial argv/envp.
///
/// Called by the child during startup to read argv and envp data
/// that the parent passed via `SYS_PROCESS_SPAWN_EX`.
///
/// `arg0`: pointer to output buffer.
/// `arg1`: output buffer capacity (bytes).
///
/// Output format: `SpawnArgsHeader` (16 bytes) + packed argv strings
/// + packed envp strings.  Each string is null-terminated.
///
/// Returns: total bytes needed.  0 if no args were set.
pub fn sys_process_get_args(args: &SyscallArgs) -> SyscallResult {
    use crate::proc::spawn::SpawnArgsHeader;
    use crate::proc::pcb;

    let out_ptr = args.arg0 as usize;
    let out_cap = args.arg1 as usize;

    let pid = match caller_pid() {
        Some(p) => p,
        None => return SyscallResult::ok(0),
    };

    let (argv, envp) = pcb::take_initial_args(pid);

    if argv.is_empty() && envp.is_empty() {
        return SyscallResult::ok(0);
    }

    // Calculate total sizes.
    let argv_data_len: usize = argv.iter().map(|a| a.len().wrapping_add(1)).sum();
    let envp_data_len: usize = envp.iter().map(|e| e.len().wrapping_add(1)).sum();
    let header_size = core::mem::size_of::<SpawnArgsHeader>();
    let total_needed = header_size
        .saturating_add(argv_data_len)
        .saturating_add(envp_data_len);

    // If the caller's buffer is too small, put the data back and
    // return the required size so they can retry.
    if out_cap < total_needed || out_ptr == 0 {
        if let Err(_e) = pcb::set_initial_args(pid, argv, envp) {
            // Shouldn't happen — we just took these from the same PID.
        }
        #[allow(clippy::cast_possible_wrap)]
        return SyscallResult::ok(total_needed as i64);
    }

    // Validate output buffer.
    if let Err(e) = crate::mm::user::validate_user_write(args.arg0, total_needed) {
        if let Err(_e) = pcb::set_initial_args(pid, argv, envp) {}
        return SyscallResult::err(e);
    }

    // Write the header.
    // SAFETY: Validated above — out_ptr is writable user memory.
    let header_ptr = out_ptr as *mut SpawnArgsHeader;
    unsafe {
        (*header_ptr) = SpawnArgsHeader {
            #[allow(clippy::cast_possible_truncation)]
            argc: argv.len() as u32,
            #[allow(clippy::cast_possible_truncation)]
            envc: envp.len() as u32,
            #[allow(clippy::cast_possible_truncation)]
            argv_data_len: argv_data_len as u32,
            #[allow(clippy::cast_possible_truncation)]
            envp_data_len: envp_data_len as u32,
        };
    }

    // Write packed argv strings.
    let mut offset = out_ptr.wrapping_add(header_size);
    for arg in &argv {
        let dst = offset as *mut u8;
        // SAFETY: Within validated buffer.
        unsafe {
            core::ptr::copy_nonoverlapping(arg.as_ptr(), dst, arg.len());
            *dst.add(arg.len()) = 0; // Null terminator.
        }
        offset = offset.wrapping_add(arg.len().wrapping_add(1));
    }

    // Write packed envp strings.
    for env in &envp {
        let dst = offset as *mut u8;
        // SAFETY: Within validated buffer.
        unsafe {
            core::ptr::copy_nonoverlapping(env.as_ptr(), dst, env.len());
            *dst.add(env.len()) = 0; // Null terminator.
        }
        offset = offset.wrapping_add(env.len().wrapping_add(1));
    }

    #[allow(clippy::cast_possible_wrap)]
    SyscallResult::ok(total_needed as i64)
}

/// `SYS_PROCESS_WAIT` — wait for a child process to exit.
///
/// `arg0`: child process ID.
///
/// Returns: exit code on success, negative error on failure.
///
/// If the child is still running, blocks the calling task until the
/// child exits.
pub fn sys_process_wait(args: &SyscallArgs) -> SyscallResult {
    use crate::proc::pcb;

    let child_pid = args.arg0;
    // Resolve the calling process's PID so try_reap can verify the
    // parent–child relationship.  Falls back to 0 (kernel) for bare
    // kernel tasks, which matches processes spawned with parent=0.
    let parent_pid = caller_pid().unwrap_or(0);

    // Try to reap immediately.
    match pcb::try_reap(parent_pid, child_pid) {
        Ok(Some(info)) => {
            #[allow(clippy::cast_possible_wrap)]
            return SyscallResult::ok(info.exit_code as i64);
        }
        Ok(None) => {
            // Child still running — register wait and block.
            let task_id = sched::current_task_id();
            if let Err(e) = pcb::set_wait_task(child_pid, task_id) {
                return SyscallResult::err(e);
            }
            sched::block_current();

            // Woken up — try to reap again.
            match pcb::try_reap(parent_pid, child_pid) {
                Ok(Some(info)) => {
                    #[allow(clippy::cast_possible_wrap)]
                    SyscallResult::ok(info.exit_code as i64)
                }
                Ok(None) => {
                    // Shouldn't happen — we were woken because it became
                    // a zombie.  Return WouldBlock defensively.
                    SyscallResult::err(KernelError::WouldBlock)
                }
                Err(e) => SyscallResult::err(e),
            }
        }
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_PROCESS_TRY_WAIT` — non-blocking wait for a child process.
///
/// Like `SYS_PROCESS_WAIT` but returns `WouldBlock` immediately if
/// the child is still running, instead of blocking the caller.
pub fn sys_process_try_wait(args: &SyscallArgs) -> SyscallResult {
    use crate::proc::pcb;

    let child_pid = args.arg0;
    // Resolve calling process PID for parent–child verification.
    let parent_pid = caller_pid().unwrap_or(0);

    match pcb::try_reap(parent_pid, child_pid) {
        Ok(Some(info)) => {
            #[allow(clippy::cast_possible_wrap)]
            SyscallResult::ok(info.exit_code as i64)
        }
        Ok(None) => {
            // Child still running — return WouldBlock.
            SyscallResult::err(KernelError::WouldBlock)
        }
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_NOTIFY_READY` — signal that the calling process is initialized.
///
/// Services call this after completing startup.  The init service
/// manager can query this flag to implement dependency ordering.
///
/// Returns: 0 on success.
pub fn sys_notify_ready(args: &SyscallArgs) -> SyscallResult {
    let _ = args;
    use crate::proc::{pcb, thread};

    let task_id = sched::current_task_id();
    let pid = thread::owner_process(task_id).unwrap_or(0);

    if pid == 0 {
        return SyscallResult::err(KernelError::InvalidArgument);
    }

    match pcb::set_ready(pid) {
        Ok(()) => SyscallResult::ok(0),
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_PROCESS_IS_READY` — query whether a process is ready.
///
/// `arg0`: process ID to query.
///
/// Returns: 1 if ready, 0 if not yet, negative error on failure.
pub fn sys_process_is_ready(args: &SyscallArgs) -> SyscallResult {
    use crate::proc::pcb;

    let pid = args.arg0;

    match pcb::is_ready(pid) {
        Ok(true) => SyscallResult::ok(1),
        Ok(false) => SyscallResult::ok(0),
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_PROCESS_CRASH_INFO` — retrieve crash details for a zombie process.
///
/// `arg0`: child PID.
/// `arg1`: pointer to a 4×u64 output buffer in userspace.
///
/// If the process crashed (unhandled exception), writes the crash info
/// into the buffer and returns 1.  If the process exited normally,
/// returns 0.  The buffer layout:
///   [0] exception_code (1=DivideError, 8=AccessViolation, etc.)
///   [1] faulting_rip
///   [2] aux (page fault address, GP fault error code, etc.)
///   [3] thread_id that caused the crash
///
/// Must be called before reaping (try_wait returns Some → process is
/// removed from the process table).
pub fn sys_process_crash_info(args: &SyscallArgs) -> SyscallResult {
    use crate::proc::pcb;

    let child_pid = args.arg0;
    let buf_ptr = args.arg1;

    // Validate the buffer pointer.
    if buf_ptr == 0 {
        return SyscallResult::err(KernelError::InvalidArgument);
    }

    match pcb::get_crash_info(child_pid) {
        Some(info) => {
            // Write crash info to userspace buffer.
            // SAFETY: validated non-null, and the caller is responsible
            // for providing a writable 32-byte buffer.  In kernel mode
            // (current state), this is always valid kernel memory.
            let buf = buf_ptr as *mut u64;
            unsafe {
                buf.write(info.exception_code);
                buf.add(1).write(info.faulting_rip);
                buf.add(2).write(info.aux);
                buf.add(3).write(info.thread_id);
            }
            SyscallResult::ok(1)
        }
        None => SyscallResult::ok(0), // Normal exit, no crash info.
    }
}

/// `SYS_PROCESS_ID` — get the current process ID.
///
/// Returns: the calling process's PID, or 0 if the task isn't
/// associated with a process.
pub fn sys_process_id(args: &SyscallArgs) -> SyscallResult {
    let _ = args;
    use crate::proc::thread;

    let task_id = sched::current_task_id();
    let pid = thread::owner_process(task_id).unwrap_or(0);

    #[allow(clippy::cast_possible_wrap)]
    SyscallResult::ok(pid as i64)
}

/// `SYS_CAP_QUERY` — query the calling process's capabilities.
///
/// Returns the number of valid capabilities held by the calling process.
///
/// This is a simple count query.  A future extension will support
/// filling a user-space buffer with detailed capability entries.
pub fn sys_cap_query(args: &SyscallArgs) -> SyscallResult {
    let _ = args;
    use crate::proc::{pcb, thread};

    let task_id = sched::current_task_id();
    let pid = thread::owner_process(task_id).unwrap_or(0);

    // PID 0 (kernel) has no per-process cap table.
    if pid == 0 {
        return SyscallResult::ok(0);
    }

    let count = pcb::cap_count(pid).unwrap_or(0);

    #[allow(clippy::cast_possible_wrap)]
    SyscallResult::ok(count as i64)
}

/// `SYS_CAP_REQUEST` — request a capability the caller does not hold.
///
/// Submits a request to the security policy handler.  The request
/// includes a human-readable reason string that will be presented to
/// the user for approval or denial.
///
/// `arg0`: resource type (`ResourceType` discriminant as u16).
/// `arg1`: rights bitfield (`Rights` bits as u32).
/// `arg2`: pointer to reason string (UTF-8, user buffer).
/// `arg3`: length of reason string in bytes (max 256).
///
/// Returns: request ID (positive u64) on success.
pub fn sys_cap_request(args: &SyscallArgs) -> SyscallResult {
    use crate::cap::{self, request, Rights};
    use crate::proc::{pcb, thread};

    let resource_type_raw = args.arg0 as u16;
    let rights_raw = args.arg1 as u32;
    let reason_ptr = args.arg2 as *const u8;
    let reason_len = args.arg3 as usize;

    // Validate resource type.
    let resource_type = match resource_type_raw {
        1 => cap::ResourceType::Channel,
        2 => cap::ResourceType::Pipe,
        3 => cap::ResourceType::SharedMemory,
        4 => cap::ResourceType::EventFd,
        5 => cap::ResourceType::CompletionPort,
        6 => cap::ResourceType::Process,
        7 => cap::ResourceType::Thread,
        8 => cap::ResourceType::PortIo,
        9 => cap::ResourceType::DeviceIrq,
        10 => cap::ResourceType::File,
        11 => cap::ResourceType::Socket,
        12 => cap::ResourceType::Timer,
        13 => cap::ResourceType::IoScheduler,
        14 => cap::ResourceType::Service,
        15 => cap::ResourceType::Namespace,
        _ => return SyscallResult::err(KernelError::InvalidArgument),
    };

    // Validate rights (must be non-zero).
    let rights = Rights::from_raw(rights_raw as u64);
    if rights.is_empty() {
        return SyscallResult::err(KernelError::InvalidArgument);
    }

    // Validate reason string.
    if reason_ptr.is_null() || reason_len == 0 {
        return SyscallResult::err(KernelError::InvalidArgument);
    }
    let safe_len = reason_len.min(256);

    if let Err(e) = crate::mm::user::validate_user_read(args.arg2, safe_len) {
        return SyscallResult::err(e);
    }

    // SAFETY: Buffer validated above — in user space and mapped.
    let reason_bytes = unsafe { core::slice::from_raw_parts(reason_ptr, safe_len) };
    let reason_str = match core::str::from_utf8(reason_bytes) {
        Ok(s) => s,
        Err(_) => return SyscallResult::err(KernelError::InvalidArgument),
    };

    // Get the calling process's PID and name.
    let task_id = sched::current_task_id();
    let pid = match thread::owner_process(task_id) {
        Some(pid) if pid != 0 => pid,
        _ => return SyscallResult::err(KernelError::PermissionDenied),
    };
    let proc_name = pcb::name(pid).unwrap_or_else(|| alloc::string::String::from("unknown"));

    // Submit the request.
    match request::request_capability(pid, &proc_name, resource_type, rights, reason_str) {
        Ok(id) => {
            #[allow(clippy::cast_possible_wrap)]
            SyscallResult::ok(id as i64)
        }
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_CAP_REQUEST_STATUS` — check the status of a capability request.
///
/// `arg0`: request ID (from `SYS_CAP_REQUEST`).
///
/// Returns status as an integer:
/// - 0 = Pending
/// - 1 = Approved
/// - 2 = Denied
/// - 3 = TimedOut
/// - 4 = Cancelled
pub fn sys_cap_request_status(args: &SyscallArgs) -> SyscallResult {
    use crate::cap::request::{self, RequestStatus};

    let request_id = args.arg0;

    match request::get_status(request_id) {
        Some(status) => {
            let code = match status {
                RequestStatus::Pending => 0,
                RequestStatus::Approved => 1,
                RequestStatus::Denied => 2,
                RequestStatus::TimedOut => 3,
                RequestStatus::Cancelled => 4,
            };
            SyscallResult::ok(code)
        }
        None => SyscallResult::err(KernelError::NotFound),
    }
}

/// `SYS_CAP_REQUEST_CANCEL` — cancel a pending capability request.
///
/// Only the process that submitted the request can cancel it.
///
/// `arg0`: request ID (from `SYS_CAP_REQUEST`).
///
/// Returns 0 on success.
pub fn sys_cap_request_cancel(args: &SyscallArgs) -> SyscallResult {
    use crate::cap::request;
    use crate::proc::thread;

    let request_id = args.arg0;

    // Get the calling process's PID.
    let task_id = sched::current_task_id();
    let pid = match thread::owner_process(task_id) {
        Some(pid) if pid != 0 => pid,
        _ => return SyscallResult::err(KernelError::PermissionDenied),
    };

    match request::cancel(request_id, pid) {
        Ok(()) => SyscallResult::ok(0),
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_SET_EXCEPTION_HANDLER` — register a per-process exception handler.
///
/// `arg0`: handler function address, or 0 to unregister.
pub fn sys_set_exception_handler(args: &SyscallArgs) -> SyscallResult {
    use crate::proc::{exception, thread};

    let handler_addr = args.arg0;
    let task_id = sched::current_task_id();

    let pid = match thread::owner_process(task_id) {
        Some(pid) if pid != 0 => pid,
        _ => {
            return SyscallResult::err(KernelError::NoSuchProcess);
        }
    };

    exception::set_handler(pid, handler_addr);
    SyscallResult::ok(0)
}

/// `SYS_EXCEPTION_RETURN` — resume from an exception handler.
///
/// `arg0`: pointer to the `ExceptionContext` on the user stack.
///
/// Restores the saved CPU state and resumes execution.  This syscall
/// does NOT return to the caller — it modifies the SYSRET frame to
/// jump to the context's saved RIP.
///
/// Since this needs to modify the syscall frame, it's handled as a
/// special case in `syscall_handler_inner` (like exec).
pub fn sys_exception_return_with_frame(
    frame: &mut super::entry::SyscallFrame,
) -> i64 {
    use crate::proc::exception::ExceptionContext;

    let ctx_ptr = frame.arg0 as *const ExceptionContext;

    // Validate that the exception context is in user space and mapped.
    // The context is sizeof(ExceptionContext) = 160 bytes on the user stack.
    if let Err(e) = crate::mm::user::validate_user_read(
        frame.arg0,
        core::mem::size_of::<ExceptionContext>(),
    ) {
        return e.code() as i64;
    }

    // SAFETY: Validated above — ctx_ptr is in user space and mapped.
    // Originally written by the kernel's exception dispatch onto the
    // user stack; the handler may have modified fields.
    let ctx = unsafe { &*ctx_ptr };

    // Restore the SYSRET frame from the exception context.
    frame.user_rip = ctx.rip;
    frame.user_rsp = ctx.rsp;
    frame.user_rflags = ctx.rflags;
    frame.arg0 = ctx.rdi;  // rdi
    frame.arg1 = ctx.rsi;  // rsi
    frame.arg2 = ctx.rdx;  // rdx
    frame.arg3 = ctx.r10;  // r10
    frame.arg4 = ctx.r8;   // r8
    frame.arg5 = ctx.r9;   // r9
    frame.rbx = ctx.rbx;
    frame.rbp = ctx.rbp;
    frame.r12 = ctx.r12;
    frame.r13 = ctx.r13;
    frame.r14 = ctx.r14;
    frame.r15 = ctx.r15;

    serial_println!(
        "[exception] Returning from exception handler to {:#x}",
        ctx.rip
    );

    // Return value in RAX (not meaningful — the restored rax from
    // the context won't be used since we're restoring from the frame).
    ctx.rax as i64
}

/// `SYS_PROCESS_KILL` — force-terminate a process.
///
/// `arg0`: target process ID.
/// `arg1`: exit code (i32, sign-extended to u64).
///
/// Authority: the caller must be the parent of the target, or PID 0.
/// Cannot kill PID 0 (kernel) or the calling process itself (use
/// SYS_EXIT instead).
///
/// Returns: number of threads killed.
pub fn sys_process_kill(args: &super::dispatch::SyscallArgs) -> super::dispatch::SyscallResult {
    use crate::proc::{pcb, thread};
    use super::dispatch::SyscallResult;

    let target_pid = args.arg0;
    #[allow(clippy::cast_possible_wrap)]
    let exit_code = args.arg1 as i32;

    // Can't kill PID 0 (kernel).
    if target_pid == 0 {
        return SyscallResult::err(KernelError::InvalidArgument);
    }

    // Get the caller's process ID.
    let task_id = sched::current_task_id();
    let caller_pid = thread::owner_process(task_id).unwrap_or(0);

    // Can't kill self — use SYS_EXIT instead.
    if target_pid == caller_pid {
        return SyscallResult::err(KernelError::InvalidArgument);
    }

    // Authority check: caller must be the target's parent, PID 0,
    // or hold a Process capability with DELETE rights for the target.
    let target_parent = match pcb::parent(target_pid) {
        Some(p) => p,
        None => return SyscallResult::err(KernelError::NoSuchProcess),
    };
    let has_parent_auth = caller_pid == 0 || caller_pid == target_parent;
    let has_cap_auth = pcb::has_capability_for(
        caller_pid,
        crate::cap::ResourceType::Process,
        target_pid,
        crate::cap::Rights::DELETE,
    );
    if !has_parent_auth && !has_cap_auth {
        return SyscallResult::err(KernelError::PermissionDenied);
    }

    // Check the process isn't already a zombie or gone.
    match pcb::state(target_pid) {
        Some(pcb::ProcessState::Zombie) => {
            return SyscallResult::err(KernelError::ProcessExited);
        }
        None => {
            return SyscallResult::err(KernelError::NoSuchProcess);
        }
        _ => {}
    }

    // Set the exit code before killing threads so the zombie
    // transition has the correct code.
    if let Err(e) = pcb::set_exit_code(target_pid, exit_code) {
        return SyscallResult::err(e);
    }

    // Kill all threads in the target process.
    let killed = thread::kill_process_threads(target_pid);

    serial_println!(
        "[proc] Process {} killed by {} ({} threads, exit_code={})",
        target_pid, caller_pid, killed, exit_code
    );

    #[allow(clippy::cast_possible_wrap)]
    SyscallResult::ok(killed as i64)
}

/// `SYS_PROCESS_EXEC` — replace the current process image.
///
/// This handler receives the full `SyscallFrame` (not just args) because
/// on success it must rewrite the saved user RIP and RSP so that when
/// the SYSRET assembly path runs, it jumps to the new binary's entry
/// point with a fresh stack.
///
/// `frame.arg0`: pointer to ELF data in user memory.
/// `frame.arg1`: length of the ELF data (bytes).
/// `frame.arg2`: pointer to packed argv data (0 = no args).
/// `frame.arg3`: total byte length of packed argv data.
/// `frame.arg4`: pointer to packed envp data (0 = no env).
/// `frame.arg5`: total byte length of packed envp data.
///
/// Argv/envp are copied into kernel buffers before the old address
/// space is torn down, then stored in the PCB for the new binary to
/// read via `SYS_PROCESS_GET_ARGS`.
///
/// On success: returns 0 in RAX, with user_rip and user_rsp in the
/// frame modified to point at the new binary.  All other saved
/// registers are zeroed (clean slate for the new binary).
///
/// On failure: returns a negative error code.  If the failure happens
/// after the old address space was torn down, the process is in a
/// broken state and should be killed.
pub fn sys_process_exec_with_frame(
    frame: &mut super::entry::SyscallFrame,
) -> i64 {
    use crate::proc::spawn::exec_process;
    use crate::proc::thread;

    let elf_ptr = frame.arg0 as usize;
    let elf_len = frame.arg1 as usize;
    let argv_ptr = frame.arg2 as usize;
    let argv_len = frame.arg3 as usize;
    let envp_ptr = frame.arg4 as usize;
    let envp_len = frame.arg5 as usize;

    // Validate arguments.
    if elf_len == 0 {
        return KernelError::InvalidArgument.code() as i64;
    }

    // Get the calling process's PID.
    let task_id = sched::current_task_id();
    let pid = match thread::owner_process(task_id) {
        Some(pid) if pid != 0 => pid,
        _ => {
            serial_println!("[exec] Task {} has no owning process", task_id);
            return KernelError::NoSuchProcess.code() as i64;
        }
    };

    // Validate the ELF data pointer before reading.
    if let Err(e) = crate::mm::user::validate_user_read(frame.arg0, elf_len) {
        return e.code() as i64;
    }

    // Validate argv pointer (if provided).
    const MAX_PACKED_BYTES: usize = 256 * 1024;
    if argv_len > MAX_PACKED_BYTES {
        return KernelError::InvalidArgument.code() as i64;
    }
    if argv_len > 0 && argv_ptr != 0 {
        if let Err(e) = crate::mm::user::validate_user_read(frame.arg2, argv_len) {
            return e.code() as i64;
        }
    }

    // Validate envp pointer (if provided).
    if envp_len > MAX_PACKED_BYTES {
        return KernelError::InvalidArgument.code() as i64;
    }
    if envp_len > 0 && envp_ptr != 0 {
        if let Err(e) = crate::mm::user::validate_user_read(frame.arg4, envp_len) {
            return e.code() as i64;
        }
    }

    // Read the ELF data from userspace.
    //
    // SAFETY: Validated above — elf_ptr is in user space and mapped.
    // We copy into a kernel buffer before tearing down the address space.
    let elf_data = unsafe {
        core::slice::from_raw_parts(elf_ptr as *const u8, elf_len)
    };

    // We need to copy the ELF data into a kernel buffer BEFORE we
    // tear down the user address space (which would unmap the source).
    let elf_copy = alloc::vec::Vec::from(elf_data);

    // Copy argv/envp into kernel buffers before address space teardown.
    let argv_copy = if argv_len > 0 && argv_ptr != 0 {
        // SAFETY: Validated above — argv_ptr is mapped user memory.
        let data = unsafe {
            core::slice::from_raw_parts(argv_ptr as *const u8, argv_len)
        };
        alloc::vec::Vec::from(data)
    } else {
        alloc::vec::Vec::new()
    };

    let envp_copy = if envp_len > 0 && envp_ptr != 0 {
        // SAFETY: Validated above — envp_ptr is mapped user memory.
        let data = unsafe {
            core::slice::from_raw_parts(envp_ptr as *const u8, envp_len)
        };
        alloc::vec::Vec::from(data)
    } else {
        alloc::vec::Vec::new()
    };

    // Parse the copied packed strings into slices.
    // Count is derived from the packed data (count null terminators).
    let argv_slices: alloc::vec::Vec<&[u8]> = if !argv_copy.is_empty() {
        parse_packed_strings(&argv_copy, usize::MAX)
    } else {
        alloc::vec::Vec::new()
    };
    let envp_slices: alloc::vec::Vec<&[u8]> = if !envp_copy.is_empty() {
        parse_packed_strings(&envp_copy, usize::MAX)
    } else {
        alloc::vec::Vec::new()
    };

    // Exec: validate ELF, tear down old AS, load new AS, set up stack,
    // store argv/envp.
    match exec_process(pid, &elf_copy, &argv_slices, &envp_slices) {
        Ok(result) => {
            // Success: rewrite the saved frame so SYSRET returns to the
            // new entry point with a fresh stack and clean registers.
            frame.user_rip = result.entry_rip;
            frame.user_rsp = result.user_rsp;

            // Zero all saved general-purpose registers — the new binary
            // starts with a clean slate.
            frame.arg0 = 0;  // rdi
            frame.arg1 = 0;  // rsi
            frame.arg2 = 0;  // rdx
            frame.arg3 = 0;  // r10
            frame.arg4 = 0;  // r8
            frame.arg5 = 0;  // r9
            frame.rbx = 0;
            frame.rbp = 0;
            frame.r12 = 0;
            frame.r13 = 0;
            frame.r14 = 0;
            frame.r15 = 0;

            // RFLAGS: keep IF=1 (interrupts enabled), reserved bit 1.
            // Clear everything else (DF, TF, etc.).
            frame.user_rflags = 0x202;

            serial_println!(
                "[exec] Process {} exec successful — returning to {:#x}",
                pid, result.entry_rip
            );
            0 // Success (returned in RAX).
        }
        Err(e) => {
            serial_println!(
                "[exec] Process {} exec failed: {:?} — process may be broken",
                pid, e
            );
            e.code() as i64
        }
    }
}

// ---------------------------------------------------------------------------
// Time and sleep handlers (10–19)
// ---------------------------------------------------------------------------

/// `SYS_CLOCK_MONOTONIC` — get monotonic time since boot in nanoseconds.
///
/// Uses the best available hardware clock source:
/// 1. HPET (High Precision Event Timer) — ~10 ns resolution, hardware counter
/// 2. TSC (Time Stamp Counter) — sub-microsecond, needs calibration
/// 3. APIC tick count — 10 ms resolution (fallback only)
///
/// The hrtimer subsystem's `now_ns()` already implements this priority
/// chain, so we delegate to it for consistency with kernel-internal
/// timing (sleep_ns, wait_timeout_ns, etc.).
pub fn sys_clock_monotonic(args: &SyscallArgs) -> SyscallResult {
    let _ = args;

    let ns = crate::hrtimer::now_ns();

    #[allow(clippy::cast_possible_wrap)]
    SyscallResult::ok(ns as i64)
}

/// `SYS_SLEEP` — sleep for a specified duration in nanoseconds.
///
/// `arg0`: duration in nanoseconds.
///
/// Blocks the calling task for the specified duration.  Uses the
/// high-resolution timer subsystem (HPET-backed hrtimer) for sleeps
/// ≤ 100ms, falling back to tick-based sleep for longer durations.
/// Typical precision: ~1-4 ms scheduling latency on top of the
/// requested duration.
///
/// Returns: 0 on success.
pub fn sys_sleep(args: &SyscallArgs) -> SyscallResult {
    let duration_ns = args.arg0;

    if duration_ns == 0 {
        // Zero sleep → just yield.
        sched::yield_now();
        return SyscallResult::ok(0);
    }

    // Delegate to sleep_ns which automatically selects the best path:
    // - hrtimer for ≤ 100ms (nanosecond-precision wake via HPET)
    // - tick-based for > 100ms (efficient, avoids hrtimer slot pressure)
    sched::sleep_ns(duration_ns);

    SyscallResult::ok(0)
}

// ---------------------------------------------------------------------------
// Timer handlers (12–13)
// ---------------------------------------------------------------------------

/// `SYS_TIMER_CREATE` — create a kernel timer.
///
/// `arg0`: duration in nanoseconds.
/// `arg1`: flags (bit 0 = periodic).
///
/// Returns: timer handle (> 0) on success, 0 on failure.
pub fn sys_timer_create(args: &SyscallArgs) -> SyscallResult {
    let duration_ns = args.arg0;
    let flags = args.arg1;

    if duration_ns == 0 {
        return SyscallResult::err(KernelError::InvalidArgument);
    }

    match crate::ipc::timer::create(duration_ns, flags) {
        Ok(handle) => {
            if let Some(pid) = caller_pid() {
                pcb::register_ipc_handle(pid, ResourceType::Timer, handle);
            }
            #[allow(clippy::cast_possible_wrap)]
            SyscallResult::ok(handle as i64)
        }
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_TIMER_CANCEL` — cancel and destroy a timer.
///
/// `arg0`: timer handle.
///
/// Returns: 0 on success, negative error if not found.
pub fn sys_timer_cancel(args: &SyscallArgs) -> SyscallResult {
    let handle = args.arg0;

    if handle == 0 {
        return SyscallResult::err(KernelError::InvalidArgument);
    }

    if let Some(pid) = caller_pid() {
        pcb::deregister_ipc_handle(pid, ResourceType::Timer, handle);
    }
    if crate::ipc::timer::cancel(handle) {
        SyscallResult::ok(0)
    } else {
        SyscallResult::err(KernelError::InvalidHandle)
    }
}

// ---------------------------------------------------------------------------
// Console I/O handlers (100–109)
// ---------------------------------------------------------------------------

/// `SYS_CONSOLE_WRITE` — write bytes to the framebuffer console.
///
/// Handles ASCII control characters (`\n`, `\r`, `\t`).
/// Also mirrors to serial output via `console::write_str`.
pub fn sys_console_write(args: &SyscallArgs) -> SyscallResult {
    let ptr = args.arg0 as *const u8;
    let len = args.arg1 as usize;

    if ptr.is_null() || len == 0 {
        return SyscallResult::ok(0);
    }

    // Cap length to prevent excessive output in a single syscall.
    let safe_len = len.min(4096);

    // Validate the user buffer is in user space and mapped.
    if let Err(e) = crate::mm::user::validate_user_read(args.arg0, safe_len) {
        return SyscallResult::err(e);
    }

    // SAFETY: Buffer validated above — in user space and mapped.
    let bytes = unsafe { core::slice::from_raw_parts(ptr, safe_len) };

    // Use write_str when possible — it writes to both framebuffer
    // and serial.  For non-UTF8 data, write bytes individually.
    if let Ok(s) = core::str::from_utf8(bytes) {
        crate::console::write_str(s);
    } else {
        // Non-UTF8: write each byte to framebuffer (putchar) and
        // serial (via serial_print).
        for &b in bytes {
            crate::console::putchar(b);
        }
        // Mirror to serial.
        crate::serial_print!("<{} bytes>", safe_len);
    }

    #[allow(clippy::cast_possible_wrap)]
    SyscallResult::ok(safe_len as i64)
}

/// `SYS_CONSOLE_READ_CHAR` — read one character from the keyboard.
///
/// Blocks (via HLT) until a key is pressed.  Returns the ASCII code
/// in the single-byte buffer pointed to by `arg0`.
pub fn sys_console_read_char(args: &SyscallArgs) -> SyscallResult {
    let ptr = args.arg0 as *mut u8;

    if ptr.is_null() {
        return SyscallResult::err(KernelError::InvalidArgument);
    }

    // Validate the output byte is in user space and writable.
    if let Err(e) = crate::mm::user::validate_user_write(args.arg0, 1) {
        return SyscallResult::err(e);
    }

    // Block until a key is available.
    let ch = crate::keyboard::read_char();

    // SAFETY: Pointer validated above — in user space, mapped, writable.
    unsafe { core::ptr::write(ptr, ch); }

    SyscallResult::ok(1)
}

/// `SYS_CONSOLE_TRY_READ_CHAR` — non-blocking read of one keyboard character.
///
/// If a keypress is buffered, writes the ASCII code to the output byte
/// and returns 1.  Otherwise returns `WouldBlock` immediately.
pub fn sys_console_try_read_char(args: &SyscallArgs) -> SyscallResult {
    let ptr = args.arg0 as *mut u8;

    if ptr.is_null() {
        return SyscallResult::err(KernelError::InvalidArgument);
    }

    // Validate the output byte is in user space and writable.
    if let Err(e) = crate::mm::user::validate_user_write(args.arg0, 1) {
        return SyscallResult::err(e);
    }

    match crate::keyboard::try_read_char() {
        Some(ch) => {
            // SAFETY: Pointer validated above — in user space, mapped, writable.
            unsafe { core::ptr::write(ptr, ch); }
            SyscallResult::ok(1)
        }
        None => SyscallResult::err(KernelError::WouldBlock),
    }
}

// ---------------------------------------------------------------------------
// Logging handlers (102)
// ---------------------------------------------------------------------------

/// `SYS_LOG_READ` — read kernel log entries from the ring buffer.
///
/// Returns JSON-lines in the output buffer.  Each entry is a single
/// JSON object followed by `\n`.
///
/// `arg0`: after_seq — read entries newer than this sequence number.
///         Pass `u64::MAX` to start from the oldest available.
/// `arg1`: pointer to output buffer.
/// `arg2`: buffer capacity in bytes.
///
/// Returns: entry count in `value`, newest sequence in `value2`.
pub fn sys_log_read(args: &SyscallArgs) -> SyscallResult {
    let after_seq = args.arg0;
    let buf_ptr = args.arg1 as *mut u8;
    let buf_cap = args.arg2 as usize;

    if buf_ptr.is_null() || buf_cap == 0 {
        return SyscallResult::err(KernelError::InvalidArgument);
    }

    // Validate the output buffer.
    if let Err(e) = crate::mm::user::validate_user_write(args.arg1, buf_cap) {
        return SyscallResult::err(e);
    }

    // SAFETY: Validated above — buf_ptr is in user space, mapped, writable.
    let buf = unsafe { core::slice::from_raw_parts_mut(buf_ptr, buf_cap) };

    let (count, newest_seq) = crate::klog::read_logs(after_seq, buf);

    #[allow(clippy::cast_possible_wrap)]
    SyscallResult::ok2(count as i64, newest_seq as i64)
}

// ---------------------------------------------------------------------------
// Filesystem handlers (600–799)
// ---------------------------------------------------------------------------

/// `SYS_FS_READ_FILE` — read an entire file into a buffer.
pub fn sys_fs_read_file(args: &SyscallArgs) -> SyscallResult {
    // Capability check: requires File capability with READ rights.
    if let Err(e) = require_cap_type(
        crate::cap::ResourceType::File,
        crate::cap::Rights::READ,
    ) {
        return SyscallResult::err(e);
    }

    let path_ptr = args.arg0 as *const u8;
    let path_len = args.arg1 as usize;
    let buf_ptr = args.arg2 as *mut u8;
    let buf_cap = args.arg3 as usize;

    if path_ptr.is_null() || path_len == 0 || buf_ptr.is_null() || buf_cap == 0 {
        return SyscallResult::err(KernelError::InvalidArgument);
    }

    let safe_path_len = path_len.min(256);
    if let Err(e) = crate::mm::user::validate_user_read(args.arg0, safe_path_len) {
        return SyscallResult::err(e);
    }
    if let Err(e) = crate::mm::user::validate_user_write(args.arg2, buf_cap) {
        return SyscallResult::err(e);
    }

    // SAFETY: Validated above — path_ptr is in user space and mapped.
    let path_bytes = unsafe { core::slice::from_raw_parts(path_ptr, safe_path_len) };
    let path = match core::str::from_utf8(path_bytes) {
        Ok(s) => s,
        Err(_) => return SyscallResult::err(KernelError::InvalidArgument),
    };

    // Read the file via the VFS.
    let data = match crate::fs::Vfs::read_file(path) {
        Ok(d) => d,
        Err(e) => return SyscallResult::err(e),
    };

    // Copy to the user buffer (up to capacity).
    let copy_len = data.len().min(buf_cap);
    // SAFETY: Validated above — buf_ptr is in user space, mapped, and writable.
    unsafe {
        core::ptr::copy_nonoverlapping(data.as_ptr(), buf_ptr, copy_len);
    }

    #[allow(clippy::cast_possible_wrap)]
    SyscallResult::ok(copy_len as i64)
}

/// `SYS_FS_WRITE_FILE` — write data to a file (create or overwrite).
pub fn sys_fs_write_file(args: &SyscallArgs) -> SyscallResult {
    // Capability check: requires File capability with WRITE rights.
    if let Err(e) = require_cap_type(
        crate::cap::ResourceType::File,
        crate::cap::Rights::WRITE,
    ) {
        return SyscallResult::err(e);
    }

    let path_ptr = args.arg0 as *const u8;
    let path_len = args.arg1 as usize;
    let data_ptr = args.arg2 as *const u8;
    let data_len = args.arg3 as usize;

    if path_ptr.is_null() || path_len == 0 || data_ptr.is_null() {
        return SyscallResult::err(KernelError::InvalidArgument);
    }

    let safe_path_len = path_len.min(256);
    if let Err(e) = crate::mm::user::validate_user_read(args.arg0, safe_path_len) {
        return SyscallResult::err(e);
    }
    if data_len > 0 {
        if let Err(e) = crate::mm::user::validate_user_read(args.arg2, data_len) {
            return SyscallResult::err(e);
        }
    }

    // SAFETY: Validated above — pointers are in user space and mapped.
    let path_bytes = unsafe { core::slice::from_raw_parts(path_ptr, safe_path_len) };
    let path = match core::str::from_utf8(path_bytes) {
        Ok(s) => s,
        Err(_) => return SyscallResult::err(KernelError::InvalidArgument),
    };

    let data = unsafe { core::slice::from_raw_parts(data_ptr, data_len) };

    match crate::fs::Vfs::write_file(path, data) {
        Ok(()) => SyscallResult::ok(0),
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_FS_DELETE` — delete a file.
pub fn sys_fs_delete(args: &SyscallArgs) -> SyscallResult {
    // Capability check: requires File capability with DELETE rights.
    if let Err(e) = require_cap_type(
        crate::cap::ResourceType::File,
        crate::cap::Rights::DELETE,
    ) {
        return SyscallResult::err(e);
    }

    let path_ptr = args.arg0 as *const u8;
    let path_len = args.arg1 as usize;

    if path_ptr.is_null() || path_len == 0 {
        return SyscallResult::err(KernelError::InvalidArgument);
    }

    let safe_path_len = path_len.min(256);
    if let Err(e) = crate::mm::user::validate_user_read(args.arg0, safe_path_len) {
        return SyscallResult::err(e);
    }

    // SAFETY: Validated above — path_ptr is in user space and mapped.
    let path_bytes = unsafe { core::slice::from_raw_parts(path_ptr, safe_path_len) };
    let path = match core::str::from_utf8(path_bytes) {
        Ok(s) => s,
        Err(_) => return SyscallResult::err(KernelError::InvalidArgument),
    };

    match crate::fs::Vfs::remove(path) {
        Ok(()) => SyscallResult::ok(0),
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_FS_LIST_DIR` — list directory entries.
///
/// Packs entries as `FS_DIR_ENTRY_SIZE`-byte records into the buffer.
pub fn sys_fs_list_dir(args: &SyscallArgs) -> SyscallResult {
    // Capability check: requires File capability with READ rights.
    if let Err(e) = require_cap_type(
        crate::cap::ResourceType::File,
        crate::cap::Rights::READ,
    ) {
        return SyscallResult::err(e);
    }

    use super::number::FS_DIR_ENTRY_SIZE;

    let path_ptr = args.arg0 as *const u8;
    let path_len = args.arg1 as usize;
    let buf_ptr = args.arg2 as *mut u8;
    let buf_cap = args.arg3 as usize;

    if path_ptr.is_null() || path_len == 0 || buf_ptr.is_null() {
        return SyscallResult::err(KernelError::InvalidArgument);
    }

    let safe_path_len = path_len.min(256);
    if let Err(e) = crate::mm::user::validate_user_read(args.arg0, safe_path_len) {
        return SyscallResult::err(e);
    }
    if buf_cap > 0 {
        if let Err(e) = crate::mm::user::validate_user_write(args.arg2, buf_cap) {
            return SyscallResult::err(e);
        }
    }

    // SAFETY: Validated above — path_ptr is in user space and mapped.
    let path_bytes = unsafe { core::slice::from_raw_parts(path_ptr, safe_path_len) };
    let path = match core::str::from_utf8(path_bytes) {
        Ok(s) => s,
        Err(_) => return SyscallResult::err(KernelError::InvalidArgument),
    };

    let entries = match crate::fs::Vfs::readdir(path) {
        Ok(e) => e,
        Err(e) => return SyscallResult::err(e),
    };

    // Pack entries into the buffer.
    let max_entries = buf_cap / FS_DIR_ENTRY_SIZE;
    let mut count = 0usize;

    for entry in &entries {
        if count >= max_entries {
            break;
        }

        let offset = count.wrapping_mul(FS_DIR_ENTRY_SIZE);
        // SAFETY: buf_ptr + offset is within buf_cap (checked above).
        unsafe {
            let dest = buf_ptr.add(offset);
            // Zero the entry first.
            core::ptr::write_bytes(dest, 0, FS_DIR_ENTRY_SIZE);

            // Copy filename (up to 255 bytes + null terminator).
            let name_bytes = entry.name.as_bytes();
            let name_len = name_bytes.len().min(255);
            core::ptr::copy_nonoverlapping(name_bytes.as_ptr(), dest, name_len);

            // File size (u32 at offset 256).
            let size_ptr = dest.add(256) as *mut u32;
            #[allow(clippy::cast_possible_truncation)]
            core::ptr::write(size_ptr, entry.size as u32);

            // Entry type (offset 260): 0=file, 1=directory.
            let type_ptr = dest.add(260);
            let type_byte = match entry.entry_type {
                crate::fs::EntryType::File => 0u8,
                crate::fs::EntryType::Directory => 1u8,
                crate::fs::EntryType::Symlink => 3u8,
                // Skip volume labels — they're metadata, not real entries.
                crate::fs::EntryType::VolumeLabel => continue,
            };
            core::ptr::write(type_ptr, type_byte);
        }

        count = count.wrapping_add(1);
    }

    #[allow(clippy::cast_possible_wrap)]
    SyscallResult::ok(count as i64)
}

/// `SYS_FS_MKDIR` — create a directory.
pub fn sys_fs_mkdir(args: &SyscallArgs) -> SyscallResult {
    // Capability check: requires File capability with CREATE rights.
    if let Err(e) = require_cap_type(
        crate::cap::ResourceType::File,
        crate::cap::Rights::CREATE,
    ) {
        return SyscallResult::err(e);
    }

    let path_ptr = args.arg0 as *const u8;
    let path_len = args.arg1 as usize;

    if path_ptr.is_null() || path_len == 0 {
        return SyscallResult::err(KernelError::InvalidArgument);
    }

    let safe_path_len = path_len.min(256);
    if let Err(e) = crate::mm::user::validate_user_read(args.arg0, safe_path_len) {
        return SyscallResult::err(e);
    }

    // SAFETY: Validated above — path_ptr is in user space and mapped.
    let path_bytes = unsafe { core::slice::from_raw_parts(path_ptr, safe_path_len) };
    let path = match core::str::from_utf8(path_bytes) {
        Ok(s) => s,
        Err(_) => return SyscallResult::err(KernelError::InvalidArgument),
    };

    match crate::fs::Vfs::mkdir(path) {
        Ok(()) => SyscallResult::ok(0),
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_FS_RMDIR` — remove an empty directory.
pub fn sys_fs_rmdir(args: &SyscallArgs) -> SyscallResult {
    // Capability check: requires File capability with DELETE rights.
    if let Err(e) = require_cap_type(
        crate::cap::ResourceType::File,
        crate::cap::Rights::DELETE,
    ) {
        return SyscallResult::err(e);
    }

    let path_ptr = args.arg0 as *const u8;
    let path_len = args.arg1 as usize;

    if path_ptr.is_null() || path_len == 0 {
        return SyscallResult::err(KernelError::InvalidArgument);
    }

    let safe_path_len = path_len.min(256);
    if let Err(e) = crate::mm::user::validate_user_read(args.arg0, safe_path_len) {
        return SyscallResult::err(e);
    }

    // SAFETY: Validated above — path_ptr is in user space and mapped.
    let path_bytes = unsafe { core::slice::from_raw_parts(path_ptr, safe_path_len) };
    let path = match core::str::from_utf8(path_bytes) {
        Ok(s) => s,
        Err(_) => return SyscallResult::err(KernelError::InvalidArgument),
    };

    match crate::fs::Vfs::rmdir(path) {
        Ok(()) => SyscallResult::ok(0),
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_FS_STAT` — stat a file or directory.
///
/// Returns metadata in a 16-byte `FsStatResult` buffer:
/// - bytes 0–7: file size (u64, little-endian)
/// - byte 8: entry type (0=file, 1=directory)
/// - bytes 9–15: reserved (zeros)
pub fn sys_fs_stat(args: &SyscallArgs) -> SyscallResult {
    // Capability check: requires File capability with METADATA rights.
    if let Err(e) = require_cap_type(
        crate::cap::ResourceType::File,
        crate::cap::Rights::METADATA,
    ) {
        return SyscallResult::err(e);
    }

    let path_ptr = args.arg0 as *const u8;
    let path_len = args.arg1 as usize;
    let out_ptr = args.arg2 as *mut u8;

    if path_ptr.is_null() || path_len == 0 || out_ptr.is_null() {
        return SyscallResult::err(KernelError::InvalidArgument);
    }

    let safe_path_len = path_len.min(256);
    if let Err(e) = crate::mm::user::validate_user_read(args.arg0, safe_path_len) {
        return SyscallResult::err(e);
    }
    // FsStatResult is 16 bytes.
    if let Err(e) = crate::mm::user::validate_user_write(args.arg2, 16) {
        return SyscallResult::err(e);
    }

    // SAFETY: Validated above — path_ptr is in user space and mapped.
    let path_bytes = unsafe { core::slice::from_raw_parts(path_ptr, safe_path_len) };
    let path = match core::str::from_utf8(path_bytes) {
        Ok(s) => s,
        Err(_) => return SyscallResult::err(KernelError::InvalidArgument),
    };

    let entry = match crate::fs::Vfs::stat(path) {
        Ok(e) => e,
        Err(e) => return SyscallResult::err(e),
    };

    // Write the 16-byte FsStatResult.
    // SAFETY: Validated above — out_ptr is in user space, mapped, and writable.
    unsafe {
        // Zero the buffer first.
        core::ptr::write_bytes(out_ptr, 0, 16);

        // File size (u64 LE) at offset 0.
        let size_ptr = out_ptr as *mut u64;
        core::ptr::write(size_ptr, entry.size);

        // Entry type at offset 8: 0=file, 1=directory, 2=volume label.
        let type_byte = match entry.entry_type {
            crate::fs::EntryType::File => 0u8,
            crate::fs::EntryType::Directory => 1u8,
            crate::fs::EntryType::VolumeLabel => 2u8,
            crate::fs::EntryType::Symlink => 3u8,
        };
        core::ptr::write(out_ptr.add(8), type_byte);
    }

    SyscallResult::ok(0)
}

// ---------------------------------------------------------------------------
// Handle-based filesystem handlers (610–699)
// ---------------------------------------------------------------------------

/// `SYS_FS_OPEN` — open a file, return a handle.
pub fn sys_fs_open(args: &SyscallArgs) -> SyscallResult {
    // Capability: require READ for read-only, WRITE for write.
    // We check the broader File capability — specific rights are
    // enforced by the handle flags.
    if let Err(e) = require_cap_type(
        crate::cap::ResourceType::File,
        crate::cap::Rights::READ,
    ) {
        return SyscallResult::err(e);
    }

    let path_ptr = args.arg0 as *const u8;
    let path_len = args.arg1 as usize;
    #[allow(clippy::cast_possible_truncation)]
    let flags_raw = args.arg2 as u32;

    if path_ptr.is_null() || path_len == 0 {
        return SyscallResult::err(KernelError::InvalidArgument);
    }

    let safe_path_len = path_len.min(256);
    if let Err(e) = crate::mm::user::validate_user_read(args.arg0, safe_path_len) {
        return SyscallResult::err(e);
    }

    // SAFETY: Validated above.
    let path_bytes = unsafe { core::slice::from_raw_parts(path_ptr, safe_path_len) };
    let path = match core::str::from_utf8(path_bytes) {
        Ok(s) => s,
        Err(_) => return SyscallResult::err(KernelError::InvalidArgument),
    };

    let flags = crate::fs::handle::OpenFlags::from_bits(flags_raw);

    match crate::fs::handle::open(path, flags) {
        Ok(handle) => {
            #[allow(clippy::cast_possible_wrap)]
            SyscallResult::ok(handle as i64)
        }
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_FS_CLOSE` — close a file handle.
pub fn sys_fs_close(args: &SyscallArgs) -> SyscallResult {
    let handle = args.arg0;
    match crate::fs::handle::close(handle) {
        Ok(()) => SyscallResult::ok(0),
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_FS_READ` — read from a file handle at the current offset.
pub fn sys_fs_read(args: &SyscallArgs) -> SyscallResult {
    let handle = args.arg0;
    let buf_ptr = args.arg1 as *mut u8;
    let buf_cap = args.arg2 as usize;

    if buf_ptr.is_null() || buf_cap == 0 {
        return SyscallResult::err(KernelError::InvalidArgument);
    }
    if let Err(e) = crate::mm::user::validate_user_write(args.arg1, buf_cap) {
        return SyscallResult::err(e);
    }

    // Allocate a kernel-side buffer, read into it, then copy to user.
    // This avoids passing raw user pointers into the VFS.
    let mut kbuf = alloc::vec![0u8; buf_cap];

    match crate::fs::handle::read(handle, &mut kbuf) {
        Ok(n) => {
            // SAFETY: Validated above — buf_ptr is in user space, mapped, writable.
            unsafe {
                core::ptr::copy_nonoverlapping(kbuf.as_ptr(), buf_ptr, n);
            }
            #[allow(clippy::cast_possible_wrap)]
            SyscallResult::ok(n as i64)
        }
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_FS_WRITE` — write to a file handle at the current offset.
pub fn sys_fs_write(args: &SyscallArgs) -> SyscallResult {
    let handle = args.arg0;
    let data_ptr = args.arg1 as *const u8;
    let data_len = args.arg2 as usize;

    if data_ptr.is_null() && data_len > 0 {
        return SyscallResult::err(KernelError::InvalidArgument);
    }
    if data_len > 0 {
        if let Err(e) = crate::mm::user::validate_user_read(args.arg1, data_len) {
            return SyscallResult::err(e);
        }
    }

    // SAFETY: Validated above.
    let data = if data_len > 0 {
        unsafe { core::slice::from_raw_parts(data_ptr, data_len) }
    } else {
        &[]
    };

    match crate::fs::handle::write(handle, data) {
        Ok(n) => {
            #[allow(clippy::cast_possible_wrap)]
            SyscallResult::ok(n as i64)
        }
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_FS_SEEK` — seek to a new position in a file.
pub fn sys_fs_seek(args: &SyscallArgs) -> SyscallResult {
    let handle = args.arg0;
    #[allow(clippy::cast_possible_wrap)]
    let offset_raw = args.arg1 as i64;
    let whence = args.arg2;

    let seek_from = match whence {
        super::number::SEEK_SET => {
            #[allow(clippy::cast_sign_loss)]
            let pos = if offset_raw < 0 {
                return SyscallResult::err(KernelError::InvalidArgument);
            } else {
                offset_raw as u64
            };
            crate::fs::handle::SeekFrom::Start(pos)
        }
        super::number::SEEK_CUR => crate::fs::handle::SeekFrom::Current(offset_raw),
        super::number::SEEK_END => crate::fs::handle::SeekFrom::End(offset_raw),
        _ => return SyscallResult::err(KernelError::InvalidArgument),
    };

    match crate::fs::handle::seek(handle, seek_from) {
        Ok(new_pos) => {
            #[allow(clippy::cast_possible_wrap)]
            SyscallResult::ok(new_pos as i64)
        }
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_FS_TRUNCATE` — truncate a file to a given size.
pub fn sys_fs_truncate(args: &SyscallArgs) -> SyscallResult {
    if let Err(e) = require_cap_type(
        crate::cap::ResourceType::File,
        crate::cap::Rights::WRITE,
    ) {
        return SyscallResult::err(e);
    }

    let path_ptr = args.arg0 as *const u8;
    let path_len = args.arg1 as usize;
    let new_size = args.arg2;

    if path_ptr.is_null() || path_len == 0 {
        return SyscallResult::err(KernelError::InvalidArgument);
    }

    let safe_path_len = path_len.min(256);
    if let Err(e) = crate::mm::user::validate_user_read(args.arg0, safe_path_len) {
        return SyscallResult::err(e);
    }

    // SAFETY: Validated above.
    let path_bytes = unsafe { core::slice::from_raw_parts(path_ptr, safe_path_len) };
    let path = match core::str::from_utf8(path_bytes) {
        Ok(s) => s,
        Err(_) => return SyscallResult::err(KernelError::InvalidArgument),
    };

    match crate::fs::Vfs::truncate(path, new_size) {
        Ok(()) => SyscallResult::ok(0),
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_FS_RENAME` — rename or move a file or directory.
pub fn sys_fs_rename(args: &SyscallArgs) -> SyscallResult {
    if let Err(e) = require_cap_type(
        crate::cap::ResourceType::File,
        crate::cap::Rights::WRITE,
    ) {
        return SyscallResult::err(e);
    }

    let from_ptr = args.arg0 as *const u8;
    let from_len = args.arg1 as usize;
    let to_ptr = args.arg2 as *const u8;
    let to_len = args.arg3 as usize;

    if from_ptr.is_null() || from_len == 0 || to_ptr.is_null() || to_len == 0 {
        return SyscallResult::err(KernelError::InvalidArgument);
    }

    let safe_from_len = from_len.min(256);
    let safe_to_len = to_len.min(256);

    if let Err(e) = crate::mm::user::validate_user_read(args.arg0, safe_from_len) {
        return SyscallResult::err(e);
    }
    if let Err(e) = crate::mm::user::validate_user_read(args.arg2, safe_to_len) {
        return SyscallResult::err(e);
    }

    // SAFETY: Validated above.
    let from_bytes = unsafe { core::slice::from_raw_parts(from_ptr, safe_from_len) };
    let from_path = match core::str::from_utf8(from_bytes) {
        Ok(s) => s,
        Err(_) => return SyscallResult::err(KernelError::InvalidArgument),
    };

    let to_bytes = unsafe { core::slice::from_raw_parts(to_ptr, safe_to_len) };
    let to_path = match core::str::from_utf8(to_bytes) {
        Ok(s) => s,
        Err(_) => return SyscallResult::err(KernelError::InvalidArgument),
    };

    match crate::fs::Vfs::rename(from_path, to_path) {
        Ok(()) => SyscallResult::ok(0),
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_FS_FSTAT` — stat a file by handle.
pub fn sys_fs_fstat(args: &SyscallArgs) -> SyscallResult {
    let handle = args.arg0;
    let out_ptr = args.arg1 as *mut u8;

    if out_ptr.is_null() {
        return SyscallResult::err(KernelError::InvalidArgument);
    }
    if let Err(e) = crate::mm::user::validate_user_write(args.arg1, 16) {
        return SyscallResult::err(e);
    }

    match crate::fs::handle::fstat(handle) {
        Ok(result) => {
            // Write 16-byte FsStatResult:
            //   [0..8]:   size (u64, LE)
            //   [8]:      entry_type (u8)
            //   [9..12]:  padding
            //   [12..16]: nlinks (u32, LE)
            // SAFETY: Validated above.
            unsafe {
                core::ptr::write_bytes(out_ptr, 0, 16);
                core::ptr::write(out_ptr as *mut u64, result.size);
                core::ptr::write(out_ptr.add(8), result.entry_type);
                core::ptr::write(out_ptr.add(12) as *mut u32, result.nlinks);
            }
            SyscallResult::ok(0)
        }
        Err(e) => SyscallResult::err(e),
    }
}

// ---------------------------------------------------------------------------
// Recycle bin / trash handlers (618–621)
// ---------------------------------------------------------------------------

/// `SYS_FS_TRASH` — move a file to the recycle bin.
///
/// `arg0`: pointer to path string.
/// `arg1`: path length.
pub fn sys_fs_trash(args: &SyscallArgs) -> SyscallResult {
    // Capability check: requires File capability with WRITE rights
    // (deleting requires write access to the directory).
    if let Err(e) = require_cap_type(
        crate::cap::ResourceType::File,
        crate::cap::Rights::WRITE,
    ) {
        return SyscallResult::err(e);
    }

    let ptr = args.arg0 as *const u8;
    let len = args.arg1 as usize;

    if ptr.is_null() || len == 0 || len > 4096 {
        return SyscallResult::err(KernelError::InvalidArgument);
    }
    if let Err(e) = crate::mm::user::validate_user_read(args.arg0, len) {
        return SyscallResult::err(e);
    }

    // SAFETY: Validated above — pointer is in user-space and mapped.
    let path_bytes = unsafe { core::slice::from_raw_parts(ptr, len) };
    let path = match core::str::from_utf8(path_bytes) {
        Ok(s) => s,
        Err(_) => return SyscallResult::err(KernelError::InvalidArgument),
    };

    match crate::fs::trash::trash(path) {
        Ok(()) => SyscallResult::ok(0),
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_FS_TRASH_LIST` — list items in the recycle bin.
///
/// `arg0`: pointer to output buffer.
/// `arg1`: max number of entries.
pub fn sys_fs_trash_list(args: &SyscallArgs) -> SyscallResult {
    // Capability check: requires File capability with READ rights.
    if let Err(e) = require_cap_type(
        crate::cap::ResourceType::File,
        crate::cap::Rights::READ,
    ) {
        return SyscallResult::err(e);
    }

    let out_ptr = args.arg0 as *mut u8;
    let max_entries = args.arg1 as usize;

    if out_ptr.is_null() || max_entries == 0 {
        return SyscallResult::err(KernelError::InvalidArgument);
    }

    let buf_size = max_entries.saturating_mul(crate::syscall::number::FS_TRASH_ENTRY_SIZE);
    if let Err(e) = crate::mm::user::validate_user_write(args.arg0, buf_size) {
        return SyscallResult::err(e);
    }

    match crate::fs::trash::list() {
        Ok(items) => {
            let count = items.len().min(max_entries);

            // SAFETY: Validated above.
            unsafe {
                core::ptr::write_bytes(out_ptr, 0, buf_size);

                for (i, item) in items.iter().take(count).enumerate() {
                    let entry_base = out_ptr.add(
                        i.wrapping_mul(crate::syscall::number::FS_TRASH_ENTRY_SIZE),
                    );

                    // Write trash name (bytes 0..256).
                    let name_bytes = item.trash_name.as_bytes();
                    let name_len = name_bytes.len().min(255);
                    core::ptr::copy_nonoverlapping(
                        name_bytes.as_ptr(),
                        entry_base,
                        name_len,
                    );

                    // Write original path (bytes 256..512).
                    let path_bytes = item.original_path.as_bytes();
                    let path_len = path_bytes.len().min(255);
                    core::ptr::copy_nonoverlapping(
                        path_bytes.as_ptr(),
                        entry_base.add(256),
                        path_len,
                    );

                    // Write file size (bytes 512..520).
                    let size_ptr = entry_base.add(512) as *mut u64;
                    core::ptr::write(size_ptr, item.size);

                    // Write flags (bytes 520..524).
                    let flags: u32 = if item.is_directory { 1 } else { 0 };
                    let flags_ptr = entry_base.add(520) as *mut u32;
                    core::ptr::write(flags_ptr, flags);
                }
            }

            #[allow(clippy::cast_possible_wrap)]
            SyscallResult::ok(count as i64)
        }
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_FS_TRASH_RESTORE` — restore a file from the recycle bin.
///
/// `arg0`: pointer to trash filename string.
/// `arg1`: trash filename length.
/// `arg2`: pointer to 256-byte output buffer for restored path.
pub fn sys_fs_trash_restore(args: &SyscallArgs) -> SyscallResult {
    // Capability check: requires File capability with WRITE rights.
    if let Err(e) = require_cap_type(
        crate::cap::ResourceType::File,
        crate::cap::Rights::WRITE,
    ) {
        return SyscallResult::err(e);
    }

    let name_ptr = args.arg0 as *const u8;
    let name_len = args.arg1 as usize;
    let out_ptr = args.arg2 as *mut u8;

    if name_ptr.is_null() || name_len == 0 || name_len > 256 {
        return SyscallResult::err(KernelError::InvalidArgument);
    }
    if let Err(e) = crate::mm::user::validate_user_read(args.arg0, name_len) {
        return SyscallResult::err(e);
    }

    if !out_ptr.is_null() {
        if let Err(e) = crate::mm::user::validate_user_write(args.arg2, 256) {
            return SyscallResult::err(e);
        }
    }

    // SAFETY: Validated above.
    let name_bytes = unsafe { core::slice::from_raw_parts(name_ptr, name_len) };
    let trash_name = match core::str::from_utf8(name_bytes) {
        Ok(s) => s,
        Err(_) => return SyscallResult::err(KernelError::InvalidArgument),
    };

    match crate::fs::trash::restore(trash_name) {
        Ok(restored_path) => {
            // Write the restored path to the output buffer.
            if !out_ptr.is_null() {
                let path_bytes = restored_path.as_bytes();
                let copy_len = path_bytes.len().min(255);
                // SAFETY: Validated above.
                unsafe {
                    core::ptr::write_bytes(out_ptr, 0, 256);
                    core::ptr::copy_nonoverlapping(
                        path_bytes.as_ptr(),
                        out_ptr,
                        copy_len,
                    );
                }
            }

            #[allow(clippy::cast_possible_wrap)]
            SyscallResult::ok(restored_path.len() as i64)
        }
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_FS_TRASH_EMPTY` — permanently delete all items in the recycle bin.
pub fn sys_fs_trash_empty(_args: &SyscallArgs) -> SyscallResult {
    // Capability check: requires File capability with WRITE rights.
    if let Err(e) = require_cap_type(
        crate::cap::ResourceType::File,
        crate::cap::Rights::WRITE,
    ) {
        return SyscallResult::err(e);
    }

    match crate::fs::trash::empty() {
        Ok(()) => SyscallResult::ok(0),
        Err(e) => SyscallResult::err(e),
    }
}

// ---------------------------------------------------------------------------
// Filesystem watch handlers (622–624)
// ---------------------------------------------------------------------------

/// `SYS_FS_WATCH_CREATE` — create a filesystem change watch.
///
/// `arg0`: pointer to path string.
/// `arg1`: path length.
/// `arg2`: event mask.
/// `arg3`: flags (bit 0 = recursive).
pub fn sys_fs_watch_create(args: &SyscallArgs) -> SyscallResult {
    if let Err(e) = require_cap_type(
        crate::cap::ResourceType::File,
        crate::cap::Rights::READ,
    ) {
        return SyscallResult::err(e);
    }

    let ptr = args.arg0 as *const u8;
    let len = args.arg1 as usize;
    let mask = args.arg2 as u32;
    let flags = args.arg3;

    if ptr.is_null() || len == 0 || len > 4096 {
        return SyscallResult::err(KernelError::InvalidArgument);
    }
    if let Err(e) = crate::mm::user::validate_user_read(args.arg0, len) {
        return SyscallResult::err(e);
    }

    // SAFETY: Validated above.
    let path_bytes = unsafe { core::slice::from_raw_parts(ptr, len) };
    let path = match core::str::from_utf8(path_bytes) {
        Ok(s) => s,
        Err(_) => return SyscallResult::err(KernelError::InvalidArgument),
    };

    let recursive = (flags & 1) != 0;
    let event_mask = crate::fs::notify::FsEventMask(mask);

    match crate::fs::notify::create_watch(path, event_mask, recursive) {
        Ok(id) => {
            #[allow(clippy::cast_possible_wrap)]
            SyscallResult::ok(id as i64)
        }
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_FS_WATCH_READ` — read pending filesystem change events.
///
/// `arg0`: watch ID.
/// `arg1`: pointer to output buffer.
/// `arg2`: max number of events.
pub fn sys_fs_watch_read(args: &SyscallArgs) -> SyscallResult {
    let watch_id = args.arg0;
    let out_ptr = args.arg1 as *mut u8;
    let max_events = args.arg2 as usize;

    if out_ptr.is_null() || max_events == 0 {
        return SyscallResult::err(KernelError::InvalidArgument);
    }

    let buf_size = max_events.saturating_mul(crate::syscall::number::FS_WATCH_EVENT_SIZE);
    if let Err(e) = crate::mm::user::validate_user_write(args.arg1, buf_size) {
        return SyscallResult::err(e);
    }

    match crate::fs::notify::read_events(watch_id, max_events) {
        Ok(events) => {
            let count = events.len();

            // SAFETY: Validated above.
            unsafe {
                core::ptr::write_bytes(out_ptr, 0, buf_size);

                for (i, event) in events.iter().enumerate() {
                    let base = out_ptr.add(
                        i.wrapping_mul(crate::syscall::number::FS_WATCH_EVENT_SIZE),
                    );

                    // Write path (bytes 0..256).
                    let path_bytes = event.path.as_bytes();
                    let path_len = path_bytes.len().min(255);
                    core::ptr::copy_nonoverlapping(
                        path_bytes.as_ptr(),
                        base,
                        path_len,
                    );

                    // Write new_path for rename events (bytes 256..512).
                    if let Some(ref np) = event.new_path {
                        let np_bytes = np.as_bytes();
                        let np_len = np_bytes.len().min(255);
                        core::ptr::copy_nonoverlapping(
                            np_bytes.as_ptr(),
                            base.add(256),
                            np_len,
                        );
                    }

                    // Write watch ID (bytes 512..520).
                    let id_ptr = base.add(512) as *mut u64;
                    core::ptr::write(id_ptr, event.watch_id);

                    // Write event type (bytes 520..524).
                    let type_ptr = base.add(520) as *mut u32;
                    core::ptr::write(type_ptr, event.event_type as u32);
                }
            }

            #[allow(clippy::cast_possible_wrap)]
            SyscallResult::ok(count as i64)
        }
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_FS_WATCH_CLOSE` — close a filesystem watch.
///
/// `arg0`: watch ID.
pub fn sys_fs_watch_close(args: &SyscallArgs) -> SyscallResult {
    let watch_id = args.arg0;

    match crate::fs::notify::close_watch(watch_id) {
        Ok(()) => SyscallResult::ok(0),
        Err(e) => SyscallResult::err(e),
    }
}

// ---------------------------------------------------------------------------
// Change journal handlers (625–627)
// ---------------------------------------------------------------------------

/// `SYS_FS_JOURNAL_CURSOR` — get the current journal sequence number.
///
/// No arguments.
pub fn sys_fs_journal_cursor(_args: &SyscallArgs) -> SyscallResult {
    SyscallResult::ok(crate::fs::journal::cursor() as i64)
}

/// `SYS_FS_JOURNAL_READ` — read journal entries since a sequence number.
///
/// `arg0`: sequence number (exclusive — returns entries with seq > arg0).
/// `arg1`: pointer to output buffer.
/// `arg2`: buffer length in bytes.
pub fn sys_fs_journal_read(args: &SyscallArgs) -> SyscallResult {
    use crate::syscall::number::FS_JOURNAL_ENTRY_SIZE;

    let since_seq = args.arg0;
    let buf_ptr = args.arg1 as usize;
    let buf_len = args.arg2 as usize;

    // Calculate how many entries fit in the buffer.
    if buf_len < FS_JOURNAL_ENTRY_SIZE {
        return SyscallResult::err(KernelError::InvalidArgument);
    }
    let max_entries = buf_len / FS_JOURNAL_ENTRY_SIZE;

    // Read entries from the journal.
    let (entries, _current) = crate::fs::journal::read_since(since_seq);

    let count = entries.len().min(max_entries);

    // Marshal entries into the user buffer.
    for (i, entry) in entries.iter().take(count).enumerate() {
        let offset = i * FS_JOURNAL_ENTRY_SIZE;
        let entry_ptr = buf_ptr + offset;

        // Validate user buffer for this entry.
        if let Err(e) = crate::mm::user::validate_user_write(entry_ptr as u64, FS_JOURNAL_ENTRY_SIZE) {
            return SyscallResult::err(e);
        }

        // Write the entry to user memory.
        // Layout: seq(8) + timestamp_ns(8) + event_type(1) + path(256) + old_path(256) = 529
        // SAFETY: Buffer has been validated for the full entry size.
        unsafe {
            let base = entry_ptr as *mut u8;

            // seq (u64 LE, offset 0)
            core::ptr::copy_nonoverlapping(
                entry.seq.to_le_bytes().as_ptr(),
                base,
                8,
            );

            // timestamp_ns (u64 LE, offset 8)
            core::ptr::copy_nonoverlapping(
                entry.timestamp_ns.to_le_bytes().as_ptr(),
                base.add(8),
                8,
            );

            // event_type (u8, offset 16)
            *base.add(16) = entry.event_type as u8;

            // path (256 bytes, null-terminated, offset 17)
            let path_bytes = entry.path.as_bytes();
            let path_len = path_bytes.len().min(255);
            core::ptr::copy_nonoverlapping(path_bytes.as_ptr(), base.add(17), path_len);
            core::ptr::write_bytes(base.add(17 + path_len), 0, 256 - path_len);

            // old_path (256 bytes, null-terminated, offset 273)
            let old_bytes = entry.old_path.as_bytes();
            let old_len = old_bytes.len().min(255);
            core::ptr::copy_nonoverlapping(old_bytes.as_ptr(), base.add(273), old_len);
            core::ptr::write_bytes(base.add(273 + old_len), 0, 256 - old_len);
        }
    }

    SyscallResult::ok(count as i64)
}

/// `SYS_FS_JOURNAL_FLUSH` — flush the change journal to disk.
///
/// No arguments.
pub fn sys_fs_journal_flush(_args: &SyscallArgs) -> SyscallResult {
    match crate::fs::journal::flush() {
        Ok(()) => SyscallResult::ok(0),
        Err(e) => SyscallResult::err(e),
    }
}

// ---------------------------------------------------------------------------
// Metadata handlers (628–636)
// ---------------------------------------------------------------------------

/// Helper: read a path string from user pointers (arg0=ptr, arg1=len).
///
/// Returns the path as a `&str`.  Validates memory and UTF-8.
unsafe fn read_user_path<'a>(ptr: u64, len: u64) -> Result<&'a str, SyscallResult> {
    let path_ptr = ptr as *const u8;
    let path_len = (len as usize).min(256);
    if path_ptr.is_null() || path_len == 0 {
        return Err(SyscallResult::err(KernelError::InvalidArgument));
    }
    if let Err(e) = crate::mm::user::validate_user_read(ptr, path_len) {
        return Err(SyscallResult::err(e));
    }
    // SAFETY: Validated above — path_ptr is in user space and mapped.
    let bytes = unsafe { core::slice::from_raw_parts(path_ptr, path_len) };
    core::str::from_utf8(bytes).map_err(|_| SyscallResult::err(KernelError::InvalidArgument))
}

/// `SYS_FS_METADATA` — get rich file metadata.
///
/// `arg0`: pointer to path string.
/// `arg1`: path length.
/// `arg2`: pointer to output buffer (FS_META_SIZE = 64 bytes).
#[allow(clippy::cast_possible_truncation)]
pub fn sys_fs_metadata(args: &SyscallArgs) -> SyscallResult {
    if let Err(e) = require_cap_type(
        crate::cap::ResourceType::File,
        crate::cap::Rights::METADATA,
    ) {
        return SyscallResult::err(e);
    }

    let out_ptr = args.arg2 as *mut u8;
    if out_ptr.is_null() {
        return SyscallResult::err(KernelError::InvalidArgument);
    }
    if let Err(e) = crate::mm::user::validate_user_write(
        args.arg2,
        crate::syscall::number::FS_META_SIZE,
    ) {
        return SyscallResult::err(e);
    }

    // SAFETY: Validated above.
    let path = match unsafe { read_user_path(args.arg0, args.arg1) } {
        Ok(p) => p,
        Err(r) => return r,
    };

    let meta = match crate::fs::Vfs::metadata(path) {
        Ok(m) => m,
        Err(e) => return SyscallResult::err(e),
    };

    // Marshal to output buffer.
    // SAFETY: out_ptr validated for FS_META_SIZE bytes.
    unsafe {
        core::ptr::write(out_ptr as *mut u64, meta.size);
        *out_ptr.add(8) = match meta.entry_type {
            crate::fs::EntryType::File => 0,
            crate::fs::EntryType::Directory => 1,
            crate::fs::EntryType::VolumeLabel => 2,
            crate::fs::EntryType::Symlink => 3,
        };
        // [9..16] padding (zero)
        core::ptr::write_bytes(out_ptr.add(9), 0, 7);
        core::ptr::write(out_ptr.add(16) as *mut u64, meta.created_ns);
        core::ptr::write(out_ptr.add(24) as *mut u64, meta.modified_ns);
        core::ptr::write(out_ptr.add(32) as *mut u64, meta.accessed_ns);
        core::ptr::write(out_ptr.add(40) as *mut u64, meta.changed_ns);
        core::ptr::write(out_ptr.add(48) as *mut u32, meta.uid);
        core::ptr::write(out_ptr.add(52) as *mut u32, meta.gid);
        core::ptr::write(out_ptr.add(56) as *mut u16, meta.permissions);
        core::ptr::write(out_ptr.add(58) as *mut u32, meta.attributes.bits());
        core::ptr::write_bytes(out_ptr.add(62), 0, 2);
    }

    SyscallResult::ok(0)
}

/// `SYS_FS_SET_ATTR` — set file attributes.
///
/// `arg0`: path pointer.  `arg1`: path length.  `arg2`: attribute bits.
pub fn sys_fs_set_attr(args: &SyscallArgs) -> SyscallResult {
    if let Err(e) = require_cap_type(
        crate::cap::ResourceType::File,
        crate::cap::Rights::WRITE,
    ) {
        return SyscallResult::err(e);
    }
    // SAFETY: Validated in read_user_path.
    let path = match unsafe { read_user_path(args.arg0, args.arg1) } {
        Ok(p) => p,
        Err(r) => return r,
    };
    let attrs = crate::fs::FileAttr::from_bits(args.arg2 as u32);
    match crate::fs::Vfs::set_attributes(path, attrs) {
        Ok(()) => SyscallResult::ok(0),
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_FS_SET_OWNER` — set file ownership.
///
/// `arg0`: path pointer.  `arg1`: path length.
/// `arg2`: uid.  `arg3`: gid.
#[allow(clippy::cast_possible_truncation)]
pub fn sys_fs_set_owner(args: &SyscallArgs) -> SyscallResult {
    if let Err(e) = require_cap_type(
        crate::cap::ResourceType::File,
        crate::cap::Rights::WRITE,
    ) {
        return SyscallResult::err(e);
    }
    // SAFETY: Validated in read_user_path.
    let path = match unsafe { read_user_path(args.arg0, args.arg1) } {
        Ok(p) => p,
        Err(r) => return r,
    };
    match crate::fs::Vfs::set_owner(path, args.arg2 as u32, args.arg3 as u32) {
        Ok(()) => SyscallResult::ok(0),
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_FS_SET_PERMS` — set permission bits.
///
/// `arg0`: path pointer.  `arg1`: path length.
/// `arg2`: permission bits (u16).
#[allow(clippy::cast_possible_truncation)]
pub fn sys_fs_set_perms(args: &SyscallArgs) -> SyscallResult {
    if let Err(e) = require_cap_type(
        crate::cap::ResourceType::File,
        crate::cap::Rights::WRITE,
    ) {
        return SyscallResult::err(e);
    }
    // SAFETY: Validated in read_user_path.
    let path = match unsafe { read_user_path(args.arg0, args.arg1) } {
        Ok(p) => p,
        Err(r) => return r,
    };
    match crate::fs::Vfs::set_permissions(path, args.arg2 as u16) {
        Ok(()) => SyscallResult::ok(0),
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_FS_SET_TIMES` — set timestamps.
///
/// `arg0`: path pointer.  `arg1`: path length.
/// `arg2`: accessed_ns (0 = unchanged).  `arg3`: modified_ns (0 = unchanged).
pub fn sys_fs_set_times(args: &SyscallArgs) -> SyscallResult {
    if let Err(e) = require_cap_type(
        crate::cap::ResourceType::File,
        crate::cap::Rights::WRITE,
    ) {
        return SyscallResult::err(e);
    }
    // SAFETY: Validated in read_user_path.
    let path = match unsafe { read_user_path(args.arg0, args.arg1) } {
        Ok(p) => p,
        Err(r) => return r,
    };
    match crate::fs::Vfs::set_times(path, args.arg2, args.arg3) {
        Ok(()) => SyscallResult::ok(0),
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_FS_GET_XATTR` — get an extended attribute.
///
/// `arg0`: path pointer.  `arg1`: path length.
/// `arg2`: key pointer (null-terminated).  `arg3`: output buffer pointer.
/// `arg4`: buffer capacity.
#[allow(clippy::cast_possible_truncation)]
pub fn sys_fs_get_xattr(args: &SyscallArgs) -> SyscallResult {
    if let Err(e) = require_cap_type(
        crate::cap::ResourceType::File,
        crate::cap::Rights::METADATA,
    ) {
        return SyscallResult::err(e);
    }
    // SAFETY: Validated in read_user_path.
    let path = match unsafe { read_user_path(args.arg0, args.arg1) } {
        Ok(p) => p,
        Err(r) => return r,
    };
    // Read key from arg2 (null-terminated string, max 255 bytes).
    let key_ptr = args.arg2 as *const u8;
    if key_ptr.is_null() {
        return SyscallResult::err(KernelError::InvalidArgument);
    }
    if let Err(e) = crate::mm::user::validate_user_read(args.arg2, 1) {
        return SyscallResult::err(e);
    }
    // SAFETY: Validated above, scan for null within 256 bytes.
    let key = unsafe {
        let mut len = 0usize;
        while len < 256 {
            if *key_ptr.add(len) == 0 {
                break;
            }
            len = len.wrapping_add(1);
        }
        let bytes = core::slice::from_raw_parts(key_ptr, len);
        match core::str::from_utf8(bytes) {
            Ok(s) => s,
            Err(_) => return SyscallResult::err(KernelError::InvalidArgument),
        }
    };

    let out_ptr = args.arg3 as *mut u8;
    let capacity = args.arg4 as usize;
    if out_ptr.is_null() || capacity == 0 {
        return SyscallResult::err(KernelError::InvalidArgument);
    }
    if let Err(e) = crate::mm::user::validate_user_write(args.arg3, capacity) {
        return SyscallResult::err(e);
    }

    match crate::fs::Vfs::get_xattr(path, key) {
        Ok(val) => {
            let copy_len = val.len().min(capacity);
            // SAFETY: out_ptr validated for capacity bytes.
            unsafe {
                core::ptr::copy_nonoverlapping(val.as_ptr(), out_ptr, copy_len);
            }
            SyscallResult::ok(copy_len as i64)
        }
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_FS_SET_XATTR` — set an extended attribute.
///
/// `arg0`: path pointer.  `arg1`: path length.
/// `arg2`: key pointer (null-terminated).  `arg3`: value pointer.
/// `arg4`: value length.
pub fn sys_fs_set_xattr(args: &SyscallArgs) -> SyscallResult {
    if let Err(e) = require_cap_type(
        crate::cap::ResourceType::File,
        crate::cap::Rights::WRITE,
    ) {
        return SyscallResult::err(e);
    }
    // SAFETY: Validated in read_user_path.
    let path = match unsafe { read_user_path(args.arg0, args.arg1) } {
        Ok(p) => p,
        Err(r) => return r,
    };
    // Read key.
    let key_ptr = args.arg2 as *const u8;
    if key_ptr.is_null() {
        return SyscallResult::err(KernelError::InvalidArgument);
    }
    if let Err(e) = crate::mm::user::validate_user_read(args.arg2, 1) {
        return SyscallResult::err(e);
    }
    // SAFETY: Validated.
    let key = unsafe {
        let mut len = 0usize;
        while len < 256 {
            if *key_ptr.add(len) == 0 {
                break;
            }
            len = len.wrapping_add(1);
        }
        let bytes = core::slice::from_raw_parts(key_ptr, len);
        match core::str::from_utf8(bytes) {
            Ok(s) => s,
            Err(_) => return SyscallResult::err(KernelError::InvalidArgument),
        }
    };
    // Read value.
    let val_ptr = args.arg3 as *const u8;
    let val_len = (args.arg4 as usize).min(65536);
    if val_ptr.is_null() && val_len > 0 {
        return SyscallResult::err(KernelError::InvalidArgument);
    }
    if val_len > 0 {
        if let Err(e) = crate::mm::user::validate_user_read(args.arg3, val_len) {
            return SyscallResult::err(e);
        }
    }
    // SAFETY: Validated.
    let value = if val_len > 0 {
        unsafe { core::slice::from_raw_parts(val_ptr, val_len) }
    } else {
        &[]
    };

    match crate::fs::Vfs::set_xattr(path, key, value) {
        Ok(()) => SyscallResult::ok(0),
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_FS_REMOVE_XATTR` — remove an extended attribute.
///
/// `arg0`: path pointer.  `arg1`: path length.
/// `arg2`: key pointer (null-terminated).
pub fn sys_fs_remove_xattr(args: &SyscallArgs) -> SyscallResult {
    if let Err(e) = require_cap_type(
        crate::cap::ResourceType::File,
        crate::cap::Rights::WRITE,
    ) {
        return SyscallResult::err(e);
    }
    // SAFETY: Validated in read_user_path.
    let path = match unsafe { read_user_path(args.arg0, args.arg1) } {
        Ok(p) => p,
        Err(r) => return r,
    };
    let key_ptr = args.arg2 as *const u8;
    if key_ptr.is_null() {
        return SyscallResult::err(KernelError::InvalidArgument);
    }
    if let Err(e) = crate::mm::user::validate_user_read(args.arg2, 1) {
        return SyscallResult::err(e);
    }
    // SAFETY: Validated.
    let key = unsafe {
        let mut len = 0usize;
        while len < 256 {
            if *key_ptr.add(len) == 0 {
                break;
            }
            len = len.wrapping_add(1);
        }
        let bytes = core::slice::from_raw_parts(key_ptr, len);
        match core::str::from_utf8(bytes) {
            Ok(s) => s,
            Err(_) => return SyscallResult::err(KernelError::InvalidArgument),
        }
    };

    match crate::fs::Vfs::remove_xattr(path, key) {
        Ok(()) => SyscallResult::ok(0),
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_FS_LIST_XATTRS` — list extended attribute keys.
///
/// `arg0`: path pointer.  `arg1`: path length.
/// `arg2`: output buffer pointer.  `arg3`: buffer capacity.
#[allow(clippy::cast_possible_truncation)]
pub fn sys_fs_list_xattrs(args: &SyscallArgs) -> SyscallResult {
    if let Err(e) = require_cap_type(
        crate::cap::ResourceType::File,
        crate::cap::Rights::METADATA,
    ) {
        return SyscallResult::err(e);
    }
    // SAFETY: Validated in read_user_path.
    let path = match unsafe { read_user_path(args.arg0, args.arg1) } {
        Ok(p) => p,
        Err(r) => return r,
    };
    let out_ptr = args.arg2 as *mut u8;
    let capacity = args.arg3 as usize;
    if out_ptr.is_null() || capacity == 0 {
        return SyscallResult::err(KernelError::InvalidArgument);
    }
    if let Err(e) = crate::mm::user::validate_user_write(args.arg2, capacity) {
        return SyscallResult::err(e);
    }

    let keys = match crate::fs::Vfs::list_xattrs(path) {
        Ok(k) => k,
        Err(e) => return SyscallResult::err(e),
    };

    // Pack keys as null-terminated strings.
    let mut offset = 0usize;
    for key in &keys {
        let needed = key.len().wrapping_add(1); // +1 for null terminator
        if offset.wrapping_add(needed) > capacity {
            break;
        }
        // SAFETY: out_ptr validated for capacity bytes.
        unsafe {
            core::ptr::copy_nonoverlapping(key.as_ptr(), out_ptr.add(offset), key.len());
            *out_ptr.add(offset.wrapping_add(key.len())) = 0;
        }
        offset = offset.wrapping_add(needed);
    }

    SyscallResult::ok(offset as i64)
}

// ---------------------------------------------------------------------------
// Symlink handlers (637–639)
// ---------------------------------------------------------------------------

/// `SYS_FS_SYMLINK` — create a symbolic link.
///
/// `arg0`: pointer to link path string.
/// `arg1`: link path length.
/// `arg2`: pointer to target string.
/// `arg3`: target string length.
pub fn sys_fs_symlink(args: &SyscallArgs) -> SyscallResult {
    if let Err(e) = require_cap_type(
        crate::cap::ResourceType::File,
        crate::cap::Rights::CREATE,
    ) {
        return SyscallResult::err(e);
    }

    // SAFETY: Validated in read_user_path.
    let path = match unsafe { read_user_path(args.arg0, args.arg1) } {
        Ok(p) => p,
        Err(r) => return r,
    };

    // Read the target string (arg2=ptr, arg3=len).
    let target_ptr = args.arg2 as *const u8;
    let target_len = (args.arg3 as usize).min(256);
    if target_ptr.is_null() || target_len == 0 {
        return SyscallResult::err(KernelError::InvalidArgument);
    }
    if let Err(e) = crate::mm::user::validate_user_read(args.arg2, target_len) {
        return SyscallResult::err(e);
    }
    // SAFETY: Validated above — target_ptr is in user space and mapped.
    let target_bytes = unsafe { core::slice::from_raw_parts(target_ptr, target_len) };
    let target = match core::str::from_utf8(target_bytes) {
        Ok(s) => s,
        Err(_) => return SyscallResult::err(KernelError::InvalidArgument),
    };

    match crate::fs::Vfs::symlink(path, target) {
        Ok(()) => SyscallResult::ok(0),
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_FS_READLINK` — read the target of a symbolic link.
///
/// `arg0`: pointer to path string.
/// `arg1`: path length.
/// `arg2`: pointer to output buffer.
/// `arg3`: output buffer capacity.
///
/// Returns the number of bytes written to the output buffer (the target
/// string length, not null-terminated).  If the buffer is too small, the
/// target is truncated.
#[allow(clippy::cast_possible_wrap)]
pub fn sys_fs_readlink(args: &SyscallArgs) -> SyscallResult {
    if let Err(e) = require_cap_type(
        crate::cap::ResourceType::File,
        crate::cap::Rights::METADATA,
    ) {
        return SyscallResult::err(e);
    }

    // SAFETY: Validated in read_user_path.
    let path = match unsafe { read_user_path(args.arg0, args.arg1) } {
        Ok(p) => p,
        Err(r) => return r,
    };

    let out_ptr = args.arg2 as *mut u8;
    let capacity = args.arg3 as usize;
    if out_ptr.is_null() || capacity == 0 {
        return SyscallResult::err(KernelError::InvalidArgument);
    }
    if let Err(e) = crate::mm::user::validate_user_write(args.arg2, capacity) {
        return SyscallResult::err(e);
    }

    let target = match crate::fs::Vfs::readlink(path) {
        Ok(t) => t,
        Err(e) => return SyscallResult::err(e),
    };

    // Copy target to user buffer, truncating if necessary.
    let copy_len = target.len().min(capacity);
    // SAFETY: out_ptr validated for capacity bytes.
    unsafe {
        core::ptr::copy_nonoverlapping(target.as_ptr(), out_ptr, copy_len);
    }

    SyscallResult::ok(copy_len as i64)
}

/// `SYS_FS_LSTAT` — stat a path without following the final symlink.
///
/// `arg0`: pointer to path string.
/// `arg1`: path length.
/// `arg2`: pointer to output buffer (16-byte `FsStatResult`).
///
/// Same output format as `SYS_FS_STAT`:
/// - bytes 0–7: file size (u64, little-endian).  For symlinks, this is the
///   length of the target path string (matching Linux lstat behavior).
/// - byte 8: entry type (0=file, 1=directory, 2=volume label, 3=symlink).
/// - bytes 9–15: reserved (zeros).
pub fn sys_fs_lstat(args: &SyscallArgs) -> SyscallResult {
    if let Err(e) = require_cap_type(
        crate::cap::ResourceType::File,
        crate::cap::Rights::METADATA,
    ) {
        return SyscallResult::err(e);
    }

    // SAFETY: Validated in read_user_path.
    let path = match unsafe { read_user_path(args.arg0, args.arg1) } {
        Ok(p) => p,
        Err(r) => return r,
    };

    let out_ptr = args.arg2 as *mut u8;
    if out_ptr.is_null() {
        return SyscallResult::err(KernelError::InvalidArgument);
    }
    // FsStatResult is 16 bytes.
    if let Err(e) = crate::mm::user::validate_user_write(args.arg2, 16) {
        return SyscallResult::err(e);
    }

    let entry = match crate::fs::Vfs::lstat(path) {
        Ok(e) => e,
        Err(e) => return SyscallResult::err(e),
    };

    // Write the 16-byte FsStatResult.
    // SAFETY: Validated above — out_ptr is in user space, mapped, and writable.
    unsafe {
        core::ptr::write_bytes(out_ptr, 0, 16);
        let size_ptr = out_ptr as *mut u64;
        core::ptr::write(size_ptr, entry.size);
        let type_byte = match entry.entry_type {
            crate::fs::EntryType::File => 0u8,
            crate::fs::EntryType::Directory => 1u8,
            crate::fs::EntryType::VolumeLabel => 2u8,
            crate::fs::EntryType::Symlink => 3u8,
        };
        core::ptr::write(out_ptr.add(8), type_byte);
    }

    SyscallResult::ok(0)
}

/// `SYS_FS_FLOCK` — acquire an advisory file lock.
///
/// `arg0`: pointer to path string.
/// `arg1`: path length.
/// `arg2`: lock type (0 = shared, 1 = exclusive).
/// `arg3`: owner ID.
pub fn sys_fs_flock(args: &SyscallArgs) -> SyscallResult {
    if let Err(e) = require_cap_type(
        crate::cap::ResourceType::File,
        crate::cap::Rights::METADATA,
    ) {
        return SyscallResult::err(e);
    }

    // SAFETY: Validated in read_user_path.
    let path = match unsafe { read_user_path(args.arg0, args.arg1) } {
        Ok(p) => p,
        Err(r) => return r,
    };

    let lock_type = match args.arg2 {
        0 => crate::fs::LockType::Shared,
        1 => crate::fs::LockType::Exclusive,
        _ => return SyscallResult::err(KernelError::InvalidArgument),
    };

    let owner = args.arg3;

    match crate::fs::Vfs::flock(path, owner, lock_type) {
        Ok(()) => SyscallResult::ok(0),
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_FS_FUNLOCK` — release an advisory file lock.
///
/// `arg0`: pointer to path string.
/// `arg1`: path length.
/// `arg2`: owner ID.
pub fn sys_fs_funlock(args: &SyscallArgs) -> SyscallResult {
    // SAFETY: Validated in read_user_path.
    let path = match unsafe { read_user_path(args.arg0, args.arg1) } {
        Ok(p) => p,
        Err(r) => return r,
    };

    let owner = args.arg2;

    match crate::fs::Vfs::funlock(path, owner) {
        Ok(()) => SyscallResult::ok(0),
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_FS_SYNC` — flush all filesystems to stable storage.
pub fn sys_fs_sync(_args: &SyscallArgs) -> SyscallResult {
    match crate::fs::Vfs::sync() {
        Ok(()) => SyscallResult::ok(0),
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_FS_LINK` — create a hard link.
///
/// `arg0`: pointer to existing path string.
/// `arg1`: existing path length.
/// `arg2`: pointer to new link path string.
/// `arg3`: new link path length.
pub fn sys_fs_link(args: &SyscallArgs) -> SyscallResult {
    if let Err(e) = require_cap_type(
        crate::cap::ResourceType::File,
        crate::cap::Rights::CREATE,
    ) {
        return SyscallResult::err(e);
    }

    // SAFETY: Validated in read_user_path.
    let existing = match unsafe { read_user_path(args.arg0, args.arg1) } {
        Ok(p) => p,
        Err(r) => return r,
    };

    // Read the new link path (arg2=ptr, arg3=len).
    let new_ptr = args.arg2 as *const u8;
    let new_len = (args.arg3 as usize).min(4096);
    if new_ptr.is_null() || new_len == 0 {
        return SyscallResult::err(KernelError::InvalidArgument);
    }
    if let Err(e) = crate::mm::user::validate_user_read(args.arg2, new_len) {
        return SyscallResult::err(e);
    }
    // SAFETY: Validated above — new_ptr is in user space and mapped.
    let new_bytes = unsafe { core::slice::from_raw_parts(new_ptr, new_len) };
    let new_path = match core::str::from_utf8(new_bytes) {
        Ok(s) => s,
        Err(_) => return SyscallResult::err(KernelError::InvalidArgument),
    };

    match crate::fs::Vfs::link(existing, new_path) {
        Ok(()) => SyscallResult::ok(0),
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_FS_STATVFS` — query filesystem space and configuration info.
///
/// `arg0`: pointer to path string (any path on the target filesystem).
/// `arg1`: path length.
/// `arg2`: pointer to 64-byte output buffer.
#[allow(clippy::cast_possible_truncation)]
pub fn sys_fs_statvfs(args: &SyscallArgs) -> SyscallResult {
    if let Err(e) = require_cap_type(
        crate::cap::ResourceType::File,
        crate::cap::Rights::METADATA,
    ) {
        return SyscallResult::err(e);
    }

    // SAFETY: Validated in read_user_path.
    let path = match unsafe { read_user_path(args.arg0, args.arg1) } {
        Ok(p) => p,
        Err(r) => return r,
    };

    let out_ptr = args.arg2 as *mut u8;
    if out_ptr.is_null() {
        return SyscallResult::err(KernelError::InvalidArgument);
    }
    if let Err(e) = crate::mm::user::validate_user_write(
        args.arg2,
        crate::syscall::number::FS_STATVFS_SIZE,
    ) {
        return SyscallResult::err(e);
    }

    let info = match crate::fs::Vfs::statvfs(path) {
        Ok(i) => i,
        Err(e) => return SyscallResult::err(e),
    };

    // Write the 64-byte output buffer.
    // SAFETY: Validated above — out_ptr is in user space, mapped, and writable.
    unsafe {
        core::ptr::write_bytes(out_ptr, 0, 64);
        // block_size: u64 at offset 0
        core::ptr::write(out_ptr as *mut u64, info.block_size);
        // total_blocks: u64 at offset 8
        core::ptr::write(out_ptr.add(8) as *mut u64, info.total_blocks);
        // free_blocks: u64 at offset 16
        core::ptr::write(out_ptr.add(16) as *mut u64, info.free_blocks);
        // total_inodes: u64 at offset 24
        core::ptr::write(out_ptr.add(24) as *mut u64, info.total_inodes);
        // free_inodes: u64 at offset 32
        core::ptr::write(out_ptr.add(32) as *mut u64, info.free_inodes);
        // max_name_len: u64 at offset 40
        core::ptr::write(out_ptr.add(40) as *mut u64, info.max_name_len);
        // read_only: u8 at offset 48
        core::ptr::write(out_ptr.add(48), u8::from(info.read_only));
    }

    SyscallResult::ok(0)
}

// ---------------------------------------------------------------------------
// Filesystem — copy, append, ftruncate, dup, handle_path (642–646)
// ---------------------------------------------------------------------------

/// `SYS_FS_COPY` — copy a file from one path to another.
///
/// `arg0`: pointer to source path string.
/// `arg1`: source path length.
/// `arg2`: pointer to destination path string.
/// `arg3`: destination path length.
///
/// Returns: number of bytes copied on success.
pub fn sys_fs_copy(args: &SyscallArgs) -> SyscallResult {
    if let Err(e) = require_cap_type(
        crate::cap::ResourceType::File,
        crate::cap::Rights::CREATE,
    ) {
        return SyscallResult::err(e);
    }

    // Read source path.
    // SAFETY: Validated in read_user_path.
    let src = match unsafe { read_user_path(args.arg0, args.arg1) } {
        Ok(p) => p,
        Err(r) => return r,
    };

    // Read destination path (arg2=ptr, arg3=len).
    let dst_ptr = args.arg2 as *const u8;
    let dst_len = (args.arg3 as usize).min(4096);
    if dst_ptr.is_null() || dst_len == 0 {
        return SyscallResult::err(KernelError::InvalidArgument);
    }
    if let Err(e) = crate::mm::user::validate_user_read(args.arg2, dst_len) {
        return SyscallResult::err(e);
    }
    // SAFETY: Validated above — dst_ptr is in user space and mapped.
    let dst_bytes = unsafe { core::slice::from_raw_parts(dst_ptr, dst_len) };
    let dst = match core::str::from_utf8(dst_bytes) {
        Ok(s) => s,
        Err(_) => return SyscallResult::err(KernelError::InvalidArgument),
    };

    match crate::fs::Vfs::copy(src, dst) {
        Ok(bytes) => {
            #[allow(clippy::cast_possible_wrap)]
            let n = bytes as i64;
            SyscallResult::ok(n)
        }
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_FS_APPEND` — append data to a file.
///
/// `arg0`: pointer to path string.
/// `arg1`: path length.
/// `arg2`: pointer to data buffer.
/// `arg3`: data length.
pub fn sys_fs_append(args: &SyscallArgs) -> SyscallResult {
    if let Err(e) = require_cap_type(
        crate::cap::ResourceType::File,
        crate::cap::Rights::WRITE,
    ) {
        return SyscallResult::err(e);
    }

    // SAFETY: Validated in read_user_path.
    let path = match unsafe { read_user_path(args.arg0, args.arg1) } {
        Ok(p) => p,
        Err(r) => return r,
    };

    let data_ptr = args.arg2 as *const u8;
    let data_len = (args.arg3 as usize).min(64 * 1024);
    if data_ptr.is_null() && data_len > 0 {
        return SyscallResult::err(KernelError::InvalidArgument);
    }
    if data_len > 0 {
        if let Err(e) = crate::mm::user::validate_user_read(args.arg2, data_len) {
            return SyscallResult::err(e);
        }
    }

    let data = if data_len == 0 {
        &[]
    } else {
        // SAFETY: Validated above — data_ptr is in user space and mapped.
        unsafe { core::slice::from_raw_parts(data_ptr, data_len) }
    };

    match crate::fs::Vfs::append(path, data) {
        Ok(()) => SyscallResult::ok(0),
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_FS_FTRUNCATE` — truncate an open file handle.
///
/// `arg0`: file handle.
/// `arg1`: new size in bytes.
pub fn sys_fs_ftruncate(args: &SyscallArgs) -> SyscallResult {
    let handle = args.arg0;
    let size = args.arg1;

    match crate::fs::handle::ftruncate(handle, size) {
        Ok(()) => SyscallResult::ok(0),
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_FS_DUP` — duplicate an open file handle.
///
/// `arg0`: source file handle.
///
/// Returns: new file handle.
pub fn sys_fs_dup(args: &SyscallArgs) -> SyscallResult {
    let handle = args.arg0;

    match crate::fs::handle::dup(handle) {
        Ok(new_handle) => {
            #[allow(clippy::cast_possible_wrap)]
            let h = new_handle as i64;
            SyscallResult::ok(h)
        }
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_FS_HANDLE_PATH` — get the VFS path of an open file handle.
///
/// `arg0`: file handle.
/// `arg1`: pointer to output buffer.
/// `arg2`: buffer capacity.
///
/// Returns: path length in bytes (excluding null terminator).
pub fn sys_fs_handle_path(args: &SyscallArgs) -> SyscallResult {
    let handle = args.arg0;
    let out_ptr = args.arg1 as *mut u8;
    let out_cap = args.arg2 as usize;

    if out_ptr.is_null() || out_cap == 0 {
        return SyscallResult::err(KernelError::InvalidArgument);
    }
    if let Err(e) = crate::mm::user::validate_user_write(args.arg1, out_cap) {
        return SyscallResult::err(e);
    }

    let path = match crate::fs::handle::handle_path(handle) {
        Ok(p) => p,
        Err(e) => return SyscallResult::err(e),
    };

    let path_bytes = path.as_bytes();
    // Copy as much as fits, plus null terminator.
    let copy_len = path_bytes.len().min(out_cap.saturating_sub(1));

    // SAFETY: Validated above — out_ptr is in user space, mapped, writable.
    unsafe {
        core::ptr::copy_nonoverlapping(path_bytes.as_ptr(), out_ptr, copy_len);
        // Null terminator.
        core::ptr::write(out_ptr.add(copy_len), 0u8);
    }

    #[allow(clippy::cast_possible_wrap)]
    let len = copy_len as i64;
    SyscallResult::ok(len)
}

/// `SYS_FS_READDIR_AT` — paginated directory listing.
///
/// `arg0`: pointer to directory path string.
/// `arg1`: path length (bytes).
/// `arg2`: packed `(offset << 32) | count`.
/// `arg3`: pointer to output buffer.
/// `arg4`: buffer capacity.
///
/// Each entry is serialized as:
///   `u8 entry_type | u32 name_len | name bytes | u64 size`
///
/// Returns: packed `(total_entries << 32) | entries_written`.
#[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
pub fn sys_fs_readdir_at(args: &SyscallArgs) -> SyscallResult {
    // Validate path pointer.
    let path_len = args.arg1 as usize;
    if path_len == 0 || path_len > 4096 {
        return SyscallResult::err(KernelError::InvalidArgument);
    }
    if let Err(e) = crate::mm::user::validate_user_read(args.arg0, path_len) {
        return SyscallResult::err(e);
    }

    let path_bytes = unsafe { core::slice::from_raw_parts(args.arg0 as *const u8, path_len) };
    let path = match core::str::from_utf8(path_bytes) {
        Ok(s) => s,
        Err(_) => return SyscallResult::err(KernelError::InvalidArgument),
    };

    // Unpack offset and count from arg2.
    let offset = (args.arg2 >> 32) as usize;
    let count = (args.arg2 & 0xFFFF_FFFF) as usize;

    if count == 0 {
        return SyscallResult::err(KernelError::InvalidArgument);
    }

    // Validate output buffer.
    let out_ptr = args.arg3 as *mut u8;
    let out_cap = args.arg4 as usize;
    if out_ptr.is_null() || out_cap == 0 {
        return SyscallResult::err(KernelError::InvalidArgument);
    }
    if let Err(e) = crate::mm::user::validate_user_write(args.arg3, out_cap) {
        return SyscallResult::err(e);
    }

    // Perform paginated readdir.
    let (entries, total) = match crate::fs::Vfs::readdir_at(path, offset, count) {
        Ok(r) => r,
        Err(e) => return SyscallResult::err(e),
    };

    // Serialize entries into the output buffer.
    // Format: [u8 type][u32 name_len][name bytes][u64 size] per entry.
    let out_slice = unsafe { core::slice::from_raw_parts_mut(out_ptr, out_cap) };
    let mut pos = 0usize;
    let mut written = 0u32;

    for entry in &entries {
        let name_bytes = entry.name.as_bytes();
        // Each entry needs: 1 + 4 + name_len + 8 bytes.
        let entry_size = 1usize
            .saturating_add(4)
            .saturating_add(name_bytes.len())
            .saturating_add(8);

        if pos.saturating_add(entry_size) > out_cap {
            break; // Buffer full — stop writing (not an error).
        }

        // Entry type: 0=file, 1=dir, 2=symlink, 3=volume_label.
        out_slice[pos] = match entry.entry_type {
            crate::fs::vfs::EntryType::File => 0,
            crate::fs::vfs::EntryType::Directory => 1,
            crate::fs::vfs::EntryType::Symlink => 2,
            crate::fs::vfs::EntryType::VolumeLabel => 3,
        };
        pos = pos.saturating_add(1);

        // Name length (u32 LE).
        let name_len = name_bytes.len() as u32;
        out_slice[pos..pos.saturating_add(4)]
            .copy_from_slice(&name_len.to_le_bytes());
        pos = pos.saturating_add(4);

        // Name bytes.
        out_slice[pos..pos.saturating_add(name_bytes.len())]
            .copy_from_slice(name_bytes);
        pos = pos.saturating_add(name_bytes.len());

        // Size (u64 LE).
        out_slice[pos..pos.saturating_add(8)]
            .copy_from_slice(&entry.size.to_le_bytes());
        pos = pos.saturating_add(8);

        written = written.saturating_add(1);
    }

    // Pack result: (total << 32) | entries_written.
    let result = ((total as u64) << 32) | (written as u64);
    SyscallResult::ok(result as i64)
}

/// `SYS_FS_TMPFILE` — create a temporary file with no directory entry.
///
/// `arg0`: pointer to directory path string.
/// `arg1`: path length (bytes).
/// `arg2`: open flags.
///
/// Returns: file handle on success.
#[allow(clippy::cast_possible_wrap)]
pub fn sys_fs_tmpfile(args: &SyscallArgs) -> SyscallResult {
    let path_len = args.arg1 as usize;
    if path_len == 0 || path_len > 4096 {
        return SyscallResult::err(KernelError::InvalidArgument);
    }
    if let Err(e) = crate::mm::user::validate_user_read(args.arg0, path_len) {
        return SyscallResult::err(e);
    }

    let path_bytes = unsafe { core::slice::from_raw_parts(args.arg0 as *const u8, path_len) };
    let dir_path = match core::str::from_utf8(path_bytes) {
        Ok(s) => s,
        Err(_) => return SyscallResult::err(KernelError::InvalidArgument),
    };

    // Generate a unique temporary filename using the TSC for entropy.
    let tsc = unsafe { core::arch::x86_64::_rdtsc() };
    let tmp_name = alloc::format!("{}/.tmp_{:016x}", dir_path, tsc);

    // Create the file.
    if let Err(e) = crate::fs::Vfs::write_file(&tmp_name, &[]) {
        return SyscallResult::err(e);
    }

    // Open it as a handle.
    let flags = args.arg2 as u32;
    match crate::fs::handle::open(&tmp_name, crate::fs::handle::OpenFlags::from_bits(flags)) {
        Ok(handle) => SyscallResult::ok(handle as i64),
        Err(e) => {
            // Clean up the file if we can't open it.
            let _ = crate::fs::Vfs::remove(&tmp_name);
            SyscallResult::err(e)
        }
    }
    // Note: the file is NOT auto-deleted on close in this implementation.
    // True tmpfile (unlinked at creation) requires filesystem support
    // (ext4 O_TMPFILE).  For now, callers should delete after use.
}

/// `SYS_FS_FALLOCATE` — pre-allocate disk space.
///
/// `arg0`: pointer to path string.
/// `arg1`: path length (bytes).
/// `arg2`: size in bytes to pre-allocate.
pub fn sys_fs_fallocate(args: &SyscallArgs) -> SyscallResult {
    let path_len = args.arg1 as usize;
    if path_len == 0 || path_len > 4096 {
        return SyscallResult::err(KernelError::InvalidArgument);
    }
    if let Err(e) = crate::mm::user::validate_user_read(args.arg0, path_len) {
        return SyscallResult::err(e);
    }

    let path_bytes = unsafe { core::slice::from_raw_parts(args.arg0 as *const u8, path_len) };
    let path = match core::str::from_utf8(path_bytes) {
        Ok(s) => s,
        Err(_) => return SyscallResult::err(KernelError::InvalidArgument),
    };

    let size = args.arg2;
    match crate::fs::Vfs::fallocate(path, size) {
        Ok(()) => SyscallResult::ok(0),
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_FS_SEEK_DATA` — seek to next data region.
///
/// `arg0`: file handle.
/// `arg1`: offset to search from.
#[allow(clippy::cast_possible_wrap)]
pub fn sys_fs_seek_data(args: &SyscallArgs) -> SyscallResult {
    let handle = args.arg0;
    let offset = args.arg1;

    match crate::fs::handle::seek(handle, crate::fs::handle::SeekFrom::Data(offset)) {
        Ok(pos) => SyscallResult::ok(pos as i64),
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_FS_SEEK_HOLE` — seek to next hole in file.
///
/// `arg0`: file handle.
/// `arg1`: offset to search from.
#[allow(clippy::cast_possible_wrap)]
pub fn sys_fs_seek_hole(args: &SyscallArgs) -> SyscallResult {
    let handle = args.arg0;
    let offset = args.arg1;

    match crate::fs::handle::seek(handle, crate::fs::handle::SeekFrom::Hole(offset)) {
        Ok(pos) => SyscallResult::ok(pos as i64),
        Err(e) => SyscallResult::err(e),
    }
}

// ---------------------------------------------------------------------------
// Networking handlers (800–999)
// ---------------------------------------------------------------------------

/// `SYS_TCP_CONNECT` — open a TCP connection.
///
/// `arg0`: IPv4 address as u32 (network byte order).
/// `arg1`: remote port.
pub fn sys_tcp_connect(args: &SyscallArgs) -> SyscallResult {
    // Capability check: requires Socket capability with WRITE rights.
    if let Err(e) = require_cap_type(
        crate::cap::ResourceType::Socket,
        crate::cap::Rights::WRITE,
    ) {
        return SyscallResult::err(e);
    }

    use crate::net::interface::Ipv4Addr;

    #[allow(clippy::cast_possible_truncation)]
    let ip = Ipv4Addr::from_u32(args.arg0 as u32);
    #[allow(clippy::cast_possible_truncation)]
    let port = args.arg1 as u16;

    if port == 0 {
        return SyscallResult::err(KernelError::InvalidArgument);
    }

    // arg2 (optional): flags.  Bit 0 = non-blocking (return handle in
    // SYN_SENT state without waiting for handshake completion).
    let flags = args.arg2 as u32;
    const CONNECT_NONBLOCK: u32 = 1;

    // Use the calling task's network namespace for isolation.
    let ns = crate::sched::current_task_net_ns();

    if (flags & CONNECT_NONBLOCK) != 0 {
        // Non-blocking connect: return handle immediately in SYN_SENT.
        match crate::net::tcp::connect_start(ns, ip.into(), port) {
            Ok(handle) => {
                #[allow(clippy::cast_possible_wrap)]
                SyscallResult::ok(handle as i64)
            }
            Err(e) => SyscallResult::err(e),
        }
    } else {
        // Blocking connect (original behavior).
        match crate::net::tcp::connect(ns, ip.into(), port) {
            Ok(handle) => {
                #[allow(clippy::cast_possible_wrap)]
                SyscallResult::ok(handle as i64)
            }
            Err(e) => SyscallResult::err(e),
        }
    }
}

/// `SYS_TCP_SEND` — send data on a TCP socket.
///
/// `arg0`: socket handle.
/// `arg1`: pointer to data.
/// `arg2`: data length.
pub fn sys_tcp_send(args: &SyscallArgs) -> SyscallResult {
    // Capability check: requires Socket capability with WRITE rights.
    if let Err(e) = require_cap_type(
        crate::cap::ResourceType::Socket,
        crate::cap::Rights::WRITE,
    ) {
        return SyscallResult::err(e);
    }

    let handle = args.arg0 as usize;
    let ptr = args.arg1 as *const u8;
    let len = args.arg2 as usize;

    if ptr.is_null() && len > 0 {
        return SyscallResult::err(KernelError::InvalidArgument);
    }

    if len > 0 {
        if let Err(e) = crate::mm::user::validate_user_read(args.arg1, len) {
            return SyscallResult::err(e);
        }
    }

    let data = if len == 0 {
        &[]
    } else {
        // SAFETY: Validated above — ptr is in user space and mapped.
        unsafe { core::slice::from_raw_parts(ptr, len) }
    };

    match crate::net::tcp::send(handle, data) {
        Ok(sent) => {
            #[allow(clippy::cast_possible_wrap)]
            SyscallResult::ok(sent as i64)
        }
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_TCP_RECV` — receive data from a TCP socket (blocking).
///
/// `arg0`: socket handle.
/// `arg1`: pointer to receive buffer.
/// `arg2`: buffer capacity.
/// `arg3`: flags (optional, default 0).
///     - bit 1 (0x02): MSG_PEEK — copy data without consuming it.
///     - bit 6 (0x40): MSG_DONTWAIT — non-blocking (return immediately
///       with WouldBlock if no data available).
pub fn sys_tcp_recv(args: &SyscallArgs) -> SyscallResult {
    // Capability check: requires Socket capability with READ rights.
    if let Err(e) = require_cap_type(
        crate::cap::ResourceType::Socket,
        crate::cap::Rights::READ,
    ) {
        return SyscallResult::err(e);
    }

    let handle = args.arg0 as usize;
    let buf_ptr = args.arg1 as *mut u8;
    let buf_cap = args.arg2 as usize;
    let flags = args.arg3 as u32;

    // Flag constants (match POSIX MSG_* values).
    const MSG_PEEK: u32 = 0x02;
    const MSG_DONTWAIT: u32 = 0x40;

    if buf_ptr.is_null() && buf_cap > 0 {
        return SyscallResult::err(KernelError::InvalidArgument);
    }

    if buf_cap > 0 {
        if let Err(e) = crate::mm::user::validate_user_write(args.arg1, buf_cap) {
            return SyscallResult::err(e);
        }
    }

    let peek = (flags & MSG_PEEK) != 0;
    let dontwait = (flags & MSG_DONTWAIT) != 0;

    let data = if peek {
        // MSG_PEEK: copy data without consuming it.
        match crate::net::tcp::peek(handle, buf_cap) {
            Ok(d) => d,
            Err(e) => return SyscallResult::err(e),
        }
    } else if dontwait {
        // MSG_DONTWAIT: non-blocking read — return immediately.
        match crate::net::tcp::read_up_to(handle, buf_cap) {
            Ok(d) => d,
            Err(e) => return SyscallResult::err(e),
        }
    } else {
        // Normal blocking read with a generous timeout (~5 seconds).
        match crate::net::tcp::read_blocking(handle, 500, buf_cap) {
            Ok(d) => d,
            Err(e) => return SyscallResult::err(e),
        }
    };

    if data.is_empty() {
        // No data available.  Two cases:
        // 1. Connection closed (remote FIN received): return 0 (EOF).
        // 2. Connection still open but no data arrived (blocking timeout
        //    expired, or non-blocking with nothing queued): return WouldBlock
        //    so the POSIX layer can retry or propagate EAGAIN.
        //
        // The old code only returned WouldBlock for MSG_DONTWAIT, causing
        // blocking reads to spuriously report EOF when the 5-second kernel
        // poll expired on an active connection.
        if crate::net::tcp::is_remote_closed(handle) {
            return SyscallResult::ok(0);
        }
        return SyscallResult::err(KernelError::WouldBlock);
    }

    let copy_len = data.len().min(buf_cap);
    if copy_len > 0 {
        // SAFETY: Validated above — buf_ptr is in user space, mapped, and writable.
        unsafe {
            core::ptr::copy_nonoverlapping(
                data.as_ptr(),
                buf_ptr,
                copy_len,
            );
        }
    }
    #[allow(clippy::cast_possible_wrap)]
    SyscallResult::ok(copy_len as i64)
}

/// `SYS_TCP_CLOSE` — close a TCP socket.
///
/// `arg0`: socket handle.
pub fn sys_tcp_close(args: &SyscallArgs) -> SyscallResult {
    // Capability check: requires Socket capability with WRITE rights.
    if let Err(e) = require_cap_type(
        crate::cap::ResourceType::Socket,
        crate::cap::Rights::WRITE,
    ) {
        return SyscallResult::err(e);
    }

    let handle = args.arg0 as usize;

    match crate::net::tcp::close(handle) {
        Ok(()) => SyscallResult::ok(0),
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_TCP_ABORT` — abort a TCP connection by sending RST.
///
/// `arg0`: socket handle.
pub fn sys_tcp_abort(args: &SyscallArgs) -> SyscallResult {
    // Capability check: same as close.
    if let Err(e) = require_cap_type(
        crate::cap::ResourceType::Socket,
        crate::cap::Rights::WRITE,
    ) {
        return SyscallResult::err(e);
    }

    let handle = args.arg0 as usize;

    match crate::net::tcp::abort(handle) {
        Ok(()) => SyscallResult::ok(0),
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_TCP_PEER_ADDR` — get the remote peer address of a TCP connection.
///
/// `arg0`: connection handle.
/// `arg1`: pointer to 6-byte output buffer (4 bytes IP + 2 bytes port).
pub fn sys_tcp_peer_addr(args: &SyscallArgs) -> SyscallResult {
    // Capability check: requires Socket capability with READ rights.
    if let Err(e) = require_cap_type(
        crate::cap::ResourceType::Socket,
        crate::cap::Rights::READ,
    ) {
        return SyscallResult::err(e);
    }

    let handle = args.arg0 as usize;
    let out_ptr = args.arg1 as *mut u8;

    if out_ptr.is_null() {
        return SyscallResult::err(KernelError::InvalidArgument);
    }

    // Output: 4 bytes IP + 2 bytes port = 6 bytes.
    if let Err(e) = crate::mm::user::validate_user_write(args.arg1, 6) {
        return SyscallResult::err(e);
    }

    match crate::net::tcp::peer_addr(handle) {
        Some((ip, port)) => {
            // This syscall only supports IPv4 (4 bytes IP + 2 bytes port = 6 bytes).
            let v4 = match ip.as_v4() {
                Some(v4) => v4,
                None => return SyscallResult::err(KernelError::NotSupported),
            };
            // SAFETY: out_ptr validated for 6 bytes above.
            unsafe {
                core::ptr::copy_nonoverlapping(v4.0.as_ptr(), out_ptr, 4);
                let port_bytes = port.to_be_bytes();
                core::ptr::copy_nonoverlapping(port_bytes.as_ptr(), out_ptr.add(4), 2);
            }
            SyscallResult::ok(0)
        }
        None => SyscallResult::err(KernelError::InvalidArgument),
    }
}

/// `SYS_TCP_BIND` — bind a TCP listener to a local port.
///
/// `arg0`: local port (1–65535).
pub fn sys_tcp_bind(args: &SyscallArgs) -> SyscallResult {
    // Capability check: requires Socket capability with WRITE rights.
    if let Err(e) = require_cap_type(
        crate::cap::ResourceType::Socket,
        crate::cap::Rights::WRITE,
    ) {
        return SyscallResult::err(e);
    }

    #[allow(clippy::cast_possible_truncation)]
    let port = args.arg0 as u16;

    if port == 0 {
        return SyscallResult::err(KernelError::InvalidArgument);
    }

    let ns = crate::sched::current_task_net_ns();
    match crate::net::tcp::bind(ns, port) {
        Ok(handle) => {
            #[allow(clippy::cast_possible_wrap)]
            SyscallResult::ok(handle as i64)
        }
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_TCP_ACCEPT` — accept an incoming TCP connection.
///
/// `arg0`: listener handle.
/// `arg1` (optional): flags.  Bit 0 = non-blocking (return WouldBlock
///   instead of waiting if no pending connections).
pub fn sys_tcp_accept(args: &SyscallArgs) -> SyscallResult {
    // Capability check: requires Socket capability with READ rights.
    if let Err(e) = require_cap_type(
        crate::cap::ResourceType::Socket,
        crate::cap::Rights::READ,
    ) {
        return SyscallResult::err(e);
    }

    let listener_handle = args.arg0 as usize;
    let flags = args.arg1 as u32;
    const ACCEPT_NONBLOCK: u32 = 1;

    let result = if (flags & ACCEPT_NONBLOCK) != 0 {
        crate::net::tcp::try_accept(listener_handle)
    } else {
        crate::net::tcp::accept(listener_handle)
    };

    match result {
        Ok(conn_handle) => {
            #[allow(clippy::cast_possible_wrap)]
            SyscallResult::ok(conn_handle as i64)
        }
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_TCP_CLOSE_LISTENER` — close a TCP listener.
///
/// `arg0`: listener handle.
pub fn sys_tcp_close_listener(args: &SyscallArgs) -> SyscallResult {
    // Capability check: requires Socket capability with WRITE rights.
    if let Err(e) = require_cap_type(
        crate::cap::ResourceType::Socket,
        crate::cap::Rights::WRITE,
    ) {
        return SyscallResult::err(e);
    }

    let listener_handle = args.arg0 as usize;

    match crate::net::tcp::close_listener(listener_handle) {
        Ok(()) => SyscallResult::ok(0),
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_UDP_BIND` — bind a UDP socket to a local port.
///
/// `arg0`: local port.
pub fn sys_udp_bind(args: &SyscallArgs) -> SyscallResult {
    // Capability check: requires Socket capability with WRITE rights.
    if let Err(e) = require_cap_type(
        crate::cap::ResourceType::Socket,
        crate::cap::Rights::WRITE,
    ) {
        return SyscallResult::err(e);
    }

    #[allow(clippy::cast_possible_truncation)]
    let port = args.arg0 as u16;

    if port == 0 {
        return SyscallResult::err(KernelError::InvalidArgument);
    }

    let ns = crate::sched::current_task_net_ns();
    match crate::net::udp::bind(ns, port) {
        Ok(handle) => {
            #[allow(clippy::cast_possible_wrap)]
            SyscallResult::ok(handle as i64)
        }
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_UDP_SEND` — send a UDP datagram.
///
/// `arg0`: socket handle (for source port) or 0 for ephemeral.
/// `arg1`: destination IPv4 address (u32, network byte order).
/// `arg2`: destination port.
/// `arg3`: pointer to data.
/// `arg4`: data length.
pub fn sys_udp_send(args: &SyscallArgs) -> SyscallResult {
    // Capability check: requires Socket capability with WRITE rights.
    if let Err(e) = require_cap_type(
        crate::cap::ResourceType::Socket,
        crate::cap::Rights::WRITE,
    ) {
        return SyscallResult::err(e);
    }

    use crate::net::interface::Ipv4Addr;

    #[allow(clippy::cast_possible_truncation)]
    let _handle = args.arg0 as usize;
    #[allow(clippy::cast_possible_truncation)]
    let dst_ip = Ipv4Addr::from_u32(args.arg1 as u32);
    #[allow(clippy::cast_possible_truncation)]
    let dst_port = args.arg2 as u16;
    let data_ptr = args.arg3 as *const u8;
    let data_len = args.arg4 as usize;

    if dst_port == 0 {
        return SyscallResult::err(KernelError::InvalidArgument);
    }
    if data_ptr.is_null() && data_len > 0 {
        return SyscallResult::err(KernelError::InvalidArgument);
    }

    if data_len > 0 {
        if let Err(e) = crate::mm::user::validate_user_read(args.arg3, data_len) {
            return SyscallResult::err(e);
        }
    }

    let data = if data_len == 0 {
        &[]
    } else {
        // SAFETY: Validated above — data_ptr is in user space and mapped.
        unsafe { core::slice::from_raw_parts(data_ptr, data_len) }
    };

    // Look up the actual bound port from the socket handle.
    let src_port: u16 = match crate::net::udp::local_port(_handle) {
        Some(port) => port,
        None => {
            // Invalid or inactive handle — cannot send.
            return SyscallResult::err(KernelError::InvalidHandle);
        }
    };

    match crate::net::udp::send(src_port, dst_ip, dst_port, data) {
        Ok(()) => SyscallResult::ok(0),
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_UDP_RECV` — receive a UDP datagram (non-blocking).
///
/// `arg0`: socket handle.
/// `arg1`: pointer to receive buffer.
/// `arg2`: buffer capacity.
/// `arg3`: pointer to 6-byte source address output.
pub fn sys_udp_recv(args: &SyscallArgs) -> SyscallResult {
    // Capability check: requires Socket capability with READ rights.
    if let Err(e) = require_cap_type(
        crate::cap::ResourceType::Socket,
        crate::cap::Rights::READ,
    ) {
        return SyscallResult::err(e);
    }

    let handle = args.arg0 as usize;
    let buf_ptr = args.arg1 as *mut u8;
    let buf_cap = args.arg2 as usize;
    let src_ptr = args.arg3 as *mut u8;
    // arg4: flags —
    //   bit 1 (0x02) = MSG_PEEK (peek without consuming)
    //   bit 5 (0x20) = MSG_TRUNC (return real datagram size, not truncated)
    let flags = args.arg4 as u32;
    let peek = (flags & 0x02) != 0;
    let trunc = (flags & 0x20) != 0;

    if buf_ptr.is_null() && buf_cap > 0 {
        return SyscallResult::err(KernelError::InvalidArgument);
    }

    if buf_cap > 0 {
        if let Err(e) = crate::mm::user::validate_user_write(args.arg1, buf_cap) {
            return SyscallResult::err(e);
        }
    }
    // Source address output: 4 bytes IPv4 + 2 bytes port = 6 bytes.
    if !src_ptr.is_null() {
        if let Err(e) = crate::mm::user::validate_user_write(args.arg3, 6) {
            return SyscallResult::err(e);
        }
    }

    // Either peek (clone without removing) or consume the front datagram.
    let datagram_opt = if peek {
        crate::net::udp::peek(handle)
    } else {
        crate::net::udp::recv(handle)
    };

    match datagram_opt {
        Some(datagram) => {
            let real_len = datagram.data.len();
            let copy_len = real_len.min(buf_cap);
            if copy_len > 0 && !buf_ptr.is_null() {
                // SAFETY: Validated above — buf_ptr is in user space, mapped, and writable.
                unsafe {
                    core::ptr::copy_nonoverlapping(
                        datagram.data.as_ptr(),
                        buf_ptr,
                        copy_len,
                    );
                }
            }

            // Write source address info if pointer provided.
            if !src_ptr.is_null() {
                // SAFETY: Validated above — src_ptr is in user space, mapped, and writable.
                unsafe {
                    // IPv4 address (4 bytes).
                    core::ptr::copy_nonoverlapping(
                        datagram.src_ip.0.as_ptr(),
                        src_ptr,
                        4,
                    );
                    // Source port (u16 LE, 2 bytes).
                    let port_bytes = datagram.src_port.to_le_bytes();
                    core::ptr::copy_nonoverlapping(
                        port_bytes.as_ptr(),
                        src_ptr.add(4),
                        2,
                    );
                }
            }

            // MSG_TRUNC: return real datagram size even if truncated.
            // Without MSG_TRUNC: return number of bytes copied.
            let ret_len = if trunc { real_len } else { copy_len };
            #[allow(clippy::cast_possible_wrap)]
            SyscallResult::ok(ret_len as i64)
        }
        None => SyscallResult::err(KernelError::WouldBlock),
    }
}

/// `SYS_UDP_CLOSE` — close a UDP socket.
///
/// `arg0`: socket handle.
pub fn sys_udp_close(args: &SyscallArgs) -> SyscallResult {
    // Capability check: requires Socket capability with WRITE rights.
    if let Err(e) = require_cap_type(
        crate::cap::ResourceType::Socket,
        crate::cap::Rights::WRITE,
    ) {
        return SyscallResult::err(e);
    }

    let handle = args.arg0 as usize;
    crate::net::udp::close(handle);
    SyscallResult::ok(0)
}

/// `SYS_UDP_CONNECT` — set connected peer filter for a UDP socket.
///
/// `arg0`: socket handle.
/// `arg1`: peer IPv4 address (u32, network byte order).
/// `arg2`: peer port (u16, host byte order).
///
/// Pass ip=0, port=0 to disconnect.
pub fn sys_udp_connect(args: &SyscallArgs) -> SyscallResult {
    if let Err(e) = require_cap_type(
        crate::cap::ResourceType::Socket,
        crate::cap::Rights::WRITE,
    ) {
        return SyscallResult::err(e);
    }

    let handle = args.arg0 as usize;
    let ip_nbo = args.arg1 as u32;
    let port = args.arg2 as u16;

    let peer_ip = crate::net::interface::Ipv4Addr(ip_nbo.to_be_bytes());
    match crate::net::udp::connect(handle, peer_ip, port) {
        Ok(()) => SyscallResult::ok(0),
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_UDP_LOCAL_PORT` — query the local port of a UDP socket.
pub fn sys_udp_local_port(args: &SyscallArgs) -> SyscallResult {
    let handle = args.arg0 as usize;
    match crate::net::udp::local_port(handle) {
        Some(port) => SyscallResult::ok(port as i64),
        None => SyscallResult::err(KernelError::InvalidArgument),
    }
}

/// `SYS_UDP_MCAST_JOIN` — join a multicast group on a UDP socket.
///
/// `arg0`: socket handle.
/// `arg1`: multicast group address (u32 in network byte order).
pub fn sys_udp_mcast_join(args: &SyscallArgs) -> SyscallResult {
    // Capability check: requires Socket capability with WRITE rights.
    if let Err(e) = require_cap_type(
        crate::cap::ResourceType::Socket,
        crate::cap::Rights::WRITE,
    ) {
        return SyscallResult::err(e);
    }

    let handle = args.arg0 as usize;
    let group = crate::net::interface::Ipv4Addr::from_u32(args.arg1 as u32);
    match crate::net::udp::join_group(handle, group) {
        Ok(()) => SyscallResult::ok(0),
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_UDP_MCAST_LEAVE` — leave a multicast group on a UDP socket.
///
/// `arg0`: socket handle.
/// `arg1`: multicast group address (u32 in network byte order).
pub fn sys_udp_mcast_leave(args: &SyscallArgs) -> SyscallResult {
    // Capability check: requires Socket capability with WRITE rights.
    if let Err(e) = require_cap_type(
        crate::cap::ResourceType::Socket,
        crate::cap::Rights::WRITE,
    ) {
        return SyscallResult::err(e);
    }

    let handle = args.arg0 as usize;
    let group = crate::net::interface::Ipv4Addr::from_u32(args.arg1 as u32);
    match crate::net::udp::leave_group(handle, group) {
        Ok(()) => SyscallResult::ok(0),
        Err(e) => SyscallResult::err(e),
    }
}

// ---------------------------------------------------------------------------
// Thread management handlers (510–519)
// ---------------------------------------------------------------------------

/// `SYS_THREAD_CREATE` — create a new userspace thread in the calling process.
///
/// `arg0`: entry point address (ring 3 RIP).
/// `arg1`: stack pointer (ring 3 RSP).
/// `arg2`: priority (0–31, or `u64::MAX` for default).
///
/// Returns: new thread's task ID.
pub fn sys_thread_create(args: &SyscallArgs) -> SyscallResult {
    use crate::proc::thread;
    use crate::sched::task::DEFAULT_PRIORITY;

    let entry_rip = args.arg0;
    let user_rsp = args.arg1;
    let raw_priority = args.arg2;

    // Resolve priority: u64::MAX means default.
    let priority = if raw_priority == u64::MAX {
        DEFAULT_PRIORITY
    } else if raw_priority > 31 {
        return SyscallResult::err(KernelError::InvalidArgument);
    } else {
        #[allow(clippy::cast_possible_truncation)]
        { raw_priority as u8 }
    };

    // Get the calling process's PID.
    let task_id = sched::current_task_id();
    let pid = match thread::owner_process(task_id) {
        Some(pid) if pid != 0 => pid,
        _ => {
            serial_println!(
                "[thread] SYS_THREAD_CREATE: task {} has no owning process",
                task_id
            );
            return SyscallResult::err(KernelError::NoSuchProcess);
        }
    };

    match thread::spawn_user(pid, b"user-thread", priority, entry_rip, user_rsp) {
        Ok(new_task_id) => {
            #[allow(clippy::cast_possible_wrap)]
            SyscallResult::ok(new_task_id as i64)
        }
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_THREAD_EXIT` — exit the current thread with an exit value.
///
/// `arg0`: exit value (i64).
///
/// Does not return.
pub fn sys_thread_exit(args: &SyscallArgs) -> SyscallResult {
    use crate::proc::thread;

    #[allow(clippy::cast_possible_wrap)]
    let exit_value = args.arg0 as i64;

    // This function never returns — it terminates the calling thread.
    thread::thread_exit_with_value(exit_value);

    // Unreachable.
    // SyscallResult::ok(0)
}

/// `SYS_THREAD_JOIN` — wait for a thread to exit and get its exit value.
///
/// `arg0`: task ID of the thread to wait for.
///
/// Returns: exit value of the target thread.
pub fn sys_thread_join(args: &SyscallArgs) -> SyscallResult {
    use crate::proc::thread;

    let target_task = args.arg0;

    match thread::join(target_task) {
        Ok(exit_value) => SyscallResult::ok(exit_value),
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_THREAD_SUSPEND` — pause a thread.
///
/// `arg0`: task ID of the thread to suspend.
///
/// Returns: 0 on success.
pub fn sys_thread_suspend(args: &SyscallArgs) -> SyscallResult {
    use crate::proc::thread;

    let target_task = args.arg0;

    // Get the calling process's PID for authority check.
    let caller_task = sched::current_task_id();
    let caller_pid = thread::owner_process(caller_task).unwrap_or(0);

    // Can't suspend the idle task.
    if target_task == 0 {
        return SyscallResult::err(KernelError::InvalidArgument);
    }

    // Verify the target belongs to the same process (or caller is PID 0).
    if caller_pid != 0 {
        let target_pid = thread::owner_process(target_task);
        if target_pid != Some(caller_pid) {
            return SyscallResult::err(KernelError::PermissionDenied);
        }
    }

    if sched::suspend(target_task) {
        SyscallResult::ok(0)
    } else {
        SyscallResult::err(KernelError::InvalidArgument)
    }
}

/// `SYS_THREAD_RESUME` — resume a suspended thread.
///
/// `arg0`: task ID of the thread to resume.
///
/// Returns: 0 on success.
pub fn sys_thread_resume(args: &SyscallArgs) -> SyscallResult {
    use crate::proc::thread;

    let target_task = args.arg0;

    // Get the calling process's PID for authority check.
    let caller_task = sched::current_task_id();
    let caller_pid = thread::owner_process(caller_task).unwrap_or(0);

    // Verify the target belongs to the same process (or caller is PID 0).
    if caller_pid != 0 {
        let target_pid = thread::owner_process(target_task);
        if target_pid != Some(caller_pid) {
            return SyscallResult::err(KernelError::PermissionDenied);
        }
    }

    if sched::resume(target_task) {
        SyscallResult::ok(0)
    } else {
        SyscallResult::err(KernelError::InvalidArgument)
    }
}

/// `SYS_THREAD_SET_PRIORITY` — change a thread's scheduling priority.
///
/// `arg0`: task ID (0 = current thread).
/// `arg1`: new priority (0–31).
///
/// Returns: old priority on success.
pub fn sys_thread_set_priority(args: &SyscallArgs) -> SyscallResult {
    use crate::proc::thread;

    let target_task = if args.arg0 == 0 {
        sched::current_task_id()
    } else {
        args.arg0
    };

    let new_priority = args.arg1;
    if new_priority > 31 {
        return SyscallResult::err(KernelError::InvalidArgument);
    }

    // Get the calling process's PID for authority check.
    let caller_task = sched::current_task_id();
    let caller_pid = thread::owner_process(caller_task).unwrap_or(0);

    // Verify the target belongs to the same process (or caller is PID 0).
    if caller_pid != 0 && target_task != caller_task {
        let target_pid = thread::owner_process(target_task);
        if target_pid != Some(caller_pid) {
            return SyscallResult::err(KernelError::PermissionDenied);
        }
    }

    #[allow(clippy::cast_possible_truncation)]
    match sched::set_priority(target_task, new_priority as u8) {
        Some(old) => {
            #[allow(clippy::cast_lossless)]
            SyscallResult::ok(old as i64)
        }
        None => SyscallResult::err(KernelError::NoSuchProcess),
    }
}

// ---------------------------------------------------------------------------
// io_ring handlers (260–269)
// ---------------------------------------------------------------------------

/// `SYS_IO_RING_SETUP` — create a new io_ring.
///
/// `arg0`: number of submission queue entries.
/// `arg1`: number of completion queue entries.
///
/// Returns: ring handle in `value`, header virtual address in `value2`.
pub fn sys_io_ring_setup(args: &SyscallArgs) -> SyscallResult {
    use crate::ipc::io_ring;

    #[allow(clippy::cast_possible_truncation)]
    let sq_entries = args.arg0 as u32;
    #[allow(clippy::cast_possible_truncation)]
    let cq_entries = args.arg1 as u32;

    match io_ring::setup(sq_entries, cq_entries) {
        Ok((handle, header_virt, _phys_frames)) => {
            // Return handle in rax, header vaddr in rdx.
            #[allow(clippy::cast_possible_wrap)]
            SyscallResult::ok2(handle as i64, header_virt as i64)
        }
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_IO_RING_ENTER` — process io_ring submissions.
///
/// `arg0`: ring handle.
/// `arg1`: maximum number of SQEs to process (0 = all pending).
///
/// Returns: number of SQEs processed.
pub fn sys_io_ring_enter(args: &SyscallArgs) -> SyscallResult {
    use crate::ipc::io_ring;

    let ring_handle = args.arg0;
    #[allow(clippy::cast_possible_truncation)]
    let to_submit = args.arg1 as u32;

    match io_ring::enter(ring_handle, to_submit) {
        Ok(processed) => {
            #[allow(clippy::cast_lossless)]
            SyscallResult::ok(processed as i64)
        }
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_IO_RING_DESTROY` — destroy an io_ring and free resources.
///
/// `arg0`: ring handle.
///
/// Returns: 0 on success.
pub fn sys_io_ring_destroy(args: &SyscallArgs) -> SyscallResult {
    use crate::ipc::io_ring;

    let ring_handle = args.arg0;

    match io_ring::destroy(ring_handle) {
        Ok(()) => SyscallResult::ok(0),
        Err(e) => SyscallResult::err(e),
    }
}

// ---------------------------------------------------------------------------
// IPC semaphore syscalls
// ---------------------------------------------------------------------------

/// `SYS_SEM_CREATE` — create a new IPC semaphore.
///
/// `arg0`: initial count.
/// `arg1`: maximum count (0 = default max).
///
/// Returns: semaphore handle.
pub fn sys_sem_create(args: &SyscallArgs) -> SyscallResult {
    use crate::ipc::semaphore;

    let initial = args.arg0;
    let max_count = args.arg1;

    let handle = semaphore::create(initial, max_count);

    #[allow(clippy::cast_possible_wrap)]
    SyscallResult::ok(handle.raw() as i64)
}

/// `SYS_SEM_SIGNAL` — signal (release) a semaphore.
///
/// `arg0`: semaphore handle.
/// `arg1`: count to add.
pub fn sys_sem_signal(args: &SyscallArgs) -> SyscallResult {
    use crate::ipc::semaphore::{self, SemHandle};

    let handle = SemHandle::from_raw(args.arg0);
    let count = args.arg1;

    match semaphore::signal(handle, count) {
        Ok(()) => SyscallResult::ok(0),
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_SEM_WAIT` — wait (acquire) a semaphore (blocking).
///
/// `arg0`: semaphore handle.
pub fn sys_sem_wait(args: &SyscallArgs) -> SyscallResult {
    use crate::ipc::semaphore::{self, SemHandle};

    let handle = SemHandle::from_raw(args.arg0);

    match semaphore::wait(handle) {
        Ok(()) => SyscallResult::ok(0),
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_SEM_TRY_WAIT` — try-wait (non-blocking acquire).
///
/// `arg0`: semaphore handle.
pub fn sys_sem_try_wait(args: &SyscallArgs) -> SyscallResult {
    use crate::ipc::semaphore::{self, SemHandle};

    let handle = SemHandle::from_raw(args.arg0);

    match semaphore::try_wait(handle) {
        Ok(()) => SyscallResult::ok(0),
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_SEM_CLOSE` — close (destroy) a semaphore.
///
/// `arg0`: semaphore handle.
pub fn sys_sem_close(args: &SyscallArgs) -> SyscallResult {
    use crate::ipc::semaphore::{self, SemHandle};

    let handle = SemHandle::from_raw(args.arg0);
    semaphore::close(handle);
    SyscallResult::ok(0)
}

/// `SYS_SEM_WAIT_TIMEOUT` — wait (acquire) with a deadline.
///
/// `arg0`: semaphore handle.
/// `arg1`: timeout in nanoseconds (0 = non-blocking try).
///
/// Returns: 0 on success, `TimedOut` if deadline expires.
pub fn sys_sem_wait_timeout(args: &SyscallArgs) -> SyscallResult {
    use crate::ipc::semaphore::{self, SemHandle};

    let handle = SemHandle::from_raw(args.arg0);
    let timeout_ns = args.arg1;

    match semaphore::wait_timeout(handle, timeout_ns) {
        Ok(()) => SyscallResult::ok(0),
        Err(e) => SyscallResult::err(e),
    }
}

// ---------------------------------------------------------------------------
// Service registry handlers (280–289)
// ---------------------------------------------------------------------------

/// `SYS_SERVICE_REGISTER` — register a named service.
///
/// `arg0`: pointer to service name (bytes).
/// `arg1`: name length.
///
/// Returns: listener handle.
pub fn sys_service_register(args: &SyscallArgs) -> SyscallResult {
    // Capability check: requires Service capability with WRITE rights.
    // This prevents untrusted processes from squatting on service names.
    if let Err(e) = require_cap_type(
        crate::cap::ResourceType::Service,
        crate::cap::Rights::WRITE,
    ) {
        return SyscallResult::err(e);
    }

    let name_ptr = args.arg0 as *const u8;
    let name_len = args.arg1 as usize;

    if name_ptr.is_null() || name_len == 0 {
        return SyscallResult::err(KernelError::InvalidArgument);
    }

    if let Err(e) = crate::mm::user::validate_user_read(args.arg0, name_len) {
        return SyscallResult::err(e);
    }

    // SAFETY: Validated above — ptr is in user space, mapped, readable.
    let name = unsafe { core::slice::from_raw_parts(name_ptr, name_len) };

    match service::register(name) {
        Ok(listener) => {
            #[allow(clippy::cast_possible_wrap)]
            SyscallResult::ok(listener.raw() as i64)
        }
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_SERVICE_CONNECT` — connect to a named service.
///
/// `arg0`: pointer to service name (bytes).
/// `arg1`: name length.
///
/// Returns: client channel handle.
pub fn sys_service_connect(args: &SyscallArgs) -> SyscallResult {
    let name_ptr = args.arg0 as *const u8;
    let name_len = args.arg1 as usize;

    if name_ptr.is_null() || name_len == 0 {
        return SyscallResult::err(KernelError::InvalidArgument);
    }

    if let Err(e) = crate::mm::user::validate_user_read(args.arg0, name_len) {
        return SyscallResult::err(e);
    }

    // SAFETY: Validated above.
    let name = unsafe { core::slice::from_raw_parts(name_ptr, name_len) };

    match service::connect(name) {
        Ok(handle) => {
            #[allow(clippy::cast_possible_wrap)]
            SyscallResult::ok(handle.raw() as i64)
        }
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_SERVICE_ACCEPT` — accept a connection (blocking).
///
/// `arg0`: listener handle.
///
/// Returns: server-side channel handle.
pub fn sys_service_accept(args: &SyscallArgs) -> SyscallResult {
    let listener = ServiceListenerHandle::from_raw(args.arg0);

    match service::accept(listener) {
        Ok(handle) => {
            #[allow(clippy::cast_possible_wrap)]
            SyscallResult::ok(handle.raw() as i64)
        }
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_SERVICE_TRY_ACCEPT` — accept a connection (non-blocking).
///
/// `arg0`: listener handle.
///
/// Returns: server-side channel handle, or `WouldBlock`.
pub fn sys_service_try_accept(args: &SyscallArgs) -> SyscallResult {
    let listener = ServiceListenerHandle::from_raw(args.arg0);

    match service::try_accept(listener) {
        Ok(Some(handle)) => {
            #[allow(clippy::cast_possible_wrap)]
            SyscallResult::ok(handle.raw() as i64)
        }
        Ok(None) => SyscallResult::err(KernelError::WouldBlock),
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_SERVICE_ACCEPT_TIMEOUT` — accept with timeout.
///
/// `arg0`: listener handle.
/// `arg1`: timeout in nanoseconds.
///
/// Returns: server-side channel handle, or `TimedOut`.
pub fn sys_service_accept_timeout(args: &SyscallArgs) -> SyscallResult {
    let listener = ServiceListenerHandle::from_raw(args.arg0);
    let timeout_ns = args.arg1;

    match service::accept_timeout(listener, timeout_ns) {
        Ok(handle) => {
            #[allow(clippy::cast_possible_wrap)]
            SyscallResult::ok(handle.raw() as i64)
        }
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_SERVICE_UNREGISTER` — unregister a service.
///
/// `arg0`: listener handle.
///
/// Returns: 0 on success.
pub fn sys_service_unregister(args: &SyscallArgs) -> SyscallResult {
    let listener = ServiceListenerHandle::from_raw(args.arg0);

    match service::unregister(listener) {
        Ok(()) => SyscallResult::ok(0),
        Err(e) => SyscallResult::err(e),
    }
}

// ---------------------------------------------------------------------------
// Namespace syscalls (290–295)
// ---------------------------------------------------------------------------

/// `SYS_NS_CREATE` — create a new namespace.
///
/// `arg0`: clone_from (namespace ID to copy rules from, 0 = empty).
///
/// Returns: new namespace ID.
pub fn sys_ns_create(args: &SyscallArgs) -> SyscallResult {
    // Capability check: requires Namespace capability with WRITE rights.
    if let Err(e) = require_cap_type(
        crate::cap::ResourceType::Namespace,
        crate::cap::Rights::WRITE,
    ) {
        return SyscallResult::err(e);
    }

    let clone_from = args.arg0;

    match crate::ipc::namespace::create(clone_from) {
        Ok(id) => SyscallResult::ok(id as i64),
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_NS_BIND` — add a path remapping rule to a namespace.
///
/// `arg0`: namespace ID.
/// `arg1`: pointer to source prefix string.
/// `arg2`: source prefix length.
/// `arg3`: pointer to target prefix string.
/// `arg4`: target prefix length.
pub fn sys_ns_bind(args: &SyscallArgs) -> SyscallResult {
    let ns_id = args.arg0;
    let src_ptr = args.arg1 as *const u8;
    let src_len = args.arg2 as usize;
    let tgt_ptr = args.arg3 as *const u8;
    let tgt_len = args.arg4 as usize;

    if src_ptr.is_null() || src_len == 0 || tgt_ptr.is_null() || tgt_len == 0 {
        return SyscallResult::err(KernelError::InvalidArgument);
    }

    let safe_src_len = src_len.min(1024);
    let safe_tgt_len = tgt_len.min(1024);

    if let Err(e) = crate::mm::user::validate_user_read(args.arg1, safe_src_len) {
        return SyscallResult::err(e);
    }
    if let Err(e) = crate::mm::user::validate_user_read(args.arg3, safe_tgt_len) {
        return SyscallResult::err(e);
    }

    // SAFETY: Validated user pointers above.
    let src_bytes = unsafe { core::slice::from_raw_parts(src_ptr, safe_src_len) };
    let tgt_bytes = unsafe { core::slice::from_raw_parts(tgt_ptr, safe_tgt_len) };

    let src_str = match core::str::from_utf8(src_bytes) {
        Ok(s) => s,
        Err(_) => return SyscallResult::err(KernelError::InvalidArgument),
    };
    let tgt_str = match core::str::from_utf8(tgt_bytes) {
        Ok(s) => s,
        Err(_) => return SyscallResult::err(KernelError::InvalidArgument),
    };

    match crate::ipc::namespace::bind(ns_id, src_str, tgt_str) {
        Ok(()) => SyscallResult::ok(0),
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_NS_UNBIND` — remove a bind rule from a namespace.
///
/// `arg0`: namespace ID.
/// `arg1`: pointer to source prefix string.
/// `arg2`: source prefix length.
pub fn sys_ns_unbind(args: &SyscallArgs) -> SyscallResult {
    let ns_id = args.arg0;
    let src_ptr = args.arg1 as *const u8;
    let src_len = args.arg2 as usize;

    if src_ptr.is_null() || src_len == 0 {
        return SyscallResult::err(KernelError::InvalidArgument);
    }

    let safe_len = src_len.min(1024);
    if let Err(e) = crate::mm::user::validate_user_read(args.arg1, safe_len) {
        return SyscallResult::err(e);
    }

    // SAFETY: Validated above.
    let bytes = unsafe { core::slice::from_raw_parts(src_ptr, safe_len) };
    let prefix = match core::str::from_utf8(bytes) {
        Ok(s) => s,
        Err(_) => return SyscallResult::err(KernelError::InvalidArgument),
    };

    match crate::ipc::namespace::unbind(ns_id, prefix) {
        Ok(()) => SyscallResult::ok(0),
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_NS_HIDE` — block access to a path prefix in a namespace.
///
/// `arg0`: namespace ID.
/// `arg1`: pointer to prefix string.
/// `arg2`: prefix length.
pub fn sys_ns_hide(args: &SyscallArgs) -> SyscallResult {
    let ns_id = args.arg0;
    let ptr = args.arg1 as *const u8;
    let len = args.arg2 as usize;

    if ptr.is_null() || len == 0 {
        return SyscallResult::err(KernelError::InvalidArgument);
    }

    let safe_len = len.min(1024);
    if let Err(e) = crate::mm::user::validate_user_read(args.arg1, safe_len) {
        return SyscallResult::err(e);
    }

    // SAFETY: Validated above.
    let bytes = unsafe { core::slice::from_raw_parts(ptr, safe_len) };
    let prefix = match core::str::from_utf8(bytes) {
        Ok(s) => s,
        Err(_) => return SyscallResult::err(KernelError::InvalidArgument),
    };

    match crate::ipc::namespace::hide(ns_id, prefix) {
        Ok(()) => SyscallResult::ok(0),
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_NS_ATTACH` — attach a process to a namespace.
///
/// `arg0`: process ID (0 = calling process).
/// `arg1`: namespace ID (0 = root/default).
pub fn sys_ns_attach(args: &SyscallArgs) -> SyscallResult {
    // Capability check: requires Namespace capability with WRITE rights.
    // Without this, a process cannot change its own or another's namespace.
    if let Err(e) = require_cap_type(
        crate::cap::ResourceType::Namespace,
        crate::cap::Rights::WRITE,
    ) {
        return SyscallResult::err(e);
    }

    let pid = if args.arg0 == 0 {
        // Use the calling process's PID.
        let task_id = sched::current_task_id();
        crate::proc::thread::owner_process(task_id).unwrap_or(0)
    } else {
        args.arg0
    };
    let ns_id = args.arg1;

    match crate::ipc::namespace::attach(pid, ns_id) {
        Ok(()) => SyscallResult::ok(0),
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_NS_QUERY` — query which namespace a process belongs to.
///
/// `arg0`: process ID (0 = calling process).
///
/// Returns: namespace ID (0 = root).
pub fn sys_ns_query(args: &SyscallArgs) -> SyscallResult {
    let pid = if args.arg0 == 0 {
        let task_id = sched::current_task_id();
        crate::proc::thread::owner_process(task_id).unwrap_or(0)
    } else {
        args.arg0
    };

    let ns_id = crate::ipc::namespace::query(pid);
    SyscallResult::ok(ns_id as i64)
}

/// `SYS_DNS_RESOLVE` — resolve a hostname to an IPv4 address.
///
/// `arg0`: pointer to hostname string.
/// `arg1`: hostname length.
/// `arg2`: pointer to 4-byte output buffer for IPv4 address.
pub fn sys_dns_resolve(args: &SyscallArgs) -> SyscallResult {
    // Capability check: requires Socket capability with READ rights
    // (DNS is a network operation).
    if let Err(e) = require_cap_type(
        crate::cap::ResourceType::Socket,
        crate::cap::Rights::READ,
    ) {
        return SyscallResult::err(e);
    }

    let name_ptr = args.arg0 as *const u8;
    let name_len = args.arg1 as usize;
    let out_ptr = args.arg2 as *mut u8;

    if name_ptr.is_null() || name_len == 0 || out_ptr.is_null() {
        return SyscallResult::err(KernelError::InvalidArgument);
    }

    let safe_name_len = name_len.min(253);
    if let Err(e) = crate::mm::user::validate_user_read(args.arg0, safe_name_len) {
        return SyscallResult::err(e);
    }
    // IPv4 address output = 4 bytes.
    if let Err(e) = crate::mm::user::validate_user_write(args.arg2, 4) {
        return SyscallResult::err(e);
    }

    // SAFETY: Validated above — name_ptr is in user space and mapped.
    let name_bytes = unsafe { core::slice::from_raw_parts(name_ptr, safe_name_len) };
    let name = match core::str::from_utf8(name_bytes) {
        Ok(s) => s,
        Err(_) => return SyscallResult::err(KernelError::InvalidArgument),
    };

    match crate::net::dns::resolve(name) {
        Ok(ip) => {
            // SAFETY: out_ptr is valid for 4 bytes.
            unsafe {
                core::ptr::copy_nonoverlapping(ip.0.as_ptr(), out_ptr, 4);
            }
            SyscallResult::ok(0)
        }
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_DNS_REVERSE_RESOLVE` — reverse-resolve an IPv4 address to a hostname.
///
/// `arg0`: IPv4 address as a 32-bit integer in network byte order.
/// `arg1`: pointer to output buffer for hostname string.
/// `arg2`: size of the output buffer in bytes.
///
/// Returns: number of bytes written (hostname length) on success,
/// negative error on failure.
pub fn sys_dns_reverse_resolve(args: &SyscallArgs) -> SyscallResult {
    // Capability check: same as forward DNS.
    if let Err(e) = require_cap_type(
        crate::cap::ResourceType::Socket,
        crate::cap::Rights::READ,
    ) {
        return SyscallResult::err(e);
    }

    let ip_u32 = args.arg0 as u32;
    let out_ptr = args.arg1 as *mut u8;
    let out_len = args.arg2 as usize;

    if out_ptr.is_null() || out_len == 0 {
        return SyscallResult::err(KernelError::InvalidArgument);
    }

    // Cap output to a reasonable hostname length.
    let safe_out_len = out_len.min(253);
    if let Err(e) = crate::mm::user::validate_user_write(args.arg1, safe_out_len) {
        return SyscallResult::err(e);
    }

    // Reconstruct Ipv4Addr from network byte order u32.
    let ip = crate::net::interface::Ipv4Addr::from_u32(ip_u32);

    match crate::net::dns::reverse_resolve(ip) {
        Ok(hostname) => {
            let copy_len = hostname.len().min(safe_out_len);
            if copy_len == 0 {
                return SyscallResult::err(KernelError::InvalidArgument);
            }
            // SAFETY: out_ptr validated for safe_out_len bytes above.
            unsafe {
                core::ptr::copy_nonoverlapping(
                    hostname.as_bytes().as_ptr(),
                    out_ptr,
                    copy_len,
                );
            }
            SyscallResult::ok(copy_len as i64)
        }
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_NET_STAT` — query network interface statistics.
///
/// `arg0`: pointer to 48-byte output buffer.
///
/// Returns 0 on success.
pub fn sys_net_stat(args: &SyscallArgs) -> SyscallResult {
    let out_ptr = args.arg0 as *mut u8;

    if out_ptr.is_null() {
        return SyscallResult::err(KernelError::InvalidArgument);
    }

    // Output: 6 × u64 = 48 bytes.
    const STAT_SIZE: usize = 48;
    if let Err(e) = crate::mm::user::validate_user_write(args.arg0, STAT_SIZE) {
        return SyscallResult::err(e);
    }

    let stats = crate::net::interface::stats();

    // SAFETY: out_ptr validated for STAT_SIZE bytes above.
    unsafe {
        let buf = core::slice::from_raw_parts_mut(out_ptr, STAT_SIZE);
        buf[0..8].copy_from_slice(&stats.tx_bytes.to_le_bytes());
        buf[8..16].copy_from_slice(&stats.tx_packets.to_le_bytes());
        buf[16..24].copy_from_slice(&stats.tx_errors.to_le_bytes());
        buf[24..32].copy_from_slice(&stats.rx_bytes.to_le_bytes());
        buf[32..40].copy_from_slice(&stats.rx_packets.to_le_bytes());
        buf[40..48].copy_from_slice(&stats.rx_drops.to_le_bytes());
    }

    SyscallResult::ok(0)
}

/// `SYS_ICMP_PING` — send an ICMP Echo Request.
///
/// `arg0`: IPv4 address (network byte order u32).
///
/// Returns the sequence number on success.
pub fn sys_icmp_ping(args: &SyscallArgs) -> SyscallResult {
    // Capability check: requires Socket capability with WRITE rights.
    if let Err(e) = require_cap_type(
        crate::cap::ResourceType::Socket,
        crate::cap::Rights::WRITE,
    ) {
        return SyscallResult::err(e);
    }

    let ip = crate::net::interface::Ipv4Addr::from_u32(args.arg0 as u32);
    match crate::net::icmp::ping(ip) {
        Ok(seq) => SyscallResult::ok(i64::from(seq)),
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_ICMP_PING_WAIT` — wait for an ICMP Echo Reply.
///
/// `arg0`: sequence number from `SYS_ICMP_PING`.
/// `arg1`: timeout in milliseconds (0 = default 2000ms).
///
/// Returns RTT in nanoseconds on success, or `TimedOut`.
pub fn sys_icmp_ping_wait(args: &SyscallArgs) -> SyscallResult {
    let seq = args.arg0 as u16;
    let timeout_ms = args.arg1 as u32;
    // Default timeout: 2000ms ≈ 2000 poll iterations.
    let polls = if timeout_ms == 0 { 2000 } else { timeout_ms };

    match crate::net::icmp::wait_reply_rtt(seq, polls) {
        Some(rtt_ns) => SyscallResult::ok(rtt_ns as i64),
        None => SyscallResult::err(KernelError::TimedOut),
    }
}

// ---------------------------------------------------------------------------
// Scheduler configuration (50–59)
// ---------------------------------------------------------------------------

/// `SYS_SCHED_SET_TIMESLICE` — set time slice for a priority level.
///
/// `arg0`: priority level (0–31).
/// `arg1`: time slice in timer ticks (>= 1).
pub fn sys_sched_set_timeslice(args: &SyscallArgs) -> SyscallResult {
    let level = args.arg0 as usize;
    let ticks = args.arg1 as u32;

    if sched::set_time_slice(level, ticks) {
        serial_println!(
            "[syscall] sched_set_timeslice: level {} = {} ticks",
            level, ticks
        );
        SyscallResult::ok(0)
    } else {
        SyscallResult::err(KernelError::InvalidArgument)
    }
}

/// `SYS_SCHED_GET_TIMESLICE` — get time slice for a priority level.
///
/// `arg0`: priority level (0–31).
pub fn sys_sched_get_timeslice(args: &SyscallArgs) -> SyscallResult {
    let level = args.arg0 as usize;

    match sched::get_time_slice(level) {
        Some(ticks) => {
            #[allow(clippy::cast_possible_wrap)]
            {
                SyscallResult::ok(ticks as i64)
            }
        }
        None => SyscallResult::err(KernelError::InvalidArgument),
    }
}

/// `SYS_SCHED_RECONFIGURE` — reconfigure all time slices.
///
/// `arg0`: base time slice in ticks (>= 1).
/// `arg1`: increment per priority level.
pub fn sys_sched_reconfigure(args: &SyscallArgs) -> SyscallResult {
    let base = args.arg0 as u32;
    let increment = args.arg1 as u32;

    if sched::reconfigure_time_slices(base, increment) {
        serial_println!(
            "[syscall] sched_reconfigure: base={}, increment={}",
            base, increment
        );
        SyscallResult::ok(0)
    } else {
        SyscallResult::err(KernelError::InvalidArgument)
    }
}

/// `SYS_SCHED_SET_PROFILE` — apply a workload profile preset.
///
/// `arg0`: profile ID (0=Desktop, 1=Server, 2=Development, 3=Gaming).
pub fn sys_sched_set_profile(args: &SyscallArgs) -> SyscallResult {
    let profile_id = args.arg0 as u8;

    if sched::apply_workload_profile(profile_id) {
        SyscallResult::ok(0)
    } else {
        SyscallResult::err(KernelError::InvalidArgument)
    }
}

/// `SYS_SCHED_GET_PROFILE` — query the current workload profile.
pub fn sys_sched_get_profile(args: &SyscallArgs) -> SyscallResult {
    let _ = args;

    match sched::current_workload_profile() {
        Some(profile) => {
            #[allow(clippy::cast_possible_wrap)]
            {
                SyscallResult::ok(profile as u8 as i64)
            }
        }
        None => SyscallResult::err(KernelError::InvalidArgument),
    }
}

/// `SYS_CPU_COUNT` — get the number of online CPUs.
///
/// Reads `crate::smp::cpu_count()` which is updated by the SMP
/// bootstrap as each AP comes online and stays stable thereafter.
/// Always returns at least 1 (the BSP).
///
/// Returns: number of online CPUs.
pub fn sys_cpu_count(args: &SyscallArgs) -> SyscallResult {
    let _ = args;

    let n = crate::smp::cpu_count().max(1);
    // smp::cpu_count() returns usize.  On x86_64 a CPU count never
    // exceeds i64::MAX; cast is lossless.
    #[allow(clippy::cast_possible_wrap, clippy::cast_possible_truncation)]
    SyscallResult::ok(n as i64)
}

/// `SYS_PHYS_PAGES_TOTAL` — get total physical pages (16 KiB each).
///
/// Reads `crate::mm::frame::stats()` for the total frame count.  The
/// kernel page size *is* 16 KiB, so frame count equals page count.
/// Returns 1 if frame stats are unavailable (kernel still booting),
/// matching POSIX's "always ≥ 1" expectation from sysconf.
///
/// Returns: total physical pages.
pub fn sys_phys_pages_total(args: &SyscallArgs) -> SyscallResult {
    let _ = args;
    let total = crate::mm::frame::stats().map_or(1, |s| s.total_frames);
    #[allow(clippy::cast_possible_wrap, clippy::cast_possible_truncation)]
    SyscallResult::ok(total.max(1) as i64)
}

/// `SYS_PHYS_PAGES_AVAIL` — get currently-free physical pages.
///
/// Reads `crate::mm::frame::stats().free_frames`.  Snapshot is racy by
/// design — the count can change between the read and the syscall
/// return, so callers should treat the result as a hint.
///
/// Returns: free physical pages (may be 0).
pub fn sys_phys_pages_avail(args: &SyscallArgs) -> SyscallResult {
    let _ = args;
    let free = crate::mm::frame::stats().map_or(0, |s| s.free_frames);
    #[allow(clippy::cast_possible_wrap, clippy::cast_possible_truncation)]
    SyscallResult::ok(free as i64)
}

// ---------------------------------------------------------------------------
// Sysctl — kernel parameter registry (60–69)
// ---------------------------------------------------------------------------

/// `SYS_SYSCTL_GET` — read a kernel tunable parameter.
///
/// `arg0`: parameter ID.
pub fn sys_sysctl_get(args: &SyscallArgs) -> SyscallResult {
    let id = args.arg0 as u16;

    match crate::sysctl::get(id) {
        Some(value) => {
            #[allow(clippy::cast_possible_wrap)]
            {
                SyscallResult::ok(value as i64)
            }
        }
        None => SyscallResult::err(KernelError::InvalidArgument),
    }
}

/// `SYS_SYSCTL_SET` — write a kernel tunable parameter.
///
/// `arg0`: parameter ID.
/// `arg1`: new value.
pub fn sys_sysctl_set(args: &SyscallArgs) -> SyscallResult {
    let id = args.arg0 as u16;
    let value = args.arg1;

    match crate::sysctl::set(id, value) {
        Some(old_value) => {
            #[allow(clippy::cast_possible_wrap)]
            {
                SyscallResult::ok(old_value as i64)
            }
        }
        None => SyscallResult::err(KernelError::InvalidArgument),
    }
}

// ---------------------------------------------------------------------------
// Memory workload profiles (70–79)
// ---------------------------------------------------------------------------

/// `SYS_MM_SET_PROFILE` — apply a memory workload profile preset.
///
/// `arg0`: profile ID (0=Desktop, 1=Server, 2=Development, 3=Gaming).
///
/// Sets all mm.* sysctl parameters (max_stack_frames, lazy_default,
/// oom_policy, zero_on_alloc) to the profile's preset values.
pub fn sys_mm_set_profile(args: &SyscallArgs) -> SyscallResult {
    let profile_id = args.arg0 as u8;

    if crate::sysctl::apply_memory_profile(profile_id) {
        SyscallResult::ok(0)
    } else {
        SyscallResult::err(KernelError::InvalidArgument)
    }
}

/// `SYS_MM_GET_PROFILE` — query the current memory workload profile.
///
/// Returns the profile ID (0–3) if the current mm.* parameters match
/// a known profile.  If the parameters have been manually tuned,
/// returns `InvalidArgument`.
pub fn sys_mm_get_profile(args: &SyscallArgs) -> SyscallResult {
    let _ = args;

    match crate::sysctl::current_memory_profile() {
        Some(profile) => {
            #[allow(clippy::cast_possible_wrap)]
            {
                SyscallResult::ok(profile as u8 as i64)
            }
        }
        None => SyscallResult::err(KernelError::InvalidArgument),
    }
}

// ---------------------------------------------------------------------------
// System-wide workload profiles (80–89)
// ---------------------------------------------------------------------------

/// `SYS_SYSTEM_SET_PROFILE` — apply a unified system workload profile.
///
/// `arg0`: profile ID (0=Desktop, 1=Server, 2=Development, 3=Gaming).
///
/// Configures both scheduler time slices and mm.* sysctl parameters
/// for the selected workload.
pub fn sys_system_set_profile(args: &SyscallArgs) -> SyscallResult {
    let profile_id = args.arg0 as u8;

    if crate::sysctl::apply_system_profile(profile_id) {
        SyscallResult::ok(0)
    } else {
        SyscallResult::err(KernelError::InvalidArgument)
    }
}

// ---------------------------------------------------------------------------
// TCP diagnostic syscalls (840–849)
// ---------------------------------------------------------------------------

/// `SYS_TCP_LIST` — list active TCP connections.
///
/// `arg0`: pointer to output buffer.
/// `arg1`: buffer length in bytes.
///
/// Writes 20-byte records per connection.  Returns the number of
/// connections written.
pub fn sys_tcp_list(args: &SyscallArgs) -> SyscallResult {
    let buf_ptr = args.arg0 as usize;
    let buf_len = args.arg1 as usize;

    if buf_ptr == 0 {
        return SyscallResult::err(KernelError::InvalidArgument);
    }

    const RECORD_SIZE: usize = 20;
    let max_records = buf_len / RECORD_SIZE;
    if max_records == 0 {
        return SyscallResult::ok(0);
    }

    // Get our local IP for the records.
    let local_ip = crate::net::interface::info().ip;

    let conns = crate::net::tcp::all_connections();
    let mut written: usize = 0;

    for conn in &conns {
        if written >= max_records {
            break;
        }

        let mut record = [0u8; RECORD_SIZE];

        // [0..4] local IP (network order — already stored as octets).
        record[0] = local_ip.0[0];
        record[1] = local_ip.0[1];
        record[2] = local_ip.0[2];
        record[3] = local_ip.0[3];

        // [4..6] local port (network order = big-endian).
        let lport_be = conn.local_port.to_be_bytes();
        record[4] = lport_be[0];
        record[5] = lport_be[1];

        // [6..10] remote IP (IPv4 only — write zeroes for IPv6 connections).
        match conn.remote_ip.as_v4() {
            Some(v4) => {
                record[6] = v4.0[0];
                record[7] = v4.0[1];
                record[8] = v4.0[2];
                record[9] = v4.0[3];
            }
            None => {
                // IPv6 not representable in this 4-byte field; use 0.0.0.0.
                record[6] = 0;
                record[7] = 0;
                record[8] = 0;
                record[9] = 0;
            }
        }

        // [10..12] remote port (network order).
        let rport_be = conn.remote_port.to_be_bytes();
        record[10] = rport_be[0];
        record[11] = rport_be[1];

        // [12] state.
        record[12] = conn.state as u8;

        // [13..16] rx_buffered (u24 LE, capped).
        let rx = conn.rx_buffered.min(0xFF_FFFF) as u32;
        record[13] = rx as u8;
        record[14] = (rx >> 8) as u8;
        record[15] = (rx >> 16) as u8;

        // [16..19] tx_buffered (u24 LE, capped).
        let tx = conn.tx_buffered.min(0xFF_FFFF) as u32;
        record[16] = tx as u8;
        record[17] = (tx >> 8) as u8;
        record[18] = (tx >> 16) as u8;

        // [19] flags.
        let mut flags: u8 = 0;
        if conn.keepalive { flags |= 1; }
        if conn.nagle { flags |= 2; }
        if conn.ecn_ok { flags |= 4; }
        if conn.sack_ok { flags |= 8; }
        record[19] = flags;

        // SAFETY: buf_ptr is a userspace pointer validated by the caller.
        let dst = (buf_ptr + written * RECORD_SIZE) as *mut u8;
        unsafe {
            core::ptr::copy_nonoverlapping(record.as_ptr(), dst, RECORD_SIZE);
        }
        written = written.wrapping_add(1);
    }

    #[allow(clippy::cast_possible_wrap)]
    SyscallResult::ok(written as i64)
}

/// `SYS_TCP_LISTENER_LIST` — list active TCP listeners.
///
/// `arg0`: pointer to output buffer.
/// `arg1`: buffer length in bytes.
///
/// Writes 4-byte records per listener.  Returns the number of
/// listeners written.
pub fn sys_tcp_listener_list(args: &SyscallArgs) -> SyscallResult {
    let buf_ptr = args.arg0 as usize;
    let buf_len = args.arg1 as usize;

    if buf_ptr == 0 {
        return SyscallResult::err(KernelError::InvalidArgument);
    }

    const RECORD_SIZE: usize = 4;
    let max_records = buf_len / RECORD_SIZE;
    if max_records == 0 {
        return SyscallResult::ok(0);
    }

    let (listeners, count) = crate::net::tcp::all_listeners();
    let mut written: usize = 0;

    for i in 0..count {
        if written >= max_records {
            break;
        }
        if let Some(listener) = listeners.get(i) {
            let mut record = [0u8; RECORD_SIZE];
            // [0..2] local port (network order).
            let port_be = listener.port.to_be_bytes();
            record[0] = port_be[0];
            record[1] = port_be[1];
            // [2] backlog used.
            record[2] = listener.backlog_used as u8;
            // [3] backlog max.
            record[3] = listener.backlog_max as u8;

            let dst = (buf_ptr + written * RECORD_SIZE) as *mut u8;
            unsafe {
                core::ptr::copy_nonoverlapping(record.as_ptr(), dst, RECORD_SIZE);
            }
            written = written.wrapping_add(1);
        }
    }

    #[allow(clippy::cast_possible_wrap)]
    SyscallResult::ok(written as i64)
}

/// `SYS_NET_IF_INFO` — query network interface configuration.
///
/// `arg0`: pointer to output buffer (>= 24 bytes).
/// `arg1`: buffer length in bytes.
///
/// Returns 0 on success with interface info written.
pub fn sys_net_if_info(args: &SyscallArgs) -> SyscallResult {
    let out_ptr = args.arg0 as usize;
    let buf_len = args.arg1 as usize;

    if out_ptr == 0 {
        return SyscallResult::err(KernelError::InvalidArgument);
    }

    const INFO_SIZE: usize = 24;
    if buf_len < INFO_SIZE {
        return SyscallResult::err(KernelError::InvalidArgument);
    }

    let info = crate::net::interface::info();
    let mut record = [0u8; INFO_SIZE];

    // [0..4] IPv4 address.
    record[0] = info.ip.0[0];
    record[1] = info.ip.0[1];
    record[2] = info.ip.0[2];
    record[3] = info.ip.0[3];

    // [4..8] subnet mask.
    record[4] = info.subnet_mask.0[0];
    record[5] = info.subnet_mask.0[1];
    record[6] = info.subnet_mask.0[2];
    record[7] = info.subnet_mask.0[3];

    // [8..12] gateway.
    record[8] = info.gateway.0[0];
    record[9] = info.gateway.0[1];
    record[10] = info.gateway.0[2];
    record[11] = info.gateway.0[3];

    // [12..16] DNS server.
    record[12] = info.dns.0[0];
    record[13] = info.dns.0[1];
    record[14] = info.dns.0[2];
    record[15] = info.dns.0[3];

    // [16..22] MAC address.
    record[16] = info.mac.0[0];
    record[17] = info.mac.0[1];
    record[18] = info.mac.0[2];
    record[19] = info.mac.0[3];
    record[20] = info.mac.0[4];
    record[21] = info.mac.0[5];

    // [22] flags.
    record[22] = u8::from(info.up);

    // [23] reserved.
    record[23] = 0;

    let dst = out_ptr as *mut u8;
    // SAFETY: out_ptr is a userspace pointer, buf_len >= INFO_SIZE.
    unsafe {
        core::ptr::copy_nonoverlapping(record.as_ptr(), dst, INFO_SIZE);
    }

    SyscallResult::ok(0)
}

/// `SYS_ARP_TABLE` — query the ARP cache.
///
/// `arg0`: pointer to output buffer.
/// `arg1`: buffer length in bytes.
///
/// Writes 12-byte records per entry.  Returns the number written.
pub fn sys_arp_table(args: &SyscallArgs) -> SyscallResult {
    let buf_ptr = args.arg0 as usize;
    let buf_len = args.arg1 as usize;

    if buf_ptr == 0 {
        return SyscallResult::err(KernelError::InvalidArgument);
    }

    const RECORD_SIZE: usize = 12;
    let max_records = buf_len / RECORD_SIZE;
    if max_records == 0 {
        return SyscallResult::ok(0);
    }

    let (entries, count) = crate::net::arp::cache_entries();
    let mut written: usize = 0;

    for i in 0..count {
        if written >= max_records {
            break;
        }
        if let Some(entry) = entries.get(i) {
            let mut record = [0u8; RECORD_SIZE];

            // [0..4] IP address.
            record[0] = entry.ip.0[0];
            record[1] = entry.ip.0[1];
            record[2] = entry.ip.0[2];
            record[3] = entry.ip.0[3];

            // [4..10] MAC address.
            record[4] = entry.mac.0[0];
            record[5] = entry.mac.0[1];
            record[6] = entry.mac.0[2];
            record[7] = entry.mac.0[3];
            record[8] = entry.mac.0[4];
            record[9] = entry.mac.0[5];

            // [10..12] TTL in seconds (u16 LE).
            let ttl = entry.ttl_secs.min(u64::from(u16::MAX)) as u16;
            record[10] = ttl as u8;
            record[11] = (ttl >> 8) as u8;

            let dst = (buf_ptr + written * RECORD_SIZE) as *mut u8;
            unsafe {
                core::ptr::copy_nonoverlapping(record.as_ptr(), dst, RECORD_SIZE);
            }
            written = written.wrapping_add(1);
        }
    }

    #[allow(clippy::cast_possible_wrap)]
    SyscallResult::ok(written as i64)
}

/// `SYS_DNS_CACHE_STATS` — query DNS cache statistics.
///
/// `arg0`: pointer to output buffer (>= 40 bytes).
/// `arg1`: buffer length in bytes.
///
/// Returns 0 on success.
pub fn sys_dns_cache_stats(args: &SyscallArgs) -> SyscallResult {
    let out_ptr = args.arg0 as usize;
    let buf_len = args.arg1 as usize;

    if out_ptr == 0 {
        return SyscallResult::err(KernelError::InvalidArgument);
    }

    const STATS_SIZE: usize = 40;
    if buf_len < STATS_SIZE {
        return SyscallResult::err(KernelError::InvalidArgument);
    }

    let stats = crate::net::dns::cache_stats();
    let mut buf = [0u8; STATS_SIZE];

    buf[0..8].copy_from_slice(&stats.hits.to_le_bytes());
    buf[8..16].copy_from_slice(&stats.misses.to_le_bytes());
    buf[16..24].copy_from_slice(&stats.evictions.to_le_bytes());
    buf[24..28].copy_from_slice(&(stats.entries as u32).to_le_bytes());
    buf[28..32].copy_from_slice(&(stats.capacity as u32).to_le_bytes());
    // [32..40] reserved.

    let dst = out_ptr as *mut u8;
    unsafe {
        core::ptr::copy_nonoverlapping(buf.as_ptr(), dst, STATS_SIZE);
    }

    SyscallResult::ok(0)
}

/// SYS_TCP_POLL_STATUS — query poll readiness of a TCP connection.
///
/// arg0: connection handle
///
/// Returns: readiness bitmask (POLLIN=1, POLLOUT=4, POLLERR=8, POLLHUP=16)
pub fn sys_tcp_poll_status(args: &SyscallArgs) -> SyscallResult {
    let handle = args.arg0 as usize;
    let status = crate::net::tcp::poll_status(handle);
    SyscallResult::ok(status as i64)
}

/// SYS_TCP_LAST_ERROR — query the last error code for a TCP connection.
///
/// arg0: connection handle
///
/// Returns: error code (0=none, 1=refused, 2=reset, 3=timedout).
pub fn sys_tcp_last_error(args: &SyscallArgs) -> SyscallResult {
    let handle = args.arg0 as usize;
    let clear = args.arg1 != 0;
    let err = if clear {
        crate::net::tcp::take_last_error(handle)
    } else {
        crate::net::tcp::last_error(handle)
    };
    SyscallResult::ok(err as i64)
}

/// SYS_TCP_LOCAL_PORT — query the local port of a TCP connection or listener.
///
/// arg0: handle (connection or listener, depending on arg1)
/// arg1: 0 = connection handle, 1 = listener handle
///
/// Returns the local port number (positive u16 range) on success,
/// or InvalidArgument if the handle is invalid or not active.
pub fn sys_tcp_local_port(args: &SyscallArgs) -> SyscallResult {
    let handle = args.arg0 as usize;
    let is_listener = args.arg1 != 0;
    let port = if is_listener {
        crate::net::tcp::listener_local_port(handle)
    } else {
        crate::net::tcp::local_port(handle)
    };
    match port {
        Some(p) => SyscallResult::ok(p as i64),
        None => SyscallResult::err(KernelError::InvalidArgument),
    }
}

/// SYS_TCP_LISTENER_READY — check if listener has pending connections.
///
/// arg0: listener handle
///
/// Returns: 1 if pending connections available, 0 otherwise.
pub fn sys_tcp_listener_ready(args: &SyscallArgs) -> SyscallResult {
    let handle = args.arg0 as usize;
    let ready = crate::net::tcp::listener_has_pending(handle);
    SyscallResult::ok(if ready { 1 } else { 0 })
}

/// SYS_UDP_RX_READY — check if UDP socket has queued datagrams.
///
/// arg0: socket handle
///
/// Returns: number of queued datagrams (≥0).
pub fn sys_udp_rx_ready(args: &SyscallArgs) -> SyscallResult {
    let handle = args.arg0 as usize;
    let count = crate::net::udp::rx_ready(handle);
    SyscallResult::ok(count as i64)
}

/// `SYS_UDP_RX_FRONT_BYTES` — get byte size of first deliverable datagram.
///
/// `arg0`: socket handle.
///
/// Used for FIONREAD on UDP sockets.
pub fn sys_udp_rx_front_bytes(args: &SyscallArgs) -> SyscallResult {
    let handle = args.arg0 as usize;
    let bytes = crate::net::udp::rx_front_bytes(handle);
    SyscallResult::ok(bytes as i64)
}

/// SYS_TCP_INFO — query detailed TCP connection information.
///
/// arg0: connection handle
/// arg1: output buffer pointer
/// arg2: buffer length (at least 48 bytes)
///
/// Returns 0 on success, writes 48-byte packed info struct.
pub fn sys_tcp_info(args: &SyscallArgs) -> SyscallResult {
    let handle = args.arg0 as usize;
    let out_ptr = args.arg1 as usize;
    let buf_len = args.arg2 as usize;

    if out_ptr == 0 {
        return SyscallResult::err(KernelError::InvalidArgument);
    }

    const INFO_SIZE: usize = 48;
    if buf_len < INFO_SIZE {
        return SyscallResult::err(KernelError::InvalidArgument);
    }

    if let Err(e) = crate::mm::user::validate_user_write(args.arg1, INFO_SIZE) {
        return SyscallResult::err(e);
    }

    let info = match crate::net::tcp::connection_info(handle) {
        Some(i) => i,
        None => return SyscallResult::err(KernelError::InvalidArgument),
    };

    // Pack into a 48-byte buffer.
    let mut buf = [0u8; INFO_SIZE];

    // [0] state enum as u8.
    buf[0] = match info.state {
        crate::net::tcp::TcpState::Closed => 0,
        crate::net::tcp::TcpState::Listen => 1,
        crate::net::tcp::TcpState::SynSent => 2,
        crate::net::tcp::TcpState::SynReceived => 3,
        crate::net::tcp::TcpState::Established => 4,
        crate::net::tcp::TcpState::FinWait1 => 5,
        crate::net::tcp::TcpState::FinWait2 => 6,
        crate::net::tcp::TcpState::TimeWait => 7,
        crate::net::tcp::TcpState::CloseWait => 8,
        crate::net::tcp::TcpState::LastAck => 9,
    };

    // [1] flags bitfield.
    let mut flags: u8 = 0;
    if info.keepalive { flags |= 1; }
    if info.nagle     { flags |= 2; }
    if info.ecn_ok    { flags |= 4; }
    if info.sack_ok   { flags |= 8; }
    if info.wscale_ok { flags |= 16; }
    if info.ts_ok     { flags |= 32; }
    buf[1] = flags;

    // [2..4] effective MSS (u16 LE).
    buf[2..4].copy_from_slice(&info.eff_mss.to_le_bytes());
    // [4..8] SRTT in microseconds (u32 LE).
    let srtt_us = (info.srtt_ns / 1000) as u32;
    buf[4..8].copy_from_slice(&srtt_us.to_le_bytes());
    // [8..12] RTO in microseconds (u32 LE).
    let rto_us = (info.rto_ns / 1000) as u32;
    buf[8..12].copy_from_slice(&rto_us.to_le_bytes());
    // [12..16] cwnd (u32 LE).
    buf[12..16].copy_from_slice(&info.cwnd.to_le_bytes());
    // [16..20] ssthresh (u32 LE).
    buf[16..20].copy_from_slice(&info.ssthresh.to_le_bytes());
    // [20..24] snd_wnd (u32 LE).
    buf[20..24].copy_from_slice(&info.snd_wnd.to_le_bytes());
    // [24..28] rx_buffered (u32 LE).
    buf[24..28].copy_from_slice(&(info.rx_buffered as u32).to_le_bytes());
    // [28..32] tx_buffered (u32 LE).
    buf[28..32].copy_from_slice(&(info.tx_buffered as u32).to_le_bytes());
    // [32..36] peer_mss (u32 LE).
    buf[32..36].copy_from_slice(&(info.peer_mss as u32).to_le_bytes());
    // [36..48] reserved (zeros).

    let dst = out_ptr as *mut u8;
    // SAFETY: validated above — dst is in user space and writable.
    unsafe {
        core::ptr::copy_nonoverlapping(buf.as_ptr(), dst, INFO_SIZE);
    }

    SyscallResult::ok(0)
}

/// SYS_TCP_SHUTDOWN — half-close a TCP connection.
///
/// arg0: connection handle
/// arg1: how (0=SHUT_RD, 1=SHUT_WR, 2=SHUT_RDWR)
///
/// Returns 0 on success.
pub fn sys_tcp_shutdown(args: &SyscallArgs) -> SyscallResult {
    let handle = args.arg0 as usize;
    let how = args.arg1 as u32;
    if how > 2 {
        return SyscallResult::err(KernelError::InvalidArgument);
    }
    match crate::net::tcp::shutdown(handle, how) {
        Ok(()) => SyscallResult::ok(0),
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_TCP_SET_NODELAY` — enable or disable TCP_NODELAY (Nagle algorithm).
///
/// `arg0`: socket handle.
/// `arg1`: 0 = enable Nagle (default), non-zero = disable Nagle (TCP_NODELAY).
///
/// Returns 0 on success.
pub fn sys_tcp_set_nodelay(args: &SyscallArgs) -> SyscallResult {
    if let Err(e) = require_cap_type(
        crate::cap::ResourceType::Socket,
        crate::cap::Rights::WRITE,
    ) {
        return SyscallResult::err(e);
    }

    let handle = args.arg0 as usize;
    let nodelay = args.arg1 != 0;
    match crate::net::tcp::set_nodelay(handle, nodelay) {
        Ok(()) => SyscallResult::ok(0),
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_TCP_SET_KEEPALIVE` — enable or disable TCP keepalive probes.
///
/// `arg0`: socket handle.
/// `arg1`: 0 = disable keepalive, non-zero = enable keepalive.
///
/// Returns 0 on success.
pub fn sys_tcp_set_keepalive(args: &SyscallArgs) -> SyscallResult {
    if let Err(e) = require_cap_type(
        crate::cap::ResourceType::Socket,
        crate::cap::Rights::WRITE,
    ) {
        return SyscallResult::err(e);
    }

    let handle = args.arg0 as usize;
    let enabled = args.arg1 != 0;
    match crate::net::tcp::set_keepalive(handle, enabled) {
        Ok(()) => SyscallResult::ok(0),
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_TCP_SET_KEEPALIVE_PARAMS` — configure TCP keepalive timing.
///
/// `arg0`: socket handle.
/// `arg1`: idle time in seconds (0 = use default 75s).
/// `arg2`: probe interval in seconds (0 = use default 10s).
/// `arg3`: max probe count (0 = use default 9).
///
/// Returns 0 on success.
pub fn sys_tcp_set_keepalive_params(args: &SyscallArgs) -> SyscallResult {
    if let Err(e) = require_cap_type(
        crate::cap::ResourceType::Socket,
        crate::cap::Rights::WRITE,
    ) {
        return SyscallResult::err(e);
    }

    let handle = args.arg0 as usize;
    // Convert seconds to nanoseconds (0 means "use default").
    let idle_secs = args.arg1;
    let interval_secs = args.arg2;
    let max_probes = args.arg3 as u8;

    // Default keepalive values (matching kernel constants).
    const DEFAULT_IDLE_NS: u64 = 75_000_000_000;   // 75 seconds
    const DEFAULT_INTERVAL_NS: u64 = 10_000_000_000; // 10 seconds
    const DEFAULT_MAX_PROBES: u8 = 9;

    let idle_ns = if idle_secs == 0 { DEFAULT_IDLE_NS } else { idle_secs.saturating_mul(1_000_000_000) };
    let interval_ns = if interval_secs == 0 { DEFAULT_INTERVAL_NS } else { interval_secs.saturating_mul(1_000_000_000) };
    let probes = if max_probes == 0 { DEFAULT_MAX_PROBES } else { max_probes };

    match crate::net::tcp::set_keepalive_params(handle, idle_ns, interval_ns, probes) {
        Ok(()) => SyscallResult::ok(0),
        Err(e) => SyscallResult::err(e),
    }
}
