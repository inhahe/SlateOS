//! `lscpu-cputype.c`: `/proc/cpuinfo` into CPU types and CPUs, and the
//! machine-wide facts -- the architecture, which CPUs are possible, present
//! and online, the vulnerabilities, and the NUMA nodes.
//!
//! `/proc/cpuinfo` is read as upstream reads it: `fgets` into 4096 bytes,
//! so a longer line is parsed as several; `NAME : VALUE` with `NAME` cut to
//! 31 bytes and a trailing number in it taken off as a key (`processor 5`,
//! `cache1`); and the name looked up, exactly, in three sorted tables.
//!
//! The CPU types are upstream's as Ubuntu 24.04 ships 2.39.3, with the
//! later de-duplication backported (upstream commit eb6514b4, Ubuntu's
//! LP #2111723): a type field the current type already has starts a new
//! type -- so one per CPU -- and afterwards the types are sorted by vendor,
//! model, model name and stepping and merged where those agree, keeping the
//! first of each. So a machine whose CPUs differ only in their flags shows
//! the first CPU's, and a hybrid machine's types print in that sorted order.

use crate::cpuset::CpuSet;
use crate::cstr::{self, Scanned};
use crate::path::{BUFSIZ, PathCxt};
use crate::sys;
use crate::types::{Arch, Cache, Cpu, CpuType, Cxt, Node, Vulnerability};
use std::io::{self, BufReader};

/// Which structure a `/proc/cpuinfo` field fills: `CPUINFO_LINE_*`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Domain {
    CpuType,
    Cpu,
    Cache,
}

/// `PAT_*`: what a field is, whatever it is called on this architecture.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Pat {
    AddressSizes,
    Bogomips,
    BogomipsCpu,
    Cpu,
    Family,
    Features,
    Flags,
    Implementer,
    MaxThreadId,
    Mhz,
    MhzDynamic,
    MhzStatic,
    Model,
    ModelName,
    Part,
    Processor,
    Revision,
    Stepping,
    Type,
    Variant,
    Vendor,
    Cache,
    Isa,
}

/// Which member a field is stored in: upstream's `offsetof`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Member {
    Flags,
    Addrsz,
    Bogomips,
    Family,
    Revision,
    Vendor,
    Model,
    Stepping,
    Isa,
    Modelname,
    Mtid,
    Mhz,
    DynamicMhz,
    StaticMhz,
    LogicalId,
    None,
}

/// `struct cpuinfo_pattern`.
struct Pattern {
    id: Pat,
    domain: Domain,
    name: &'static [u8],
    member: Member,
}

const fn ty(name: &'static [u8], id: Pat, member: Member) -> Pattern {
    Pattern {
        id,
        domain: Domain::CpuType,
        name,
        member,
    }
}

const fn cpu(name: &'static [u8], id: Pat, member: Member) -> Pattern {
    Pattern {
        id,
        domain: Domain::Cpu,
        name,
        member,
    }
}

/// `type_patterns[]`.
const TYPE_PATTERNS: [Pattern; 28] = [
    ty(b"ASEs implemented", Pat::Flags, Member::Flags),
    ty(b"Address Sizes", Pat::AddressSizes, Member::Addrsz),
    ty(b"BogoMIPS", Pat::Bogomips, Member::Bogomips),
    ty(b"CPU Family", Pat::Family, Member::Family),
    ty(b"CPU Revision", Pat::Revision, Member::Revision),
    ty(b"CPU implementer", Pat::Implementer, Member::Vendor),
    ty(b"CPU part", Pat::Part, Member::Model),
    ty(b"CPU revision", Pat::Revision, Member::Revision),
    ty(b"CPU variant", Pat::Variant, Member::Stepping),
    ty(b"Features", Pat::Features, Member::Flags),
    ty(b"ISA", Pat::Isa, Member::Isa),
    ty(b"Model Name", Pat::ModelName, Member::Modelname),
    ty(b"address sizes", Pat::AddressSizes, Member::Addrsz),
    ty(b"bogomips per cpu", Pat::Bogomips, Member::Bogomips),
    ty(b"cpu", Pat::Cpu, Member::Modelname),
    ty(b"cpu family", Pat::Family, Member::Family),
    ty(b"cpu model", Pat::Model, Member::Model),
    ty(b"family", Pat::Family, Member::Family),
    ty(b"features", Pat::Features, Member::Flags),
    ty(b"flags", Pat::Flags, Member::Flags),
    ty(b"max thread id", Pat::MaxThreadId, Member::Mtid),
    ty(b"model", Pat::Model, Member::Model),
    ty(b"model name", Pat::ModelName, Member::Modelname),
    ty(b"revision", Pat::Revision, Member::Revision),
    ty(b"stepping", Pat::Stepping, Member::Stepping),
    ty(b"type", Pat::Type, Member::Flags),
    ty(b"vendor", Pat::Vendor, Member::Vendor),
    ty(b"vendor_id", Pat::Vendor, Member::Vendor),
];

