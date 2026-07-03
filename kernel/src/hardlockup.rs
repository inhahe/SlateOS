//! Hard-lockup watchdog driver (Intel i6300esb, QEMU model).
//!
//! ## Why this exists
//!
//! Our timer-driven soft-lockup / liveness watchdogs (see [`crate::watchdog`]
//! and the scheduler's `liveness_*` machinery) all rely on the local-APIC
//! timer interrupt continuing to fire. That assumption fails for one specific,
//! rare, and hard-to-catch bug class: a **BSP-dead total-silence hang** where
//! the bootstrap processor wedges *inside interrupt context with `IF=0`*
//! (e.g. spinning in the timer ISR, or having taken a fault path that never
//! returns while interrupts are masked). When that happens, no timer tick ever
//! runs again, so every timer-based watchdog is dead too — the machine goes
//! completely silent with no diagnostic output.
//!
//! The only thing that can preempt a CPU spinning with `IF=0` is a
//! **Non-Maskable Interrupt**. This driver programs the QEMU i6300esb hardware
//! watchdog to fire an NMI (via `-action watchdog=inject-nmi`) if it is not
//! "kicked" within a timeout window. The kick happens on every BSP timer tick,
//! so as long as the BSP is alive and taking timer interrupts the watchdog
//! never expires. If the BSP wedges, the kicks stop, the hardware counts down,
//! and QEMU injects an NMI into the guest — which the BSP takes *despite*
//! `IF=0`, letting [`crate::idt::handle_nmi`] dump the faulting RIP and the
//! task table so we can finally see *where* the wedge is.
//!
//! ## Blast radius
//!
//! This is a diagnostic instrument, not a production feature. The i6300esb
//! device is only present when the boot harness is invoked with the opt-in
//! `--hard-lockup-watchdog` flag (`scripts/boot-test.sh`). On a normal boot the
//! PCI probe finds no device and every entry point here is a cheap no-op, so
//! this module cannot affect ordinary operation. The watchdog is only *armed*
//! during the narrow boot self-test window (armed just before the ring-3
//! container self-tests, disarmed at `BOOT_OK`).
//!
//! ## Hardware reference
//!
//! QEMU `hw/watchdog/wdt_i6300esb.c`. PCI vendor `0x8086`, device `0x25ab`.
//! Two-stage down-counter: stage 1 (`timer1_preload`) → stage 2
//! (`timer2_preload`) → `watchdog_perform_action()`. Register writes require an
//! unlock sequence (`0x80` then `0x86` to the reload register) before each
//! honored write.

// Diagnostic instrument: several helpers are only wired in under the opt-in
// boot flag, so not every path has an in-tree caller on a normal build.
#![allow(dead_code)]

use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

use crate::mm::frame::PhysFrame;
use crate::mm::page_table::{self, PageFlags, VirtAddr};
use crate::pci;

// ---------------------------------------------------------------------------
// Device identity
// ---------------------------------------------------------------------------

/// Intel vendor ID.
const I6300ESB_VENDOR: u16 = 0x8086;
/// i6300esb watchdog device ID (QEMU model).
const I6300ESB_DEVICE: u16 = 0x25ab;

// ---------------------------------------------------------------------------
// MMIO register offsets (within BAR0)
// ---------------------------------------------------------------------------

/// Timer1 preload value, reloaded at the start of stage 1.
const REG_TIMER1: u32 = 0x00;
/// Timer2 preload value, reloaded at the start of stage 2.
const REG_TIMER2: u32 = 0x04;
/// General interrupt status register.
const REG_GINTSR: u32 = 0x08;
/// Reload register — target of the unlock and ping sequences.
const REG_RELOAD: u32 = 0x0c;

/// First unlock token, written to [`REG_RELOAD`].
const UNLOCK1: u32 = 0x80;
/// Second unlock token, written to [`REG_RELOAD`] after [`UNLOCK1`].
const UNLOCK2: u32 = 0x86;
/// Reload/ping bit — restarts the counter at stage 1.
const RELOAD_PING: u32 = 0x100;

// ---------------------------------------------------------------------------
// PCI config-space registers
// ---------------------------------------------------------------------------

/// Watchdog configuration register (16-bit, PCI config space).
const CFG_CONFIG: u8 = 0x60;
/// Watchdog lock register (8-bit, PCI config space).
const CFG_LOCK: u8 = 0x68;

