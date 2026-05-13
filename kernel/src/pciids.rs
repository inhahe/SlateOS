//! PCI device identification database.
//!
//! Provides human-readable names for PCI device classes, subclasses,
//! programming interfaces, and common vendor/device IDs.  This is the
//! kernel equivalent of the `pci.ids` database used by `lspci` on Linux.
//!
//! ## Design
//!
//! The database is entirely compile-time (`const` arrays of tuples).
//! No heap allocation, no initialization step — just fast binary search
//! over sorted arrays.  The tradeoff is code size vs. runtime cost;
//! we include only the most common entries (~200 vendors, ~100 devices)
//! rather than the full 30K-entry pci.ids database.
//!
//! ## Usage
//!
//! ```ignore
//! let class_name = pciids::class_name(0x02);          // "Network controller"
//! let sub_name = pciids::subclass_name(0x02, 0x00);   // "Ethernet controller"
//! let vendor = pciids::vendor_name(0x8086);            // "Intel Corporation"
//! let device = pciids::device_name(0x8086, 0x100E);   // "82540EM Gigabit Ethernet"
//! ```
//!
//! ## References
//!
//! - https://pci-ids.ucw.cz/ (canonical pci.ids database)
//! - PCI Local Bus Specification 3.0, Appendix D (class codes)
//! - Linux `include/linux/pci_ids.h`

#![allow(dead_code)]

use alloc::string::String;
use alloc::format;

// ---------------------------------------------------------------------------
// Class codes (PCI Spec Appendix D)
// ---------------------------------------------------------------------------

/// PCI base class code → human-readable name.
///
/// Sorted by class code for binary search.
const CLASS_NAMES: &[(u8, &str)] = &[
    (0x00, "Unclassified device"),
    (0x01, "Mass storage controller"),
    (0x02, "Network controller"),
    (0x03, "Display controller"),
    (0x04, "Multimedia controller"),
    (0x05, "Memory controller"),
    (0x06, "Bridge"),
    (0x07, "Communication controller"),
    (0x08, "Generic system peripheral"),
    (0x09, "Input device controller"),
    (0x0A, "Docking station"),
    (0x0B, "Processor"),
    (0x0C, "Serial bus controller"),
    (0x0D, "Wireless controller"),
    (0x0E, "Intelligent controller"),
    (0x0F, "Satellite communication controller"),
    (0x10, "Encryption controller"),
    (0x11, "Signal processing controller"),
    (0x12, "Processing accelerator"),
    (0x13, "Non-essential instrumentation"),
    (0x40, "Co-processor"),
    (0xFF, "Unassigned class"),
];