/// `cpu_patterns[]`.
const CPU_PATTERNS: [Pattern; 7] = [
    cpu(b"CPU MHz", Pat::Mhz, Member::Mhz),
    cpu(b"bogomips", Pat::BogomipsCpu, Member::Bogomips),
    cpu(b"cpu MHz", Pat::Mhz, Member::Mhz),
    cpu(b"cpu MHz dynamic", Pat::MhzDynamic, Member::DynamicMhz),
    cpu(b"cpu MHz static", Pat::MhzStatic, Member::StaticMhz),
    cpu(b"cpu number", Pat::Processor, Member::LogicalId),
    cpu(b"processor", Pat::Processor, Member::LogicalId),
];

/// `cache_patterns[]`.
const CACHE_PATTERNS: [Pattern; 1] = [Pattern {
    id: Pat::Cache,
    domain: Domain::Cache,
    name: b"cache",
    member: Member::None,
}];

/// `CPUTYPE_PATTERN_BUFSZ`.
const PATTERN_BUFSZ: usize = 32;

/// `bsearch` of a table sorted by `strcmp`: with unique names, the entry
/// whose name is `key`.
fn find(table: &'static [Pattern], key: &[u8]) -> Option<&'static Pattern> {
    table.iter().find(|p| p.name == key)
}

/// `key_cleanup(str, &keynum)`: trailing white space off, then a trailing
/// number off -- stored as an `int` in `keynum` -- and the white space
/// before it.
fn key_cleanup(key: &[u8], keynum: &mut i32) -> Vec<u8> {
    let key = cstr::rtrim_whitespace(key);
    let digits = key.iter().rev().take_while(|b| b.is_ascii_digit()).count();
    if digits == 0 {
        return key.to_vec();
    }
    let split = key.len().saturating_sub(digits);
    let number = key.get(split..).unwrap_or_default();
    // `strtol`, refused only past `long`; the store keeps the low 32 bits.
    match ulstrutils::ul_strtos64(number, 10) {
        Ok(n) => {
            *keynum = n as i32;
            cstr::rtrim_whitespace(key.get(..split).unwrap_or_default()).to_vec()
        }
        Err(_) => key.to_vec(),
    }
}

/// `cpuinfo_parse_line(str, &value, &keynum)`: the field a line is, and its
/// value.
fn parse_line(line: &[u8], keynum: &mut i32) -> Option<(&'static Pattern, Vec<u8>)> {
    let p = cstr::skip_blank(line);
    if p.is_empty() {
        return None;
    }
    let colon = p.iter().position(|&b| b == b':')?;
    // `xstrncpy(buf, p, sizeof(buf))`, then the name ends at the colon --
    // or at 31 bytes, where upstream writes its NUL past the buffer.
    let name = p.get(..colon.min(PATTERN_BUFSZ.saturating_sub(1)))?;
    let value = cstr::skip_space(p.get(colon.saturating_add(1)..)?);
    if value.is_empty() {
        return None;
    }
    let key = key_cleanup(name, keynum);
    let pattern = find(&TYPE_PATTERNS, &key)
        .or_else(|| find(&CPU_PATTERNS, &key))
        .or_else(|| find(&CACHE_PATTERNS, &key))?;
    Some((pattern, cstr::rtrim_whitespace(value).to_vec()))
}

/// The member a type field is stored in, if it is one: upstream's
/// `(char **) ((char *) ct + offset)`.
fn type_member(ct: &CpuType, member: Member) -> Option<&Option<Vec<u8>>> {
    Some(match member {
        Member::Flags => &ct.flags,
        Member::Addrsz => &ct.addrsz,
        Member::Bogomips => &ct.bogomips,
        Member::Family => &ct.family,
        Member::Revision => &ct.revision,
        Member::Vendor => &ct.vendor,
        Member::Model => &ct.model,
        Member::Stepping => &ct.stepping,
        Member::Isa => &ct.isa,
        Member::Modelname => &ct.modelname,
        Member::Mtid => &ct.mtid,
        _ => return None,
    })
}

/// `is_nonnull_offset(ct, offset)`: this type already has the field.
fn is_nonnull(ct: &CpuType, member: Member) -> bool {
    type_member(ct, member).is_some_and(Option::is_some)
}

/// `strcmp_offsets(a, b, offset)`: a missing field sorts first.
fn strcmp_members(a: Option<&[u8]>, b: Option<&[u8]>) -> i32 {
    match (a, b) {
        (None, None) => 0,
        (None, Some(_)) => -1,
        (Some(_), None) => 1,
        (Some(a), Some(b)) => cstr::strcmp(a, b),
    }
}

/// `cmp_cputype(a, b)`: by vendor, model, model name and stepping -- "it is
/// possible that CPUs differentiate in flags or BogoMIPS values, but it
/// seems better to ignore it".
fn cmp_cputype(a: &CpuType, b: &CpuType) -> i32 {
    let pairs = [
        (&a.vendor, &b.vendor),
        (&a.model, &b.model),
        (&a.modelname, &b.modelname),
        (&a.stepping, &b.stepping),
    ];
    pairs
        .iter()
        .map(|(x, y)| strcmp_members(x.as_deref(), y.as_deref()))
        .find(|&rc| rc != 0)
        .unwrap_or(0)
}

/// `deduplicate_cputypes(cxt)`: the types, which the parser made one per
/// CPU, sorted and made unique, each CPU pointed at the one kept for it.
/// Two left where the first holds nothing the second does not -- a
/// PowerPC `/proc/cpuinfo` whose trailer adds a model to the last CPU's
/// type -- become the second.
fn deduplicate_cputypes(cxt: &mut Cxt) {
    let n = cxt.cputypes.len();
    if n == 0 {
        return;
    }
    let mut order: Vec<usize> = (0..n).collect();
    {
        let types = &cxt.cputypes;
        let cmp = |a: &usize, b: &usize| match (types.get(*a), types.get(*b)) {
            (Some(x), Some(y)) => cmp_cputype(x, y),
            _ => 0,
        };
        cstr::msort(&mut order, &cmp);
    }
    // `map[old]` is the new index of the type old index `old` became.
    let mut map = vec![0usize; n];
    let mut kept: Vec<usize> = Vec::with_capacity(n);
    for &i in &order {
        let same = kept.last().is_some_and(|&u| {
            matches!((cxt.cputypes.get(u), cxt.cputypes.get(i)), (Some(x), Some(y)) if cmp_cputype(x, y) == 0)
        });
        if !same {
            kept.push(i);
        }
        if let Some(slot) = map.get_mut(i) {
            *slot = kept.len().saturating_sub(1);
        }
    }
    if let &[first, second] = kept.as_slice()
        && let (Some(ct), Some(mt)) = (cxt.cputypes.get(first), cxt.cputypes.get(second))
    {
        let items = [
            (&ct.model, &mt.model),
            (&ct.modelname, &mt.modelname),
            (&ct.vendor, &mt.vendor),
            (&ct.bogomips, &mt.bogomips),
            (&ct.revision, &mt.revision),
        ];
        let subset = items
            .iter()
            .all(|(c, m)| c.is_none() || strcmp_members(m.as_deref(), c.as_deref()) == 0);
        if subset {
            map.fill(0);
            kept = vec![second];
        }
    }
    let mut old: Vec<Option<CpuType>> = std::mem::take(&mut cxt.cputypes)
        .into_iter()
        .map(Some)
        .collect();
    cxt.cputypes = kept
        .iter()
        .map(|&k| old.get_mut(k).and_then(Option::take).unwrap_or_default())
        .collect();
    for cpu in cxt.cpus.iter_mut().flatten() {
        cpu.cputype = cpu.cputype.and_then(|t| map.get(t).copied());
    }
}

/// `strdup_to_offset(ct, offset, value)` for a type.
fn set_type_member(ct: &mut CpuType, member: Member, value: &[u8]) {
    let slot = match member {
        Member::Flags => &mut ct.flags,
        Member::Addrsz => &mut ct.addrsz,
        Member::Bogomips => &mut ct.bogomips,
        Member::Family => &mut ct.family,
        Member::Revision => &mut ct.revision,
        Member::Vendor => &mut ct.vendor,
        Member::Model => &mut ct.model,
        Member::Stepping => &mut ct.stepping,
        Member::Isa => &mut ct.isa,
        Member::Modelname => &mut ct.modelname,
        Member::Mtid => &mut ct.mtid,
        _ => return,
    };
    *slot = Some(value.to_vec());
}

/// `strdup_to_offset(cpu, offset, value)` for a CPU.
fn set_cpu_member(cpu: &mut Cpu, member: Member, value: &[u8]) {
    let slot = match member {
        Member::Bogomips => &mut cpu.bogomips,
        Member::Mhz => &mut cpu.mhz,
        Member::DynamicMhz => &mut cpu.dynamic_mhz,
        Member::StaticMhz => &mut cpu.static_mhz,
        _ => return,
    };
    *slot = Some(value.to_vec());
}

/// `sscanf(p, "NAME=%d")`-style: the one value after `NAME=` at `at`.
fn scan_after(data: &[u8], at: usize, fmt: &[u8]) -> Option<Scanned> {
    let mut values = cstr::sscanf(data.get(at..)?, fmt)?;
    if values.len() == 1 {
        values.pop()
    } else {
        None
    }
}

/// `cpuinfo_parse_cache(cxt, keynum, data)`: an s390 `cache<N>` line, which
/// describes a cache shared across CPUs that `/sys` does not show.
///
/// Upstream reads past `strstr`'s `NULL` when the line has no `scope=` or
/// no `type=`, and crashes; such a line is skipped here. (Upstream also
/// keeps the `N` of `cache<N>`, which nothing reads.)
#[allow(
    clippy::arithmetic_side_effects,
    reason = "`size * 1024` wraps as the C `long long` multiply does on x86-64, and the divisions are guarded as upstream's are"
)]
fn parse_cache(cxt: &mut Cxt, data: &[u8]) {
    let data = cstr::c_str(data);
    let Some(scope) = cstr::strstr(data, b"scope=") else {
        return;
    };
    if data
        .get(scope.saturating_add(6)..)
        .is_some_and(|s| s.starts_with(b"Private"))
    {
        return;
    }
    let Some(level_at) = cstr::strstr(data, b"level=") else {
        return;
    };
    let Some(level) = scan_after(data, level_at, b"level=%d").map(|v| v.as_int()) else {
        return;
    };
    let Some(type_at) = cstr::strstr(data, b"type=") else {
        return;
    };
    let kind = data.get(type_at.saturating_add(5)..).unwrap_or_default();
    if kind.is_empty() {
        return;
    }
    let kind = if kind.starts_with(b"Data") {
        Some(b'd')
    } else if kind.starts_with(b"Instruction") {
        Some(b'i')
    } else if kind.starts_with(b"Unified") {
        Some(b'u')
    } else {
        None
    };
    let Some(size_at) = cstr::strstr(data, b"size=") else {
        return;
    };
    let Some(size) = scan_after(data, size_at, b"size=%lld").map(|v| v.as_long()) else {
        return;
    };
    let Some(line_at) = cstr::strstr(data, b"line_size=") else {
        return;
    };
    let Some(line_size) = scan_after(data, line_at, b"line_size=%u").map(|v| v.as_uint()) else {
        return;
    };
    let Some(assoc_at) = cstr::strstr(data, b"associativity=") else {
        return;
    };
    let Some(associativity) = scan_after(data, assoc_at, b"associativity=%u").map(|v| v.as_uint())
    else {
        return;
    };

    let name = match kind {
        Some(c @ (b'i' | b'd')) => format!("L{level}{}", char::from(c)),
        _ => format!("L{level}"),
    };
    let size = size.wrapping_mul(1024) as u64;
    // `unsigned int` both, as upstream stores them.
    let sets = if line_size == 0 {
        0
    } else {
        (size / u64::from(line_size)) as u32
    };
    let sets = if associativity == 0 {
        0
    } else {
        sets / associativity
    };
    cxt.ecaches.push(Cache {
        name: Some(name.into_bytes()),
        level,
        size,
        ways_of_associativity: associativity,
        coherency_line_size: line_size,
        number_of_sets: sets,
        kind: match kind {
            Some(b'i') => Some(b"Instruction".to_vec()),
            Some(b'd') => Some(b"Data".to_vec()),
            Some(b'u') => Some(b"Unified".to_vec()),
            _ => None,
        },
        ..Cache::default()
    });
}

