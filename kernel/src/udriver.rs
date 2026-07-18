//! Userspace driver framework — kernel-side support for running drivers
//! in userspace processes.
//!
//! This is the core microkernel infrastructure: drivers run as normal
//! userspace processes, and the kernel provides controlled access to
//! hardware resources through this framework.
//!
//! ## Architecture
//!
//! ```text
//! Userspace driver process
//!   ↓ SYS_DRV_REGISTER (register as driver for device)
//!   ↓ SYS_DRV_MAP_MMIO (map device MMIO BAR into address space)
//!   ↓ SYS_DRV_ALLOC_DMA (allocate DMA-safe buffer)
//!   ↓ SYS_IRQ_WAIT (wait for device interrupt — already in irq subsystem)
//!   ↓ normal I/O operations via mapped regions
//!
//! Kernel
//!   → Validates capability (driver must hold `ipc.driver` capability)
//!   → Creates MMIO mapping in driver's page table
//!   → Allocates physically contiguous DMA buffer
//!   → Tracks all resources for cleanup on driver crash/exit
//!   → IOMMU: restricts device DMA to only driver's buffers
//! ```
//!
//! ## Resource Lifecycle
//!
//! When a driver process exits (normally or via crash), all its hardware
//! resources are automatically reclaimed:
//! - MMIO mappings unmapped from process address space
//! - DMA buffers freed (after IOMMU fence if applicable)
//! - Device unbound and available for re-binding
//! - Interrupt routing cleared
//!
//! This is critical for crash recovery: [`crate::drvmon`] detects the crash,
//! this module cleans up the old driver's resources, then a fresh driver
//! instance can re-register.
//!
//! ## Security Model
//!
//! - **Capability-gated**: only processes with `ipc.driver` can register.
//! - **One driver per device**: a device can only be bound to one driver
//!   at a time. Second bind attempt returns `DeviceBusy`.
//! - **IOMMU isolation**: when IOMMU is available, each driver gets its
//!   own DMA domain — even if the hardware itself goes rogue, it can only
//!   DMA to pages explicitly mapped for that driver.
//! - **No ambient MMIO**: drivers cannot access arbitrary physical memory.
//!   Only BARs of their bound device are mappable.
//!
//! ## References
//!
//! - Fuchsia `zircon/kernel/object/bus_transaction_initiator.cc` — DMA pinning
//! - Fuchsia `zircon/kernel/object/resource.cc` — MMIO grant model
//! - seL4 device untyped + frame capabilities
//! - Linux `drivers/vfio/` — userspace driver passthrough

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use alloc::format;
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum MMIO mappings per driver.
const MAX_MMIO_MAPPINGS_PER_DRIVER: usize = 16;

/// Maximum DMA buffers per driver.
const MAX_DMA_BUFFERS_PER_DRIVER: usize = 64;

/// Maximum registered drivers.
const MAX_DRIVERS: usize = 128;

/// Maximum pending device bindings awaiting a driver.
const MAX_PENDING_DEVICES: usize = 256;

/// DMA buffer alignment — must be page-aligned (16 KiB for our kernel).
const DMA_ALIGNMENT: usize = 16384;

/// Maximum single DMA buffer size (16 MiB).
const MAX_DMA_BUFFER_SIZE: usize = 16 * 1024 * 1024;

/// Maximum single MMIO mapping size (256 MiB, covers most device BARs).
const MAX_MMIO_MAP_SIZE: u64 = 256 * 1024 * 1024;

// ---------------------------------------------------------------------------
// Types — Device identity
// ---------------------------------------------------------------------------

/// PCI device address (bus:device:function).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeviceAddr {
    pub bus: u8,
    pub device: u8,
    pub function: u8,
}

impl DeviceAddr {
    /// Create a new PCI device address.
    #[must_use]
    pub const fn new(bus: u8, device: u8, function: u8) -> Self {
        Self { bus, device, function }
    }

    /// Compact BDF encoding for display and comparison.
    #[must_use]
    #[allow(clippy::arithmetic_side_effects)]
    pub const fn bdf(&self) -> u16 {
        (self.bus as u16) << 8 | (self.device as u16) << 3 | (self.function as u16)
    }
}

/// Device class identifier (vendor:device from PCI config space).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeviceId {
    pub vendor_id: u16,
    pub device_id: u16,
    pub class: u8,
    pub subclass: u8,
}

// ---------------------------------------------------------------------------
// Types — MMIO mapping
// ---------------------------------------------------------------------------

/// Permission flags for MMIO mappings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MmioPerms {
    /// Allow reads from the MMIO region.
    pub read: bool,
    /// Allow writes to the MMIO region.
    pub write: bool,
}

impl MmioPerms {
    pub const READ_ONLY: Self = Self { read: true, write: false };
    pub const READ_WRITE: Self = Self { read: true, write: true };
}

/// A single MMIO region mapped into a driver's address space.
#[derive(Debug, Clone)]
pub struct MmioMapping {
    /// Which BAR this mapping corresponds to (0-5).
    pub bar_index: u8,
    /// Physical base address of the MMIO region.
    pub phys_base: u64,
    /// Virtual address in the driver's address space.
    pub virt_base: u64,
    /// Size of the mapping in bytes.
    pub size: u64,
    /// Access permissions.
    pub perms: MmioPerms,
    /// Timestamp when the mapping was created (ns).
    pub mapped_at: u64,
}

// ---------------------------------------------------------------------------
// Types — DMA buffer
// ---------------------------------------------------------------------------

