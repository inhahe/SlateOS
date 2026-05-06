//! Hypervisor detection — identify the virtualization environment.
//!
//! Uses CPUID to detect whether the kernel is running inside a virtual
//! machine, and if so, identifies the hypervisor.  This information is
//! useful for:
//!
//! - **Timing**: TSC behavior differs under virtualization (may not be
//!   invariant, may be scaled).
//! - **Drivers**: paravirtual drivers (virtio) should be preferred over
//!   emulated hardware when running under KVM/QEMU.
//! - **Performance**: some optimizations (e.g., INVPCID, PCID) may be
//!   partially emulated or not available under certain hypervisors.
//! - **Debugging**: knowing the environment helps interpret timing
//!   anomalies and jitter measurements.
//!
//! ## Detection Method
//!
//! 1. Check CPUID leaf 1, ECX bit 31 (hypervisor present bit).
//!    This bit is set by all modern hypervisors.
//! 2. If set, read CPUID leaf 0x40000000 for the hypervisor signature
//!    string (12-byte ASCII in EBX, ECX, EDX).
//! 3. Match the signature against known hypervisors.
//!
//! ## Known Signatures
//!
//! | Signature        | Hypervisor        |
//! |------------------|-------------------|
//! | "KVMKVMKVM\0\0\0" | KVM (Linux)      |
//! | "Microsoft Hv"   | Hyper-V / WHPX    |
//! | "VMwareVMware"   | VMware            |
//! | "VBoxVBoxVBox"   | VirtualBox        |
//! | "XenVMMXenVMM"   | Xen               |
//! | "TCGTCGTCGTCG"   | QEMU TCG          |
//! | "bhyve bhyve "   | bhyve (FreeBSD)   |
//!
//! ## References
//!
//! - Intel SDM Vol. 2A: CPUID — hypervisor present bit (ECX[31])
//! - Linux `arch/x86/kernel/cpu/hypervisor.c`
//! - Microsoft TLFS: Hypervisor CPUID Interface

use core::sync::atomic::{AtomicU8, Ordering};
use crate::serial_println;

// ---------------------------------------------------------------------------
// Hypervisor identification
// ---------------------------------------------------------------------------

/// Detected hypervisor type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Hypervisor {
    /// No hypervisor detected (bare metal).
    None = 0,
    /// KVM (Linux kernel-based VM).
    Kvm = 1,
    /// Microsoft Hyper-V or WHPX acceleration.
    HyperV = 2,
    /// VMware (Workstation, Fusion, ESXi).
    Vmware = 3,
    /// Oracle VirtualBox.
    VirtualBox = 4,
    /// Xen hypervisor.
    Xen = 5,
    /// QEMU Tiny Code Generator (software emulation, no hardware accel).
    QemuTcg = 6,
    /// bhyve (FreeBSD hypervisor).
    Bhyve = 7,
    /// Unknown hypervisor (bit 31 set but signature not recognized).
    Unknown = 255,
}

impl Hypervisor {
    /// Human-readable name.
    pub const fn name(self) -> &'static str {
        match self {
            Self::None => "bare metal",
            Self::Kvm => "KVM",
            Self::HyperV => "Hyper-V/WHPX",
            Self::Vmware => "VMware",
            Self::VirtualBox => "VirtualBox",
            Self::Xen => "Xen",
            Self::QemuTcg => "QEMU TCG",
            Self::Bhyve => "bhyve",
            Self::Unknown => "unknown hypervisor",
        }
    }

    /// Whether the kernel is running virtualized.
    pub fn is_virtual(self) -> bool {
        self != Self::None
    }

    /// Convert from raw u8 (for atomic loading).
    fn from_u8(v: u8) -> Self {
        match v {
            0 => Self::None,
            1 => Self::Kvm,
            2 => Self::HyperV,
            3 => Self::Vmware,
            4 => Self::VirtualBox,
            5 => Self::Xen,
            6 => Self::QemuTcg,
            7 => Self::Bhyve,
            _ => Self::Unknown,
        }
    }
}

// ---------------------------------------------------------------------------
// Detection result cache
// ---------------------------------------------------------------------------

/// Cached detection result (initialized once at boot).
static DETECTED: AtomicU8 = AtomicU8::new(0);

/// Whether detection has been performed.
static DETECTED_VALID: AtomicU8 = AtomicU8::new(0);

/// The raw signature string (12 bytes).
static mut SIGNATURE: [u8; 12] = [0u8; 12];

// ---------------------------------------------------------------------------
// CPUID helpers
// ---------------------------------------------------------------------------

/// Execute CPUID with the given leaf.
/// Returns (eax, ebx, ecx, edx).
#[inline]
fn cpuid(leaf: u32) -> (u32, u32, u32, u32) {
    let eax: u32;
    let ebx: u32;
    let ecx: u32;
    let edx: u32;

    // SAFETY: CPUID is always available on x86_64 and has no side effects
    // other than populating registers.  We save/restore rbx via xchg
    // because LLVM reserves it.
    unsafe {
        core::arch::asm!(
            "xchg rbx, {tmp}",
            "cpuid",
            "xchg rbx, {tmp}",
            tmp = out(reg) ebx,
            inout("eax") leaf => eax,
            out("ecx") ecx,
            out("edx") edx,
            options(nomem, preserves_flags),
        );
    }

    (eax, ebx, ecx, edx)
}

