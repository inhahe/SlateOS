//! What a machine's hardware is, as the kernel publishes it.
//!
//! The types System Information shows and the Device Manager lists, and the
//! reader that fills them ([`SyscallProvider`] behind [`HardwareProvider`]): the CPU, memory, block devices, PCI functions
//! and the memory map from `/sys/devices`, the rest from `/proc`. Both
//! programs read through this one reader, so they cannot disagree about the
//! machine -- and a reader fixed here is fixed for both.
//!
//! Lifted out of `apps/sysinfo`, where it was the `hwquery` module, on
//! 2026-09-26, when the Device Manager began listing real devices instead of
//! none. (`hwinfo` would have been the name; `userspace/hwinfo`, a command,
//! has it.)

mod query;
pub use query::*;

/// CPU information.
#[derive(Clone, Debug)]
pub struct CpuInfo {
    /// The marketing name, if anything publishes one. Nothing does.
    ///
    /// The kernel serves CPUID leaf 1 -- family, model, stepping -- and not
    /// leaves 0x8000_0002..4, which are where a brand string lives. An empty
    /// `String` would draw as a processor with no name rather than as a
    /// question nobody answered, and the invented value it replaced was
    /// "Intel Core i7-13700K".
    pub brand: Option<String>,
    /// The vendor string, on the same terms as [`CpuInfo::brand`]: CPUID leaf
    /// 0, also not served.
    pub vendor: Option<String>,
    pub family: u32,
    pub model: u32,
    pub stepping: u32,
    pub physical_cores: u32,
    pub logical_processors: u32,
    /// Base clock, if a frequency source exists. None does.
    ///
    /// Lane A's `sysfs.rs` says why there is no `cpufreq/`, and the reasoning
    /// covers this field: *"a file reading 0 cannot be told from a real 0 MHz.
    /// Absent is the honest answer until a CPUID leaf-16h reader exists."*
    pub base_clock_mhz: Option<u32>,
    /// Turbo clock, on the same terms as [`CpuInfo::base_clock_mhz`].
    pub max_turbo_mhz: Option<u32>,
    /// Each cache's size, when the processor reports it (CPUID leaf 4).
    /// `None`, never 0: a 0 reads as a measurement, and "no L3" is a real
    /// answer some processors give.
    pub l1_data_kb: Option<u32>,
    pub l1_inst_kb: Option<u32>,
    pub l2_kb: Option<u32>,
    pub l3_kb: Option<u32>,
    pub features: Vec<(String, bool)>,
}

/// Memory slot information.
#[derive(Clone, Debug)]
pub struct MemorySlot {
    pub slot_name: String,
    pub size_mb: u32,
    pub mem_type: String,
    pub speed_mhz: u32,
    pub manufacturer: String,
}

/// Overall memory information.
#[derive(Clone, Debug)]
pub struct MemoryInfo {
    pub total_mb: u64,
    pub available_mb: u64,
    /// Empty when nothing reports it (it is an SMBIOS fact).
    pub mem_type: String,
    /// The memory's speed and its slots, when something reports them --
    /// SMBIOS does, and nothing here reads it. `None`, never a 0 that reads
    /// as a machine with no memory slots.
    pub speed_mhz: Option<u32>,
    pub slots_used: Option<u32>,
    pub slots_total: Option<u32>,
    pub slots: Vec<MemorySlot>,
}

/// Partition information.
///
/// Sizes are raw byte counts, not pre-scaled gigabytes. They used to be
/// `f32` fields named `*_gb` holding values divided by 1024³ and displayed
/// with a `GB` label — a 2 TB disk read `1863.0 GB`, which is neither its
/// capacity in GB (2000) nor a unit anyone sells. Keeping bytes and scaling
/// at the point of display means the divisor and the unit name are chosen
/// together, by `guitk::bytes`. See design-decisions.md §489.
#[derive(Clone, Debug)]
pub struct PartitionInfo {
    pub label: String,
    pub filesystem: String,
    pub capacity_bytes: u64,
    pub used_bytes: u64,
    pub free_bytes: u64,
    pub mount_point: String,
}

/// Disk information.
#[derive(Clone, Debug)]
pub struct DiskInfo {
    pub model: String,
    /// Raw byte count; see [`PartitionInfo`] for why this is not pre-scaled.
    pub capacity_bytes: u64,
    pub interface: String,
    pub serial: String,
    pub smart_status: String,
    pub partitions: Vec<PartitionInfo>,
}

/// One of an adapter's addresses, as far as anything says.
///
/// Three states, not two, because two different things leave an address
/// blank and a reader needs to tell them apart. "Nothing publishes this" is a
/// gap in what the system reports -- Linux's `/proc/net/dev` carries no
/// addresses at all. "The kernel says there is none" is a fact about the
/// machine -- SlateOS writes `0.0.0.0`, the unspecified address, until DHCP
/// has configured the interface -- and it is the answer to "why can nothing
/// be reached?". Shown as "Not reported" it would hide exactly that.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Address {
    /// Nothing read publishes this address.
    NotReported,
    /// It is published, and the kernel says there is none.
    Unassigned,
    /// The address, as the kernel wrote it.
    Is(String),
}

/// Network adapter information.
#[derive(Clone, Debug)]
pub struct NetworkAdapterInfo {
    pub name: String,
    pub adapter_type: String,
    /// Empty when nothing publishes one. SlateOS always does, for a real
    /// interface: an adapter whose MAC would be all zeros is the kernel's
    /// placeholder for "no network card was found", and is not listed.
    pub mac_address: String,
    pub ipv4: Address,
    pub ipv6: Address,
    pub subnet: Address,
    pub gateway: Address,
    pub dns: Address,
    /// Whether the link is up, when the kernel says (`/proc/net`).
    pub up: Option<bool>,
    /// The link speed, when something reports one -- nothing does yet.
    /// `None`, never 0, which would be a dead link.
    pub speed_mbps: Option<u32>,
    pub duplex: String,
    /// The counters, when published (`/proc/net/dev` on Linux). SlateOS's
    /// `/proc/net` has none, and `None` is not zero traffic.
    pub bytes_sent: Option<u64>,
    pub bytes_received: Option<u64>,
}

