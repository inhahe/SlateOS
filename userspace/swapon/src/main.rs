//! swapon -- enable devices and files for paging and swapping.
//!
//! A port of util-linux 2.39.3's `sys-utils/swapon.c`, function by function
//! and with upstream's names, on `ulmount` (libmount: `/proc/swaps`, fstab,
//! and resolving `LABEL=`/`UUID=` specs), `ulblkid` (libblkid: the swap
//! area's UUID and label) and `smartcols` (the `--show` table). What it
//! shares with `swapoff` is in the crate's library (swapon-common.c).
//! Measured against `swapon from util-linux 2.39.3` by
//! `scripts/swapon-diff.sh`.
//!
//! Before an area is enabled it is checked as upstream checks it: its
//! permissions and owner, holes in a swap file, the page size its header
//! was made for (re-made with `mkswap` under `--fixpgsz`), and old software
//! suspend data, which is overwritten with the swap signature.
//!
//! # What is not upstream's
//!
//! * **A name in a diagnostic** has its unprintable bytes escaped
//!   (design-decisions §370).
//! * **`mkswap` failing to start** is reported by swapon, where upstream's
//!   forked child reports it; the words and the outcome are the same.

use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::process::ExitCode;

use getoptlong::{Opt, Program, Takes};
use smartcols::Table;
use swapon::{
    Common, Stdout, errtryhelp, get_swap_prober, match_swap, probe_value, short_name, shown,
    stderr_write, strtos16_or_err, warn_errno, warnx,
};
use ulmount::tab::{Direction, Iter};

/// Getopt's errors are only sentences here; the referral follows them.
const SWAPON: Program = Program::new("swapon", 1);

/// Upstream's option string.
const SHORTS: &str = "ahd::efo:p:svVL:U:T:";

/// The long-only options: `CHAR_MAX + 1` on.
const BYTES_OPTION: i32 = 128;
const NOHEADINGS_OPTION: i32 = 129;
const RAW_OPTION: i32 = 130;
const SHOW_OPTION: i32 = 131;
const OPT_LIST_TYPES: i32 = 132;

/// Upstream's `long_opts[]`, in its order, and each one's `val`.
const LONGS: &[(&str, Takes)] = &[
    ("priority", Takes::Required),
    ("discard", Takes::Optional),
    ("ifexists", Takes::Nothing),
    ("options", Takes::Optional),
    ("summary", Takes::Nothing),
    ("fixpgsz", Takes::Nothing),
    ("all", Takes::Nothing),
    ("help", Takes::Nothing),
    ("verbose", Takes::Nothing),
    ("version", Takes::Nothing),
    ("show", Takes::Optional),
    ("output-all", Takes::Nothing),
    ("noheadings", Takes::Nothing),
    ("raw", Takes::Nothing),
    ("bytes", Takes::Nothing),
    ("fstab", Takes::Required),
];
const LONG_VALS: [i32; 16] = [
    b'p' as i32,
    b'd' as i32,
    b'e' as i32,
    b'o' as i32,
    b's' as i32,
    b'f' as i32,
    b'a' as i32,
    b'h' as i32,
    b'v' as i32,
    b'V' as i32,
    SHOW_OPTION,
    OPT_LIST_TYPES,
    NOHEADINGS_OPTION,
    RAW_OPTION,
    BYTES_OPTION,
    b'T' as i32,
];

/// `excl[]`: rows and columns in ASCII order.
const EXCL: [&[i32]; 4] = [
    &[b'a' as i32, b'o' as i32, b's' as i32, SHOW_OPTION],
    &[b'a' as i32, b'o' as i32, BYTES_OPTION],
    &[b'a' as i32, b'o' as i32, NOHEADINGS_OPTION],
    &[b'a' as i32, b'o' as i32, RAW_OPTION],
];

/// `SWAP_FLAG_DISCARD`, `_ONCE` and `_PAGES`.
const SWAP_FLAG_DISCARD: i32 = 0x1_0000;
const SWAP_FLAG_DISCARD_ONCE: i32 = 0x2_0000;
const SWAP_FLAG_DISCARD_PAGES: i32 = 0x4_0000;
const SWAP_FLAGS_DISCARD_VALID: i32 =
    SWAP_FLAG_DISCARD | SWAP_FLAG_DISCARD_ONCE | SWAP_FLAG_DISCARD_PAGES;
/// `SWAP_FLAG_PREFER`, `SWAP_FLAG_PRIO_MASK` (the shift is 0).
const SWAP_FLAG_PREFER: i32 = 0x8000;
const SWAP_FLAG_PRIO_MASK: i32 = 0x7fff;

/// `MAX_PAGESIZE`.
const MAX_PAGESIZE: usize = 64 * 1024;
/// `SWAP_SIGNATURE` and its size.
const SWAP_SIGNATURE: &[u8] = b"SWAPSPACE2";
const SWAP_SIGNATURE_SZ: usize = SWAP_SIGNATURE.len();

/// What `swap_detect_signature` found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Sig {
    SwapSpace,
    SwSuspend,
}

