//! A small machine's `/sys` and `/proc`, built in a scratch directory, read
//! through `--sysroot` and printed.
//!
//! The machine: four CPUs, two cores of two threads, one socket; L1d, L1i
//! and L2 per core and one L3; one NUMA node; CPU 3 present but offline.
//! What each printer shows for it was worked out from upstream's code, and
//! `scripts/lscpu-diff.sh` holds the same kind of tree against upstream
//! itself.

#![allow(clippy::indexing_slicing, clippy::panic)]

use super::*;
use std::fs;

struct Machine {
    dir: scratchdir::ScratchDir,
}

impl Machine {
    fn put(&self, rel: &str, data: &str) {
        let path = self.dir.path(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).ok();
        }
        fs::write(path, data).ok();
    }

    fn root(&self) -> Vec<u8> {
        os_bytes(self.dir.dir().as_os_str()).into_owned()
    }
}

fn machine() -> Machine {
    let m = Machine {
        dir: scratchdir::ScratchDir::new("lscpu-machine"),
    };
    let cpu = "sys/devices/system/cpu";
    m.put(&format!("{cpu}/kernel_max"), "63\n");
    m.put(&format!("{cpu}/possible"), "0-3\n");
    m.put(&format!("{cpu}/present"), "0-3\n");
    m.put(&format!("{cpu}/online"), "0-2\n");
    for n in 0..4u32 {
        let core = n / 2;
        let siblings = if core == 0 { "3" } else { "c" };
        let t = format!("{cpu}/cpu{n}/topology");
        m.put(&format!("{t}/thread_siblings"), &format!("{siblings}\n"));
        m.put(&format!("{t}/core_siblings"), "f\n");
        m.put(&format!("{t}/core_id"), &format!("{core}\n"));
        m.put(&format!("{t}/physical_package_id"), "0\n");
        let f = format!("{cpu}/cpu{n}/cpufreq");
        m.put(&format!("{f}/cpuinfo_max_freq"), "3000000\n");
        m.put(&format!("{f}/cpuinfo_min_freq"), "800000\n");
        m.put(&format!("{f}/scaling_cur_freq"), "1500000\n");
        let caches = [
            ("Data", 1, "32K", siblings),
            ("Instruction", 1, "32K", siblings),
            ("Unified", 2, "256K", siblings),
            ("Unified", 3, "6144K", "f"),
        ];
        for (i, (kind, level, size, map)) in caches.iter().enumerate() {
            let c = format!("{cpu}/cpu{n}/cache/index{i}");
            m.put(&format!("{c}/type"), &format!("{kind}\n"));
            m.put(&format!("{c}/level"), &format!("{level}\n"));
            m.put(&format!("{c}/size"), &format!("{size}\n"));
            m.put(&format!("{c}/shared_cpu_map"), &format!("{map}\n"));
            m.put(&format!("{c}/ways_of_associativity"), "8\n");
            m.put(&format!("{c}/coherency_line_size"), "64\n");
        }
    }
    m.put(
        &format!("{cpu}/vulnerabilities/meltdown"),
        "Mitigation: PTI\n",
    );
    m.put(
        &format!("{cpu}/vulnerabilities/spectre_v1"),
        "Mitigation: usercopy/swapgs barriers\n",
    );
    m.put("sys/devices/system/node/node0/cpumap", "f\n");
    let mut info = String::new();
    for n in 0..4 {
        info.push_str(&format!(
            "processor\t: {n}\nvendor_id\t: GenuineIntel\ncpu family\t: 6\nmodel\t\t: 142\n\
             model name\t: Test CPU @ 3.00GHz\nstepping\t: 10\ncpu MHz\t\t: 1500.000\n\
             flags\t\t: fpu vme lm vmx\nbogomips\t: 5999.99\naddress sizes\t: 39 bits physical, 48 bits virtual\n\n"
        ));
    }
    m.put("proc/cpuinfo", &info);
    m
}

fn gathered(m: &Machine) -> Cxt {
    let mut cxt = Cxt::new();
    cxt.prefix = Some(m.root());
    cxt.noalive = true;
    cxt.show_online = true;
    let gathered = gather(&mut cxt);
    assert!(gathered.is_ok(), "{gathered:?}");
    cxt
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8(bytes.to_vec()).unwrap_or_default()
}