/// PCI subclass → human-readable name.
///
/// Entries are (class, subclass, name).  Sorted by (class, subclass)
/// for binary search.
const SUBCLASS_NAMES: &[(u8, u8, &str)] = &[
    // 0x00 — Unclassified
    (0x00, 0x00, "Non-VGA compatible device"),
    (0x00, 0x01, "VGA compatible device"),
    // 0x01 — Mass storage
    (0x01, 0x00, "SCSI storage controller"),
    (0x01, 0x01, "IDE interface"),
    (0x01, 0x02, "Floppy disk controller"),
    (0x01, 0x03, "IPI bus controller"),
    (0x01, 0x04, "RAID bus controller"),
    (0x01, 0x05, "ATA controller"),
    (0x01, 0x06, "SATA controller"),
    (0x01, 0x07, "Serial Attached SCSI controller"),
    (0x01, 0x08, "Non-volatile memory controller"),
    (0x01, 0x09, "Universal Flash Storage controller"),
    (0x01, 0x80, "Mass storage controller"),
    // 0x02 — Network
    (0x02, 0x00, "Ethernet controller"),
    (0x02, 0x01, "Token Ring controller"),
    (0x02, 0x02, "FDDI controller"),
    (0x02, 0x03, "ATM controller"),
    (0x02, 0x04, "ISDN controller"),
    (0x02, 0x05, "WorldFip controller"),
    (0x02, 0x06, "PICMG 2.14 controller"),
    (0x02, 0x07, "Infiniband controller"),
    (0x02, 0x08, "Fabric controller"),
    (0x02, 0x80, "Network controller"),
    // 0x03 — Display
    (0x03, 0x00, "VGA compatible controller"),
    (0x03, 0x01, "XGA controller"),
    (0x03, 0x02, "3D controller"),
    (0x03, 0x80, "Display controller"),
    // 0x04 — Multimedia
    (0x04, 0x00, "Multimedia video controller"),
    (0x04, 0x01, "Multimedia audio controller"),
    (0x04, 0x02, "Computer telephony device"),
    (0x04, 0x03, "Audio device"),
    (0x04, 0x80, "Multimedia controller"),
    // 0x05 — Memory
    (0x05, 0x00, "RAM controller"),
    (0x05, 0x01, "FLASH controller"),
    (0x05, 0x80, "Memory controller"),
    // 0x06 — Bridge
    (0x06, 0x00, "Host bridge"),
    (0x06, 0x01, "ISA bridge"),
    (0x06, 0x02, "EISA bridge"),
    (0x06, 0x03, "MCA bridge"),
    (0x06, 0x04, "PCI bridge"),
    (0x06, 0x05, "PCMCIA bridge"),
    (0x06, 0x06, "NuBus bridge"),
    (0x06, 0x07, "CardBus bridge"),
    (0x06, 0x08, "RACEway bridge"),
    (0x06, 0x09, "PCI-to-PCI bridge"),
    (0x06, 0x0A, "InfiniBand-to-PCI bridge"),
    (0x06, 0x80, "Bridge"),
    // 0x07 — Communication
    (0x07, 0x00, "Serial controller"),
    (0x07, 0x01, "Parallel controller"),
    (0x07, 0x02, "Multiport serial controller"),
    (0x07, 0x03, "Modem"),
    (0x07, 0x04, "GPIB controller"),
    (0x07, 0x05, "Smart Card controller"),
    (0x07, 0x80, "Communication controller"),
    // 0x08 — System peripheral
    (0x08, 0x00, "PIC"),
    (0x08, 0x01, "DMA controller"),
    (0x08, 0x02, "Timer"),
    (0x08, 0x03, "RTC"),
    (0x08, 0x04, "PCI hot-plug controller"),
    (0x08, 0x05, "SD host controller"),
    (0x08, 0x06, "IOMMU"),
    (0x08, 0x80, "System peripheral"),
    // 0x09 — Input device
    (0x09, 0x00, "Keyboard controller"),
    (0x09, 0x01, "Digitizer pen"),
    (0x09, 0x02, "Mouse controller"),
    (0x09, 0x03, "Scanner controller"),
    (0x09, 0x04, "Gameport controller"),
    (0x09, 0x80, "Input device controller"),
    // 0x0C — Serial bus
    (0x0C, 0x00, "IEEE 1394 controller"),
    (0x0C, 0x01, "ACCESS bus controller"),
    (0x0C, 0x02, "SSA controller"),
    (0x0C, 0x03, "USB controller"),
    (0x0C, 0x04, "Fibre Channel"),
    (0x0C, 0x05, "SMBus controller"),
    (0x0C, 0x06, "InfiniBand controller"),
    (0x0C, 0x07, "IPMI interface"),
    (0x0C, 0x08, "SERCOS interface"),
    (0x0C, 0x09, "CAN bus controller"),
    (0x0C, 0x80, "Serial bus controller"),
    // 0x0D — Wireless
    (0x0D, 0x00, "IRDA controller"),
    (0x0D, 0x01, "Consumer IR controller"),
    (0x0D, 0x10, "RF controller"),
    (0x0D, 0x11, "Bluetooth controller"),
    (0x0D, 0x12, "Broadband controller"),
    (0x0D, 0x20, "Ethernet (802.1a) controller"),
    (0x0D, 0x21, "Ethernet (802.1b) controller"),
    (0x0D, 0x80, "Wireless controller"),
    // 0x10 — Encryption
    (0x10, 0x00, "Network/computing encryption"),
    (0x10, 0x10, "Entertainment encryption"),
    (0x10, 0x80, "Encryption controller"),
    // 0x11 — Signal processing
    (0x11, 0x00, "DPIO module"),
    (0x11, 0x01, "Performance counters"),
    (0x11, 0x10, "Communication synchronizer"),
    (0x11, 0x20, "Signal processing management"),
    (0x11, 0x80, "Signal processing controller"),
];

/// USB programming interface → controller type name.
const USB_PROG_IF: &[(u8, &str)] = &[
    (0x00, "UHCI"),
    (0x10, "OHCI"),
    (0x20, "EHCI"),
    (0x30, "xHCI"),
    (0x40, "USB4 Host Interface"),
    (0x80, "Unspecified"),
    (0xFE, "USB Device (not controller)"),
];