/// The `--show` columns, `COL_*`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Col {
    Path,
    Type,
    Size,
    Used,
    Prio,
    Uuid,
    Label,
}

/// `infos[]`: name, flags and help, each column 0.20 of the terminal.
const INFOS: [(Col, &str, u32, &str); 7] = [
    (Col::Path, "NAME", 0, "device file or partition path"),
    (Col::Type, "TYPE", smartcols::FL_TRUNC, "type of the device"),
    (
        Col::Size,
        "SIZE",
        smartcols::FL_RIGHT,
        "size of the swap area",
    ),
    (Col::Used, "USED", smartcols::FL_RIGHT, "bytes in use"),
    (Col::Prio, "PRIO", smartcols::FL_RIGHT, "swap priority"),
    (Col::Uuid, "UUID", 0, "swap uuid"),
    (Col::Label, "LABEL", 0, "swap label"),
];

/// `struct swap_prop`.
#[derive(Clone, Copy, Debug, Default)]
struct SwapProp {
    discard: i32,
    priority: i32,
    no_fail: bool,
}

/// `struct swap_device`.
#[derive(Clone, Debug, Default)]
struct SwapDevice {
    path: Vec<u8>,
    label: Option<Vec<u8>>,
    uuid: Option<Vec<u8>>,
    pagesize: usize,
}

/// `struct swapon_ctl`.
#[derive(Default)]
struct Ctl {
    columns: Vec<Col>,
    props: SwapProp,
    all: bool,
    bytes: bool,
    fix_page_size: bool,
    no_heading: bool,
    raw: bool,
    show: bool,
    verbose: bool,
}

/// `column_name_to_id(name, namesz)`: an exact name, in any case. An
/// unknown one is reported with the rest of the list after it, as
/// upstream's C string runs on to the list's end.
fn column_name_to_id(name: &[u8], rest: &[u8], short: &[u8]) -> Option<Col> {
    let found = INFOS
        .iter()
        .find(|(_, n, _, _)| n.as_bytes().eq_ignore_ascii_case(name))
        .map(|&(c, ..)| c);
    if found.is_none() {
        warnx(short, &format!("unknown column: {}", shown(rest)));
    }
    found
}

/// `(size_t) off_t` for `size_to_human_string`, as C converts it.
#[allow(
    clippy::cast_sign_loss,
    reason = "C passes the off_t to a uint64_t parameter"
)]
fn human(size: i64) -> Vec<u8> {
    ulstrutils::size_to_human_string(ulstrutils::SIZE_SUFFIX_1LETTER, size as u64).into_bytes()
}

/// `add_scols_line(ctl, table, fs)`.
fn add_scols_line(ctl: &Ctl, common: &Common, table: &mut Table, fs: &ulmount::fs::Fs) {
    let Ok(line) = table.new_line(None) else {
        return;
    };
    let source = fs.source.clone();
    let pr = source
        .as_deref()
        .filter(|s| access_r(s))
        .and_then(|s| get_swap_prober(&common.short, s));
    // `%s` of NULL is "(null)" in glibc.
    let or_null = |v: Option<&Vec<u8>>| v.cloned().unwrap_or_else(|| b"(null)".to_vec());
    for (i, &col) in ctl.columns.iter().enumerate() {
        let data: Option<Vec<u8>> = match col {
            Col::Path => Some(or_null(fs.source.as_ref())),
            Col::Type => Some(or_null(fs.swaptype.as_ref())),
            Col::Size | Col::Used => {
                let kib = if col == Col::Size {
                    fs.size
                } else {
                    fs.usedsize
                };
                let size = kib.wrapping_mul(1024);
                Some(if ctl.bytes {
                    size.to_string().into_bytes()
                } else {
                    human(size)
                })
            }
            Col::Prio => Some(fs.priority.to_string().into_bytes()),
            Col::Uuid => pr.as_ref().and_then(|p| probe_value(p, "UUID")),
            Col::Label => pr.as_ref().and_then(|p| probe_value(p, "LABEL")),
        };
        if let Some(d) = data {
            // The line and its cell are the table's.
            let _ = table.line_refer_data(line, i, &d);
        }
    }
}

/// `access(path, R_OK) == 0`.
fn access_r(path: &[u8]) -> bool {
    #[cfg(unix)]
    {
        unsafe extern "C" {
            fn access(path: *const u8, mode: i32) -> i32;
        }
        if path.contains(&0) {
            return false;
        }
        let c = [path, b"\0"].concat();
        // SAFETY: `c` is NUL-terminated and outlives the call, which only
        // reads it.
        unsafe { access(c.as_ptr(), 4) == 0 }
    }
    #[cfg(not(unix))]
    {
        std::fs::metadata(quoting::os_from_bytes(path)).is_ok()
    }
}

