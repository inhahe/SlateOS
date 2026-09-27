//! blkid -- locate and print block device attributes.
//!
//! A port of util-linux 2.39.3's `misc-utils/blkid.c`, function by function
//! and with upstream's names, on top of `ulblkid` -- the port of libblkid:
//! its probing for `-p` and `-i`, its device cache (`/run/blkid/blkid.tab`)
//! for the default mode and `-l`, its tag evaluation for `-L` and `-U`.
//! Measured against `blkid from util-linux 2.39.3` by
//! `scripts/blkid-cli-diff.sh`.
//!
//! This replaces a hand-written program that recognised a handful of
//! filesystems itself, read argv as UTF-8, and doubled as `findfs` (which is
//! a program of its own, `userspace/findfs`).
//!
//! Upstream's quirks are kept where they show:
//!
//! * The default output format is "full", but `-o full` is a different
//!   value that means the same -- except that only the latter lets `-i`
//!   switch to the export format by itself.
//! * `-o list` widens its columns on each of the first two lines of a wide
//!   terminal, as upstream's static widths do.
//! * A device whose values `-s` filters out entirely still ends its (empty)
//!   line.
//! * A hint that does not parse is reported with whatever `errno` was left
//!   -- by loading the locale, if nothing since.
//!
//! # What is not upstream's
//!
//! * **A name in a diagnostic** has its unprintable bytes escaped
//!   (design-decisions §370).
//! * **`-o udev` with an encoded value too long for its buffer** prints the
//!   part that was encoded; upstream's buffer is then unterminated, and its
//!   output runs on into whatever follows it in memory.

use getoptlong::{Opt, Program, Takes};
use quoting::{escape_unprintable, escaped_in_quotes, os_bytes};
use std::ffi::{OsStr, OsString};
use std::process::ExitCode;
use std::rc::Rc;
use ulblkid::cache::{BlkCache, DEV_NORMAL};
use ulblkid::encode::{encode_string_into, safe_string};
use ulblkid::{
    FLTR_NOTIN, FLTR_ONLYIN, PARTS_ENTRY_DETAILS, PROBE_AMBIGUOUS, Probe, SUBLKS_FSINFO,
    SUBLKS_LABEL, SUBLKS_SECTYPE, SUBLKS_TYPE, SUBLKS_USAGE, SUBLKS_UUID, SUBLKS_VERSION,
    USAGE_CRYPTO, USAGE_FILESYSTEM, USAGE_OTHER, USAGE_RAID,
};
use ulclosestream::{Stdout, stderr_write, warn, warnx};
use ulsysfs::ismounted::{MF_BUSY, MF_MOUNTED, check_mount_point};

/// Getopt's errors are only sentences here; the referral follows them.
const BLKID: Program = Program::new("blkid", 1);

/// `OUTPUT_FULL`.
const OUTPUT_FULL: i32 = 1 << 0;
/// `OUTPUT_VALUE_ONLY`.
const OUTPUT_VALUE_ONLY: i32 = 1 << 1;
/// `OUTPUT_DEVICE_ONLY`.
const OUTPUT_DEVICE_ONLY: i32 = 1 << 2;
/// `OUTPUT_PRETTY_LIST`: deprecated.
const OUTPUT_PRETTY_LIST: i32 = 1 << 3;
/// `OUTPUT_UDEV_LIST`: deprecated.
const OUTPUT_UDEV_LIST: i32 = 1 << 4;
/// `OUTPUT_EXPORT_LIST`.
const OUTPUT_EXPORT_LIST: i32 = 1 << 5;

/// `BLKID_EXIT_NOTFOUND`: token or device not found.
const BLKID_EXIT_NOTFOUND: u8 = 2;
/// `BLKID_EXIT_OTHER`: bad usage or other error.
const BLKID_EXIT_OTHER: u8 = 4;
/// `BLKID_EXIT_AMBIVAL`: ambivalent low-level probing detected.
const BLKID_EXIT_AMBIVAL: u8 = 8;

/// `ERANGE`.
const ERANGE: i32 = 34;

/// Upstream's option string.
const SHORTS: &str = "c:DdgH:hilL:n:ko:O:ps:S:t:u:U:w:Vv";

/// Upstream's `longopts[]`, in its order, and each one's `val`.
const LONGS: &[(&str, Takes)] = &[
    ("cache-file", Takes::Required),
    ("no-encoding", Takes::Nothing),
    ("no-part-details", Takes::Nothing),
    ("garbage-collect", Takes::Nothing),
    ("output", Takes::Required),
    ("list-filesystems", Takes::Nothing),
    ("match-tag", Takes::Required),
    ("match-token", Takes::Required),
    ("list-one", Takes::Nothing),
    ("label", Takes::Required),
    ("uuid", Takes::Required),
    ("probe", Takes::Nothing),
    ("hint", Takes::Required),
    ("info", Takes::Nothing),
    ("size", Takes::Required),
    ("offset", Takes::Required),
    ("usages", Takes::Required),
    ("match-types", Takes::Required),
    ("version", Takes::Nothing),
    ("help", Takes::Nothing),
];
const LONG_VALS: [u8; 20] = *b"cdDgokstlLUpHiSOunVh";

/// `excl[]`: `-n` and `-u` exclude each other.
const EXCL: [&[i32]; 1] = [&[b'n' as i32, b'u' as i32]];