impl NetworkAdapterInfo {
    /// An adapter of which only the name is known: every other field reads
    /// as not reported, so what a source does publish is filled in over a
    /// blank rather than over a guess.
    #[must_use]
    pub fn named(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            adapter_type: String::new(),
            mac_address: String::new(),
            ipv4: Address::NotReported,
            ipv6: Address::NotReported,
            subnet: Address::NotReported,
            gateway: Address::NotReported,
            dns: Address::NotReported,
            up: None,
            speed_mbps: None,
            duplex: String::new(),
            bytes_sent: None,
            bytes_received: None,
        }
    }
}

/// Display/GPU information.
#[derive(Clone, Debug)]
pub struct DisplayInfo {
    pub gpu_name: String,
    pub vendor: String,
    /// The adapter's memory, when something reports it -- nothing does yet.
    pub vram_mb: Option<u32>,
    /// The primary output's mode, `WIDTHxHEIGHT`; empty when there is no
    /// primary output, rather than the first row's.
    pub resolution: String,
    /// The primary output's refresh rate, when there is a primary output.
    pub refresh_rate_hz: Option<u32>,
    pub outputs: Vec<(String, bool)>,
    pub driver_version: String,
}

/// PCI device entry.
#[derive(Clone, Debug)]
pub struct PciDeviceInfo {
    pub bus: u8,
    pub device: u8,
    pub function: u8,
    pub vendor_id: u16,
    pub device_id: u16,
    pub class: String,
    pub description: String,
    pub vendor_name: String,
    /// The base class and subclass codes, as the kernel publishes them --
    /// what `class` and `description` put in words. Kept as numbers for the
    /// programs that sort devices by kind, which should not have to parse
    /// the words back.
    pub class_code: Option<u8>,
    pub subclass_code: Option<u8>,
}

/// Service entry.
#[derive(Clone, Debug)]
pub struct ServiceInfo {
    pub name: String,
    pub status: String,
    pub start_type: String,
}

/// Process entry (for the sysinfo view).
#[derive(Clone, Debug)]
pub struct ProcessEntry {
    pub pid: u32,
    pub name: String,
    pub memory_kb: u64,
    pub cpu_percent: f32,
}

/// Driver entry.
#[derive(Clone, Debug)]
pub struct DriverInfo {
    pub name: String,
    pub path: String,
    pub status: String,
}

/// IRQ assignment.
#[derive(Clone, Debug)]
pub struct IrqInfo {
    pub irq_number: u32,
    pub device: String,
    /// Left empty when read from `/proc/interrupts`: the kernel publishes a
    /// label and a flag, and nothing that is a *type*. It used to default to
    /// `"Edge"`, which is a trigger mode nobody reported.
    pub irq_type: String,
    /// Whether the IOAPIC showed the line asserted at the moment of the read.
    ///
    /// **A sample, not a total, and never to be drawn as activity.** Reading
    /// `/proc/interrupts` twice can show `false` both times while thousands of
    /// interrupts were serviced in between, so a count synthesised from this
    /// would agree with itself forever *and* agree with the real one. That is
    /// worse than a wrong constant, which at least fails on a second look:
    /// **the observation that would falsify it is the observation that
    /// confirms it.** Lane B declined to synthesise the count in `procinfo`
    /// for this reason; the same answer applies one layer up. A rate needs the
    /// counters to exist first.
    pub asserted: bool,
}

/// I/O port range.
#[derive(Clone, Debug)]
pub struct IoPortInfo {
    pub start: u16,
    pub end: u16,
    pub device: String,
}

/// Memory map region.
#[derive(Clone, Debug)]
pub struct MemoryMapEntry {
    pub start: u64,
    pub end: u64,
    pub region_type: String,
    pub description: String,
}

/// DMA channel assignment.
#[derive(Clone, Debug)]
pub struct DmaInfo {
    pub channel: u8,
    pub device: String,
    pub mode: String,
}

/// USB device entry.
#[derive(Clone, Debug)]
pub struct UsbDeviceInfo {
    pub port: String,
    pub vendor_id: u16,
    pub product_id: u16,
    pub description: String,
    pub speed: String,
}

/// Sound device.
#[derive(Clone, Debug)]
pub struct SoundInfo {
    pub name: String,
    pub device_type: String,
    pub driver: String,
    pub status: String,
}

/// Startup program entry.
#[derive(Clone, Debug)]
pub struct StartupEntry {
    pub name: String,
    /// The command line the item runs. `/proc/autostart`'s COMMAND column.
    pub path: String,
    /// **When** it runs -- boot, login, session -- not where it came from.
    ///
    /// This field was `source`, and `source` in a startup manager means the
    /// place the entry was registered: a folder, a registry key, a unit file.
    /// `/proc/autostart` publishes no such thing. It publishes a PHASE, and
    /// putting a phase in a column meaning origin is the same defect as
    /// putting a bus *type* in a column meaning bus *number* -- which is why
    /// `/proc/devicemgr` was left unwired. Renamed rather than repurposed.
    pub phase: String,
    /// Whether the item is set to run at all.
    ///
    /// A disabled entry listed like an enabled one is a claim that it runs.
    /// The file says which, so the window can too.
    pub enabled: bool,
}