/// `display_summary()`: `-s`, the old `/proc/swaps` layout. -1 when the
/// table cannot be read.
fn display_summary(common: &mut Common, out: &mut Stdout) -> i32 {
    let Some(st) = common.get_swaps() else {
        return -1;
    };
    if st.nents() == 0 {
        return 0;
    }
    out.write(b"Filename\t\t\t\tType\t\tSize\t\tUsed\t\tPriority\n");
    for i in st.order(Direction::Forward) {
        let Some(fs) = st.ents.get(i) else {
            continue;
        };
        let src = fs.source.as_deref().unwrap_or_default();
        let ty = fs.swaptype.as_deref().unwrap_or_default();
        let pad = if src.len() < 40 {
            40usize.saturating_sub(src.len())
        } else {
            1
        };
        let mut line = src.to_vec();
        // `%*s` of " ": that many blanks.
        line.extend(std::iter::repeat_n(b' ', pad));
        line.extend_from_slice(ty);
        if ty.len() < 8 {
            line.push(b'\t');
        }
        line.extend_from_slice(
            format!(
                "\t{}{}\t{}{}\t{}\n",
                fs.size,
                if fs.size < 10_000_000 { "\t" } else { "" },
                fs.usedsize,
                if fs.usedsize < 10_000_000 { "\t" } else { "" },
                fs.priority
            )
            .as_bytes(),
        );
        out.write(&line);
    }
    0
}

/// `show_table(ctl)`: `--show`. -1 when the table cannot be read.
fn show_table(ctl: &Ctl, common: &mut Common, out: &mut Stdout) -> i32 {
    if common.get_swaps().is_none() {
        return -1;
    }
    let mut table = Table::new();
    table.enable_raw(ctl.raw);
    table.enable_noheadings(ctl.no_heading);
    for &col in &ctl.columns {
        if let Some(&(_, name, flags, _)) = INFOS.iter().find(|(c, ..)| *c == col) {
            table.new_column(name.as_bytes(), 0.20, flags);
        }
    }
    let entries: Vec<ulmount::fs::Fs> = common
        .swaps_read()
        .map(|st| st.ents.clone())
        .unwrap_or_default();
    for fs in &entries {
        add_scols_line(ctl, common, &mut table, fs);
    }
    let mut text = Vec::new();
    // `scols_print_table`'s status is not looked at upstream.
    let _ = table.print_into(&mut text);
    out.write(&text);
    0
}

/// `swap_reinitialize(dev)`: `mkswap` run on the area again, keeping its
/// label and UUID -- by the invoking user, when swapon is setuid.
fn swap_reinitialize(dev: &SwapDevice, short: &[u8]) -> i32 {
    warnx(
        short,
        &format!("{}: reinitializing the swap.", shown(&dev.path)),
    );
    let mut cmd = std::process::Command::new("mkswap");
    if let Some(l) = &dev.label {
        cmd.arg("-L").arg(quoting::os_from_bytes(l));
    }
    if let Some(u) = &dev.uuid {
        cmd.arg("-U").arg(quoting::os_from_bytes(u));
    }
    cmd.arg(quoting::os_from_bytes(&dev.path));
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        unsafe extern "C" {
            fn getuid() -> u32;
            fn geteuid() -> u32;
            fn getgid() -> u32;
            fn setuid(uid: u32) -> i32;
            fn setgid(gid: u32) -> i32;
        }
        // SAFETY: plain queries of the process's own IDs.
        let setuid_run = unsafe { geteuid() != getuid() };
        if setuid_run {
            // SAFETY: the closure runs in the forked child before exec and
            // calls only setgid/setuid, which are async-signal-safe.
            unsafe {
                cmd.pre_exec(|| {
                    // `drop_permissions()`: the group first, then the user.
                    if setgid(getgid()) < 0 || setuid(getuid()) < 0 {
                        return Err(std::io::Error::last_os_error());
                    }
                    Ok(())
                });
            }
        }
    }
    match cmd.status() {
        Ok(st) if st.success() => 0,
        Ok(_) => -1,
        Err(e) => {
            // Upstream's child reports its failed exec and exits; the
            // parent then only sees the failure.
            ulclosestream::warn(short, "failed to execute mkswap", &e);
            -1
        }
    }
}

/// `close_fd(fd)`: a close whose failure is reported.
#[cfg_attr(
    not(unix),
    allow(clippy::unnecessary_wraps, reason = "only the unix half can fail")
)]
fn close_checked(file: File) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::fd::IntoRawFd;
        unsafe extern "C" {
            fn close(fd: i32) -> i32;
        }
        let fd = file.into_raw_fd();
        // SAFETY: `fd` was just released by the File that owned it, so this
        // is its only close.
        if unsafe { close(fd) } != 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(())
    }
    #[cfg(not(unix))]
    {
        drop(file);
        Ok(())
    }
}