/// CONFIG: enable reboot action on stage-2 expiry.
///
/// NOTE the inverted hardware logic in QEMU:
/// `reboot_enabled = (data & ESB_WDT_REBOOT) == 0`. The action fires only when
/// this bit is **clear**, so we deliberately leave it 0.
const CONFIG_REBOOT: u16 = 1 << 5;
/// CONFIG: decrement-frequency select. 0 = 1 kHz (our choice), 1 = 1 MHz.
const CONFIG_FREQ: u16 = 1 << 2;
/// CONFIG: interrupt type on stage-1 expiry. `0b11` = disabled (no IRQ/SMI —
/// we want only the stage-2 NMI action, not a maskable stage-1 interrupt).
const CONFIG_INTTYPE_DISABLED: u16 = 0x3;

/// LOCK: free-run function bit (leave clear for one-shot watchdog mode).
const LOCK_FUNC: u8 = 1 << 2;
/// LOCK: enable the watchdog counter.
const LOCK_ENABLE: u8 = 1 << 1;
/// LOCK: nowayout lock (leave clear so we can disarm at `BOOT_OK`).
const LOCK_LOCK: u8 = 1 << 0;

// ---------------------------------------------------------------------------
// Timer preloads
// ---------------------------------------------------------------------------

/// Stage preload (20-bit). In 1 kHz mode each unit is `32768 * 30 ns`
/// ≈ 983 µs, so ~5000 units ≈ 4.9 s per stage → ~9.8 s across both stages
/// before the NMI fires. Comfortably longer than any legitimate `IF=0` gap
/// during the boot self-test window, short enough to catch a wedge quickly.
const STAGE_PRELOAD: u32 = 5000;

/// Approximate per-stage timeout in milliseconds, for logging.
/// 1 kHz mode: `STAGE_PRELOAD * 32768 * 30 ns` ≈ `STAGE_PRELOAD * 0.983 ms`.
const STAGE_MS: u64 = (STAGE_PRELOAD as u64 * 983) / 1000;

// ---------------------------------------------------------------------------
// Driver state
// ---------------------------------------------------------------------------

/// Mapped BAR0 virtual base address, or 0 if no device is present.
static MMIO_BASE: AtomicU64 = AtomicU64::new(0);
/// Whether the watchdog is currently armed (kicks are honored, NMI is "ours").
static ARMED: AtomicBool = AtomicBool::new(false);
/// Count of watchdog NMIs observed by `handle_nmi`. Lets the fire self-test
/// confirm the NMI→dump chain actually ran, and is a cheap tripwire in general.
static FIRED: AtomicU32 = AtomicU32::new(0);

/// BSP timer-tick heartbeat sampled at the previous watchdog NMI, or
/// [`HEARTBEAT_SENTINEL`] if no NMI has been classified since the last [`arm`].
///
/// Used by [`classify_nmi`] to distinguish a real BSP-dead wedge from a
/// spurious NMI: see that function for the full rationale.
static PREV_NMI_HEARTBEAT: AtomicU64 = AtomicU64::new(HEARTBEAT_SENTINEL);

/// Sentinel meaning "no prior NMI heartbeat recorded since arming".
const HEARTBEAT_SENTINEL: u64 = u64::MAX;

/// Minimum BSP heartbeat advance between two consecutive watchdog NMIs for the
/// BSP to be considered *alive* (a spurious NMI rather than a real wedge).
///
/// A genuine BSP-dead wedge spins with `IF=0`, so `timer_tick` never runs and
/// the heartbeat is frozen — the delta between two consecutive NMIs is exactly
/// 0. A live-but-busy BSP (heavy debug-build compute burst) still takes timer
/// interrupts at TCG translation-block boundaries, so it advances the heartbeat
/// by hundreds of ticks per ~9.8 s window. A threshold of 4 sits far below any
/// live-BSP delta and far above the wedge's 0, so the classification is robust.
const ALIVE_TICKS: u64 = 4;

/// Record that a hard-lockup watchdog NMI was observed (called from
/// [`crate::idt::handle_nmi`] on the armed, no-hardware-error path).
#[inline]
pub fn note_fired() {
    FIRED.fetch_add(1, Ordering::Relaxed);
}

