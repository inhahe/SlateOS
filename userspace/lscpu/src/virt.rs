//! `lscpu-virt.c`: whether this is a virtual machine or a container, and
//! whose.
//!
//! The order of the questions is upstream's, and it decides the answer: the
//! CPU's `svm`/`vmx` flag; WSL 1 by name; on a live system `cpuid`'s
//! hypervisor leaf, then the DMI tables; then a list of places each
//! hypervisor leaves a mark -- a PowerPC device tree, `/proc/xen`, a PCI
//! device only a hypervisor's virtual hardware has, s390's `/proc/sysinfo`,
//! OpenVZ's `/proc/vz`, a UML model name, a VServer `VxID`.
//!
//! Two of upstream's paths are dead, and are dead here too, for the reason
//! each is dead upstream:
//!
//! * **Xen's PV/PVH features are never read.** Upstream opens them with
//!   `ul_prefix_fopen(prefix, "r", path)`, whose arguments are path then
//!   mode -- so it asks for a file named `r` with a mode string starting
//!   `/`, which `fopen` refuses. A Xen guest is always `full`.
//! * **The VMware backdoor never answers.** Upstream's `vmware_bdoor` loads
//!   the command into `eax`, the magic number into `ebx` and the port into
//!   `ecx`, then executes `inl (%dx)` -- a read of I/O port 0, with `edx`
//!   0, where VMware listens on 0x5658 for a request with the magic number
//!   in `eax`. Port 0 without I/O privilege (which `lscpu` never asks for)
//!   is a protection fault, which upstream catches and takes for "not
//!   VMware"; for anyone but root it does not even try.

use crate::cstr;
use crate::dmi;
use crate::path::BUFSIZ;
use crate::sys;
use crate::types::{
    Cxt, VIRT_TYPE_CONTAINER, VIRT_TYPE_FULL, VIRT_TYPE_NONE, VIRT_TYPE_PARA, VIRT_VENDOR_IBM,
    VIRT_VENDOR_KVM, VIRT_VENDOR_MSHV, VIRT_VENDOR_NONE, VIRT_VENDOR_OS400, VIRT_VENDOR_PARALLELS,
    VIRT_VENDOR_PHYP, VIRT_VENDOR_SPAR, VIRT_VENDOR_UML, VIRT_VENDOR_VBOX, VIRT_VENDOR_VMWARE,
    VIRT_VENDOR_VSERVER, VIRT_VENDOR_WSL, VIRT_VENDOR_XEN, Virt,
};
use std::io::{BufReader, Read};

/// `hv_vendor_pci[]` and `hv_graphics_pci[]`: the PCI vendor and device of
/// a hypervisor's virtual graphics card.
const XEN_PCI: (u32, u32) = (0x5853, 0x0001);
const VMWARE_PCI: (u32, u32) = (0x15ad, 0x0710);
const VBOX_PCI: (u32, u32) = (0x80ee, 0xbeef);

/// `read_hypervisor_cpuid()`.
fn hypervisor_cpuid() -> usize {
    let Some(id) = sys::hypervisor_signature() else {
        return VIRT_VENDOR_NONE;
    };
    if id[0] == 0 {
        return VIRT_VENDOR_NONE;
    }
    // `strncmp` of the NUL-terminated signature.
    let sig = cstr::c_str(&id);
    let is = |name: &[u8]| sig.get(..name.len()) == Some(name) || (sig == name);
    if is(b"XenVMMXenVMM") {
        VIRT_VENDOR_XEN
    } else if is(b"KVMKVMKVM") {
        VIRT_VENDOR_KVM
    } else if is(b"Microsoft Hv") {
        VIRT_VENDOR_MSHV
    } else if is(b"VMwareVMware") {
        VIRT_VENDOR_VMWARE
    } else if is(b"UnisysSpar64") {
        VIRT_VENDOR_SPAR
    } else {
        VIRT_VENDOR_NONE
    }
}

/// `is_vmware_platform()`: never, for the reason in the module comment.
fn is_vmware_platform() -> bool {
    false
}

/// `has_pci_device(cxt, vendor, device)`: a line of
/// `/proc/bus/pci/devices` naming that card.
fn has_pci_device(cxt: &Cxt, (vendor, device): (u32, u32)) -> bool {
    let Ok(file) = cxt.procfs.open(b"bus/pci/devices") else {
        return false;
    };
    let mut src = cstr::StreamSource::new(BufReader::new(file));
    while let Some(v) = cstr::scanf(&mut src, b"%02x%02x\t%04x%04x\t%*[^\n]") {
        if v.len() != 4 {
            break;
        }
        let ven = v.get(2).map_or(0, cstr::Scanned::as_uint);
        let dev = v.get(3).map_or(0, cstr::Scanned::as_uint);
        if ven == vendor && dev == device {
            return true;
        }
    }
    false
}