/// `swap_rewrite_signature(dev)`: the swap signature written over old
/// software suspend data.
fn swap_rewrite_signature(dev: &SwapDevice, short: &[u8]) -> i32 {
    let mut f = match std::fs::OpenOptions::new()
        .write(true)
        .open(quoting::os_from_bytes(&dev.path))
    {
        Ok(f) => f,
        Err(e) => {
            ulclosestream::warn(short, &format!("cannot open {}", shown(&dev.path)), &e);
            return -1;
        }
    };
    let mut rc = -1;
    let at = dev.pagesize.saturating_sub(SWAP_SIGNATURE_SZ) as u64;
    if let Err(e) = f.seek(SeekFrom::Start(at)) {
        ulclosestream::warn(short, &format!("{}: lseek failed", shown(&dev.path)), &e);
    } else {
        match f.write(SWAP_SIGNATURE) {
            Ok(n) if n == SWAP_SIGNATURE_SZ => rc = 0,
            Ok(_) => warnx(
                short,
                &format!("{}: write signature failed", shown(&dev.path)),
            ),
            Err(e) => ulclosestream::warn(
                short,
                &format!("{}: write signature failed", shown(&dev.path)),
                &e,
            ),
        }
    }
    if let Err(e) = close_checked(f) {
        ulclosestream::warn(short, &format!("write failed: {}", shown(&dev.path)), &e);
        rc = -1;
    }
    rc
}

/// `swap_detect_signature(buf, &sig)`.
fn swap_detect_signature(buf: &[u8]) -> Option<Sig> {
    if buf.starts_with(SWAP_SIGNATURE) {
        Some(Sig::SwapSpace)
    } else if buf.starts_with(b"S1SUSPEND")
        || buf.starts_with(b"S2SUSPEND")
        || buf.starts_with(b"ULSUSPEND")
        || buf.starts_with(b"\xed\xc3\x02\xe9\x98\x56\xe5\x0c")
        || buf.starts_with(b"LINHIB0001")
    {
        Some(Sig::SwSuspend)
    } else {
        None
    }
}

/// `swap_get_header(fd, &sig, &pagesize)`: the first 64 KiB, in one read,
/// and the page size whose last bytes hold a signature -- 4, 8, 16 or 64
/// KiB (32 KiB is not one swap supports).
fn swap_get_header(f: &mut File) -> Option<(Vec<u8>, Sig, usize)> {
    let mut buf = vec![0u8; MAX_PAGESIZE];
    let datasz = f.read(&mut buf).ok()?;
    let mut page = 0x1000usize;
    while page <= MAX_PAGESIZE {
        if page != 0x8000 {
            if datasz < page.saturating_sub(SWAP_SIGNATURE_SZ) {
                break;
            }
            if let Some(sig) =
                swap_detect_signature(buf.get(page.saturating_sub(SWAP_SIGNATURE_SZ)..)?)
            {
                return Some((buf, sig, page));
            }
        }
        page <<= 1;
    }
    None
}

/// The header's `u32` at `at`, in the machine's order.
fn u32_at(hdr: &[u8], at: usize) -> u32 {
    hdr.get(at..at.saturating_add(4))
        .and_then(|b| <[u8; 4]>::try_from(b).ok())
        .map_or(0, u32::from_ne_bytes)
}

/// `swap_get_size(dev, hdr)`: the area's size by its header -- `last_page`
/// read in either byte order.
fn swap_get_size(dev: &SwapDevice, hdr: &[u8]) -> u64 {
    let version = u32_at(hdr, 1024);
    let last_page = if version == 1 {
        u32_at(hdr, 1028)
    } else if version.swap_bytes() == 1 {
        u32_at(hdr, 1028).swap_bytes()
    } else {
        0
    };
    (u64::from(last_page) + 1).wrapping_mul(dev.pagesize as u64)
}

/// `swap_get_info(dev, hdr)`: the label and UUID from the header, for
/// `mkswap` to keep.
fn swap_get_info(dev: &mut SwapDevice, hdr: &[u8]) {
    let uuid = hdr.get(1036..1052).unwrap_or_default();
    let label = hdr.get(1052..1068).unwrap_or_default();
    if label.first().is_some_and(|&b| b != 0) {
        dev.label = Some(ulsysfs_c_str(label).to_vec());
    }
    if uuid.first().is_some_and(|&b| b != 0) {
        let u: Vec<String> = uuid.iter().map(|b| format!("{b:02x}")).collect();
        let s = format!(
            "{}-{}-{}-{}-{}",
            u.get(0..4).unwrap_or_default().concat(),
            u.get(4..6).unwrap_or_default().concat(),
            u.get(6..8).unwrap_or_default().concat(),
            u.get(8..10).unwrap_or_default().concat(),
            u.get(10..16).unwrap_or_default().concat()
        );
        dev.uuid = Some(s.into_bytes());
    }
}

/// `xstrdup` of a C string: up to its first NUL.
fn ulsysfs_c_str(b: &[u8]) -> &[u8] {
    b.iter()
        .position(|&c| c == 0)
        .map_or(b, |n| b.get(..n).unwrap_or_default())
}

/// What `fstat` says, the parts used here.
struct Stat {
    mode: u32,
    uid: u32,
    blocks: u64,
    size: u64,
}

