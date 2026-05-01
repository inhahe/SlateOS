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

use crate::error::KernelError;
use crate::ipc::channel::{self, ChannelHandle, Message};
use crate::ipc::completion::{self, CpHandle, WaitSource};
use crate::ipc::eventfd::{self, EventFdHandle};
use crate::ipc::futex;
use crate::ipc::pipe::{self, PipeHandle};
use crate::ipc::shm::{self, ShmHandle};
use crate::sched;
use crate::serial_println;

use super::dispatch::{SyscallArgs, SyscallResult};

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
    // TODO: Store exit code (args.arg0) for parent to retrieve.
    let _ = args;

    // Notify the thread system so the owning process can transition
    // to Zombie when its last thread exits.  For bare kernel tasks
    // (not owned by any process), this is a harmless no-op.
    let task_id = sched::current_task_id();
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
    // Currently type-level (any DeviceIrq cap with WRITE grants access).
    // Future: per-IRQ capabilities with resource_id = IRQ number.
    if let Err(e) = require_cap_type(
        crate::cap::ResourceType::DeviceIrq,
        crate::cap::Rights::WRITE,
    ) {
        return SyscallResult::err(e);
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

    // Capability check: caller must hold a PortIo capability.
    // Currently type-level (any PortIo cap with READ grants access).
    // Future: per-port capabilities with resource_id = port number.
    if let Err(e) = require_cap_type(
        crate::cap::ResourceType::PortIo,
        crate::cap::Rights::READ,
    ) {
        return SyscallResult::err(e);
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

    // Capability check: caller must hold a PortIo capability.
    // Currently type-level (any PortIo cap with WRITE grants access).
    // Future: per-port capabilities with resource_id = port number.
    if let Err(e) = require_cap_type(
        crate::cap::ResourceType::PortIo,
        crate::cap::Rights::WRITE,
    ) {
        return SyscallResult::err(e);
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
    use crate::mm::frame::{self, PhysFrame, FRAME_SIZE};
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
        if vaddr_hint % frame_size != 0 {
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

    let hhdm = match page_table::hhdm() {
        Some(h) => h,
        None => return SyscallResult::err(KernelError::InternalError),
    };

    if flags & MAP_MMIO != 0 {
        // MMIO mapping: map specific physical address.
        if phys_addr % frame_size != 0 {
            return SyscallResult::err(KernelError::BadAlignment);
        }

        for i in 0..num_frames {
            #[allow(clippy::arithmetic_side_effects)]
            let pa = phys_addr + (i as u64) * frame_size;
            #[allow(clippy::arithmetic_side_effects)]
            let va = base_vaddr + (i as u64) * frame_size;

            let phys = match PhysFrame::from_addr(pa) {
                Some(f) => f,
                None => return SyscallResult::err(KernelError::BadAlignment),
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
                // TODO: unmap already-mapped frames on partial failure.
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
        // fresh zeroed frames immediately.
        for i in 0..num_frames {
            let phys = match frame::alloc_frame() {
                Ok(f) => f,
                Err(e) => {
                    serial_println!("[mmap] Frame alloc failed at frame {}: {:?}", i, e);
                    // TODO: free already-allocated frames on partial failure.
                    return SyscallResult::err(e);
                }
            };

            // Zero the frame.
            let frame_virt = phys.to_virt(hhdm);
            // SAFETY: frame_virt is the HHDM mapping of a freshly allocated frame.
            unsafe {
                core::ptr::write_bytes(frame_virt as *mut u8, 0, FRAME_SIZE);
            }

            #[allow(clippy::arithmetic_side_effects)]
            let va = base_vaddr + (i as u64) * frame_size;

            // SAFETY: pml4_phys is valid, phys is freshly allocated.
            if let Err(e) = unsafe {
                page_table::map_frame(pml4_phys, VirtAddr::new(va), phys, page_flags)
            } {
                serial_println!(
                    "[mmap] Anonymous map failed at va={:#x}: {:?}",
                    va, e
                );
                // Leak the allocated frame rather than risk double-free.
                return SyscallResult::err(e);
            }
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
    if vaddr % frame_size != 0 {
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
fn mmap_alloc_vaddr(size: u64) -> u64 {
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
/// Returns both handles: `value` = ep0, `value2` = ep1.
pub fn sys_channel_create(args: &SyscallArgs) -> SyscallResult {
    let _ = args;
    let (ep0, ep1) = channel::create();

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
    channel::close(handle);
    SyscallResult::ok(0)
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
    pipe::close(handle);
    SyscallResult::ok(0)
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
    shm::close(handle);
    SyscallResult::ok(0)
}

// ---------------------------------------------------------------------------
// Eventfd handlers (240–249)
// ---------------------------------------------------------------------------

/// `SYS_EVENTFD_CREATE` — create a new eventfd counter.
///
/// `arg0`: initial counter value.
///
/// Returns: eventfd handle.
pub fn sys_eventfd_create(args: &SyscallArgs) -> SyscallResult {
    let initial = args.arg0;
    let handle = eventfd::create(initial);

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
    eventfd::close(handle);
    SyscallResult::ok(0)
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
        _ => None,
    }
}

/// `SYS_CP_CREATE` — create a completion port.
///
/// Returns: completion port handle.
pub fn sys_cp_create(args: &SyscallArgs) -> SyscallResult {
    let _ = args;
    let handle = completion::create();

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
    // The parent PID — for now, use 0 (kernel) as the "current process".
    // TODO: get actual current process ID from the calling task's
    // process association.
    let parent_pid = 0;

    // Try to reap immediately.
    match pcb::try_reap(parent_pid, child_pid) {
        Ok(Some(exit_code)) => {
            #[allow(clippy::cast_possible_wrap)]
            return SyscallResult::ok(exit_code as i64);
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
                Ok(Some(exit_code)) => {
                    #[allow(clippy::cast_possible_wrap)]
                    SyscallResult::ok(exit_code as i64)
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
    // Parent PID: use 0 (kernel) for now — same as sys_process_wait.
    let parent_pid = 0;

    match pcb::try_reap(parent_pid, child_pid) {
        Ok(Some(exit_code)) => {
            #[allow(clippy::cast_possible_wrap)]
            SyscallResult::ok(exit_code as i64)
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

    // Exec: validate ELF, tear down old AS, load new AS, set up stack.
    match exec_process(pid, &elf_copy) {
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
/// Returns an approximate monotonic clock based on the APIC timer tick
/// count.  At 100 Hz, resolution is 10 ms.  Good enough for coarse
/// timing; high-resolution timing will be added via TSC or HPET later.
pub fn sys_clock_monotonic(args: &SyscallArgs) -> SyscallResult {
    let _ = args;

    let ticks = crate::apic::tick_count();
    // 100 Hz → 10 ms per tick → 10_000_000 ns per tick.
    let ns = ticks.saturating_mul(10_000_000);

    #[allow(clippy::cast_possible_wrap)]
    SyscallResult::ok(ns as i64)
}

/// `SYS_SLEEP` — sleep for a specified duration in nanoseconds.
///
/// `arg0`: duration in nanoseconds.
///
/// Blocks the calling task until the timer fires a wakeup.  The actual
/// sleep time is rounded up to the next timer tick (10 ms granularity
/// at 100 Hz).
///
/// Returns: 0 on success.
pub fn sys_sleep(args: &SyscallArgs) -> SyscallResult {
    let duration_ns = args.arg0;

    if duration_ns == 0 {
        // Zero sleep → just yield.
        sched::yield_now();
        return SyscallResult::ok(0);
    }

    // Convert nanoseconds to ticks (100 Hz → 10 ms per tick).
    // Round up so the task sleeps at least the requested duration.
    let ticks_needed = duration_ns
        .saturating_add(9_999_999)
        .saturating_div(10_000_000);

    // Ensure at least 1 tick.
    let ticks_needed = ticks_needed.max(1);

    let current_tick = crate::apic::tick_count();
    let wake_tick = current_tick.saturating_add(ticks_needed);

    sched::sleep_until_tick(wake_tick);

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

    let handle = crate::ipc::timer::create(duration_ns, flags);
    if handle == 0 {
        return SyscallResult::err(KernelError::OutOfMemory);
    }

    #[allow(clippy::cast_possible_wrap)]
    SyscallResult::ok(handle as i64)
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
        Ok((size, entry_type)) => {
            // Write 16-byte FsStatResult (same format as SYS_FS_STAT).
            // SAFETY: Validated above.
            unsafe {
                core::ptr::write_bytes(out_ptr, 0, 16);
                let size_ptr = out_ptr as *mut u64;
                core::ptr::write(size_ptr, size);
                core::ptr::write(out_ptr.add(8), entry_type);
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

    match crate::net::tcp::connect(ip, port) {
        Ok(handle) => {
            #[allow(clippy::cast_possible_wrap)]
            SyscallResult::ok(handle as i64)
        }
        Err(e) => SyscallResult::err(e),
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
        Ok(()) => {
            #[allow(clippy::cast_possible_wrap)]
            SyscallResult::ok(len as i64)
        }
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_TCP_RECV` — receive data from a TCP socket (blocking).
///
/// `arg0`: socket handle.
/// `arg1`: pointer to receive buffer.
/// `arg2`: buffer capacity.
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

    if buf_ptr.is_null() && buf_cap > 0 {
        return SyscallResult::err(KernelError::InvalidArgument);
    }

    if buf_cap > 0 {
        if let Err(e) = crate::mm::user::validate_user_write(args.arg1, buf_cap) {
            return SyscallResult::err(e);
        }
    }

    // Use blocking read with a generous timeout (~5 seconds at 100Hz).
    match crate::net::tcp::read_blocking(handle, 500) {
        Ok(data) => {
            if data.is_empty() {
                // Connection closed — EOF.
                return SyscallResult::ok(0);
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
        Err(e) => SyscallResult::err(e),
    }
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

    match crate::net::udp::bind(port) {
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

    // Use an ephemeral source port based on the handle, or a default.
    // The UDP send function takes a source port directly.
    let src_port: u16 = if _handle == 0 { 49152 } else {
        // Look up the bound port from the socket handle.
        // For simplicity, use 49152 + handle as ephemeral.
        #[allow(clippy::cast_possible_truncation)]
        let p = 49152u16.saturating_add(_handle as u16);
        p
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

    match crate::net::udp::recv(handle) {
        Some(datagram) => {
            let copy_len = datagram.data.len().min(buf_cap);
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

            #[allow(clippy::cast_possible_wrap)]
            SyscallResult::ok(copy_len as i64)
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
