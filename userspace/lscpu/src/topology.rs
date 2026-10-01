//! `lscpu-topology.c`: how the CPUs are grouped -- threads into cores into
//! sockets into books into drawers -- from each CPU's sibling masks in
//! `/sys`, and the caches, IDs, polarization, addresses and frequencies of
//! each CPU.
//!
//! A type's cores are its *distinct* `thread_siblings` masks and its
//! sockets its distinct `core_siblings` masks, in the order first seen;
//! a CPU's logical core number is the index of the first of them holding
//! it. The caches are identified by type, level and id -- the `id` file,
//! or failing that the number of same-kind caches seen before the first
//! one holding this CPU.

use crate::cpuset::CpuSet;
use crate::cputype::sort_caches;
use crate::cstr;
use crate::types::{
    Cache, Cpu, Cxt, POLAR_HORIZONTAL, POLAR_UNKNOWN, POLAR_VHIGH, POLAR_VLOW, POLAR_VMEDIUM,
};
use std::io::BufReader;

/// `add_cpuset_to_array(ary, &items, set)`: `set` appended unless an equal
/// one is there already.
fn add_cpuset(ary: &mut Vec<CpuSet>, set: CpuSet) {
    if !ary.contains(&set) {
        ary.push(set);
    }
}

/// `cputype_read_topology(cxt, ct)`.
#[allow(
    clippy::arithmetic_side_effects,
    reason = "the divisions are guarded by their zero tests, as upstream's are, and `x + 1` is range-checked first"
)]
fn read_type_topology(cxt: &mut Cxt, t: usize) {
    let ncpus = cxt.ncpus();
    let mut nthreads = 0usize;
    let mut cores = Vec::new();
    let mut sockets = Vec::new();
    let mut books = Vec::new();
    let mut drawers = Vec::new();
    for cpu in cxt.cpus.iter().flatten() {
        if cpu.cputype != Some(t) {
            continue;
        }
        let num = cpu.logical_id;
        let rel = |what: &str| format!("cpu{num}/topology/{what}");
        if !cxt.syscpu.exists(rel("thread_siblings").as_bytes()) {
            continue;
        }
        let read = |what: &str| {
            cxt.syscpu
                .read_cpuset(rel(what).as_bytes(), ncpus, false)
                .ok()
        };
        let thread_siblings = read("thread_siblings");
        let core_siblings = read("core_siblings");
        let book_siblings = read("book_siblings");
        let drawer_siblings = read("drawer_siblings");
        let n = thread_siblings.as_ref().map_or(0, CpuSet::count).max(1);
        nthreads = nthreads.max(n);
        if let Some(set) = thread_siblings {
            add_cpuset(&mut cores, set);
        }
        if let Some(set) = core_siblings {
            add_cpuset(&mut sockets, set);
        }
        if let Some(set) = book_siblings {
            add_cpuset(&mut books, set);
        }
        if let Some(set) = drawer_siblings {
            add_cpuset(&mut drawers, set);
        }
    }

    // s390 reads its topology from /proc/sysinfo when it is there: in a
    // virtual machine the masks in /sys do not add up.
    let mut sw_topo: Option<[usize; 4]> = None;
    if let Ok(file) = cxt.procfs.open(b"sysinfo") {
        let mut reader = BufReader::new(file);
        while let Some(line) = cstr::fgets(&mut reader, crate::path::BUFSIZ) {
            if let Some(v) = cstr::sscanf(&line, b"CPU Topology SW: %d %d %zu %zu %zu %zu")
                && v.len() == 6
            {
                let get = |i: usize| {
                    v.get(i)
                        .map_or(0, |x| usize::try_from(x.as_ulong()).unwrap_or(usize::MAX))
                };
                sw_topo = Some([get(2), get(3), get(4), get(5)]);
                break;
            }
        }
    }

    let Some(ct) = cxt.cputypes.get_mut(t) else {
        return;
    };
    ct.coremaps = cores;
    ct.socketmaps = sockets;
    ct.bookmaps = books;
    ct.drawermaps = drawers;
    ct.nthreads_per_core = nthreads;
    if let Some(mtid) = &ct.mtid
        && let Ok(x) = ulstrutils::ul_strtou64(mtid, 10)
        && let Ok(x) = usize::try_from(x)
    {
        // `(size_t) x + 1`, which wraps for the largest.
        ct.nthreads_per_core = x.wrapping_add(1);
    }
    if let Some([drawers, books, sockets, cores]) = sw_topo {
        ct.ndrawers_per_system = drawers;
        ct.nbooks_per_drawer = books;
        ct.nsockets_per_book = sockets;
        ct.ncores_per_socket = cores;
    } else {
        let per = |a: usize, b: usize| if b == 0 { 0 } else { a / b };
        ct.ncores_per_socket = per(ct.coremaps.len(), ct.socketmaps.len());
        ct.nsockets_per_book = per(ct.socketmaps.len(), ct.bookmaps.len());
        ct.nbooks_per_drawer = per(ct.bookmaps.len(), ct.drawermaps.len());
        ct.ndrawers_per_system = ct.drawermaps.len();
    }
}