// ---------------------------------------------------------------------------
// Common vendor IDs
// ---------------------------------------------------------------------------

/// Common PCI vendor IDs → names.
///
/// Sorted by vendor ID for binary search.  Includes the most commonly
/// encountered vendors in real hardware and virtual machines.
const VENDOR_NAMES: &[(u16, &str)] = &[
    (0x1002, "AMD/ATI"),
    (0x1013, "Cirrus Logic"),
    (0x1014, "IBM"),
    (0x1022, "AMD"),
    (0x1033, "NEC"),
    (0x104C, "Texas Instruments"),
    (0x1050, "Winbond"),
    (0x105A, "Promise Technology"),
    (0x1095, "Silicon Image"),
    (0x10B5, "PLX Technology"),
    (0x10B7, "3Com"),
    (0x10DE, "NVIDIA"),
    (0x10EC, "Realtek"),
    (0x1106, "VIA Technologies"),
    (0x1180, "Ricoh"),
    (0x1217, "O2 Micro"),
    (0x125B, "Asix Electronics"),
    (0x1274, "Ensoniq"),
    (0x12D8, "Pericom Semiconductor"),
    (0x1307, "LAVA Computer"),
    (0x13F6, "C-Media Electronics"),
    (0x14C3, "MediaTek"),
    (0x14E4, "Broadcom"),
    (0x15AD, "VMware"),
    (0x168C, "Qualcomm Atheros"),
    (0x17CB, "Qualcomm"),
    (0x1912, "Renesas Technology"),
    (0x197B, "JMicron Technology"),
    (0x19A2, "Emulex"),
    (0x1AE0, "Google"),
    (0x1AF4, "Red Hat (virtio)"),
    (0x1B21, "ASMedia Technology"),
    (0x1B36, "Red Hat (QEMU)"),
    (0x1B4B, "Marvell Technology"),
    (0x1C5C, "SK Hynix"),
    (0x1D6A, "Aquantia"),
    (0x1DB7, "Phison Electronics"),
    (0x1E0F, "KIOXIA"),
    (0x1FC9, "Tehuti Networks"),
    (0x2646, "Kingston Technology"),
    (0x8086, "Intel Corporation"),
    (0x80EE, "InnoTek (VirtualBox)"),
    (0x9005, "Adaptec"),
];

// ---------------------------------------------------------------------------
// Common device IDs (per vendor)
// ---------------------------------------------------------------------------