/// Why `/proc/cpuinfo` could not be read: `err(EXIT_FAILURE, "cannot open
/// /proc/cpuinfo")`.
#[derive(Debug)]
pub struct CpuinfoError(pub io::Error);

/// `lscpu_read_cpuinfo(cxt)`.
///
/// # Errors
///
/// The file will not open.
pub fn read_cpuinfo(cxt: &mut Cxt) -> Result<(), CpuinfoError> {
    /// Upstream's buffer: "used to be BUFSIZ which is small on some
    /// platforms e.g, musl, therefore hardcode to 4K".
    const LINE: usize = 4096;
    let file = cxt.procfs.open(b"cpuinfo").map_err(CpuinfoError)?;
    let mut reader = BufReader::new(file);
    let mut curr_cpu: Option<usize> = None;
    let mut curr_type: Option<usize> = None;

    while let Some(raw) = cstr::fgets(&mut reader, LINE) {
        let mut keynum: i32 = -1;
        let buf = cstr::c_str(&raw);
        let p = cstr::skip_space(buf);
        if !buf.is_empty() && p.is_empty() {
            // A blank line separates CPUs.
            continue;
        }
        let line = cstr::rtrim_whitespace(p);
        let Some((pattern, value)) = parse_line(line, &mut keynum) else {
            continue;
        };
        match pattern.domain {
            Domain::Cpu => {
                if pattern.id == Pat::Processor {
                    let id = if keynum >= 0 {
                        keynum
                    } else {
                        // `ul_strtou32(value, &n, 10)`, stored as an `int`;
                        // 0 when it does not parse.
                        ulstrutils::ul_strtou64(&value, 10)
                            .ok()
                            .and_then(|n| u32::try_from(n).ok())
                            .map_or(0, |n| n as i32)
                    };
                    if let (Some(c), Some(t)) = (curr_cpu, curr_type)
                        && let Some(Some(cpu)) = cxt.cpus.get_mut(c)
                    {
                        cpu.cputype = Some(t);
                    }
                    curr_cpu = cxt.cpu_index(id);
                    continue;
                }
                if let Some(Some(cpu)) = curr_cpu.and_then(|c| cxt.cpus.get_mut(c)) {
                    set_cpu_member(cpu, pattern.member, &value);
                }
                if let Some(ct) = curr_type.and_then(|t| cxt.cputypes.get_mut(t)) {
                    match pattern.id {
                        Pat::MhzDynamic if ct.dynamic_mhz.is_none() => {
                            ct.dynamic_mhz = Some(value.clone());
                        }
                        Pat::MhzStatic if ct.static_mhz.is_none() => {
                            ct.static_mhz = Some(value.clone());
                        }
                        Pat::BogomipsCpu if ct.bogomips.is_none() => {
                            ct.bogomips = Some(value.clone());
                        }
                        _ => {}
                    }
                }
                if pattern.id == Pat::Mhz
                    && let Some(Some(cpu)) = curr_cpu.and_then(|c| cxt.cpus.get_mut(c))
                {
                    let r = ulstrutils::strtod(&value);
                    cpu.mhz_cur_freq = if r.erange { 0.0 } else { r.value as f32 };
                }
            }
            Domain::CpuType => {
                // A field this type already has starts a new one: one type
                // per CPU, which `deduplicate_cputypes` then merges.
                if let Some(t) = curr_type
                    && cxt
                        .cputypes
                        .get(t)
                        .is_some_and(|ct| is_nonnull(ct, pattern.member))
                {
                    curr_type = None;
                }
                let t = match curr_type {
                    Some(t) => t,
                    None => {
                        cxt.cputypes.push(CpuType::new());
                        let t = cxt.cputypes.len().saturating_sub(1);
                        curr_type = Some(t);
                        t
                    }
                };
                if let Some(ct) = cxt.cputypes.get_mut(t) {
                    set_type_member(ct, pattern.member, &value);
                }
            }
            Domain::Cache => {
                if pattern.id == Pat::Cache {
                    parse_cache(cxt, &value);
                }
            }
        }
    }

    if let Some(Some(cpu)) = curr_cpu.and_then(|c| cxt.cpus.get_mut(c))
        && cpu.cputype.is_none()
    {
        cpu.cputype = curr_type;
    }
    deduplicate_cputypes(cxt);
    sort_caches(&mut cxt.ecaches);

    // CPUs cpuinfo did not describe get the first type.
    if !cxt.cputypes.is_empty() {
        for cpu in cxt.cpus.iter_mut().flatten() {
            if cpu.cputype.is_none() {
                cpu.cputype = Some(0);
            }
        }
    }
    Ok(())
}

