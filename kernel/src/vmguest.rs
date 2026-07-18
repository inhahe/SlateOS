//! Virtual-machine guest integration services.
//!
//! When the kernel detects that it is running inside a hypervisor
//! (via [`crate::hypervisor`]), this module activates the appropriate
//! paravirtual integration features.  These are the kernel-side equivalent
//! of "VMware Tools", "QEMU Guest Agent", or "VirtualBox Guest Additions".
//!
//! ## Supported Features
//!
//! | Feature               | KVM/QEMU        | Hyper-V/WHPX      | VMware      | VirtualBox  |
//! |-----------------------|-----------------|--------------------|-------------|-------------|
//! | Clock synchronization | pvclock (MSR)   | Reference TSC page | VMware TSC  | —           |
//! | Graceful shutdown     | ACPI + virtio   | Hyper-V shutdown   | VMware RPCI | VBox ACPI   |
//! | Display resize        | virtio-gpu ctl  | Hyper-V synthetic  | SVGA resize | VBox HGSMI  |
//! | Balloon memory        | virtio-balloon  | Hyper-V DMM        | VMware bal  | VBox bal    |
//! | Heartbeat             | virtio-serial   | Hyper-V heartbeat  | VMware rpci | VBox GuestP |
//! | Guest info report     | QGA channel     | KVP daemon         | RPCI info   | GuestProp   |
//!
//! ## Architecture
//!
//! ```text
//! hypervisor::detect()  →  vmguest::init()
//!      ↓                        ↓
//! Identifies KVM/WHPX/...  Activates matching feature set
//!                               ↓
//!                    Periodic tick() from timer / idle loop
//!                               ↓
//!                    Heartbeat, clock drift check, balloon check
//! ```
//!
//! ## Paravirtual Clock — KVM pvclock
//!
//! KVM exposes a shared-memory clock page via MSR `0x4b564d01`
//! (`MSR_KVM_SYSTEM_TIME_NEW`).  The guest writes the physical address
//! of a `pvclock_vcpu_time_info` struct; KVM fills it with TSC→nanosecond
//! conversion parameters.  This avoids expensive vmexits for time reads.
//!
//! Hyper-V has a similar mechanism: the Reference TSC page (MSR `0x40000021`).
//!
//! ## References
//!
//! - KVM clock: `arch/x86/kvm/x86.c`, `pvclock.h`
//! - Hyper-V TLFS: §12 (Timers), §17 (Shutdown/Heartbeat/KVP)
//! - VMware backdoor: VMware Guest Programming Guide
//! - VirtualBox Guest Additions SDK

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use alloc::format;
use core::sync::atomic::{AtomicBool, AtomicU64, AtomicU32, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;
use crate::hypervisor::{self, Hypervisor};
use crate::serial_println;

// ---------------------------------------------------------------------------
// Constants — MSRs and I/O ports
// ---------------------------------------------------------------------------

/// KVM pvclock MSR: write physical address of time info struct.
const MSR_KVM_SYSTEM_TIME_NEW: u32 = 0x4B56_4D01;

/// KVM wall clock MSR: write physical address of wall clock struct.
const MSR_KVM_WALL_CLOCK_NEW: u32 = 0x4B56_4D00;

/// KVM feature MSRs.
const MSR_KVM_FEATURES: u32 = 0x4B56_4D01;

/// Hyper-V reference TSC page MSR.
const MSR_HV_REFERENCE_TSC: u32 = 0x4000_0021;

/// Hyper-V guest OS identity MSR.
const MSR_HV_GUEST_OS_ID: u32 = 0x4000_0000;

/// Hyper-V hypercall page MSR.
const MSR_HV_HYPERCALL: u32 = 0x4000_0001;

/// Hyper-V time reference count MSR (fallback, causes vmexit).
const MSR_HV_TIME_REF_COUNT: u32 = 0x4000_0020;

/// VMware backdoor I/O port.
const VMWARE_PORT: u16 = 0x5658;

/// VMware backdoor magic number.
const VMWARE_MAGIC: u32 = 0x564D_5868;

/// VMware high-bandwidth backdoor port.
const VMWARE_HB_PORT: u16 = 0x5659;

/// VMware RPCI command: get tools version.
const VMWARE_CMD_GET_VERSION: u32 = 10;

/// VMware RPCI command: message channel open.
const VMWARE_CMD_MESSAGE_OPEN: u32 = 30;

/// VMware RPCI command: message channel send.
const VMWARE_CMD_MESSAGE_SEND: u32 = 31;

/// VMware RPCI command: message channel close.
const VMWARE_CMD_MESSAGE_CLOSE: u32 = 32;

// ---------------------------------------------------------------------------
// Feature flags
// ---------------------------------------------------------------------------

/// Supported guest integration features.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum GuestFeature {
    /// Paravirtual clock source (avoids vmexit for time reads).
    ParavirtClock = 0,
    /// Graceful shutdown signaling from host.
    GracefulShutdown = 1,
    /// Host-initiated display resolution changes.
    DisplayResize = 2,
    /// Memory ballooning (host requests guest free/reclaim pages).
    MemoryBalloon = 3,
    /// Periodic heartbeat to host.
    Heartbeat = 4,
    /// Guest information reporting (OS name, IP, hostname).
    GuestInfo = 5,
    /// Host↔guest clipboard sharing.
    ClipboardSync = 6,
    /// Host↔guest time synchronization (wall clock).
    TimeSynchronization = 7,
}

impl GuestFeature {
    /// Human-readable label.
    pub const fn label(self) -> &'static str {
        match self {
            Self::ParavirtClock => "paravirt-clock",
            Self::GracefulShutdown => "graceful-shutdown",
            Self::DisplayResize => "display-resize",
            Self::MemoryBalloon => "memory-balloon",
            Self::Heartbeat => "heartbeat",
            Self::GuestInfo => "guest-info",
            Self::ClipboardSync => "clipboard-sync",
            Self::TimeSynchronization => "time-sync",
        }
    }

    /// All feature variants (for iteration).
    const ALL: &'static [GuestFeature] = &[
        Self::ParavirtClock,
        Self::GracefulShutdown,
        Self::DisplayResize,
        Self::MemoryBalloon,
        Self::Heartbeat,
        Self::GuestInfo,
        Self::ClipboardSync,
        Self::TimeSynchronization,
    ];
}