/// Common device IDs → names.
///
/// Entries are (vendor_id, device_id, name).  Sorted by (vendor, device)
/// for binary search.  Includes devices commonly found in VMs and popular
/// real hardware.
const DEVICE_NAMES: &[(u16, u16, &str)] = &[
    // Intel (0x8086)
    (0x8086, 0x0044, "Core Processor DRAM Controller"),
    (0x8086, 0x0100, "2nd Gen Core DRAM Controller"),
    (0x8086, 0x0102, "HD Graphics 2000"),
    (0x8086, 0x0152, "HD Graphics 2500"),
    (0x8086, 0x0412, "HD Graphics 4600"),
    (0x8086, 0x0C00, "4th Gen Core DRAM Controller"),
    (0x8086, 0x100E, "82540EM Gigabit Ethernet (e1000)"),
    (0x8086, 0x100F, "82545EM Gigabit Ethernet (e1000)"),
    (0x8086, 0x10D3, "82574L Gigabit Ethernet (e1000e)"),
    (0x8086, 0x10EA, "I217-LM Ethernet"),
    (0x8086, 0x153A, "I217-V Ethernet (e1000e)"),
    (0x8086, 0x15B8, "I219-V Ethernet"),
    (0x8086, 0x1901, "Xeon E3-1200 v5 PCI Express"),
    (0x8086, 0x1911, "Xeon E3-1200 v5 Gaussian Mixture Model"),
    (0x8086, 0x1C10, "6 Series/C200 PCI Express Root Port"),
    (0x8086, 0x1C20, "6 Series/C200 HD Audio"),
    (0x8086, 0x1C22, "6 Series/C200 SMBus Controller"),
    (0x8086, 0x1C26, "6 Series/C200 USB Enhanced Host"),
    (0x8086, 0x1C2D, "6 Series/C200 USB Enhanced Host"),
    (0x8086, 0x1D00, "C600/X79 PCI Express Root Port"),
    (0x8086, 0x1E10, "7 Series/C210 PCI Express Root Port"),
    (0x8086, 0x1E20, "7 Series/C210 HD Audio"),
    (0x8086, 0x1E22, "7 Series/C210 SMBus Controller"),
    (0x8086, 0x1E26, "7 Series/C210 USB Enhanced Host"),
    (0x8086, 0x1E31, "7 Series/C210 USB xHCI Host"),
    (0x8086, 0x2668, "82801FB/FBM/FR/FW/FRW HDA Controller"),
    (0x8086, 0x2770, "82945G/GZ/P/PL Memory Controller Hub"),
    (0x8086, 0x2918, "82801IB/IR/IH LPC Interface Controller"),
    (0x8086, 0x2922, "82801IR/IO/IH SATA Controller (AHCI)"),
    (0x8086, 0x2930, "82801I SMBus Controller"),
    (0x8086, 0x293E, "82801I HD Audio Controller"),
    (0x8086, 0x29C0, "82G33/G31/P35/P31 DRAM Controller"),
    (0x8086, 0x2934, "82801I USB UHCI Controller"),
    (0x8086, 0x2935, "82801I USB UHCI Controller"),
    (0x8086, 0x2936, "82801I USB UHCI Controller"),
    (0x8086, 0x2937, "82801I USB UHCI Controller"),
    (0x8086, 0x293A, "82801I USB EHCI Controller"),
    (0x8086, 0x3A18, "82801JIB HD Audio Controller"),
    (0x8086, 0x3B42, "5 Series/3400 SMBus Controller"),
    (0x8086, 0x3B56, "5 Series/3400 HD Audio"),
    (0x8086, 0x7000, "82371SB PIIX3 ISA"),
    (0x8086, 0x7010, "82371SB PIIX3 IDE"),
    (0x8086, 0x7020, "82371SB PIIX3 USB"),
    (0x8086, 0x7110, "82371AB/EB/MB PIIX4 ISA"),
    (0x8086, 0x7111, "82371AB/EB/MB PIIX4 IDE"),
    (0x8086, 0x7113, "82371AB/EB/MB PIIX4 ACPI"),
    (0x8086, 0xA0A8, "Tiger Lake USB 3.2 xHCI"),
    (0x8086, 0xA0C8, "Tiger Lake HD Audio Controller"),
    (0x8086, 0xA0EF, "Tiger Lake PCI Express Root Port"),
    (0x8086, 0xA170, "100 Series/C230 HD Audio"),
    (0x8086, 0xA282, "200 Series/Z370 SATA Controller"),
    (0x8086, 0xA2B1, "200 Series/Z370 SMBus Controller"),
    // AMD/ATI (0x1002)
    (0x1002, 0x1638, "Renoir Radeon Graphics"),
    (0x1002, 0x164E, "Raphael Radeon Graphics"),
    (0x1002, 0x6600, "Mars [Radeon HD 8600/8700M]"),
    (0x1002, 0x67DF, "Ellesmere [Radeon RX 470/480/570/580]"),
    (0x1002, 0x6798, "Tahiti XT [Radeon HD 7970/8970 OC]"),
    (0x1002, 0x6900, "Topaz [Radeon R7 M260/M265]"),
    (0x1002, 0x7310, "Navi 10 [Radeon RX 5600 OEM/Pro]"),
    (0x1002, 0x731F, "Navi 10 [Radeon RX 5600/5700]"),
    (0x1002, 0x73BF, "Navi 21 [Radeon RX 6800/6900 XT]"),
    (0x1002, 0x73DF, "Navi 22 [Radeon RX 6700 XT]"),
    (0x1002, 0x744C, "Navi 31 [Radeon RX 7900 XT/XTX]"),
    (0x1002, 0x7480, "Navi 33 [Radeon RX 7600]"),
    // AMD (0x1022)
    (0x1022, 0x1450, "Family 17h Root Complex"),
    (0x1022, 0x1480, "Family 17h/19h IOMMU"),
    (0x1022, 0x1482, "Family 17h/19h PCIe GPP Bridge"),
    (0x1022, 0x1630, "Family 19h Root Complex"),
    (0x1022, 0x790B, "FCH SMBus Controller"),
    (0x1022, 0x790E, "FCH LPC Bridge"),
    // NVIDIA (0x10DE)
    (0x10DE, 0x0FC6, "GeForce GTX 650"),
    (0x10DE, 0x1180, "GeForce GTX 680"),
    (0x10DE, 0x1380, "GeForce GTX 750 Ti"),
    (0x10DE, 0x1B80, "GeForce GTX 1080"),
    (0x10DE, 0x1B81, "GeForce GTX 1070"),
    (0x10DE, 0x1C02, "GeForce GTX 1060 3GB"),
    (0x10DE, 0x1C03, "GeForce GTX 1060 6GB"),
    (0x10DE, 0x1E04, "GeForce RTX 2080 Ti"),
    (0x10DE, 0x1E07, "GeForce RTX 2080 Ti Rev. A"),
    (0x10DE, 0x1F02, "GeForce RTX 2070"),
    (0x10DE, 0x1F07, "GeForce RTX 2070 Rev. A"),
    (0x10DE, 0x2204, "GeForce RTX 3090"),
    (0x10DE, 0x2206, "GeForce RTX 3080"),
    (0x10DE, 0x2208, "GeForce RTX 3080 Ti"),
    (0x10DE, 0x2216, "GeForce RTX 3080 Laptop"),
    (0x10DE, 0x2482, "GeForce RTX 4070 Ti"),
    (0x10DE, 0x2484, "GeForce RTX 4070"),
    (0x10DE, 0x2504, "GeForce RTX 4080"),
    (0x10DE, 0x2684, "GeForce RTX 4090"),
    // Realtek (0x10EC)
    (0x10EC, 0x0887, "ALC887 HD Audio"),
    (0x10EC, 0x0892, "ALC892 HD Audio"),
    (0x10EC, 0x0900, "ALC1220 HD Audio"),
    (0x10EC, 0x5229, "RTS5229 Card Reader"),
    (0x10EC, 0x8125, "RTL8125 2.5GbE Ethernet"),
    (0x10EC, 0x8139, "RTL-8100/8101L/8139 Fast Ethernet"),
    (0x10EC, 0x8168, "RTL8111/8168/8411 Gigabit Ethernet"),
    (0x10EC, 0x8852, "RTL8852BE WiFi 6"),
    // Broadcom (0x14E4)
    (0x14E4, 0x1682, "BCM57762 Gigabit Ethernet"),
    (0x14E4, 0x43A0, "BCM4360 802.11ac Wireless"),
    (0x14E4, 0x43B1, "BCM4352 802.11ac Wireless"),
    // virtio (0x1AF4) — Red Hat
    (0x1AF4, 0x1000, "virtio-net (legacy)"),
    (0x1AF4, 0x1001, "virtio-blk (legacy)"),
    (0x1AF4, 0x1002, "virtio-balloon (legacy)"),
    (0x1AF4, 0x1003, "virtio-console (legacy)"),
    (0x1AF4, 0x1004, "virtio-scsi (legacy)"),
    (0x1AF4, 0x1005, "virtio-rng (legacy)"),
    (0x1AF4, 0x1009, "virtio-9p (legacy)"),
    (0x1AF4, 0x1041, "virtio-net (modern)"),
    (0x1AF4, 0x1042, "virtio-blk (modern)"),
    (0x1AF4, 0x1043, "virtio-console (modern)"),
    (0x1AF4, 0x1044, "virtio-rng (modern)"),
    (0x1AF4, 0x1045, "virtio-balloon (modern)"),
    (0x1AF4, 0x1048, "virtio-scsi (modern)"),
    (0x1AF4, 0x1050, "virtio-gpu (modern)"),
    (0x1AF4, 0x1052, "virtio-input (modern)"),
    (0x1AF4, 0x1059, "virtio-sound (modern)"),
    // QEMU (0x1B36)
    (0x1B36, 0x0001, "QEMU PCI-PCI bridge"),
    (0x1B36, 0x0002, "QEMU PCI serial port"),
    (0x1B36, 0x0003, "QEMU PCI serial port (2 ports)"),
    (0x1B36, 0x0004, "QEMU PCI serial port (4 ports)"),
    (0x1B36, 0x0005, "QEMU PCI test device"),
    (0x1B36, 0x0008, "QEMU PCIe host bridge"),
    (0x1B36, 0x000D, "QEMU XHCI USB host controller"),
    (0x1B36, 0x0010, "QEMU SD/MMC controller"),
    (0x1B36, 0x0100, "QXL paravirtual GPU"),
    // VMware (0x15AD)
    (0x15AD, 0x0405, "SVGA II Adapter"),
    (0x15AD, 0x0710, "SVGA Adapter"),
    (0x15AD, 0x0720, "VMXNET Ethernet Controller"),
    (0x15AD, 0x0740, "Virtual Machine Communication Interface"),
    (0x15AD, 0x0770, "USB2 EHCI Controller"),
    (0x15AD, 0x0774, "USB1.1 UHCI Controller"),
    (0x15AD, 0x0778, "USB3 xHCI Controller"),
    (0x15AD, 0x0790, "PCI bridge"),
    (0x15AD, 0x07A0, "PCI Express Root Port"),
    (0x15AD, 0x07B0, "VMXNET3 Ethernet Controller"),
    (0x15AD, 0x07C0, "PVSCSI SCSI Controller"),
    (0x15AD, 0x07E0, "SATA AHCI Controller"),
    (0x15AD, 0x0801, "Virtual Machine Interface"),
    // VirtualBox (0x80EE)
    (0x80EE, 0xBEEF, "VirtualBox Graphics Adapter"),
    (0x80EE, 0xCAFE, "VirtualBox Guest Service"),
    // Samsung (NVMe)
    (0x144D, 0xA808, "NVMe SSD 970 EVO Plus"),
    (0x144D, 0xA809, "NVMe SSD 980 PRO"),
    (0x144D, 0xA80A, "NVMe SSD 990 PRO"),
    // Western Digital
    (0x15B7, 0x5006, "WD Black SN850 NVMe SSD"),
    (0x15B7, 0x5009, "WD Blue SN570 NVMe SSD"),
    (0x15B7, 0x5030, "WD Black SN850X NVMe SSD"),
    // ASMedia (0x1B21)
    (0x1B21, 0x1042, "ASM1042 USB 3.0 Host Controller"),
    (0x1B21, 0x1142, "ASM1042A USB 3.0 Host Controller"),
    (0x1B21, 0x1242, "ASM1142 USB 3.1 Host Controller"),
    (0x1B21, 0x2142, "ASM2142/ASM3142 USB 3.1 Host Controller"),
    (0x1B21, 0x3242, "ASM3242 USB 3.2 Host Controller"),
    // Qualcomm Atheros (0x168C)
    (0x168C, 0x002E, "AR9287 Wireless"),
    (0x168C, 0x0030, "AR93xx Wireless"),
    (0x168C, 0x003C, "QCA986x/988x 802.11ac"),
    (0x168C, 0x003E, "QCA6174 802.11ac Wireless"),
    (0x168C, 0x0042, "QCA9377 802.11ac Wireless"),
    (0x168C, 0x0046, "QCA9984 802.11ac Wave 2 Wireless"),
];