#[test]
fn the_machine_is_read_as_upstream_reads_it() {
    let m = machine();
    let cxt = gathered(&m);
    assert_eq!(cxt.maxcpus, 64);
    assert_eq!(cxt.cpus.len(), 4);
    assert_eq!(cxt.npresents, 4);
    assert_eq!(cxt.nonlines, 3);
    assert_eq!(cxt.cputypes.len(), 1);
    let ct = &cxt.cputypes[0];
    assert_eq!(ct.coremaps.len(), 2);
    assert_eq!(ct.socketmaps.len(), 1);
    assert_eq!(ct.nthreads_per_core, 2);
    assert_eq!(ct.ncores_per_socket, 2);
    assert!(ct.has_freq);
    // L1d, L1i, L2 per core, one L3, sorted by name.
    let names: Vec<String> = cxt.caches.iter().map(|c| text(c.name_bytes())).collect();
    assert_eq!(names, ["L1d", "L1d", "L1i", "L1i", "L2", "L2", "L3"]);
    assert_eq!(cxt.nodes.len(), 1);
    let vuls = cxt.vuls.as_ref().map(Vec::len);
    assert_eq!(vuls, Some(2));
}

#[test]
fn parsable_output_is_upstreams() {
    let m = machine();
    let mut cxt = gathered(&m);
    cxt.mode = Mode::Parsable;
    cxt.show_compatible = true;
    let cols = [
        CpuCol::Cpu,
        CpuCol::Core,
        CpuCol::Socket,
        CpuCol::Node,
        CpuCol::Cache,
    ];
    let out = text(&print_cpus_parsable(&cxt, &cols));
    let expected = "# The following is the parsable format, which can be fed to other\n\
# programs. Each different item in every column has an unique ID\n\
# starting usually from zero.\n\
# CPU,Core,Socket,Node,,L1d,L1i,L2,L3\n\
0,0,0,0,,0,0,0,0\n\
1,0,0,0,,0,0,0,0\n\
2,1,0,0,,1,1,1,0\n";
    assert_eq!(out, expected);
}

#[test]
fn user_columns_are_not_compatible() {
    let m = machine();
    let mut cxt = gathered(&m);
    cxt.mode = Mode::Parsable;
    cxt.show_offline = true;
    let cols = [
        CpuCol::Cpu,
        CpuCol::Cache,
        CpuCol::Online,
        CpuCol::Drawer,
        CpuCol::Scalmhz,
        CpuCol::Maxmhz,
    ];
    let out = text(&print_cpus_parsable(&cxt, &cols));
    let lines: Vec<&str> = out.lines().skip(3).collect();
    assert_eq!(
        lines[0],
        "# CPU,L1d:L1i:L2:L3,Online,DRAWER,SCALMHZ%,Maxmhz"
    );
    // Without -y a drawer is the index of its map, and there are none.
    assert_eq!(lines[1], "0,0:0:0:0,Y,,50%,3000.0000");
    assert_eq!(lines[4], "3,1:1:1:0,N,,50%,3000.0000");
}

#[test]
fn physical_ids_and_the_drawer_quirk() {
    let m = machine();
    let mut cxt = gathered(&m);
    cxt.mode = Mode::Parsable;
    cxt.show_physical = true;
    let cols = [CpuCol::Cpu, CpuCol::Core, CpuCol::Book, CpuCol::Drawer];
    let out = text(&print_cpus_parsable(&cxt, &cols));
    // No book_id or drawer_id file: -1 for the book, but the drawer reads
    // -1 too once `topology/` exists.
    assert!(
        out.ends_with("# CPU,Core,Book,DRAWER\n0,0,-,-\n1,0,-,-\n2,1,-,-\n"),
        "{out}"
    );
}

#[test]
fn the_summary_is_upstreams() {
    let m = machine();
    let cxt = gathered(&m);
    let out = text(&print_summary(&cxt, false).unwrap_or_default());
    // The widest field, "Vulnerability Spectre v1:", is 25 columns.
    let want = [
        ("CPU op-mode(s):", "32-bit, 64-bit"),
        ("Address sizes:", "39 bits physical, 48 bits virtual"),
        ("CPU(s):", "4"),
        ("On-line CPU(s) list:", "0-2"),
        ("Off-line CPU(s) list:", "3"),
        ("Vendor ID:", "GenuineIntel"),
        ("Model name:", "Test CPU @ 3.00GHz"),
        ("CPU family:", "6"),
        ("Model:", "142"),
        ("Thread(s) per core:", "2"),
        ("Core(s) per socket:", "2"),
        ("Socket(s):", "1"),
        ("Stepping:", "10"),
        ("CPU(s) scaling MHz:", "50%"),
        ("CPU max MHz:", "3000.0000"),
        ("CPU min MHz:", "800.0000"),
        ("BogoMIPS:", "5999.99"),
        ("Flags:", "fpu vme lm vmx"),
        ("Virtualization:", "VT-x"),
        ("L1d cache:", "64 KiB (2 instances)"),
        ("L3 cache:", "6 MiB (1 instance)"),
        ("NUMA node(s):", "1"),
        ("NUMA node0 CPU(s):", "0-3"),
        ("Vulnerability Meltdown:", "Mitigation; PTI"),
        (
            "Vulnerability Spectre v1:",
            "Mitigation; usercopy/swapgs barriers",
        ),
    ];
    for (field, data) in want {
        let line = format!("{field:<25} {data}");
        assert!(out.lines().any(|l| l == line), "missing {line:?} in\n{out}");
    }
}

