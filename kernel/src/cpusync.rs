//! Cross-CPU barriers: making the CPUs that run a process pass a full memory
//! barrier, and a serializing instruction before they run its code again --
//! what `membarrier`'s expedited commands promise (`syscall::linux`'s
//! `sys_membarrier`, the native `SYS_MEMBARRIER`; design-decisions 1545).
//!
//! ## Why an interrupt is the barrier
//!
//! A CPU running user code cannot be asked to fence; it can be interrupted.
//! The interrupt stops it between two of its instructions, and this module's
//! handler acknowledges with a locked read-modify-write -- a full barrier on
//! x86: every store the interrupted code made is globally visible before the
//! acknowledgement is, and the initiator reads the acknowledgement with
//! acquire ordering. The handler returns to user mode through `iretq`, a
//! serializing instruction, which is what
//! `MEMBARRIER_CMD_PRIVATE_EXPEDITED_SYNC_CORE` asks for: a CPU that runs the
//! process after new code was written cannot run stale bytes.
//!
//! A CPU that is not running the process when the request is made needs no
//! interrupt: before it runs the process again it switches to it, and the
//! switch records the address space it is about to run
//! (`sched::note_dispatch`) with a sequentially consistent store, takes and
//! drops the scheduler's lock, and loads CR3 (serializing). The initiator
//! fences before it reads which CPU runs what, so of the two -- the
//! initiator's earlier stores, the switching CPU's record -- at least one is
//! seen by the other: either the CPU is asked, or it runs the process only
//! after the initiator's stores are visible to it. Linux's reasoning for the
//! same shortcut (`membarrier_switch_mm`).
//!
//! ## The protocol
//!
//! The TLB shootdown's (`tlb.rs`), with a CPU mask, for the same two hangs:
//!
//! 1. The initiator takes [`SYNC_LOCK`] with preemption disabled, publishes
//!    the mask, resets [`ACK_COUNT`], then bumps [`SYNC_SEQ`] -- so a CPU that
//!    sees the new number sees the request it names.
//! 2. It sends [`CPU_SYNC_VECTOR`] to each CPU in the mask, and waits until
//!    each has acknowledged.
//! 3. A CPU answers a request once ([`HANDLED_SEQ`], a compare-and-swap), and
//!    acknowledges it only if the mask names it: from the interrupt handler,
//!    or while it spins for [`SYNC_LOCK`] itself, so two CPUs asking at once
//!    cannot wait for each other.
//!
//! The initiator spins with interrupts enabled, so a TLB shootdown started
//! meanwhile by another CPU is still answered here, and a barrier request is
//! answered by a CPU that is waiting for a shootdown.

use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

use crate::serial_println;

/// The IPI vector of a barrier request: below the TLB shootdown's (251) and
/// the reschedule IPI (252), above every device vector (33-56).
pub const CPU_SYNC_VECTOR: u8 = 250;

/// One request at a time (see the module doc).
static SYNC_LOCK: spin::Mutex<()> = spin::Mutex::new(());

/// Sequence number of the latest request; 0 before the first.
static SYNC_SEQ: AtomicU64 = AtomicU64::new(0);

/// The CPUs the latest request asks (bit N = CPU N).
static SYNC_TARGETS: AtomicU64 = AtomicU64::new(0);

/// How many of them have acknowledged it.
static ACK_COUNT: AtomicU32 = AtomicU32::new(0);

/// Whether the latest request also restarts the rseq critical section of
/// each thread it interrupts (`MEMBARRIER_CMD_PRIVATE_EXPEDITED_RSEQ`): the
/// answering CPU is marked for the rseq work its interrupt's exit then does
/// (`crate::rseq`), as Linux's `ipi_rseq` calls `rseq_preempt`.
static SYNC_RSEQ: AtomicBool = AtomicBool::new(false);

/// Per CPU: the latest request it has answered (or, for its initiator,
/// issued), so that a request answered from the handler and from a spin on
/// the lock is acknowledged once.
static HANDLED_SEQ: [AtomicU64; crate::smp::MAX_CPUS] =
    [const { AtomicU64::new(0) }; crate::smp::MAX_CPUS];

/// Requests that interrupted at least one CPU, since boot.
static REQUESTS: AtomicU64 = AtomicU64::new(0);

/// Interrupts sent for them, since boot.
static IPIS_SENT: AtomicU64 = AtomicU64::new(0);

/// The requests that interrupted a CPU, and the interrupts they sent, since
/// boot.
#[must_use]
pub fn stats() -> (u64, u64) {
    (
        REQUESTS.load(Ordering::Relaxed),
        IPIS_SENT.load(Ordering::Relaxed),
    )
}

/// Bit `cpu` of a CPU mask (0 for a CPU number past the mask).
fn cpu_bit(cpu: usize) -> u64 {
    u32::try_from(cpu)
        .ok()
        .and_then(|c| 1u64.checked_shl(c))
        .unwrap_or(0)
}

/// Make every online CPU in `targets` other than this one pass a full memory
/// barrier, and a serializing instruction before it next runs user code;
/// return once each has. This CPU fences itself, before and after.
///
/// From task context with interrupts enabled: the wait is a spin, and a CPU
/// waiting with interrupts off could be the one a concurrent request needs.
pub fn sync_cpus(targets: u64) {
    sync_cpus_for(targets, false);
}

/// [`sync_cpus`], and each thread it interrupts that is inside an rseq
/// critical section restarts it, on its way back to user mode
/// (`MEMBARRIER_CMD_PRIVATE_EXPEDITED_RSEQ`).
pub fn sync_cpus_restarting_rseq(targets: u64) {
    sync_cpus_for(targets, true);
}