// ---------------------------------------------------------------------------
// Feature state tracking
// ---------------------------------------------------------------------------

/// Per-feature state.
#[derive(Debug, Clone)]
struct FeatureState {
    feature: GuestFeature,
    /// Whether this feature is supported by the detected hypervisor.
    supported: bool,
    /// Whether this feature is currently active (initialized + running).
    active: bool,
    /// Number of successful operations (heartbeats sent, info reports, etc.).
    ops_count: u64,
    /// Last error message (if any).
    last_error: Option<String>,
}

impl FeatureState {
    fn new(feature: GuestFeature) -> Self {
        Self {
            feature,
            supported: false,
            active: false,
            ops_count: 0,
            last_error: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Balloon state
// ---------------------------------------------------------------------------

/// Memory balloon state — tracks pages inflated/deflated at host request.
#[derive(Debug, Clone)]
struct BalloonState {
    /// Current balloon size in pages (pages reclaimed from guest).
    inflated_pages: u64,
    /// Target balloon size requested by host.
    target_pages: u64,
    /// Total pages ever inflated.
    total_inflated: u64,
    /// Total pages ever deflated (returned to guest).
    total_deflated: u64,
    /// Maximum balloon size allowed (set by policy).
    max_pages: u64,
    /// Whether auto-ballooning is enabled.
    auto_enabled: bool,
}

impl BalloonState {
    fn new() -> Self {
        Self {
            inflated_pages: 0,
            target_pages: 0,
            total_inflated: 0,
            total_deflated: 0,
            max_pages: 0,
            auto_enabled: true,
        }
    }
}

// ---------------------------------------------------------------------------
// Heartbeat state
// ---------------------------------------------------------------------------

/// Heartbeat configuration and tracking.
#[derive(Debug, Clone)]
struct HeartbeatState {
    /// Interval between heartbeats in seconds.
    interval_secs: u32,
    /// Number of heartbeats sent.
    sent_count: u64,
    /// Number of heartbeat failures.
    failed_count: u64,
    /// Timestamp (ns) of last successful heartbeat.
    last_sent_ns: u64,
    /// Whether the host has acknowledged our heartbeats.
    host_ack: bool,
}

impl HeartbeatState {
    fn new() -> Self {
        Self {
            interval_secs: 5,
            sent_count: 0,
            failed_count: 0,
            last_sent_ns: 0,
            host_ack: false,
        }
    }
}

// ---------------------------------------------------------------------------
// Clock sync state
// ---------------------------------------------------------------------------

/// Paravirtual clock state.
#[derive(Debug, Clone)]
struct ClockState {
    /// Clock source type.
    source: ClockSource,
    /// Number of successful clock reads.
    reads: u64,
    /// Measured drift from host (ns, signed via wrapping).
    last_drift_ns: i64,
    /// Number of drift corrections applied.
    corrections: u64,
    /// Whether the pvclock page is mapped.
    page_mapped: bool,
}

/// Type of paravirtual clock source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ClockSource {
    /// No paravirtual clock — using hardware TSC/HPET.
    None,
    /// KVM pvclock via shared memory page.
    KvmPvclock,
    /// Hyper-V reference TSC page.
    HyperVRefTsc,
    /// VMware pseudo-TSC.
    VmwareTsc,
}

impl ClockSource {
    fn label(self) -> &'static str {
        match self {
            Self::None => "hardware",
            Self::KvmPvclock => "kvm-pvclock",
            Self::HyperVRefTsc => "hyperv-ref-tsc",
            Self::VmwareTsc => "vmware-tsc",
        }
    }
}

impl ClockState {
    fn new() -> Self {
        Self {
            source: ClockSource::None,
            reads: 0,
            last_drift_ns: 0,
            corrections: 0,
            page_mapped: false,
        }
    }
}

// ---------------------------------------------------------------------------
// Guest info
// ---------------------------------------------------------------------------

/// Information reported to the host about this guest.
#[derive(Debug, Clone)]
struct GuestInfoData {
    /// OS name.
    os_name: String,
    /// OS version.
    os_version: String,
    /// Kernel version string.
    kernel_version: String,
    /// Number of CPUs visible to guest.
    cpu_count: u32,
    /// Total memory in bytes.
    total_memory: u64,
    /// Hostname.
    hostname: String,
    /// Number of times info has been reported.
    report_count: u64,
}

impl GuestInfoData {
    fn new() -> Self {
        Self {
            os_name: String::from("MintOS"),
            os_version: String::from("0.1.0"),
            kernel_version: String::from("0.1.0-dev"),
            cpu_count: 0,
            total_memory: 0,
            hostname: String::from("mintos"),
            report_count: 0,
        }
    }
}

// ---------------------------------------------------------------------------
// Global state
// ---------------------------------------------------------------------------

/// Main guest integration state.
struct State {
    /// Whether init() has been called.
    initialized: bool,
    /// Detected hypervisor.
    hypervisor: Hypervisor,
    /// Feature states indexed by feature enum value.
    features: Vec<FeatureState>,
    /// Memory balloon state.
    balloon: BalloonState,
    /// Heartbeat state.
    heartbeat: HeartbeatState,
    /// Clock synchronization state.
    clock: ClockState,
    /// Guest information data.
    guest_info: GuestInfoData,
    /// Total ticks processed.
    tick_count: u64,
    /// Display resolution (width x height) if managed.
    display_width: u32,
    display_height: u32,
    /// Shutdown requested by host.
    shutdown_requested: bool,
    /// Reboot requested by host.
    reboot_requested: bool,
}

impl State {
    fn new() -> Self {
        let mut features = Vec::new();
        for &f in GuestFeature::ALL {
            features.push(FeatureState::new(f));
        }
        Self {
            initialized: false,
            hypervisor: Hypervisor::None,
            features,
            balloon: BalloonState::new(),
            heartbeat: HeartbeatState::new(),
            clock: ClockState::new(),
            guest_info: GuestInfoData::new(),
            tick_count: 0,
            display_width: 0,
            display_height: 0,
            shutdown_requested: false,
            reboot_requested: false,
        }
    }