/// `struct blkid_control`, and the statics upstream keeps in functions.
struct Ctl {
    output: i32,
    offset: u64,
    size: u64,
    /// `show[]`: the tags `-s` asked for.
    show: Vec<Vec<u8>>,
    eval: bool,
    gc: bool,
    lookup: bool,
    lowprobe: bool,
    lowprobe_superblocks: bool,
    lowprobe_topology: bool,
    no_part_details: bool,
    raw_chars: bool,
    /// `print_tags`'s `static int first`.
    tags_first: bool,
    /// `lowprobe_device`'s `static int first`.
    lowprobe_first: bool,
    /// `pretty_print_line`'s statics: the column widths, and the terminal's
    /// width as it is used up.
    pretty: Pretty,
}

/// `pretty_print_line`'s statics.
struct Pretty {
    device_len: usize,
    fs_type_len: usize,
    label_len: usize,
    mtpt_len: usize,
    term_width: Option<i64>,
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

/// `errtryhelp(status)`.
fn errtryhelp(short: &[u8], status: u8) -> u8 {
    stderr_write(format!("Try '{} --help' for more information.\n", shown(short)).as_bytes());
    status
}

/// `warn(msg)` with an `errno` that may be 0 -- glibc's `Success`.
fn warn_errno(short: &[u8], msg: &str, errno: i32) {
    if errno == 0 {
        warnx(short, &format!("{msg}: Success"));
    } else {
        warn(short, msg, &std::io::Error::from_raw_os_error(errno));
    }
}

/// `get_terminal_width(default)`.
fn get_terminal_width(default: i64) -> i64 {
    match smartcols::tty::terminal_dimension().0 {
        Some(w) if w > 0 => i64::try_from(w).unwrap_or(i64::MAX),
        _ => default,
    }
}

/// `usage()`.
fn usage(short: &[u8]) -> Vec<u8> {
    let s = shown(short);
    let mut t = String::from("\nUsage:\n");
    t.push_str(&format!(" {s} --label <label> | --uuid <uuid>\n\n"));
    t.push_str(&format!(
        " {s} [--cache-file <file>] [-ghlLv] [--output <format>] [--match-tag <tag>] \n       [--match-token <token>] [<dev> ...]\n\n"
    ));
    t.push_str(&format!(
        " {s} -p [--match-tag <tag>] [--offset <offset>] [--size <size>] \n       [--output <format>] <dev> ...\n\n"
    ));
    t.push_str(&format!(
        " {s} -i [--match-tag <tag>] [--output <format>] <dev> ...\n"
    ));
    t.push_str("\nOptions:\n");
    t.push_str(" -c, --cache-file <file>    read from <file> instead of reading from the default\n                              cache file (-c /dev/null means no cache)\n");
    t.push_str(" -d, --no-encoding          don't encode non-printing characters\n");
    t.push_str(" -g, --garbage-collect      garbage collect the blkid cache\n");
    t.push_str(" -o, --output <format>      output format; can be one of:\n                              value, device, export or full; (default: full)\n");
    t.push_str(" -k, --list-filesystems     list all known filesystems/RAIDs and exit\n");
    t.push_str(" -s, --match-tag <tag>      show specified tag(s) (default show all tags)\n");
    t.push_str(" -t, --match-token <token>  find device with a specific token (NAME=value pair)\n");
    t.push_str(
        " -l, --list-one             look up only first device with token specified by -t\n",
    );
    t.push_str(" -L, --label <label>        convert LABEL to device name\n");
    t.push_str(" -U, --uuid <uuid>          convert UUID to device name\n");
    t.push('\n');
    t.push_str("Low-level probing options:\n");
    t.push_str(" -p, --probe                low-level superblocks probing (bypass cache)\n");
    t.push_str(" -i, --info                 gather information about I/O limits\n");
    t.push_str(" -H, --hint <value>         set hint for probing function\n");
    t.push_str(" -S, --size <size>          overwrite device size\n");
    t.push_str(" -O, --offset <offset>      probe at the given offset\n");
    t.push_str(" -u, --usages <list>        filter by \"usage\" (e.g. -u filesystem,raid)\n");
    t.push_str(" -n, --match-types <list>   filter by filesystem type (e.g. -n vfat,ext3)\n");
    t.push_str(" -D, --no-part-details      don't print info from partition table\n");
    t.push('\n');
    t.push_str(&format!(
        "{:<28}{}\n{:<28}{}\n",
        " -h, --help", "display this help", " -V, --version", "display version"
    ));
    t.push_str("\nArguments:\n");
    t.push_str(" <size> and <offset> arguments may be followed by the suffixes for\n   GiB, TiB, PiB, EiB, ZiB, and YiB (the \"iB\" is optional)\n");
    t.push('\n');
    t.push_str(" <dev> specify device(s) to probe (default: all devices)\n");
    t.push_str("\nFor more details see blkid(8).\n");
    t.into_bytes()
}

/// `safe_print(ctl, cp, len, esc)`: non-printing bytes in `^` and `M-`
/// notation, and each byte of `esc` after a backslash -- unless `-d`.
fn safe_print(ctl: &Ctl, out: &mut Stdout, cp: &[u8], esc: &[u8]) {
    let mut buf = Vec::with_capacity(cp.len());
    for &c in cp {
        let mut ch = c;
        if !ctl.raw_chars {
            if ch >= 128 {
                buf.extend_from_slice(b"M-");
                ch = ch.wrapping_sub(128);
            }
            if ch < 32 || ch == 0x7f {
                buf.push(b'^');
                // ^@, ^A, ^B...; ^? for DEL.
                ch ^= 0x40;
            } else if esc.contains(&ch) {
                buf.push(b'\\');
            }
        }
        buf.push(ch);
    }
    out.write(&buf);
}

/// `pretty_print_word(str, max_len, left_len, overflow_nl)`: the word, then
/// spaces to its column's end; how far it ran past the column.
fn pretty_print_word(
    out: &mut Stdout,
    s: &[u8],
    max_len: usize,
    left_len: usize,
    overflow_nl: bool,
) -> usize {
    let mut len = s.len().saturating_add(left_len);
    let mut ret = 0usize;
    out.write(s);
    if overflow_nl && len > max_len {
        out.write(b"\n");
        len = 0;
    } else if len > max_len {
        ret = len.saturating_sub(max_len);
    }
    // `do { fputc(' ') } while (len++ < max_len);`: at least one space.
    let spaces = max_len.saturating_sub(len).saturating_add(1);
    out.write(&vec![b' '; spaces]);
    ret
}

/// `pretty_print_line(device, fs_type, label, mtpt, uuid)`. On a terminal
/// wider than 80 columns the widths grow -- on each line while the
/// terminal's width, used up 80 columns at a time, still exceeds 80.
fn pretty_print_line(
    ctl: &mut Ctl,
    out: &mut Stdout,
    device: &[u8],
    fs_type: &[u8],
    label: &[u8],
    mtpt: &[u8],
    uuid: &[u8],
) {
    let p = &mut ctl.pretty;
    let term_width = *p.term_width.get_or_insert_with(|| get_terminal_width(80));
    if term_width > 80 {
        let mut tw = term_width.saturating_sub(80);
        let w = (tw / 10).min(8);
        tw = tw.saturating_sub(w.saturating_mul(2));
        let w_u = usize::try_from(w).unwrap_or(0);
        p.label_len = p.label_len.saturating_add(w_u);
        p.fs_type_len = p.fs_type_len.saturating_add(w_u);
        let half = usize::try_from(tw / 2).unwrap_or(0);
        p.device_len = p.device_len.saturating_add(half);
        p.mtpt_len = p.mtpt_len.saturating_add(half);
        p.term_width = Some(tw);
    }
    let (dl, fl, ll, ml) = (p.device_len, p.fs_type_len, p.label_len, p.mtpt_len);
    let len = pretty_print_word(out, device, dl, 0, true);
    let len = pretty_print_word(out, fs_type, fl, len, false);
    let len = pretty_print_word(out, label, ll, len, false);
    pretty_print_word(out, mtpt, ml, len, false);
    out.write(uuid);
    out.write(b"\n");
}

/// `pretty_print_dev(dev)`: one line of `-o list` -- or, for no device, its
/// heading and a rule the terminal's width.
fn pretty_print_dev(ctl: &mut Ctl, out: &mut Stdout, cache: Option<&BlkCache>, dev: Option<usize>) {
    let (Some(cache), Some(dev)) = (cache, dev) else {
        pretty_print_line(
            ctl,
            out,
            b"device",
            b"fs_type",
            b"label",
            b"mount point",
            b"UUID",
        );
        let n = get_terminal_width(0).saturating_sub(1);
        if n > 0 {
            out.write(&vec![b'-'; usize::try_from(n).unwrap_or(0)]);
        }
        out.write(b"\n");
        return;
    };
    let Some(devname) = cache.devname(dev).map(<[u8]>::to_vec) else {
        return;
    };
    // `access(devname, F_OK)`.
    if std::fs::metadata(quoting::os_from_bytes(&devname)).is_err() {
        return;
    }
    let (mut uuid, mut fs_type, mut label) = (Vec::new(), Vec::new(), Vec::new());
    for (ty, value) in cache.tags(dev) {
        match ty.as_slice() {
            b"UUID" => uuid = value,
            b"TYPE" => fs_type = value,
            b"LABEL" => label = value,
            _ => {}
        }
    }
    // The mount point, in a buffer of 80.
    let mut mtpt = Vec::new();
    if let Ok(mc) = check_mount_point(&devname, 80) {
        mtpt = mc.mtpt;
        let msg: Option<&[u8]> = if mc.flags & MF_MOUNTED != 0 {
            mtpt.is_empty().then_some(b"(mounted, mtpt unknown)")
        } else if mc.flags & MF_BUSY != 0 {
            Some(b"(in use)")
        } else {
            Some(b"(not mounted)")
        };
        if let Some(m) = msg {
            mtpt = m.to_vec();
        }
    }
    pretty_print_line(ctl, out, &devname, &fs_type, &label, &mtpt, &uuid);
}

/// `blkid_encode_string(value, enc, size)` as upstream then prints `enc`.
fn encoded(value: &[u8], size: usize) -> Vec<u8> {
    match encode_string_into(value, size) {
        Ok(e) | Err(e) => e,
    }
}

/// `print_udev_format(name, value)`: udev's `ID_FS_*` names.
fn print_udev_format(out: &mut Stdout, name: &[u8], value: &[u8]) {
    let mut line = Vec::new();
    let is = |n: &[u8]| name == n;
    if is(b"TYPE")
        || is(b"VERSION")
        || is(b"SYSTEM_ID")
        || is(b"PUBLISHER_ID")
        || is(b"APPLICATION_ID")
        || is(b"BOOT_SYSTEM_ID")
        || is(b"VOLUME_ID")
        || is(b"LOGICAL_VOLUME_ID")
        || is(b"VOLUME_SET_ID")
        || is(b"DATA_PREPARER_ID")
    {
        line.extend_from_slice(b"ID_FS_");
        line.extend_from_slice(name);
        line.push(b'=');
        line.extend_from_slice(&encoded(value, 265));
        line.push(b'\n');
    } else if is(b"UUID") || name.starts_with(b"LABEL") || is(b"UUID_SUB") {
        line.extend_from_slice(b"ID_FS_");
        line.extend_from_slice(name);
        line.push(b'=');
        line.extend_from_slice(&safe_string(value, 256));
        line.extend_from_slice(b"\nID_FS_");
        line.extend_from_slice(name);
        line.extend_from_slice(b"_ENC=");
        line.extend_from_slice(&encoded(value, 265));
        line.push(b'\n');
    } else if is(b"PTUUID") {
        line.extend_from_slice(b"ID_PART_TABLE_UUID=");
        line.extend_from_slice(value);
        line.push(b'\n');
    } else if is(b"PTTYPE") {
        line.extend_from_slice(b"ID_PART_TABLE_TYPE=");
        line.extend_from_slice(value);
        line.push(b'\n');
    } else if is(b"PART_ENTRY_NAME") || is(b"PART_ENTRY_TYPE") {
        line.extend_from_slice(b"ID_");
        line.extend_from_slice(name);
        line.push(b'=');
        line.extend_from_slice(&encoded(value, 265));
        line.push(b'\n');
    } else if name.starts_with(b"PART_ENTRY_") {
        line.extend_from_slice(b"ID_");
        line.extend_from_slice(name);
        line.push(b'=');
        line.extend_from_slice(value);
        line.push(b'\n');
    } else if name.len() >= 15
        && (name.ends_with(b"_SECTOR_SIZE")
            || name.ends_with(b"_IO_SIZE")
            || is(b"ALIGNMENT_OFFSET"))
    {
        line.extend_from_slice(b"ID_IOLIMIT_");
        line.extend_from_slice(name);
        line.push(b'=');
        line.extend_from_slice(value);
        line.push(b'\n');
    } else {
        line.extend_from_slice(b"ID_FS_");
        line.extend_from_slice(name);
        line.push(b'=');
        line.extend_from_slice(value);
        line.push(b'\n');
    }
    out.write(&line);
}

/// `has_item(ctl, item)`: `-s` named it.
fn has_item(ctl: &Ctl, item: &[u8]) -> bool {
    ctl.show.iter().any(|s| s.as_slice() == item)
}

/// `print_value(ctl, num, devname, value, name, valsz)`.
fn print_value(
    ctl: &Ctl,
    out: &mut Stdout,
    num: usize,
    devname: Option<&[u8]>,
    value: &[u8],
    name: &[u8],
) {
    if ctl.output & OUTPUT_VALUE_ONLY != 0 {
        out.write(value);
        out.write(b"\n");
    } else if ctl.output & OUTPUT_UDEV_LIST != 0 {
        print_udev_format(out, name, value);
    } else if ctl.output & OUTPUT_EXPORT_LIST != 0 {
        if num == 1
            && let Some(d) = devname
        {
            out.write(b"DEVNAME=");
            out.write(d);
            out.write(b"\n");
        }
        out.write(name);
        out.write(b"=");
        safe_print(ctl, out, value, b" \\\"'$`<>");
        out.write(b"\n");
    } else {
        if num == 1
            && let Some(d) = devname
        {
            out.write(d);
            out.write(b":");
        }
        out.write(b" ");
        out.write(name);
        out.write(b"=\"");
        safe_print(ctl, out, value, b"\"\\");
        out.write(b"\"");
    }
}

/// `print_tags(ctl, dev)`: a cached device's tags.
fn print_tags(ctl: &mut Ctl, out: &mut Stdout, cache: &BlkCache, dev: usize) {
    if ctl.output & OUTPUT_PRETTY_LIST != 0 {
        pretty_print_dev(ctl, out, Some(cache), Some(dev));
        return;
    }
    let Some(devname) = cache.devname(dev).map(<[u8]>::to_vec) else {
        return;
    };
    if ctl.output & OUTPUT_DEVICE_ONLY != 0 {
        out.write(&devname);
        out.write(b"\n");
        return;
    }
    let mut num = 1usize;
    for (ty, value) in cache.tags(dev) {
        if !ctl.show.is_empty() && !has_item(ctl, &ty) {
            continue;
        }
        if num == 1 && !ctl.tags_first && ctl.output & (OUTPUT_UDEV_LIST | OUTPUT_EXPORT_LIST) != 0
        {
            // An empty line between devices.
            out.write(b"\n");
        }
        // `strlen(value)`.
        let v = ulblkid::c_str(&value);
        print_value(ctl, out, num, Some(&devname), v, &ty);
        num = num.saturating_add(1);
    }
    if num > 1 {
        if ctl.output & (OUTPUT_VALUE_ONLY | OUTPUT_UDEV_LIST | OUTPUT_EXPORT_LIST) == 0 {
            out.write(b"\n");
        }
        ctl.tags_first = false;
    }
}

/// `print_udev_ambivalent(pr)`: every signature on the device, as
/// `ID_FS_AMBIVALENT=usage:type[:version] ...`, when there is more than one.
fn print_udev_ambivalent(out: &mut Stdout, pr: &mut Probe) {
    let mut val: Vec<u8> = Vec::new();
    let mut count = 0usize;
    while pr.do_probe() == 0 {
        let get = |pr: &Probe, n: &str| pr.lookup_value(n).map(|v| v.as_c_str().to_vec());
        let (Some(usage_txt), Some(ty)) = (get(pr, "USAGE"), get(pr, "TYPE")) else {
            continue;
        };
        let version = get(pr, "VERSION");
        val.extend_from_slice(&encoded(&usage_txt, 256));
        val.push(b':');
        val.extend_from_slice(&encoded(&ty, 256));
        match &version {
            Some(v) => {
                val.push(b':');
                val.extend_from_slice(&encoded(v, 256));
                val.push(b' ');
            }
            None => val.push(b' '),
        }
        count = count.saturating_add(1);
    }
    if count > 1 {
        // The trailing space off.
        val.pop();
        out.write(b"ID_FS_AMBIVALENT=");
        out.write(&val);
        out.write(b"\n");
    }
}

/// `lowprobe_superblocks(pr, ctl)`: a small whole disk is looked at for a
/// partition table first, and not for filesystems if it has one.
fn lowprobe_superblocks(pr: &mut Probe, ctl: &Ctl) -> i32 {
    let Some(meta) = pr.file().and_then(|f| f.metadata().ok()) else {
        return -1;
    };
    pr.enable_partitions(true);
    let is_chr = {
        #[cfg(unix)]
        {
            use std::os::unix::fs::FileTypeExt;
            meta.file_type().is_char_device()
        }
        #[cfg(not(unix))]
        {
            let _ = &meta;
            false
        }
    };
    if !is_chr && pr.size() <= 1024 * 1440 && pr.is_wholedisk() {
        pr.enable_superblocks(false);
        let rc = pr.do_fullprobe();
        if rc < 0 {
            return rc;
        }
        if pr.lookup_value("PTTYPE").is_some() {
            // A partition table.
            return 0;
        }
    }
    if !ctl.no_part_details {
        pr.set_partitions_flags(PARTS_ENTRY_DETAILS);
    }
    pr.enable_superblocks(true);
    pr.do_safeprobe()
}

/// `lowprobe_topology(pr)`: topology alone.
fn lowprobe_topology(pr: &mut Probe) -> i32 {
    pr.enable_topology(true);
    pr.enable_superblocks(false);
    pr.enable_partitions(false);
    pr.do_fullprobe()
}

/// `lowprobe_device(pr, devname, ctl)`: `-p` and `-i` on one device.
fn lowprobe_device(
    pr: &mut Probe,
    devname: &[u8],
    ctl: &mut Ctl,
    out: &mut Stdout,
    short: &[u8],
) -> u8 {
    let file = match ulblkid::open_nonblock(devname) {
        Ok(f) => f,
        Err(e) => {
            warn(
                short,
                &format!("error: {}", shown(devname)),
                &std::io::Error::from_raw_os_error(e),
            );
            return BLKID_EXIT_NOTFOUND;
        }
    };
    let mut rc = 0;
    let mut nvals = 0usize;
    pr.errno = 0;
    // The offset and size as the `blkid_loff_t`s they are passed as.
    let (off, size) = (ctl.offset.cast_signed(), ctl.size.cast_signed());
    if pr.set_device(Some(Rc::new(file)), off, size) != 0 {
        if pr.errno != 0 {
            warn(
                short,
                &format!("error: {}", shown(devname)),
                &std::io::Error::from_raw_os_error(pr.errno),
            );
        }
    } else {
        'probe: {
            if ctl.lowprobe_topology {
                rc = lowprobe_topology(pr);
            }
            if rc >= 0 && ctl.lowprobe_superblocks {
                rc = lowprobe_superblocks(pr, ctl);
            }
            if rc < 0 {
                break 'probe;
            }
            if rc == 0 {
                nvals = pr.numof_values();
            }
            if nvals != 0
                && !ctl.lowprobe_first
                && ctl.output & (OUTPUT_UDEV_LIST | OUTPUT_EXPORT_LIST) != 0
            {
                // An empty line between devices.
                out.write(b"\n");
            }
            if nvals != 0 && ctl.output & OUTPUT_DEVICE_ONLY != 0 {
                out.write(devname);
                out.write(b"\n");
                break 'probe;
            }
            let values: Vec<(Vec<u8>, Vec<u8>)> = pr
                .values()
                .iter()
                .map(|v| (v.name.as_bytes().to_vec(), v.as_c_str().to_vec()))
                .collect();
            let mut num = 1usize;
            for (name, data) in values {
                if !ctl.show.is_empty() && !has_item(ctl, &name) {
                    continue;
                }
                print_value(ctl, out, num, Some(devname), &data, &name);
                num = num.saturating_add(1);
            }
            ctl.lowprobe_first = false;
            if nvals >= 1
                && ctl.output & (OUTPUT_VALUE_ONLY | OUTPUT_UDEV_LIST | OUTPUT_EXPORT_LIST) == 0
            {
                out.write(b"\n");
            }
        }
    }
    if rc == PROBE_AMBIGUOUS {
        if ctl.output & OUTPUT_UDEV_LIST != 0 {
            print_udev_ambivalent(out, pr);
        } else {
            warnx(
                short,
                &format!(
                    "{}: ambivalent result (probably more filesystems on the device, use wipefs(8) to see more details)",
                    shown(devname)
                ),
            );
        }
    }
    if rc == PROBE_AMBIGUOUS {
        return BLKID_EXIT_AMBIVAL;
    }
    if nvals == 0 {
        return BLKID_EXIT_NOTFOUND;
    }
    0
}

