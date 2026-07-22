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
use crate::ipc::stream_socket::{self, StreamSocketHandle};
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
pub(crate) fn require_cap_type(
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

/// Shared root-authority predicate for the privileged-syscall gates below.
///
/// All of our `CAP_SYS_*`-class authorities (set the clock, mount/unmount,
/// format a device, run fsck) reduce to the same check on our current
/// user/group model: the caller must be root (uid 0), with kernel/bare tasks
/// (no owning process, or PID 0) bypassing. The distinct named wrappers
/// (`require_clock_authority`, `require_mount_authority`, …) exist for doc
/// clarity and so individual authorities can be split off later (e.g. a storage
/// daemon granted format-but-not-mount rights); they all funnel through here so
/// the actual root check lives in exactly one place.
///
/// # Errors
///
/// - `PermissionDenied` — the calling process exists and is not root.
fn require_root_authority() -> Result<(), KernelError> {
    let pid = match caller_pid() {
        Some(pid) => pid,
        None => return Ok(()), // Bare kernel task — bypass.
    };
    if pid == 0 {
        return Ok(()); // Kernel process — implicit authority.
    }
    match crate::proc::pcb::get_credentials(pid) {
        Some(creds) if creds.is_root() => Ok(()),
        // A live, non-root process: deny.
        Some(_) => Err(KernelError::PermissionDenied),
        // No credentials record (process exited out from under us): fail closed.
        None => Err(KernelError::PermissionDenied),
    }
}

/// Check that the calling process is privileged enough to set the system
/// wall clock (`SYS_CLOCK_SETTIME` / `SYS_CLOCK_ADJTIME`).
///
/// Setting the global wall clock is a system-wide side effect: it affects
/// every process's notion of time, filesystem timestamps, certificate
/// validity, scheduled jobs, etc.  In POSIX terms it requires `CAP_SYS_TIME`,
/// which on our system maps to running as root (uid 0).  The user/group model
/// documents that all processes currently run as uid 0 during early
/// development, so this gate is a no-op today but becomes load-bearing the
/// moment a login service assigns non-root credentials — at which point the
/// userspace advisory check in the posix layer is no longer the only line of
/// defence.
///
/// Kernel tasks (no owning process, or PID 0) bypass the check: the kernel and
/// its bare tasks have implicit authority, mirroring [`require_cap_type`].
///
/// # Errors
///
/// - `PermissionDenied` — the calling process is not root.
fn require_clock_authority() -> Result<(), KernelError> {
    require_root_authority()
}

/// Check that the calling process is privileged enough to mount or unmount
/// filesystems (`SYS_FS_MOUNT` / `SYS_FS_UMOUNT`).
///
/// Mounting changes the global filesystem namespace: it can shadow existing
/// paths, expose attacker-controlled on-disk data at a trusted location, or
/// hide files from other processes.  In POSIX terms this requires
/// `CAP_SYS_ADMIN`, which on our system maps to running as root (uid 0).
///
/// Kernel tasks (no owning process, or PID 0) bypass the check, mirroring
/// [`require_cap_type`] and [`require_clock_authority`].
///
/// # Errors
///
/// - `PermissionDenied` — the calling process is not root.
fn require_mount_authority() -> Result<(), KernelError> {
    require_root_authority()
}

/// Check that the calling process is privileged enough to format a block
/// device (`SYS_FS_FORMAT`).
///
/// Formatting writes a fresh filesystem over the entire device, irreversibly
/// destroying any existing data.  Like mount, this is `CAP_SYS_ADMIN`-class
/// authority, which on our system maps to running as root (uid 0).  Kept
/// distinct from [`require_mount_authority`] so the two authorities can be
/// split later (e.g. a storage daemon granted format-but-not-mount rights)
/// without conflating their doc semantics.
///
/// Kernel tasks (no owning process, or PID 0) bypass the check.
///
/// # Errors
///
/// - `PermissionDenied` — the calling process is not root.
fn require_format_authority() -> Result<(), KernelError> {
    require_root_authority()
}

/// Check that the calling process is privileged enough to check/repair a
/// filesystem on a block device (`SYS_FS_CHECK`).
///
/// fsck reads the raw block device and, in repair mode, writes corrected FAT
/// tables and directory entries — both `CAP_SYS_ADMIN`-class operations that
/// map to root (uid 0) on our user/group model.  Kept distinct from
/// [`require_format_authority`] so a future storage daemon could be granted
/// check-but-not-format rights.
///
/// Kernel tasks (no owning process, or PID 0) bypass the check.
///
/// # Errors
///
/// - `PermissionDenied` — the calling process is not root.
fn require_fsck_authority() -> Result<(), KernelError> {
    require_root_authority()
}

/// Check that the calling process is privileged enough to discard (TRIM) the
/// free space of a mounted filesystem (`SYS_FS_TRIM`).
///
/// fstrim issues device-level discard for free blocks — a
/// `CAP_SYS_ADMIN`-class operation that maps to root (uid 0) on our
/// user/group model.  Kept distinct from [`require_fsck_authority`] so a
/// future storage daemon could be granted trim-but-not-fsck rights.
///
/// Kernel tasks (no owning process, or PID 0) bypass the check.
///
/// # Errors
///
/// - `PermissionDenied` — the calling process is not root.
fn require_trim_authority() -> Result<(), KernelError> {
    require_root_authority()
}

/// Check that the calling process is privileged enough to reconfigure a
/// network interface (`SYS_NET_IF_CONFIG`).
///
/// Changing an interface's address, routes, DNS, or up/down state is a
/// system-wide side effect: it redirects every process's traffic, can hijack
/// connectivity, and can expose the host on an attacker-chosen address. In
/// POSIX terms this is `CAP_NET_ADMIN`, which on our system maps to running as
/// root (uid 0). Kept distinct from the storage authorities so a future
/// network daemon (dhcpcd, a config service) could be granted net-admin rights
/// without also gaining mount/format authority.
///
/// Kernel tasks (no owning process, or PID 0) bypass the check.
///
/// # Errors
///
/// - `PermissionDenied` — the calling process is not root.
fn require_netadmin_authority() -> Result<(), KernelError> {
    require_root_authority()
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
    use super::number::{MAP_EXEC, MAP_LAZY, MAP_MMIO, MAP_NOCACHE, MAP_READ, MAP_WRITE};

    let vaddr_hint = args.arg0;
    let size = args.arg1;
    let mut flags = args.arg2;
    let phys_addr = args.arg3;

    // If the caller didn't explicitly specify a commit bit (MAP_LAZY /
    // MAP_MMIO), pick the default commit mode.  A per-process policy
    // override (set via Settings; design-decisions.md §11) wins; otherwise
    // the system-wide default (PARAM_MM_LAZY_DEFAULT) applies.  MMIO
    // mappings are always committed (they must map specific physical
    // addresses), so they bypass this entirely.
    if flags & (MAP_LAZY | MAP_MMIO) == 0 {
        let sysctl_lazy =
            crate::sysctl::get(crate::sysctl::PARAM_MM_LAZY_DEFAULT) == Some(1);
        let policy = thread::owner_process(sched::current_task_id())
            .and_then(pcb::get_mmap_commit_policy)
            .unwrap_or_default();
        if policy.native_lazy(sysctl_lazy) {
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
    //
    // `USER_ACCESSIBLE` is granted iff the caller requested at least one of
    // read/write/execute. A request with *none* of those bits is the native
    // spelling of `PROT_NONE` (the Linux mmap layer translates a Linux
    // `PROT_NONE` into exactly this — no MAP_READ/WRITE/EXEC): the resulting
    // VMA/PTE is inaccessible to ring 3, so a touch raises an access violation
    // rather than demand-paging a zero page (design-decisions §32). Matching
    // x86-64, `MAP_WRITE`/`MAP_EXEC` alone also imply readability.
    let mut page_flags = PageFlags::PRESENT;
    if flags & (MAP_READ | MAP_WRITE | MAP_EXEC) != 0 {
        page_flags |= PageFlags::USER_ACCESSIBLE;
    }
    if flags & MAP_WRITE != 0 {
        page_flags |= PageFlags::WRITABLE;
    }
    if flags & MAP_EXEC == 0 {
        page_flags |= PageFlags::NO_EXECUTE;
    }
    if flags & MAP_NOCACHE != 0 {
        page_flags |= PageFlags::NO_CACHE;
    }

    // Pick a virtual address.
    //
    // `reserved` records whether the address was chosen by the VMA-aware
    // reservation path, which *atomically* finds a free gap and inserts the
    // anonymous VMA under one lock (closing the find/insert race between
    // concurrent same-process mmaps on this SMP kernel).  When set, the
    // anonymous branches below must NOT add the VMA again.
    let mut reserved = false;
    let base_vaddr = if vaddr_hint != 0 {
        // Caller specified an address — validate alignment.
        if !vaddr_hint.is_multiple_of(frame_size) {
            return SyscallResult::err(KernelError::BadAlignment);
        }
        vaddr_hint
    } else if flags & MAP_MMIO != 0 {
        // MMIO mappings are not VMA-tracked, so they use the device mmap
        // region's bump allocator, disjoint from the VMA-tracked general
        // region the gap finder serves (they can never collide).
        mmap_alloc_vaddr(size_aligned)
    } else {
        // Anonymous mapping in the VMA-tracked general region: atomically
        // reserve a free gap with its Anonymous VMA.  Reusing freed gaps
        // (rather than a monotonic bump) means a map/unmap-heavy process
        // never exhausts the window, and the result can never overlap an
        // existing mapping.
        use crate::mm::vma::VmaKind;
        reserved = true;
        alloc_user_mmap_reserve(pid, size_aligned, VmaKind::Anonymous, page_flags)
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
        //
        // When `reserved`, the gap reservation already inserted this
        // Anonymous VMA atomically; adding it again would overlap.  Only
        // the explicit-hint path (which doesn't reserve) registers it here.
        if !reserved {
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
        }

        serial_println!(
            "[mmap] Lazy mapped {:#x}..{:#x} ({} frames, demand-paged)",
            base_vaddr, base_vaddr + size_aligned, num_frames
        );
    } else {
        // Committed anonymous mapping (default): allocate and map
        // fresh zeroed frames immediately.  map_committed_range handles
        // alloc + zero + map atomically with full rollback on partial failure.
        //
        // Register a VMA for the region first.  Committed regions are
        // first-class address-space entries just like lazy ones: this is
        // what makes them visible in `/proc/<pid>/maps` and keeps the VMA
        // list consistent with the RLIMIT_AS accounting that drives
        // `/proc/<pid>/status` VmSize and `/proc/<pid>/statm`.  The frames
        // are pre-populated here, but the VMA still describes the logical
        // anonymous region (matching Linux MAP_POPULATE semantics: VMA
        // present + pages pre-faulted); a stray not-present fault inside it
        // would correctly resolve to a fresh zero page via the demand-fault
        // handler.  We add the VMA before mapping so a mapping failure rolls
        // back cleanly with `remove_vma` (no unmap loop needed).
        //
        // When `reserved`, the gap reservation already inserted this
        // Anonymous VMA atomically; only the explicit-hint path registers
        // it here.  Either way the rollback below removes it on failure.
        use crate::mm::vma::{Vma, VmaKind};

        if !reserved {
            let vma = Vma {
                start: base_vaddr,
                end: base_vaddr.saturating_add(size_aligned),
                kind: VmaKind::Anonymous,
                flags: page_flags,
            };

            if let Err(e) = pcb::add_vma(pid, vma) {
                serial_println!(
                    "[mmap] Committed VMA registration failed at {:#x}: {:?}",
                    base_vaddr, e
                );
                return SyscallResult::err(e);
            }
        }

        // SAFETY: pml4_phys is a valid page table for this process.
        if let Err(e) = unsafe {
            page_table::map_committed_range(
                pml4_phys,
                VirtAddr::new(base_vaddr),
                num_frames,
                page_flags,
            )
        } {
            // Roll back the VMA we just registered so the list stays
            // consistent with what's actually mapped.
            pcb::remove_vma(pid, base_vaddr);
            serial_println!(
                "[mmap] Committed map failed at {:#x} ({} frames): {:?}",
                base_vaddr, num_frames, e
            );
            return SyscallResult::err(e);
        }

        // Flush the TLB for the freshly-mapped range.  `map_committed_range`
        // leaves TLB invalidation to the caller.  x86 does not cache
        // not-present entries in the common case, but a VA reused shortly
        // after a munmap (heavy allocator churn does exactly this) may still
        // have a stale entry lingering until the matching flush; invalidating
        // here guarantees the CPU observes the new PTEs immediately.
        mmap_flush_range(base_vaddr, base_vaddr.saturating_add(size_aligned));

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

    // Flush the TLB for the whole unmapped range.  CRITICAL: `unmap_frame`
    // only clears the page-table entries — it does NOT invalidate the TLB
    // (its doc-comment makes flushing the caller's responsibility).  Without
    // this flush the CPU keeps a stale VA→frame translation cached, so the
    // process can continue to read/write a frame that we have already freed
    // back to the buddy allocator.  Once that frame is recycled (its first
    // 16 bytes become an intrusive `FreeNode`, or it is remapped elsewhere),
    // the stale writes corrupt allocator state — observed as a kernel #PF in
    // `BuddyAllocator::remove_free` dereferencing a user VA that had leaked
    // into the free list.  Flush before the frames can be reused by anyone.
    mmap_flush_range(vaddr, vaddr.saturating_add(size_aligned));

    // Also remove any per-process VMA that starts at this address.
    // Both committed and lazy (MAP_LAZY) mmap regions register a VMA, so
    // this drops the address-space record alongside the unmapped frames.
    // If no VMA matches (e.g. a partial unmap that doesn't start on a VMA
    // boundary), this is a no-op.
    pcb::remove_vma(pid, vaddr);

    serial_println!(
        "[mmap] Unmapped {} frames at {:#x}..{:#x}",
        unmapped, vaddr, vaddr + size_aligned
    );

    SyscallResult::ok(0)
}

/// `mprotect(addr, len, prot)` — native ABI (`SYS_MPROTECT` = 22).
///
/// Change the protection of the pages in `[addr, addr+len)` in the calling
/// process's address space.  Delegates the entire operation — Linux-style
/// argument validation, VMA-coverage check, VMA bookkeeping, per-4 KiB-page
/// PTE update, and a single batched TLB shootdown — to the ABI-neutral
/// [`crate::syscall::linux::mprotect_core`], which is the *same* core the
/// Linux-ABI `mprotect` (Linux syscall #10) runs.  We only differ in error
/// encoding: the native ABI returns raw [`KernelError`] codes (mapped by
/// posix's `errno::translate`), whereas the Linux ABI remaps them to Linux
/// errno.  This closes the TD-NATIVE-MPROTECT stub (previously the native
/// path had no handler at 22 and resolved to `ENOTSUP`).
///
/// `prot` is the standard `PROT_READ`=1 / `PROT_WRITE`=2 / `PROT_EXEC`=4
/// mask (matching what posix `mman::mprotect` forwards); unknown bits are
/// rejected with `InvalidArgument`.  `PROT_NONE` (prot == 0) makes the
/// range user-inaccessible for real (see design-decisions §32).
pub fn sys_mprotect(args: &SyscallArgs) -> SyscallResult {
    match crate::syscall::linux::mprotect_core(args.arg0, args.arg1, args.arg2) {
        Ok(()) => SyscallResult::ok(0),
        Err(e) => SyscallResult::err(e),
    }
}

/// Threshold (in 4 KiB pages) at which the mmap/munmap TLB flush switches
/// from a per-page range shootdown (`invlpg` per page on each CPU) to a
/// full TLB flush (CR3 reload).  Mirrors the `mprotect` path
/// (`MPROTECT_FULL_FLUSH_PAGES`) and Linux's `tlb_single_page_flush_ceiling`:
/// 64 × 4 KiB = 16 frames = 256 KiB — small enough that 64 `invlpg`s are
/// cheap, large enough that most (un)map calls take the range path.
const MMAP_FULL_FLUSH_PAGES: u64 = 64;

/// Flush the TLB on every online CPU for the frame-aligned range
/// `[start, end)`.  Used by `sys_mmap` (committed path) and `sys_munmap`
/// after they mutate page-table entries, since the underlying
/// `map_committed_range` / `unmap_frame` primitives leave TLB invalidation
/// to the caller.  Picks a per-page shootdown for small ranges and a full
/// flush for large ones.
#[allow(clippy::arithmetic_side_effects)]
fn mmap_flush_range(start: u64, end: u64) {
    if end <= start {
        return;
    }
    // Both are frame-aligned (16 KiB multiples), so this divides evenly
    // into 4 KiB hardware pages.
    let page_count = end.saturating_sub(start) / 4096;
    if page_count == 0 {
        return;
    }
    if page_count > MMAP_FULL_FLUSH_PAGES {
        // Large range — one CR3 reload per CPU beats N×invlpg.  Also covers
        // page_count > u32::MAX.
        crate::tlb::flush_all();
    } else {
        // page_count <= 64 — fits in u32.
        #[allow(clippy::cast_possible_truncation)]
        crate::tlb::flush_range(start, page_count as u32);
    }
}

// User mmap address-space layout.
//
// The user mmap window `0x0000_0060_0000_0000 .. 0x0000_0070_0000_0000`
// is split into two disjoint sub-regions so two allocators with different
// views of the address space can coexist without ever colliding:
//
// * **General region** (VMA-tracked).  Every mapping placed here —
//   anonymous mmap and file-backed mmap — registers a VMA, so
//   [`alloc_user_mmap_reserve`] can scan the process VMA list for a free gap
//   and *reuse* space freed by `munmap`.  This is the bulk of the window.
// * **Device region** (not VMA-tracked).  DRM dumb-buffer mmap maps device
//   frames directly without registering a VMA, so the gap finder cannot
//   see those mappings.  Keeping them in a disjoint window with a simple
//   bump allocator ([`mmap_alloc_vaddr`]) guarantees they never overlap a
//   gap-finder allocation.  (Reuse of freed device space remains future
//   work, tracked alongside the broader DRM mmap debt; device buffers are
//   few and long-lived, so a bump allocator is adequate.)

/// Base of the VMA-tracked general user mmap region (inclusive).
pub(crate) const USER_MMAP_BASE: u64 = 0x0000_0060_0000_0000;
/// End of the VMA-tracked general user mmap region (exclusive).
pub(crate) const USER_MMAP_END: u64 = 0x0000_006f_0000_0000;
/// Base of the device (non-VMA-tracked) mmap region (inclusive).
const DEVICE_MMAP_BASE: u64 = 0x0000_006f_0000_0000;
/// End of the device mmap region (exclusive).
const DEVICE_MMAP_END: u64 = 0x0000_0070_0000_0000;

/// Atomically reserve a free, frame-aligned virtual-address gap of `size`
/// bytes in the VMA-tracked general user mmap region of process `pid`,
/// registering a VMA of `kind`/`flags` there, and return the base address.
///
/// Scans the process VMA list via [`pcb::reserve_unmapped_area`] for the
/// lowest free gap and inserts the VMA under a single lock, so `munmap`'d
/// space is reused (no monotonic leak), the result can never overlap an
/// existing mapping, and two concurrent same-process mmaps can never be
/// handed the same gap.  Returns 0 if no record/gap is available (the
/// caller maps 0 to `OutOfMemory`/`ENOMEM`).
///
/// Because the VMA is inserted here, the caller must NOT register it again,
/// and must remove it (via [`pcb::remove_vma`]) if a later step fails.
pub(crate) fn alloc_user_mmap_reserve(
    pid: pcb::ProcessId,
    size: u64,
    kind: crate::mm::vma::VmaKind,
    flags: crate::mm::page_table::PageFlags,
) -> u64 {
    pcb::reserve_unmapped_area(pid, size, USER_MMAP_BASE, USER_MMAP_END, kind, flags)
        .unwrap_or(0)
}

/// Simple bump allocator for the device mmap region (DRM dumb buffers).
///
/// Device mappings are not VMA-tracked, so they live in a window disjoint
/// from the VMA-tracked general region (see the layout note above) and use
/// a monotonic bump allocator, which cannot collide with
/// [`alloc_user_mmap_reserve`].  Returns 0 when the device window is
/// exhausted.
pub(crate) fn mmap_alloc_vaddr(size: u64) -> u64 {
    use core::sync::atomic::{AtomicU64, Ordering};

    static NEXT_VADDR: AtomicU64 = AtomicU64::new(DEVICE_MMAP_BASE);

    let addr = NEXT_VADDR.fetch_add(size, Ordering::Relaxed);
    if addr.checked_add(size).is_none_or(|end| end > DEVICE_MMAP_END) {
        // Ran out of device mmap space.
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

    // Get sender PID.  A kernel task (no caller) maps to PID 0, which is
    // the default and needs no cap-table management.
    let sender_pid = caller_pid().unwrap_or_default();

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
        // An indefinite FUTEX_WAIT interrupted by a signal is restartable
        // (Linux returns ERESTARTSYS): re-running the syscall re-checks the
        // futex word, so a restart is correct.  The sentinel is resolved by the
        // signal-delivery checkpoint into a restart or a user-visible EINTR.
        Err(crate::error::KernelError::Interrupted) => {
            crate::syscall::linux::restart::restart_result(
                crate::syscall::linux::restart::ERESTARTSYS,
            )
        }
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

/// `FUTEX_WAIT_BITSET` backing (indefinite) — block if `*addr == expected`,
/// recording a wakeup bitset so only a matching [`sys_futex_wake_bitset`]
/// wakes us.  Used when the caller passed a NULL timeout (wait forever).
///
/// `arg0`: pointer to 32-bit futex word (4-byte aligned).
/// `arg1`: expected value.
/// `arg2`: the wakeup bitset (must be non-zero; the syscall layer rejects
///         a zero mask with `EINVAL` before reaching here).
///
/// Returns: 1 if blocked and woken, 0 if value didn't match.
pub fn sys_futex_wait_bitset(args: &SyscallArgs) -> SyscallResult {
    let addr = args.arg0;
    let expected = args.arg1 as u32;
    let bitset = args.arg2 as u32;

    if let Err(e) = crate::mm::user::validate_user_read(addr, 4) {
        return SyscallResult::err(e);
    }

    match futex::futex_wait_bitset(addr, expected, bitset) {
        Ok(true) => SyscallResult::ok(1),
        Ok(false) => SyscallResult::ok(0),
        // Indefinite FUTEX_WAIT_BITSET interrupted by a signal — restartable
        // (ERESTARTSYS), resolved by the signal-delivery checkpoint.
        Err(crate::error::KernelError::Interrupted) => {
            crate::syscall::linux::restart::restart_result(
                crate::syscall::linux::restart::ERESTARTSYS,
            )
        }
        Err(e) => SyscallResult::err(e),
    }
}

/// `FUTEX_WAIT_BITSET` backing (timed) — like [`sys_futex_wait_bitset`] but
/// with a relative nanosecond deadline.  Mirrors [`sys_futex_wait_timeout`]:
/// a `timeout_ns` of `0` is a non-blocking value check (returns `TimedOut`
/// on a value match), which is exactly the "absolute deadline already in
/// the past" case the translator resolves to `rel_ns == 0`.
///
/// `arg0`: pointer to 32-bit futex word (4-byte aligned).
/// `arg1`: expected value.
/// `arg2`: relative timeout in nanoseconds (0 = non-blocking check).
/// `arg3`: the wakeup bitset (non-zero; zero rejected upstream).
///
/// Returns: 1 if blocked and woken, 0 if value didn't match, `TimedOut`.
pub fn sys_futex_wait_bitset_timeout(args: &SyscallArgs) -> SyscallResult {
    let addr = args.arg0;
    let expected = args.arg1 as u32;
    let timeout_ns = args.arg2;
    let bitset = args.arg3 as u32;

    if let Err(e) = crate::mm::user::validate_user_read(addr, 4) {
        return SyscallResult::err(e);
    }

    match futex::futex_wait_bitset_timeout(addr, expected, timeout_ns, bitset) {
        Ok(true) => SyscallResult::ok(1),
        Ok(false) => SyscallResult::ok(0),
        Err(e) => SyscallResult::err(e),
    }
}

/// `FUTEX_WAKE_BITSET` backing — wake up to `max_wake` waiters whose
/// registered bitset shares a bit with `bitset`.
///
/// `arg0`: pointer to futex word.
/// `arg1`: maximum number of tasks to wake.
/// `arg2`: the wakeup bitset (non-zero; zero is rejected upstream).
///
/// Returns: number of tasks actually woken.
pub fn sys_futex_wake_bitset(args: &SyscallArgs) -> SyscallResult {
    let addr = args.arg0;
    let max_wake = args.arg1 as u32;
    let bitset = args.arg2 as u32;

    if let Err(e) = crate::mm::user::validate_user_ptr(addr) {
        return SyscallResult::err(e);
    }

    let woken = futex::futex_wake_bitset(addr, max_wake, bitset);
    SyscallResult::ok(i64::from(woken))
}

/// `SYS_FUTEX_REQUEUE` — wake N waiters on `addr1`, requeue M to `addr2`.
///
/// `arg0`: source futex address (`addr1`).
/// `arg1`: destination futex address (`addr2`; 0 = wake-only).
/// `arg2`: maximum number of tasks to wake from `addr1`.
/// `arg3`: maximum number of tasks to requeue to `addr2`.
///
/// Returns: total tasks affected (woken + requeued).
pub fn sys_futex_requeue(args: &SyscallArgs) -> SyscallResult {
    let addr1 = args.arg0;
    let addr2 = args.arg1;
    let max_wake = args.arg2 as u32;
    let max_requeue = args.arg3 as u32;

    // Both addresses serve as wait-queue keys.  Validate that addr1 is a
    // user pointer; validate addr2 only when it participates (non-zero).
    if let Err(e) = crate::mm::user::validate_user_ptr(addr1) {
        return SyscallResult::err(e);
    }
    if addr2 != 0 {
        if let Err(e) = crate::mm::user::validate_user_ptr(addr2) {
            return SyscallResult::err(e);
        }
    }

    let affected = futex::futex_requeue(addr1, addr2, max_wake, max_requeue);
    SyscallResult::ok(i64::from(affected))
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

/// `SYS_FUTEX_TRYLOCK_PI` — try to lock a PI futex without blocking.
///
/// `arg0`: pointer to 32-bit futex word (4-byte aligned).
///
/// Acquires the lock if uncontended.  If held by another task, returns
/// `WouldBlock` (`EAGAIN`).  If the caller already owns it, returns
/// `Deadlock` (`EDEADLK`).
///
/// Returns: 0 on success.
pub fn sys_futex_trylock_pi(args: &SyscallArgs) -> SyscallResult {
    let addr = args.arg0;

    // PI futex reads and writes the 4-byte futex word.
    if let Err(e) = crate::mm::user::validate_user_write(addr, 4) {
        return SyscallResult::err(e);
    }

    match futex::futex_trylock_pi(addr) {
        Ok(()) => SyscallResult::ok(0),
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_FUTEX_LOCK_PI_TIMEOUT` — lock a PI futex with a relative timeout.
///
/// `arg0`: pointer to 32-bit futex word (4-byte aligned).
/// `arg1`: timeout in nanoseconds (0 = try once, never block).
///
/// Acquires the lock, blocking up to `arg1` nanoseconds.  Returns
/// `TimedOut` (`ETIMEDOUT`) if the deadline expires first.
///
/// Returns: 0 on success.
pub fn sys_futex_lock_pi_timeout(args: &SyscallArgs) -> SyscallResult {
    let addr = args.arg0;
    let timeout_ns = args.arg1;

    // PI futex reads and writes the 4-byte futex word.
    if let Err(e) = crate::mm::user::validate_user_write(addr, 4) {
        return SyscallResult::err(e);
    }

    match futex::futex_lock_pi_timeout(addr, timeout_ns) {
        Ok(()) => SyscallResult::ok(0),
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_FUTEX_WAIT_REQUEUE_PI` — park on a condvar to be requeued onto a
/// PI mutex (the `pthread_cond_wait`-on-PI-mutex path).
///
/// `arg0`: condvar futex word pointer (readable, 4-byte aligned).
/// `arg1`: expected condvar value.
/// `arg2`: PI mutex futex word pointer (writable, 4-byte aligned).
/// `arg3`: timeout in nanoseconds (used only when `arg4` is non-zero).
/// `arg4`: timeout flag — 0 = wait indefinitely, non-zero = use `arg3`.
///
/// Returns: 0 on success (now owns the PI mutex).
pub fn sys_futex_wait_requeue_pi(args: &SyscallArgs) -> SyscallResult {
    let cond_addr = args.arg0;
    #[allow(clippy::cast_possible_truncation)]
    let expected = args.arg1 as u32;
    let pi_addr = args.arg2;
    let timeout_ns = if args.arg4 != 0 { Some(args.arg3) } else { None };

    // The condvar word is read; the PI mutex word is read and written.
    if let Err(e) = crate::mm::user::validate_user_read(cond_addr, 4) {
        return SyscallResult::err(e);
    }
    if let Err(e) = crate::mm::user::validate_user_write(pi_addr, 4) {
        return SyscallResult::err(e);
    }

    match futex::futex_wait_requeue_pi(cond_addr, expected, pi_addr, timeout_ns) {
        Ok(()) => SyscallResult::ok(0),
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_FUTEX_CMP_REQUEUE_PI` — signal a PI condvar, requeuing waiters
/// onto the PI mutex.
///
/// `arg0`: condvar futex word pointer (readable, 4-byte aligned).
/// `arg1`: PI mutex futex word pointer (writable, 4-byte aligned).
/// `arg2`: maximum number of waiters to requeue.
/// `arg3`: expected condvar value (mismatch → `EAGAIN`).
///
/// Returns: number of waiters affected (woken + requeued).
pub fn sys_futex_cmp_requeue_pi(args: &SyscallArgs) -> SyscallResult {
    let cond_addr = args.arg0;
    let pi_addr = args.arg1;
    #[allow(clippy::cast_possible_truncation)]
    let max_requeue = args.arg2 as u32;
    #[allow(clippy::cast_possible_truncation)]
    let expected = args.arg3 as u32;

    // The condvar word is read; the PI mutex word is read and written.
    if let Err(e) = crate::mm::user::validate_user_read(cond_addr, 4) {
        return SyscallResult::err(e);
    }
    if let Err(e) = crate::mm::user::validate_user_write(pi_addr, 4) {
        return SyscallResult::err(e);
    }

    match futex::futex_cmp_requeue_pi(cond_addr, pi_addr, max_requeue, expected) {
        Ok(n) => SyscallResult::ok(i64::from(n)),
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

/// `SYS_PIPE_PEEK` — copy buffered bytes out of a pipe without consuming them.
///
/// `arg0`: pipe handle (read end).
/// `arg1`: byte offset into the buffered data.
/// `arg2`: pointer to the caller's receive buffer.
/// `arg3`: buffer capacity.
///
/// Returns: bytes copied (0 at or past end of buffered data). The pipe contents
/// are left untouched — this is the non-destructive primitive behind `tee(2)`.
pub fn sys_pipe_peek(args: &SyscallArgs) -> SyscallResult {
    let handle = PipeHandle::from_raw(args.arg0);
    let offset = args.arg1;
    let buf_ptr = args.arg2 as *mut u8;
    let buf_cap = args.arg3 as usize;

    if buf_ptr.is_null() && buf_cap > 0 {
        return SyscallResult::err(KernelError::InvalidArgument);
    }

    if buf_cap > 0 {
        if let Err(e) = crate::mm::user::validate_user_write(args.arg2, buf_cap) {
            return SyscallResult::err(e);
        }
    }

    let buf = if buf_cap == 0 {
        &mut []
    } else {
        // SAFETY: Validated above — buf_ptr is in user space, mapped, and writable.
        unsafe { core::slice::from_raw_parts_mut(buf_ptr, buf_cap) }
    };

    match pipe::peek_at(handle, offset, buf) {
        Ok(n) => {
            #[allow(clippy::cast_possible_wrap)]
            let copied = n as i64;
            SyscallResult::ok(copied)
        }
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_PIPE_WAIT_READABLE` — block until a pipe has data or reaches EOF.
///
/// `arg0`: pipe handle (read end).
///
/// Returns: 1 if data is available, 0 on EOF (write end closed, buffer drained).
/// Consumes no bytes — the blocking-wait primitive `tee(2)` uses before peeking.
pub fn sys_pipe_wait_readable(args: &SyscallArgs) -> SyscallResult {
    let handle = PipeHandle::from_raw(args.arg0);
    match pipe::wait_readable(handle) {
        Ok(true) => SyscallResult::ok(1),
        Ok(false) => SyscallResult::ok(0),
        Err(e) => SyscallResult::err(e),
    }
}

// ---------------------------------------------------------------------------
// Stream socket handlers (300–310) — socketpair backing
// ---------------------------------------------------------------------------

/// `SYS_SOCKETPAIR_CREATE` — create a bidirectional stream socket pair.
///
/// Returns the two endpoint handles in `rax` and `rdx`.
pub fn sys_socketpair_create(args: &SyscallArgs) -> SyscallResult {
    let _ = args;
    let (ep0, ep1) = stream_socket::create();

    if let Some(pid) = caller_pid() {
        pcb::register_ipc_handle(pid, ResourceType::StreamSocket, ep0.raw());
        pcb::register_ipc_handle(pid, ResourceType::StreamSocket, ep1.raw());
    }

    #[allow(clippy::cast_possible_wrap)]
    let r0 = ep0.raw() as i64;
    #[allow(clippy::cast_possible_wrap)]
    let r1 = ep1.raw() as i64;
    SyscallResult::ok2(r0, r1)
}

/// `SYS_SOCKETPAIR_SEND` — send bytes on an endpoint (blocking).
pub fn sys_socketpair_send(args: &SyscallArgs) -> SyscallResult {
    let handle = StreamSocketHandle::from_raw(args.arg0);
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

    match stream_socket::send(handle, data) {
        Ok(n) => {
            #[allow(clippy::cast_possible_wrap)]
            let sent = n as i64;
            SyscallResult::ok(sent)
        }
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_SOCKETPAIR_RECV` — receive bytes from an endpoint (blocking).
pub fn sys_socketpair_recv(args: &SyscallArgs) -> SyscallResult {
    let handle = StreamSocketHandle::from_raw(args.arg0);
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

    match stream_socket::recv(handle, buf) {
        Ok(n) => {
            #[allow(clippy::cast_possible_wrap)]
            let recvd = n as i64;
            SyscallResult::ok(recvd)
        }
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_SOCKETPAIR_TRY_SEND` — non-blocking send.
pub fn sys_socketpair_try_send(args: &SyscallArgs) -> SyscallResult {
    let handle = StreamSocketHandle::from_raw(args.arg0);
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

    match stream_socket::try_send(handle, data) {
        Ok(n) => {
            #[allow(clippy::cast_possible_wrap)]
            let sent = n as i64;
            SyscallResult::ok(sent)
        }
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_SOCKETPAIR_TRY_RECV` — non-blocking receive.
pub fn sys_socketpair_try_recv(args: &SyscallArgs) -> SyscallResult {
    let handle = StreamSocketHandle::from_raw(args.arg0);
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
        // SAFETY: Validated above — buf_ptr is in user space, mapped, writable.
        unsafe { core::slice::from_raw_parts_mut(buf_ptr, buf_cap) }
    };

    match stream_socket::try_recv(handle, buf) {
        Ok(n) => {
            #[allow(clippy::cast_possible_wrap)]
            let recvd = n as i64;
            SyscallResult::ok(recvd)
        }
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_SOCKETPAIR_CLOSE` — close an endpoint handle.
pub fn sys_socketpair_close(args: &SyscallArgs) -> SyscallResult {
    let handle = StreamSocketHandle::from_raw(args.arg0);
    if let Some(pid) = caller_pid() {
        pcb::deregister_ipc_handle(pid, ResourceType::StreamSocket, handle.raw());
    }
    stream_socket::close(handle);
    SyscallResult::ok(0)
}

/// `SYS_SOCKETPAIR_SEND_TIMEOUT` — send with a deadline.
pub fn sys_socketpair_send_timeout(args: &SyscallArgs) -> SyscallResult {
    let handle = StreamSocketHandle::from_raw(args.arg0);
    let ptr = args.arg1 as *const u8;
    let len = args.arg2 as usize;
    let timeout_ns = args.arg3;

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

    match stream_socket::send_timeout(handle, data, timeout_ns) {
        Ok(n) => {
            #[allow(clippy::cast_possible_wrap)]
            let sent = n as i64;
            SyscallResult::ok(sent)
        }
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_SOCKETPAIR_RECV_TIMEOUT` — receive with a deadline.
pub fn sys_socketpair_recv_timeout(args: &SyscallArgs) -> SyscallResult {
    let handle = StreamSocketHandle::from_raw(args.arg0);
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
        // SAFETY: Validated above — buf_ptr is in user space, mapped, writable.
        unsafe { core::slice::from_raw_parts_mut(buf_ptr, buf_cap) }
    };

    match stream_socket::recv_timeout(handle, buf, timeout_ns) {
        Ok(n) => {
            #[allow(clippy::cast_possible_wrap)]
            let recvd = n as i64;
            SyscallResult::ok(recvd)
        }
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_SOCKETPAIR_POLL` — query endpoint readiness.
pub fn sys_socketpair_poll(args: &SyscallArgs) -> SyscallResult {
    let handle = StreamSocketHandle::from_raw(args.arg0);
    let flags = stream_socket::poll_status(handle);
    SyscallResult::ok(i64::from(flags))
}

/// `SYS_SOCKETPAIR_READABLE_BYTES` — bytes available to receive.
pub fn sys_socketpair_readable_bytes(args: &SyscallArgs) -> SyscallResult {
    let handle = StreamSocketHandle::from_raw(args.arg0);
    let bytes = stream_socket::readable_bytes(handle);
    #[allow(clippy::cast_possible_wrap)]
    let b = bytes as i64;
    SyscallResult::ok(b)
}

/// `SYS_SOCKETPAIR_SHUTDOWN` — shut down one or both directions.
pub fn sys_socketpair_shutdown(args: &SyscallArgs) -> SyscallResult {
    let handle = StreamSocketHandle::from_raw(args.arg0);
    let how = args.arg1 as u32;
    match stream_socket::shutdown(handle, how) {
        Ok(()) => SyscallResult::ok(0),
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
            if let Some(pid) = caller_pid()
                && pid != 0
            {
                pcb::register_ipc_handle(pid, ResourceType::SharedMemory, handle.raw());
                // The creating process is authorized to operate on its own
                // region (map/size/close). Only `InvalidHandle` can fail here,
                // impossible for a region we just created — safe to ignore.
                let _ = shm::authorize(handle, pid);
            }
            #[allow(clippy::cast_possible_wrap)]
            let h = handle.raw() as i64;
            SyscallResult::ok(h)
        }
        Err(e) => SyscallResult::err(e),
    }
}

/// Verify that a userspace caller is authorized to operate on a SHM region.
///
/// Returns `Ok(())` for kernel-context callers (no PID / PID 0 — the kernel is
/// the TCB) and for userspace processes that were granted access (the region's
/// creator, or a process authorized at a kernel→daemon handoff). Returns
/// `PermissionDenied` for any other userspace caller — this closes
/// D-SHM-MAP-NOCAP, where any process holding a raw handle value (which is just
/// the small monotonic region ID) could map another process's region.
fn shm_check_authorized(handle: ShmHandle) -> crate::error::KernelResult<()> {
    match caller_pid() {
        Some(pid) if pid != 0 => {
            if shm::is_authorized(handle, pid) {
                Ok(())
            } else {
                Err(KernelError::PermissionDenied)
            }
        }
        // Kernel task (no owning process, or PID 0): implicit authority.
        _ => Ok(()),
    }
}

/// `SYS_SHM_SIZE` — query the size of a shared memory region.
///
/// `arg0`: shared memory handle.
///
/// Returns: size in bytes.
pub fn sys_shm_size(args: &SyscallArgs) -> SyscallResult {
    let handle = ShmHandle::from_raw(args.arg0);

    if let Err(e) = shm_check_authorized(handle) {
        return SyscallResult::err(e);
    }

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
    if let Err(e) = shm_check_authorized(handle) {
        return SyscallResult::err(e);
    }
    if let Some(pid) = caller_pid() {
        pcb::deregister_ipc_handle(pid, ResourceType::SharedMemory, handle.raw());
    }
    shm::close(handle);
    SyscallResult::ok(0)
}

/// `SYS_SHM_MAP` — map a shared memory region into the caller's address space.
///
/// `arg0`: shared memory handle.
/// `arg1`: flags (`MAP_READ` | `MAP_WRITE`; execute is never granted — shared
///          data regions are not executable).
///
/// Returns: user virtual address of the mapping.
///
/// The region's physical frames are ref-counted. Mapping bumps each frame's
/// refcount (so the mapping keeps the memory alive even after every SHM handle
/// is closed); unmapping — via [`sys_shm_unmap`]/`SYS_MUNMAP` or process exit,
/// both of which use the refcount-aware `free_frame` — drops that reference.
/// This is what lets two *different* address spaces (e.g. the kernel-side
/// netstack forwarders and the ring-3 `netstack` daemon) share one region: the
/// last reference dropped frees the frames, in any order.
pub fn sys_shm_map(args: &SyscallArgs) -> SyscallResult {
    use crate::mm::frame::{self, PhysFrame, FRAME_SIZE};
    use crate::mm::page_table::{self, PageFlags, VirtAddr};
    use crate::mm::vma::VmaKind;
    use crate::proc::{pcb, thread};
    use super::number::{MAP_READ, MAP_WRITE};

    let handle = ShmHandle::from_raw(args.arg0);
    let flags = args.arg1;

    // Enforce region authorization: a userspace caller must be the region's
    // creator or have been granted access at a kernel→daemon handoff. Kernel
    // context is the TCB and always allowed. Closes D-SHM-MAP-NOCAP.
    if let Err(e) = shm_check_authorized(handle) {
        return SyscallResult::err(e);
    }

    // A mapping with no access requested is meaningless (and would map the
    // region PROT_NONE — reject rather than silently create dead PTEs).
    if flags & (MAP_READ | MAP_WRITE) == 0 {
        return SyscallResult::err(KernelError::InvalidArgument);
    }

    // Snapshot the region's backing frames (physical addresses, in order).
    let frame_addrs = match shm::frame_addrs(handle) {
        Ok(f) => f,
        Err(e) => return SyscallResult::err(e),
    };
    if frame_addrs.is_empty() {
        return SyscallResult::err(KernelError::InvalidHandle);
    }

    // The caller's address space.
    let task_id = sched::current_task_id();
    let pid = thread::owner_process(task_id).unwrap_or(0);
    let pml4_phys = match pcb::get_pml4(pid) {
        Some(p) if p != 0 => p,
        _ => return SyscallResult::err(KernelError::NoSuchProcess),
    };

    // User page flags. Always PRESENT + USER + NO_EXECUTE; WRITABLE opt-in.
    let mut page_flags = PageFlags::PRESENT | PageFlags::USER_ACCESSIBLE | PageFlags::NO_EXECUTE;
    if flags & MAP_WRITE != 0 {
        page_flags |= PageFlags::WRITABLE;
    }

    #[allow(clippy::arithmetic_side_effects)]
    let frame_size = FRAME_SIZE as u64;
    #[allow(clippy::cast_possible_truncation, clippy::arithmetic_side_effects)]
    let size_aligned = (frame_addrs.len() as u64) * frame_size;

    // Reserve a VA gap with a Fixed VMA (frames are pre-backed — a fault in
    // this range is a bug, not demand paging). The reservation inserts the
    // VMA atomically, so a rollback below must `remove_vma`.
    let base = alloc_user_mmap_reserve(pid, size_aligned, VmaKind::Fixed, page_flags);
    if base == 0 {
        return SyscallResult::err(KernelError::OutOfMemory);
    }

    // Roll back frames [0, up_to): unmap each and drop the ref we added.
    let rollback = |up_to: usize| {
        for j in 0..up_to {
            #[allow(clippy::arithmetic_side_effects)]
            let va = base + (j as u64) * frame_size;
            // SAFETY: these frames were just mapped by this function.
            if let Ok(phys) = unsafe { page_table::unmap_frame(pml4_phys, VirtAddr::new(va)) } {
                // SAFETY: refcount-aware; drops the ref_inc we added — the
                // region still holds its own reference, so this never frees.
                unsafe { let _ = frame::free_frame(phys); }
            }
        }
        pcb::remove_vma(pid, base);
    };

    for (i, &pa) in frame_addrs.iter().enumerate() {
        #[allow(clippy::arithmetic_side_effects)]
        let va = base + (i as u64) * frame_size;
        let phys = match PhysFrame::from_addr(pa) {
            Some(f) => f,
            None => {
                rollback(i);
                return SyscallResult::err(KernelError::InternalError);
            }
        };

        // Bump the refcount BEFORE mapping so a concurrent `shm::close` can
        // never free the frame out from under the new mapping.
        // SAFETY: `pa` is a live frame of a region that existed a moment ago
        // (frame_addrs came from a locked region lookup); ref_inc rejects a
        // frame whose refcount already hit 0.
        if let Err(e) = unsafe { frame::ref_inc(phys) } {
            rollback(i);
            return SyscallResult::err(e);
        }

        // SAFETY: pml4_phys is the caller's page table; phys is now a
        // refcounted live frame; va is a freshly reserved user VA.
        if let Err(e) = unsafe {
            page_table::map_frame(pml4_phys, VirtAddr::new(va), phys, page_flags)
        } {
            // Undo this frame's ref_inc (mapping never took), then roll back
            // the earlier frames.
            // SAFETY: refcount-aware; drops the ref we just added.
            unsafe { let _ = frame::free_frame(phys); }
            rollback(i);
            return SyscallResult::err(e);
        }
    }

    // Charge the mapped frames to the process RSS (mirrors the committed
    // mmap path; teardown resets RSS wholesale so no matching uncharge here).
    #[allow(clippy::cast_possible_truncation)]
    crate::mm::accounting::charge(pml4_phys, frame_addrs.len() as u64);

    #[allow(clippy::cast_possible_wrap)]
    SyscallResult::ok(base as i64)
}

/// `SYS_SHM_UNMAP` — unmap a shared memory region previously mapped with
/// [`sys_shm_map`].
///
/// `arg0`: user virtual address returned by `SYS_SHM_MAP`.
/// `arg1`: size in bytes.
///
/// This is exactly [`sys_munmap`]: SHM frames are allocator-owned, so the
/// generic munmap path unmaps each frame and drops its reference via the
/// refcount-aware `free_frame` (freeing the backing memory only when the last
/// mapper *and* every SHM handle are gone). Exposed under a distinct number so
/// callers can express intent; behaviourally identical.
pub fn sys_shm_unmap(args: &SyscallArgs) -> SyscallResult {
    sys_munmap(args)
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
    let fd_map_byte_len = fd_map_count.saturating_mul(core::mem::size_of::<FdMapEntry>());
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
        // SAFETY: name_ptr/name_len validated by validate_user_read above.
        let name_bytes = unsafe {
            core::slice::from_raw_parts(name_ptr as *const u8, name_len)
        };
        core::str::from_utf8(name_bytes).unwrap_or("unnamed")
    } else {
        "unnamed"
    };

    // Read the fd map entries.
    let fd_pairs: alloc::vec::Vec<(i32, u8, u64)> = if fd_map_count > 0 && fd_map_ptr != 0 {
        // SAFETY: fd_map_ptr/fd_map_count validated by validate_user_read above.
        let entries = unsafe {
            core::slice::from_raw_parts(fd_map_ptr as *const FdMapEntry, fd_map_count)
        };
        entries.iter().map(|e| (e.fd, e.handle_type, e.handle)).collect()
    } else {
        alloc::vec::Vec::new()
    };

    // Parse packed argv strings (null-terminated, concatenated).
    let argv_slices: alloc::vec::Vec<&[u8]> = if argc > 0 && argv_len > 0 && argv_ptr != 0 {
        // SAFETY: argv_ptr/argv_len validated by validate_user_read above.
        let data = unsafe {
            core::slice::from_raw_parts(argv_ptr as *const u8, argv_len)
        };
        parse_packed_strings(data, argc)
    } else {
        alloc::vec::Vec::new()
    };

    // Parse packed envp strings.
    let envp_slices: alloc::vec::Vec<&[u8]> = if envc > 0 && envp_len > 0 && envp_ptr != 0 {
        // SAFETY: envp_ptr/envp_len validated by validate_user_read above.
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
        let byte_len = count.saturating_mul(core::mem::size_of::<FdMapEntry>());

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

/// Write the reaped child PID to an optional userspace output pointer.
///
/// `out_ptr` is `arg1` from the wait syscalls: a user `*mut i32` where
/// the kernel stores which child was reaped, or 0 to skip.  This is how
/// `waitpid(-1)` learns which child it reaped (the exit code goes in
/// `rax`).  A bad pointer is ignored (best-effort) — the exit status is
/// still returned, matching the spirit of POSIX where a NULL/bad status
/// pointer doesn't fail the wait.
fn write_reaped_pid(out_ptr: u64, child_pid: crate::proc::pcb::ProcessId) {
    if out_ptr == 0 {
        return;
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    let pid_i32 = child_pid as i32;
    // SAFETY: write_user validates the pointer is a writable user range
    // before writing.  We ignore failure (best-effort status report).
    let _ = unsafe { crate::mm::user::write_user::<i32>(out_ptr, pid_i32) };
}

/// `SYS_PROCESS_WAIT` — wait for a child process to exit.
///
/// `arg0`: child process ID to wait for, interpreted as a signed value:
///   - `> 0`: wait for that specific child.
///   - `<= 0`: wait for *any* child (POSIX `waitpid(-1)`; process groups
///     are not yet implemented, so `0` and `< -1` also mean "any child").
///     `arg1`: optional user `*mut i32` that receives the reaped child PID
///     (0 = don't report).  The exit code is returned in `rax`.
///
/// If no child is ready, blocks the calling task until one exits.
/// Returns the exit code on success, or a negative error
/// (`NoChildProcess`/ECHILD when there are no children).
pub fn sys_process_wait(args: &SyscallArgs) -> SyscallResult {
    use crate::proc::pcb;

    let pid_arg = args.arg0 as i64;
    let out_ptr = args.arg1;
    // Resolve the calling process's PID so try_reap can verify the
    // parent–child relationship.  Falls back to 0 (kernel) for bare
    // kernel tasks, which matches processes spawned with parent=0.
    let parent_pid = caller_pid().unwrap_or(0);

    let task_id = sched::current_task_id();

    if pid_arg > 0 {
        // --- Wait for a specific child ---
        #[allow(clippy::cast_sign_loss)]
        let child_pid = pid_arg as u64;

        // Check-first, then register-and-recheck before blocking.
        //
        // We must NOT register the wait task until we've confirmed the
        // target really is our running child: `try_reap` returns
        // `PermissionDenied` for a non-child, and registering a waiter on
        // someone else's process would deliver a stale wake when that
        // process is later reaped.  Once we know the child is still
        // running, we register then re-check — if the child zombied in
        // the gap, `wake()` set `pending_wake` and the second `try_reap`
        // (or `block_current`) sees it, so there is no lost wakeup.  The
        // waiter flag lives on the child, which is destroyed when reaped,
        // so there is nothing stale to clean up on success.
        //
        // Specific-pid does NOT write `arg1`: callers using a 1-arg
        // syscall wrapper leave a stale (possibly valid) pointer in that
        // register, and writing it would corrupt their memory.  Only the
        // wait-any path, whose sole caller passes a real pointer, writes.
        loop {
            match pcb::try_reap(parent_pid, child_pid) {
                Ok(Some(info)) => {
                    #[allow(clippy::cast_possible_wrap)]
                    return SyscallResult::ok(info.exit_code as i64);
                }
                Ok(None) => {} // Still running — fall through to register.
                Err(e) => return SyscallResult::err(e),
            }

            // Register, then re-check to close the lost-wakeup race.
            if let Err(e) = pcb::set_wait_task(child_pid, task_id) {
                return SyscallResult::err(e);
            }
            match pcb::try_reap(parent_pid, child_pid) {
                Ok(Some(info)) => {
                    #[allow(clippy::cast_possible_wrap)]
                    return SyscallResult::ok(info.exit_code as i64);
                }
                Ok(None) => {
                    sched::block_current();
                    // Woken (or spurious) — loop and re-check.
                }
                Err(e) => return SyscallResult::err(e),
            }
        }
    } else {
        // --- Wait for any child (POSIX waitpid(-1)) ---
        //
        // Same register-before-check discipline, but the waiter flag
        // lives on the *parent* (this process), which survives reaping,
        // so we must clear it on every exit path to avoid delivering a
        // stale wake to a later, unrelated `block_current`.
        loop {
            if let Err(e) = pcb::set_wait_any_task(parent_pid, task_id) {
                return SyscallResult::err(e);
            }
            match pcb::try_reap_any(parent_pid) {
                Ok(Some((child_pid, info))) => {
                    pcb::clear_wait_any_task(parent_pid, task_id);
                    write_reaped_pid(out_ptr, child_pid);
                    #[allow(clippy::cast_possible_wrap)]
                    return SyscallResult::ok(info.exit_code as i64);
                }
                Ok(None) => {
                    // Living children exist but none are zombies — block
                    // until one exits, then re-scan.
                    sched::block_current();
                }
                Err(e) => {
                    // No children left (ECHILD) or other error.
                    pcb::clear_wait_any_task(parent_pid, task_id);
                    return SyscallResult::err(e);
                }
            }
        }
    }
}

/// `SYS_PROCESS_TRY_WAIT` — non-blocking wait for a child process.
///
/// Like `SYS_PROCESS_WAIT` but returns `WouldBlock` immediately if no
/// matching child is ready, instead of blocking the caller.  `arg0` and
/// `arg1` have the same meaning as in [`sys_process_wait`].
pub fn sys_process_try_wait(args: &SyscallArgs) -> SyscallResult {
    use crate::proc::pcb;

    let pid_arg = args.arg0 as i64;
    let out_ptr = args.arg1;
    // Resolve calling process PID for parent–child verification.
    let parent_pid = caller_pid().unwrap_or(0);

    if pid_arg > 0 {
        #[allow(clippy::cast_sign_loss)]
        let child_pid = pid_arg as u64;
        match pcb::try_reap(parent_pid, child_pid) {
            Ok(Some(info)) => {
                // See sys_process_wait: specific-pid does not write arg1.
                #[allow(clippy::cast_possible_wrap)]
                SyscallResult::ok(info.exit_code as i64)
            }
            Ok(None) => SyscallResult::err(KernelError::WouldBlock),
            Err(e) => SyscallResult::err(e),
        }
    } else {
        match pcb::try_reap_any(parent_pid) {
            Ok(Some((child_pid, info))) => {
                write_reaped_pid(out_ptr, child_pid);
                #[allow(clippy::cast_possible_wrap)]
                SyscallResult::ok(info.exit_code as i64)
            }
            Ok(None) => SyscallResult::err(KernelError::WouldBlock),
            Err(e) => SyscallResult::err(e),
        }
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

/// `SYS_PROCESS_PARENT_ID` — get the parent PID of the calling process.
///
/// Returns: parent PID on success, 0 if the calling task isn't owned by
/// any process (kernel thread) or if the process has no recorded parent
/// (init/pid 1, or a process whose parent has already been reaped).
pub fn sys_process_parent_id(args: &SyscallArgs) -> SyscallResult {
    let _ = args;
    use crate::proc::{thread, pcb};

    let task_id = sched::current_task_id();
    let Some(pid) = thread::owner_process(task_id) else {
        return SyscallResult::ok(0);
    };
    let ppid = pcb::parent(pid).unwrap_or(0);

    #[allow(clippy::cast_possible_wrap)]
    SyscallResult::ok(ppid as i64)
}

/// `SYS_PROCESS_COUNT` — return the count of live processes.
///
/// Wraps `pcb::count()`, which returns the size of the process table
/// (all live entries regardless of state).  Used by `sysinfo()` to fill
/// `struct sysinfo.procs`.
///
/// Saturating-casts the `usize` table size to `i64`.  On a 64-bit host
/// this only matters if more than `i64::MAX` processes exist, which is
/// impossible in practice but guarded against for forward-compatibility.
pub fn sys_process_count(args: &SyscallArgs) -> SyscallResult {
    let _ = args;
    use crate::proc::pcb;

    let n = pcb::count();
    #[allow(clippy::cast_possible_wrap, clippy::cast_possible_truncation)]
    let clamped: i64 = if n > i64::MAX as usize { i64::MAX } else { n as i64 };
    SyscallResult::ok(clamped)
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

// ---------------------------------------------------------------------------
// POSIX signal-shim handlers
// ---------------------------------------------------------------------------

/// Resolve the calling task's owning process, or return `NoSuchProcess`.
///
/// Kernel threads (pid 0) are not valid signal targets/callers.
fn caller_process_or_err() -> Result<crate::proc::pcb::ProcessId, KernelError> {
    let task_id = sched::current_task_id();
    match crate::proc::thread::owner_process(task_id) {
        Some(pid) if pid != 0 => Ok(pid),
        _ => Err(KernelError::NoSuchProcess),
    }
}

/// `SYS_SIGNAL_REGISTER` — register the process-wide signal trampoline.
///
/// `arg0`: trampoline address (0 to unregister).
pub fn sys_signal_register(
    args: &super::dispatch::SyscallArgs,
) -> super::dispatch::SyscallResult {
    use super::dispatch::SyscallResult;
    let pid = match caller_process_or_err() {
        Ok(p) => p,
        Err(e) => return SyscallResult::err(e),
    };
    crate::proc::signal::register_trampoline(pid, args.arg0);
    SyscallResult::ok(0)
}

/// `SYS_SIGNAL_SEND` — post a signal to a target process.
///
/// `arg0`: target PID. `arg1`: signal number (1..=NSIG).
///
/// Authority matches `SYS_PROCESS_KILL`: the caller must be the target's
/// parent, PID 0, the target itself (self-signal), or hold a Process
/// capability with DELETE rights for the target.
pub fn sys_signal_send(
    args: &super::dispatch::SyscallArgs,
) -> super::dispatch::SyscallResult {
    // Default sender class for a process-directed `kill(2)`.
    sys_signal_send_with_code(args, crate::proc::signal::si_code::SI_USER)
}

/// Like [`sys_signal_send`], but stamps an explicit `si_code` into the
/// delivered `siginfo_t`.
///
/// `kill(2)` uses `SI_USER`; `tkill`/`tgkill` (and thus `raise`/`pthread_kill`)
/// use `SI_TKILL`. The sender pid/uid recorded in the siginfo are the caller's,
/// matching Linux's `do_send_sig_info`/`do_tkill`.
pub fn sys_signal_send_with_code(
    args: &super::dispatch::SyscallArgs,
    si_code: i32,
) -> super::dispatch::SyscallResult {
    // kill/tkill/tgkill carry no data word — stamp si_value = 0.
    sys_signal_send_with_info(args, si_code, 0)
}

/// Like [`sys_signal_send_with_code`], but also stamps an `si_value` data word
/// into the delivered `siginfo_t`.
///
/// This is the funnel for `rt_sigqueueinfo(2)`/`sigqueue(3)` (`SI_QUEUE`),
/// where the sender attaches a `union sigval` payload the receiving
/// `SA_SIGINFO` handler reads as `info->si_value`. The sender pid/uid recorded
/// are still the caller's real identity (matching Linux's `prepare_signal`),
/// not a value the caller can forge.
pub fn sys_signal_send_with_info(
    args: &super::dispatch::SyscallArgs,
    si_code: i32,
    value: u64,
) -> super::dispatch::SyscallResult {
    use crate::proc::{pcb, signal, thread};
    use super::dispatch::SyscallResult;

    let target = args.arg0;
    #[allow(clippy::cast_possible_truncation)]
    let sig = args.arg1 as u32;

    if target == 0 {
        return SyscallResult::err(KernelError::InvalidArgument);
    }
    if !signal::is_valid_signal(sig) {
        return SyscallResult::err(KernelError::InvalidArgument);
    }

    let task_id = sched::current_task_id();
    let caller = thread::owner_process(task_id).unwrap_or(0);

    // Existence + authority. A self-signal is always permitted.
    if target != caller {
        let target_parent = match pcb::parent(target) {
            Some(p) => p,
            None => return SyscallResult::err(KernelError::NoSuchProcess),
        };
        let has_parent_auth = caller == 0 || caller == target_parent;
        let has_cap_auth = pcb::has_capability_for(
            caller,
            crate::cap::ResourceType::Process,
            target,
            crate::cap::Rights::DELETE,
        );
        if !has_parent_auth && !has_cap_auth {
            return SyscallResult::err(KernelError::PermissionDenied);
        }
    }

    // Reject signals to a dead/unknown process.
    match pcb::state(target) {
        Some(pcb::ProcessState::Zombie) => {
            return SyscallResult::err(KernelError::ProcessExited);
        }
        None => return SyscallResult::err(KernelError::NoSuchProcess),
        _ => {}
    }

    // Record the sender identity (caller pid + real uid) so an SA_SIGINFO
    // handler on the target sees a faithful siginfo_t. `value` is the queued
    // `si_value` payload (0 for the plain kill/tkill/tgkill path).
    #[allow(clippy::cast_possible_truncation)]
    let info = signal::SigInfo {
        code: si_code,
        sender_pid: caller as u32,
        sender_uid: pcb::process_uid(caller).unwrap_or(0),
        value,
    };
    match signal::classify_post_info(target, sig, info) {
        signal::PostDecision::Deliver | signal::PostDecision::Drop => {
            SyscallResult::ok(0)
        }
        signal::PostDecision::Terminate(code) => {
            // No userspace handler (or SIGKILL): terminate like kill().
            if let Err(e) = pcb::set_exit_code(target, code) {
                return SyscallResult::err(e);
            }
            thread::kill_process_threads(target);
            serial_println!(
                "[signal] Process {} terminated by signal {} (from {})",
                target, sig, caller
            );
            SyscallResult::ok(0)
        }
        signal::PostDecision::Stop(s) => {
            // Suspend the target's threads for job control. If the caller is
            // signalling itself, this is a self-stop: pass the current task
            // so `stop_process_for_signal` parks it last (it yields and only
            // returns on a later SIGCONT).
            let self_task = if target == caller {
                Some(task_id)
            } else {
                None
            };
            stop_process_for_signal(target, s, self_task);
            SyscallResult::ok(0)
        }
        signal::PostDecision::Continue => {
            // Resume the target's threads. A pending SIGCONT handler (set by
            // classify_post when a trampoline is registered) runs on the
            // target's next return to userspace.
            continue_process(target);
            SyscallResult::ok(0)
        }
    }
}

/// `SYS_SIGNAL_MASK` — set the calling process's blocked-signal mask.
///
/// `arg0`: new blocked mask. `arg1`: out-pointer for the old mask (0 to
/// discard).
pub fn sys_signal_mask(
    args: &super::dispatch::SyscallArgs,
) -> super::dispatch::SyscallResult {
    use super::dispatch::SyscallResult;
    let pid = match caller_process_or_err() {
        Ok(p) => p,
        Err(e) => return SyscallResult::err(e),
    };
    let old = crate::proc::signal::set_blocked(pid, args.arg0);
    if args.arg1 != 0 {
        if let Err(e) = crate::mm::user::validate_user_write(
            args.arg1,
            core::mem::size_of::<u64>(),
        ) {
            return SyscallResult::err(e);
        }
        // SAFETY: validated as a writable user pointer of u64 size above.
        unsafe {
            core::ptr::write(args.arg1 as *mut u64, old);
        }
    }
    SyscallResult::ok(0)
}

/// `SYS_SIGNAL_PENDING` — query the calling process's pending set.
///
/// `arg0`: out-pointer for the pending mask.
pub fn sys_signal_pending(
    args: &super::dispatch::SyscallArgs,
) -> super::dispatch::SyscallResult {
    use super::dispatch::SyscallResult;
    let pid = match caller_process_or_err() {
        Ok(p) => p,
        Err(e) => return SyscallResult::err(e),
    };
    if args.arg0 == 0 {
        return SyscallResult::err(KernelError::InvalidArgument);
    }
    if let Err(e) = crate::mm::user::validate_user_write(
        args.arg0,
        core::mem::size_of::<u64>(),
    ) {
        return SyscallResult::err(e);
    }
    let pending = crate::proc::signal::pending(pid);
    // SAFETY: validated as a writable user pointer of u64 size above.
    unsafe {
        core::ptr::write(args.arg0 as *mut u64, pending);
    }
    SyscallResult::ok(0)
}

/// `SYS_SIGNAL_RETURN` — resume from a signal handler (sigreturn).
///
/// `arg0`: pointer to the `SignalContext` on the user stack.
///
/// Restores the interrupted CPU state by rewriting the syscall frame
/// (like `SYS_EXCEPTION_RETURN`). Handled as a special case in
/// `syscall_handler_inner`. Does not return to the caller.
pub fn sys_signal_return_with_frame(
    frame: &mut super::entry::SyscallFrame,
) -> i64 {
    use crate::proc::signal::SignalContext;

    if let Err(e) = crate::mm::user::validate_user_read(
        frame.arg0,
        core::mem::size_of::<SignalContext>(),
    ) {
        return e.code() as i64;
    }

    let ctx_ptr = frame.arg0 as *const SignalContext;
    // SAFETY: validated above — ctx_ptr is a mapped, readable user pointer
    // sized for SignalContext. The kernel wrote it during delivery; the
    // handler may have adjusted fields.
    let ctx = unsafe { &*ctx_ptr };

    // Restore the interrupted SYSRET frame.
    frame.user_rip = ctx.rip;
    frame.user_rsp = ctx.rsp;
    frame.user_rflags = ctx.rflags;
    frame.arg0 = ctx.rdi;
    frame.arg1 = ctx.rsi;
    frame.arg2 = ctx.rdx;
    frame.arg3 = ctx.r10;
    frame.arg4 = ctx.r8;
    frame.arg5 = ctx.r9;
    frame.rbx = ctx.rbx;
    frame.rbp = ctx.rbp;
    frame.r12 = ctx.r12;
    frame.r13 = ctx.r13;
    frame.r14 = ctx.r14;
    frame.r15 = ctx.r15;

    // Restore the interrupted syscall's return value into RAX.
    #[allow(clippy::cast_possible_wrap)]
    {
        ctx.rax as i64
    }
}

/// Terminate the current process because a fatal signal was posted and no
/// userspace handler trampoline is registered.
///
/// Mirrors the kernel default action for a terminating signal: the process
/// exits with the conventional "killed by signal N" status (`128 + sig`),
/// and every thread of the process is torn down. This is invoked from the
/// syscall-return delivery checkpoint ([`deliver_pending_signal`]) for a
/// process that has no trampoline — the same self-termination mechanism
/// `sys_exit` uses, so it is safe to call here: `task_exit()` switches away
/// and never returns.
///
/// Closes the gap where an asynchronously-posted fatal signal (e.g.
/// `ITIMER_REAL`'s `SIGALRM`, or a future keyboard `SIGINT`) to a process
/// with no handler would otherwise sit pending forever instead of killing
/// it. (`kill()` to a non-POSIX process already terminates synchronously via
/// `signal::classify_post`.)
fn terminate_current_process_for_signal(
    pid: crate::proc::pcb::ProcessId,
    current_task: crate::sched::task::TaskId,
    sig: u32,
) {
    use crate::proc::{pcb, thread};

    // 128 + sig is the conventional wait-status for "terminated by signal".
    #[allow(clippy::cast_possible_wrap)]
    let exit_code = 128i32.wrapping_add(sig as i32);
    let _ = pcb::set_exit_code(pid, exit_code);

    // Tear down sibling threads first. `kill_task` refuses the *current*
    // task (it must self-terminate via `task_exit`), so we only kill the
    // others here and finish with the current thread below.
    if let Some(threads) = pcb::get_threads(pid) {
        for t in threads {
            if t != current_task {
                sched::kill_task(t);
                thread::on_thread_exit(t);
            }
        }
    }

    // Tear down the current thread and switch away. `task_exit` never
    // returns; the abandoned kernel stack is reaped by the scheduler.
    thread::on_thread_exit(current_task);
    sched::task_exit();
}

/// Wake the parent-side observers of a job-control transition.
///
/// Mirrors the zombie-transition wake in `thread::on_thread_exit`: `.0` is a
/// task blocked in `waitpid(pid)` for this specific child; `.1` is a task in
/// the parent blocked in `waitpid(-1)`. Each re-registers on its next
/// blocking wait if the report does not match its `wait` options.
fn wake_jc_waiters(waiters: crate::proc::pcb::JcWaiters) {
    let (wake_task, any_waiter) = waiters;
    if let Some(t) = wake_task {
        sched::wake(t);
    }
    if let Some(t) = any_waiter {
        sched::wake(t);
    }
}

/// Stop a process for job control: suspend all its threads and record the
/// stop so the parent's `wait4`/`waitid` can observe it (WUNTRACED/WSTOPPED).
///
/// `current_task` is `Some(tid)` only for a **self-stop** — when the running
/// thread itself belongs to the target (e.g. `raise(SIGSTOP)`, or a stop at
/// the syscall-return checkpoint). In that case the current thread is
/// suspended **last**, because `sched::suspend(current)` yields and only
/// returns once a `SIGCONT` resumes the thread; suspending it first would
/// strand the remaining siblings un-suspended. For a cross-process stop the
/// current thread is not part of the target, so `current_task` is `None` and
/// every thread is suspended immediately before this returns.
///
/// Records the stop and wakes the parent's waiters *before* parking the
/// current thread, so a tracing parent observes the stop without having to
/// wait for this thread to be resumed.
fn stop_process_for_signal(
    pid: crate::proc::pcb::ProcessId,
    sig: u32,
    current_task: Option<crate::sched::task::TaskId>,
) {
    use crate::proc::pcb;

    // Suspend every thread except the current one (if this is a self-stop).
    let mut self_thread: Option<crate::sched::task::TaskId> = None;
    if let Some(threads) = pcb::get_threads(pid) {
        for t in threads {
            if Some(t) == current_task {
                self_thread = Some(t);
                continue;
            }
            sched::suspend(t);
        }
    }

    // Mark stopped and wake parent observers. If the process vanished
    // between the signal check and here, there is nothing to record.
    if let Ok(waiters) = pcb::record_jc_stopped(pid, sig) {
        wake_jc_waiters(waiters);
    }

    serial_println!("[signal] Process {} stopped by signal {}", pid, sig);

    // Finally park the current thread (self-stop only). Returns when a
    // SIGCONT resumes us via `continue_process`.
    if let Some(t) = self_thread {
        sched::suspend(t);
    }
}

/// Continue a stopped process: resume all its suspended threads and record
/// the continue so the parent's `wait4`/`waitid` can observe it (WCONTINUED).
///
/// `sched::resume` is a no-op for threads that are not Suspended, so calling
/// this on a process that is not actually stopped is harmless.
fn continue_process(pid: crate::proc::pcb::ProcessId) {
    use crate::proc::pcb;

    if let Some(threads) = pcb::get_threads(pid) {
        for t in threads {
            sched::resume(t);
        }
    }

    if let Ok(waiters) = pcb::record_jc_continued(pid) {
        wake_jc_waiters(waiters);
    }

    serial_println!("[signal] Process {} continued", pid);
}

/// Post a kernel-originated signal to `pid` and carry out its default action.
///
/// This is the authority-free sibling of [`sys_signal_send_with_info`]: there
/// is no caller, so no parent/capability check is performed. It is used for
/// signals the kernel itself raises against a process it is not "calling" from
/// — currently the orphaned-process-group `SIGHUP`/`SIGCONT` on session-leader
/// exit. Zombie/unknown targets are skipped silently (the process raced us to
/// exit, which is exactly the orphan case we are handling).
///
/// The signal is classified with [`signal::classify_post_info`] and acted on
/// identically to the user-`kill` path: a userspace handler simply leaves the
/// signal pending (`Deliver`); the default action terminates, stops, or
/// continues the process. There is never a self-stop here (the caller is the
/// exiting thread, not a member of the target group), so `stop_process_for_signal`
/// is always called with `None`.
pub fn deliver_kernel_signal(pid: crate::proc::pcb::ProcessId, sig: u32) {
    use crate::proc::{pcb, signal, thread};

    if !signal::is_valid_signal(sig) {
        return;
    }
    // Skip dead/unknown targets: a zombie cannot be signalled and an absent
    // pid has nothing to receive.
    match pcb::state(pid) {
        Some(pcb::ProcessState::Zombie) | None => return,
        _ => {}
    }

    match signal::classify_post_info(pid, sig, signal::SigInfo::kernel()) {
        signal::PostDecision::Deliver | signal::PostDecision::Drop => {}
        signal::PostDecision::Terminate(code) => {
            if pcb::set_exit_code(pid, code).is_ok() {
                thread::kill_process_threads(pid);
                serial_println!(
                    "[signal] Process {} terminated by kernel signal {}",
                    pid, sig
                );
            }
        }
        signal::PostDecision::Stop(s) => {
            stop_process_for_signal(pid, s, None);
        }
        signal::PostDecision::Continue => {
            continue_process(pid);
        }
    }
}

/// POSIX orphaned-process-group hangup: if `pgid` is now an orphaned process
/// group (no live member has a parent in a different group of the same
/// session) that still contains a **stopped** member, send `SIGHUP` followed
/// by `SIGCONT` to every member.
///
/// This is the action required by POSIX "Orphaned Process Group" semantics
/// (`setpgid(2)`, `termios(3)`): when a controlling process / guardian exits
/// and leaves a process group with stopped jobs and no shell able to continue
/// them, the kernel hangs the group up and then continues it so the jobs are
/// not wedged forever. Groups that are not orphaned, are empty, or have no
/// stopped member are left untouched.
///
/// Call after the orphaning exit has completed (i.e. once the exiting process
/// is a zombie and its children reparented) so the orphan re-check sees the
/// post-exit topology.
pub fn kill_orphaned_pgrp(pgid: crate::proc::pcb::ProcessId) {
    use crate::proc::{pcb, signal};

    if !pcb::pgrp_orphaned_with_stopped(pgid) {
        return;
    }
    serial_println!(
        "[signal] Orphaned process group {} with stopped jobs: SIGHUP+SIGCONT",
        pgid
    );
    // SIGHUP first (default-terminates members without a handler), then
    // SIGCONT to wake any that stopped and survived (a member with a SIGHUP
    // handler stays alive and must still be continued).
    for member in pcb::pids_in_group(pgid) {
        deliver_kernel_signal(member, signal::SIGHUP);
    }
    for member in pcb::pids_in_group(pgid) {
        deliver_kernel_signal(member, signal::SIGCONT);
    }
}

/// Deliver a pending signal to the current process on the way back to
/// userspace, if one is deliverable.
///
/// If the process has a handler trampoline registered, this mirrors the
/// SEH-style exception delivery (`idt::try_dispatch_user_exception`): build a
/// [`SignalContext`](crate::proc::signal::SignalContext) on the user stack
/// capturing the interrupted state (including the syscall's return value in
/// RAX), then rewrite the syscall frame so the SYSRET path jumps to the
/// trampoline with `rdi = signum` and `rsi = &ctx`.
///
/// If the process has **no** trampoline, the kernel default action applies
/// instead: a terminating signal kills the process (exit `128 + sig`, via
/// [`terminate_current_process_for_signal`] — this call does not return),
/// while ignore/stop/continue defaults are consumed and dropped.
///
/// `ret_val` is the value the interrupted syscall was about to return in
/// RAX; it is saved into the context and restored on `SYS_SIGNAL_RETURN`.
///
/// Returns `true` if a signal was delivered to a handler (the frame was
/// rewritten), `false` otherwise (the normal return value should be used).
/// Note that a fatal no-handler signal does not return at all.
///
/// If the user stack cannot hold the context (e.g. it would cross into an
/// unmapped guard page), delivery is skipped and the signal stays pending
/// — it will be retried on the next return to userspace. This avoids
/// corrupting memory; a proper alternate signal stack (`sigaltstack`) is
/// a documented future enhancement.
pub fn deliver_pending_signal(
    frame: &mut super::entry::SyscallFrame,
    ret_val: i64,
) -> bool {
    use crate::proc::signal::{self, SignalContext, SIGNAL_CONTEXT_SIZE};

    // Fast path: nothing pending anywhere.
    if !signal::any_pending() {
        return false;
    }

    let task_id = sched::current_task_id();
    let pid = match crate::proc::thread::owner_process(task_id) {
        Some(pid) if pid != 0 => pid,
        _ => return false,
    };

    // Linux-ABI processes get a byte-exact Linux `rt_sigframe` so an
    // unmodified glibc/WINE handler sees correct siginfo/ucontext and
    // returns via its own `sa_restorer` → `rt_sigreturn`. The native
    // SEH-style `SignalContext` trampoline path below is for native
    // POSIX-shim processes only.
    if crate::proc::pcb::get_abi_mode(pid)
        == Some(crate::proc::pcb::AbiMode::Linux)
    {
        return deliver_linux_signal(frame, ret_val, pid, task_id);
    }

    let trampoline = match signal::trampoline(pid) {
        Some(addr) => addr,
        None => {
            // No userspace handler trampoline: apply the kernel default
            // action to each deliverable signal. A terminating default kills
            // the process (exit 128+sig, never returns); ignore/stop/continue
            // defaults are consumed and dropped. This closes the gap where an
            // async-posted fatal signal (e.g. ITIMER_REAL's SIGALRM) to a
            // process with no handler would sit pending forever.
            while let Some(sig) = signal::take_deliverable(pid) {
                match signal::default_action(sig) {
                    signal::DefaultAction::Terminate => {
                        terminate_current_process_for_signal(pid, task_id, sig);
                        // Unreachable: task_exit never returns.
                    }
                    signal::DefaultAction::Stop => {
                        // A stop signal that was blocked at post time (kept
                        // pending) is now deliverable: self-stop at the
                        // checkpoint. Discard any pending SIGCONT first
                        // (mutual cancellation now that the stop takes
                        // effect), then park this thread (and siblings).
                        // `stop_process_for_signal` returns once a SIGCONT
                        // resumes us; the loop then re-checks for more.
                        signal::discard_pending_cont(pid);
                        stop_process_for_signal(pid, sig, Some(task_id));
                    }
                    signal::DefaultAction::Ignore
                    | signal::DefaultAction::Continue => {
                        // Dropped; check the next.
                    }
                }
            }
            return false;
        }
    };

    let sig = match signal::take_deliverable(pid) {
        Some(s) => s,
        None => return false,
    };

    // Compute the placement of the SignalContext on the user stack.
    //
    //   sp = user_rsp
    //   sp -= ctx_size; sp &= !0xF;   (16-byte aligned context)
    //   ctx_addr = sp
    //   sp -= 8;                       (fake return slot — null)
    //   new_rsp = sp                   (RSP%16 == 8 at handler entry,
    //                                   matching the SysV call convention)
    let ctx_size = SIGNAL_CONTEXT_SIZE as u64;
    let ctx_addr = (frame.user_rsp.wrapping_sub(ctx_size)) & !0xFu64;
    let new_rsp = ctx_addr.wrapping_sub(8);

    // Validate the whole region [new_rsp, ctx_addr + ctx_size) is a
    // writable user mapping before touching it.
    let region_len = (ctx_addr.wrapping_add(ctx_size)).wrapping_sub(new_rsp);
    if crate::mm::user::validate_user_write(new_rsp, region_len as usize).is_err()
    {
        // Cannot place the frame; re-arm the signal and skip delivery.
        signal::set_pending(pid, sig);
        return false;
    }

    // A restart sentinel must never be saved into the native trampoline
    // context: native processes have no per-signal SA_RESTART disposition, so
    // convert any sentinel to the user-visible -EINTR. (No native syscall emits
    // a sentinel today; this keeps the never-leak invariant safe-by-
    // construction if one ever does.)
    let ret_val = crate::syscall::linux::restart::leaked_sentinel_to_eintr(ret_val);

    let ctx = SignalContext {
        signum: u64::from(sig),
        rax: ret_val as u64,
        rdi: frame.arg0,
        rsi: frame.arg1,
        rdx: frame.arg2,
        r10: frame.arg3,
        r8: frame.arg4,
        r9: frame.arg5,
        rbx: frame.rbx,
        rbp: frame.rbp,
        r12: frame.r12,
        r13: frame.r13,
        r14: frame.r14,
        r15: frame.r15,
        rip: frame.user_rip,
        rsp: frame.user_rsp,
        rflags: frame.user_rflags,
    };

    // SAFETY: the region was validated as writable user memory above, and
    // CR3 still points at this process's address space (we are returning
    // to it). ctx_addr is 16-byte aligned and within the region.
    unsafe {
        core::ptr::write(ctx_addr as *mut SignalContext, ctx);
        core::ptr::write(new_rsp as *mut u64, 0u64); // null return address
    }

    // Rewrite the frame so SYSRET jumps to the trampoline.
    frame.user_rip = trampoline;
    frame.user_rsp = new_rsp;
    frame.arg0 = u64::from(sig); // rdi = signum
    frame.arg1 = ctx_addr; // rsi = &SignalContext
    // Clear other argument registers for cleanliness.
    frame.arg2 = 0;
    frame.arg3 = 0;
    frame.arg4 = 0;
    frame.arg5 = 0;

    true
}

/// Deliver pending signals to a **Linux-ABI** process at the
/// syscall-return checkpoint.
///
/// Unlike the native path (one trampoline pointer per process), a Linux
/// process has a per-signal `struct sigaction` disposition. This loop
/// consumes deliverable signals lowest-first; for each:
///
///   * **Handler** — build a Linux `rt_sigframe`
///     ([`crate::syscall::linux::build_linux_rt_frame`]) and enter the
///     handler. One handler per return-to-user (matching the native
///     path and Linux's "handle one, re-check on `rt_sigreturn`" model),
///     so we stop and return `true` once a frame is built. If the stack
///     cannot hold the frame the signal is re-armed and we return
///     `false`.
///   * **Ignore** (`SIG_IGN`) — drop and continue to the next signal.
///   * **Default** (`SIG_DFL`) — apply the kernel default action
///     (terminate / stop / ignore / continue), reusing the same helpers
///     as the native no-handler path. A terminating default never
///     returns.
///
/// Returns `true` if a handler frame was built (frame rewritten),
/// `false` if nothing was delivered to a handler.
fn deliver_linux_signal(
    frame: &mut super::entry::SyscallFrame,
    ret_val: i64,
    pid: crate::proc::pcb::ProcessId,
    task_id: crate::sched::task::TaskId,
) -> bool {
    use crate::proc::signal;
    use crate::syscall::linux::{self, LinuxDisposition};

    loop {
        let (sig, info) = match signal::take_deliverable_info(pid) {
            Some(t) => t,
            None => {
                // No (more) deliverable signal. If a `sigsuspend` saved a
                // mask but no handler frame consumed it (e.g. the signal
                // that woke us had a SIG_DFL "ignore" disposition), restore
                // the original blocked mask now so the suspend's temporary
                // mask does not leak past the syscall. The handler path
                // instead restores it via `uc_sigmask` on `rt_sigreturn`.
                if let Some(orig) = signal::take_saved_sigmask(pid) {
                    let _ = signal::set_blocked(pid, orig);
                }
                return false;
            }
        };

        match linux::linux_disposition(pid, sig) {
            LinuxDisposition::Handler(act) => {
                // Build a Linux rt_sigframe and enter the handler, passing the
                // recorded source metadata so the siginfo_t is sender-faithful.
                // On a stack-placement failure the signal is re-armed (with its
                // info) inside build_linux_rt_frame and we fall through to false.
                return linux::build_linux_rt_frame(
                    frame, ret_val, pid, sig, &act, info,
                );
            }
            LinuxDisposition::Ignore => {
                // Explicit SIG_IGN: drop and check the next signal.
            }
            LinuxDisposition::Default => match signal::default_action(sig) {
                signal::DefaultAction::Terminate => {
                    terminate_current_process_for_signal(pid, task_id, sig);
                    // Unreachable: task_exit never returns.
                }
                signal::DefaultAction::Stop => {
                    signal::discard_pending_cont(pid);
                    stop_process_for_signal(pid, sig, Some(task_id));
                }
                signal::DefaultAction::Ignore
                | signal::DefaultAction::Continue => {
                    // Dropped; check the next.
                }
            },
        }
    }
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
    // store argv/envp.  This native exec variant takes raw ELF bytes with
    // no filesystem path, so we pass None — exec_process clears any stale
    // /proc/<pid>/exe path from the replaced image.
    match exec_process(pid, &elf_copy, &argv_slices, &envp_slices, None) {
        Ok(result) => {
            // POSIX: exec resets caught signals to default and drops the
            // (now-stale) signal trampoline — the new image's libc init
            // re-registers it. Pending signals are preserved.
            crate::proc::signal::on_exec(pid);
            crate::syscall::linux::linux_sigaction_on_exec(pid);

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

/// Handle `SYS_PROCESS_FORK`.
///
/// Reads — but never modifies — the parent's saved syscall frame to
/// snapshot the register state the child must resume with, then forks
/// the calling process.  The parent observes the child PID as the
/// return value; the child's freshly spawned thread resumes at the same
/// instruction with `RAX = 0` (handled by the fork trampoline in
/// `crate::proc::fork`).
///
/// Returns the child PID (> 0) to the parent, or a negative
/// `KernelError` code on failure.
pub fn sys_process_fork_with_frame(frame: &mut super::entry::SyscallFrame) -> i64 {
    use crate::proc::{fork, thread};

    let task_id = sched::current_task_id();
    let parent_pid = match thread::owner_process(task_id) {
        Some(pid) if pid != 0 => pid,
        _ => {
            serial_println!("[fork] Task {} has no owning process", task_id);
            return KernelError::NoSuchProcess.code() as i64;
        }
    };

    match fork::fork_process(parent_pid, frame) {
        Ok(child_pid) => {
            // PIDs are small monotonic counters that never approach
            // i64::MAX, so the cast cannot wrap in practice.
            #[allow(clippy::cast_possible_wrap)]
            {
                child_pid as i64
            }
        }
        Err(e) => {
            serial_println!(
                "[fork] fork_process failed for pid {}: {:?}",
                parent_pid, e
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

/// `SYS_CLOCK_REALTIME` — get wall-clock time in nanoseconds since the
/// Unix epoch (1970-01-01 00:00:00 UTC).
///
/// Delegates to [`crate::timekeeping::clock_realtime`], which combines the
/// boot-time CMOS RTC reading with TSC-based elapsed time (plus any NTP or
/// manual adjustments).  Unlike `SYS_CLOCK_MONOTONIC` (boot-relative), this
/// is suitable for POSIX `CLOCK_REALTIME` / `gettimeofday` / `time`.
///
/// If timekeeping was never initialized (no usable RTC), `clock_realtime`
/// returns 0; we propagate that so callers see the epoch rather than a
/// bogus uptime-based value.
pub fn sys_clock_realtime(args: &SyscallArgs) -> SyscallResult {
    let _ = args;

    let ns = crate::timekeeping::clock_realtime();

    // ns is u64 nanoseconds-since-epoch. It will not exceed i64::MAX until
    // year 2262, so the cast cannot wrap in any realistic scenario.
    #[allow(clippy::cast_possible_wrap)]
    SyscallResult::ok(ns as i64)
}

/// `SYS_CLOCK_SETTIME` — set the wall-clock time.
///
/// `arg0`: target time in nanoseconds since the Unix epoch.
///
/// Delegates to [`crate::timekeeping::set_realtime`], which stores the
/// adjustment that makes [`crate::timekeeping::clock_realtime`] return the
/// requested value.  Backs POSIX `clock_settime(CLOCK_REALTIME)` /
/// `settimeofday`.
///
/// Requires `CAP_SYS_TIME` (root): setting the wall clock is a system-wide
/// side effect, so an unprivileged process is rejected with `PermissionDenied`
/// via [`require_clock_authority`] before any state is touched.
///
/// Rejects the call with `EINVAL` when timekeeping is uninitialized: with no
/// RTC base, `clock_realtime` returns 0 and `set_realtime` would compute its
/// offset against a meaningless base, so the clock would jump the moment the
/// base later becomes valid.  Better to fail loudly than to silently lock in a
/// wrong offset.
pub fn sys_clock_settime(args: &SyscallArgs) -> SyscallResult {
    if let Err(e) = require_clock_authority() {
        return SyscallResult::err(e);
    }
    if !crate::timekeeping::is_initialized() {
        return SyscallResult::err(KernelError::InvalidArgument);
    }

    let target_epoch_ns = args.arg0;
    crate::timekeeping::set_realtime(target_epoch_ns);
    // The realtime clock was discontinuously stepped: wake any timerfd reader
    // parked on a TFD_TIMER_CANCEL_ON_SET timer so it returns ECANCELED.
    crate::ipc::timerfd::clock_was_set();

    SyscallResult::ok(0)
}

/// `SYS_CLOCK_ADJTIME` — adjust the wall-clock time by a signed delta.
///
/// `arg0`: signed nanosecond offset (the `u64` is reinterpreted as `i64`).
///
/// Delegates to [`crate::timekeeping::adjust_realtime`], which atomically
/// adds the delta to the standing realtime adjustment.  Backs the
/// `ADJ_SETOFFSET` step path of POSIX `adjtimex`/`clock_adjtime`: a relative
/// shift rather than the absolute write that `SYS_CLOCK_SETTIME` performs,
/// so there is no read-modify-write race.
///
/// Requires `CAP_SYS_TIME` (root) via [`require_clock_authority`], same as
/// [`sys_clock_settime`]: an unprivileged caller is rejected with
/// `PermissionDenied` before any state is touched.
///
/// Rejects the call with `EINVAL` when timekeeping is uninitialized — with no
/// RTC base, `clock_realtime` returns 0 and the adjustment would apply against
/// a meaningless base once the RTC later becomes valid.  Mirrors the guard in
/// [`sys_clock_settime`].
pub fn sys_clock_adjtime(args: &SyscallArgs) -> SyscallResult {
    if let Err(e) = require_clock_authority() {
        return SyscallResult::err(e);
    }
    if !crate::timekeeping::is_initialized() {
        return SyscallResult::err(KernelError::InvalidArgument);
    }

    // The ABI passes args as u64; the realtime delta is signed.  Reinterpret
    // the bit pattern as i64 (two's-complement) — this is the inverse of the
    // `delta_ns as u64` the userspace caller performs.
    #[allow(clippy::cast_possible_wrap)]
    let delta_ns = args.arg0 as i64;
    crate::timekeeping::adjust_realtime(delta_ns);
    // A relative step (ADJ_SETOFFSET) is a clock discontinuity too: notify
    // timerfd so CANCEL_ON_SET readers are woken to return ECANCELED.
    crate::ipc::timerfd::clock_was_set();

    SyscallResult::ok(0)
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

/// Byte length of the kernel `FsStatResult` written by stat/fstat/lstat.
///
/// Layout (all multi-byte fields little-endian).  The low 16 bytes are
/// ABI-stable with the historical 16-byte result, so a reader that only
/// parses size/type/nlinks keeps working after the widening:
/// ```text
///   [0..8]   size         (u64)
///   [8]      entry_type   (u8: 0=file 1=dir 2=volume-label 3=symlink)
///   [9..12]  reserved     (zero)
///   [12..16] nlinks       (u32)
///   [16..20] permissions  (u32, Unix mode bits; 0 = unknown, synthesize)
///   [20..24] uid          (u32)
///   [24..28] gid          (u32)
///   [28..32] attributes   (u32, FileAttr bits)
///   [32..40] blocks       (u64, 512-byte sectors)
///   [40..48] modified_ns  (u64, wall-clock ns since the Unix epoch)
///   [48..56] accessed_ns  (u64)
///   [56..64] changed_ns   (u64)
///   [64..72] created_ns   (u64)
///   [72..80] ino          (u64, inode number; 0 = not available)
/// ```
pub const FS_STAT_RESULT_LEN: usize = 80;

/// Serialize file metadata into a user `FsStatResult` buffer.
///
/// Writes exactly [`FS_STAT_RESULT_LEN`] bytes in the layout documented on
/// that constant.  Uses unaligned stores because the user buffer is not
/// guaranteed to be aligned (e.g. a `[u8; N]` on a userspace stack).
///
/// # Safety
///
/// `out_ptr` must point to at least [`FS_STAT_RESULT_LEN`] writable, mapped
/// user bytes — the caller must have validated this via
/// [`crate::mm::user::validate_user_write`].
unsafe fn write_fs_stat_result(out_ptr: *mut u8, meta: &crate::fs::FileMeta) {
    let type_byte = match meta.entry_type {
        crate::fs::EntryType::File => 0u8,
        crate::fs::EntryType::Directory => 1u8,
        crate::fs::EntryType::VolumeLabel => 2u8,
        crate::fs::EntryType::Symlink => 3u8,
    };
    // SAFETY: out_ptr is valid for FS_STAT_RESULT_LEN bytes (caller
    // contract).  Every store is unaligned, so no alignment precondition is
    // imposed on the user buffer; offsets stay within the validated length.
    unsafe {
        core::ptr::write_bytes(out_ptr, 0, FS_STAT_RESULT_LEN);
        core::ptr::write_unaligned(out_ptr.cast::<u64>(), meta.size);
        out_ptr.add(8).write(type_byte);
        core::ptr::write_unaligned(out_ptr.add(12).cast::<u32>(), meta.nlinks);
        core::ptr::write_unaligned(
            out_ptr.add(16).cast::<u32>(),
            u32::from(meta.permissions),
        );
        core::ptr::write_unaligned(out_ptr.add(20).cast::<u32>(), meta.uid);
        core::ptr::write_unaligned(out_ptr.add(24).cast::<u32>(), meta.gid);
        core::ptr::write_unaligned(
            out_ptr.add(28).cast::<u32>(),
            meta.attributes.bits(),
        );
        core::ptr::write_unaligned(out_ptr.add(32).cast::<u64>(), meta.blocks);
        core::ptr::write_unaligned(out_ptr.add(40).cast::<u64>(), meta.modified_ns);
        core::ptr::write_unaligned(out_ptr.add(48).cast::<u64>(), meta.accessed_ns);
        core::ptr::write_unaligned(out_ptr.add(56).cast::<u64>(), meta.changed_ns);
        core::ptr::write_unaligned(out_ptr.add(64).cast::<u64>(), meta.created_ns);
        core::ptr::write_unaligned(out_ptr.add(72).cast::<u64>(), meta.ino);
    }
}

/// `SYS_FS_STAT` — stat a file or directory.
///
/// Returns metadata in a [`FS_STAT_RESULT_LEN`]-byte `FsStatResult` buffer
/// (layout documented on that constant).  Follows symlinks.
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
    if let Err(e) =
        crate::mm::user::validate_user_write(args.arg2, FS_STAT_RESULT_LEN)
    {
        return SyscallResult::err(e);
    }

    // SAFETY: Validated above — path_ptr is in user space and mapped.
    let path_bytes = unsafe { core::slice::from_raw_parts(path_ptr, safe_path_len) };
    let path = match core::str::from_utf8(path_bytes) {
        Ok(s) => s,
        Err(_) => return SyscallResult::err(KernelError::InvalidArgument),
    };

    // Use metadata() (follows symlinks) so the caller gets the rich fields
    // — timestamps, ownership, mode, blocks — not just size and type.
    let meta = match crate::fs::Vfs::metadata(path) {
        Ok(m) => m,
        Err(e) => return SyscallResult::err(e),
    };

    // SAFETY: out_ptr was validated for FS_STAT_RESULT_LEN bytes above.
    unsafe { write_fs_stat_result(out_ptr, &meta) };

    SyscallResult::ok(0)
}

// ---------------------------------------------------------------------------
// Handle-based filesystem handlers (610–699)
// ---------------------------------------------------------------------------

/// `SYS_FS_OPEN` — open a file, return a handle.
/// Open an already-resolved **absolute kernel path** string with the given
/// native [`OpenFlags`](crate::fs::handle::OpenFlags) bits, returning the raw
/// open-file handle as the syscall value.
///
/// This is the shared core of file opening that works from a kernel-owned
/// path string rather than a userspace pointer.  The Linux ABI's
/// `open`/`openat(AT_FDCWD)` path uses it after canonicalising a (possibly
/// relative) userspace path against the caller's per-process cwd — something
/// the VFS resolver cannot do on its own because it has no notion of which
/// process is calling.  `sys_fs_open` keeps its own userspace-pointer path
/// for the native ABI; this helper does not read userspace memory.
///
/// Performs the same File-READ capability check and per-process handle
/// registration `sys_fs_open` does, so the returned handle is closed on
/// process exit and refcount-shared across `fork`.
pub fn fs_open_kernel_path(path: &str, flags_raw: u32) -> SyscallResult {
    if let Err(e) = require_cap_type(
        crate::cap::ResourceType::File,
        crate::cap::Rights::READ,
    ) {
        return SyscallResult::err(e);
    }
    let flags = crate::fs::handle::OpenFlags::from_bits(flags_raw);
    match crate::fs::handle::open(path, flags) {
        Ok(handle) => {
            if let Some(pid) = caller_pid() {
                pcb::register_ipc_handle(pid, ResourceType::File, handle);
            }
            #[allow(clippy::cast_possible_wrap)]
            SyscallResult::ok(handle as i64)
        }
        Err(e) => SyscallResult::err(e),
    }
}

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
            // Track the open file handle as a per-process resource so it
            // is closed when the process exits (cleanup_handles), and so
            // fork() can enumerate and refcount-share it with the child.
            // File handles are refcounted in the open-file table, so the
            // matching deregister-on-close + cleanup-on-exit drop exactly
            // one reference each.
            if let Some(pid) = caller_pid() {
                pcb::register_ipc_handle(pid, ResourceType::File, handle);
            }
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
        Ok(()) => {
            // Drop the per-process ownership record so the handle is not
            // double-closed by cleanup_handles on process exit.  The
            // deregister is keyed on (File, handle); if the handle was
            // never registered (e.g. a kernel task) this is a no-op.
            if let Some(pid) = caller_pid() {
                pcb::deregister_ipc_handle(pid, ResourceType::File, handle);
            }
            SyscallResult::ok(0)
        }
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_FS_READ` — read from a file handle at the current offset.
pub fn sys_fs_read(args: &SyscallArgs) -> SyscallResult {
    let handle = args.arg0;
    let buf_ptr = args.arg1 as *mut u8;
    let buf_cap = args.arg2 as usize;

    // POSIX/Linux: a zero-length read returns 0 with no other effect.  It does
    // not touch the buffer (so the pointer is *not* validated — `read(fd, NULL,
    // 0)` is legal and returns 0) and does not advance the file offset.  We must
    // return 0 here rather than EINVAL: tcc's `full_read` reads exactly `count`
    // bytes and then issues a *terminal* `read(fd, buf, 0)`, expecting 0 to
    // signal completion.  Returning EINVAL made that terminal read look like a
    // failure, so `tcc_object_type` saw a short ehdr read and rejected every
    // relocatable object as "unrecognized file type" (Path-Z hosted compile).
    if buf_cap == 0 {
        return SyscallResult::ok(0);
    }
    if buf_ptr.is_null() {
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

/// Read and validate a user-space string argument (ptr + len pair).
///
/// Clamps the length to `max_len` bytes, validates the user read, and
/// decodes as UTF-8.  Returns an empty string when `len == 0` (used for
/// the optional source/device argument of pseudo-filesystems).
///
/// # Errors
///
/// - `InvalidArgument` — null pointer with non-zero length, or invalid UTF-8.
/// - Any error from [`crate::mm::user::validate_user_read`].
fn read_user_str(
    ptr_arg: u64,
    len: usize,
    max_len: usize,
    buf: &mut [u8],
) -> Result<&str, KernelError> {
    if len == 0 {
        return Ok("");
    }
    let ptr = ptr_arg as *const u8;
    if ptr.is_null() {
        return Err(KernelError::InvalidArgument);
    }
    let safe_len = len.min(max_len);
    crate::mm::user::validate_user_read(ptr_arg, safe_len)?;
    // SAFETY: validated above for `safe_len` bytes in user space.
    let src = unsafe { core::slice::from_raw_parts(ptr, safe_len) };
    let dst = match buf.get_mut(..safe_len) {
        Some(d) => d,
        None => return Err(KernelError::InvalidArgument),
    };
    dst.copy_from_slice(src);
    core::str::from_utf8(dst).map_err(|_| KernelError::InvalidArgument)
}

/// `SYS_FS_MOUNT` — mount a filesystem at a target path.
///
/// Root-only.  Dispatches on the filesystem-type string to the matching
/// in-kernel backend.  All six argument slots are consumed by three
/// string pairs (source, target, fstype); mount flags are deferred to a
/// future versioned extension.
pub fn sys_fs_mount(args: &SyscallArgs) -> SyscallResult {
    if let Err(e) = require_mount_authority() {
        return SyscallResult::err(e);
    }

    // Target and fstype are mandatory; source may be empty (pseudo-fs).
    if args.arg3 == 0 || args.arg5 == 0 {
        return SyscallResult::err(KernelError::InvalidArgument);
    }

    let mut src_buf = [0u8; 256];
    let mut tgt_buf = [0u8; 256];
    let mut fst_buf = [0u8; 64];

    let source = match read_user_str(args.arg0, args.arg1 as usize, 256, &mut src_buf)
    {
        Ok(s) => s,
        Err(e) => return SyscallResult::err(e),
    };
    let target = match read_user_str(args.arg2, args.arg3 as usize, 256, &mut tgt_buf)
    {
        Ok(s) => s,
        Err(e) => return SyscallResult::err(e),
    };
    let fstype = match read_user_str(args.arg4, args.arg5 as usize, 64, &mut fst_buf)
    {
        Ok(s) => s,
        Err(e) => return SyscallResult::err(e),
    };

    let result = match fstype {
        "ext4" => crate::fs::ext4::mount(source, target),
        "tmpfs" | "memfs" | "ramfs" => crate::fs::memfs::mount(target),
        "iso9660" | "iso" | "cd9660" => crate::fs::iso9660::mount(source, target),
        "devfs" | "dev" => crate::fs::devfs::mount(target),
        "proc" | "procfs" => crate::fs::procfs::mount(target),
        "sysfs" | "sys" => crate::fs::sysfs::mount(target),
        "vfat" | "fat" | "fat32" | "fat16" | "msdos" => {
            match crate::fs::fat::FatFs::mount(source) {
                Ok(fs) => crate::fs::Vfs::mount(target, alloc::boxed::Box::new(fs)),
                Err(e) => Err(e),
            }
        }
        // Unknown filesystem type.
        _ => Err(KernelError::NotSupported),
    };

    match result {
        Ok(()) => SyscallResult::ok(0),
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_FS_UMOUNT` — unmount the filesystem at a target path.
///
/// Root-only.  Refuses to unmount the root filesystem and refuses if
/// sub-mounts exist beneath the target (`DeviceBusy`).
pub fn sys_fs_umount(args: &SyscallArgs) -> SyscallResult {
    if let Err(e) = require_mount_authority() {
        return SyscallResult::err(e);
    }

    if args.arg1 == 0 {
        return SyscallResult::err(KernelError::InvalidArgument);
    }

    let mut tgt_buf = [0u8; 256];
    let target = match read_user_str(args.arg0, args.arg1 as usize, 256, &mut tgt_buf)
    {
        Ok(s) => s,
        Err(e) => return SyscallResult::err(e),
    };

    match crate::fs::Vfs::unmount(target) {
        Ok(()) => SyscallResult::ok(0),
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_FS_FORMAT` — create a fresh filesystem on a block device.
///
/// Root-only and **destructive**.  Currently only the FAT family is
/// supported (via the in-kernel `mkfs_fat` formatter); other fstypes return
/// `NotSupported`.
pub fn sys_fs_format(args: &SyscallArgs) -> SyscallResult {
    if let Err(e) = require_format_authority() {
        return SyscallResult::err(e);
    }

    // Device name and fstype are mandatory; label is optional.
    if args.arg1 == 0 || args.arg3 == 0 {
        return SyscallResult::err(KernelError::InvalidArgument);
    }

    let mut dev_buf = [0u8; 64];
    let mut fst_buf = [0u8; 64];
    let mut lbl_buf = [0u8; 64];

    let device = match read_user_str(args.arg0, args.arg1 as usize, 64, &mut dev_buf) {
        Ok(s) => s,
        Err(e) => return SyscallResult::err(e),
    };
    let fstype = match read_user_str(args.arg2, args.arg3 as usize, 64, &mut fst_buf) {
        Ok(s) => s,
        Err(e) => return SyscallResult::err(e),
    };
    let label = match read_user_str(args.arg4, args.arg5 as usize, 64, &mut lbl_buf) {
        Ok(s) => s,
        Err(e) => return SyscallResult::err(e),
    };
    let label_opt = if label.is_empty() { None } else { Some(label) };

    let result = match fstype {
        "vfat" | "fat" | "fat32" | "fat16" | "msdos" => {
            crate::fs::fat::mkfs_fat(device, label_opt)
        }
        // ext4 and other types have no in-kernel formatter yet.
        _ => Err(KernelError::NotSupported),
    };

    match result {
        Ok(()) => SyscallResult::ok(0),
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_FS_CHECK` — check (and optionally repair) a filesystem on a block
/// device (fsck).
///
/// ABI: `arg0`/`arg1` = device-name ptr+len (the block-device registry name,
/// e.g. "vda" — **not** a `/dev/` path); `arg2` = flags, where bit 0 requests
/// repair mode (write corrected metadata) and all other bits are reserved.
///
/// Root-only.  Currently only the FAT family is supported (via the in-kernel
/// `fsck_fat` checker); other on-disk formats fail when the FAT superblock
/// cannot be parsed.
///
/// Returns (as a non-negative success value) the number of *outstanding*
/// errors: in check-only mode the count of problems detected; in repair mode
/// the count that remain *after* repair (0 = fully clean/repaired).  A negative
/// return is a `KernelError` (e.g. the device does not exist, or the volume is
/// not a recognised FAT filesystem).
pub fn sys_fs_check(args: &SyscallArgs) -> SyscallResult {
    if let Err(e) = require_fsck_authority() {
        return SyscallResult::err(e);
    }

    // Device name is mandatory.
    if args.arg1 == 0 {
        return SyscallResult::err(KernelError::InvalidArgument);
    }

    let mut dev_buf = [0u8; 64];
    let device = match read_user_str(args.arg0, args.arg1 as usize, 64, &mut dev_buf) {
        Ok(s) => s,
        Err(e) => return SyscallResult::err(e),
    };

    let repair = (args.arg2 & 1) != 0;

    match crate::fs::fat::fsck_fat(device, repair) {
        Ok(report) => {
            // In repair mode, report.errors is the total found and
            // report.repaired is how many were fixed; the outstanding count is
            // the difference.  In check-only mode nothing is repaired, so the
            // outstanding count is simply the number found.
            let outstanding = if repair {
                report.errors.saturating_sub(report.repaired)
            } else {
                report.errors
            };
            SyscallResult::ok(i64::from(outstanding))
        }
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_FS_TRIM` — discard the free space of a mounted filesystem (fstrim).
///
/// ABI: `arg0`/`arg1` = device-name ptr+len (the block-device registry name,
/// e.g. "vda" — **not** a `/dev/` path).
///
/// Root-only.  Locates the mounted filesystem backed by the named device and
/// trims its free space.  Returns (as a non-negative success value) the number
/// of bytes discarded; 0 if the filesystem cannot trim (e.g. the backing
/// device does not support discard).  A negative return is a `KernelError`
/// (e.g. the device is not mounted, or the name is invalid).
pub fn sys_fs_trim(args: &SyscallArgs) -> SyscallResult {
    if let Err(e) = require_trim_authority() {
        return SyscallResult::err(e);
    }

    // Device name is mandatory.
    if args.arg1 == 0 {
        return SyscallResult::err(KernelError::InvalidArgument);
    }

    let mut dev_buf = [0u8; 64];
    let device = match read_user_str(args.arg0, args.arg1 as usize, 64, &mut dev_buf) {
        Ok(s) => s,
        Err(e) => return SyscallResult::err(e),
    };

    match crate::fs::Vfs::trim_device(device) {
        // Clamp to i64::MAX rather than overflow the signed return; a discard
        // larger than 8 EiB is not physically reachable, so this never loses
        // real information.
        Ok(bytes) => SyscallResult::ok(i64::try_from(bytes).unwrap_or(i64::MAX)),
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
    if let Err(e) =
        crate::mm::user::validate_user_write(args.arg1, FS_STAT_RESULT_LEN)
    {
        return SyscallResult::err(e);
    }

    match crate::fs::handle::fstat(handle) {
        Ok(meta) => {
            // SAFETY: out_ptr was validated for FS_STAT_RESULT_LEN bytes above.
            unsafe { write_fs_stat_result(out_ptr, &meta) };
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

                    // Write is_dir flag (byte 524): 1 if the subject is a
                    // directory.  The buffer was zero-filled above, so a
                    // non-directory event leaves this 0 without an extra write.
                    if event.is_dir {
                        core::ptr::write(base.add(524), 1u8);
                    }
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
///
/// A uid or gid of `u32::MAX` (`(uid_t)-1` / `(gid_t)-1`) means "leave that
/// field unchanged", per POSIX `chown`.  `Vfs::set_owner` resolves the
/// sentinel against the file's current owner.
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
    // capacity == 0 is a valid "size query" (POSIX getxattr): the caller
    // wants the attribute length without copying, so don't require a buffer.
    // Only validate the output buffer when one is actually provided.
    if capacity > 0 {
        if out_ptr.is_null() {
            return SyscallResult::err(KernelError::InvalidArgument);
        }
        if let Err(e) = crate::mm::user::validate_user_write(args.arg3, capacity) {
            return SyscallResult::err(e);
        }
    }

    match crate::fs::Vfs::get_xattr(path, key) {
        Ok(val) => {
            // Copy as much as fits, but always report the TRUE length so the
            // caller can perform a size query or detect truncation (ERANGE).
            if capacity > 0 {
                let copy_len = val.len().min(capacity);
                // SAFETY: out_ptr validated for capacity bytes above, and
                // copy_len <= capacity.
                unsafe {
                    core::ptr::copy_nonoverlapping(val.as_ptr(), out_ptr, copy_len);
                }
            }
            SyscallResult::ok(val.len() as i64)
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
    // capacity == 0 is a valid "size query" (POSIX listxattr): return the
    // total bytes needed without writing.  Validate the buffer only when one
    // is provided.
    if capacity > 0 {
        if out_ptr.is_null() {
            return SyscallResult::err(KernelError::InvalidArgument);
        }
        if let Err(e) = crate::mm::user::validate_user_write(args.arg2, capacity) {
            return SyscallResult::err(e);
        }
    }

    let keys = match crate::fs::Vfs::list_xattrs(path) {
        Ok(k) => k,
        Err(e) => return SyscallResult::err(e),
    };

    // Total size of the packed null-terminated key list.
    let mut total = 0usize;
    for key in &keys {
        total = total.wrapping_add(key.len().wrapping_add(1));
    }

    // Only fill the buffer when the whole list fits; otherwise report the
    // required size and let the caller retry with a bigger buffer (the posix
    // layer maps total > capacity to ERANGE).  This avoids handing back a
    // partially-packed list that the caller cannot distinguish from a
    // complete one.
    if capacity > 0 && total <= capacity {
        let mut offset = 0usize;
        for key in &keys {
            // SAFETY: total <= capacity and out_ptr is validated for
            // capacity bytes, so every write stays in bounds.
            unsafe {
                core::ptr::copy_nonoverlapping(
                    key.as_ptr(),
                    out_ptr.add(offset),
                    key.len(),
                );
                *out_ptr.add(offset.wrapping_add(key.len())) = 0;
            }
            offset = offset.wrapping_add(key.len().wrapping_add(1));
        }
    }

    SyscallResult::ok(total as i64)
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
/// `arg2`: pointer to output buffer ([`FS_STAT_RESULT_LEN`]-byte `FsStatResult`).
///
/// Same output format as `SYS_FS_STAT` (see [`FS_STAT_RESULT_LEN`]).  For a
/// symlink, the size is the length of the target path string and the entry
/// type is `3` (symlink), matching Linux `lstat` behavior.
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
    if let Err(e) =
        crate::mm::user::validate_user_write(args.arg2, FS_STAT_RESULT_LEN)
    {
        return SyscallResult::err(e);
    }

    // lmetadata() does not follow the final symlink (rich no-follow stat).
    let meta = match crate::fs::Vfs::lmetadata(path) {
        Ok(m) => m,
        Err(e) => return SyscallResult::err(e),
    };

    // SAFETY: out_ptr was validated for FS_STAT_RESULT_LEN bytes above.
    unsafe { write_fs_stat_result(out_ptr, &meta) };

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

    // SAFETY: path_bytes validated by validate_user_read above.
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
    // SAFETY: out_ptr/out_cap validated by validate_user_write above.
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

    // SAFETY: path_bytes validated by validate_user_read above.
    let path_bytes = unsafe { core::slice::from_raw_parts(args.arg0 as *const u8, path_len) };
    let dir_path = match core::str::from_utf8(path_bytes) {
        Ok(s) => s,
        Err(_) => return SyscallResult::err(KernelError::InvalidArgument),
    };

    // Generate a unique temporary filename using the TSC for entropy.
    // SAFETY: _rdtsc is always available on x86_64; no side-effects.
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

    // SAFETY: path_bytes validated by validate_user_read above.
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
/// `arg1`: detached flag (non-zero → the thread is detached, so its exit
///         value is not retained for a join that will never come).
///
/// Does not return.
pub fn sys_thread_exit(args: &SyscallArgs) -> SyscallResult {
    use crate::proc::thread;

    #[allow(clippy::cast_possible_wrap)]
    let exit_value = args.arg0 as i64;
    let detached = args.arg1 != 0;

    // This function never returns — it terminates the calling thread.
    thread::thread_exit_with_value(exit_value, detached);

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

/// `SYS_SET_FS_BASE` — set the calling thread's `fs_base` (x86-64 thread
/// pointer / TLS base).
///
/// `arg0`: new base address (must be < 2^47, i.e. a canonical user
///         address).
///
/// Native counterpart of the Linux `arch_prctl(ARCH_SET_FS, addr)` shim
/// (see `linux::sys_arch_prctl`).  A native static binary gets no thread
/// pointer at exec (the kernel zeroes `fs_base`, expecting userspace to
/// set up TLS), and has no aux vector to discover its `PT_TLS` the way a
/// Linux crt would — so its crt allocates the TLS block + TCB itself and
/// installs the thread pointer through this syscall.  Writes
/// `IA32_FS_BASE` and persists the value on the current Task so the
/// scheduler restores it on every switch-in (the FS base is a global CPU
/// register not saved in the GP context).
///
/// Returns: 0 on success, or `InvalidArgument` if the address is out of
/// range.
pub fn sys_set_fs_base(args: &SyscallArgs) -> SyscallResult {
    // 2^47 — the first non-canonical user address on x86-64 (matches
    // `linux::USER_FS_BASE_MAX`).  A base at or above this would make
    // `WRMSR` raise #GP, so reject it as an invalid argument.
    const USER_FS_BASE_MAX: u64 = 1u64 << 47;

    let addr = args.arg0;
    if addr >= USER_FS_BASE_MAX {
        return SyscallResult::err(KernelError::InvalidArgument);
    }

    // SAFETY: IA32_FS_BASE is a documented architectural MSR; `addr` has
    // been validated to be strictly below 1 << 47, so it is a canonical
    // user address and `WRMSR` will not raise #GP.  This is exactly what
    // the Linux arch_prctl(ARCH_SET_FS) path does.
    unsafe {
        crate::cpu::wrmsr(crate::cpu::IA32_FS_BASE, addr);
    }
    // Persist the FS base on the current Task so the scheduler restores it
    // on every switch-in.  IA32_FS_BASE is a global CPU register not saved
    // in the GP context; without this, a context switch away and back
    // would leave this thread's TLS pointer clobbered.
    sched::set_current_task_fs_base(addr);
    SyscallResult::ok(0)
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
// Raw layer-2 NIC access (865-868) — userspace network stack (§63, Path B)
// ---------------------------------------------------------------------------

/// Minimum raw Ethernet frame length (dst+src MAC + EtherType).
const RAW_FRAME_MIN: usize = 14;
/// Maximum raw Ethernet frame length we accept on TX / require room for on RX.
/// 1514 = 14-byte header + 1500 MTU; +8 slack for one/two 802.1Q VLAN tags.
const RAW_FRAME_MAX: usize = 1522;

/// `SYS_NET_RAW_OPEN` — claim exclusive raw L2 access to the physical NIC.
///
/// Requires a `NetRaw` capability with `WRITE` rights.  `arg0` (interface
/// index) is reserved and must be 0.
pub fn sys_net_raw_open(args: &SyscallArgs) -> SyscallResult {
    // Capability gate: raw L2 access is strictly more privileged than a socket.
    if let Err(e) = require_cap_type(
        crate::cap::ResourceType::NetRaw,
        crate::cap::Rights::WRITE,
    ) {
        return SyscallResult::err(e);
    }
    if args.arg0 != 0 {
        // Only the primary NIC (index 0) is supported for now.
        return SyscallResult::err(KernelError::InvalidArgument);
    }
    let pid = match caller_pid() {
        Some(p) => p,
        None => return SyscallResult::err(KernelError::NoSuchProcess),
    };
    match crate::net::raw::claim(pid) {
        Ok(()) => SyscallResult::ok(0),
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_NET_RAW_TX` — transmit one raw Ethernet frame.
///
/// `arg0`: frame pointer, `arg1`: length.  Caller must own the raw claim.
pub fn sys_net_raw_tx(args: &SyscallArgs) -> SyscallResult {
    let pid = match caller_pid() {
        Some(p) => p,
        None => return SyscallResult::err(KernelError::NoSuchProcess),
    };
    // Defense in depth: only the current raw owner may transmit, even if the
    // process still holds the NetRaw capability.
    if crate::net::raw::owner() != Some(pid) {
        return SyscallResult::err(KernelError::PermissionDenied);
    }
    let len = args.arg1 as usize;
    if !(RAW_FRAME_MIN..=RAW_FRAME_MAX).contains(&len) {
        return SyscallResult::err(KernelError::InvalidArgument);
    }
    if let Err(e) = crate::mm::user::validate_user_read(args.arg0, len) {
        return SyscallResult::err(e);
    }
    // SAFETY: validated readable for `len` bytes above.
    let frame = unsafe { core::slice::from_raw_parts(args.arg0 as *const u8, len) };
    match crate::net::raw::transmit(frame) {
        Ok(()) => SyscallResult::ok(0),
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_NET_RAW_RX` — receive one raw Ethernet frame (non-blocking).
///
/// `arg0`: output buffer pointer, `arg1`: buffer capacity.  Caller must own the
/// raw claim.  Returns the frame length on success, `WouldBlock` if idle.
pub fn sys_net_raw_rx(args: &SyscallArgs) -> SyscallResult {
    let pid = match caller_pid() {
        Some(p) => p,
        None => return SyscallResult::err(KernelError::NoSuchProcess),
    };
    if crate::net::raw::owner() != Some(pid) {
        return SyscallResult::err(KernelError::PermissionDenied);
    }
    let cap = args.arg1 as usize;
    // Require room for a full standard frame so a pending frame is never
    // truncated or lost after being dequeued from the NIC.
    if cap < RAW_FRAME_MAX {
        return SyscallResult::err(KernelError::InvalidArgument);
    }
    if let Err(e) = crate::mm::user::validate_user_write(args.arg0, cap) {
        return SyscallResult::err(e);
    }
    match crate::net::raw::receive() {
        Some(frame) => {
            let n = frame.len();
            if n > cap {
                // Jumbo frame that doesn't fit — drop it rather than corrupt
                // the caller's buffer.  (Should not happen with a 1500 MTU.)
                return SyscallResult::err(KernelError::InvalidArgument);
            }
            // SAFETY: `args.arg0` validated writable for `cap` >= `n` bytes.
            unsafe {
                core::ptr::copy_nonoverlapping(frame.as_ptr(), args.arg0 as *mut u8, n);
            }
            SyscallResult::ok(n as i64)
        }
        None => SyscallResult::err(KernelError::WouldBlock),
    }
}

/// `SYS_NET_RAW_CLOSE` — release the caller's raw NIC claim.  Idempotent.
pub fn sys_net_raw_close(_args: &SyscallArgs) -> SyscallResult {
    if let Some(pid) = caller_pid() {
        // release() is a no-op for non-owners, so this is always safe.
        let _ = crate::net::raw::release(pid);
    }
    SyscallResult::ok(0)
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

/// `SYS_LOADAVG` — read one of the three EWMA load averages.
///
/// `arg0`: 0 = 1-min, 1 = 5-min, 2 = 15-min.
///
/// Reads `crate::loadavg::get()`, returns the requested fixed-point
/// value (FSHIFT=11, i.e., load × 2048).  Reports
/// `InvalidArgument` for any other index.
///
/// Returns: load value in fixed-point on success.
pub fn sys_loadavg(args: &SyscallArgs) -> SyscallResult {
    let (l1, l5, l15) = crate::loadavg::get();
    let val = match args.arg0 {
        0 => l1,
        1 => l5,
        2 => l15,
        _ => return SyscallResult::err(KernelError::InvalidArgument),
    };
    #[allow(clippy::cast_possible_wrap)]
    SyscallResult::ok(val as i64)
}

/// `SYS_CPU_TIMES` — read aggregate per-CPU time accounting fields.
///
/// `arg0`: field selector:
///   - 0 = system_ns (kernel + user code time)
///   - 1 = irq_ns
///   - 2 = softirq_ns
///   - 3 = idle_ns
///   - 4 = total_ns (wall time × online CPU count)
///
/// Returns: the selected field in nanoseconds, or `InvalidArgument`
/// if `arg0` is not in 0..=4.
///
/// Used by `posix::times()` and `posix::getrusage()` to populate
/// process CPU time fields with real kernel data.  Since we don't
/// currently track per-task CPU time (the scheduler runs a single
/// user process per CPU), the aggregate stats are a reasonable
/// approximation for the calling process.
pub fn sys_cpu_times(args: &SyscallArgs) -> SyscallResult {
    let agg = crate::cputime::aggregate_stats();
    let val = match args.arg0 {
        0 => agg.system_ns,
        1 => agg.irq_ns,
        2 => agg.softirq_ns,
        3 => agg.idle_ns,
        4 => agg.total_ns,
        _ => return SyscallResult::err(KernelError::InvalidArgument),
    };
    #[allow(clippy::cast_possible_wrap)]
    SyscallResult::ok(val as i64)
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

        // SAFETY: buf_ptr is a userspace pointer validated by the caller;
        // written < max_records ensures dst stays within the buffer.
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

            // SAFETY: buf_ptr is validated; written < max_records.
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

/// Field-mask bits for [`sys_net_if_config`] (record byte 17).
mod net_if_config_mask {
    /// Apply the IPv4 address field (bytes 0..4).
    pub const IP: u8 = 1 << 0;
    /// Apply the subnet-mask field (bytes 4..8).
    pub const MASK: u8 = 1 << 1;
    /// Apply the gateway field (bytes 8..12).
    pub const GATEWAY: u8 = 1 << 2;
    /// Apply the DNS-server field (bytes 12..16).
    pub const DNS: u8 = 1 << 3;
    /// Apply the up/down flag (byte 16).
    pub const UP: u8 = 1 << 4;
    /// All recognised bits — any bit outside this set is rejected.
    pub const ALL: u8 = IP | MASK | GATEWAY | DNS | UP;
}

/// `SYS_NET_IF_CONFIG` — configure the primary network interface.
///
/// Write side of [`sys_net_if_info`]: applies IPv4 address/mask/gateway/DNS
/// and/or the up/down flag to the physical NIC (root network namespace). This
/// is the native syscall behind `ifconfig`/`ip addr`/`ip link`/`route`.
///
/// Root-gated (`CAP_NET_ADMIN`-class). Reads an 18-byte record from `arg0`
/// (length in `arg1`); a per-field mask (byte 17) selects which fields to
/// apply, so callers can change only what they mean to (read-modify-write
/// against the current config). See the [`SYS_NET_IF_CONFIG`] doc for the
/// exact layout.
///
/// [`SYS_NET_IF_CONFIG`]: crate::syscall::number::SYS_NET_IF_CONFIG
pub fn sys_net_if_config(args: &SyscallArgs) -> SyscallResult {
    if let Err(e) = require_netadmin_authority() {
        return SyscallResult::err(e);
    }

    let in_ptr = args.arg0;
    let buf_len = args.arg1 as usize;

    const REC_SIZE: usize = 18;
    if in_ptr == 0 || buf_len < REC_SIZE {
        return SyscallResult::err(KernelError::InvalidArgument);
    }

    // Copy the fixed-size record out of user memory (validated first).
    if let Err(e) = crate::mm::user::validate_user_read(in_ptr, REC_SIZE) {
        return SyscallResult::err(e);
    }
    let mut record = [0u8; REC_SIZE];
    // SAFETY: validated above for exactly REC_SIZE bytes of readable user memory.
    unsafe {
        core::ptr::copy_nonoverlapping(in_ptr as *const u8, record.as_mut_ptr(), REC_SIZE);
    }

    // Destructure the fixed-size record by value: no indexing/arithmetic on the
    // hot path, and every field is named per the ABI layout.
    let [ip0, ip1, ip2, ip3, m0, m1, m2, m3, g0, g1, g2, g3, d0, d1, d2, d3, up_byte, field_mask] =
        record;

    // Reject unknown mask bits so a future ABI extension can't be silently
    // ignored by an old kernel.
    if field_mask & !net_if_config_mask::ALL != 0 {
        return SyscallResult::err(KernelError::InvalidArgument);
    }
    if field_mask == 0 {
        // Nothing to do — a no-op success (matches "ip addr" with no change).
        return SyscallResult::ok(0);
    }

    use crate::net::interface::Ipv4Addr;

    // Read-modify-write: start from the current config and override only the
    // masked address fields, then apply in one `configure` call (which also
    // syncs netns and sends a gratuitous ARP).
    let addr_bits =
        net_if_config_mask::IP | net_if_config_mask::MASK | net_if_config_mask::GATEWAY | net_if_config_mask::DNS;
    if field_mask & addr_bits != 0 {
        let cur = crate::net::interface::info();
        let ip = if field_mask & net_if_config_mask::IP != 0 {
            Ipv4Addr([ip0, ip1, ip2, ip3])
        } else {
            cur.ip
        };
        let mask = if field_mask & net_if_config_mask::MASK != 0 {
            Ipv4Addr([m0, m1, m2, m3])
        } else {
            cur.subnet_mask
        };
        let gateway = if field_mask & net_if_config_mask::GATEWAY != 0 {
            Ipv4Addr([g0, g1, g2, g3])
        } else {
            cur.gateway
        };
        let dns = if field_mask & net_if_config_mask::DNS != 0 {
            Ipv4Addr([d0, d1, d2, d3])
        } else {
            cur.dns
        };

        // Reject an interface address that can never be a host address.
        if field_mask & net_if_config_mask::IP != 0 && (ip.is_broadcast() || ip.is_multicast()) {
            return SyscallResult::err(KernelError::InvalidArgument);
        }

        crate::net::interface::configure(ip, mask, gateway, dns);
    }

    if field_mask & net_if_config_mask::UP != 0 {
        crate::net::interface::set_up(up_byte != 0);
    }

    SyscallResult::ok(0)
}

/// `SYS_NET_ROUTE_ADD` — add an IPv4 route to the caller's netns routing table.
///
/// See [`SYS_NET_ROUTE_ADD`] for the 16-byte record layout. Root-gated. Rejects
/// the default route (`0.0.0.0/0`) — that is owned by the interface gateway
/// ([`sys_net_if_config`], design-decisions §52).
///
/// [`SYS_NET_ROUTE_ADD`]: crate::syscall::number::SYS_NET_ROUTE_ADD
pub fn sys_net_route_add(args: &SyscallArgs) -> SyscallResult {
    if let Err(e) = require_netadmin_authority() {
        return SyscallResult::err(e);
    }

    let in_ptr = args.arg0;
    let buf_len = args.arg1 as usize;

    const REC_SIZE: usize = 16;
    if in_ptr == 0 || buf_len < REC_SIZE {
        return SyscallResult::err(KernelError::InvalidArgument);
    }
    if let Err(e) = crate::mm::user::validate_user_read(in_ptr, REC_SIZE) {
        return SyscallResult::err(e);
    }
    let mut record = [0u8; REC_SIZE];
    // SAFETY: validated above for exactly REC_SIZE bytes of readable user memory.
    unsafe {
        core::ptr::copy_nonoverlapping(in_ptr as *const u8, record.as_mut_ptr(), REC_SIZE);
    }

    // Destructure by value: named fields, no indexing/arithmetic on the path.
    let [d0, d1, d2, d3, m0, m1, m2, m3, g0, g1, g2, g3, mt0, mt1, mt2, mt3] = record;
    let destination = crate::netns::Ipv4Addr([d0, d1, d2, d3]);
    let mask = crate::netns::Ipv4Addr([m0, m1, m2, m3]);
    let gateway = crate::netns::Ipv4Addr([g0, g1, g2, g3]);
    let metric = u32::from_le_bytes([mt0, mt1, mt2, mt3]);

    // The default route is owned by the interface gateway, not the table.
    if destination == crate::netns::Ipv4Addr([0, 0, 0, 0])
        && mask == crate::netns::Ipv4Addr([0, 0, 0, 0])
    {
        return SyscallResult::err(KernelError::InvalidArgument);
    }

    let ns = crate::sched::current_task_net_ns();
    match crate::netns::add_route(ns, destination, mask, gateway, metric) {
        Ok(()) => SyscallResult::ok(0),
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_NET_ROUTE_DEL` — remove an IPv4 route from the caller's netns table.
///
/// See [`SYS_NET_ROUTE_DEL`] for the 8-byte record layout. Root-gated.
///
/// [`SYS_NET_ROUTE_DEL`]: crate::syscall::number::SYS_NET_ROUTE_DEL
pub fn sys_net_route_del(args: &SyscallArgs) -> SyscallResult {
    if let Err(e) = require_netadmin_authority() {
        return SyscallResult::err(e);
    }

    let in_ptr = args.arg0;
    let buf_len = args.arg1 as usize;

    const REC_SIZE: usize = 8;
    if in_ptr == 0 || buf_len < REC_SIZE {
        return SyscallResult::err(KernelError::InvalidArgument);
    }
    if let Err(e) = crate::mm::user::validate_user_read(in_ptr, REC_SIZE) {
        return SyscallResult::err(e);
    }
    let mut record = [0u8; REC_SIZE];
    // SAFETY: validated above for exactly REC_SIZE bytes of readable user memory.
    unsafe {
        core::ptr::copy_nonoverlapping(in_ptr as *const u8, record.as_mut_ptr(), REC_SIZE);
    }

    let [d0, d1, d2, d3, m0, m1, m2, m3] = record;
    let destination = crate::netns::Ipv4Addr([d0, d1, d2, d3]);
    let mask = crate::netns::Ipv4Addr([m0, m1, m2, m3]);

    let ns = crate::sched::current_task_net_ns();
    match crate::netns::remove_route(ns, destination, mask) {
        Ok(()) => SyscallResult::ok(0),
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_NET_ROUTE_LIST` — enumerate the caller's netns routing table.
///
/// See [`SYS_NET_ROUTE_LIST`]. Read-only. Writes 16-byte records (same layout
/// as [`sys_net_route_add`]) up to the buffer capacity; returns the count.
///
/// [`SYS_NET_ROUTE_LIST`]: crate::syscall::number::SYS_NET_ROUTE_LIST
pub fn sys_net_route_list(args: &SyscallArgs) -> SyscallResult {
    let buf_ptr = args.arg0;
    let buf_len = args.arg1 as usize;

    if buf_ptr == 0 {
        return SyscallResult::err(KernelError::InvalidArgument);
    }

    const RECORD_SIZE: usize = 16;
    let max_records = buf_len / RECORD_SIZE;
    if max_records == 0 {
        return SyscallResult::ok(0);
    }
    // Validate the full writable span we might touch (bounded by capacity).
    let ns = crate::sched::current_task_net_ns();
    let entries = crate::netns::routes(ns);
    let to_write = entries.len().min(max_records);
    if to_write == 0 {
        return SyscallResult::ok(0);
    }
    let span = to_write.saturating_mul(RECORD_SIZE);
    if let Err(e) = crate::mm::user::validate_user_write(buf_ptr, span) {
        return SyscallResult::err(e);
    }

    let mut written: usize = 0;
    for route in entries.iter().take(to_write) {
        let mut record = [0u8; RECORD_SIZE];
        record[0..4].copy_from_slice(&route.destination.0);
        record[4..8].copy_from_slice(&route.mask.0);
        record[8..12].copy_from_slice(&route.gateway.0);
        record[12..16].copy_from_slice(&route.metric.to_le_bytes());

        // SAFETY: `buf_ptr..buf_ptr+span` validated writable above; each record
        // lands within that span (written < to_write, offset < span).
        let dst = (buf_ptr as usize).wrapping_add(written.wrapping_mul(RECORD_SIZE)) as *mut u8;
        unsafe {
            core::ptr::copy_nonoverlapping(record.as_ptr(), dst, RECORD_SIZE);
        }
        written = written.wrapping_add(1);
    }

    #[allow(clippy::cast_possible_wrap)]
    SyscallResult::ok(written as i64)
}

/// `SYS_NET_FW_ENABLE` — enable/disable the caller's netns firewall.
///
/// See [`SYS_NET_FW_ENABLE`]. Root-gated. `arg0`: 1 = enable, 0 = disable.
///
/// [`SYS_NET_FW_ENABLE`]: crate::syscall::number::SYS_NET_FW_ENABLE
pub fn sys_net_fw_enable(args: &SyscallArgs) -> SyscallResult {
    if let Err(e) = require_netadmin_authority() {
        return SyscallResult::err(e);
    }
    let ns = crate::sched::current_task_net_ns();
    match args.arg0 {
        0 => crate::net::firewall::ns_disable(ns),
        1 => crate::net::firewall::ns_enable(ns),
        _ => return SyscallResult::err(KernelError::InvalidArgument),
    }
    SyscallResult::ok(0)
}

/// `SYS_NET_FW_SET_POLICY` — set the caller's netns default firewall policy.
///
/// See [`SYS_NET_FW_SET_POLICY`]. Root-gated. `arg0`: 0 = accept, 1 = drop.
///
/// [`SYS_NET_FW_SET_POLICY`]: crate::syscall::number::SYS_NET_FW_SET_POLICY
pub fn sys_net_fw_set_policy(args: &SyscallArgs) -> SyscallResult {
    if let Err(e) = require_netadmin_authority() {
        return SyscallResult::err(e);
    }
    let policy = match args.arg0 {
        0 => crate::net::firewall::DefaultPolicy::Accept,
        1 => crate::net::firewall::DefaultPolicy::Drop,
        _ => return SyscallResult::err(KernelError::InvalidArgument),
    };
    let ns = crate::sched::current_task_net_ns();
    crate::net::firewall::ns_set_default_policy(ns, policy);
    SyscallResult::ok(0)
}

/// `SYS_NET_FW_ADD_RULE` — add an IPv4 firewall rule to the caller's netns.
///
/// See [`SYS_NET_FW_ADD_RULE`] for the 12-byte record layout. Root-gated.
/// Returns the assigned rule index (>= 0).
///
/// [`SYS_NET_FW_ADD_RULE`]: crate::syscall::number::SYS_NET_FW_ADD_RULE
pub fn sys_net_fw_add_rule(args: &SyscallArgs) -> SyscallResult {
    use crate::net::firewall::{Action, Direction, Protocol, Rule};
    use crate::net::interface::Ipv4Addr;

    if let Err(e) = require_netadmin_authority() {
        return SyscallResult::err(e);
    }

    let in_ptr = args.arg0;
    let buf_len = args.arg1 as usize;

    const REC_SIZE: usize = 12;
    if in_ptr == 0 || buf_len < REC_SIZE {
        return SyscallResult::err(KernelError::InvalidArgument);
    }
    if let Err(e) = crate::mm::user::validate_user_read(in_ptr, REC_SIZE) {
        return SyscallResult::err(e);
    }
    let mut record = [0u8; REC_SIZE];
    // SAFETY: validated above for exactly REC_SIZE bytes of readable user memory.
    unsafe {
        core::ptr::copy_nonoverlapping(in_ptr as *const u8, record.as_mut_ptr(), REC_SIZE);
    }

    // Destructure by value: named fields, no indexing on the decode path.
    let [dir_b, act_b, proto_b, prefix_b, dp0, dp1, pr0, pr1, s0, s1, s2, s3] = record;

    let direction = match dir_b {
        0 => Direction::In,
        1 => Direction::Out,
        2 => Direction::Both,
        _ => return SyscallResult::err(KernelError::InvalidArgument),
    };
    let action = match act_b {
        0 => Action::Allow,
        1 => Action::Deny,
        _ => return SyscallResult::err(KernelError::InvalidArgument),
    };
    let protocol = match proto_b {
        0 => Protocol::Any,
        1 => Protocol::Tcp,
        2 => Protocol::Udp,
        3 => Protocol::Icmp,
        _ => return SyscallResult::err(KernelError::InvalidArgument),
    };
    if prefix_b > 32 {
        return SyscallResult::err(KernelError::InvalidArgument);
    }
    let dst_port = u16::from_le_bytes([dp0, dp1]);
    let priority = u16::from_le_bytes([pr0, pr1]);
    let src_ip = Ipv4Addr([s0, s1, s2, s3]);

    let rule = Rule {
        active: true,
        direction,
        action,
        protocol,
        src_ip,
        src_prefix: prefix_b,
        dst_port,
        priority,
        match_count: 0,
    };

    let ns = crate::sched::current_task_net_ns();
    match crate::net::firewall::ns_add_rule(ns, rule) {
        // Rule indices are bounded by MAX_RULES (small), so the cast is safe.
        #[allow(clippy::cast_possible_wrap)]
        Ok(idx) => SyscallResult::ok(idx as i64),
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_NET_FW_DEL_RULE` — remove an IPv4 firewall rule by index from the
/// caller's netns.
///
/// See [`SYS_NET_FW_DEL_RULE`]. Root-gated. `arg0`: rule index.
///
/// [`SYS_NET_FW_DEL_RULE`]: crate::syscall::number::SYS_NET_FW_DEL_RULE
pub fn sys_net_fw_del_rule(args: &SyscallArgs) -> SyscallResult {
    if let Err(e) = require_netadmin_authority() {
        return SyscallResult::err(e);
    }
    let index = args.arg0 as usize;
    let ns = crate::sched::current_task_net_ns();
    match crate::net::firewall::ns_remove_rule(ns, index) {
        Ok(()) => SyscallResult::ok(0),
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_NET_FW_FLUSH` — remove all firewall rules from the caller's netns.
///
/// See [`SYS_NET_FW_FLUSH`]. Root-gated. Leaves enabled state and default
/// policy unchanged.
///
/// [`SYS_NET_FW_FLUSH`]: crate::syscall::number::SYS_NET_FW_FLUSH
pub fn sys_net_fw_flush(_args: &SyscallArgs) -> SyscallResult {
    if let Err(e) = require_netadmin_authority() {
        return SyscallResult::err(e);
    }
    let ns = crate::sched::current_task_net_ns();
    crate::net::firewall::ns_clear_rules(ns);
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

            // SAFETY: buf_ptr is validated; written < max_records.
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

    // SAFETY: out_ptr is validated non-null and buf_len ≥ STATS_SIZE above.
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