fn fstat(f: &File) -> std::io::Result<Stat> {
    let m = f.metadata()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        Ok(Stat {
            mode: m.mode(),
            uid: m.uid(),
            blocks: m.blocks(),
            size: m.size(),
        })
    }
    #[cfg(not(unix))]
    {
        Ok(Stat {
            mode: if m.is_file() { 0o100_644 } else { 0 },
            uid: 0,
            blocks: m.len().div_ceil(512),
            size: m.len(),
        })
    }
}

/// `getpagesize()`.
fn getpagesize() -> i64 {
    #[cfg(unix)]
    {
        unsafe extern "C" {
            fn getpagesize() -> i32;
        }
        // SAFETY: no arguments, cannot fail.
        i64::from(unsafe { getpagesize() })
    }
    #[cfg(not(unix))]
    {
        4096
    }
}

/// `swapon_checks(ctl, dev)`: the area looked at before it is enabled.
fn swapon_checks(ctl: &Ctl, dev: &mut SwapDevice, short: &[u8]) -> Result<(), ()> {
    let path = dev.path.clone();
    let mut f = match File::open(quoting::os_from_bytes(&path)) {
        Ok(f) => f,
        Err(e) => {
            ulclosestream::warn(short, &format!("cannot open {}", shown(&path)), &e);
            return Err(());
        }
    };
    let st = match fstat(&f) {
        Ok(st) => st,
        Err(e) => {
            ulclosestream::warn(short, &format!("stat of {} failed", shown(&path)), &e);
            return Err(());
        }
    };
    const S_IFMT: u32 = 0o170_000;
    let is_blk = st.mode & S_IFMT == 0o060_000;
    let is_reg = st.mode & S_IFMT == 0o100_000;
    let perm_mask = if is_blk { 0o7007 } else { 0o7077 };
    if st.mode & perm_mask != 0 {
        warnx(
            short,
            &format!(
                "{}: insecure permissions {:04o}, {:04o} suggested.",
                shown(&path),
                st.mode & 0o7777,
                !perm_mask & 0o666
            ),
        );
    }
    if is_reg && st.uid != 0 {
        warnx(
            short,
            &format!(
                "{}: insecure file owner {}, 0 (root) suggested.",
                shown(&path),
                st.uid
            ),
        );
    }
    let mut devsize = 0u64;
    if is_reg {
        // Holes, by the blocks allocated.
        if st.blocks.wrapping_mul(512) < st.size {
            warnx(
                short,
                &format!("{}: skipping - it appears to have holes.", shown(&path)),
            );
            return Err(());
        }
        devsize = st.size;
    }
    if is_blk {
        match ulblkid::blkdev_get_size(&f) {
            Ok(s) => devsize = s,
            Err(_) => {
                warnx(short, &format!("{}: get size failed", shown(&path)));
                return Err(());
            }
        }
    }
    let Some((hdr, sig, pagesize)) = swap_get_header(&mut f) else {
        warnx(short, &format!("{}: read swap header failed", shown(&path)));
        return Err(());
    };
    dev.pagesize = pagesize;
    if ctl.verbose {
        warnx(
            short,
            &format!(
                "{}: found signature [pagesize={}, signature={}]",
                shown(&path),
                pagesize,
                match sig {
                    Sig::SwapSpace => "swap",
                    Sig::SwSuspend => "suspend",
                }
            ),
        );
    }
    match sig {
        Sig::SwapSpace => {
            let swapsize = swap_get_size(dev, &hdr);
            let syspg = getpagesize();
            if ctl.verbose {
                warnx(
                    short,
                    &format!(
                        "{}: pagesize={}, swapsize={}, devsize={}",
                        shown(&path),
                        pagesize,
                        swapsize,
                        devsize
                    ),
                );
            }
            if swapsize > devsize {
                if ctl.verbose {
                    warnx(
                        short,
                        &format!(
                            "{}: last_page 0x{:08x} is larger than actual size of swapspace",
                            shown(&path),
                            swapsize
                        ),
                    );
                }
            } else if syspg < 0 || syspg as u64 != pagesize as u64 {
                if ctl.fix_page_size {
                    swap_get_info(dev, &hdr);
                    warnx(
                        short,
                        &format!("{}: swap format pagesize does not match.", shown(&path)),
                    );
                    if swap_reinitialize(dev, short) < 0 {
                        return Err(());
                    }
                } else {
                    warnx(
                        short,
                        &format!(
                            "{}: swap format pagesize does not match. (Use --fixpgsz to reinitialize it.)",
                            shown(&path)
                        ),
                    );
                }
            }
        }
        Sig::SwSuspend => {
            // Old software suspend data would corrupt the next resume
            // attempt, so the swap signature goes back over it.
            warnx(
                short,
                &format!(
                    "{}: software suspend data detected. Rewriting the swap signature.",
                    shown(&path)
                ),
            );
            if swap_rewrite_signature(dev, short) < 0 {
                return Err(());
            }
        }
    }
    Ok(())
}