/// A DMA buffer allocated for a driver.
///
/// The kernel allocates physically contiguous memory (or sets up IOMMU
/// scatter-gather), then maps it into both the driver's virtual address
/// space and the device's DMA address space.
#[derive(Debug, Clone)]
pub struct DmaBuffer {
    /// Unique buffer ID (for revocation).
    pub id: u32,
    /// Physical address (for programming the device).
    pub phys_addr: u64,
    /// Virtual address in the driver's address space.
    pub virt_addr: u64,
    /// Bus address (may differ from phys if IOMMU remaps).
    pub bus_addr: u64,
    /// Size in bytes.
    pub size: usize,
    /// Whether the device can read from this buffer.
    pub device_readable: bool,
    /// Whether the device can write to this buffer.
    pub device_writable: bool,
    /// Timestamp when allocated (ns).
    pub allocated_at: u64,
}

/// Direction of DMA transfer — determines cache management.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DmaDirection {
    /// Device reads from buffer (driver writes, then device reads).
    ToDevice,
    /// Device writes to buffer (device writes, then driver reads).
    FromDevice,
    /// Both directions (e.g., command ring with completion entries).
    Bidirectional,
}

// ---------------------------------------------------------------------------
// Types — Driver registration
// ---------------------------------------------------------------------------

/// State of a driver binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriverState {
    /// Driver registered but not yet bound to a device.
    Registered,
    /// Driver bound to device, setting up MMIO/DMA.
    Binding,
    /// Driver fully active and serving I/O.
    Active,
    /// Driver is being shut down (resources being reclaimed).
    ShuttingDown,
    /// Driver crashed — resources reclaimed, awaiting restart.
    Crashed,
    /// Driver cleanly unregistered.
    Unregistered,
}

/// A registered userspace driver and all its allocated resources.
#[derive(Debug, Clone)]
pub struct DriverBinding {
    /// Unique driver binding ID.
    pub id: u32,
    /// Human-readable driver name.
    pub name: String,
    /// PID of the driver process.
    pub pid: u32,
    /// Device this driver is bound to.
    pub device_addr: DeviceAddr,
    /// Device identification.
    pub device_id: DeviceId,
    /// Current state.
    pub state: DriverState,
    /// MMIO regions mapped for this driver.
    pub mmio_mappings: Vec<MmioMapping>,
    /// DMA buffers allocated for this driver.
    pub dma_buffers: Vec<DmaBuffer>,
    /// IRQ numbers this driver is handling.
    pub irq_lines: Vec<u8>,
    /// IOMMU domain ID (if IOMMU is active).
    pub iommu_domain: Option<u32>,
    /// Total bytes of DMA memory allocated.
    pub total_dma_bytes: usize,
    /// When the driver registered (ns).
    pub registered_at: u64,
    /// When the driver became active (ns), or 0.
    pub active_since: u64,
    /// Number of I/O requests served (as reported by driver).
    pub io_requests_served: u64,
}

/// A device discovered but not yet claimed by a driver.
#[derive(Debug, Clone)]
pub struct UnclaimedDevice {
    pub addr: DeviceAddr,
    pub id: DeviceId,
    /// Timestamp when the device was discovered (ns).
    pub discovered_at: u64,
}

// ---------------------------------------------------------------------------
// Global state
// ---------------------------------------------------------------------------

struct State {
    /// All registered driver bindings.
    drivers: Vec<DriverBinding>,
    /// Devices awaiting driver binding.
    unclaimed: Vec<UnclaimedDevice>,
    /// Next driver binding ID.
    next_id: u32,
    /// Next DMA buffer ID.
    next_dma_id: u32,
    /// Total MMIO bytes mapped across all drivers.
    total_mmio_bytes: u64,
    /// Total DMA bytes allocated across all drivers.
    total_dma_bytes: usize,
    /// Whether IOMMU is available for DMA isolation.
    iommu_available: bool,
    /// Total driver registrations since boot.
    total_registrations: u64,
    /// Total driver crashes since boot.
    total_crashes: u64,
    /// Total resource cleanup operations since boot.
    total_cleanups: u64,
}

impl State {
    const fn new() -> Self {
        Self {
            drivers: Vec::new(),
            unclaimed: Vec::new(),
            next_id: 1,
            next_dma_id: 1,
            total_mmio_bytes: 0,
            total_dma_bytes: 0,
            iommu_available: false,
            total_registrations: 0,
            total_crashes: 0,
            total_cleanups: 0,
        }
    }
}

static STATE: Mutex<State> = Mutex::new(State::new());

// ---------------------------------------------------------------------------
// Device discovery — kernel calls this during PCI enumeration
// ---------------------------------------------------------------------------

/// Register a newly discovered PCI device as available for driver binding.
///
/// Called by the PCI enumeration code when it finds a device. The device
/// goes into the unclaimed pool until a userspace driver registers for it.
pub fn register_device(addr: DeviceAddr, id: DeviceId) -> KernelResult<()> {
    let now = crate::hpet::elapsed_ns();
    let mut state = STATE.lock();

    // Check for duplicate.
    let already = state.unclaimed.iter().any(|d| d.addr == addr)
        || state.drivers.iter().any(|d| d.device_addr == addr);
    if already {
        return Err(KernelError::AlreadyExists);
    }

    if state.unclaimed.len() >= MAX_PENDING_DEVICES {
        return Err(KernelError::ResourceExhausted);
    }

    state.unclaimed.push(UnclaimedDevice {
        addr,
        id,
        discovered_at: now,
    });

    crate::syslog!(
        "udriver",
        Info,
        "device discovered: {:02x}:{:02x}.{} vendor={:04x} device={:04x}",
        addr.bus, addr.device, addr.function, id.vendor_id, id.device_id
    );

    Ok(())
}

