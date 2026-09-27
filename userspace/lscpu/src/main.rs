//! lscpu -- the CPU architecture, from `/proc/cpuinfo` and `/sys`.
//!
//! A port of util-linux 2.39.3's `sys-utils/lscpu*.c` and the parts of
//! `lib/` they use (`cpuset.c`, `path.c`), file by file and function by
//! function with upstream's names, printing through `smartcols` (the
//! libsmartcols port). Measured against `lscpu from util-linux 2.39.3` by
//! `scripts/lscpu-diff.sh`: on the machine itself, and with `--sysroot` on
//! util-linux's own snapshots of eighteen machines -- x86 laptops and
//! servers, ARM boards, POWER, s390 in four kinds of virtual machine,
//! SPARC, RISC-V, LoongArch.
//!
//! It replaces a hand-written program that printed its own idea of the
//! summary, and none of `-e`, `-p`, `-C` or `--sysroot`.
//!
//! # Where the modules come from
//!
//! | Module | Upstream |
//! |---|---|
//! | `main.rs` | `lscpu.c`: options and output |
//! | [`types`] | `lscpu.h`, `lscpu-cpu.c` |
//! | [`cputype`] | `lscpu-cputype.c`: `/proc/cpuinfo`, CPU lists, vulnerabilities, NUMA |
//! | [`topology`] | `lscpu-topology.c`: cores, sockets, caches, frequencies |
//! | [`virt`] | `lscpu-virt.c` |
//! | [`dmi`] | `lscpu-dmi.c` and `lscpu-virt.c`'s DMI half |
//! | [`arm`] | `lscpu-arm.c` |
//! | [`cpuset`] | `lib/cpuset.c` |
//! | [`path`] | `lib/path.c` |
//! | [`cstr`] | `fgets`, `scanf`, `qsort`, `%f`, `include/strutils.h` |
//! | [`sys`] | `uname`, `sched_getaffinity`, `cpuid` |
//!
//! # What is upstream's, and easy to get wrong
//!
//! * **The machine name is `uname`'s even with `--sysroot`**, whose tree
//!   may be another architecture's; and the "Byte Order" comes from the
//!   tree's `/sys/kernel/cpu_byteorder`, else from how this binary was
//!   built.
//! * **Columns are quirky where their descriptions are.** `DRAWER` and
//!   `SCALMHZ%` skip an initializer upstream, so both keep their names in
//!   capitals in `-p`'s header and are strings in `--json`.
//! * **A CPU's drawer is 0, not `-`,** when its `topology` directory is
//!   missing: `lscpu_new_cpu` sets the book id to -1 twice and the drawer's
//!   never.
//! * **A list that does not fit prints as `(null)`.** The buffer for a CPU
//!   list is `7 * maxcpus` bytes; a snapshot whose `kernel_max` is small and
//!   whose lists are long overflows it, and upstream passes the `NULL` to
//!   `printf("%s")`.
//! * **Sizes of one** in `-C` are `size_to_human_string` with one-letter
//!   units (`32K`); in the summary three-letter ones with a space
//!   (`1.5 MiB`), summed over the instances.
//! * **Frequencies are `float`**, divided and summed in single precision
//!   and printed with `%.4f`; BogoMIPS is `(float) strtod` printed `%.2f`.
//!
//! # What is not upstream's
//!
//! Where upstream's behaviour is undefined -- a crash, a hang, a read of
//! freed or uninitialised memory -- this does something defined instead:
//!
//! * **A CPU list or mask that does not parse** is missing here. Upstream
//!   frees the set and leaves the caller its pointer, then reads it.
//! * **A `cache<N>` line in `/proc/cpuinfo` without `scope=` or `type=`**
//!   is skipped; upstream dereferences `strstr`'s `NULL` plus an offset.
//! * **A NUMA node without a `cpumap`, a cache without a
//!   `shared_cpu_map`,** and a CPU `possible` counts but that was never made
//!   (a `possible` wider than `kernel_max`), hold no CPUs here; upstream
//!   dereferences `NULL`.
//! * **A CPU range ending at 4294967295** would never end upstream (see
//!   [`cpuset::cpulist_parse`]).
//! * **A `dispatching` mode other than 0 or 1** is not shown; upstream reads
//!   past its two-entry table.
//! * **The DMI tables** are read with each out-of-range byte as zero, where
//!   upstream reads past its buffer (see [`dmi`]).
//! * **`LSCPU_DEBUG`, `LIBSMARTCOLS_DEBUG` and `ULPATH_DEBUG`** print
//!   nothing.
//! * **A file system that does not report entry types** (`DT_UNKNOWN`):
//!   upstream skips every vulnerability file on it, and counts every entry
//!   as a possible NUMA node; here each entry's type is looked up.
//! * **A name in a diagnostic** has its unprintable bytes escaped
//!   (design-decisions §370).

stdfdguard::guard_std_fds!();

mod arm;
mod cpuset;
mod cputype;
mod cstr;
mod dmi;
mod path;
mod sys;
mod topology;
mod types;
mod virt;

use getoptlong::{Opt, Program, Takes};
use quoting::{escape_unprintable, escaped_in_quotes, os_bytes};
use smartcols::{ColumnId, FL_NOEXTREMES, FL_RIGHT, FL_TREE, FL_WRAP, JsonType, LineId, Table};
use std::ffi::{OsStr, OsString};
use std::io;
use std::process::ExitCode;
use types::{Cache, Cpu, CpuType, Cxt, Mode};
use ulclosestream::{Stdout, stderr_write, warn, warnx};
use ulstrutils::{
    IdListError, SIZE_SUFFIX_1LETTER, SIZE_SUFFIX_3LETTER, SIZE_SUFFIX_SPACE,
    err_exclusive_options, size_to_human_string, string_add_to_idarray,
};

/// Getopt's errors are only sentences here; the referral follows them.
const LSCPU: Program = Program::new("lscpu", 1);

/// Upstream's option string.
const SHORTS: &str = "aBbC::ce::hJp::s:xyV";

/// Upstream's `longopts[]`, in its order, and each one's `val`.
const LONGS: &[(&str, Takes)] = &[
    ("all", Takes::Nothing),
    ("online", Takes::Nothing),
    ("bytes", Takes::Nothing),
    ("caches", Takes::Optional),
    ("offline", Takes::Nothing),
    ("help", Takes::Nothing),
    ("extended", Takes::Optional),
    ("json", Takes::Nothing),
    ("parse", Takes::Optional),
    ("sysroot", Takes::Required),
    ("physical", Takes::Nothing),
    ("hex", Takes::Nothing),
    ("version", Takes::Nothing),
    ("output-all", Takes::Nothing),
    ("hierarchic", Takes::Optional),
];

/// `OPT_OUTPUT_ALL` and `OPT_HIERARCHIC`: `CHAR_MAX + 1` and on.
const OPT_OUTPUT_ALL: i32 = 128;
const OPT_HIERARCHIC: i32 = 129;

/// Each long option's `val`, in `LONGS`' order.
const LONG_VALS: [i32; 15] = [
    b'a' as i32,
    b'b' as i32,
    b'B' as i32,
    b'C' as i32,
    b'c' as i32,
    b'h' as i32,
    b'e' as i32,
    b'J' as i32,
    b'p' as i32,
    b's' as i32,
    b'y' as i32,
    b'x' as i32,
    b'V' as i32,
    OPT_OUTPUT_ALL,
    OPT_HIERARCHIC,
];

/// `excl[]`.
const EXCL: [&[i32]; 2] = [
    &[b'C' as i32, b'e' as i32, b'p' as i32],
    &[b'a' as i32, b'b' as i32, b'c' as i32],
];

/// `virt_types[]`.
const VIRT_TYPES: [&str; 4] = ["none", "para", "full", "container"];