/// `list_to_usage(list, &flag)`: a comma-separated list of usages, `no`
/// before it for all but these. An unknown word ends the program.
fn list_to_usage(list: &[u8], flag: &mut u32, short: &[u8]) -> Result<u32, u8> {
    let mut mask = 0u32;
    let mut p: Option<&[u8]> = Some(list);
    if list.starts_with(b"no") {
        *flag = FLTR_NOTIN;
        p = list.get(2..);
    }
    let mut word: Option<&[u8]> = None;
    let fail = |flag: &mut u32, word: Option<&[u8]>| {
        *flag = 0;
        warnx(
            short,
            &format!(
                "unknown keyword in -u <list> argument: {}",
                escaped_in_quotes(word.unwrap_or(list))
            ),
        );
        BLKID_EXIT_OTHER
    };
    if p.is_none_or(<[u8]>::is_empty) {
        return Err(fail(flag, word));
    }
    while let Some(rest) = p {
        word = Some(rest);
        p = rest
            .iter()
            .position(|&b| b == b',')
            .and_then(|i| rest.get(i.saturating_add(1)..));
        // `strncmp(word, "filesystem", 10)`: a prefix of the rest.
        if rest.starts_with(b"filesystem") {
            mask |= USAGE_FILESYSTEM;
        } else if rest.starts_with(b"raid") {
            mask |= USAGE_RAID;
        } else if rest.starts_with(b"crypto") {
            mask |= USAGE_CRYPTO;
        } else if rest.starts_with(b"other") {
            mask |= USAGE_OTHER;
        } else {
            return Err(fail(flag, word));
        }
    }
    Ok(mask)
}