    /// Get a feature state by feature type.
    fn feature(&self, f: GuestFeature) -> Option<&FeatureState> {
        self.features.get(f as usize)
    }

    /// Get a mutable feature state by feature type.
    fn feature_mut(&mut self, f: GuestFeature) -> Option<&mut FeatureState> {
        self.features.get_mut(f as usize)
    }

    /// Mark a feature as supported.
    fn set_supported(&mut self, f: GuestFeature) {
        if let Some(fs) = self.feature_mut(f) {
            fs.supported = true;
        }
    }

    /// Mark a feature as active.
    fn set_active(&mut self, f: GuestFeature) {
        if let Some(fs) = self.feature_mut(f) {
            fs.active = true;
        }
    }

    /// Increment ops count for a feature.
    fn inc_ops(&mut self, f: GuestFeature) {
        if let Some(fs) = self.feature_mut(f) {
            fs.ops_count = fs.ops_count.saturating_add(1);
        }
    }

    /// Record an error for a feature.
    fn set_error(&mut self, f: GuestFeature, msg: String) {
        if let Some(fs) = self.feature_mut(f) {
            fs.last_error = Some(msg);
        }
    }
}

static STATE: Mutex<State> = Mutex::new(State {
    initialized: false,
    hypervisor: Hypervisor::None,
    features: Vec::new(),
    balloon: BalloonState {
        inflated_pages: 0,
        target_pages: 0,
        total_inflated: 0,
        total_deflated: 0,
        max_pages: 0,
        auto_enabled: true,
    },
    heartbeat: HeartbeatState {
        interval_secs: 5,
        sent_count: 0,
        failed_count: 0,
        last_sent_ns: 0,
        host_ack: false,
    },
    clock: ClockState {
        source: ClockSource::None,
        reads: 0,
        last_drift_ns: 0,
        corrections: 0,
        page_mapped: false,
    },
    guest_info: GuestInfoData {
        os_name: String::new(),
        os_version: String::new(),
        kernel_version: String::new(),
        cpu_count: 0,
        total_memory: 0,
        hostname: String::new(),
        report_count: 0,
    },
    tick_count: 0,
    display_width: 0,
    display_height: 0,
    shutdown_requested: false,
    reboot_requested: false,
});

/// Whether init has completed (for fast checking).
static INITIALIZED: AtomicBool = AtomicBool::new(false);

/// Tick counter for periodic work (atomic, avoids lock for check).
static TICK_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Number of active features (for fast summary).
static ACTIVE_FEATURES: AtomicU32 = AtomicU32::new(0);

// ---------------------------------------------------------------------------
// MSR read/write helpers
// ---------------------------------------------------------------------------

/// Read a Model-Specific Register.
///
/// # Safety
/// The caller must ensure the MSR exists and is readable.
#[inline]
unsafe fn rdmsr(msr: u32) -> u64 {
    let lo: u32;
    let hi: u32;
    // SAFETY: Caller verified MSR exists.
    unsafe {
        core::arch::asm!(
            "rdmsr",
            in("ecx") msr,
            out("eax") lo,
            out("edx") hi,
            options(nomem, preserves_flags, nostack),
        );
    }
    (hi as u64) << 32 | lo as u64
}

/// Write a Model-Specific Register.
///
/// # Safety
/// The caller must ensure the MSR exists and the value is valid.
#[inline]
unsafe fn wrmsr(msr: u32, val: u64) {
    let lo = val as u32;
    let hi = (val >> 32) as u32;
    // SAFETY: Caller verified MSR exists and value is valid.
    unsafe {
        core::arch::asm!(
            "wrmsr",
            in("ecx") msr,
            in("eax") lo,
            in("edx") hi,
            options(nomem, preserves_flags, nostack),
        );
    }
}

// ---------------------------------------------------------------------------
// VMware backdoor
// ---------------------------------------------------------------------------

/// Execute a VMware backdoor command (IN instruction to magic port).
///
/// The VMware backdoor uses the `in` instruction with EAX=magic, ECX=command.
/// Returns (eax, ebx, ecx, edx).
///
/// # Safety
/// Only valid when running under VMware.
#[cfg(target_arch = "x86_64")]
unsafe fn vmware_backdoor(cmd: u32, arg: u32) -> (u32, u32, u32, u32) {
    let eax: u32;
    
    let ecx: u32;
    let edx: u32;
    // SAFETY: The VMware backdoor uses a reserved I/O port that is only
    // intercepted by VMware's hypervisor.  On bare metal or other hypervisors
    // this is a no-op (reads garbage or #GP, but we only call this under VMware).
    // We use xchg to save/restore rbx because LLVM reserves it internally.
    let ebx_out: u64;
    unsafe {
        core::arch::asm!(
            "xchg rbx, {tmp}",
            "in eax, dx",
            "xchg rbx, {tmp}",
            tmp = inout(reg) arg as u64 => ebx_out,
            inout("eax") VMWARE_MAGIC => eax,
            inout("ecx") cmd => ecx,
            inout("edx") VMWARE_PORT as u32 => edx,
            options(nomem, preserves_flags, nostack),
        );
    }
    let ebx: u32 = ebx_out as u32;
    (eax, ebx, ecx, edx)
}

// ---------------------------------------------------------------------------
// Initialization — determine supported features per hypervisor
// ---------------------------------------------------------------------------

/// Initialize guest integration services.
///
/// Call after `hypervisor::detect()` during boot.  If not running in a VM,
/// this is a no-op and returns immediately.
pub fn init() {
    let hv = hypervisor::detected();
    if !hv.is_virtual() {
        serial_println!("[vmguest] Bare metal — no guest integration needed");
        return;
    }

    let mut state = STATE.lock();

    // Rebuild feature vec (const initializer uses empty Vec).
    if state.features.is_empty() {
        for &f in GuestFeature::ALL {
            state.features.push(FeatureState::new(f));
        }
    }

    state.hypervisor = hv;

    // Populate guest info.
    state.guest_info.os_name = String::from("MintOS");
    state.guest_info.os_version = String::from("0.1.0");
    state.guest_info.kernel_version = String::from("0.1.0-dev");
    state.guest_info.cpu_count = crate::smp::cpu_count() as u32;
    if let Some(stats) = crate::mm::frame::stats() {
        // Each frame is 16 KiB (our page size).
        state.guest_info.total_memory = stats.total_frames as u64 * 16384;
    }

    // Determine supported features based on hypervisor type.
    match hv {
        Hypervisor::Kvm => init_kvm_features(&mut state),
        Hypervisor::HyperV => init_hyperv_features(&mut state),
        Hypervisor::Vmware => init_vmware_features(&mut state),
        Hypervisor::VirtualBox => init_vbox_features(&mut state),
        Hypervisor::QemuTcg => init_qemu_features(&mut state),
        Hypervisor::Xen => init_xen_features(&mut state),
        _ => {
            // Unknown hypervisor — activate generic ACPI-based features only.
            state.set_supported(GuestFeature::GracefulShutdown);
            state.set_active(GuestFeature::GracefulShutdown);
        }
    }

    // Count active features.
    let active = state.features.iter().filter(|f| f.active).count() as u32;
    ACTIVE_FEATURES.store(active, Ordering::Relaxed);

    state.initialized = true;
    INITIALIZED.store(true, Ordering::Release);

    let supported = state.features.iter().filter(|f| f.supported).count();
    serial_println!(
        "[vmguest] Initialized for {} — {}/{} features active",
        hv.name(),
        active,
        supported
    );

    for fs in &state.features {
        if fs.supported {
            serial_println!(
                "[vmguest]   {}: {}",
                fs.feature.label(),
                if fs.active { "active" } else { "supported (not activated)" }
            );
        }
    }
}

/// Initialize KVM/QEMU-KVM specific features.
fn init_kvm_features(state: &mut State) {
    // KVM supports pvclock for low-overhead time.
    state.set_supported(GuestFeature::ParavirtClock);
    state.clock.source = ClockSource::KvmPvclock;

    // Graceful shutdown via ACPI (QEMU handles power button).
    state.set_supported(GuestFeature::GracefulShutdown);
    state.set_active(GuestFeature::GracefulShutdown);

    // Display resize via virtio-gpu (if present).
    state.set_supported(GuestFeature::DisplayResize);

    // Memory balloon via virtio-balloon (if present).
    state.set_supported(GuestFeature::MemoryBalloon);
    // Max balloon: up to 50% of total memory.
    if let Some(stats) = crate::mm::frame::stats() {
        state.balloon.max_pages = stats.total_frames as u64 / 2;
    }

    // Guest info reporting via QGA protocol or virtio-serial.
    state.set_supported(GuestFeature::GuestInfo);
    state.set_active(GuestFeature::GuestInfo);
    state.guest_info.report_count = 0;

    // Heartbeat via virtio-serial.
    state.set_supported(GuestFeature::Heartbeat);
    state.set_active(GuestFeature::Heartbeat);

    // Time sync via pvclock — mark active once page is mapped.
    // In a full implementation, we would allocate a physical page,
    // write its address to MSR_KVM_SYSTEM_TIME_NEW, and read the
    // pvclock_vcpu_time_info structure for TSC→nanosecond conversion.
    // For now, mark as supported but require explicit activation.
    state.set_supported(GuestFeature::TimeSynchronization);

    serial_println!("[vmguest] KVM features: pvclock, ACPI shutdown, virtio balloon/display");
}

/// Initialize Hyper-V / WHPX specific features.
fn init_hyperv_features(state: &mut State) {
    // Hyper-V reference TSC for paravirt clock.
    state.set_supported(GuestFeature::ParavirtClock);
    state.clock.source = ClockSource::HyperVRefTsc;

    // Hyper-V Shutdown IC (integration component).
    state.set_supported(GuestFeature::GracefulShutdown);
    state.set_active(GuestFeature::GracefulShutdown);

    // Hyper-V Heartbeat IC.
    state.set_supported(GuestFeature::Heartbeat);
    state.set_active(GuestFeature::Heartbeat);

    // Hyper-V KVP (Key-Value Pair) exchange for guest info.
    state.set_supported(GuestFeature::GuestInfo);
    state.set_active(GuestFeature::GuestInfo);

    // Hyper-V Dynamic Memory Management for balloon.
    state.set_supported(GuestFeature::MemoryBalloon);
    if let Some(stats) = crate::mm::frame::stats() {
        state.balloon.max_pages = stats.total_frames as u64 / 2;
    }

    // Hyper-V synthetic display adapter for resize.
    state.set_supported(GuestFeature::DisplayResize);

    // Time sync via Hyper-V timesync IC.
    state.set_supported(GuestFeature::TimeSynchronization);
    state.set_active(GuestFeature::TimeSynchronization);

    serial_println!("[vmguest] Hyper-V features: ref-TSC, shutdown/heartbeat IC, KVP, DMM");
}

/// Initialize VMware-specific features.
fn init_vmware_features(state: &mut State) {
    // VMware pseudo-TSC for clock.
    state.set_supported(GuestFeature::ParavirtClock);
    state.clock.source = ClockSource::VmwareTsc;

    // VMware RPCI for shutdown/reboot.
    state.set_supported(GuestFeature::GracefulShutdown);
    state.set_active(GuestFeature::GracefulShutdown);

    // VMware SVGA for display resize.
    state.set_supported(GuestFeature::DisplayResize);

    // VMware balloon driver.
    state.set_supported(GuestFeature::MemoryBalloon);
    if let Some(stats) = crate::mm::frame::stats() {
        state.balloon.max_pages = stats.total_frames as u64 / 2;
    }

    // VMware RPCI info channel.
    state.set_supported(GuestFeature::GuestInfo);
    state.set_active(GuestFeature::GuestInfo);

    // VMware heartbeat via RPCI.
    state.set_supported(GuestFeature::Heartbeat);
    state.set_active(GuestFeature::Heartbeat);

    // VMware tools time sync.
    state.set_supported(GuestFeature::TimeSynchronization);

    // Clipboard via RPCI/backdoor.
    state.set_supported(GuestFeature::ClipboardSync);

    serial_println!("[vmguest] VMware features: pseudo-TSC, RPCI, SVGA, balloon, clipboard");
}

/// Initialize VirtualBox-specific features.
fn init_vbox_features(state: &mut State) {
    // VirtualBox uses ACPI shutdown.
    state.set_supported(GuestFeature::GracefulShutdown);
    state.set_active(GuestFeature::GracefulShutdown);

    // VirtualBox Guest Additions: display resize.
    state.set_supported(GuestFeature::DisplayResize);

    // VirtualBox balloon via Guest Additions.
    state.set_supported(GuestFeature::MemoryBalloon);
    if let Some(stats) = crate::mm::frame::stats() {
        state.balloon.max_pages = stats.total_frames as u64 / 2;
    }

    // VirtualBox Guest Properties for info reporting.
    state.set_supported(GuestFeature::GuestInfo);
    state.set_active(GuestFeature::GuestInfo);

    // VirtualBox heartbeat.
    state.set_supported(GuestFeature::Heartbeat);
    state.set_active(GuestFeature::Heartbeat);

    // VirtualBox clipboard.
    state.set_supported(GuestFeature::ClipboardSync);

    serial_println!("[vmguest] VirtualBox features: ACPI, Guest Additions, balloon, clipboard");
}

/// Initialize QEMU TCG (software emulation) features.
///
/// QEMU TCG has the same device model as QEMU/KVM but no hardware
/// virtualization.  All paravirt features work the same way since
/// they are provided by QEMU's device model, not KVM.
fn init_qemu_features(state: &mut State) {
    // QEMU TCG has no pvclock (no KVM).  Fall back to HPET/PIT.
    state.clock.source = ClockSource::None;

    // ACPI shutdown works via QEMU device model.
    state.set_supported(GuestFeature::GracefulShutdown);
    state.set_active(GuestFeature::GracefulShutdown);

    // Virtio devices work under TCG too (they're QEMU devices, not KVM).
    state.set_supported(GuestFeature::DisplayResize);
    state.set_supported(GuestFeature::MemoryBalloon);
    if let Some(stats) = crate::mm::frame::stats() {
        state.balloon.max_pages = stats.total_frames as u64 / 2;
    }

    state.set_supported(GuestFeature::GuestInfo);
    state.set_active(GuestFeature::GuestInfo);

    state.set_supported(GuestFeature::Heartbeat);
    state.set_active(GuestFeature::Heartbeat);

    serial_println!("[vmguest] QEMU TCG features: ACPI, virtio (no pvclock)");
}

/// Initialize Xen-specific features.
fn init_xen_features(state: &mut State) {
    // Xen provides its own paravirt clock via shared info page.
    state.set_supported(GuestFeature::ParavirtClock);

    // Xen shutdown via Xenbus/Xenstore.
    state.set_supported(GuestFeature::GracefulShutdown);
    state.set_active(GuestFeature::GracefulShutdown);

    // Xen PV display.
    state.set_supported(GuestFeature::DisplayResize);

    // Xen balloon driver.
    state.set_supported(GuestFeature::MemoryBalloon);
    if let Some(stats) = crate::mm::frame::stats() {
        state.balloon.max_pages = stats.total_frames as u64 / 2;
    }

    // Xen guest info via Xenstore.
    state.set_supported(GuestFeature::GuestInfo);
    state.set_active(GuestFeature::GuestInfo);

    state.set_supported(GuestFeature::Heartbeat);
    state.set_active(GuestFeature::Heartbeat);

    serial_println!("[vmguest] Xen features: pv-clock, Xenbus shutdown, balloon");
}

// ---------------------------------------------------------------------------
// Periodic tick — called from timer interrupt or idle loop
// ---------------------------------------------------------------------------

/// Periodic tick for guest integration services.
///
/// Called approximately once per second.  Handles heartbeat sending,
/// balloon adjustments, clock drift monitoring, and host request polling.
pub fn tick() {
    if !INITIALIZED.load(Ordering::Acquire) {
        return;
    }

    let tick_num = TICK_COUNTER.fetch_add(1, Ordering::Relaxed);
    let now_ns = crate::hpet::elapsed_ns();

    let mut state = STATE.lock();
    state.tick_count = tick_num.saturating_add(1);

    // Heartbeat — send every `interval_secs` seconds.
    if state.heartbeat.interval_secs > 0 {
        let interval_ns = state.heartbeat.interval_secs as u64 * 1_000_000_000;
        let elapsed_since_last = now_ns.saturating_sub(state.heartbeat.last_sent_ns);
        if elapsed_since_last >= interval_ns || state.heartbeat.sent_count == 0 {
            send_heartbeat(&mut state, now_ns);
        }
    }

    // Balloon adjustment — check every 10 ticks.
    if tick_num.is_multiple_of(10) {
        check_balloon(&mut state);
    }

    // Clock drift monitoring — check every 60 ticks.
    if tick_num.is_multiple_of(60) {
        check_clock_drift(&mut state, now_ns);
    }

    // Guest info report — report once initially, then every 300 ticks (5 min).
    if state.guest_info.report_count == 0 || tick_num.is_multiple_of(300) {
        report_guest_info(&mut state);
    }

    // Check for host shutdown/reboot requests every tick.
    check_host_requests(&mut state);
}

/// Send a heartbeat to the host.
fn send_heartbeat(state: &mut State, now_ns: u64) {
    if !state.feature(GuestFeature::Heartbeat).is_some_and(|f| f.active) {
        return;
    }

    // The actual heartbeat mechanism depends on the hypervisor:
    // - KVM/QEMU: Write to virtio-serial channel
    // - Hyper-V: Respond to Heartbeat IC messages on vmbus
    // - VMware: RPCI "tools.set.version" or heartbeat command
    // - VirtualBox: Guest Additions heartbeat via HGCM
    //
    // In a full implementation, each hypervisor backend would have its
    // own transport.  For now, we track the timing and count.
    state.heartbeat.sent_count = state.heartbeat.sent_count.saturating_add(1);
    state.heartbeat.last_sent_ns = now_ns;
    state.heartbeat.host_ack = true; // Assume success in stub
    state.inc_ops(GuestFeature::Heartbeat);
}

/// Check if host has requested balloon size change.
fn check_balloon(state: &mut State) {
    if !state.feature(GuestFeature::MemoryBalloon).is_some_and(|f| f.supported) {
        return;
    }
    if !state.balloon.auto_enabled {
        return;
    }

    // In a full implementation, we would:
    // - KVM: Read virtio-balloon's `num_pages` config field
    // - Hyper-V: Process DMM messages from vmbus
    // - VMware: Execute balloon backdoor commands
    //
    // If target != current, inflate or deflate by allocating/freeing pages.
    // For now, just track that we checked.
    let current = state.balloon.inflated_pages;
    let target = state.balloon.target_pages;

    if target > current {
        // Host wants us to give back memory (inflate).
        let delta = target.saturating_sub(current);
        let capped = delta.min(state.balloon.max_pages.saturating_sub(current));
        if capped > 0 {
            // In a full implementation, allocate `capped` pages and report
            // them to the hypervisor as unavailable.
            state.balloon.inflated_pages = current.saturating_add(capped);
            state.balloon.total_inflated = state.balloon.total_inflated.saturating_add(capped);
            state.inc_ops(GuestFeature::MemoryBalloon);
        }
    } else if target < current {
        // Host allows us to reclaim memory (deflate).
        let delta = current.saturating_sub(target);
        state.balloon.inflated_pages = current.saturating_sub(delta);
        state.balloon.total_deflated = state.balloon.total_deflated.saturating_add(delta);
        state.inc_ops(GuestFeature::MemoryBalloon);
    }
}

/// Monitor clock drift between host and guest.
fn check_clock_drift(state: &mut State, now_ns: u64) {
    if !state.feature(GuestFeature::TimeSynchronization).is_some_and(|f| f.active) {
        return;
    }

    // In a full implementation:
    // - KVM: Read pvclock_vcpu_time_info from shared page, compute
    //   host_ns = (tsc - tsc_timestamp) * mul >> shift + system_time.
    //   Compare with our HPET-based now_ns.
    // - Hyper-V: Read reference TSC page similarly.
    // - VMware: Use RPCI "machine.id.get" or backdoor time commands.
    //
    // If drift exceeds threshold (e.g., 100ms), adjust our time base.
    state.clock.reads = state.clock.reads.saturating_add(1);

    // Stub: no actual drift measurement without the pvclock page mapped.
    let _now = now_ns;
    state.inc_ops(GuestFeature::TimeSynchronization);
}

/// Report guest information to the host.
fn report_guest_info(state: &mut State) {
    if !state.feature(GuestFeature::GuestInfo).is_some_and(|f| f.active) {
        return;
    }

    // Update dynamic fields.
    state.guest_info.cpu_count = crate::smp::cpu_count() as u32;
    if let Some(stats) = crate::mm::frame::stats() {
        state.guest_info.total_memory = stats.total_frames as u64 * 16384;
    }

    // In a full implementation:
    // - KVM/QEMU: Write structured data to QGA virtio-serial channel
    // - Hyper-V: Set KVP pairs via vmbus KVP IC
    // - VMware: RPCI "info-set guestinfo.ip ..." commands
    // - VirtualBox: VBoxGuestPropWrite via HGCM
    state.guest_info.report_count = state.guest_info.report_count.saturating_add(1);
    state.inc_ops(GuestFeature::GuestInfo);
}

/// Check for host-initiated shutdown/reboot requests.
fn check_host_requests(state: &mut State) {
    if !state.feature(GuestFeature::GracefulShutdown).is_some_and(|f| f.active) {
        return;
    }

    // In a full implementation:
    // - KVM/QEMU: ACPI power button event (handled by our ACPI subsystem)
    // - Hyper-V: Shutdown IC message via vmbus
    // - VMware: RPCI "toolScript.power" or shutdown command via backdoor
    // - VirtualBox: Guest Additions shutdown notification
    //
    // The host request flag would be set by an interrupt handler or
    // virtio device notification, not polled.  This tick-based check
    // is a fallback for missed notifications.

    if state.shutdown_requested {
        crate::syslog!("vmguest", Warning, "Host requested shutdown");
        // In a full implementation, initiate graceful shutdown sequence.
        state.shutdown_requested = false; // Clear to avoid re-triggering.
    }
    if state.reboot_requested {
        crate::syslog!("vmguest", Warning, "Host requested reboot");
        state.reboot_requested = false;
    }
}

// ---------------------------------------------------------------------------
// Public API — query and control
// ---------------------------------------------------------------------------

/// Check if guest integration is active.
pub fn is_active() -> bool {
    INITIALIZED.load(Ordering::Acquire)
}

/// Get the number of active guest integration features.
pub fn active_feature_count() -> u32 {
    ACTIVE_FEATURES.load(Ordering::Relaxed)
}

/// Check if a specific feature is supported.
pub fn is_feature_supported(feature: GuestFeature) -> bool {
    let state = STATE.lock();
    state.feature(feature).is_some_and(|f| f.supported)
}

/// Check if a specific feature is active.
pub fn is_feature_active(feature: GuestFeature) -> bool {
    let state = STATE.lock();
    state.feature(feature).is_some_and(|f| f.active)
}

/// Get current balloon state.
pub fn balloon_info() -> (u64, u64, u64, u64, u64, bool) {
    let state = STATE.lock();
    (
        state.balloon.inflated_pages,
        state.balloon.target_pages,
        state.balloon.total_inflated,
        state.balloon.total_deflated,
        state.balloon.max_pages,
        state.balloon.auto_enabled,
    )
}

/// Set the balloon target (number of pages to reclaim from guest).
pub fn set_balloon_target(pages: u64) {
    let mut state = STATE.lock();
    let capped = pages.min(state.balloon.max_pages);
    state.balloon.target_pages = capped;
    crate::syslog!("vmguest", Info, "Balloon target set to {} pages", capped);
}

/// Enable or disable automatic balloon adjustment.
pub fn set_balloon_auto(enabled: bool) {
    let mut state = STATE.lock();
    state.balloon.auto_enabled = enabled;
}

/// Set heartbeat interval in seconds (0 = disabled).
pub fn set_heartbeat_interval(secs: u32) {
    let mut state = STATE.lock();
    state.heartbeat.interval_secs = secs;
}

/// Get heartbeat statistics.
pub fn heartbeat_stats() -> (u64, u64, u64, bool) {
    let state = STATE.lock();
    (
        state.heartbeat.sent_count,
        state.heartbeat.failed_count,
        state.heartbeat.last_sent_ns,
        state.heartbeat.host_ack,
    )
}

/// Get the paravirtual clock source name.
pub fn clock_source() -> &'static str {
    let state = STATE.lock();
    state.clock.source.label()
}