/// `hv_vendors[]`, `VIRT_VENDOR_NONE`'s `NULL` as the empty string.
const HV_VENDORS: [&str; 16] = [
    "",
    "Xen",
    "KVM",
    "Microsoft",
    "VMware",
    "IBM",
    "Linux-VServer",
    "User-mode Linux",
    "Innotek GmbH",
    "Hitachi",
    "Parallels",
    "Oracle",
    "OS/400",
    "pHyp",
    "Unisys s-Par",
    "Windows Subsystem for Linux",
];

/// `disp_modes[]`.
const DISP_MODES: [&str; 2] = ["horizontal", "vertical"];

/// `polar_modes[]`: parsable and readable.
const POLAR_MODES: [(&str, &str); 5] = [
    ("U", "-"),
    ("VL", "vert-low"),
    ("VM", "vert-medium"),
    ("VH", "vert-high"),
    ("H", "horizontal"),
];

/// `COL_CPU_*`, in `enum` order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CpuCol {
    Bogomips,
    Cpu,
    Core,
    Socket,
    Cluster,
    Node,
    Book,
    Drawer,
    Cache,
    Polarization,
    Address,
    Configured,
    Online,
    Mhz,
    Scalmhz,
    Maxmhz,
    Minmhz,
    Modelname,
}

/// `COL_CACHE_*`, in `enum` order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CacheCol {
    AllSize,
    Level,
    Name,
    OneSize,
    Type,
    Ways,
    AllocPol,
    WritePol,
    PhyLine,
    Sets,
    CoherencySize,
}

/// `struct lscpu_coldesc`.
struct ColDesc<C> {
    id: C,
    name: &'static str,
    help: &'static str,
    flags: u32,
    is_abbr: bool,
    json_type: JsonType,
}

/// `coldescs_cpu[]`. `DRAWER` and `SCALMHZ%` are upstream's, one
/// initializer short: `SCOLS_JSON_NUMBER` lands in `is_abbr` and the JSON
/// type stays a string.
const COLDESCS_CPU: [ColDesc<CpuCol>; 18] = [
    ColDesc {
        id: CpuCol::Bogomips,
        name: "BOGOMIPS",
        help: "crude measurement of CPU speed",
        flags: FL_RIGHT,
        is_abbr: true,
        json_type: JsonType::Number,
    },
    ColDesc {
        id: CpuCol::Cpu,
        name: "CPU",
        help: "logical CPU number",
        flags: FL_RIGHT,
        is_abbr: true,
        json_type: JsonType::Number,
    },
    ColDesc {
        id: CpuCol::Core,
        name: "CORE",
        help: "logical core number",
        flags: FL_RIGHT,
        is_abbr: false,
        json_type: JsonType::Number,
    },
    ColDesc {
        id: CpuCol::Socket,
        name: "SOCKET",
        help: "logical socket number",
        flags: FL_RIGHT,
        is_abbr: false,
        json_type: JsonType::Number,
    },
    ColDesc {
        id: CpuCol::Cluster,
        name: "CLUSTER",
        help: "logical cluster number",
        flags: FL_RIGHT,
        is_abbr: false,
        json_type: JsonType::Number,
    },
    ColDesc {
        id: CpuCol::Node,
        name: "NODE",
        help: "logical NUMA node number",
        flags: FL_RIGHT,
        is_abbr: false,
        json_type: JsonType::Number,
    },
    ColDesc {
        id: CpuCol::Book,
        name: "BOOK",
        help: "logical book number",
        flags: FL_RIGHT,
        is_abbr: false,
        json_type: JsonType::Number,
    },
    ColDesc {
        id: CpuCol::Drawer,
        name: "DRAWER",
        help: "logical drawer number",
        flags: FL_RIGHT,
        is_abbr: true,
        json_type: JsonType::String,
    },
    ColDesc {
        id: CpuCol::Cache,
        name: "CACHE",
        help: "shows how caches are shared between CPUs",
        flags: 0,
        is_abbr: false,
        json_type: JsonType::String,
    },
    ColDesc {
        id: CpuCol::Polarization,
        name: "POLARIZATION",
        help: "CPU dispatching mode on virtual hardware",
        flags: 0,
        is_abbr: false,
        json_type: JsonType::String,
    },
    ColDesc {
        id: CpuCol::Address,
        name: "ADDRESS",
        help: "physical address of a CPU",
        flags: 0,
        is_abbr: false,
        json_type: JsonType::String,
    },
    ColDesc {
        id: CpuCol::Configured,
        name: "CONFIGURED",
        help: "shows if the hypervisor has allocated the CPU",
        flags: 0,
        is_abbr: false,
        json_type: JsonType::BooleanOptional,
    },
    ColDesc {
        id: CpuCol::Online,
        name: "ONLINE",
        help: "shows if Linux currently makes use of the CPU",
        flags: FL_RIGHT,
        is_abbr: false,
        json_type: JsonType::BooleanOptional,
    },
    ColDesc {
        id: CpuCol::Mhz,
        name: "MHZ",
        help: "shows the currently MHz of the CPU",
        flags: FL_RIGHT,
        is_abbr: false,
        json_type: JsonType::Number,
    },
    ColDesc {
        id: CpuCol::Scalmhz,
        name: "SCALMHZ%",
        help: "shows scaling percentage of the CPU frequency",
        flags: FL_RIGHT,
        is_abbr: true,
        json_type: JsonType::String,
    },
    ColDesc {
        id: CpuCol::Maxmhz,
        name: "MAXMHZ",
        help: "shows the maximum MHz of the CPU",
        flags: FL_RIGHT,
        is_abbr: false,
        json_type: JsonType::Number,
    },
    ColDesc {
        id: CpuCol::Minmhz,
        name: "MINMHZ",
        help: "shows the minimum MHz of the CPU",
        flags: FL_RIGHT,
        is_abbr: false,
        json_type: JsonType::Number,
    },
    ColDesc {
        id: CpuCol::Modelname,
        name: "MODELNAME",
        help: "shows CPU model name",
        flags: 0,
        is_abbr: false,
        json_type: JsonType::String,
    },
];

/// `coldescs_cache[]`.
const COLDESCS_CACHE: [ColDesc<CacheCol>; 11] = [
    ColDesc {
        id: CacheCol::AllSize,
        name: "ALL-SIZE",
        help: "size of all system caches",
        flags: FL_RIGHT,
        is_abbr: false,
        json_type: JsonType::String,
    },
    ColDesc {
        id: CacheCol::Level,
        name: "LEVEL",
        help: "cache level",
        flags: FL_RIGHT,
        is_abbr: false,
        json_type: JsonType::Number,
    },
    ColDesc {
        id: CacheCol::Name,
        name: "NAME",
        help: "cache name",
        flags: 0,
        is_abbr: false,
        json_type: JsonType::String,
    },
    ColDesc {
        id: CacheCol::OneSize,
        name: "ONE-SIZE",
        help: "size of one cache",
        flags: FL_RIGHT,
        is_abbr: false,
        json_type: JsonType::String,
    },
    ColDesc {
        id: CacheCol::Type,
        name: "TYPE",
        help: "cache type",
        flags: 0,
        is_abbr: false,
        json_type: JsonType::String,
    },
    ColDesc {
        id: CacheCol::Ways,
        name: "WAYS",
        help: "ways of associativity",
        flags: FL_RIGHT,
        is_abbr: false,
        json_type: JsonType::Number,
    },
    ColDesc {
        id: CacheCol::AllocPol,
        name: "ALLOC-POLICY",
        help: "allocation policy",
        flags: 0,
        is_abbr: false,
        json_type: JsonType::String,
    },
    ColDesc {
        id: CacheCol::WritePol,
        name: "WRITE-POLICY",
        help: "write policy",
        flags: 0,
        is_abbr: false,
        json_type: JsonType::String,
    },
    ColDesc {
        id: CacheCol::PhyLine,
        name: "PHY-LINE",
        help: "number of physical cache line per cache tag",
        flags: FL_RIGHT,
        is_abbr: false,
        json_type: JsonType::Number,
    },
    ColDesc {
        id: CacheCol::Sets,
        name: "SETS",
        help: "number of sets in the cache; set lines has the same cache index",
        flags: FL_RIGHT,
        is_abbr: false,
        json_type: JsonType::Number,
    },
    ColDesc {
        id: CacheCol::CoherencySize,
        name: "COHERENCY-SIZE",
        help: "minimum amount of data in bytes transferred from memory to cache",
        flags: FL_RIGHT,
        is_abbr: false,
        json_type: JsonType::Number,
    },
];