/// `is_devtree_compatible(cxt, str)`: one of the NUL-separated strings in
/// the device tree's `compatible`, within its first 255 bytes.
fn is_devtree_compatible(cxt: &Cxt, what: &[u8]) -> bool {
    let Ok(mut file) = cxt.procfs.open(b"device-tree/compatible") else {
        return false;
    };
    let mut buf = Vec::new();
    // `fread(buf, 1, 255, fd)`: what could be read.
    if file.by_ref().take(255).read_to_end(&mut buf).is_err() {
        return false;
    }
    let len = buf.len();
    let mut i = 0usize;
    while i < len {
        let s = cstr::c_str(buf.get(i..).unwrap_or_default());
        if s == what {
            return true;
        }
        i = i.saturating_add(s.len()).saturating_add(1);
    }
    false
}

/// `read_hypervisor_powerpc(cxt, &type)`: the vendor and type.
fn hypervisor_powerpc(cxt: &Cxt) -> (usize, usize) {
    if cxt.procfs.exists(b"iSeries") {
        // IBM iSeries: legacy, paravirtualized on top of OS/400.
        return (VIRT_VENDOR_OS400, VIRT_TYPE_PARA);
    }
    if is_devtree_compatible(cxt, b"ibm,powernv") {
        // PowerNV: bare metal.
        return (VIRT_VENDOR_NONE, VIRT_TYPE_NONE);
    }
    if cxt.procfs.exists(b"device-tree/ibm,partition-name")
        && cxt.procfs.exists(b"device-tree/hmc-managed?")
        && !cxt.procfs.exists(b"device-tree/chosen/qemu,graphic-width")
    {
        // PowerVM, "pHyp".
        let mut kind = VIRT_TYPE_PARA;
        if let Ok(Some(v)) = cxt
            .procfs
            .scanf(b"device-tree/ibm,partition-name", b"%255s")
            && v.first().is_some_and(|s| s.as_bytes() == b"full")
        {
            kind = VIRT_TYPE_NONE;
        }
        return (VIRT_VENDOR_PHYP, kind);
    }
    if is_devtree_compatible(cxt, b"qemu,pseries") {
        return (VIRT_VENDOR_KVM, VIRT_TYPE_PARA);
    }
    (VIRT_VENDOR_NONE, VIRT_TYPE_NONE)
}

/// `lscpu_read_virtualization(cxt)`: `None` when nothing at all was found.
#[must_use]
pub fn read_virtualization(cxt: &Cxt) -> Option<Virt> {
    let mut virt = Virt::default();
    let ct = cxt.default_type();
    if let Some(flags) = ct.and_then(|ct| ct.flags.as_deref()) {
        if cstr::has_word(flags, b"svm", BUFSIZ) {
            virt.cpuflag = Some("svm");
        } else if cstr::has_word(flags, b"vmx", BUFSIZ) {
            virt.cpuflag = Some("vmx");
        }
    }

    // WSL first: "is_vmware_platform() crashes on Windows 10".
    if let Ok(file) = cxt.procfs.open(b"sys/kernel/osrelease") {
        let mut reader = BufReader::new(file);
        if let Some(line) = cstr::fgets(&mut reader, BUFSIZ)
            && cstr::strstr(cstr::c_str(&line), b"Microsoft").is_some()
        {
            virt.vendor = VIRT_VENDOR_WSL;
            virt.kind = VIRT_TYPE_CONTAINER;
        }
        if virt.kind != VIRT_TYPE_NONE {
            return Some(virt);
        }
    }

    if !cxt.noalive {
        virt.vendor = hypervisor_cpuid();
        if virt.vendor == VIRT_VENDOR_NONE {
            virt.vendor = dmi::read_hypervisor_dmi();
        }
        if virt.vendor == VIRT_VENDOR_NONE && is_vmware_platform() {
            virt.vendor = VIRT_VENDOR_VMWARE;
        }
    }

    if virt.vendor != VIRT_VENDOR_NONE {
        // A Xen guest's PV and PVH features would be read here; upstream's
        // `fopen` of them always fails (see the module comment).
        virt.kind = VIRT_TYPE_FULL;
    } else {
        let (vendor, kind) = hypervisor_powerpc(cxt);
        virt.kind = kind;
        virt.vendor = vendor;
        if vendor == VIRT_VENDOR_NONE {
            detect_by_marks(cxt, &mut virt);
        }
    }
    if virt.cpuflag.is_none()
        && virt.hypervisor.is_none()
        && virt.vendor == VIRT_VENDOR_NONE
        && virt.kind == VIRT_TYPE_NONE
    {
        return None;
    }
    Some(virt)
}