/// Number of hard-lockup watchdog NMIs observed since boot.
#[inline]
pub fn fired_count() -> u32 {
    FIRED.load(Ordering::Relaxed)
}

/// Classify a watchdog NMI as a real BSP-dead wedge vs. a spurious TCG NMI.
///
/// `current_heartbeat` is the value read from [`crate::sched::bsp_heartbeat`]
/// at the moment the NMI is handled. Returns `true` iff the BSP heartbeat has
/// *not* advanced meaningfully since the previous watchdog NMI — i.e. the BSP
/// is genuinely wedged (spinning with `IF=0`, so `timer_tick` cannot run and
/// the heartbeat is frozen). Returns `false` for the first NMI since arming
/// (benefit of the doubt) and for any NMI where the heartbeat has advanced by
/// at least [`ALIVE_TICKS`] (a live-but-busy BSP that took a spurious NMI from
/// QEMU/TCG virtual-clock-vs-APIC-timer divergence during a heavy compute
/// burst).
///
/// # NMI safety
///
/// Runs entirely in NMI context. Touches only a single atomic swap and integer
/// arithmetic — no locks, no allocation, no re-entrant paths.
#[must_use]
pub fn classify_nmi(current_heartbeat: u64) -> bool {
    let prev = PREV_NMI_HEARTBEAT.swap(current_heartbeat, Ordering::AcqRel);
    if prev == HEARTBEAT_SENTINEL {
        // First NMI since arming: we have no baseline to compare against, so we
        // extend the benefit of the doubt and treat it as spurious (re-kick and
        // resume). A genuine wedge will fire a *second* NMI ~9.8 s later, and
        // that one will show a frozen heartbeat and be caught.
        return false;
    }
    current_heartbeat.wrapping_sub(prev) < ALIVE_TICKS
}

/// Returns `true` if the i6300esb device was found and mapped at init.
#[inline]
fn present() -> bool {
    MMIO_BASE.load(Ordering::Acquire) != 0
}

/// Returns `true` if the watchdog is currently armed.
///
/// Used by [`crate::idt::handle_nmi`] to decide whether a software/external
/// NMI should be attributed to this watchdog (and trigger a diagnostic dump).
#[inline]
pub fn is_armed() -> bool {
    ARMED.load(Ordering::Acquire)
}

/// Write a 32-bit MMIO register. No-op if no device is mapped.
///
/// QEMU's i6300esb honors both `writew` and `writel`; we use 32-bit writes
/// uniformly since the timer preloads are 20-bit.
#[inline]
fn write_reg(offset: u32, value: u32) {
    let base = MMIO_BASE.load(Ordering::Acquire);
    if base == 0 {
        return;
    }
    let addr = base.wrapping_add(u64::from(offset)) as *mut u32;
    // SAFETY: `base` is the virtual address of the i6300esb BAR0 MMIO region,
    // mapped NO_CACHE during `init`; `offset` (0x00..=0x0c) stays within the
    // 16-byte register window. Volatile ensures the device sees every write.
    unsafe {
        core::ptr::write_volatile(addr, value);
    }
}

/// Read a 32-bit MMIO register. Returns 0 if no device is mapped.
#[inline]
fn read_reg(offset: u32) -> u32 {
    let base = MMIO_BASE.load(Ordering::Acquire);
    if base == 0 {
        return 0;
    }
    let addr = base.wrapping_add(u64::from(offset)) as *const u32;
    // SAFETY: same invariants as `write_reg`.
    unsafe { core::ptr::read_volatile(addr) }
}