/// `int columns[ARRAY_SIZE(coldescs_cpu)]`: room for this many columns,
/// in either mode.
const MAX_COLUMNS: usize = 18;

/// `BUFSIZ`: the buffer every cell is formatted into.
const BUFSIZ: usize = path::BUFSIZ;

/// A column of either table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Column {
    Cpu(CpuCol),
    Cache(CacheCol),
}

/// Why `lscpu` stopped: what `err`/`errx` would print, and the status.
#[derive(Debug)]
enum Fatal {
    /// `err(EXIT_FAILURE, "MSG")`: `lscpu: MSG: strerror(errno)`.
    Err(String, io::Error),
    /// `errx(EXIT_FAILURE, "MSG")`.
    Errx(String),
    /// Exit silently with this status (the message, if any, is out).
    Status(u8),
}

fn main() -> ExitCode {
    // Before anything touches standard I/O: a descriptor the process was
    // started without stays closed, as upstream would find it.
    stdfdguard::restore();
    let argv: Vec<OsString> = std::env::args_os().collect();
    let short = short_name(
        argv.first()
            .map_or(OsStr::new("lscpu"), OsString::as_os_str),
    );
    let mut out = Stdout::new(1);
    let status = match run(&argv, &short, &mut out) {
        Ok(status) => status,
        Err(Fatal::Err(msg, e)) => {
            warn(&short, &msg, &e);
            1
        }
        Err(Fatal::Errx(msg)) => {
            warnx(&short, &msg);
            1
        }
        Err(Fatal::Status(status)) => status,
    };
    // `close_stdout_atexit()`.
    ExitCode::from(out.close(status, &short))
}

/// `program_invocation_short_name`: argv[0] past its last `/`.
fn short_name(arg0: &OsStr) -> Vec<u8> {
    let bytes = os_bytes(arg0);
    let start = bytes
        .iter()
        .rposition(|&b| b == b'/')
        .map_or(0, |i| i.saturating_add(1));
    bytes.get(start..).unwrap_or_default().to_vec()
}

/// Bytes shown in a diagnostic: upstream's text, unprintable bytes escaped.
fn shown(text: &[u8]) -> String {
    escape_unprintable(text)
}

/// `errtryhelp(EXIT_FAILURE)`.
fn errtryhelp(short: &[u8]) -> Fatal {
    stderr_write(format!("Try '{} --help' for more information.\n", shown(short)).as_bytes());
    Fatal::Status(1)
}

/// `usage()`.
fn usage(short: &[u8]) -> Vec<u8> {
    let mut text = b"\nUsage:\n ".to_vec();
    text.extend_from_slice(short);
    text.extend_from_slice(
        b" [options]\n\
\nDisplay information about the CPU architecture.\n\
\nOptions:\n\
\x20-a, --all               print both online and offline CPUs (default for -e)\n\
\x20-b, --online            print online CPUs only (default for -p)\n\
\x20-B, --bytes             print sizes in bytes rather than in human readable format\n\
\x20-C, --caches[=<list>]   info about caches in extended readable format\n\
\x20-c, --offline           print offline CPUs only\n\
\x20-J, --json              use JSON for default or extended format\n\
\x20-e, --extended[=<list>] print out an extended readable format\n\
\x20-p, --parse[=<list>]    print out a parsable format\n\
\x20-s, --sysroot <dir>     use specified directory as system root\n\
\x20-x, --hex               print hexadecimal masks rather than lists of CPUs\n\
\x20-y, --physical          print physical instead of logical IDs\n\
\x20    --hierarchic[=when] use subsections in summary (auto, never, always)\n\
\x20    --output-all        print all available columns for -e, -p or -C\n\
\n",
    );
    text.extend_from_slice(format!("{:<25}{}\n", " -h, --help", "display this help").as_bytes());
    text.extend_from_slice(format!("{:<25}{}\n", " -V, --version", "display version").as_bytes());
    text.extend_from_slice(b"\nAvailable output columns for -e or -p:\n");
    for cd in &COLDESCS_CPU {
        text.extend_from_slice(format!(" {:>13}  {}\n", cd.name, cd.help).as_bytes());
    }
    text.extend_from_slice(b"\nAvailable output columns for -C:\n");
    for cd in &COLDESCS_CACHE {
        text.extend_from_slice(format!(" {:>13}  {}\n", cd.name, cd.help).as_bytes());
    }
    text.extend_from_slice(b"\nFor more details see lscpu(1).\n");
    text
}

/// `cpu_column_name_to_id` / `cache_column_name_to_id`: a column by its
/// name, in any case. Unknown, it is reported with the rest of the list
/// after it, as upstream's C string runs on to the list's end.
fn column_name_to_id(mode: Mode, name: &[u8], rest: &[u8], short: &[u8]) -> Option<Column> {
    let found = if mode == Mode::Caches {
        COLDESCS_CACHE
            .iter()
            .find(|cd| cd.name.as_bytes().eq_ignore_ascii_case(name))
            .map(|cd| Column::Cache(cd.id))
    } else {
        COLDESCS_CPU
            .iter()
            .find(|cd| cd.name.as_bytes().eq_ignore_ascii_case(name))
            .map(|cd| Column::Cpu(cd.id))
    };
    if found.is_none() {
        warnx(short, &format!("unknown column: {}", shown(rest)));
    }
    found
}

/// A CPU column's description.
fn cpu_desc(col: CpuCol) -> &'static ColDesc<CpuCol> {
    COLDESCS_CPU
        .iter()
        .find(|cd| cd.id == col)
        .unwrap_or(&COLDESCS_CPU[0])
}

/// A cache column's description.
fn cache_desc(col: CacheCol) -> &'static ColDesc<CacheCol> {
    COLDESCS_CACHE
        .iter()
        .find(|cd| cd.id == col)
        .unwrap_or(&COLDESCS_CACHE[0])
}

/// The option each parsed item stands for, as upstream's switch sees it.
fn option_code(opt: &Opt<'_>) -> Option<(i32, Option<OsString>)> {
    match opt {
        Opt::Short(c, value) => Some((i32::from(*c), value.clone())),
        Opt::Long(name, value) => {
            let i = LONGS.iter().position(|&(n, _)| n == *name)?;
            Some((*LONG_VALS.get(i)?, value.clone()))
        }
        Opt::Operand(_) => None,
    }
}

/// `option_to_longopt(c, longopts)`.
fn option_to_longopt(c: i32) -> Option<&'static str> {
    LONG_VALS
        .iter()
        .position(|&v| v == c)
        .and_then(|i| LONGS.get(i))
        .map(|&(name, _)| name)
}

/// The settings `main`'s option loop leaves.
struct Options {
    columns: Vec<Column>,
    outarg: Option<Vec<u8>>,
    all: bool,
    cpu_modifier_specified: bool,
    /// -1 auto, 0 never, 1 always.
    hierarchic: i32,
    operands: usize,
}

