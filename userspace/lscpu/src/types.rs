//! `lscpu.h`: what `lscpu` learns about the machine, and `lscpu-cpu.c`,
//! which makes the CPUs.
//!
//! Upstream's structures are reference-counted and point at each other; here
//! the context owns everything, a CPU names its type by index into
//! [`Cxt::cputypes`], and a CPU that `possible` counts but that was never
//! made is a `None` in [`Cxt::cpus`], as upstream's array has a `NULL`.

use crate::cpuset::CpuSet;
use crate::path::PathCxt;

/// `struct lscpu_cache`.
#[derive(Clone, Debug, Default)]
pub struct Cache {
    /// `id`: unique among the caches of its type and level.
    pub id: i32,
    /// `name`: `L1d`, `L2`, ...
    pub name: Option<Vec<u8>>,
    /// `type`: `Data`, `Instruction`, `Unified`.
    pub kind: Option<Vec<u8>>,
    pub allocation_policy: Option<Vec<u8>>,
    pub write_policy: Option<Vec<u8>>,
    pub level: i32,
    pub size: u64,
    pub ways_of_associativity: u32,
    pub physical_line_partition: u32,
    pub number_of_sets: u32,
    pub coherency_line_size: u32,
    /// `sharedmap`: the CPUs sharing this cache.
    pub sharedmap: Option<CpuSet>,
}

impl Cache {
    /// The name, as upstream's `strcmp` reads a `NULL` one (it never is).
    #[must_use]
    pub fn name_bytes(&self) -> &[u8] {
        self.name.as_deref().unwrap_or_default()
    }
}

/// `struct lscpu_cputype`.
#[derive(Clone, Debug)]
pub struct CpuType {
    pub vendor: Option<Vec<u8>>,
    /// `vendor_id`: the ARM implementer, parsed once (0 until then).
    pub vendor_id: i32,
    pub bios_vendor: Option<Vec<u8>>,
    pub machinetype: Option<Vec<u8>>,
    pub family: Option<Vec<u8>>,
    pub model: Option<Vec<u8>>,
    pub modelname: Option<Vec<u8>>,
    pub bios_modelname: Option<Vec<u8>>,
    pub bios_family: Option<Vec<u8>>,
    pub revision: Option<Vec<u8>>,
    pub stepping: Option<Vec<u8>>,
    pub bogomips: Option<Vec<u8>>,
    pub flags: Option<Vec<u8>>,
    pub mtid: Option<Vec<u8>>,
    pub addrsz: Option<Vec<u8>>,
    /// -1 when unknown, else `DISP_*`.
    pub dispatching: i32,
    /// -1 when unknown.
    pub freqboost: i32,
    pub physsockets: usize,
    pub physchips: usize,
    pub physcoresperchip: usize,
    pub nthreads_per_core: usize,
    pub ncores_per_socket: usize,
    pub nsockets_per_book: usize,
    pub nbooks_per_drawer: usize,
    pub ndrawers_per_system: usize,
    pub dynamic_mhz: Option<Vec<u8>>,
    pub static_mhz: Option<Vec<u8>>,
    /// The distinct `thread_siblings` sets: one per core.
    pub coremaps: Vec<CpuSet>,
    /// The distinct `core_siblings` sets: one per socket.
    pub socketmaps: Vec<CpuSet>,
    pub bookmaps: Vec<CpuSet>,
    pub drawermaps: Vec<CpuSet>,
    pub has_freq: bool,
    pub has_configured: bool,
    pub has_polarization: bool,
    pub has_addresses: bool,
    pub nr_socket_on_cluster: usize,
    pub isa: Option<Vec<u8>>,
}

impl CpuType {
    /// `lscpu_new_cputype()`.
    #[must_use]
    pub fn new() -> Self {
        CpuType {
            vendor: None,
            vendor_id: 0,
            bios_vendor: None,
            machinetype: None,
            family: None,
            model: None,
            modelname: None,
            bios_modelname: None,
            bios_family: None,
            revision: None,
            stepping: None,
            bogomips: None,
            flags: None,
            mtid: None,
            addrsz: None,
            dispatching: -1,
            freqboost: -1,
            physsockets: 0,
            physchips: 0,
            physcoresperchip: 0,
            nthreads_per_core: 0,
            ncores_per_socket: 0,
            nsockets_per_book: 0,
            nbooks_per_drawer: 0,
            ndrawers_per_system: 0,
            dynamic_mhz: None,
            static_mhz: None,
            coremaps: Vec::new(),
            socketmaps: Vec::new(),
            bookmaps: Vec::new(),
            drawermaps: Vec::new(),
            has_freq: false,
            has_configured: false,
            has_polarization: false,
            has_addresses: false,
            nr_socket_on_cluster: 0,
            isa: None,
        }
    }
}