/// Execute CPUID leaf 1 and check hypervisor present bit.
fn is_hypervisor_present() -> bool {
    let (_eax, _ebx, ecx, _edx) = cpuid(1);
    // Bit 31 of ECX from CPUID.1: hypervisor present.
    (ecx & (1 << 31)) != 0
}

/// Read the hypervisor signature from CPUID leaf 0x40000000.
fn read_signature() -> [u8; 12] {
    let (_eax, ebx, ecx, edx) = cpuid(0x4000_0000);

    let mut sig = [0u8; 12];
    sig[0..4].copy_from_slice(&ebx.to_le_bytes());
    sig[4..8].copy_from_slice(&ecx.to_le_bytes());
    sig[8..12].copy_from_slice(&edx.to_le_bytes());
    sig
}

/// Match a 12-byte signature to a known hypervisor.
fn identify(sig: &[u8; 12]) -> Hypervisor {
    match sig {
        b"KVMKVMKVM\0\0\0" => Hypervisor::Kvm,
        b"Microsoft Hv" => Hypervisor::HyperV,
        b"VMwareVMware" => Hypervisor::Vmware,
        b"VBoxVBoxVBox" => Hypervisor::VirtualBox,
        b"XenVMMXenVMM" => Hypervisor::Xen,
        b"TCGTCGTCGTCG" => Hypervisor::QemuTcg,
        b"bhyve bhyve " => Hypervisor::Bhyve,
        _ => Hypervisor::Unknown,
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Detect the hypervisor (called once at boot).
///
/// Caches the result so subsequent calls to [`detected()`] are O(1).
pub fn detect() {
    let hv = if is_hypervisor_present() {
        let sig = read_signature();
        // SAFETY: Only called once from BSP during boot (single-threaded).
        unsafe {
            SIGNATURE = sig;
        }
        identify(&sig)
    } else {
        Hypervisor::None
    };

    DETECTED.store(hv as u8, Ordering::Release);
    DETECTED_VALID.store(1, Ordering::Release);

    if hv.is_virtual() {
        serial_println!("[hypervisor] Detected: {} (signature: {:?})",
            hv.name(), signature_str());
    } else {
        serial_println!("[hypervisor] Running on bare metal (no hypervisor detected)");
    }
}

/// Get the detected hypervisor.
///
/// Returns `Hypervisor::None` if [`detect()`] hasn't been called yet
/// or if running on bare metal.
pub fn detected() -> Hypervisor {
    if DETECTED_VALID.load(Ordering::Acquire) == 0 {
        return Hypervisor::None;
    }
    Hypervisor::from_u8(DETECTED.load(Ordering::Relaxed))
}

/// Whether the kernel is running inside a VM.
pub fn is_virtual() -> bool {
    detected().is_virtual()
}

/// Get the raw signature string (for display).
pub fn signature_str() -> &'static str {
    // SAFETY: SIGNATURE is only written once during detect() (single-threaded
    // BSP context) and then only read afterwards.  We use a raw pointer to
    // avoid creating a reference to a mutable static.
    let sig: &[u8; 12] = unsafe {
        &*core::ptr::addr_of!(SIGNATURE)
    };
    core::str::from_utf8(sig).unwrap_or("<invalid>")
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Self-test for hypervisor detection.
pub fn self_test() {
    serial_println!("[hypervisor] Running self-test...");

    // Test 1: Detection has been run (detect() called before self_test).
    assert_eq!(DETECTED_VALID.load(Ordering::Relaxed), 1,
        "detect() should have been called before self_test()");
    serial_println!("[hypervisor]   Detection completed: OK");

    // Test 2: Result is accessible.
    let hv = detected();
    serial_println!("[hypervisor]   Detected: {} (virtual={})",
        hv.name(), hv.is_virtual());

    // Test 3: Signature is valid ASCII (or zeros).
    let sig_str = signature_str();
    // Under QEMU with KVM or WHPX, we expect a non-empty signature.
    if hv.is_virtual() {
        assert!(!sig_str.trim_end_matches('\0').is_empty(),
            "virtual env should have a signature");
    }
    serial_println!("[hypervisor]   Signature: {:?}", sig_str);

    // Test 4: identify() correctly handles known signatures.
    assert_eq!(identify(b"KVMKVMKVM\0\0\0"), Hypervisor::Kvm);
    assert_eq!(identify(b"Microsoft Hv"), Hypervisor::HyperV);
    assert_eq!(identify(b"VMwareVMware"), Hypervisor::Vmware);
    assert_eq!(identify(b"VBoxVBoxVBox"), Hypervisor::VirtualBox);
    assert_eq!(identify(b"XenVMMXenVMM"), Hypervisor::Xen);
    assert_eq!(identify(b"TCGTCGTCGTCG"), Hypervisor::QemuTcg);
    assert_eq!(identify(b"\0\0\0\0\0\0\0\0\0\0\0\0"), Hypervisor::Unknown);
    serial_println!("[hypervisor]   Signature matching: OK");

    serial_println!("[hypervisor] Self-test PASSED");
}