/// `main()`'s option loop, into `cxt` and the returned settings.
fn parse_options(
    argv: &[OsString],
    short: &[u8],
    cxt: &mut Cxt,
    out: &mut Stdout,
) -> Result<Options, Fatal> {
    let arg0 = argv
        .first()
        .map_or(OsStr::new("lscpu"), OsString::as_os_str);
    let mut opts = Options {
        columns: Vec::new(),
        outarg: None,
        all: false,
        cpu_modifier_specified: false,
        hierarchic: -1,
        operands: 0,
    };
    let mut excl_st = [0i32; 2];
    let own = argv.get(1..).unwrap_or_default();
    for item in LSCPU.parse(own, SHORTS, LONGS) {
        let opt = match item {
            Ok(opt) => opt,
            Err(e) => {
                // glibc names the program by argv[0] as given.
                stderr_write(format!("{}: {}\n", shown(&os_bytes(arg0)), e.sentence).as_bytes());
                return Err(errtryhelp(short));
            }
        };
        let Some((c, value)) = option_code(&opt) else {
            opts.operands = opts.operands.saturating_add(1);
            continue;
        };
        if let Some(msg) = err_exclusive_options(c, &EXCL, &mut excl_st, option_to_longopt, short) {
            stderr_write(msg.as_bytes());
            return Err(Fatal::Status(1));
        }
        let arg = value.as_deref().map(|v| os_bytes(v).into_owned());
        // `if (*optarg == '=') optarg++;`
        let list = || {
            arg.as_ref()
                .map(|a| a.strip_prefix(b"=").unwrap_or(a).to_vec())
        };
        match c {
            _ if c == i32::from(b'a') => {
                cxt.show_online = true;
                cxt.show_offline = true;
                opts.cpu_modifier_specified = true;
            }
            _ if c == i32::from(b'B') => cxt.bytes = true,
            _ if c == i32::from(b'b') => {
                cxt.show_online = true;
                opts.cpu_modifier_specified = true;
            }
            _ if c == i32::from(b'c') => {
                cxt.show_offline = true;
                opts.cpu_modifier_specified = true;
            }
            _ if c == i32::from(b'C') => {
                if let Some(l) = list() {
                    opts.outarg = Some(l);
                }
                cxt.mode = Mode::Caches;
            }
            _ if c == i32::from(b'J') => cxt.json = true,
            _ if c == i32::from(b'p') || c == i32::from(b'e') => {
                if let Some(l) = list() {
                    opts.outarg = Some(l);
                }
                cxt.mode = if c == i32::from(b'p') {
                    Mode::Parsable
                } else {
                    Mode::Readable
                };
            }
            _ if c == i32::from(b's') => {
                cxt.prefix = Some(arg.unwrap_or_default());
                cxt.noalive = true;
            }
            _ if c == i32::from(b'x') => cxt.hex = true,
            _ if c == i32::from(b'y') => cxt.show_physical = true,
            OPT_OUTPUT_ALL => opts.all = true,
            OPT_HIERARCHIC => {
                opts.hierarchic = match arg.as_deref() {
                    None => 1,
                    Some(b"auto") => -1,
                    Some(b"never") => 0,
                    Some(b"always") => 1,
                    Some(_) => return Err(Fatal::Errx("unsupported --flat argument".to_string())),
                };
            }
            _ if c == i32::from(b'h') => {
                out.write(&usage(short));
                return Err(Fatal::Status(0));
            }
            _ if c == i32::from(b'V') => {
                let mut line = short.to_vec();
                line.extend_from_slice(b" from util-linux 2.39.3\n");
                out.write(&line);
                return Err(Fatal::Status(0));
            }
            _ => return Err(errtryhelp(short)),
        }
    }
    Ok(opts)
}

/// `main()`.
fn run(argv: &[OsString], short: &[u8], out: &mut Stdout) -> Result<u8, Fatal> {
    let mut cxt = Cxt::new();
    let mut opts = parse_options(argv, short, &mut cxt, out)?;

    if opts.all && opts.columns.is_empty() {
        opts.columns = if cxt.mode == Mode::Caches {
            COLDESCS_CACHE
                .iter()
                .map(|cd| Column::Cache(cd.id))
                .collect()
        } else {
            COLDESCS_CPU.iter().map(|cd| Column::Cpu(cd.id)).collect()
        };
    }
    if opts.cpu_modifier_specified && cxt.mode == Mode::Summary {
        stderr_write(
            format!(
                "{}: options --all, --online and --offline may only be used with options --extended or --parse.\n",
                shown(short)
            )
            .as_bytes(),
        );
        return Err(Fatal::Status(1));
    }
    if opts.operands != 0 {
        warnx(short, "bad usage");
        return Err(errtryhelp(short));
    }
    // The default CPUs to show.
    if !cxt.show_online && !cxt.show_offline {
        cxt.show_online = true;
        cxt.show_offline = cxt.mode == Mode::Readable;
    }

    gather(&mut cxt)?;

    let hierarchic = if opts.hierarchic == -1 {
        std::io::IsTerminal::is_terminal(&std::io::stdout())
    } else {
        opts.hierarchic == 1
    };

    match cxt.mode {
        Mode::Summary => {
            let text = print_summary(&cxt, hierarchic)?;
            out.write(&text);
        }
        Mode::Caches => {
            if opts.columns.is_empty() {
                opts.columns = [
                    CacheCol::Name,
                    CacheCol::OneSize,
                    CacheCol::AllSize,
                    CacheCol::Ways,
                    CacheCol::Type,
                    CacheCol::Level,
                    CacheCol::Sets,
                    CacheCol::PhyLine,
                    CacheCol::CoherencySize,
                ]
                .into_iter()
                .map(Column::Cache)
                .collect();
            }
            add_outarg(&cxt, opts.outarg.as_deref(), &mut opts.columns, short)?;
            let cols: Vec<CacheCol> = opts
                .columns
                .iter()
                .filter_map(|c| {
                    if let Column::Cache(c) = c {
                        Some(*c)
                    } else {
                        None
                    }
                })
                .collect();
            let text = print_caches_readable(&cxt, &cols)?;
            out.write(&text);
        }
        Mode::Readable => {
            if opts.columns.is_empty() {
                opts.columns = default_readable_columns(&cxt)
                    .into_iter()
                    .map(Column::Cpu)
                    .collect();
            }
            add_outarg(&cxt, opts.outarg.as_deref(), &mut opts.columns, short)?;
            let cols = cpu_columns(&opts.columns);
            let text = print_cpus_readable(&cxt, &cols)?;
            out.write(&text);
        }
        Mode::Parsable => {
            cxt.show_compatible = true;
            if opts.columns.is_empty() {
                opts.columns = [
                    CpuCol::Cpu,
                    CpuCol::Core,
                    if cxt.is_cluster {
                        CpuCol::Cluster
                    } else {
                        CpuCol::Socket
                    },
                    CpuCol::Node,
                    CpuCol::Cache,
                ]
                .into_iter()
                .map(Column::Cpu)
                .collect();
            }
            if opts.outarg.is_some() {
                add_outarg(&cxt, opts.outarg.as_deref(), &mut opts.columns, short)?;
                cxt.show_compatible = false;
            }
            let cols = cpu_columns(&opts.columns);
            out.write(&print_cpus_parsable(&cxt, &cols));
        }
    }
    Ok(0)
}

/// Everything `lscpu` reads, in upstream's order: `lscpu_context_init_paths`
/// through `lscpu_read_virtualization`.
fn gather(cxt: &mut Cxt) -> Result<(), Fatal> {
    cxt.init_paths();
    cputype::read_cpulists(cxt).map_err(|e| {
        Fatal::Err(
            "failed to determine number of CPUs: /sys/devices/system/cpu/possible".to_string(),
            e.0,
        )
    })?;
    cputype::read_cpuinfo(cxt)
        .map_err(|e| Fatal::Err("cannot open /proc/cpuinfo".to_string(), e.0))?;
    cxt.arch = cputype::read_architecture(cxt)
        .map_err(|e| Fatal::Err("error: uname failed".to_string(), e))?;
    cputype::read_archext(cxt);
    cputype::read_vulnerabilities(cxt);
    cputype::read_numas(cxt).map_err(|e| {
        Fatal::Err(
            format!(
                "Failed to extract the node number: {}",
                escaped_in_quotes(&e.0)
            ),
            io::Error::from_raw_os_error(34),
        )
    })?;
    topology::read_topology(cxt);
    arm::decode_arm(cxt);
    cxt.virt = virt::read_virtualization(cxt);

    Ok(())
}

/// The CPU columns of a list.
fn cpu_columns(columns: &[Column]) -> Vec<CpuCol> {
    columns
        .iter()
        .filter_map(|c| {
            if let Column::Cpu(c) = c {
                Some(*c)
            } else {
                None
            }
        })
        .collect()
}