/// `lscpu_get_cache_full_size(cxt, name, &instances)`: the size of every
/// cache of that name, summed, and how many there are.
#[must_use]
pub fn cache_full_size(cxt: &Cxt, name: &[u8]) -> (u64, usize) {
    cxt.caches
        .iter()
        .filter(|c| c.name_bytes() == name)
        .fold((0u64, 0usize), |(sz, n), c| {
            (sz.wrapping_add(c.size), n.saturating_add(1))
        })
}

/// `lscpu_cpu_get_cache(cxt, cpu, name)`: the first cache of that name
/// shared by `cpu`.
#[must_use]
pub fn cpu_get_cache<'c>(cxt: &'c Cxt, cpu: &Cpu, name: &[u8]) -> Option<&'c Cache> {
    cxt.caches.iter().find(|c| {
        c.name_bytes() == name && c.sharedmap.as_ref().is_some_and(|m| m.is_set(cpu.index()))
    })
}

/// `get_cache(cxt, type, level, id)`.
fn get_cache(caches: &[Cache], kind: &[u8], level: i32, id: i32) -> Option<usize> {
    caches.iter().position(|c| {
        c.id == id && c.level == level && c.kind.as_deref().unwrap_or_default() == kind
    })
}

/// `add_cache(cxt, type, level, id)`.
fn add_cache(caches: &mut Vec<Cache>, kind: &[u8], level: i32, id: i32) -> usize {
    caches.push(Cache {
        id,
        level,
        kind: Some(kind.to_vec()),
        ..Cache::default()
    });
    caches.len().saturating_sub(1)
}

/// `mk_cache_id(cxt, cpu, type, level)`: how many caches of this type and
/// level come before the first one `cpu` shares.
fn mk_cache_id(caches: &[Cache], cpu: usize, kind: &[u8], level: i32) -> i32 {
    let mut idx = 0i32;
    for c in caches {
        if c.level != level || c.kind.as_deref().unwrap_or_default() != kind {
            continue;
        }
        if c.sharedmap.as_ref().is_some_and(|m| m.is_set(cpu)) {
            return idx;
        }
        idx = idx.saturating_add(1);
    }
    idx
}