// ---------------------------------------------------------------------------
// Lookup functions
// ---------------------------------------------------------------------------

/// Look up the base class name for a PCI class code.
pub fn class_name(class: u8) -> &'static str {
    match CLASS_NAMES.binary_search_by_key(&class, |&(c, _)| c) {
        Ok(idx) => CLASS_NAMES[idx].1,
        Err(_) => "Unknown class",
    }
}

/// Look up the subclass name for a (class, subclass) pair.
pub fn subclass_name(class: u8, subclass: u8) -> &'static str {
    let key = (class, subclass);
    match SUBCLASS_NAMES.binary_search_by_key(&key, |&(c, s, _)| (c, s)) {
        Ok(idx) => SUBCLASS_NAMES[idx].2,
        Err(_) => "Unknown subclass",
    }
}

/// Look up the vendor name for a PCI vendor ID.
pub fn vendor_name(vendor: u16) -> Option<&'static str> {
    match VENDOR_NAMES.binary_search_by_key(&vendor, |&(v, _)| v) {
        Ok(idx) => Some(VENDOR_NAMES[idx].1),
        Err(_) => None,
    }
}

/// Look up the device name for a (vendor, device) ID pair.
pub fn device_name(vendor: u16, device: u16) -> Option<&'static str> {
    let key = (vendor, device);
    match DEVICE_NAMES.binary_search_by_key(&key, |&(v, d, _)| (v, d)) {
        Ok(idx) => Some(DEVICE_NAMES[idx].2),
        Err(_) => None,
    }
}