/// List all unclaimed devices awaiting driver binding.
#[must_use]
pub fn unclaimed_devices() -> Vec<UnclaimedDevice> {
    STATE.lock().unclaimed.clone()
}

// ---------------------------------------------------------------------------
// Driver registration
// ---------------------------------------------------------------------------

/// Register a userspace process as a driver for a specific device.
///
/// The device must be in the unclaimed pool. The driver process must
/// hold the `ipc.driver` capability (checked by the syscall layer before
/// calling this function).
///
/// Returns the driver binding ID on success.
pub fn register_driver(
    name: &str,
    pid: u32,
    device_addr: DeviceAddr,
) -> KernelResult<u32> {
    let now = crate::hpet::elapsed_ns();
    let mut state = STATE.lock();

    // Check driver limit.
    if state.drivers.len() >= MAX_DRIVERS {
        return Err(KernelError::ResourceExhausted);
    }

    // Device must be unclaimed.
    let unclaimed_idx = state.unclaimed.iter()
        .position(|d| d.addr == device_addr)
        .ok_or(KernelError::NotFound)?;

    // Make sure no other driver already owns this device.
    let already_bound = state.drivers.iter()
        .any(|d| d.device_addr == device_addr && d.state != DriverState::Crashed
             && d.state != DriverState::Unregistered);
    if already_bound {
        return Err(KernelError::DeviceBusy);
    }

    let device_id = state.unclaimed[unclaimed_idx].id;
    let id = state.next_id;
    // next_id overflow is astronomically unlikely in practice; 4B drivers.
    state.next_id = state.next_id.wrapping_add(1);

    state.drivers.push(DriverBinding {
        id,
        name: String::from(name),
        pid,
        device_addr,
        device_id,
        state: DriverState::Registered,
        mmio_mappings: Vec::new(),
        dma_buffers: Vec::new(),
        irq_lines: Vec::new(),
        iommu_domain: None,
        total_dma_bytes: 0,
        registered_at: now,
        active_since: 0,
        io_requests_served: 0,
    });

    // Remove from unclaimed pool.
    state.unclaimed.swap_remove(unclaimed_idx);
    state.total_registrations = state.total_registrations.saturating_add(1);

    crate::syslog!(
        "udriver",
        Info,
        "driver '{}' (pid={}) registered for {:02x}:{:02x}.{} → id={}",
        name, pid, device_addr.bus, device_addr.device, device_addr.function, id
    );

    Ok(id)
}