/// Perform the two-token unlock sequence that opens the next register write.
#[inline]
fn unlock() {
    write_reg(REG_RELOAD, UNLOCK1);
    write_reg(REG_RELOAD, UNLOCK2);
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Probe for and program the i6300esb watchdog.
///
/// If the device is absent (normal boot, no `--hard-lockup-watchdog`), this
/// logs a one-line notice and returns; every other entry point becomes a
/// no-op. If present, maps BAR0, programs the config/timer registers, and
/// enables the counter — but leaves the driver **disarmed** so kicks are not
/// yet required. Call [`arm`] to begin the countdown-and-kick regime.
///
/// `hhdm_offset` is the higher-half direct-map offset used to reach the BAR's
/// physical MMIO from kernel virtual space.
pub fn init(hhdm_offset: u64) {
    let Some(dev) = pci::find_device(I6300ESB_VENDOR, I6300ESB_DEVICE) else {
        crate::serial_println!("[hardlockup] i6300esb not present (watchdog disabled)");
        return;
    };

    // Resolve BAR0 MMIO physical base (combine BAR1 for a 64-bit BAR).
    let Some(bar0_phys) = dev.bar0_mmio_addr() else {
        crate::serial_println!("[hardlockup] i6300esb BAR0 is not MMIO — disabled");
        return;
    };
    let bar0_type = (dev.bars[0] >> 1) & 0x3;
    let mmio_phys = if bar0_type == 2 {
        bar0_phys | (u64::from(dev.bars[1]) << 32)
    } else {
        bar0_phys
    };

    // Enable memory-space decoding + bus mastering for the device.
    pci::enable_bus_master(dev.address);

    // Map the single 16 KiB frame covering the 16-byte register window,
    // NO_CACHE (device memory must not be cached).
    let mmio_virt = mmio_phys.wrapping_add(hhdm_offset);
    let pml4_phys = page_table::cr3_to_pml4(page_table::read_cr3());
    let flags = PageFlags::PRESENT | PageFlags::WRITABLE | PageFlags::NO_CACHE;
    if let Some(frame) = PhysFrame::from_addr(mmio_phys & !0x3fff) {
        let virt = VirtAddr::new(mmio_virt & !0x3fff);
        // SAFETY: `frame` is the PCI BAR0 MMIO region for the watchdog; we map
        // it into kernel VA space with device-memory attributes. An existing
        // mapping (e.g. already within HHDM on a large-RAM system) is tolerated
        // — the region is reachable either way.
        let _ = unsafe { page_table::map_frame(pml4_phys, virt, frame, flags) };
        // Flush any stale TLB entry for the mapped page.
        let flush_addr = mmio_virt & !0x3fff;
        // SAFETY: standard invlpg to flush the freshly mapped page.
        unsafe {
            core::arch::asm!("invlpg [{}]", in(reg) flush_addr, options(nostack, preserves_flags));
        }
    } else {
        crate::serial_println!("[hardlockup] i6300esb BAR0 phys invalid — disabled");
        return;
    }

    MMIO_BASE.store(mmio_virt, Ordering::Release);

    // Program CONFIG (16-bit PCI config): reboot action enabled (REBOOT bit
    // CLEAR — inverted logic), 1 kHz mode (FREQ clear), stage-1 interrupt
    // disabled (we only want the stage-2 NMI action).
    // REBOOT=0 (enables action, inverted logic), FREQ=0 (1 kHz), INTTYPE=0b11
    // (stage-1 interrupt disabled). See CONFIG_REBOOT / CONFIG_FREQ docs.
    // Must use a genuine 16-bit access: QEMU's i6300esb only handles the
    // CONFIG register on a 2-byte write (a 4-byte read-modify-write is ignored
    // by the device model and never programs the config).
    let config: u16 = CONFIG_INTTYPE_DISABLED;
    pci::config_write16_native(dev.address.bus, dev.address.device, dev.address.function,
        CFG_CONFIG, config);

    // Program both stage preloads (each write needs a fresh unlock).
    unlock();
    write_reg(REG_TIMER1, STAGE_PRELOAD);
    unlock();
    write_reg(REG_TIMER2, STAGE_PRELOAD);

    crate::serial_println!(
        "[hardlockup] i6300esb armed-ready at {:#x} (phys {:#x}), ~{} ms/stage",
        mmio_virt, mmio_phys, STAGE_MS
    );
}

/// Arm the watchdog: enable the hardware counter and begin requiring kicks.
///
/// After this, [`kick`] must be called at least once per timeout window
/// (~9.8 s) or QEMU injects an NMI. Called just before the ring-3 container
/// self-tests, the window where the total-silence hang was observed.
pub fn arm() {
    if !present() {
        return;
    }

    // Enable the counter via the LOCK register (8-bit): set ENABLE, leave
    // FUNC and LOCK clear (one-shot, disarmable).
    enable_counter(true);

    // Reset the NMI-classification baseline so the first NMI in this arming
    // window gets the benefit of the doubt (see `classify_nmi`).
    PREV_NMI_HEARTBEAT.store(HEARTBEAT_SENTINEL, Ordering::Release);

    // Prime the counter at stage 1 before we start relying on kicks.
    kick();
    ARMED.store(true, Ordering::Release);
    crate::serial_println!("[hardlockup] armed (NMI on ~9.8s BSP silence)");
}

/// Disarm the watchdog: stop the counter and stop requiring kicks.
///
/// Called at `BOOT_OK`, once the boot self-test window (and its hang risk) is
/// past. Safe to call when the device is absent or already disarmed.
pub fn disarm() {
    if !present() {
        return;
    }
    ARMED.store(false, Ordering::Release);
    enable_counter(false);
    crate::serial_println!("[hardlockup] disarmed");
}

/// Kick (reload) the watchdog, restarting the countdown at stage 1.
///
/// Must be called from the BSP timer tick while armed. A no-op if the device
/// is absent or the watchdog is disarmed, so it is cheap to call every tick.
#[inline]
pub fn kick() {
    if !ARMED.load(Ordering::Acquire) {
        return;
    }
    // Each honored register write needs a fresh unlock; the ping bit restarts
    // the counter at stage 1.
    unlock();
    write_reg(REG_RELOAD, RELOAD_PING);
}

/// Enable or disable the hardware counter via the PCI LOCK register.
fn enable_counter(enable: bool) {
    let Some(dev) = pci::find_device(I6300ESB_VENDOR, I6300ESB_DEVICE) else {
        return;
    };
    let (bus, device, function) = (dev.address.bus, dev.address.device, dev.address.function);
    // The LOCK register is 8-bit at 0x68. QEMU's i6300esb only handles it on a
    // genuine 1-byte access — a 4-byte read-modify-write (config_write16) is
    // ignored by the device model, so the ENABLE bit never reaches
    // i6300esb_restart_timer and the counter never starts. Read the current
    // byte, adjust, and write it back with a true byte access.
    let mut byte = pci::config_read8(bus, device, function, CFG_LOCK);
    if enable {
        byte |= LOCK_ENABLE;
        byte &= !LOCK_LOCK; // keep disarmable
        byte &= !LOCK_FUNC; // one-shot mode
    } else {
        byte &= !LOCK_ENABLE;
    }
    pci::config_write8(bus, device, function, CFG_LOCK, byte);
}

/// Deliberately force the watchdog to fire, to validate the NMI→dump path.
///
/// This is a **diagnostic self-test**, not part of normal boot — it is never
/// auto-invoked (calling it unconditionally would add a ~15 s stall to every
/// watchdog boot and latch the one-shot dump, spoiling a real catch). Wire a
/// temporary call in for a one-off validation, or trigger it from a debug
/// path.
///
/// It arms the watchdog, then spins with interrupts disabled and **without**
/// kicking for longer than the ~9.8 s timeout — reproducing the exact BSP-dead
/// `IF=0` condition of the hang we hunt — so QEMU injects an NMI that
/// [`crate::idt::handle_nmi`] must catch. Because the injected NMI is
/// non-maskable it is delivered despite `IF=0`, and the TSC (hence
/// [`crate::cpu::delay_us`]) keeps advancing, so the spin is bounded. Prints a
/// PASS/FAIL verdict by comparing [`fired_count`] across the spin.
///
/// No-op (with a note) when the device is absent.
pub fn self_test_fire() {
    if !present() {
        crate::serial_println!("[hardlockup] self-test-fire: no device present, skipping");
        return;
    }
    arm();
    let before = fired_count();
    crate::serial_println!(
        "[hardlockup] self-test-fire: BSP entering IF=0 no-kick spin (~15s); expect NMI ~10s in"
    );
    // Reproduce the BSP-dead condition: interrupts masked (so timer_tick — and
    // thus kick() — cannot run) while we busy-wait past the watchdog timeout.
    // without_interrupts restores the prior IF state afterward. The NMI fires
    // *inside* this window; handle_nmi runs, dumps, and returns here.
    crate::cpu::without_interrupts(|| {
        crate::cpu::delay_us(15_000_000);
    });
    let after = fired_count();
    if after > before {
        crate::serial_println!(
            "[hardlockup] self-test-fire: PASS — NMI observed (fired {before} -> {after})"
        );
    } else {
        crate::serial_println!(
            "[hardlockup] self-test-fire: FAIL — no NMI during 15s IF=0 spin (fired={after})"
        );
    }
}