/// Look up the USB controller type from the programming interface byte.
pub fn usb_controller_type(prog_if: u8) -> &'static str {
    match USB_PROG_IF.binary_search_by_key(&prog_if, |&(p, _)| p) {
        Ok(idx) => USB_PROG_IF[idx].1,
        Err(_) => "Unknown USB type",
    }
}

/// Get a full human-readable description of a PCI device.
///
/// Returns a string like:
///   "Intel Corporation 82540EM Gigabit Ethernet (e1000) [Ethernet controller]"
///
/// Falls back to hex IDs for unknown vendors/devices.
pub fn describe(vendor_id: u16, device_id: u16, class: u8, subclass: u8) -> String {
    let vendor = vendor_name(vendor_id)
        .unwrap_or("Unknown vendor");
    let device = device_name(vendor_id, device_id);
    let sub = subclass_name(class, subclass);

    match device {
        Some(dev_name) => format!("{} {} [{}]", vendor, dev_name, sub),
        None => format!("{} {:04x}:{:04x} [{}]", vendor, vendor_id, device_id, sub),
    }
}

/// Get a compact device label (just device name or vendor:device hex).
pub fn device_label(vendor_id: u16, device_id: u16) -> String {
    match device_name(vendor_id, device_id) {
        Some(name) => String::from(name),
        None => format!("{:04x}:{:04x}", vendor_id, device_id),
    }
}