/// `string_add_to_idarray(outarg, columns, ...)`, whose refusal ends
/// `lscpu` with status 1 and no message of its own.
fn add_outarg(
    cxt: &Cxt,
    outarg: Option<&[u8]>,
    columns: &mut Vec<Column>,
    short: &[u8],
) -> Result<(), Fatal> {
    let Some(list) = outarg else {
        return Ok(());
    };
    match string_add_to_idarray(list, columns, MAX_COLUMNS, |name, rest| {
        column_name_to_id(cxt.mode, name, rest, short)
    }) {
        Ok(_) => Ok(()),
        Err(IdListError::Invalid | IdListError::Full) => Err(Fatal::Status(1)),
    }
}

/// `-e` without a list: whatever there is to show.
fn default_readable_columns(cxt: &Cxt) -> Vec<CpuCol> {
    let ct = cxt.default_type();
    let mut cols = vec![CpuCol::Cpu];
    if !cxt.nodes.is_empty() {
        cols.push(CpuCol::Node);
    }
    if ct.is_some_and(|ct| !ct.drawermaps.is_empty()) {
        cols.push(CpuCol::Drawer);
    }
    if ct.is_some_and(|ct| !ct.bookmaps.is_empty()) {
        cols.push(CpuCol::Book);
    }
    if ct.is_some_and(|ct| !ct.socketmaps.is_empty()) {
        cols.push(if cxt.is_cluster {
            CpuCol::Cluster
        } else {
            CpuCol::Socket
        });
    }
    if ct.is_some_and(|ct| !ct.coremaps.is_empty()) {
        cols.push(CpuCol::Core);
    }
    if !cxt.caches.is_empty() {
        cols.push(CpuCol::Cache);
    }
    if cxt.online.is_some() {
        cols.push(CpuCol::Online);
    }
    if ct.is_some_and(|ct| ct.has_configured) {
        cols.push(CpuCol::Configured);
    }
    if ct.is_some_and(|ct| ct.has_polarization) {
        cols.push(CpuCol::Polarization);
    }
    if ct.is_some_and(|ct| ct.has_addresses) {
        cols.push(CpuCol::Address);
    }
    if ct.is_some_and(|ct| ct.has_freq) {
        cols.extend([CpuCol::Maxmhz, CpuCol::Minmhz, CpuCol::Mhz]);
    }
    cols
}

/// `__fill_id(cxt, cpu, id, map, nitems, buf, bufsz)`: with `-y` the id
/// from `/sys` (`-` when unknown), else the index of the first map holding
/// the CPU.
fn fill_id(cxt: &Cxt, cpu: &Cpu, id: i32, maps: &[cpuset::CpuSet]) -> Vec<u8> {
    if cxt.show_physical {
        if id < 0 {
            b"-".to_vec()
        } else {
            id.to_string().into_bytes()
        }
    } else {
        maps.iter()
            .position(|m| m.is_set(cpu.index()))
            .map(|i| i.to_string().into_bytes())
            .unwrap_or_default()
    }
}

/// `get_cell_boolean(cxt, has_data, data, buf, bufsz)`.
fn cell_boolean(cxt: &Cxt, has_data: bool, data: bool) -> Vec<u8> {
    if !has_data {
        return Vec::new();
    }
    let text = if cxt.mode == Mode::Parsable || cxt.json {
        if data { "Y" } else { "N" }
    } else if data {
        "yes"
    } else {
        "no"
    };
    text.as_bytes().to_vec()
}

/// `(float) c_strtod(s, NULL)`.
fn strtof(s: &[u8]) -> f32 {
    ulstrutils::strtod(s).value as f32
}

/// The cache ids or names upstream joins into one cell: each piece
/// `snprintf`ed into what is left of a `BUFSIZ` buffer, `None` when one
/// does not fit or leaves no room for its separator.
fn join_cache_cells(pieces: impl Iterator<Item = Vec<u8>>, sep: u8) -> Option<Vec<u8>> {
    let mut buf = Vec::new();
    let mut sz = BUFSIZ;
    for piece in pieces {
        if piece.len() >= sz {
            return None;
        }
        sz = sz.saturating_sub(piece.len());
        buf.extend_from_slice(&piece);
        if sz < 2 {
            return None;
        }
        buf.push(sep);
        sz = sz.saturating_sub(1);
    }
    if matches!(buf.last(), Some(b',' | b':')) {
        buf.pop();
    }
    Some(buf)
}

/// The caches' distinct names, in order: `last` skipping repeats of the
/// one before.
fn distinct_cache_names(caches: &[Cache]) -> Vec<&[u8]> {
    let mut names: Vec<&[u8]> = Vec::new();
    for c in caches {
        if names.last() == Some(&c.name_bytes()) {
            continue;
        }
        names.push(c.name_bytes());
    }
    names
}

/// `get_cell_data(cxt, cpu, col, buf, bufsz)`: `None` for a CPU of no type
/// and for a cache cell that overflows.
fn cell_data(cxt: &Cxt, cpu: &Cpu, col: CpuCol) -> Option<Vec<u8>> {
    let ct: &CpuType = cxt.cputypes.get(cpu.cputype?)?;
    let data = match col {
        CpuCol::Cpu => cpu.logical_id.to_string().into_bytes(),
        CpuCol::Bogomips => match cpu.bogomips.as_deref().or(ct.bogomips.as_deref()) {
            Some(b) => cstr::fmt_f(f64::from(strtof(b)), 2).into_bytes(),
            None => Vec::new(),
        },
        CpuCol::Core => fill_id(cxt, cpu, cpu.coreid, &ct.coremaps),
        CpuCol::Socket => fill_id(cxt, cpu, cpu.socketid, &ct.socketmaps),
        CpuCol::Cluster => {
            if cxt.is_cluster {
                fill_id(cxt, cpu, cpu.socketid, &ct.socketmaps)
            } else {
                Vec::new()
            }
        }
        CpuCol::Drawer => fill_id(cxt, cpu, cpu.drawerid, &ct.drawermaps),
        CpuCol::Book => fill_id(cxt, cpu, cpu.bookid, &ct.bookmaps),
        CpuCol::Node => cxt
            .nodes
            .iter()
            .find(|n| cputype::set_has(n.map.as_ref(), cpu.index()))
            .map(|n| n.num.to_string().into_bytes())
            .unwrap_or_default(),
        CpuCol::Cache => {
            let sep = if cxt.show_compatible { b',' } else { b':' };
            let ids = distinct_cache_names(&cxt.caches)
                .into_iter()
                .filter_map(|name| topology::cpu_get_cache(cxt, cpu, name))
                .map(|ca| ca.id.to_string().into_bytes());
            join_cache_cells(ids, sep)?
        }
        CpuCol::Polarization => {
            let (parsable, readable) = usize::try_from(cpu.polarization)
                .ok()
                .and_then(|p| POLAR_MODES.get(p).copied())
                .unwrap_or(POLAR_MODES[0]);
            if cxt.mode == Mode::Parsable {
                parsable
            } else {
                readable
            }
            .as_bytes()
            .to_vec()
        }
        CpuCol::Address => {
            if cpu.address < 0 {
                Vec::new()
            } else {
                cpu.address.to_string().into_bytes()
            }
        }
        CpuCol::Configured => cell_boolean(cxt, cpu.configured >= 0, cpu.configured != 0),
        CpuCol::Online => cell_boolean(cxt, cxt.online.is_some(), cxt.is_online(cpu)),
        CpuCol::Mhz => {
            if cpu.mhz_cur_freq == 0.0 {
                Vec::new()
            } else {
                cstr::fmt_f(f64::from(cpu.mhz_cur_freq), 4).into_bytes()
            }
        }
        CpuCol::Scalmhz => {
            if cpu.mhz_cur_freq == 0.0 || cpu.mhz_max_freq == 0.0 {
                Vec::new()
            } else {
                let pct = cpu.mhz_cur_freq / cpu.mhz_max_freq * 100.0;
                format!("{}%", cstr::fmt_f(f64::from(pct), 0)).into_bytes()
            }
        }
        CpuCol::Maxmhz => {
            if cpu.mhz_max_freq == 0.0 {
                Vec::new()
            } else {
                cstr::fmt_f(f64::from(cpu.mhz_max_freq), 4).into_bytes()
            }
        }
        CpuCol::Minmhz => {
            if cpu.mhz_min_freq == 0.0 {
                Vec::new()
            } else {
                cstr::fmt_f(f64::from(cpu.mhz_min_freq), 4).into_bytes()
            }
        }
        CpuCol::Modelname => {
            // `xstrncpy(buf, modelname, bufsz)`.
            let mut m = ct.modelname.clone().unwrap_or_default();
            m.truncate(BUFSIZ.saturating_sub(1));
            m
        }
    };
    Some(data)
}