/// [`sync_cpus`], with or without the rseq restart.
fn sync_cpus_for(targets: u64, restart_rseq: bool) {
    core::sync::atomic::fence(Ordering::SeqCst);
    let me = crate::smp::fast_cpu_index();
    let online = crate::cpu_hotplug::online_mask();
    // Before hotplug's mask exists, every CPU smp brought up is online.
    let online = if online == 0 {
        (0..crate::smp::cpu_count()).fold(0, |m, c| m | cpu_bit(c))
    } else {
        online
    };
    let others = targets & online & !cpu_bit(me);
    if others == 0 {
        return;
    }
    REQUESTS.fetch_add(1, Ordering::Relaxed);
    crate::sched::preempt_disable();
    let guard = loop {
        if let Some(g) = SYNC_LOCK.try_lock() {
            break g;
        }
        // Another CPU is asking, and may be asking this one.
        service_pending(me);
        core::hint::spin_loop();
    };
    SYNC_TARGETS.store(others, Ordering::Release);
    SYNC_RSEQ.store(restart_rseq, Ordering::Release);
    ACK_COUNT.store(0, Ordering::Release);
    let seq = SYNC_SEQ.fetch_add(1, Ordering::AcqRel).wrapping_add(1);
    if let Some(mine) = HANDLED_SEQ.get(me) {
        mine.store(seq, Ordering::Release);
    }
    let wanted = others.count_ones();
    let mut pending = others;
    while pending != 0 {
        let cpu = pending.trailing_zeros() as usize;
        pending &= pending.wrapping_sub(1);
        if let Some(apic_id) = crate::smp::cpu_apic_id(cpu) {
            IPIS_SENT.fetch_add(1, Ordering::Relaxed);
            // SAFETY: the APIC is initialised (another CPU is online), vector
            // 250 has a handler in the IDT, and `cpu` is not this CPU.
            unsafe { crate::apic::send_fixed_ipi(apic_id, CPU_SYNC_VECTOR) };
        } else {
            // No APIC id to send to: count it answered rather than wait for
            // an acknowledgement that cannot come. It is not running user
            // code without one.
            ACK_COUNT.fetch_add(1, Ordering::AcqRel);
        }
    }
    while ACK_COUNT.load(Ordering::Acquire) < wanted {
        core::hint::spin_loop();
    }
    drop(guard);
    crate::sched::preempt_enable();
    core::sync::atomic::fence(Ordering::SeqCst);
}

/// Answer the latest request on `cpu`, if this CPU has not answered it and
/// the request asks it: the barrier is the acknowledgement itself, a locked
/// add.
fn service_pending(cpu: usize) {
    let Some(handled) = HANDLED_SEQ.get(cpu) else {
        return;
    };
    let seq = SYNC_SEQ.load(Ordering::Acquire);
    let seen = handled.load(Ordering::Acquire);
    if seen >= seq {
        return;
    }
    if handled
        .compare_exchange(seen, seq, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return;
    }
    if SYNC_TARGETS.load(Ordering::Acquire) & cpu_bit(cpu) != 0 {
        // Marked before the acknowledgement, so the initiator returns only
        // once every interrupted thread is sure to do the rseq work.
        if SYNC_RSEQ.load(Ordering::Acquire) {
            crate::rseq::notify_this_cpu();
        }
        ACK_COUNT.fetch_add(1, Ordering::AcqRel);
    }
}

/// The handler of [`CPU_SYNC_VECTOR`]: answer the pending request, then
/// end the interrupt. Called from `idt::dispatch_vector`.
pub fn handle_cpu_sync_irq() {
    service_pending(crate::smp::fast_cpu_index());
    // SAFETY: writing the local APIC's EOI register is always safe.
    unsafe {
        crate::apic::eoi();
    }
}

/// Every other online CPU is interrupted and answers; asking no CPU, or
/// only this one, sends nothing. With one CPU online, both are no-ops.
/// After `smp::init`.
pub fn self_test() -> crate::error::KernelResult<()> {
    let me = crate::smp::fast_cpu_index();
    let (requests0, ipis0) = stats();
    sync_cpus(0);
    sync_cpus(cpu_bit(me));
    if stats() != (requests0, ipis0) {
        serial_println!("[cpusync]   FAIL: asking no other CPU interrupted one");
        return Err(crate::error::KernelError::InternalError);
    }
    let cpus = crate::smp::cpu_count();
    if cpus < 2 {
        serial_println!("[cpusync]   one CPU online: nothing to interrupt: OK");
        return Ok(());
    }
    sync_cpus(u64::MAX);
    let seq = SYNC_SEQ.load(Ordering::Acquire);
    let (requests1, ipis1) = stats();
    let asked = u64::try_from(cpus.saturating_sub(1)).unwrap_or(u64::MAX);
    let answered = (0..cpus).filter(|&c| c != me).all(|c| {
        HANDLED_SEQ
            .get(c)
            .is_some_and(|h| h.load(Ordering::Acquire) >= seq)
    });
    if requests1 != requests0.saturating_add(1) || ipis1.saturating_sub(ipis0) != asked || !answered
    {
        serial_println!(
            "[cpusync]   FAIL: {} CPUs: requests {} -> {}, interrupts {} -> {}, all answered {}",
            cpus,
            requests0,
            requests1,
            ipis0,
            ipis1,
            answered
        );
        return Err(crate::error::KernelError::InternalError);
    }
    serial_println!(
        "[cpusync]   every other CPU of {} interrupted and answered: OK",
        cpus
    );
    Ok(())
}