impl Default for CpuType {
    fn default() -> Self {
        Self::new()
    }
}

/// `POLAR_*`.
pub const POLAR_UNKNOWN: i32 = 0;
pub const POLAR_VLOW: i32 = 1;
pub const POLAR_VMEDIUM: i32 = 2;
pub const POLAR_VHIGH: i32 = 3;
pub const POLAR_HORIZONTAL: i32 = 4;

/// `struct lscpu_cpu`.
#[derive(Clone, Debug)]
pub struct Cpu {
    /// Index into [`Cxt::cputypes`].
    pub cputype: Option<usize>,
    pub logical_id: i32,
    pub bogomips: Option<Vec<u8>>,
    pub mhz: Option<Vec<u8>>,
    pub dynamic_mhz: Option<Vec<u8>>,
    pub static_mhz: Option<Vec<u8>>,
    pub mhz_max_freq: f32,
    pub mhz_min_freq: f32,
    pub mhz_cur_freq: f32,
    pub coreid: i32,
    pub socketid: i32,
    pub bookid: i32,
    pub drawerid: i32,
    pub polarization: i32,
    pub address: i32,
    pub configured: i32,
}

impl Cpu {
    /// `lscpu_new_cpu(id)`. Upstream sets `bookid` to -1 twice and
    /// `drawerid` never, so a CPU without a `topology` directory is on
    /// drawer 0 -- which `-p -y` shows.
    #[must_use]
    pub fn new(id: i32) -> Self {
        Cpu {
            cputype: None,
            logical_id: id,
            bogomips: None,
            mhz: None,
            dynamic_mhz: None,
            static_mhz: None,
            mhz_max_freq: 0.0,
            mhz_min_freq: 0.0,
            mhz_cur_freq: 0.0,
            coreid: -1,
            socketid: -1,
            bookid: -1,
            drawerid: 0,
            polarization: POLAR_UNKNOWN,
            address: -1,
            configured: -1,
        }
    }

    /// The CPU's number as `CPU_ISSET_S` takes it.
    #[must_use]
    pub fn index(&self) -> usize {
        usize::try_from(self.logical_id).unwrap_or(usize::MAX)
    }
}

/// `struct lscpu_arch`.
#[derive(Clone, Debug, Default)]
pub struct Arch {
    /// `uname().machine`.
    pub name: Vec<u8>,
    pub bit32: bool,
    pub bit64: bool,
}

/// `struct lscpu_vulnerability`.
#[derive(Clone, Debug)]
pub struct Vulnerability {
    pub name: Vec<u8>,
    pub text: Vec<u8>,
}

/// `VIRT_TYPE_*`.
pub const VIRT_TYPE_NONE: usize = 0;
pub const VIRT_TYPE_PARA: usize = 1;
pub const VIRT_TYPE_FULL: usize = 2;
pub const VIRT_TYPE_CONTAINER: usize = 3;

/// `VIRT_VENDOR_*`.
pub const VIRT_VENDOR_NONE: usize = 0;
pub const VIRT_VENDOR_XEN: usize = 1;
pub const VIRT_VENDOR_KVM: usize = 2;
pub const VIRT_VENDOR_MSHV: usize = 3;
pub const VIRT_VENDOR_VMWARE: usize = 4;
pub const VIRT_VENDOR_IBM: usize = 5;
pub const VIRT_VENDOR_VSERVER: usize = 6;
pub const VIRT_VENDOR_UML: usize = 7;
pub const VIRT_VENDOR_INNOTEK: usize = 8;
pub const VIRT_VENDOR_HITACHI: usize = 9;
pub const VIRT_VENDOR_PARALLELS: usize = 10;
pub const VIRT_VENDOR_VBOX: usize = 11;
pub const VIRT_VENDOR_OS400: usize = 12;
pub const VIRT_VENDOR_PHYP: usize = 13;
pub const VIRT_VENDOR_SPAR: usize = 14;
pub const VIRT_VENDOR_WSL: usize = 15;

/// `struct lscpu_virt`.
#[derive(Clone, Debug, Default)]
pub struct Virt {
    /// `svm` or `vmx`.
    pub cpuflag: Option<&'static str>,
    pub hypervisor: Option<Vec<u8>>,
    /// `VIRT_VENDOR_*`.
    pub vendor: usize,
    /// `VIRT_TYPE_*`.
    pub kind: usize,
}

/// `LSCPU_OUTPUT_*`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Summary,
    Caches,
    Parsable,
    Readable,
}

/// A NUMA node: `idx2nodenum[i]` and `nodemaps[i]`.
#[derive(Clone, Debug)]
pub struct Node {
    pub num: i32,
    /// `NULL` when its `cpumap` would not read.
    pub map: Option<CpuSet>,
}