#[test]
fn the_hierarchic_summary_nests() {
    let m = machine();
    let cxt = gathered(&m);
    let out = text(&print_summary(&cxt, true).unwrap_or_default());
    assert!(
        out.lines().any(|l| l.starts_with("Caches (sum of all):")),
        "{out}"
    );
    assert!(out.lines().any(|l| l.starts_with("  L1d:")), "{out}");
    assert!(
        out.lines().any(|l| l.starts_with("Vulnerabilities:")),
        "{out}"
    );
    assert!(
        out.lines()
            .any(|l| l.starts_with("    Thread(s) per core:")),
        "{out}"
    );
}

#[test]
fn caches_table_is_upstreams() {
    let m = machine();
    let mut cxt = gathered(&m);
    cxt.mode = Mode::Caches;
    let cols = [
        CacheCol::Name,
        CacheCol::OneSize,
        CacheCol::AllSize,
        CacheCol::Ways,
        CacheCol::Type,
        CacheCol::Level,
    ];
    let out = text(&print_caches_readable(&cxt, &cols).unwrap_or_default());
    let expected = "NAME ONE-SIZE ALL-SIZE WAYS TYPE        LEVEL\n\
L1d       32K      64K    8 Data            1\n\
L1i       32K      64K    8 Instruction     1\n\
L2       256K     512K    8 Unified         2\n\
L3         6M       6M    8 Unified         3\n";
    assert_eq!(out, expected);
}

#[test]
fn readable_defaults_show_what_there_is() {
    let m = machine();
    let cxt = gathered(&m);
    let cols = default_readable_columns(&cxt);
    assert_eq!(
        cols,
        [
            CpuCol::Cpu,
            CpuCol::Node,
            CpuCol::Socket,
            CpuCol::Core,
            CpuCol::Cache,
            CpuCol::Online,
            CpuCol::Maxmhz,
            CpuCol::Minmhz,
            CpuCol::Mhz
        ]
    );
}

#[test]
fn json_booleans_are_y_and_n() {
    let m = machine();
    let mut cxt = gathered(&m);
    cxt.mode = Mode::Readable;
    cxt.json = true;
    cxt.show_offline = true;
    let out = text(&print_cpus_readable(&cxt, &[CpuCol::Cpu, CpuCol::Online]).unwrap_or_default());
    assert!(out.contains("\"online\": true"), "{out}");
    assert!(out.contains("\"online\": false"), "{out}");
    assert!(out.starts_with("{\n   \"cpus\": ["), "{out}");
}

#[test]
fn a_missing_root_is_upstreams_error() {
    let mut cxt = Cxt::new();
    cxt.prefix = Some(b"/nonexistent-lscpu-root".to_vec());
    cxt.noalive = true;
    match gather(&mut cxt) {
        Err(Fatal::Err(msg, _)) => assert_eq!(
            msg,
            "failed to determine number of CPUs: /sys/devices/system/cpu/possible"
        ),
        other => panic!("{other:?}"),
    }
}

#[test]
fn lists_too_long_for_their_buffer_are_null() {
    let m = machine();
    // One CPU's worth of buffer: 7 bytes for the whole list -- both to
    // read it (six bytes and the NUL) and to print it.
    m.put("sys/devices/system/cpu/kernel_max", "0\n");
    m.put("sys/devices/system/cpu/online", "0,2,3\n");
    let cxt = gathered(&m);
    let out = text(&print_summary(&cxt, false).unwrap_or_default());
    let line = format!("{:<25} {}", "On-line CPU(s) list:", "0,2,3");
    assert!(out.lines().any(|l| l == line), "{out}");
    // Six bytes to read, but eight to print as "9,11,13,".
    m.put("sys/devices/system/cpu/online", "9-13:2\n");
    let cxt = gathered(&m);
    let out = text(&print_summary(&cxt, false).unwrap_or_default());
    let line = format!("{:<25} {}", "On-line CPU(s) list:", "(null)");
    assert!(out.lines().any(|l| l == line), "{out}");
}

#[test]
fn usage_matches_upstream() {
    let text = text(&usage(b"lscpu"));
    assert!(text.starts_with(
        "\nUsage:\n lscpu [options]\n\nDisplay information about the CPU architecture.\n"
    ));
    assert!(text.contains("\n -h, --help              display this help\n"));
    assert!(text.contains("\n      SCALMHZ%  shows scaling percentage of the CPU frequency\n"));
    assert!(text.contains(
        "\n COHERENCY-SIZE  minimum amount of data in bytes transferred from memory to cache\n"
    ));
    assert!(text.ends_with("\nFor more details see lscpu(1).\n"));
}