/// The rest of upstream's `else if` chain, after the PowerPC test.
fn detect_by_marks(cxt: &Cxt, virt: &mut Virt) {
    let ct = cxt.default_type();
    if cxt.procfs.exists(b"xen") {
        // Xen paravirtualized, or dom0.
        let dom0 = cxt
            .procfs
            .scanf(b"xen/capabilities", b"%255s")
            .is_ok_and(|v| v.is_some_and(|v| v.first().is_some_and(|s| s.as_bytes() == b"control_d")));
        virt.kind = if dom0 { VIRT_TYPE_NONE } else { VIRT_TYPE_PARA };
        virt.vendor = VIRT_VENDOR_XEN;
    } else if has_pci_device(cxt, XEN_PCI) {
        // Xen fully virtualized, not on x86-64.
        virt.vendor = VIRT_VENDOR_XEN;
        virt.kind = VIRT_TYPE_FULL;
    } else if has_pci_device(cxt, VMWARE_PCI) {
        virt.vendor = VIRT_VENDOR_VMWARE;
        virt.kind = VIRT_TYPE_FULL;
    } else if has_pci_device(cxt, VBOX_PCI) {
        virt.vendor = VIRT_VENDOR_VBOX;
        virt.kind = VIRT_TYPE_FULL;
    } else if let Ok(file) = cxt.procfs.open(b"sysinfo") {
        // IBM PR/SM -- or what its "Control Program:" line names.
        virt.vendor = VIRT_VENDOR_IBM;
        virt.kind = VIRT_TYPE_FULL;
        let mut hypervisor = b"PR/SM".to_vec();
        let mut reader = BufReader::new(file);
        while let Some(line) = cstr::fgets(&mut reader, BUFSIZ) {
            let line = cstr::c_str(&line);
            if cstr::strstr(line, b"Control Program:").is_none() {
                continue;
            }
            virt.vendor = if cstr::strstr(line, b"KVM").is_some() {
                VIRT_VENDOR_KVM
            } else {
                VIRT_VENDOR_IBM
            };
            if let Some(colon) = line.iter().position(|&b| b == b':') {
                hypervisor = cstr::normalize_whitespace(
                    line.get(colon.saturating_add(1)..).unwrap_or_default(),
                );
                break;
            }
        }
        virt.hypervisor = Some(hypervisor);
    } else if cxt.procfs.exists(b"vz") && !cxt.procfs.exists(b"bc") {
        // OpenVZ/Virtuozzo: /proc/vz without /proc/bc.
        virt.vendor = VIRT_VENDOR_PARALLELS;
        virt.kind = VIRT_TYPE_CONTAINER;
    } else if virt
        .hypervisor
        .as_deref()
        .is_some_and(|h| h == b"PowerVM Lx86" || h == b"IBM/S390")
    {
        // Unreachable upstream too: nothing before this sets `hypervisor`.
        virt.vendor = VIRT_VENDOR_IBM;
        virt.kind = VIRT_TYPE_FULL;
    } else if ct
        .and_then(|ct| ct.modelname.as_deref())
        .is_some_and(|m| cstr::strstr(m, b"UML").is_some())
    {
        virt.vendor = VIRT_VENDOR_UML;
        virt.kind = VIRT_TYPE_PARA;
    } else if let Ok(file) = cxt.procfs.open(b"self/status") {
        // Linux-VServer: an all-digit VxID -- or an empty one.
        let mut val: Option<Vec<u8>> = None;
        let mut reader = BufReader::new(file);
        while let Some(line) = cstr::fgets(&mut reader, BUFSIZ) {
            if cstr::lookup(&line, b"VxID", &mut val) {
                break;
            }
        }
        if let Some(val) = val
            && val.iter().all(u8::is_ascii_digit)
        {
            virt.vendor = VIRT_VENDOR_VSERVER;
            virt.kind = VIRT_TYPE_CONTAINER;
        }
    }
}