/// `do_swapon(ctl, prop, spec, canonic)`: 0, or -1 with the reason said.
fn do_swapon(
    ctl: &Ctl,
    common: &mut Common,
    prop: &SwapProp,
    spec: &[u8],
    canonic: bool,
    out: &mut Stdout,
) -> i32 {
    let path = if canonic {
        spec.to_vec()
    } else {
        match common.cache.resolve_spec(spec) {
            Some(p) => p,
            None => return common.cannot_find(spec),
        }
    };
    let short = common.short.clone();
    let mut dev = SwapDevice {
        path,
        ..SwapDevice::default()
    };
    if swapon_checks(ctl, &mut dev, &short).is_err() {
        return -1;
    }
    let mut flags = 0i32;
    let mut priority = prop.priority;
    if priority >= 0 {
        if priority > SWAP_FLAG_PRIO_MASK {
            priority = SWAP_FLAG_PRIO_MASK;
        }
        flags = SWAP_FLAG_PREFER | (priority & SWAP_FLAG_PRIO_MASK);
    }
    // Both discard policies together are what the kernel does for a plain
    // "discard".
    if prop.discard != 0 && prop.discard & !SWAP_FLAGS_DISCARD_VALID == 0 {
        if prop.discard & SWAP_FLAG_DISCARD_ONCE != 0 && prop.discard & SWAP_FLAG_DISCARD_PAGES != 0
        {
            flags |= SWAP_FLAG_DISCARD;
        } else {
            flags |= prop.discard;
        }
    }
    if ctl.verbose {
        let mut line = b"swapon ".to_vec();
        line.extend_from_slice(&dev.path);
        line.push(b'\n');
        out.write(&line);
    }
    let Ok(cpath) = std::ffi::CString::new(dev.path.clone()) else {
        // A C string ends at its NUL; no path read from a table has one.
        warn_errno(
            &short,
            &format!("{}: swapon failed", shown(&dev.path)),
            libcall::ENOENT,
        );
        return -1;
    };
    match libcall::swapon(&cpath, flags) {
        Ok(()) => 0,
        Err(e) => {
            warn_errno(&short, &format!("{}: swapon failed", shown(&dev.path)), e);
            -1
        }
    }
}

/// `parse_options(props, options)`: `nofail`, `discard[=once|pages]` and
/// `pri=N` from `-o` or an fstab entry. A discard policy is compared over
/// as many bytes as were given, as upstream compares it: `discard=on` asks
/// for `once`.
fn parse_options(props: &mut SwapProp, options: &[u8]) {
    let get = |name: &[u8]| ulmount::optstr::get_option(options, name).ok().flatten();
    if get(b"nofail").is_some() {
        props.no_fail = true;
    }
    if let Some(arg) = get(b"discard") {
        props.discard |= SWAP_FLAG_DISCARD;
        if let Some(arg) = arg {
            let starts = |word: &[u8]| {
                // `strncmp(arg, word, argsz) == 0`.
                let n = arg.len();
                let w = word.get(..n.min(word.len())).unwrap_or_default();
                arg.get(..n.min(word.len())) == Some(w) && (n <= word.len())
            };
            if starts(b"once") {
                props.discard |= SWAP_FLAG_DISCARD_ONCE;
            }
            if starts(b"pages") {
                props.discard |= SWAP_FLAG_DISCARD_PAGES;
            }
        }
    }
    if let Some(Some(arg)) = get(b"pri")
        && let Some(sc) = ulstrutils::scan_integer(arg, 10)
    {
        // `strtol` succeeded (no ERANGE) with digits read: the long cut to
        // an int.
        let max = u128::from(i64::MAX.unsigned_abs());
        if !(sc.saturated || sc.magnitude > max.saturating_add(u128::from(sc.negative))) {
            let m = i128::try_from(sc.magnitude).unwrap_or(0);
            let v = if sc.negative { m.saturating_neg() } else { m };
            #[allow(
                clippy::cast_possible_truncation,
                reason = "C casts the long to an int"
            )]
            {
                props.priority = v as i64 as i32;
            }
        }
    }
}

/// `swapon_all(ctl, filename)`: every swap area in fstab not `noauto`.
fn swapon_all(
    ctl: &Ctl,
    common: &mut Common,
    filename: Option<&[u8]>,
    out: &mut Stdout,
) -> Result<i32, u8> {
    if common.get_fstab(filename).is_none() {
        // Upstream names the default fstab here, whichever was read.
        warn_errno(
            &common.short,
            &format!(
                "failed to parse {}",
                shown(&ulmount::tab_parse::fstab_path())
            ),
            libcall::ENOENT,
        );
        return Err(1);
    }
    let entries: Vec<ulmount::fs::Fs> = {
        let Some(tb) = common.get_fstab(filename) else {
            return Err(1);
        };
        let mut itr = Iter::new(Direction::Forward);
        let mut v = Vec::new();
        while let Some(i) = tb.find_next_fs(&mut itr, match_swap) {
            if let Some(fs) = tb.ents.get(i) {
                v.push(fs.clone());
            }
        }
        v
    };
    let mut status = 0i32;
    for fs in &entries {
        let source = fs.source.clone().unwrap_or_default();
        if fs.get_option(b"noauto").ok().flatten().is_some() {
            if ctl.verbose {
                warnx(
                    &common.short,
                    &format!("{}: noauto option -- ignored", shown(&source)),
                );
            }
            continue;
        }
        let mut prop = ctl.props;
        if let Some(opts) = fs.options() {
            parse_options(&mut prop, opts);
        }
        let Some(device) = common.cache.resolve_spec(&source) else {
            if !prop.no_fail {
                status |= common.cannot_find(&source);
            }
            continue;
        };
        if common.is_active_swap(&device) {
            if ctl.verbose {
                warnx(
                    &common.short,
                    &format!("{}: already active -- ignored", shown(&device)),
                );
            }
            continue;
        }
        if prop.no_fail && !access_r(&device) {
            if ctl.verbose {
                warnx(
                    &common.short,
                    &format!("{}: inaccessible -- ignored", shown(&device)),
                );
            }
            continue;
        }
        status |= do_swapon(ctl, common, &prop, &device, true, out);
    }
    Ok(status)
}