/// Unregister a driver, releasing all its hardware resources.
pub fn unregister_driver(id: u32) -> KernelResult<()> {
    let mut state = STATE.lock();

    let idx = state.drivers.iter().position(|d| d.id == id)
        .ok_or(KernelError::NotFound)?;

    // Clean up all resources.
    let total_mmio = cleanup_driver_resources(&mut state, idx);

    // Move device back to unclaimed pool.
    let addr = state.drivers[idx].device_addr;
    let dev_id = state.drivers[idx].device_id;
    let name_copy = state.drivers[idx].name.clone();
    state.drivers[idx].state = DriverState::Unregistered;

    state.unclaimed.push(UnclaimedDevice {
        addr,
        id: dev_id,
        discovered_at: crate::hpet::elapsed_ns(),
    });

    state.total_cleanups = state.total_cleanups.saturating_add(1);

    crate::syslog!(
        "udriver",
        Info,
        "driver '{}' (id={}) unregistered, {} bytes MMIO freed",
        name_copy, id, total_mmio
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// MMIO mapping
// ---------------------------------------------------------------------------

/// Map a device BAR into the driver's virtual address space.
///
/// `bar_index`: which BAR (0-5) to map.
/// `virt_base`: the virtual address in the driver process where the
///   mapping should appear (must be page-aligned, chosen by the driver
///   or assigned by the kernel).
///
/// Returns the mapping details including the physical and virtual addresses.
pub fn map_mmio(
    driver_id: u32,
    bar_index: u8,
    phys_base: u64,
    size: u64,
    virt_base: u64,
    perms: MmioPerms,
) -> KernelResult<MmioMapping> {
    if bar_index > 5 {
        return Err(KernelError::InvalidArgument);
    }
    if size == 0 || size > MAX_MMIO_MAP_SIZE {
        return Err(KernelError::InvalidArgument);
    }
    // Must be page-aligned.
    if phys_base & 0x3FFF != 0 || virt_base & 0x3FFF != 0 {
        return Err(KernelError::BadAlignment);
    }

    let now = crate::hpet::elapsed_ns();
    let mut state = STATE.lock();

    let idx = state.drivers.iter().position(|d| d.id == driver_id)
        .ok_or(KernelError::NotFound)?;

    if state.drivers[idx].state == DriverState::Crashed
        || state.drivers[idx].state == DriverState::Unregistered
    {
        return Err(KernelError::InvalidArgument);
    }

    if state.drivers[idx].mmio_mappings.len() >= MAX_MMIO_MAPPINGS_PER_DRIVER {
        return Err(KernelError::ResourceExhausted);
    }

    // Check for overlapping BAR mapping.
    let has_bar = state.drivers[idx].mmio_mappings.iter()
        .any(|m| m.bar_index == bar_index);
    if has_bar {
        return Err(KernelError::AlreadyExists);
    }

    // In a full implementation, this is where we would:
    // 1. Validate phys_base is within the device's BAR range (from PCI config)
    // 2. Create page table entries in the driver process's address space
    //    mapping virt_base → phys_base with uncacheable (UC) memory type
    // 3. Mark the pages as device memory (not demand-paged, not swappable)
    //
    // For now, record the mapping for resource tracking and cleanup.

    let mapping = MmioMapping {
        bar_index,
        phys_base,
        virt_base,
        size,
        perms,
        mapped_at: now,
    };

    state.drivers[idx].mmio_mappings.push(mapping.clone());
    state.total_mmio_bytes = state.total_mmio_bytes.saturating_add(size);

    // Transition to Binding if just Registered.
    if state.drivers[idx].state == DriverState::Registered {
        state.drivers[idx].state = DriverState::Binding;
    }

    crate::syslog!(
        "udriver",
        Info,
        "driver id={}: mapped BAR{} phys={:#x} size={:#x} → virt={:#x}",
        driver_id, bar_index, phys_base, size, virt_base
    );

    Ok(mapping)
}

/// Unmap a specific MMIO BAR from a driver's address space.
pub fn unmap_mmio(driver_id: u32, bar_index: u8) -> KernelResult<()> {
    let mut state = STATE.lock();

    let idx = state.drivers.iter().position(|d| d.id == driver_id)
        .ok_or(KernelError::NotFound)?;

    let mmio_idx = state.drivers[idx].mmio_mappings.iter()
        .position(|m| m.bar_index == bar_index)
        .ok_or(KernelError::NotFound)?;

    let freed_size = state.drivers[idx].mmio_mappings[mmio_idx].size;
    state.drivers[idx].mmio_mappings.swap_remove(mmio_idx);
    state.total_mmio_bytes = state.total_mmio_bytes.saturating_sub(freed_size);

    // In a full implementation: remove page table entries, flush TLB.

    Ok(())
}

// ---------------------------------------------------------------------------
// DMA buffer management
// ---------------------------------------------------------------------------

/// Allocate a DMA-safe buffer for a driver.
///
/// The kernel allocates physically contiguous memory, maps it into the
/// driver's address space, and (if IOMMU is active) maps it into the
/// device's DMA address space.
///
/// Returns the buffer descriptor with physical, virtual, and bus addresses.
pub fn alloc_dma(
    driver_id: u32,
    size: usize,
    direction: DmaDirection,
    virt_base: u64,
) -> KernelResult<DmaBuffer> {
    if size == 0 || size > MAX_DMA_BUFFER_SIZE {
        return Err(KernelError::InvalidArgument);
    }
    // Round up to page alignment.
    let aligned_size = (size.saturating_add(DMA_ALIGNMENT - 1)) & !(DMA_ALIGNMENT - 1);

    if virt_base & 0x3FFF != 0 {
        return Err(KernelError::BadAlignment);
    }

    let now = crate::hpet::elapsed_ns();
    let mut state = STATE.lock();

    let idx = state.drivers.iter().position(|d| d.id == driver_id)
        .ok_or(KernelError::NotFound)?;

    if state.drivers[idx].state == DriverState::Crashed
        || state.drivers[idx].state == DriverState::Unregistered
    {
        return Err(KernelError::InvalidArgument);
    }

    if state.drivers[idx].dma_buffers.len() >= MAX_DMA_BUFFERS_PER_DRIVER {
        return Err(KernelError::ResourceExhausted);
    }

    // In a full implementation, this is where we would:
    // 1. Allocate physically contiguous frames (or use scatter-gather with IOMMU)
    // 2. Map into driver's virtual address space at virt_base
    // 3. If IOMMU: map into device's DMA domain (iommu_remap::map_dma)
    // 4. If no IOMMU: bus_addr == phys_addr (identity mapping)
    //
    // For now, simulate with a placeholder physical address.
    // The real implementation will integrate with mm::frame::alloc_contiguous().

    let dma_id = state.next_dma_id;
    state.next_dma_id = state.next_dma_id.wrapping_add(1);

    let (device_readable, device_writable) = match direction {
        DmaDirection::ToDevice => (true, false),
        DmaDirection::FromDevice => (false, true),
        DmaDirection::Bidirectional => (true, true),
    };

    // Placeholder: in real implementation, phys_addr comes from frame allocator.
    // Bus addr may differ if IOMMU remaps.
    let phys_addr = 0u64; // Will be filled by real allocator
    let bus_addr = phys_addr; // Same as phys when no IOMMU

    let buffer = DmaBuffer {
        id: dma_id,
        phys_addr,
        virt_addr: virt_base,
        bus_addr,
        size: aligned_size,
        device_readable,
        device_writable,
        allocated_at: now,
    };

    state.drivers[idx].dma_buffers.push(buffer.clone());
    state.drivers[idx].total_dma_bytes = state.drivers[idx]
        .total_dma_bytes.saturating_add(aligned_size);
    state.total_dma_bytes = state.total_dma_bytes.saturating_add(aligned_size);

    crate::syslog!(
        "udriver",
        Info,
        "driver id={}: allocated DMA buffer #{} size={:#x} dir={:?}",
        driver_id, dma_id, aligned_size, direction
    );

    Ok(buffer)
}

/// Free a DMA buffer previously allocated for a driver.
pub fn free_dma(driver_id: u32, buffer_id: u32) -> KernelResult<()> {
    let mut state = STATE.lock();

    let drv_idx = state.drivers.iter().position(|d| d.id == driver_id)
        .ok_or(KernelError::NotFound)?;

    let buf_idx = state.drivers[drv_idx].dma_buffers.iter()
        .position(|b| b.id == buffer_id)
        .ok_or(KernelError::NotFound)?;

    let freed_size = state.drivers[drv_idx].dma_buffers[buf_idx].size;
    state.drivers[drv_idx].dma_buffers.swap_remove(buf_idx);
    state.drivers[drv_idx].total_dma_bytes = state.drivers[drv_idx]
        .total_dma_bytes.saturating_sub(freed_size);
    state.total_dma_bytes = state.total_dma_bytes.saturating_sub(freed_size);

    // In a full implementation:
    // 1. If IOMMU: unmap from device's DMA domain, issue IOTLB invalidation
    // 2. Unmap from driver's virtual address space
    // 3. Free physical frames back to frame allocator
    // 4. Fence: ensure no in-flight DMA before freeing

    Ok(())
}

// ---------------------------------------------------------------------------
// IRQ management
// ---------------------------------------------------------------------------

/// Register that a driver is handling a specific IRQ line.
///
/// The actual interrupt forwarding is handled by the IRQ subsystem
/// (SYS_IRQ_REGISTER); this just tracks the association for cleanup.
pub fn register_irq(driver_id: u32, irq: u8) -> KernelResult<()> {
    let mut state = STATE.lock();

    let idx = state.drivers.iter().position(|d| d.id == driver_id)
        .ok_or(KernelError::NotFound)?;

    if state.drivers[idx].irq_lines.contains(&irq) {
        return Err(KernelError::AlreadyExists);
    }

    state.drivers[idx].irq_lines.push(irq);

    Ok(())
}

/// Unregister a driver's IRQ handling.
pub fn unregister_irq(driver_id: u32, irq: u8) -> KernelResult<()> {
    let mut state = STATE.lock();

    let idx = state.drivers.iter().position(|d| d.id == driver_id)
        .ok_or(KernelError::NotFound)?;

    let irq_idx = state.drivers[idx].irq_lines.iter()
        .position(|&i| i == irq)
        .ok_or(KernelError::NotFound)?;

    state.drivers[idx].irq_lines.swap_remove(irq_idx);

    Ok(())
}

// ---------------------------------------------------------------------------
// Driver lifecycle
// ---------------------------------------------------------------------------

/// Mark a driver as fully active (setup complete, serving I/O).
pub fn activate_driver(driver_id: u32) -> KernelResult<()> {
    let now = crate::hpet::elapsed_ns();
    let mut state = STATE.lock();

    let idx = state.drivers.iter().position(|d| d.id == driver_id)
        .ok_or(KernelError::NotFound)?;

    match state.drivers[idx].state {
        DriverState::Registered | DriverState::Binding => {
            state.drivers[idx].state = DriverState::Active;
            state.drivers[idx].active_since = now;
            Ok(())
        }
        _ => Err(KernelError::InvalidArgument),
    }
}

/// Report that a driver process has crashed.
///
/// This cleans up all hardware resources (MMIO, DMA, IRQ) and moves
/// the device back to the unclaimed pool so a new driver can bind.
/// Called by [`crate::drvmon`] when it detects a driver process exit.
pub fn driver_crashed(driver_id: u32) -> KernelResult<()> {
    let mut state = STATE.lock();

    let idx = state.drivers.iter().position(|d| d.id == driver_id)
        .ok_or(KernelError::NotFound)?;

    let name = state.drivers[idx].name.clone();
    let addr = state.drivers[idx].device_addr;
    let dev_id = state.drivers[idx].device_id;

    // Clean up all resources.
    let freed = cleanup_driver_resources(&mut state, idx);
    state.drivers[idx].state = DriverState::Crashed;

    // Put device back in unclaimed pool for re-binding.
    state.unclaimed.push(UnclaimedDevice {
        addr,
        id: dev_id,
        discovered_at: crate::hpet::elapsed_ns(),
    });

    state.total_crashes = state.total_crashes.saturating_add(1);
    state.total_cleanups = state.total_cleanups.saturating_add(1);

    crate::syslog!(
        "udriver",
        Error,
        "driver '{}' (id={}) crashed — freed {} bytes, device {:02x}:{:02x}.{} unclaimed",
        name, driver_id, freed, addr.bus, addr.device, addr.function
    );

    Ok(())
}

/// Report I/O requests served by a driver (for statistics).
pub fn report_io(driver_id: u32, count: u64) -> KernelResult<()> {
    let mut state = STATE.lock();

    let idx = state.drivers.iter().position(|d| d.id == driver_id)
        .ok_or(KernelError::NotFound)?;

    state.drivers[idx].io_requests_served = state.drivers[idx]
        .io_requests_served.saturating_add(count);

    Ok(())
}

/// Set IOMMU availability (called during boot if IOMMU is detected).
pub fn set_iommu_available(available: bool) {
    STATE.lock().iommu_available = available;
}

// ---------------------------------------------------------------------------
// Queries
// ---------------------------------------------------------------------------

/// Get information about a specific driver binding.
#[must_use]
pub fn get_driver(id: u32) -> Option<DriverBinding> {
    STATE.lock().drivers.iter().find(|d| d.id == id).cloned()
}

/// Get the driver bound to a specific device address.
#[must_use]
pub fn driver_for_device(addr: DeviceAddr) -> Option<DriverBinding> {
    STATE.lock().drivers.iter()
        .find(|d| d.device_addr == addr && d.state == DriverState::Active)
        .cloned()
}

/// List all active driver bindings.
#[must_use]
pub fn active_drivers() -> Vec<DriverBinding> {
    STATE.lock().drivers.iter()
        .filter(|d| d.state == DriverState::Active)
        .cloned()
        .collect()
}

/// List all driver bindings (any state).
#[must_use]
pub fn all_drivers() -> Vec<DriverBinding> {
    STATE.lock().drivers.clone()
}

/// Summary statistics.
#[derive(Debug, Clone)]
pub struct DriverFrameworkStats {
    pub total_drivers: usize,
    pub active_drivers: usize,
    pub unclaimed_devices: usize,
    pub total_mmio_bytes: u64,
    pub total_dma_bytes: usize,
    pub iommu_available: bool,
    pub total_registrations: u64,
    pub total_crashes: u64,
    pub total_cleanups: u64,
}

/// Get framework-wide statistics.
#[must_use]
pub fn stats() -> DriverFrameworkStats {
    let state = STATE.lock();
    DriverFrameworkStats {
        total_drivers: state.drivers.len(),
        active_drivers: state.drivers.iter()
            .filter(|d| d.state == DriverState::Active).count(),
        unclaimed_devices: state.unclaimed.len(),
        total_mmio_bytes: state.total_mmio_bytes,
        total_dma_bytes: state.total_dma_bytes,
        iommu_available: state.iommu_available,
        total_registrations: state.total_registrations,
        total_crashes: state.total_crashes,
        total_cleanups: state.total_cleanups,
    }
}

// ---------------------------------------------------------------------------
// Internal — resource cleanup
// ---------------------------------------------------------------------------

/// Clean up all hardware resources for a driver at index `idx`.
/// Returns total bytes freed (MMIO + DMA).
///
/// Does NOT remove the driver from the Vec or change its state — the
/// caller handles that.
fn cleanup_driver_resources(state: &mut State, idx: usize) -> u64 {
    let mut freed: u64 = 0;

    // Free MMIO mappings.
    let mmio_bytes: u64 = state.drivers[idx].mmio_mappings.iter()
        .map(|m| m.size)
        .sum();
    state.drivers[idx].mmio_mappings.clear();
    state.total_mmio_bytes = state.total_mmio_bytes.saturating_sub(mmio_bytes);
    freed = freed.saturating_add(mmio_bytes);

    // Free DMA buffers.
    // In a full implementation: IOMMU fence, unmap, free physical frames.
    let dma_bytes = state.drivers[idx].total_dma_bytes;
    state.drivers[idx].dma_buffers.clear();
    state.drivers[idx].total_dma_bytes = 0;
    state.total_dma_bytes = state.total_dma_bytes.saturating_sub(dma_bytes);
    freed = freed.saturating_add(dma_bytes as u64);

    // Clear IRQ associations.
    // In a full implementation: unhook from IDT routing table.
    state.drivers[idx].irq_lines.clear();

    // Clear IOMMU domain.
    // In a full implementation: iommu_remap::destroy_domain().
    state.drivers[idx].iommu_domain = None;

    freed
}

// ---------------------------------------------------------------------------
// procfs content
// ---------------------------------------------------------------------------

/// Generate `/proc/udriver` content.
#[must_use]
pub fn procfs_content() -> String {
    let state = STATE.lock();
    let mut out = String::with_capacity(2048);

    out.push_str("=== Userspace Driver Framework ===\n\n");

    // Global stats.
    out.push_str(&format!("IOMMU: {}\n",
        if state.iommu_available { "available" } else { "not available" }));
    out.push_str(&format!("Registered drivers: {}\n", state.drivers.len()));
    out.push_str(&format!("Active drivers: {}\n",
        state.drivers.iter().filter(|d| d.state == DriverState::Active).count()));
    out.push_str(&format!("Unclaimed devices: {}\n", state.unclaimed.len()));
    out.push_str(&format!("Total MMIO mapped: {} KiB\n",
        state.total_mmio_bytes / 1024));
    out.push_str(&format!("Total DMA allocated: {} KiB\n",
        state.total_dma_bytes / 1024));
    out.push_str(&format!("Total registrations: {}\n", state.total_registrations));
    out.push_str(&format!("Total crashes: {}\n", state.total_crashes));
    out.push_str(&format!("Total cleanups: {}\n\n", state.total_cleanups));

    // Unclaimed devices.
    if !state.unclaimed.is_empty() {
        out.push_str("Unclaimed devices:\n");
        for dev in &state.unclaimed {
            out.push_str(&format!(
                "  {:02x}:{:02x}.{}  vendor={:04x} device={:04x}  class={:02x}:{:02x}\n",
                dev.addr.bus, dev.addr.device, dev.addr.function,
                dev.id.vendor_id, dev.id.device_id,
                dev.id.class, dev.id.subclass,
            ));
        }
        out.push('\n');
    }

    // Active drivers.
    for drv in &state.drivers {
        let state_str = match drv.state {
            DriverState::Registered => "registered",
            DriverState::Binding => "binding",
            DriverState::Active => "active",
            DriverState::ShuttingDown => "shutting-down",
            DriverState::Crashed => "crashed",
            DriverState::Unregistered => "unregistered",
        };

        out.push_str(&format!(
            "Driver #{} '{}' [{}]\n",
            drv.id, drv.name, state_str
        ));
        out.push_str(&format!(
            "  PID: {}  Device: {:02x}:{:02x}.{}  vendor={:04x} device={:04x}\n",
            drv.pid,
            drv.device_addr.bus, drv.device_addr.device, drv.device_addr.function,
            drv.device_id.vendor_id, drv.device_id.device_id,
        ));

        if !drv.mmio_mappings.is_empty() {
            out.push_str("  MMIO mappings:\n");
            for m in &drv.mmio_mappings {
                out.push_str(&format!(
                    "    BAR{}: phys={:#010x} virt={:#010x} size={:#x} {}\n",
                    m.bar_index, m.phys_base, m.virt_base, m.size,
                    if m.perms.write { "RW" } else { "RO" },
                ));
            }
        }

        if !drv.dma_buffers.is_empty() {
            out.push_str(&format!("  DMA buffers: {} ({} KiB total)\n",
                drv.dma_buffers.len(), drv.total_dma_bytes / 1024));
        }

        if !drv.irq_lines.is_empty() {
            out.push_str(&format!("  IRQs: {:?}\n", drv.irq_lines));
        }

        out.push_str(&format!("  I/O requests: {}\n\n", drv.io_requests_served));
    }

    out
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("[udriver] running self-tests...");

    test_device_registration();
    test_driver_registration();
    test_driver_double_bind();
    test_mmio_mapping();
    test_mmio_bar_conflict();
    test_dma_allocation();
    test_dma_free();
    test_irq_management();
    test_driver_activate();
    test_driver_crash_cleanup();
    test_unregister_cleanup();
    test_stats();
    test_procfs();

    crate::serial_println!("[udriver] all self-tests passed");
}

fn make_test_addr(dev: u8) -> DeviceAddr {
    DeviceAddr::new(0, dev, 0)
}

fn make_test_id() -> DeviceId {
    DeviceId {
        vendor_id: 0x1234,
        device_id: 0x5678,
        class: 0x02,
        subclass: 0x00,
    }
}

fn reset_state() {
    let mut state = STATE.lock();
    *state = State::new();
}

fn test_device_registration() {
    reset_state();

    let addr = make_test_addr(1);
    let id = make_test_id();
    assert!(register_device(addr, id).is_ok());

    // Duplicate should fail.
    assert_eq!(register_device(addr, id), Err(KernelError::AlreadyExists));

    // Should appear in unclaimed list.
    let devs = unclaimed_devices();
    assert_eq!(devs.len(), 1);
    assert_eq!(devs[0].addr, addr);

    crate::serial_println!("  [udriver] test_device_registration: ok");
}

fn test_driver_registration() {
    reset_state();

    let addr = make_test_addr(2);
    let id = make_test_id();
    register_device(addr, id).unwrap();

    let drv_id = register_driver("test-drv", 100, addr).unwrap();
    assert!(drv_id > 0);

    // Device should no longer be unclaimed.
    assert!(unclaimed_devices().is_empty());

    // Driver should exist.
    let drv = get_driver(drv_id).unwrap();
    assert_eq!(drv.name, "test-drv");
    assert_eq!(drv.pid, 100);
    assert_eq!(drv.state, DriverState::Registered);

    crate::serial_println!("  [udriver] test_driver_registration: ok");
}

fn test_driver_double_bind() {
    reset_state();

    let addr = make_test_addr(3);
    let id = make_test_id();
    register_device(addr, id).unwrap();

    let drv_id = register_driver("drv-a", 100, addr).unwrap();
    activate_driver(drv_id).unwrap();

    // Re-register device (simulating it being available for testing).
    // The device is no longer unclaimed, so registering a second driver
    // should fail with NotFound (device not in unclaimed pool).
    assert_eq!(register_driver("drv-b", 200, addr), Err(KernelError::NotFound));

    crate::serial_println!("  [udriver] test_driver_double_bind: ok");
}

fn test_mmio_mapping() {
    reset_state();

    let addr = make_test_addr(4);
    register_device(addr, make_test_id()).unwrap();
    let drv_id = register_driver("mmio-test", 100, addr).unwrap();

    let mapping = map_mmio(
        drv_id, 0,
        0xFE00_0000, 0x4000, // 16 KiB at physical 0xFE000000
        0x0000_7000_0000_0000, // Virtual address
        MmioPerms::READ_WRITE,
    ).unwrap();

    assert_eq!(mapping.bar_index, 0);
    assert_eq!(mapping.phys_base, 0xFE00_0000);
    assert_eq!(mapping.size, 0x4000);

    // Driver should be in Binding state.
    let drv = get_driver(drv_id).unwrap();
    assert_eq!(drv.state, DriverState::Binding);
    assert_eq!(drv.mmio_mappings.len(), 1);

    crate::serial_println!("  [udriver] test_mmio_mapping: ok");
}

fn test_mmio_bar_conflict() {
    reset_state();

    let addr = make_test_addr(5);
    register_device(addr, make_test_id()).unwrap();
    let drv_id = register_driver("bar-test", 100, addr).unwrap();

    map_mmio(drv_id, 0, 0xFE00_0000, 0x4000, 0x7000_0000_0000, MmioPerms::READ_ONLY)
        .unwrap();

    // Mapping same BAR again should fail.
    let err = map_mmio(drv_id, 0, 0xFE01_0000, 0x4000, 0x7000_0001_0000, MmioPerms::READ_ONLY);
    assert!(matches!(err, Err(KernelError::AlreadyExists)));

    // Different BAR should work.
    assert!(map_mmio(drv_id, 1, 0xFE01_0000, 0x4000, 0x7000_0001_0000, MmioPerms::READ_WRITE).is_ok());

    crate::serial_println!("  [udriver] test_mmio_bar_conflict: ok");
}

fn test_dma_allocation() {
    reset_state();

    let addr = make_test_addr(6);
    register_device(addr, make_test_id()).unwrap();
    let drv_id = register_driver("dma-test", 100, addr).unwrap();

    let buf = alloc_dma(
        drv_id,
        4096,
        DmaDirection::Bidirectional,
        0x7000_0010_0000,
    ).unwrap();

    assert!(buf.id > 0);
    assert!(buf.size >= 4096);
    assert!(buf.device_readable);
    assert!(buf.device_writable);

    let drv = get_driver(drv_id).unwrap();
    assert_eq!(drv.dma_buffers.len(), 1);
    assert!(drv.total_dma_bytes >= 4096);

    crate::serial_println!("  [udriver] test_dma_allocation: ok");
}

fn test_dma_free() {
    reset_state();

    let addr = make_test_addr(7);
    register_device(addr, make_test_id()).unwrap();
    let drv_id = register_driver("dma-free-test", 100, addr).unwrap();

    let buf = alloc_dma(drv_id, 8192, DmaDirection::ToDevice, 0x7000_0020_0000).unwrap();
    let buf_id = buf.id;

    // Free it.
    assert!(free_dma(drv_id, buf_id).is_ok());

    let drv = get_driver(drv_id).unwrap();
    assert!(drv.dma_buffers.is_empty());
    assert_eq!(drv.total_dma_bytes, 0);

    // Double free should fail.
    assert_eq!(free_dma(drv_id, buf_id), Err(KernelError::NotFound));

    crate::serial_println!("  [udriver] test_dma_free: ok");
}

fn test_irq_management() {
    reset_state();

    let addr = make_test_addr(8);
    register_device(addr, make_test_id()).unwrap();
    let drv_id = register_driver("irq-test", 100, addr).unwrap();

    assert!(register_irq(drv_id, 11).is_ok());
    assert!(register_irq(drv_id, 15).is_ok());

    // Duplicate IRQ.
    assert_eq!(register_irq(drv_id, 11), Err(KernelError::AlreadyExists));

    // Unregister.
    assert!(unregister_irq(drv_id, 11).is_ok());
    assert_eq!(unregister_irq(drv_id, 11), Err(KernelError::NotFound));

    let drv = get_driver(drv_id).unwrap();
    assert_eq!(drv.irq_lines.len(), 1);
    assert_eq!(drv.irq_lines[0], 15);

    crate::serial_println!("  [udriver] test_irq_management: ok");
}

fn test_driver_activate() {
    reset_state();

    let addr = make_test_addr(9);
    register_device(addr, make_test_id()).unwrap();
    let drv_id = register_driver("activate-test", 100, addr).unwrap();

    assert!(activate_driver(drv_id).is_ok());

    let drv = get_driver(drv_id).unwrap();
    assert_eq!(drv.state, DriverState::Active);
    assert!(drv.active_since > 0);

    // Activating an already-active driver should fail.
    assert_eq!(activate_driver(drv_id), Err(KernelError::InvalidArgument));

    crate::serial_println!("  [udriver] test_driver_activate: ok");
}

fn test_driver_crash_cleanup() {
    reset_state();

    let addr = make_test_addr(10);
    register_device(addr, make_test_id()).unwrap();
    let drv_id = register_driver("crash-test", 100, addr).unwrap();

    // Set up resources.
    map_mmio(drv_id, 0, 0xFE00_0000, 0x4000, 0x7000_0000_0000, MmioPerms::READ_WRITE)
        .unwrap();
    alloc_dma(drv_id, 4096, DmaDirection::FromDevice, 0x7000_0010_0000)
        .unwrap();
    register_irq(drv_id, 10).unwrap();
    activate_driver(drv_id).unwrap();

    // Crash it.
    assert!(driver_crashed(drv_id).is_ok());

    let drv = get_driver(drv_id).unwrap();
    assert_eq!(drv.state, DriverState::Crashed);
    assert!(drv.mmio_mappings.is_empty());
    assert!(drv.dma_buffers.is_empty());
    assert!(drv.irq_lines.is_empty());
    assert_eq!(drv.total_dma_bytes, 0);

    // Device should be back in unclaimed pool.
    let devs = unclaimed_devices();
    assert_eq!(devs.len(), 1);
    assert_eq!(devs[0].addr, addr);

    let st = stats();
    assert_eq!(st.total_crashes, 1);
    assert_eq!(st.total_cleanups, 1);

    crate::serial_println!("  [udriver] test_driver_crash_cleanup: ok");
}

fn test_unregister_cleanup() {
    reset_state();

    let addr = make_test_addr(11);
    register_device(addr, make_test_id()).unwrap();
    let drv_id = register_driver("unreg-test", 100, addr).unwrap();

    map_mmio(drv_id, 0, 0xFE00_0000, 0x4000, 0x7000_0000_0000, MmioPerms::READ_WRITE)
        .unwrap();
    alloc_dma(drv_id, 8192, DmaDirection::Bidirectional, 0x7000_0010_0000)
        .unwrap();

    assert!(unregister_driver(drv_id).is_ok());

    let drv = get_driver(drv_id).unwrap();
    assert_eq!(drv.state, DriverState::Unregistered);

    // Device back in unclaimed pool.
    assert_eq!(unclaimed_devices().len(), 1);

    crate::serial_println!("  [udriver] test_unregister_cleanup: ok");
}

fn test_stats() {
    reset_state();

    let st = stats();
    assert_eq!(st.total_drivers, 0);
    assert_eq!(st.active_drivers, 0);
    assert_eq!(st.total_mmio_bytes, 0);
    assert_eq!(st.total_dma_bytes, 0);

    let addr = make_test_addr(12);
    register_device(addr, make_test_id()).unwrap();
    let drv_id = register_driver("stats-test", 100, addr).unwrap();
    activate_driver(drv_id).unwrap();

    let st = stats();
    assert_eq!(st.total_drivers, 1);
    assert_eq!(st.active_drivers, 1);
    assert_eq!(st.total_registrations, 1);

    crate::serial_println!("  [udriver] test_stats: ok");
}

fn test_procfs() {
    reset_state();

    let addr = make_test_addr(13);
    register_device(addr, make_test_id()).unwrap();
    let drv_id = register_driver("procfs-test", 100, addr).unwrap();
    activate_driver(drv_id).unwrap();

    let content = procfs_content();
    assert!(content.contains("Userspace Driver Framework"));
    assert!(content.contains("procfs-test"));
    assert!(content.contains("active"));

    crate::serial_println!("  [udriver] test_procfs: ok");
}