/// `read_sparc_onecache(cxt, cpu, level, typestr, type)`: SPARC's caches,
/// which `/sys` describes per CPU without saying who shares them -- so
/// each is taken to be the CPU's own.
fn read_sparc_onecache(cxt: &mut Cxt, cpu: &Cpu, level: i32, kind: &[u8], letter: Option<u8>) {
    let num = cpu.logical_id;
    let tag = match letter {
        Some(c) => format!("l{level}_{}", char::from(c)),
        None => format!("l{level}_"),
    };
    let Some(size) = cxt
        .syscpu
        .read_u32(format!("cpu{num}/{tag}cache_size").as_bytes())
    else {
        return;
    };
    let id = mk_cache_id(&cxt.caches, cpu.index(), kind, level);
    let i = match get_cache(&cxt.caches, kind, level, id) {
        Some(i) => i,
        None => add_cache(&mut cxt.caches, kind, level, id),
    };
    let line_size = cxt
        .syscpu
        .read_u32(format!("cpu{num}/{tag}cache_line_size").as_bytes());
    let ncpus = cxt.ncpus();
    let Some(ca) = cxt.caches.get_mut(i) else {
        return;
    };
    if ca.name.is_none() {
        if let Some(l) = line_size {
            ca.coherency_line_size = l;
        }
        ca.name = Some(match letter {
            Some(c) => format!("L{}{}", ca.level, char::from(c)).into_bytes(),
            None => format!("L{}", ca.level).into_bytes(),
        });
        ca.size = u64::from(size);
    }
    if ca.sharedmap.is_none() {
        let mut map = CpuSet::new(ncpus);
        map.set(cpu.index());
        ca.sharedmap = Some(map);
    }
}

/// `read_sparc_caches(cxt, cpu)`. (The level-2 cache is read twice, as
/// upstream reads it.)
fn read_sparc_caches(cxt: &mut Cxt, cpu: &Cpu) {
    read_sparc_onecache(cxt, cpu, 1, b"Instruction", Some(b'i'));
    read_sparc_onecache(cxt, cpu, 1, b"Data", Some(b'd'));
    read_sparc_onecache(cxt, cpu, 2, b"Unified", None);
    read_sparc_onecache(cxt, cpu, 2, b"Unified", None);
}

/// `read_caches(cxt, cpu)`.
fn read_caches(cxt: &mut Cxt, cpu: &Cpu) {
    let num = cpu.logical_id;
    let mut ncaches = 0usize;
    while cxt
        .syscpu
        .exists(format!("cpu{num}/cache/index{ncaches}").as_bytes())
    {
        ncaches = ncaches.saturating_add(1);
    }
    if ncaches == 0
        && cxt
            .syscpu
            .exists(format!("cpu{num}/l1_icache_size").as_bytes())
    {
        read_sparc_caches(cxt, cpu);
        return;
    }
    let ncpus = cxt.ncpus();
    for i in 0..ncaches {
        let rel = |what: &str| format!("cpu{num}/cache/index{i}/{what}");
        let id = cxt.syscpu.read_s32(rel("id").as_bytes()).unwrap_or(-1);
        let Some(level) = cxt.syscpu.read_s32(rel("level").as_bytes()) else {
            continue;
        };
        let kind = match cxt.syscpu.read_buffer(rel("type").as_bytes(), 256) {
            Ok((n, kind)) if n > 0 => kind,
            _ => continue,
        };
        let id = if id == -1 {
            mk_cache_id(&cxt.caches, cpu.index(), &kind, level)
        } else {
            id
        };
        let at = match get_cache(&cxt.caches, &kind, level, id) {
            Some(at) => at,
            None => add_cache(&mut cxt.caches, &kind, level, id),
        };
        let needs_name = cxt.caches.get(at).is_some_and(|c| c.name.is_none());
        if needs_name {
            let ways = cxt.syscpu.read_u32(rel("ways_of_associativity").as_bytes());
            let phyline = cxt
                .syscpu
                .read_u32(rel("physical_line_partition").as_bytes());
            let sets = cxt.syscpu.read_u32(rel("number_of_sets").as_bytes());
            let coherency = cxt.syscpu.read_u32(rel("coherency_line_size").as_bytes());
            let string = |what: &str| {
                cxt.syscpu
                    .read_string(rel(what).as_bytes())
                    .ok()
                    .and_then(|(_, s)| s)
            };
            let allocation_policy = string("allocation_policy");
            let write_policy = string("write_policy");
            let size = match cxt.syscpu.read_buffer(rel("size").as_bytes(), 256) {
                // `parse_size`, its status ignored: 0 when the text is no
                // size, the partly scaled number when scaling overflowed.
                Ok((n, text)) if n > 0 => ulstrutils::parse_size_res(&text).1,
                _ => 0,
            };
            if let Some(ca) = cxt.caches.get_mut(at) {
                let letter = match ca.kind.as_deref() {
                    Some(b"Data") => Some('d'),
                    Some(b"Instruction") => Some('i'),
                    _ => None,
                };
                ca.name = Some(match letter {
                    Some(c) => format!("L{}{c}", ca.level).into_bytes(),
                    None => format!("L{}", ca.level).into_bytes(),
                });
                if let Some(v) = ways {
                    ca.ways_of_associativity = v;
                }
                if let Some(v) = phyline {
                    ca.physical_line_partition = v;
                }
                if let Some(v) = sets {
                    ca.number_of_sets = v;
                }
                if let Some(v) = coherency {
                    ca.coherency_line_size = v;
                }
                ca.allocation_policy = allocation_policy;
                ca.write_policy = write_policy;
                ca.size = size;
            }
        }
        let needs_map = cxt.caches.get(at).is_some_and(|c| c.sharedmap.is_none());
        if needs_map {
            let map = cxt
                .syscpu
                .read_cpuset(rel("shared_cpu_map").as_bytes(), ncpus, false)
                .ok();
            if let Some(ca) = cxt.caches.get_mut(at) {
                ca.sharedmap = map;
            }
        }
    }
}