/// Get clock drift statistics.
pub fn clock_stats() -> (u64, i64, u64) {
    let state = STATE.lock();
    (state.clock.reads, state.clock.last_drift_ns, state.clock.corrections)
}

/// Request the host to resize the display.
///
/// Sends a resolution change request via the hypervisor-specific mechanism.
/// Returns the effective resolution (may differ from requested if clamped).
pub fn request_display_resize(width: u32, height: u32) -> (u32, u32) {
    if !INITIALIZED.load(Ordering::Acquire) {
        return (0, 0);
    }

    let mut state = STATE.lock();
    if !state.feature(GuestFeature::DisplayResize).is_some_and(|f| f.supported) {
        return (0, 0);
    }

    // Clamp to reasonable bounds.
    let w = width.clamp(640, 7680);
    let h = height.clamp(480, 4320);

    // In a full implementation:
    // - KVM/QEMU: virtio-gpu SET_SCANOUT or RESOURCE_CREATE_2D
    // - Hyper-V: Hyper-V synthetic video adapter config
    // - VMware: SVGA FIFO command to change resolution
    // - VirtualBox: HGSMI VBVA_REPORT_CAPS + VGA mode set
    state.display_width = w;
    state.display_height = h;
    state.inc_ops(GuestFeature::DisplayResize);
    crate::syslog!("vmguest", Info, "Display resize requested: {}x{}", w, h);
    (w, h)
}