/// `usage()`.
fn usage(short: &[u8]) -> Vec<u8> {
    let mut t = String::from("\nUsage:\n");
    t.push_str(&format!(" {} [options] [<spec>]\n", shown(short)));
    t.push('\n');
    t.push_str("Enable devices and files for paging and swapping.\n");
    t.push_str("\nOptions:\n");
    t.push_str(" -a, --all                enable all swaps from /etc/fstab\n");
    t.push_str(" -d, --discard[=<policy>] enable swap discards, if supported by device\n");
    t.push_str(" -e, --ifexists           silently skip devices that do not exist\n");
    t.push_str(" -f, --fixpgsz            reinitialize the swap space if necessary\n");
    t.push_str(" -o, --options <list>     comma-separated list of swap options\n");
    t.push_str(" -p, --priority <prio>    specify the priority of the swap device\n");
    t.push_str(" -s, --summary            display summary about used swap devices (DEPRECATED)\n");
    t.push_str(" -T, --fstab <path>       alternative file to /etc/fstab\n");
    t.push_str("     --show[=<columns>]   display summary in definable table\n");
    t.push_str("     --noheadings         don't print table heading (with --show)\n");
    t.push_str("     --raw                use the raw output format (with --show)\n");
    t.push_str("     --bytes              display swap size in bytes in --show output\n");
    t.push_str(" -v, --verbose            verbose mode\n");
    t.push('\n');
    t.push_str(&format!(
        "{:<26}{}\n{:<26}{}\n",
        " -h, --help", "display this help", " -V, --version", "display version"
    ));
    t.push_str(
        "\nThe <spec> parameter:\n -L <label>             synonym for LABEL=<label>\n -U <uuid>              synonym for UUID=<uuid>\n LABEL=<label>          specifies device by swap area label\n UUID=<uuid>            specifies device by swap area UUID\n PARTLABEL=<label>      specifies device by partition label\n PARTUUID=<uuid>        specifies device by partition UUID\n <device>               name of device to be used\n <file>                 name of file to be used\n",
    );
    t.push_str(
        "\nAvailable discard policy types (for --discard):\n once    : only single-time area discards are issued\n pages   : freed pages are discarded before they are reused\nIf no policy is selected, both discard types are enabled (default).\n",
    );
    t.push_str("\nAvailable output columns:\n");
    for &(_, name, _, help) in &INFOS {
        t.push_str(&format!(" {name:<5}  {help}\n"));
    }
    t.push_str("\nFor more details see swapon(8).\n");
    t.into_bytes()
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

/// `option_to_longopt(c, opts)`: the first long option with this `val`.
fn option_to_longopt(c: i32) -> Option<&'static str> {
    LONG_VALS
        .iter()
        .position(|&v| v == c)
        .and_then(|i| LONGS.get(i))
        .map(|&(name, _)| name)
}

/// `main`'s `int` status as the process's exit code: C's `& 0xff`.
fn exit_byte(status: i32) -> u8 {
    status.to_le_bytes().first().copied().unwrap_or(0)
}

stdfdguard::guard_std_fds!();

fn main() -> ExitCode {
    stdfdguard::restore();
    let argv: Vec<OsString> = std::env::args_os().collect();
    let short = short_name(
        argv.first()
            .map_or(OsStr::new("swapon"), OsString::as_os_str),
    );
    let mut out = Stdout::new(1);
    let status = run(&argv, &short, &mut out);
    ExitCode::from(out.close(status, &short))
}