/// `get_cell_header(cxt, col, buf, bufsz)`: a column's name -- for `CACHE`
/// with caches, their names joined; `None` when those overflow.
fn cell_header(cxt: &Cxt, col: CpuCol) -> Option<Vec<u8>> {
    if col == CpuCol::Cache && !cxt.caches.is_empty() {
        let sep = if cxt.show_compatible { b',' } else { b':' };
        let names = distinct_cache_names(&cxt.caches)
            .into_iter()
            .map(<[u8]>::to_vec);
        return join_cache_cells(names, sep);
    }
    Some(cpu_desc(col).name.as_bytes().to_vec())
}

/// Whether `print_cpus_*` shows this CPU: `-b`/`-c`/`-a` against the
/// online set, then present only. A CPU `possible` counts but that was
/// never made is never shown (upstream would dereference its `NULL`).
fn shown_cpu<'c>(cxt: &Cxt, cpu: Option<&'c Cpu>) -> Option<&'c Cpu> {
    let cpu = cpu?;
    if cxt.online.is_some() {
        let online = cxt.is_online(cpu);
        if !cxt.show_offline && !online {
            return None;
        }
        if !cxt.show_online && online {
            return None;
        }
    }
    if cxt.present.is_some() && !cxt.is_present(cpu) {
        return None;
    }
    Some(cpu)
}

/// `print_cpus_parsable(cxt, cols, ncols)`: `-p`.
fn print_cpus_parsable(cxt: &Cxt, cols: &[CpuCol]) -> Vec<u8> {
    let mut out = b"# The following is the parsable format, which can be fed to other\n\
# programs. Each different item in every column has an unique ID\n\
# starting usually from zero.\n"
        .to_vec();
    out.extend_from_slice(b"# ");
    for (i, &col) in cols.iter().enumerate() {
        if col == CpuCol::Cache {
            if cxt.show_compatible && cxt.caches.is_empty() {
                continue;
            }
            if cxt.show_compatible && i != 0 {
                out.push(b',');
            }
        }
        if i > 0 {
            out.push(b',');
        }
        // `fputs(data && *data ? data : "")`: a cache header too long for
        // upstream's buffer is `NULL`, and prints as nothing.
        let Some(mut data) = cell_header(cxt, col) else {
            continue;
        };
        if !data.is_empty() && col != CpuCol::Cache && !cpu_desc(col).is_abbr {
            // "Socket", not "SOCKET".
            if let Some(rest) = data.get_mut(1..) {
                rest.make_ascii_lowercase();
            }
        }
        out.extend_from_slice(&data);
    }
    out.push(b'\n');

    for cpu in cxt.cpus.iter() {
        let Some(cpu) = shown_cpu(cxt, cpu.as_ref()) else {
            continue;
        };
        for (c, &col) in cols.iter().enumerate() {
            if cxt.show_compatible && col == CpuCol::Cache {
                if cxt.caches.is_empty() {
                    continue;
                }
                if c > 0 {
                    out.push(b',');
                }
            }
            if c > 0 {
                out.push(b',');
            }
            if let Some(data) = cell_data(cxt, cpu, col) {
                out.extend_from_slice(&data);
            }
        }
        out.push(b'\n');
    }
    out
}

/// A table's print, into bytes.
fn printed(tb: &mut Table) -> Vec<u8> {
    let mut text = Vec::new();
    // Upstream does not look at `scols_print_table`'s status.
    let _ = tb.print_into(&mut text);
    text
}

/// `err(EXIT_FAILURE, "failed to add output data")` for a table call that
/// cannot fail on ids it made itself.
fn add_failed(_e: smartcols::Error) -> Fatal {
    Fatal::Err(
        "failed to add output data".to_string(),
        io::Error::from_raw_os_error(path::EINVAL),
    )
}

/// `print_cpus_readable(cxt, cols, ncols)`: `-e`.
fn print_cpus_readable(cxt: &Cxt, cols: &[CpuCol]) -> Result<Vec<u8>, Fatal> {
    let mut tb = Table::new();
    if cxt.json {
        tb.enable_json(true);
        tb.set_name(b"cpus");
    }
    let mut ids = Vec::with_capacity(cols.len());
    for &col in cols {
        let cd = cpu_desc(col);
        let cl = match cell_header(cxt, col) {
            Some(name) => tb.new_column(&name, 0.0, cd.flags),
            None => tb.new_unnamed_column(0.0, cd.flags),
        };
        if cxt.json {
            tb.column_set_json_type(cl, cd.json_type)
                .map_err(add_failed)?;
        }
        ids.push(cl);
    }
    for cpu in cxt.cpus.iter() {
        let Some(cpu) = shown_cpu(cxt, cpu.as_ref()) else {
            continue;
        };
        let ln = tb.new_line(None).map_err(add_failed)?;
        for (&col, &cl) in cols.iter().zip(&ids) {
            let data = cell_data(cxt, cpu, col)
                .filter(|d| !d.is_empty())
                .unwrap_or_else(|| b"-".to_vec());
            tb.line_set_data(ln, cl, &data).map_err(add_failed)?;
        }
    }
    Ok(printed(&mut tb))
}

/// `caches_add_line(cxt, tb, ca, cols, ncols)`.
fn caches_add_line(cxt: &Cxt, tb: &mut Table, ca: &Cache, cols: &[CacheCol]) -> Result<(), Fatal> {
    let ln = tb.new_line(None).map_err(add_failed)?;
    let size = |sz: u64| {
        if cxt.bytes {
            sz.to_string().into_bytes()
        } else {
            size_to_human_string(SIZE_SUFFIX_1LETTER, sz).into_bytes()
        }
    };
    for (n, &col) in cols.iter().enumerate() {
        let data: Option<Vec<u8>> = match col {
            CacheCol::Name => ca.name.clone(),
            CacheCol::OneSize => (ca.size != 0).then(|| size(ca.size)),
            CacheCol::AllSize => {
                let sz = ca
                    .name
                    .as_deref()
                    .map_or(0, |name| topology::cache_full_size(cxt, name).0);
                (sz != 0).then(|| size(sz))
            }
            CacheCol::Ways => (ca.ways_of_associativity != 0)
                .then(|| ca.ways_of_associativity.to_string().into_bytes()),
            CacheCol::Type => ca.kind.clone(),
            CacheCol::Level => (ca.level != 0).then(|| ca.level.to_string().into_bytes()),
            CacheCol::AllocPol => ca.allocation_policy.clone(),
            CacheCol::WritePol => ca.write_policy.clone(),
            CacheCol::PhyLine => (ca.physical_line_partition != 0)
                .then(|| ca.physical_line_partition.to_string().into_bytes()),
            CacheCol::Sets => {
                (ca.number_of_sets != 0).then(|| ca.number_of_sets.to_string().into_bytes())
            }
            CacheCol::CoherencySize => (ca.coherency_line_size != 0)
                .then(|| ca.coherency_line_size.to_string().into_bytes()),
        };
        if let Some(data) = data {
            tb.line_refer_data(ln, n, &data).map_err(add_failed)?;
        }
    }
    Ok(())
}