/// `list_to_types(list, &flag)`: a comma-separated list of types, `no`
/// before it for all but these.
fn list_to_types(list: &[u8], flag: &mut u32, short: &[u8]) -> Result<Vec<Vec<u8>>, u8> {
    let mut p = list;
    if list.starts_with(b"no") {
        *flag = FLTR_NOTIN;
        p = list.get(2..).unwrap_or_default();
    }
    if p.is_empty() {
        warnx(short, "error: -u <list> argument is empty");
        *flag = 0;
        return Err(BLKID_EXIT_OTHER);
    }
    // Upstream starts again from the list, skipping "no" only if the flag
    // says NOTIN -- which a second `-n` without "no" leaves in place.
    let p = if *flag & FLTR_NOTIN != 0 {
        list.get(2..).unwrap_or_default()
    } else {
        list
    };
    Ok(p.split(|&b| b == b',').map(<[u8]>::to_vec).collect())
}

/// The option each parsed item stands for, as upstream's switch sees it.
fn option_code(opt: &Opt<'_>) -> Option<(i32, Option<OsString>)> {
    match opt {
        Opt::Short(c, value) => Some((i32::from(*c), value.clone())),
        Opt::Long(name, value) => {
            let i = LONGS.iter().position(|&(n, _)| n == *name)?;
            Some((i32::from(*LONG_VALS.get(i)?), value.clone()))
        }
        Opt::Operand(_) => None,
    }
}