/// `main()`.
#[allow(
    clippy::too_many_lines,
    reason = "upstream's main, kept in one piece so it can be read against it"
)]
fn run(argv: &[OsString], short: &[u8], out: &mut Stdout) -> u8 {
    let arg0 = argv
        .first()
        .map_or(OsStr::new("swapon"), OsString::as_os_str);
    let mut ctl = Ctl {
        props: SwapProp {
            priority: -1,
            ..SwapProp::default()
        },
        ..Ctl::default()
    };
    let mut common = Common::new(short.to_vec());
    let mut options: Option<Vec<u8>> = None;
    let mut fstab_filename: Option<Vec<u8>> = None;
    let mut operands: Vec<Vec<u8>> = Vec::new();
    let mut excl_st = [0i32; EXCL.len()];

    let own = argv.get(1..).unwrap_or_default();
    for item in SWAPON.parse(own, SHORTS, LONGS) {
        let opt = match item {
            Ok(opt) => opt,
            Err(e) => {
                // glibc names the program by argv[0] as given.
                stderr_write(
                    format!("{}: {}\n", shown(&quoting::os_bytes(arg0)), e.sentence).as_bytes(),
                );
                return errtryhelp(short, 1);
            }
        };
        let Some((c, value)) = option_code(&opt) else {
            if let Opt::Operand(o) = &opt {
                operands.push(quoting::os_bytes(o).into_owned());
            }
            continue;
        };
        if let Some(msg) =
            ulstrutils::err_exclusive_options(c, &EXCL, &mut excl_st, option_to_longopt, short)
        {
            stderr_write(msg.as_bytes());
            return 1;
        }
        let arg = value.as_deref().map(|v| quoting::os_bytes(v).into_owned());
        match c {
            SHOW_OPTION => {
                if let Some(list) = arg {
                    ctl.columns.clear();
                    if ulstrutils::string_to_idarray(&list, &mut ctl.columns, 14, |name, rest| {
                        column_name_to_id(name, rest, short)
                    })
                    .is_err()
                    {
                        return 1;
                    }
                }
                ctl.show = true;
                continue;
            }
            OPT_LIST_TYPES => {
                ctl.columns = INFOS.iter().map(|&(c, ..)| c).collect();
                continue;
            }
            NOHEADINGS_OPTION => {
                ctl.no_heading = true;
                continue;
            }
            RAW_OPTION => {
                ctl.raw = true;
                continue;
            }
            BYTES_OPTION => {
                ctl.bytes = true;
                continue;
            }
            _ => {}
        }
        match u8::try_from(c).unwrap_or(0) {
            b'a' => ctl.all = true,
            b'o' => options = arg,
            b'p' => {
                match strtos16_or_err(&arg.unwrap_or_default(), "failed to parse priority", short) {
                    Ok(p) => ctl.props.priority = i32::from(p),
                    Err(rc) => return rc,
                }
            }
            b'L' => common.labels.push(arg.unwrap_or_default()),
            b'U' => common.uuids.push(arg.unwrap_or_default()),
            b'T' => fstab_filename = arg,
            b'd' => {
                ctl.props.discard |= SWAP_FLAG_DISCARD;
                if let Some(a) = arg {
                    let policy = a.strip_prefix(b"=").unwrap_or(&a);
                    match policy {
                        b"once" => ctl.props.discard |= SWAP_FLAG_DISCARD_ONCE,
                        b"pages" => ctl.props.discard |= SWAP_FLAG_DISCARD_PAGES,
                        _ => {
                            warnx(
                                short,
                                &format!("unsupported discard policy: {}", shown(policy)),
                            );
                            return 1;
                        }
                    }
                }
            }
            b'e' => ctl.props.no_fail = true,
            b'f' => ctl.fix_page_size = true,
            b's' => return exit_byte(display_summary(&mut common, out)),
            b'v' => ctl.verbose = true,
            b'h' => {
                out.write(&usage(short));
                return 0;
            }
            b'V' => {
                out.write(format!("{} from util-linux 2.39.3\n", shown(short)).as_bytes());
                return 0;
            }
            _ => return errtryhelp(short, 1),
        }
    }

    if ctl.show
        || (!ctl.all && common.labels.is_empty() && common.uuids.is_empty() && operands.is_empty())
    {
        if ctl.columns.is_empty() {
            ctl.columns = vec![Col::Path, Col::Type, Col::Size, Col::Used, Col::Prio];
        }
        return exit_byte(show_table(&ctl, &mut common, out));
    }

    if ctl.props.no_fail && !ctl.all {
        warnx(short, "bad usage");
        return errtryhelp(short, 1);
    }

    let mut status = 0i32;
    if ctl.all {
        match swapon_all(&ctl, &mut common, fstab_filename.as_deref(), out) {
            Ok(s) => status |= s,
            Err(rc) => return rc,
        }
    }
    if let Some(o) = &options {
        parse_options(&mut ctl.props, o);
    }
    for label in common.labels.clone() {
        status |= match common.cache.resolve_tag(b"LABEL", &label) {
            Some(device) => do_swapon(&ctl, &mut common, &ctl.props, &device, true, out),
            None => common.cannot_find(&label),
        };
    }
    for uuid in common.uuids.clone() {
        status |= match common.cache.resolve_tag(b"UUID", &uuid) {
            Some(device) => do_swapon(&ctl, &mut common, &ctl.props, &device, true, out),
            None => common.cannot_find(&uuid),
        };
    }
    for spec in &operands {
        status |= do_swapon(&ctl, &mut common, &ctl.props, spec, false, out);
    }
    exit_byte(status)
}

#[cfg(test)]
mod tests;