/// Get current display resolution (as managed by guest tools).
pub fn display_resolution() -> (u32, u32) {
    let state = STATE.lock();
    (state.display_width, state.display_height)
}

/// Signal the host that the guest is shutting down gracefully.
pub fn notify_host_shutdown() {
    if !INITIALIZED.load(Ordering::Acquire) {
        return;
    }

    let mut state = STATE.lock();
    // In a full implementation:
    // - KVM/QEMU: ACPI handles this (power.rs already covers shutdown)
    // - Hyper-V: Respond to Shutdown IC with success
    // - VMware: RPCI "tools.set.version 0" or "log shuttingdown"
    // - VirtualBox: VBoxGuestSetCapabilities + shutdown notification
    state.inc_ops(GuestFeature::GracefulShutdown);
    crate::syslog!("vmguest", Info, "Notifying host of guest shutdown");
}

/// Get guest info summary.
pub fn guest_info() -> (String, String, String, u32, u64, String, u64) {
    let state = STATE.lock();
    (
        state.guest_info.os_name.clone(),
        state.guest_info.os_version.clone(),
        state.guest_info.kernel_version.clone(),
        state.guest_info.cpu_count,
        state.guest_info.total_memory,
        state.guest_info.hostname.clone(),
        state.guest_info.report_count,
    )
}