/// `print_caches_readable(cxt, cols, ncols)`: `-C`.
fn print_caches_readable(cxt: &Cxt, cols: &[CacheCol]) -> Result<Vec<u8>, Fatal> {
    let mut tb = Table::new();
    if cxt.json {
        tb.enable_json(true);
        tb.set_name(b"caches");
    }
    for &col in cols {
        let cd = cache_desc(col);
        let cl = tb.new_column(cd.name.as_bytes(), 0.0, cd.flags);
        if cxt.json {
            tb.column_set_json_type(cl, cd.json_type)
                .map_err(add_failed)?;
        }
    }
    // One line per name; `last` carries over into the extra caches.
    let mut last: Option<&[u8]> = None;
    for ca in cxt.caches.iter().chain(cxt.ecaches.iter()) {
        if last == Some(ca.name_bytes()) {
            continue;
        }
        last = Some(ca.name_bytes());
        caches_add_line(cxt, &mut tb, ca, cols)?;
    }
    Ok(printed(&mut tb))
}

/// The summary table being built: `add_summary_*`'s context.
struct Summary {
    tb: Table,
    hierarchic: bool,
    field: ColumnId,
}

impl Summary {
    /// `add_summary_sprint(tb, sec, txt, fmt, ...)`: a line under `sec` --
    /// or, with no data and no hierarchy, none (and so its children go to
    /// the top).
    fn add(
        &mut self,
        sec: Option<LineId>,
        txt: &[u8],
        data: Option<Vec<u8>>,
    ) -> Result<Option<LineId>, Fatal> {
        if !self.hierarchic && data.is_none() {
            return Ok(None);
        }
        let ln = self.tb.new_line(sec).map_err(|_| {
            Fatal::Err(
                "failed to allocate output line".to_string(),
                io::Error::from_raw_os_error(path::EINVAL),
            )
        })?;
        self.tb
            .line_set_data(ln, self.field, txt)
            .map_err(add_failed)?;
        if let Some(data) = data {
            self.tb.line_refer_data(ln, 1, &data).map_err(add_failed)?;
        }
        Ok(Some(ln))
    }

    /// `add_summary_e`: a section heading.
    fn e(&mut self, sec: Option<LineId>, txt: &str) -> Result<Option<LineId>, Fatal> {
        self.add(sec, txt.as_bytes(), None)
    }

    /// `add_summary_s`.
    fn s(&mut self, sec: Option<LineId>, txt: &str, data: &[u8]) -> Result<Option<LineId>, Fatal> {
        self.add(sec, txt.as_bytes(), Some(data.to_vec()))
    }

    /// `add_summary_n`: `%zu`.
    fn n(&mut self, sec: Option<LineId>, txt: &str, n: usize) -> Result<Option<LineId>, Fatal> {
        self.add(sec, txt.as_bytes(), Some(n.to_string().into_bytes()))
    }
}

/// `print_cpuset(cxt, tb, sec, key, set)`: a list, or with `-x` a mask, in
/// a buffer of `7 * maxcpus` bytes -- `(null)` when a list overflows it.
fn print_cpuset(
    cxt: &Cxt,
    sm: &mut Summary,
    sec: Option<LineId>,
    key: &[u8],
    set: &cpuset::CpuSet,
) -> Result<(), Fatal> {
    let len = cxt.ncpus().saturating_mul(7);
    let text = if cxt.hex {
        cpuset::cpumask_create(set, len)
    } else {
        cpuset::cpulist_create(set, len).unwrap_or_else(|| b"(null)".to_vec())
    };
    sm.add(sec, key, Some(text))?;
    Ok(())
}

/// `print_summary_cputype(cxt, ct, tb, sec)`.
fn print_summary_cputype(
    cxt: &Cxt,
    t: usize,
    sm: &mut Summary,
    sec: Option<LineId>,
) -> Result<(), Fatal> {
    let Some(ct) = cxt.cputypes.get(t) else {
        return Ok(());
    };
    let sec = sm.s(sec, "Model name:", ct.modelname.as_deref().unwrap_or(b"-"))?;
    if let Some(v) = &ct.bios_modelname {
        sm.s(sec, "BIOS Model name:", v)?;
    }
    if let Some(v) = &ct.bios_family {
        sm.s(sec, "BIOS CPU family:", v)?;
    }
    if let Some(v) = &ct.machinetype {
        sm.s(sec, "Machine type:", v)?;
    }
    if let Some(v) = &ct.family {
        sm.s(sec, "CPU family:", v)?;
    }
    if let Some(v) = ct.revision.as_deref().or(ct.model.as_deref()) {
        sm.s(sec, "Model:", v)?;
    }
    sm.n(sec, "Thread(s) per core:", ct.nthreads_per_core)?;
    if cxt.is_cluster {
        sm.n(sec, "Core(s) per cluster:", ct.ncores_per_socket)?;
    } else {
        sm.n(sec, "Core(s) per socket:", ct.ncores_per_socket)?;
    }
    let or = |a: usize, b: usize| if a != 0 { a } else { b };
    if !ct.bookmaps.is_empty() {
        sm.n(sec, "Socket(s) per book:", ct.nsockets_per_book)?;
        if ct.ndrawers_per_system != 0 || !ct.drawermaps.is_empty() {
            sm.n(sec, "Book(s) per drawer:", ct.nbooks_per_drawer)?;
            sm.n(
                sec,
                "Drawer(s):",
                or(ct.ndrawers_per_system, ct.drawermaps.len()),
            )?;
        } else {
            sm.n(sec, "Book(s):", or(ct.nbooks_per_drawer, ct.bookmaps.len()))?;
        }
    } else if cxt.is_cluster {
        if ct.nr_socket_on_cluster > 0 {
            sm.n(sec, "Socket(s):", ct.nr_socket_on_cluster)?;
        } else {
            sm.s(sec, "Socket(s):", b"-")?;
        }
        sm.n(
            sec,
            "Cluster(s):",
            or(ct.nsockets_per_book, ct.socketmaps.len()),
        )?;
    } else {
        sm.n(
            sec,
            "Socket(s):",
            or(ct.nsockets_per_book, ct.socketmaps.len()),
        )?;
    }
    if let Some(v) = &ct.stepping {
        sm.s(sec, "Stepping:", v)?;
    }
    if ct.freqboost >= 0 {
        let v: &[u8] = if ct.freqboost != 0 {
            b"enabled"
        } else {
            b"disabled"
        };
        sm.s(sec, "Frequency boost:", v)?;
    }
    if let Some(v) = &ct.dynamic_mhz {
        sm.s(sec, "CPU dynamic MHz:", v)?;
    }
    if let Some(v) = &ct.static_mhz {
        sm.s(sec, "CPU static MHz:", v)?;
    }
    if ct.has_freq {
        let scal = topology::get_scalmhz(cxt, t);
        if scal > 0.0 {
            sm.s(
                sec,
                "CPU(s) scaling MHz:",
                format!("{}%", cstr::fmt_f(f64::from(scal), 0)).as_bytes(),
            )?;
        }
        sm.s(
            sec,
            "CPU max MHz:",
            cstr::fmt_f(f64::from(topology::get_maxmhz(cxt, t)), 4).as_bytes(),
        )?;
        sm.s(
            sec,
            "CPU min MHz:",
            cstr::fmt_f(f64::from(topology::get_minmhz(cxt, t)), 4).as_bytes(),
        )?;
    }
    if let Some(v) = &ct.bogomips {
        sm.s(
            sec,
            "BogoMIPS:",
            cstr::fmt_f(f64::from(strtof(v)), 2).as_bytes(),
        )?;
    }
    if let Some(mode) = usize::try_from(ct.dispatching)
        .ok()
        .and_then(|d| DISP_MODES.get(d))
    {
        sm.s(sec, "Dispatching mode:", mode.as_bytes())?;
    }
    if ct.physsockets != 0 {
        sm.n(sec, "Physical sockets:", ct.physsockets)?;
        sm.n(sec, "Physical chips:", ct.physchips)?;
        sm.n(sec, "Physical cores/chip:", ct.physcoresperchip)?;
    }
    if let Some(v) = &ct.flags {
        sm.s(sec, "Flags:", v)?;
    }
    Ok(())
}