/// `lscpu_sort_caches(caches, n)`: by name, as glibc's `qsort` orders them.
pub fn sort_caches(caches: &mut [Cache]) {
    cstr::msort(caches, &|a: &Cache, b: &Cache| {
        cstr::strcmp(a.name_bytes(), b.name_bytes())
    });
}

/// `lscpu_read_architecture(cxt)`.
///
/// # Errors
///
/// `uname` failed.
pub fn read_architecture(cxt: &Cxt) -> io::Result<Arch> {
    let mut ar = Arch {
        name: sys::machine()?,
        bit32: false,
        bit64: false,
    };
    if !cxt.noalive {
        // What this build's own architecture implies, when the CPU being
        // described is the one running it.
        if cfg!(any(
            target_arch = "x86_64",
            target_arch = "x86",
            target_arch = "s390x",
            target_arch = "sparc64"
        )) {
            ar.bit32 = true;
        }
        if cfg!(target_arch = "aarch64") {
            if sys::has_aarch32() {
                ar.bit32 = true;
            }
            ar.bit64 = true;
        }
    }
    if let Some(ct) = cxt.default_type() {
        if let Some(flags) = &ct.flags {
            if cstr::has_word(flags, b"lm", BUFSIZ)
                || cstr::has_word(flags, b"zarch", BUFSIZ)
                || cstr::has_word(flags, b"sun4v", BUFSIZ)
                || cstr::has_word(flags, b"sun4u", BUFSIZ)
            {
                ar.bit32 = true;
                ar.bit64 = true;
            }
        }
        if let Some(isa) = &ct.isa {
            if cstr::has_word(isa, b"loongarch32", BUFSIZ) {
                ar.bit32 = true;
            }
            if cstr::has_word(isa, b"loongarch64", BUFSIZ) {
                ar.bit64 = true;
            }
        }
    }
    if !cxt.noalive {
        if ar.name == b"ppc64" {
            ar.bit32 = true;
            ar.bit64 = true;
        } else if ar.name == b"ppc" {
            ar.bit32 = true;
        }
    }
    Ok(ar)
}