/// `struct lscpu_cxt`.
#[derive(Debug)]
pub struct Cxt {
    /// `maxcpus`: bits in the kernel's CPU mask.
    pub maxcpus: i32,
    /// `prefix`: `--sysroot`.
    pub prefix: Option<Vec<u8>>,
    /// `syscpu`: `/sys/devices/system/cpu`.
    pub syscpu: PathCxt,
    /// `procfs`: `/proc`.
    pub procfs: PathCxt,
    /// `rootfs`: `/`, only with a prefix.
    pub rootfs: Option<PathCxt>,
    pub cputypes: Vec<CpuType>,
    /// The possible CPUs, `npossibles` of them; `None` for one counted but
    /// never made.
    pub cpus: Vec<Option<Cpu>>,
    pub npresents: usize,
    pub present: Option<CpuSet>,
    pub nonlines: usize,
    pub online: Option<CpuSet>,
    pub arch: Arch,
    pub virt: Option<Virt>,
    /// `vuls`: `None` when the directory was missing or empty.
    pub vuls: Option<Vec<Vulnerability>>,
    pub caches: Vec<Cache>,
    /// Extra caches (s390), from `/proc/cpuinfo`.
    pub ecaches: Vec<Cache>,
    pub nodes: Vec<Node>,
    pub mode: Mode,
    pub noalive: bool,
    pub show_online: bool,
    pub show_offline: bool,
    pub show_physical: bool,
    pub show_compatible: bool,
    pub hex: bool,
    pub json: bool,
    pub bytes: bool,
    pub is_cluster: bool,
}

impl Cxt {
    /// `lscpu_new_context()`, with `lscpu_context_init_paths`'s handlers.
    #[must_use]
    pub fn new() -> Self {
        Cxt {
            maxcpus: 0,
            prefix: None,
            syscpu: PathCxt::new(b"/sys/devices/system/cpu"),
            procfs: PathCxt::new(b"/proc"),
            rootfs: None,
            cputypes: Vec::new(),
            cpus: Vec::new(),
            npresents: 0,
            present: None,
            nonlines: 0,
            online: None,
            arch: Arch::default(),
            virt: None,
            vuls: None,
            caches: Vec::new(),
            ecaches: Vec::new(),
            nodes: Vec::new(),
            mode: Mode::Summary,
            noalive: false,
            show_online: false,
            show_offline: false,
            show_physical: false,
            show_compatible: false,
            hex: false,
            json: false,
            bytes: false,
            is_cluster: false,
        }
    }

    /// `maxcpus` as a count, for sizing sets (it is positive by the time
    /// anything is sized).
    #[must_use]
    pub fn ncpus(&self) -> usize {
        usize::try_from(self.maxcpus).unwrap_or(0)
    }

    /// `lscpu_context_init_paths(cxt)`.
    pub fn init_paths(&mut self) {
        if let Some(prefix) = &self.prefix {
            let mut root = PathCxt::new(b"/");
            root.set_prefix(Some(prefix));
            self.rootfs = Some(root);
            self.syscpu.set_prefix(Some(prefix));
            self.procfs.set_prefix(Some(prefix));
        }
    }

    /// `lscpu_cputype_get_default(cxt)`: the first type.
    #[must_use]
    pub fn default_type(&self) -> Option<&CpuType> {
        self.cputypes.first()
    }

    /// `is_cpu_online(cxt, cpu)`.
    #[must_use]
    pub fn is_online(&self, cpu: &Cpu) -> bool {
        self.online.as_ref().is_some_and(|s| s.is_set(cpu.index()))
    }

    /// `is_cpu_present(cxt, cpu)`.
    #[must_use]
    pub fn is_present(&self, cpu: &Cpu) -> bool {
        self.present.as_ref().is_some_and(|s| s.is_set(cpu.index()))
    }

    /// `lscpu_get_cpu(cxt, logical_id)`: the made CPU with that number.
    #[must_use]
    pub fn cpu_index(&self, logical_id: i32) -> Option<usize> {
        self.cpus
            .iter()
            .position(|c| c.as_ref().is_some_and(|c| c.logical_id == logical_id))
    }

    /// `lscpu_create_cpus(cxt, set)`: a CPU for each of the first
    /// `maxcpus` CPUs in `set`, and a gap at the end for each one past
    /// them that the count still includes.
    pub fn create_cpus(&mut self, set: &CpuSet) {
        let npossibles = set.count();
        let mut cpus: Vec<Option<Cpu>> = Vec::with_capacity(npossibles);
        for n in set.iter() {
            if n >= self.ncpus() || cpus.len() >= npossibles {
                break;
            }
            cpus.push(Some(Cpu::new(i32::try_from(n).unwrap_or(i32::MAX))));
        }
        cpus.resize(npossibles, None);
        self.cpus = cpus;
    }
}

impl Default for Cxt {
    fn default() -> Self {
        Self::new()
    }
}