/// `option_to_longopt(c, opts)`: the first long option with this `val`.
fn option_to_longopt(c: i32) -> Option<&'static str> {
    LONG_VALS
        .iter()
        .position(|&v| i32::from(v) == c)
        .and_then(|i| LONGS.get(i))
        .map(|&(name, _)| name)
}

/// The `errno` a failed `blkid_probe_set_hint(pr, hint, 0)` leaves for
/// `warn` to print: what was there before (`errno_left`) when the hint does
/// not parse as `NAME=value`, else `strtoumax`'s -- `ERANGE`, or the 0 it
/// was cleared to.
fn hint_errno(hint: &[u8], errno_left: i32) -> i32 {
    let Some(eq) = hint.iter().position(|&b| b == b'=') else {
        return errno_left;
    };
    let value = hint.get(eq.saturating_add(1)..).unwrap_or_default();
    let quoted = value.first().is_some_and(|&q| q == b'"' || q == b'\'');
    let parsed = if quoted {
        let q = value.first().copied().unwrap_or(0);
        let inner = value.get(1..).unwrap_or_default();
        inner
            .iter()
            .rposition(|&b| b == q)
            .and_then(|i| inner.get(..i))
    } else {
        Some(ulblkid::c_str(value))
    };
    match parsed {
        Some(v) if !v.is_empty() => match ulstrutils::scan_integer(v, 10) {
            Some(sc) if sc.saturated || sc.magnitude > u128::from(u64::MAX) => ERANGE,
            _ => 0,
        },
        _ => errno_left,
    }
}