/// Why the CPU lists could not be read: `failed to determine number of
/// CPUs: /sys/devices/system/cpu/possible`, and why.
#[derive(Debug)]
pub struct CpulistError(pub io::Error);

/// `lscpu_read_cpulists(cxt)`.
///
/// # Errors
///
/// `possible` will not read.
pub fn read_cpulists(cxt: &mut Cxt) -> Result<(), CpulistError> {
    if let Some(max) = cxt.syscpu.read_s32(b"kernel_max") {
        // kernel_max is the highest index, NR_CPUS - 1.
        cxt.maxcpus = max.wrapping_add(1);
    } else if !cxt.noalive {
        cxt.maxcpus = sys::max_number_of_cpus();
    }
    if cxt.maxcpus <= 0 {
        // "error or we are reading some /sys snapshot instead of the real
        // /sys, let's use any crazy number..."
        cxt.maxcpus = 2048;
    }
    let ncpus = cxt.ncpus();
    let possible = cxt
        .syscpu
        .read_cpuset(b"possible", ncpus, true)
        .map_err(CpulistError)?;
    cxt.create_cpus(&possible);
    if let Ok(set) = cxt.syscpu.read_cpuset(b"present", ncpus, true) {
        cxt.npresents = set.count();
        cxt.present = Some(set);
    }
    if let Ok(set) = cxt.syscpu.read_cpuset(b"online", ncpus, true) {
        cxt.nonlines = set.count();
        cxt.online = Some(set);
    }
    Ok(())
}