/// `read_ids(cxt, cpu)`: core, socket, book and drawer ids, -1 each that
/// will not read -- all left as they were without a `topology` directory.
fn read_ids(cxt: &Cxt, cpu: &mut Cpu) {
    let num = cpu.logical_id;
    if !cxt.syscpu.exists(format!("cpu{num}/topology").as_bytes()) {
        return;
    }
    let id = |what: &str| {
        cxt.syscpu
            .read_s32(format!("cpu{num}/topology/{what}").as_bytes())
            .unwrap_or(-1)
    };
    cpu.coreid = id("core_id");
    cpu.socketid = id("physical_package_id");
    cpu.bookid = id("book_id");
    cpu.drawerid = id("drawer_id");
}

/// `read_polarization(cxt, cpu)`: whether the type has one; the CPU's.
fn read_polarization(cxt: &Cxt, cpu: &mut Cpu) -> bool {
    let rel = format!("cpu{}/polarization", cpu.logical_id);
    if !cxt.syscpu.exists(rel.as_bytes()) {
        return false;
    }
    let mode = cxt
        .syscpu
        .read_buffer(rel.as_bytes(), 64)
        .map(|(_, m)| m)
        .unwrap_or_default();
    cpu.polarization = match mode.as_slice() {
        b"vertical:low" => POLAR_VLOW,
        b"vertical:medium" => POLAR_VMEDIUM,
        b"vertical:high" => POLAR_VHIGH,
        b"horizontal" => POLAR_HORIZONTAL,
        _ => POLAR_UNKNOWN,
    };
    true
}

/// `read_address(cxt, cpu)`: whether the type has addresses.
fn read_address(cxt: &Cxt, cpu: &mut Cpu) -> bool {
    let rel = format!("cpu{}/address", cpu.logical_id);
    if !cxt.syscpu.exists(rel.as_bytes()) {
        return false;
    }
    if let Some(a) = cxt.syscpu.read_s32(rel.as_bytes()) {
        cpu.address = a;
    }
    true
}

/// `read_configure(cxt, cpu)`: whether the type has a `configure` state.
fn read_configure(cxt: &Cxt, cpu: &mut Cpu) -> bool {
    let rel = format!("cpu{}/configure", cpu.logical_id);
    if !cxt.syscpu.exists(rel.as_bytes()) {
        return false;
    }
    if let Some(c) = cxt.syscpu.read_s32(rel.as_bytes()) {
        cpu.configured = c;
    }
    true
}