/// `S_ISBLK`, `S_ISREG`, or a UBI character device: what upstream probes.
fn is_probeable(dev: &[u8]) -> bool {
    let Ok(meta) = std::fs::metadata(quoting::os_from_bytes(dev)) else {
        return false;
    };
    if meta.is_file() {
        return true;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{FileTypeExt, MetadataExt};
        let ft = meta.file_type();
        if ft.is_block_device() {
            return true;
        }
        if ft.is_char_device() {
            return ulsysfs::chrdev_devno_to_devname(meta.rdev(), ulsysfs::PATH_MAX)
                .is_some_and(|n| n.starts_with(b"ubi"));
        }
    }
    false
}

stdfdguard::guard_std_fds!();

fn main() -> ExitCode {
    stdfdguard::restore();
    let argv: Vec<OsString> = std::env::args_os().collect();
    let short = short_name(
        argv.first()
            .map_or(OsStr::new("blkid"), OsString::as_os_str),
    );
    // `close_stdout`, with CLOSE_EXIT_CODE = BLKID_EXIT_OTHER.
    let mut out = Stdout::new(BLKID_EXIT_OTHER);
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
        .map_or(OsStr::new("blkid"), OsString::as_os_str);
    // `setlocale` may leave an `errno` that a later message prints.
    let errno_left = smartcols::tty::setlocale_errno();
    let mut ctl = Ctl {
        output: OUTPUT_FULL,
        offset: 0,
        size: 0,
        show: Vec::new(),
        eval: false,
        gc: false,
        lookup: false,
        lowprobe: false,
        lowprobe_superblocks: false,
        lowprobe_topology: false,
        no_part_details: false,
        raw_chars: false,
        tags_first: true,
        lowprobe_first: true,
        pretty: Pretty {
            device_len: 10,
            fs_type_len: 7,
            label_len: 8,
            mtpt_len: 14,
            term_width: None,
        },
    };
    let mut devices: Vec<Vec<u8>> = Vec::new();
    let mut operands: Vec<Vec<u8>> = Vec::new();
    let mut search: Option<(Vec<u8>, Vec<u8>)> = None;
    let mut read: Option<Vec<u8>> = None;
    let mut hint: Option<Vec<u8>> = None;
    let mut fltr_usage = 0u32;
    let mut fltr_type: Option<Vec<Vec<u8>>> = None;
    let mut fltr_flag = FLTR_ONLYIN;
    let mut excl_st = [0i32; 1];

    let own = argv.get(1..).unwrap_or_default();
    for item in BLKID.parse(own, SHORTS, LONGS) {
        let opt = match item {
            Ok(opt) => opt,
            Err(e) => {
                // glibc names the program by argv[0] as given.
                stderr_write(format!("{}: {}\n", shown(&os_bytes(arg0)), e.sentence).as_bytes());
                return errtryhelp(short, 1);
            }
        };
        let Some((c, value)) = option_code(&opt) else {
            if let Opt::Operand(o) = &opt {
                operands.push(os_bytes(o).into_owned());
            }
            continue;
        };
        if let Some(msg) =
            ulstrutils::err_exclusive_options(c, &EXCL, &mut excl_st, option_to_longopt, short)
        {
            stderr_write(msg.as_bytes());
            return BLKID_EXIT_OTHER;
        }
        let arg = value.as_deref().map(|v| os_bytes(v).into_owned());
        let arg_os = value.clone().unwrap_or_default();
        match u8::try_from(c).unwrap_or(0) {
            b'c' => read = arg,
            b'd' => ctl.raw_chars = true,
            b'D' => ctl.no_part_details = true,
            b'H' => hint = arg,
            b'L' => {
                ctl.eval = true;
                search = Some((b"LABEL".to_vec(), arg.unwrap_or_default()));
            }
            b'n' => match list_to_types(&arg.unwrap_or_default(), &mut fltr_flag, short) {
                Ok(t) => fltr_type = Some(t),
                Err(rc) => return rc,
            },
            b'u' => match list_to_usage(&arg.unwrap_or_default(), &mut fltr_flag, short) {
                Ok(m) => fltr_usage = m,
                Err(rc) => return rc,
            },
            b'U' => {
                ctl.eval = true;
                search = Some((b"UUID".to_vec(), arg.unwrap_or_default()));
            }
            b'i' => ctl.lowprobe_topology = true,
            b'l' => ctl.lookup = true,
            b'g' => ctl.gc = true,
            b'k' => {
                let mut idx = 0usize;
                while let Some((name, _)) = ulblkid::superblocks::get_name(idx) {
                    out.write(name.as_bytes());
                    out.write(b"\n");
                    idx = idx.saturating_add(1);
                }
                return 0;
            }
            b'o' => {
                ctl.output = match arg.as_deref() {
                    Some(b"value") => OUTPUT_VALUE_ONLY,
                    Some(b"device") => OUTPUT_DEVICE_ONLY,
                    Some(b"list") => OUTPUT_PRETTY_LIST,
                    Some(b"udev") => OUTPUT_UDEV_LIST,
                    Some(b"export") => OUTPUT_EXPORT_LIST,
                    Some(b"full") => 0,
                    _ => {
                        warnx(
                            short,
                            &format!(
                                "unsupported output format {}",
                                shown(&arg.unwrap_or_default())
                            ),
                        );
                        return BLKID_EXIT_OTHER;
                    }
                };
            }
            b'O' => match ulstrutils::parse_size(&arg.unwrap_or_default()) {
                Ok(v) => ctl.offset = v,
                Err(e) => {
                    warnx(
                        short,
                        &ulstrutils::size_error_message("invalid offset argument", &arg_os, e),
                    );
                    return BLKID_EXIT_OTHER;
                }
            },
            b'p' => ctl.lowprobe_superblocks = true,
            b's' => {
                // `numtag + 1 >= 128`.
                if ctl.show.len().saturating_add(1) >= 128 {
                    warnx(short, "Too many tags specified");
                    return errtryhelp(short, BLKID_EXIT_OTHER);
                }
                ctl.show.push(arg.unwrap_or_default());
            }
            b'S' => match ulstrutils::parse_size(&arg.unwrap_or_default()) {
                Ok(v) => ctl.size = v,
                Err(e) => {
                    warnx(
                        short,
                        &ulstrutils::size_error_message("invalid size argument", &arg_os, e),
                    );
                    return BLKID_EXIT_OTHER;
                }
            },
            b't' => {
                if search.is_some() {
                    warnx(short, "Can only search for one NAME=value pair");
                    return errtryhelp(short, BLKID_EXIT_OTHER);
                }
                match ulblkid::parse_tag_string(&arg.unwrap_or_default()) {
                    Some(tv) => search = Some(tv),
                    None => {
                        warnx(short, "-t needs NAME=value pair");
                        return errtryhelp(short, BLKID_EXIT_OTHER);
                    }
                }
            }
            b'V' | b'v' => {
                out.write(
                    format!(
                        "{} from util-linux 2.39.3  (libblkid 2.39.3, 04-Dec-2023)\n",
                        shown(short)
                    )
                    .as_bytes(),
                );
                return 0;
            }
            // Ignored, for backward compatibility.
            b'w' => {}
            b'h' => {
                out.write(&usage(short));
                return 0;
            }
            _ => return errtryhelp(short, 1),
        }
    }

    if ctl.lowprobe_topology || ctl.lowprobe_superblocks {
        ctl.lowprobe = true;
    }

    // The rest of the arguments are devices: block devices, regular files
    // and UBI volumes; anything else is skipped.
    if !operands.is_empty() {
        devices = operands.into_iter().filter(|d| is_probeable(d)).collect();
        if devices.is_empty() {
            // Only unsupported devices.
            return BLKID_EXIT_NOTFOUND;
        }
    }

    // A LABEL or UUID lookup of a device name is an evaluation.
    if ctl.lookup
        && ctl.output == OUTPUT_DEVICE_ONLY
        && search
            .as_ref()
            .is_some_and(|(t, _)| t.as_slice() == b"LABEL" || t.as_slice() == b"UUID")
    {
        ctl.eval = true;
        ctl.lookup = false;
    }

    let mut cache: Option<BlkCache> = None;
    if !ctl.lowprobe && !ctl.eval {
        cache = Some(BlkCache::get_cache(read.as_deref()));
    }

    if ctl.gc {
        if let Some(c) = cache.as_mut() {
            c.gc_cache();
        }
        return 0;
    }
    let mut err = BLKID_EXIT_NOTFOUND;

    if !ctl.eval && ctl.output & OUTPUT_PRETTY_LIST != 0 {
        if ctl.lowprobe {
            warnx(
                short,
                "The low-level probing mode does not support 'list' output format",
            );
            return BLKID_EXIT_OTHER;
        }
        pretty_print_dev(&mut ctl, out, None, None);
    }

    if ctl.lowprobe {
        // The low-level API.
        if devices.is_empty() {
            warnx(short, "The low-level probing mode requires a device");
            return BLKID_EXIT_OTHER;
        }
        // I/O limits print as `export` unless asked otherwise.
        if ctl.output == 0 && ctl.lowprobe_topology {
            ctl.output = OUTPUT_EXPORT_LIST;
        }
        let mut pr = Probe::new();
        if let Some(h) = &hint
            && pr.set_hint(h, 0) != 0
        {
            warn_errno(
                short,
                &format!("Failed to use probing hint: {}", shown(h)),
                hint_errno(h, errno_left),
            );
            return err;
        }
        if ctl.lowprobe_superblocks {
            pr.set_superblocks_flags(
                SUBLKS_LABEL
                    | SUBLKS_UUID
                    | SUBLKS_TYPE
                    | SUBLKS_SECTYPE
                    | SUBLKS_USAGE
                    | SUBLKS_VERSION
                    | SUBLKS_FSINFO,
            );
            if fltr_usage != 0 {
                if pr.filter_superblocks_usage(fltr_flag, fltr_usage) != 0 {
                    return err;
                }
            } else if let Some(types) = &fltr_type {
                let names: Vec<&[u8]> = types.iter().map(Vec::as_slice).collect();
                if pr.filter_superblocks_type(fltr_flag, &names) != 0 {
                    return err;
                }
            }
        }
        for dev in &devices {
            err = lowprobe_device(&mut pr, dev, &mut ctl, out, short);
            if err != 0 {
                break;
            }
        }
    } else if ctl.eval {
        // The evaluation API.
        let (t, v) = search.clone().unwrap_or_default();
        if let Some(res) = ulblkid::evaluate::evaluate_tag(&t, Some(&v), None) {
            err = 0;
            out.write(&res);
            out.write(b"\n");
        }
    } else if ctl.lookup {
        // The classic, cache-based API.
        let Some((t, v)) = search.clone() else {
            warnx(
                short,
                "The lookup option requires a search type specified using -t",
            );
            return BLKID_EXIT_OTHER;
        };
        let Some(c) = cache.as_mut() else {
            return err;
        };
        // Devices not in the cache yet.
        for dev in &devices {
            // Upstream ignores what `blkid_get_dev` returns here.
            let _ = c.get_dev(dev, DEV_NORMAL);
        }
        if let Some(dev) = c.find_dev_with_tag(&t, &v) {
            print_tags(&mut ctl, out, c, dev);
            err = 0;
        }
    } else if devices.is_empty() {
        // No devices named: every device there is.
        let Some(c) = cache.as_mut() else {
            return err;
        };
        c.probe_all();
        let mut cursor = c.dev_iter_begin();
        let search_ref = search.as_ref().map(|(t, v)| (t.as_slice(), v.as_slice()));
        while let Some(id) = c.dev_next(&mut cursor, search_ref) {
            let Some(id) = c.verify(id) else {
                continue;
            };
            print_tags(&mut ctl, out, c, id);
            err = 0;
        }
    } else {
        // The devices named, added to the cache (and shown).
        let Some(c) = cache.as_mut() else {
            return err;
        };
        for dev in &devices {
            let Some(id) = c.get_dev(dev, DEV_NORMAL) else {
                continue;
            };
            if let Some((t, v)) = &search
                && !c.dev_has_tag(id, t, Some(v))
            {
                continue;
            }
            print_tags(&mut ctl, out, c, id);
            err = 0;
        }
    }
    // The cache is put (written back) as it goes out of scope.
    drop(cache);
    err
}

#[cfg(test)]
mod tests;