/// `lscpu_read_archext(cxt)`: the default type's dispatching mode, boost,
/// and s390 machine type.
pub fn read_archext(cxt: &mut Cxt) {
    let dispatching = cxt.syscpu.read_s32(b"dispatching");
    let freqboost = cxt.syscpu.read_s32(b"cpufreq/boost");
    let sysinfo = cxt.procfs.open(b"sysinfo").ok();
    let Some(ct) = cxt.cputypes.first_mut() else {
        return;
    };
    ct.dispatching = dispatching.unwrap_or(-1);
    ct.freqboost = freqboost.unwrap_or(-1);
    if let Some(file) = sysinfo {
        let mut reader = BufReader::new(file);
        while let Some(line) = cstr::fgets(&mut reader, BUFSIZ) {
            if cstr::lookup(&line, b"Type", &mut ct.machinetype) {
                break;
            }
        }
    }
}

/// `lscpu_read_vulnerabilities(cxt)`.
pub fn read_vulnerabilities(cxt: &mut Cxt) {
    let Ok(entries) = cxt.syscpu.read_dir(Some(b"vulnerabilities")) else {
        return;
    };
    if entries.is_empty() {
        return;
    }
    let mut vuls = Vec::with_capacity(entries.len());
    for (name, is_dir) in entries {
        if is_dir {
            continue;
        }
        let mut rel = b"vulnerabilities/".to_vec();
        rel.extend_from_slice(&name);
        let text = match cxt.syscpu.read_string(&rel) {
            Ok((n, Some(text))) if n > 0 => text,
            _ => continue,
        };
        let mut vname = name;
        if let Some(first) = vname.first_mut() {
            *first = first.to_ascii_uppercase();
        }
        for b in &mut vname {
            if *b == b'_' {
                *b = b' ';
            }
        }
        vuls.push(Vulnerability {
            name: vname,
            text: mitigation(text),
        });
    }
    cstr::msort(&mut vuls, &|a: &Vulnerability, b: &Vulnerability| {
        cstr::strcmp(&a.name, &b.name)
    });
    cxt.vuls = Some(vuls);
}