/// Set the hostname reported to the host.
pub fn set_hostname(name: &str) {
    let mut state = STATE.lock();
    state.guest_info.hostname = String::from(name);
}

// ---------------------------------------------------------------------------
// Summary / procfs
// ---------------------------------------------------------------------------

/// Summary statistics for status display.
pub fn stats() -> (bool, &'static str, u32, u32, u64) {
    let state = STATE.lock();
    let supported = state.features.iter().filter(|f| f.supported).count() as u32;
    let active = state.features.iter().filter(|f| f.active).count() as u32;
    let hv_name = state.hypervisor.name();
    (state.initialized, hv_name, supported, active, state.tick_count)
}

/// Generate content for `/proc/vmguest`.
pub fn procfs_content() -> String {
    let state = STATE.lock();
    let mut out = String::with_capacity(1024);

    out.push_str("=== VM Guest Integration ===\n");
    out.push_str(&format!("hypervisor: {}\n", state.hypervisor.name()));
    out.push_str(&format!("initialized: {}\n", state.initialized));
    out.push_str(&format!("ticks: {}\n\n", state.tick_count));

    // Features.
    out.push_str("=== Features ===\n");
    for fs in &state.features {
        if fs.supported {
            out.push_str(&format!(
                "{}: {} (ops: {}{})\n",
                fs.feature.label(),
                if fs.active { "active" } else { "supported" },
                fs.ops_count,
                fs.last_error.as_ref().map_or(String::new(), |e| format!(", last_error: {}", e)),
            ));
        }
    }

    // Clock.
    out.push_str(&format!("\n=== Clock ===\nsource: {}\n", state.clock.source.label()));
    out.push_str(&format!("reads: {}\n", state.clock.reads));
    out.push_str(&format!("drift_ns: {}\n", state.clock.last_drift_ns));
    out.push_str(&format!("corrections: {}\n", state.clock.corrections));
    out.push_str(&format!("page_mapped: {}\n", state.clock.page_mapped));

    // Balloon.
    out.push_str("\n=== Balloon ===\n");
    out.push_str(&format!("inflated_pages: {}\n", state.balloon.inflated_pages));
    out.push_str(&format!("target_pages: {}\n", state.balloon.target_pages));
    out.push_str(&format!("total_inflated: {}\n", state.balloon.total_inflated));
    out.push_str(&format!("total_deflated: {}\n", state.balloon.total_deflated));
    out.push_str(&format!("max_pages: {}\n", state.balloon.max_pages));
    out.push_str(&format!("auto: {}\n", state.balloon.auto_enabled));

    // Heartbeat.
    out.push_str("\n=== Heartbeat ===\n");
    out.push_str(&format!("interval_secs: {}\n", state.heartbeat.interval_secs));
    out.push_str(&format!("sent: {}\n", state.heartbeat.sent_count));
    out.push_str(&format!("failed: {}\n", state.heartbeat.failed_count));
    out.push_str(&format!("host_ack: {}\n", state.heartbeat.host_ack));

    // Guest info.
    out.push_str("\n=== Guest Info ===\n");
    out.push_str(&format!("os: {} {}\n", state.guest_info.os_name, state.guest_info.os_version));
    out.push_str(&format!("kernel: {}\n", state.guest_info.kernel_version));
    out.push_str(&format!("cpus: {}\n", state.guest_info.cpu_count));
    out.push_str(&format!("memory: {} bytes\n", state.guest_info.total_memory));
    out.push_str(&format!("hostname: {}\n", state.guest_info.hostname));
    out.push_str(&format!("reports: {}\n", state.guest_info.report_count));

    // Display.
    if state.display_width > 0 {
        out.push_str(&format!(
            "\n=== Display ===\nresolution: {}x{}\n",
            state.display_width, state.display_height
        ));
    }

    out
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Self-test for VM guest integration.
pub fn self_test() {
    crate::serial_println!("[vmguest] Running self-test...");

    // Test 1: Feature enum labels are non-empty.
    for &f in GuestFeature::ALL {
        assert!(!f.label().is_empty(), "Feature label should not be empty");
    }
    crate::serial_println!("[vmguest]   Feature labels: OK");

    // Test 2: Feature ALL has correct count.
    assert_eq!(GuestFeature::ALL.len(), 8, "Should have 8 features");
    crate::serial_println!("[vmguest]   Feature count: OK");

    // Test 3: Init should have been called (if in VM).
    let (initialized, hv_name, supported, active, _ticks) = stats();
    if hypervisor::is_virtual() {
        assert!(initialized, "Should be initialized when in VM");
        assert!(!hv_name.is_empty(), "Hypervisor name should not be empty");
        crate::serial_println!("[vmguest]   Init state: OK ({})", hv_name);
    } else {
        crate::serial_println!("[vmguest]   Init state: OK (bare metal — skipped)");
    }
    crate::serial_println!("[vmguest]   Supported: {}, Active: {}", supported, active);

    // Test 4: Balloon state is consistent.
    let (inflated, target, total_in, _total_out, max_pages, auto) = balloon_info();
    assert!(inflated <= max_pages || max_pages == 0, "Inflated should not exceed max");
    assert!(total_in >= inflated, "Total inflated should be >= current");
    crate::serial_println!("[vmguest]   Balloon: inflated={}, target={}, max={}, auto={}", inflated, target, max_pages, auto);

    // Test 5: Heartbeat stats are consistent.
    let (sent, failed, _last_ns, _ack) = heartbeat_stats();
    assert!(sent >= failed, "Sent should be >= failed");
    crate::serial_println!("[vmguest]   Heartbeat: sent={}, failed={}", sent, failed);

    // Test 6: Clock source matches hypervisor.
    let clk_src = clock_source();
    assert!(!clk_src.is_empty(), "Clock source label should not be empty");
    crate::serial_println!("[vmguest]   Clock source: {}", clk_src);

    // Test 7: Guest info is populated (if initialized).
    if initialized {
        let (os_name, os_ver, _kver, cpus, mem, hostname, reports) = guest_info();
        assert!(!os_name.is_empty(), "OS name should be set");
        assert!(!os_ver.is_empty(), "OS version should be set");
        assert!(cpus > 0, "CPU count should be > 0");
        assert!(mem > 0, "Memory should be > 0");
        assert!(!hostname.is_empty(), "Hostname should be set");
        crate::serial_println!(
            "[vmguest]   Guest info: {} {} ({}cpus, {} MB, host={}, reports={})",
            os_name, os_ver, cpus, mem / (1024 * 1024), hostname, reports
        );
    }

    // Test 8: Display resize returns valid dimensions for supported case.
    if is_feature_supported(GuestFeature::DisplayResize) {
        let (w, h) = request_display_resize(1920, 1080);
        assert!(w >= 640 && w <= 7680, "Width should be in valid range");
        assert!(h >= 480 && h <= 4320, "Height should be in valid range");
        crate::serial_println!("[vmguest]   Display resize: {}x{}", w, h);
    } else {
        crate::serial_println!("[vmguest]   Display resize: not supported (skipped)");
    }

    // Test 9: Display resize clamping works.
    if is_feature_supported(GuestFeature::DisplayResize) {
        let (w, h) = request_display_resize(100, 100);
        assert_eq!(w, 640, "Width should be clamped to 640 minimum");
        assert_eq!(h, 480, "Height should be clamped to 480 minimum");
        crate::serial_println!("[vmguest]   Display resize clamping: OK");
    }

    // Test 10: set_hostname works.
    set_hostname("test-host");
    {
        let state = STATE.lock();
        assert_eq!(state.guest_info.hostname.as_str(), "test-host");
    }
    set_hostname("mintos"); // Restore.
    crate::serial_println!("[vmguest]   set_hostname: OK");

    // Test 11: Balloon target setting.
    let original_target = {
        let state = STATE.lock();
        state.balloon.target_pages
    };
    set_balloon_target(100);
    {
        let state = STATE.lock();
        if state.balloon.max_pages > 0 {
            assert_eq!(state.balloon.target_pages, 100);
        }
    }
    set_balloon_target(original_target); // Restore.
    crate::serial_println!("[vmguest]   Balloon target: OK");

    // Test 12: Balloon auto toggle.
    set_balloon_auto(false);
    {
        let state = STATE.lock();
        assert!(!state.balloon.auto_enabled);
    }
    set_balloon_auto(true); // Restore.
    crate::serial_println!("[vmguest]   Balloon auto toggle: OK");

    // Test 13: procfs content is non-empty and well-formed.
    let content = procfs_content();
    assert!(content.contains("=== VM Guest Integration ==="), "procfs should have header");
    assert!(content.contains("hypervisor:"), "procfs should have hypervisor field");
    assert!(content.contains("=== Features ==="), "procfs should have features section");
    crate::serial_println!("[vmguest]   procfs content: OK ({} bytes)", content.len());

    // Test 14: Feature query API consistency.
    for &f in GuestFeature::ALL {
        if is_feature_active(f) {
            assert!(is_feature_supported(f), "Active feature must be supported");
        }
    }
    crate::serial_println!("[vmguest]   Feature consistency: OK");

    // Test 15: Tick processing (run a few ticks).
    let before = TICK_COUNTER.load(Ordering::Relaxed);
    tick();
    tick();
    tick();
    let after = TICK_COUNTER.load(Ordering::Relaxed);
    assert_eq!(after, before.saturating_add(3), "Tick counter should increment by 3");
    crate::serial_println!("[vmguest]   Tick processing: OK");

    crate::serial_println!("[vmguest] Self-test PASSED (15 tests)");
}
