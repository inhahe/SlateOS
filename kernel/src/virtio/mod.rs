//! Virtio device support.
//!
//! Provides the virtio legacy (0.9.5) PCI transport and virtqueue
//! implementation.  Individual device types (blk, net, etc.) are in
//! submodules.
//!
//! ## Transport
//!
//! We use the legacy I/O port transport (BAR0) because:
//! - QEMU's default virtio-pci devices support it
//! - It avoids MMIO BAR mapping and capability parsing
//! - It's the simplest path to a working driver
//!
//! ## References
//!
//! - Virtio 0.9.5 spec (legacy): <https://ozlabs.org/~rusty/virtio-spec/virtio-0.9.5.pdf>
//! - Virtio 1.0+ spec: <https://docs.oasis-open.org/virtio/virtio/v1.1/virtio-v1.1.html>

pub mod blk;
pub mod net;
pub mod queue;

// ---------------------------------------------------------------------------
// Legacy PCI transport register offsets (from BAR0)
// ---------------------------------------------------------------------------

/// Device features (read-only for guest).
const REG_DEVICE_FEATURES: u16 = 0x00;
/// Guest features (write-only from guest).
const REG_GUEST_FEATURES: u16 = 0x04;
/// Queue address (PFN, 4096-byte page granularity).
const REG_QUEUE_PFN: u16 = 0x08;
/// Queue size (read-only, number of descriptors).
const REG_QUEUE_SIZE: u16 = 0x0C;
/// Queue select (which queue to configure).
const REG_QUEUE_SELECT: u16 = 0x0E;
/// Queue notify (kick the device after adding to available ring).
const REG_QUEUE_NOTIFY: u16 = 0x10;
/// Device status.
const REG_DEVICE_STATUS: u16 = 0x12;
/// ISR status (read to acknowledge interrupt).
const REG_ISR_STATUS: u16 = 0x13;

// Device status bits
/// Guest has found the device and recognized it.
pub const STATUS_ACKNOWLEDGE: u8 = 1;
/// Guest knows how to drive the device.
pub const STATUS_DRIVER: u8 = 2;
/// Guest driver is ready.
pub const STATUS_DRIVER_OK: u8 = 4;
/// Something went wrong — device is unusable.
pub const STATUS_FAILED: u8 = 128;

// ---------------------------------------------------------------------------
// Legacy PCI transport
// ---------------------------------------------------------------------------

/// Legacy virtio-PCI transport using I/O port BAR0.
pub struct VirtioLegacyPci {
    /// Base I/O port from BAR0.
    io_base: u16,
}

impl VirtioLegacyPci {
    /// Create a new transport from a BAR0 I/O port base.
    pub const fn new(io_base: u16) -> Self {
        Self { io_base }
    }

    /// Read a 32-bit register.
    pub fn read32(&self, offset: u16) -> u32 {
        // SAFETY: The I/O ports are for the virtio device; valid after
        // PCI discovery.
        unsafe { crate::port::inl(self.io_base.wrapping_add(offset)) }
    }

    /// Write a 32-bit register.
    pub fn write32(&self, offset: u16, value: u32) {
        unsafe { crate::port::outl(self.io_base.wrapping_add(offset), value); }
    }

    /// Read a 16-bit register.
    pub fn read16(&self, offset: u16) -> u16 {
        unsafe { crate::port::inw(self.io_base.wrapping_add(offset)) }
    }

    /// Write a 16-bit register.
    pub fn write16(&self, offset: u16, value: u16) {
        unsafe { crate::port::outw(self.io_base.wrapping_add(offset), value); }
    }

    /// Read an 8-bit register.
    pub fn read8(&self, offset: u16) -> u8 {
        unsafe { crate::port::inb(self.io_base.wrapping_add(offset)) }
    }

    /// Write an 8-bit register.
    pub fn write8(&self, offset: u16, value: u8) {
        unsafe { crate::port::outb(self.io_base.wrapping_add(offset), value); }
    }

    // -- Convenience wrappers for virtio registers --

    /// Reset the device (write 0 to status).
    pub fn reset(&self) {
        self.write8(REG_DEVICE_STATUS, 0);
    }

    /// Read the current device status.
    pub fn status(&self) -> u8 {
        self.read8(REG_DEVICE_STATUS)
    }

    /// Set status bits (OR with current status).
    pub fn set_status(&self, bits: u8) {
        let current = self.status();
        self.write8(REG_DEVICE_STATUS, current | bits);
    }

    /// Read device features.
    pub fn device_features(&self) -> u32 {
        self.read32(REG_DEVICE_FEATURES)
    }

    /// Write guest features.
    pub fn set_guest_features(&self, features: u32) {
        self.write32(REG_GUEST_FEATURES, features);
    }

    /// Select a virtqueue for configuration.
    pub fn select_queue(&self, index: u16) {
        self.write16(REG_QUEUE_SELECT, index);
    }

    /// Read the selected queue's size (number of descriptors).
    pub fn queue_size(&self) -> u16 {
        self.read16(REG_QUEUE_SIZE)
    }

    /// Set the selected queue's physical page frame number.
    ///
    /// The PFN uses 4096-byte page granularity (physical_address >> 12).
    pub fn set_queue_pfn(&self, pfn: u32) {
        self.write32(REG_QUEUE_PFN, pfn);
    }

    /// Notify the device that a queue has new entries.
    pub fn notify_queue(&self, queue_index: u16) {
        self.write16(REG_QUEUE_NOTIFY, queue_index);
    }

    /// Read and clear the ISR status register.
    pub fn read_isr(&self) -> u8 {
        self.read8(REG_ISR_STATUS)
    }

    /// Read a device-specific config byte.
    ///
    /// Device-specific config starts at offset 0x14 in legacy mode.
    pub fn read_device_config8(&self, offset: u16) -> u8 {
        self.read8(0x14u16.wrapping_add(offset))
    }

    /// Read a device-specific config 32-bit word.
    pub fn read_device_config32(&self, offset: u16) -> u32 {
        self.read32(0x14u16.wrapping_add(offset))
    }

    /// Read a device-specific config 64-bit word (two 32-bit reads).
    pub fn read_device_config64(&self, offset: u16) -> u64 {
        let lo = u64::from(self.read_device_config32(offset));
        let hi = u64::from(self.read_device_config32(offset.wrapping_add(4)));
        lo | (hi << 32)
    }
}