/// A vulnerability's text: after a leading `Mitigation`, the next byte
/// becomes `;` and every `:` in the text goes. (A text that *is*
/// `Mitigation` has its terminating NUL overwritten upstream; here it
/// gains the `;`.)
fn mitigation(mut text: Vec<u8>) -> Vec<u8> {
    const WORD: &[u8] = b"Mitigation";
    if text.starts_with(WORD) {
        match text.get_mut(WORD.len()) {
            Some(b) => *b = b';',
            None => text.push(b';'),
        }
        text.retain(|&b| b != b':');
    }
    text
}

/// `is_node_dirent(d)`: a directory named `node` and digits.
fn is_node_dirent(name: &[u8], is_dir: bool) -> bool {
    is_dir
        && name.starts_with(b"node")
        && ulstrutils::isdigit_string(name.get(4..).unwrap_or_default())
}

/// Why the NUMA nodes could not be read: a node's number is past `long`
/// -- `Failed to extract the node number`.
#[derive(Debug)]
pub struct NodeNumberError(pub Vec<u8>);

/// `lscpu_read_numas(cxt)`.
///
/// # Errors
///
/// A node directory's number does not fit in a `long`.
pub fn read_numas(cxt: &mut Cxt) -> Result<(), NodeNumberError> {
    let mut sys = PathCxt::new(b"/sys/devices/system/node");
    sys.set_prefix(cxt.prefix.as_deref());
    let Ok(entries) = sys.read_dir(None) else {
        return Ok(());
    };
    let mut nums: Vec<i32> = Vec::new();
    for (name, is_dir) in &entries {
        if !is_node_dirent(name, *is_dir) {
            continue;
        }
        let digits = name.get(4..).unwrap_or_default();
        let n =
            ulstrutils::ul_strtos64(digits, 10).map_err(|_| NodeNumberError(digits.to_vec()))?;
        // Stored in an `int`.
        nums.push(n as i32);
    }
    if nums.is_empty() {
        return Ok(());
    }
    // `nodecmp`: `*a - *b`, which overflows for numbers far apart.
    cstr::msort(&mut nums, &|a: &i32, b: &i32| a.wrapping_sub(*b));
    let ncpus = cxt.ncpus();
    cxt.nodes = nums
        .into_iter()
        .map(|num| {
            let rel = format!("node{num}/cpumap");
            Node {
                num,
                map: sys.read_cpuset(rel.as_bytes(), ncpus, false).ok(),
            }
        })
        .collect();
    Ok(())
}