/// Get the number of entries in each database table.
pub fn db_stats() -> (usize, usize, usize, usize) {
    (
        CLASS_NAMES.len(),
        SUBCLASS_NAMES.len(),
        VENDOR_NAMES.len(),
        DEVICE_NAMES.len(),
    )
}

// ---------------------------------------------------------------------------
// procfs content
// ---------------------------------------------------------------------------

/// Generate content for `/proc/pciids`.
pub fn procfs_content() -> String {
    let mut out = String::with_capacity(512);
    out.push_str("=== PCI ID Database ===\n");
    let (classes, subclasses, vendors, devices) = db_stats();
    out.push_str(&format!("classes:    {}\n", classes));
    out.push_str(&format!("subclasses: {}\n", subclasses));
    out.push_str(&format!("vendors:    {}\n", vendors));
    out.push_str(&format!("devices:    {}\n", devices));

    // Show all known vendors.
    out.push_str("\n=== Vendors ===\n");
    for &(vid, name) in VENDOR_NAMES {
        out.push_str(&format!("{:04X}  {}\n", vid, name));
    }

    out
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Self-test for PCI ID database.
pub fn self_test() {
    crate::serial_println!("[pciids] Running self-test...");

    // Test 1: Known class lookups.
    assert_eq!(class_name(0x01), "Mass storage controller");
    assert_eq!(class_name(0x02), "Network controller");
    assert_eq!(class_name(0x03), "Display controller");
    assert_eq!(class_name(0x06), "Bridge");
    assert_eq!(class_name(0x0C), "Serial bus controller");
    assert_eq!(class_name(0xFE), "Unknown class"); // Not in table.
    crate::serial_println!("[pciids]   Class lookups: OK");

    // Test 2: Known subclass lookups.
    assert_eq!(subclass_name(0x01, 0x06), "SATA controller");
    assert_eq!(subclass_name(0x01, 0x08), "Non-volatile memory controller");
    assert_eq!(subclass_name(0x02, 0x00), "Ethernet controller");
    assert_eq!(subclass_name(0x03, 0x00), "VGA compatible controller");
    assert_eq!(subclass_name(0x0C, 0x03), "USB controller");
    assert_eq!(subclass_name(0x06, 0x00), "Host bridge");
    assert_eq!(subclass_name(0xFF, 0xFF), "Unknown subclass");
    crate::serial_println!("[pciids]   Subclass lookups: OK");

    // Test 3: Known vendor lookups.
    assert_eq!(vendor_name(0x8086), Some("Intel Corporation"));
    assert_eq!(vendor_name(0x10DE), Some("NVIDIA"));
    assert_eq!(vendor_name(0x1002), Some("AMD/ATI"));
    assert_eq!(vendor_name(0x10EC), Some("Realtek"));
    assert_eq!(vendor_name(0x1AF4), Some("Red Hat (virtio)"));
    assert_eq!(vendor_name(0x15AD), Some("VMware"));
    assert_eq!(vendor_name(0x0000), None); // Unknown.
    crate::serial_println!("[pciids]   Vendor lookups: OK");

    // Test 4: Known device lookups.
    assert_eq!(device_name(0x8086, 0x100E), Some("82540EM Gigabit Ethernet (e1000)"));
    assert_eq!(device_name(0x10EC, 0x8139), Some("RTL-8100/8101L/8139 Fast Ethernet"));
    assert_eq!(device_name(0x1AF4, 0x1000), Some("virtio-net (legacy)"));
    assert_eq!(device_name(0x15AD, 0x0405), Some("SVGA II Adapter"));
    assert_eq!(device_name(0x80EE, 0xBEEF), Some("VirtualBox Graphics Adapter"));
    assert_eq!(device_name(0x8086, 0xFFFF), None); // Unknown device.
    crate::serial_println!("[pciids]   Device lookups: OK");

    // Test 5: USB prog-if lookups.
    assert_eq!(usb_controller_type(0x00), "UHCI");
    assert_eq!(usb_controller_type(0x20), "EHCI");
    assert_eq!(usb_controller_type(0x30), "xHCI");
    crate::serial_println!("[pciids]   USB prog-if lookups: OK");

    // Test 6: describe() produces reasonable output.
    let desc = describe(0x8086, 0x100E, 0x02, 0x00);
    assert!(desc.contains("Intel"), "Description should contain vendor");
    assert!(desc.contains("82540EM"), "Description should contain device name");
    assert!(desc.contains("Ethernet"), "Description should contain class");
    crate::serial_println!("[pciids]   describe(): {}", desc);

    // Test 7: describe() with unknown device.
    let desc2 = describe(0x8086, 0xFFFF, 0x02, 0x00);
    assert!(desc2.contains("8086:ffff"), "Unknown device should show hex IDs");
    crate::serial_println!("[pciids]   describe(unknown): {}", desc2);

    // Test 8: device_label() for known and unknown.
    let lbl = device_label(0x10EC, 0x8168);
    assert_eq!(lbl, "RTL8111/8168/8411 Gigabit Ethernet");
    let lbl2 = device_label(0x0000, 0x0000);
    assert_eq!(lbl2, "0000:0000");
    crate::serial_println!("[pciids]   device_label(): OK");

    // Test 9: Database tables are sorted (binary search prerequisite).
    for i in 1..CLASS_NAMES.len() {
        assert!(CLASS_NAMES[i].0 >= CLASS_NAMES[i - 1].0, "CLASS_NAMES not sorted at {}", i);
    }
    for i in 1..SUBCLASS_NAMES.len() {
        let prev = (SUBCLASS_NAMES[i - 1].0, SUBCLASS_NAMES[i - 1].1);
        let curr = (SUBCLASS_NAMES[i].0, SUBCLASS_NAMES[i].1);
        assert!(curr >= prev, "SUBCLASS_NAMES not sorted at {}", i);
    }
    for i in 1..VENDOR_NAMES.len() {
        assert!(VENDOR_NAMES[i].0 >= VENDOR_NAMES[i - 1].0, "VENDOR_NAMES not sorted at {}", i);
    }
    for i in 1..DEVICE_NAMES.len() {
        let prev = (DEVICE_NAMES[i - 1].0, DEVICE_NAMES[i - 1].1);
        let curr = (DEVICE_NAMES[i].0, DEVICE_NAMES[i].1);
        assert!(curr >= prev, "DEVICE_NAMES not sorted at {} ({:04x}:{:04x})", i, curr.0, curr.1);
    }
    crate::serial_println!("[pciids]   Sort order: OK");

    // Test 10: db_stats returns correct counts.
    let (c, s, v, d) = db_stats();
    assert!(c > 15, "Should have >15 class entries");
    assert!(s > 50, "Should have >50 subclass entries");
    assert!(v > 30, "Should have >30 vendor entries");
    assert!(d > 80, "Should have >80 device entries");
    crate::serial_println!("[pciids]   DB stats: {} classes, {} subclasses, {} vendors, {} devices", c, s, v, d);

    // Test 11: procfs content is non-empty and well-formed.
    let content = procfs_content();
    assert!(content.contains("=== PCI ID Database ==="), "procfs should have header");
    assert!(content.contains("=== Vendors ==="), "procfs should have vendors section");
    assert!(content.contains("Intel Corporation"), "procfs should list Intel");
    crate::serial_println!("[pciids]   procfs: OK ({} bytes)", content.len());

    crate::serial_println!("[pciids] Self-test PASSED (11 tests)");
}