/// `read_mhz(cxt, cpu)`: whether the type has frequencies. kHz in `/sys`,
/// divided in `float` as upstream divides.
fn read_mhz(cxt: &Cxt, cpu: &mut Cpu) -> bool {
    let num = cpu.logical_id;
    let khz = |what: &str| {
        cxt.syscpu
            .read_s32(format!("cpu{num}/cpufreq/{what}").as_bytes())
    };
    if let Some(v) = khz("cpuinfo_max_freq") {
        cpu.mhz_max_freq = v as f32 / 1000.0;
    }
    if let Some(v) = khz("cpuinfo_min_freq") {
        cpu.mhz_min_freq = v as f32 / 1000.0;
    }
    if let Some(v) = khz("scaling_cur_freq") {
        cpu.mhz_cur_freq = v as f32 / 1000.0;
    }
    cpu.mhz_min_freq != 0.0 || cpu.mhz_max_freq != 0.0
}

/// The present CPUs of type `t`.
fn present_of_type(cxt: &Cxt, t: usize) -> impl Iterator<Item = &Cpu> {
    cxt.cpus
        .iter()
        .flatten()
        .filter(move |c| c.cputype == Some(t) && cxt.is_present(c))
}

/// `lsblk_cputype_get_maxmhz(cxt, ct)`.
#[must_use]
pub fn get_maxmhz(cxt: &Cxt, t: usize) -> f32 {
    present_of_type(cxt, t).fold(0.0f32, |res, c| {
        if res > c.mhz_max_freq {
            res
        } else {
            c.mhz_max_freq
        }
    })
}

/// `lsblk_cputype_get_minmhz(cxt, ct)`: -1 with no present CPU of the type.
#[must_use]
pub fn get_minmhz(cxt: &Cxt, t: usize) -> f32 {
    present_of_type(cxt, t).fold(-1.0f32, |res, c| {
        if res < 0.0 || c.mhz_min_freq < res {
            c.mhz_min_freq
        } else {
            res
        }
    })
}

/// `lsblk_cputype_get_scalmhz(cxt, ct)`: current over maximum, summed over
/// the type's present CPUs, as a percentage.
#[must_use]
pub fn get_scalmhz(cxt: &Cxt, t: usize) -> f32 {
    let (fmax, fcur) = present_of_type(cxt, t)
        .filter(|c| !(c.mhz_max_freq <= 0.0 || c.mhz_cur_freq <= 0.0))
        .fold((0.0f32, 0.0f32), |(m, c), cpu| {
            (m + cpu.mhz_max_freq, c + cpu.mhz_cur_freq)
        });
    if fcur <= 0.0 {
        return 0.0;
    }
    fcur / fmax * 100.0
}

/// `lscpu_read_topology(cxt)`.
pub fn read_topology(cxt: &mut Cxt) {
    for t in 0..cxt.cputypes.len() {
        read_type_topology(cxt, t);
    }
    for i in 0..cxt.cpus.len() {
        let Some(Some(mut cpu)) = cxt.cpus.get(i).cloned() else {
            continue;
        };
        let Some(t) = cpu.cputype else {
            continue;
        };
        read_ids(cxt, &mut cpu);
        let polar = read_polarization(cxt, &mut cpu);
        let address = read_address(cxt, &mut cpu);
        let configure = read_configure(cxt, &mut cpu);
        let freq = read_mhz(cxt, &mut cpu);
        if let Some(ct) = cxt.cputypes.get_mut(t) {
            ct.has_polarization |= polar;
            ct.has_addresses |= address;
            ct.has_configured |= configure;
            ct.has_freq |= freq;
        }
        read_caches(cxt, &cpu);
        if let Some(slot) = cxt.cpus.get_mut(i) {
            *slot = Some(cpu);
        }
    }
    sort_caches(&mut cxt.caches);
}