/// Whether `set` holds CPU `cpu`, a missing set holding none -- where
/// upstream would dereference `NULL`.
#[must_use]
pub fn set_has(set: Option<&CpuSet>, cpu: usize) -> bool {
    set.is_some_and(|s| s.is_set(cpu))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_lose_their_numbers() {
        let mut k = -1;
        assert_eq!(key_cleanup(b"processor 5 ", &mut k), b"processor".to_vec());
        assert_eq!(k, 5);
        let mut k = -1;
        assert_eq!(key_cleanup(b"cache12", &mut k), b"cache".to_vec());
        assert_eq!(k, 12);
        let mut k = -1;
        assert_eq!(key_cleanup(b"model name", &mut k), b"model name".to_vec());
        assert_eq!(k, -1);
        let mut k = -1;
        // Past `long`: kept whole.
        assert_eq!(
            key_cleanup(b"x99999999999999999999", &mut k),
            b"x99999999999999999999".to_vec()
        );
        assert_eq!(k, -1);
        let mut k = -1;
        assert_eq!(
            key_cleanup(b"processor 4294967295", &mut k),
            b"processor".to_vec()
        );
        assert_eq!(k, -1);
    }

    #[test]
    fn lines_parse_as_upstream() {
        let mut k = -1;
        let (p, v) = parse_line(b"model name\t: AMD EPYC", &mut k)
            .unwrap_or((&CACHE_PATTERNS[0], Vec::new()));
        assert_eq!(p.id, Pat::ModelName);
        assert_eq!(v, b"AMD EPYC".to_vec());
        assert!(parse_line(b"model name\t:", &mut k).is_none());
        assert!(parse_line(b"no colon", &mut k).is_none());
        assert!(parse_line(b"power management:", &mut k).is_none());
        // A name longer than 31 bytes is cut there, then trimmed.
        let long = b"model name                          : x";
        let (p, _) = parse_line(long, &mut k).unwrap_or((&CACHE_PATTERNS[0], Vec::new()));
        assert_eq!(p.id, Pat::ModelName);
    }

    #[test]
    fn mitigations_lose_their_colons() {
        assert_eq!(
            mitigation(b"Mitigation: PTI: x".to_vec()),
            b"Mitigation; PTI x".to_vec()
        );
        assert_eq!(
            mitigation(b"Vulnerable: x".to_vec()),
            b"Vulnerable: x".to_vec()
        );
        assert_eq!(mitigation(b"Mitigation".to_vec()), b"Mitigation;".to_vec());
    }

    #[test]
    fn nodes_are_directories_named_by_number() {
        assert!(is_node_dirent(b"node0", true));
        assert!(!is_node_dirent(b"node0", false));
        assert!(!is_node_dirent(b"node", true));
        assert!(!is_node_dirent(b"node1x", true));
    }
}