/// `sysfs_get_byteorder(cxt->rootfs)`: little unless the tree says `big`;
/// what this binary was built for when it says neither.
fn little_endian(cxt: &Cxt) -> bool {
    const PATH: &[u8] = b"/sys/kernel/cpu_byteorder";
    let read = match &cxt.rootfs {
        Some(root) => root.read_buffer(PATH, BUFSIZ).ok(),
        None => {
            // No prefix: the path as it is.
            let root = path::PathCxt::new(b"/");
            root.read_buffer(PATH, BUFSIZ).ok()
        }
    };
    match read.as_ref().map(|(_, s)| s.as_slice()) {
        Some(b"little") => true,
        Some(b"big") => false,
        _ => cfg!(target_endian = "little"),
    }
}

/// `print_summary(cxt)`: the default output.
#[allow(
    clippy::too_many_lines,
    reason = "upstream's function, section by section, kept in one piece to be read against it"
)]
fn print_summary(cxt: &Cxt, hierarchic: bool) -> Result<Vec<u8>, Fatal> {
    let mut tb = Table::new();
    tb.enable_noheadings(true);
    if cxt.json {
        tb.enable_json(true);
        tb.set_name(b"lscpu");
    } else if hierarchic {
        tb.set_tree_symbols(b"  ", b"  ", b"  ");
    }
    let field = tb.new_column(b"field", 0.0, if hierarchic { FL_TREE } else { 0 });
    tb.new_column(b"data", 0.0, FL_NOEXTREMES | FL_WRAP);
    let mut sm = Summary {
        tb,
        hierarchic,
        field,
    };
    let ct = cxt.default_type();

    // Architecture.
    let sec = sm.s(None, "Architecture:", &cxt.arch.name)?;
    if cxt.arch.bit32 || cxt.arch.bit64 {
        let modes: Vec<&str> = [(cxt.arch.bit32, "32-bit"), (cxt.arch.bit64, "64-bit")]
            .iter()
            .filter(|(on, _)| *on)
            .map(|&(_, m)| m)
            .collect();
        sm.s(sec, "CPU op-mode(s):", modes.join(", ").as_bytes())?;
    }
    if let Some(v) = ct.and_then(|ct| ct.addrsz.as_deref()) {
        sm.s(sec, "Address sizes:", v)?;
    }
    let order: &[u8] = if little_endian(cxt) {
        b"Little Endian"
    } else {
        b"Big Endian"
    };
    sm.s(sec, "Byte Order:", order)?;

    // CPU lists.
    let sec = sm.n(None, "CPU(s):", cxt.npresents)?;
    if let Some(online) = &cxt.online {
        let key: &[u8] = if cxt.hex {
            b"On-line CPU(s) mask:"
        } else {
            b"On-line CPU(s) list:"
        };
        print_cpuset(cxt, &mut sm, sec, key, online)?;
        if cxt.nonlines != cxt.npresents {
            // The kernel's own `offline` includes CPUs that are not
            // present; these are the present ones that are not online.
            let mut set = cpuset::CpuSet::new(cxt.ncpus());
            for cpu in cxt.cpus.iter().flatten() {
                if cxt.is_present(cpu) && !cxt.is_online(cpu) {
                    set.set(cpu.index());
                }
            }
            let key: &[u8] = if cxt.hex {
                b"Off-line CPU(s) mask:"
            } else {
                b"Off-line CPU(s) list:"
            };
            print_cpuset(cxt, &mut sm, sec, key, &set)?;
        }
    }

    // The CPU types.
    let mut sec = None;
    if let Some(v) = ct.and_then(|ct| ct.vendor.as_deref()) {
        sec = sm.s(None, "Vendor ID:", v)?;
    }
    if let Some(v) = ct.and_then(|ct| ct.bios_vendor.as_deref()) {
        sm.s(sec, "BIOS Vendor ID:", v)?;
    }
    for t in 0..cxt.cputypes.len() {
        print_summary_cputype(cxt, t, &mut sm, sec)?;
    }

    // Virtualization.
    if let Some(virt) = &cxt.virt {
        let sec = sm.e(None, "Virtualization features:")?;
        match virt.cpuflag {
            Some("svm") => {
                sm.s(sec, "Virtualization:", b"AMD-V")?;
            }
            Some("vmx") => {
                sm.s(sec, "Virtualization:", b"VT-x")?;
            }
            _ => {}
        }
        if let Some(h) = &virt.hypervisor {
            sm.s(sec, "Hypervisor:", h)?;
        }
        if virt.vendor != types::VIRT_VENDOR_NONE {
            let vendor = HV_VENDORS.get(virt.vendor).copied().unwrap_or_default();
            sm.s(sec, "Hypervisor vendor:", vendor.as_bytes())?;
            let kind = VIRT_TYPES.get(virt.kind).copied().unwrap_or_default();
            sm.s(sec, "Virtualization type:", kind.as_bytes())?;
        }
    }

    // Caches.
    let mut hdr = false;
    let mut sec = None;
    let mut last: Option<&[u8]> = None;
    for ca in &cxt.caches {
        let name = ca.name_bytes();
        if last == Some(name) {
            continue;
        }
        let (sz, n) = topology::cache_full_size(cxt, name);
        if sz == 0 {
            continue;
        }
        if !hdr {
            sec = sm.e(None, "Caches (sum of all):")?;
            hdr = true;
        }
        let field = cache_field(name, hierarchic);
        let noun = if n == 1 { "instance" } else { "instances" };
        let data = if cxt.bytes {
            format!("{sz} ({n} {noun})")
        } else {
            format!(
                "{} ({n} {noun})",
                size_to_human_string(SIZE_SUFFIX_3LETTER | SIZE_SUFFIX_SPACE, sz)
            )
        };
        sm.add(sec, &field, Some(data.into_bytes()))?;
        last = Some(name);
    }
    for ca in &cxt.ecaches {
        if ca.size == 0 {
            continue;
        }
        if !hdr {
            sec = sm.e(None, "Caches:")?;
            hdr = true;
        }
        let field = cache_field(ca.name_bytes(), hierarchic);
        let data = if cxt.bytes {
            ca.size.to_string()
        } else {
            size_to_human_string(SIZE_SUFFIX_3LETTER | SIZE_SUFFIX_SPACE, ca.size)
        };
        sm.add(sec, &field, Some(data.into_bytes()))?;
    }

    // NUMA.
    if !cxt.nodes.is_empty() {
        let sec = sm.e(None, "NUMA:")?;
        sm.n(sec, "NUMA node(s):", cxt.nodes.len())?;
        for node in &cxt.nodes {
            let key = format!("NUMA node{} CPU(s):", node.num);
            let empty = cpuset::CpuSet::new(cxt.ncpus());
            print_cpuset(
                cxt,
                &mut sm,
                sec,
                key.as_bytes(),
                node.map.as_ref().unwrap_or(&empty),
            )?;
        }
    }

    // Vulnerabilities.
    if let Some(vuls) = &cxt.vuls {
        let sec = sm.e(None, "Vulnerabilities:")?;
        for vu in vuls {
            let mut field = if hierarchic {
                Vec::new()
            } else {
                b"Vulnerability ".to_vec()
            };
            field.extend_from_slice(&vu.name);
            field.push(b':');
            field.truncate(255);
            sm.add(sec, &field, Some(vu.text.clone()))?;
        }
    }
    Ok(printed(&mut sm.tb))
}

/// `snprintf(field, 256, hierarchic ? "%s:" : "%s cache:", name)`.
fn cache_field(name: &[u8], hierarchic: bool) -> Vec<u8> {
    let mut field = name.to_vec();
    field.extend_from_slice(if hierarchic { b":" } else { b" cache:" });
    field.truncate(255);
    field
}

#[cfg(test)]
mod tests;
