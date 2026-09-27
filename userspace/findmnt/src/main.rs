//! findmnt -- find a filesystem.
//!
//! A port of util-linux 2.39.3's `misc-utils/findmnt.c` and
//! `findmnt-verify.c`, function by function and with upstream's names,
//! reading mount tables through `ulmount` (the port of libmount's table
//! code) and printing through `smartcols` (the libsmartcols port), as
//! upstream reads through libmount and prints through libsmartcols;
//! measured against `findmnt from util-linux 2.39.3` by
//! `scripts/findmnt-diff.sh`.
//!
//! This replaces a hand-written program that parsed the tables itself, laid
//! out its own tree, read argv as UTF-8, and doubled as `mountpoint`
//! (util-linux's `mountpoint` is a program of its own).
//!
//! Upstream's quirks are kept where they show:
//!
//! * A lone operand is a source *or* a mount point: it is looked for as a
//!   source first and, if nothing matches, as a target (`--source` and
//!   `--target` turn that off, as does a `LABEL=`-style operand).
//! * With `--target` naming something that is not a mount point, the
//!   kernel table is searched again for the mount point it is on.
//! * `-t` and `-O` only filter; `--first-only` stops at the first match.
//! * Extra operands after the second are ignored.
//! * `--poll` reports a mount with an empty source as moved on every
//!   change it notices (libmount's `mnt_table_find_pair` refuses an empty
//!   source).
//!
//! # What is not upstream's
//!
//! * **A name in a diagnostic** has its unprintable bytes escaped
//!   (design-decisions §370).

mod sys;
mod verify;

use getoptlong::{Opt, Program, Takes};
use quoting::{escape_unprintable, escaped_in_quotes, os_bytes};
use smartcols::{
    ColumnId, FL_NOEXTREMES as SCOLS_FL_NOEXTREMES, FL_RIGHT as SCOLS_FL_RIGHT,
    FL_STRICTWIDTH as SCOLS_FL_STRICTWIDTH, FL_TREE as SCOLS_FL_TREE, FL_TRUNC as SCOLS_FL_TRUNC,
    FL_WRAP as SCOLS_FL_WRAP, JsonType, LineId, Table,
};
use std::ffi::{OsStr, OsString};
use std::process::ExitCode;
use ulclosestream::{Stdout, stderr_write, warn, warnx};
use ulmount::blkid_cache::BlkCache;
use ulmount::cache::Cache;
use ulmount::fs::{Fs, major, makedev, minor};
use ulmount::optmap::{MS_SHARED, MS_SLAVE, MS_UNBINDABLE};
use ulmount::tab::{Direction, Iter, Table as MntTable, fs_match_source, fs_match_target};
use ulmount::tab_diff::{self, Change};
use ulmount::tab_parse;
use ulstrutils::{
    IdListError, NumErr, SIZE_SUFFIX_1LETTER, num_error_message, size_to_human_string,
    string_add_to_idarray, string_to_idarray,
};

/// Getopt's errors are only sentences here; the referral follows them.
const FINDMNT: Program = Program::new("findmnt", 1);

/// Upstream's option string.
const SHORTS: &str = "AabCcDd:ehiJfF:o:O:p::PklmM:nN:rst:uvRS:T:Uw:Vxy";

/// `FINDMNT_OPT_*`: `CHAR_MAX + 1` on.
const OPT_VERBOSE: i32 = 128;
const OPT_TREE: i32 = 129;
const OPT_OUTPUT_ALL: i32 = 130;
const OPT_PSEUDO: i32 = 131;
const OPT_REAL: i32 = 132;
const OPT_VFS_ALL: i32 = 133;
const OPT_SHADOWED: i32 = 134;

/// Upstream's `longopts[]`, in its order (the order an ambiguity lists),
/// and each one's `val`.
const LONGS: &[(&str, Takes)] = &[
    ("all", Takes::Nothing),
    ("ascii", Takes::Nothing),
    ("bytes", Takes::Nothing),
    ("canonicalize", Takes::Nothing),
    ("direction", Takes::Required),
    ("df", Takes::Nothing),
    ("evaluate", Takes::Nothing),
    ("first-only", Takes::Nothing),
    ("fstab", Takes::Nothing),
    ("help", Takes::Nothing),
    ("invert", Takes::Nothing),
    ("json", Takes::Nothing),
    ("kernel", Takes::Nothing),
    ("list", Takes::Nothing),
    ("mountpoint", Takes::Required),
    ("mtab", Takes::Nothing),
    ("noheadings", Takes::Nothing),
    ("notruncate", Takes::Nothing),
    ("options", Takes::Required),
    ("output", Takes::Required),
    ("output-all", Takes::Nothing),
    ("poll", Takes::Optional),
    ("pairs", Takes::Nothing),
    ("raw", Takes::Nothing),
    ("types", Takes::Required),
    ("nocanonicalize", Takes::Nothing),
    ("nofsroot", Takes::Nothing),
    ("submounts", Takes::Nothing),
    ("source", Takes::Required),
    ("tab-file", Takes::Required),
    ("task", Takes::Required),
    ("target", Takes::Required),
    ("timeout", Takes::Required),
    ("uniq", Takes::Nothing),
    ("verify", Takes::Nothing),
    ("version", Takes::Nothing),
    ("shell", Takes::Nothing),
    ("verbose", Takes::Nothing),
    ("tree", Takes::Nothing),
    ("real", Takes::Nothing),
    ("pseudo", Takes::Nothing),
    ("vfs-all", Takes::Nothing),
    ("shadowed", Takes::Nothing),
];
const LONG_VALS: [i32; 43] = [
    b'A' as i32,
    b'a' as i32,
    b'b' as i32,
    b'c' as i32,
    b'd' as i32,
    b'D' as i32,
    b'e' as i32,
    b'f' as i32,
    b's' as i32,
    b'h' as i32,
    b'i' as i32,
    b'J' as i32,
    b'k' as i32,
    b'l' as i32,
    b'M' as i32,
    b'm' as i32,
    b'n' as i32,
    b'u' as i32,
    b'O' as i32,
    b'o' as i32,
    OPT_OUTPUT_ALL,
    b'p' as i32,
    b'P' as i32,
    b'r' as i32,
    b't' as i32,
    b'C' as i32,
    b'v' as i32,
    b'R' as i32,
    b'S' as i32,
    b'F' as i32,
    b'N' as i32,
    b'T' as i32,
    b'w' as i32,
    b'U' as i32,
    b'x' as i32,
    b'V' as i32,
    b'y' as i32,
    OPT_VERBOSE,
    OPT_TREE,
    OPT_REAL,
    OPT_PSEUDO,
    OPT_VFS_ALL,
    OPT_SHADOWED,
];

/// `excl[]`, rows in upstream's order -- which is not quite ASCII order,
/// and the check's early exit makes that show (`-m -p` is accepted).
const EXCL: [&[i32]; 9] = [
    &[b'C' as i32, b'c' as i32],
    &[b'C' as i32, b'e' as i32],
    &[b'J' as i32, b'P' as i32, b'r' as i32, b'x' as i32],
    &[b'M' as i32, b'T' as i32],
    &[b'N' as i32, b'k' as i32, b'm' as i32, b's' as i32],
    &[b'P' as i32, b'l' as i32, b'r' as i32, b'x' as i32],
    &[b'p' as i32, b'x' as i32],
    &[b'm' as i32, b'p' as i32, b's' as i32],
    &[OPT_PSEUDO, OPT_REAL],
];

/// `FL_*` of `findmnt.h`.
pub(crate) const FL_EVALUATE: u32 = 1 << 1;
pub(crate) const FL_CANONICALIZE: u32 = 1 << 2;
pub(crate) const FL_FIRSTONLY: u32 = 1 << 3;
pub(crate) const FL_INVERT: u32 = 1 << 4;
pub(crate) const FL_NOSWAPMATCH: u32 = 1 << 6;
pub(crate) const FL_NOFSROOT: u32 = 1 << 7;
pub(crate) const FL_SUBMOUNTS: u32 = 1 << 8;
pub(crate) const FL_POLL: u32 = 1 << 9;
pub(crate) const FL_DF: u32 = 1 << 10;
pub(crate) const FL_ALL: u32 = 1 << 11;
pub(crate) const FL_UNIQ: u32 = 1 << 12;
pub(crate) const FL_BYTES: u32 = 1 << 13;
pub(crate) const FL_NOCACHE: u32 = 1 << 14;
pub(crate) const FL_STRICTTARGET: u32 = 1 << 15;
pub(crate) const FL_VERBOSE: u32 = 1 << 16;
pub(crate) const FL_PSEUDO: u32 = 1 << 17;
pub(crate) const FL_REAL: u32 = 1 << 18;
pub(crate) const FL_VFS_ALL: u32 = 1 << 19;
pub(crate) const FL_SHADOWED: u32 = 1 << 20;
pub(crate) const FL_SHELLVAR: u32 = 1 << 22;
pub(crate) const FL_ASCII: u32 = 1 << 25;
pub(crate) const FL_RAW: u32 = 1 << 26;
pub(crate) const FL_NOHEADINGS: u32 = 1 << 27;
pub(crate) const FL_EXPORT: u32 = 1 << 28;
pub(crate) const FL_TREE: u32 = 1 << 29;
pub(crate) const FL_JSON: u32 = 1 << 30;

/// `COL_*`, in upstream's enum order -- `infos[]`' order, and so the order
/// `--help` lists them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Col {
    Action,
    Avail,
    Freq,
    Fsroot,
    Fstype,
    FsOptions,
    Id,
    Label,
    Majmin,
    OldOptions,
    OldTarget,
    Options,
    OptFields,
    Parent,
    Partlabel,
    Partuuid,
    Passno,
    Propagation,
    Size,
    Source,
    Sources,
    Target,
    Tid,
    Used,
    Useperc,
    Uuid,
    VfsOptions,
}

/// `struct colinfo`'s constant half.
struct ColInfo {
    id: Col,
    name: &'static str,
    whint: f64,
    flags: u32,
    help: &'static str,
}

/// `infos[]`.
const INFOS: [ColInfo; 27] = [
    ColInfo {
        id: Col::Action,
        name: "ACTION",
        whint: 10.0,
        flags: SCOLS_FL_STRICTWIDTH,
        help: "action detected by --poll",
    },
    ColInfo {
        id: Col::Avail,
        name: "AVAIL",
        whint: 5.0,
        flags: SCOLS_FL_RIGHT,
        help: "filesystem size available",
    },
    ColInfo {
        id: Col::Freq,
        name: "FREQ",
        whint: 1.0,
        flags: SCOLS_FL_RIGHT,
        help: "dump(8) period in days [fstab only]",
    },
    ColInfo {
        id: Col::Fsroot,
        name: "FSROOT",
        whint: 0.25,
        flags: SCOLS_FL_NOEXTREMES,
        help: "filesystem root",
    },
    ColInfo {
        id: Col::Fstype,
        name: "FSTYPE",
        whint: 0.10,
        flags: SCOLS_FL_TRUNC,
        help: "filesystem type",
    },
    ColInfo {
        id: Col::FsOptions,
        name: "FS-OPTIONS",
        whint: 0.10,
        flags: SCOLS_FL_TRUNC,
        help: "FS specific mount options",
    },
    ColInfo {
        id: Col::Id,
        name: "ID",
        whint: 2.0,
        flags: SCOLS_FL_RIGHT,
        help: "mount ID",
    },
    ColInfo {
        id: Col::Label,
        name: "LABEL",
        whint: 0.10,
        flags: 0,
        help: "filesystem label",
    },
    ColInfo {
        id: Col::Majmin,
        name: "MAJ:MIN",
        whint: 6.0,
        flags: 0,
        help: "major:minor device number",
    },
    ColInfo {
        id: Col::OldOptions,
        name: "OLD-OPTIONS",
        whint: 0.10,
        flags: SCOLS_FL_TRUNC,
        help: "old mount options saved by --poll",
    },
    ColInfo {
        id: Col::OldTarget,
        name: "OLD-TARGET",
        whint: 0.30,
        flags: 0,
        help: "old mountpoint saved by --poll",
    },
    ColInfo {
        id: Col::Options,
        name: "OPTIONS",
        whint: 0.10,
        flags: SCOLS_FL_TRUNC,
        help: "all mount options",
    },
    ColInfo {
        id: Col::OptFields,
        name: "OPT-FIELDS",
        whint: 0.10,
        flags: SCOLS_FL_TRUNC,
        help: "optional mount fields",
    },
    ColInfo {
        id: Col::Parent,
        name: "PARENT",
        whint: 2.0,
        flags: SCOLS_FL_RIGHT,
        help: "mount parent ID",
    },
    ColInfo {
        id: Col::Partlabel,
        name: "PARTLABEL",
        whint: 0.10,
        flags: 0,
        help: "partition label",
    },
    ColInfo {
        id: Col::Partuuid,
        name: "PARTUUID",
        whint: 36.0,
        flags: 0,
        help: "partition UUID",
    },
    ColInfo {
        id: Col::Passno,
        name: "PASSNO",
        whint: 1.0,
        flags: SCOLS_FL_RIGHT,
        help: "pass number on parallel fsck(8) [fstab only]",
    },
    ColInfo {
        id: Col::Propagation,
        name: "PROPAGATION",
        whint: 0.10,
        flags: 0,
        help: "VFS propagation flags",
    },
    ColInfo {
        id: Col::Size,
        name: "SIZE",
        whint: 5.0,
        flags: SCOLS_FL_RIGHT,
        help: "filesystem size",
    },
    ColInfo {
        id: Col::Source,
        name: "SOURCE",
        whint: 0.25,
        flags: SCOLS_FL_NOEXTREMES,
        help: "source device",
    },
    ColInfo {
        id: Col::Sources,
        name: "SOURCES",
        whint: 0.25,
        flags: SCOLS_FL_WRAP,
        help: "all possible source devices",
    },
    ColInfo {
        id: Col::Target,
        name: "TARGET",
        whint: 0.30,
        flags: SCOLS_FL_TREE | SCOLS_FL_NOEXTREMES,
        help: "mountpoint",
    },
    ColInfo {
        id: Col::Tid,
        name: "TID",
        whint: 4.0,
        flags: SCOLS_FL_RIGHT,
        help: "task ID",
    },
    ColInfo {
        id: Col::Used,
        name: "USED",
        whint: 5.0,
        flags: SCOLS_FL_RIGHT,
        help: "filesystem size used",
    },
    ColInfo {
        id: Col::Useperc,
        name: "USE%",
        whint: 3.0,
        flags: SCOLS_FL_RIGHT,
        help: "filesystem use percentage",
    },
    ColInfo {
        id: Col::Uuid,
        name: "UUID",
        whint: 36.0,
        flags: 0,
        help: "filesystem UUID",
    },
    ColInfo {
        id: Col::VfsOptions,
        name: "VFS-OPTIONS",
        whint: 0.20,
        flags: SCOLS_FL_TRUNC,
        help: "VFS specific mount options",
    },
];

/// `columns[ARRAY_SIZE(infos) * 2]`.
const MAX_COLUMNS: usize = 54;
/// `actions[FINDMNT_NACTIONS]`.
const NACTIONS: usize = 4;

/// `TABTYPE_*`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TabType {
    Fstab,
    Mtab,
    Kernel,
}

/// The column's `infos[]` entry: `INFOS` is in `Col`'s order, so this is
/// the entry at the column's own index.
fn info(col: Col) -> &'static ColInfo {
    INFOS.get(col as usize).unwrap_or(&INFOS[0])
}

/// `is_tabdiff_column(id)`.
fn is_tabdiff_column(col: Col) -> bool {
    matches!(col, Col::Action | Col::OldTarget | Col::OldOptions)
}

/// The match patterns `infos[]` carries (`set_match`/`get_match`), and
/// `COL_MAJMIN`'s device number.
#[derive(Clone, Debug, Default)]
pub(crate) struct Matches {
    pub(crate) source: Option<Vec<u8>>,
    pub(crate) target: Option<Vec<u8>>,
    pub(crate) fstype: Option<Vec<u8>>,
    pub(crate) options: Option<Vec<u8>>,
    pub(crate) majmin: Option<Vec<u8>>,
    pub(crate) majmin_devno: Option<u64>,
}

/// findmnt's globals: `flags`, `parse_nerrors`, `cache`, `columns[]`,
/// `actions[]`, the writable `infos[]` flags and matches, and libblkid's
/// cache for SOURCES.
pub(crate) struct Findmnt {
    pub(crate) flags: u32,
    pub(crate) parse_nerrors: i32,
    pub(crate) cache: Option<Cache>,
    pub(crate) matches: Matches,
    columns: Vec<Col>,
    col_flags: [u32; 27],
    actions: Vec<Change>,
    blk_cache: Option<BlkCache>,
    pub(crate) short: Vec<u8>,
    /// The `errno` a failure that sets none reports: what `setlocale`
    /// left (`smartcols::tty::setlocale_errno`), since nothing between it
    /// and the table's parsing changes `errno`.
    errno_left: i32,
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
pub(crate) fn shown(text: &[u8]) -> String {
    escape_unprintable(text)
}

/// `errtryhelp(EXIT_FAILURE)`.
fn errtryhelp(short: &[u8]) -> u8 {
    stderr_write(format!("Try '{} --help' for more information.\n", shown(short)).as_bytes());
    1
}

/// `usage()`.
fn usage(short: &[u8]) -> Vec<u8> {
    let s = shown(short);
    let mut t = String::from("\nUsage:\n");
    t.push_str(&format!(
        " {s} [options]\n {s} [options] <device> | <mountpoint>\n {s} [options] <device> <mountpoint>\n {s} [options] [--source <device>] [--target <path> | --mountpoint <dir>]\n"
    ));
    t.push_str("\nFind a (mounted) filesystem.\n");
    t.push_str("\nOptions:\n");
    t.push_str(" -s, --fstab            search in static table of filesystems\n");
    t.push_str(" -m, --mtab             search in table of mounted filesystems\n                          (includes user space mount options)\n");
    t.push_str(" -k, --kernel           search in kernel table of mounted\n                          filesystems (default)\n");
    t.push('\n');
    t.push_str(" -p, --poll[=<list>]    monitor changes in table of mounted filesystems\n");
    t.push_str(" -w, --timeout <num>    upper limit in milliseconds that --poll will block\n");
    t.push('\n');
    t.push_str(" -A, --all              disable all built-in filters, print all filesystems\n");
    t.push_str(" -a, --ascii            use ASCII chars for tree formatting\n");
    t.push_str(
        " -b, --bytes            print sizes in bytes rather than in human readable format\n",
    );
    t.push_str(" -C, --nocanonicalize   don't canonicalize when comparing paths\n");
    t.push_str(" -c, --canonicalize     canonicalize printed paths\n");
    t.push_str(" -D, --df               imitate the output of df(1)\n");
    t.push_str(" -d, --direction <word> direction of search, 'forward' or 'backward'\n");
    t.push_str(" -e, --evaluate         convert tags (LABEL,UUID,PARTUUID,PARTLABEL) \n                          to device names\n");
    t.push_str(" -F, --tab-file <path>  alternative file for -s, -m or -k options\n");
    t.push_str(" -f, --first-only       print the first found filesystem only\n");
    t.push_str(" -i, --invert           invert the sense of matching\n");
    t.push_str(" -J, --json             use JSON output format\n");
    t.push_str(" -l, --list             use list format output\n");
    t.push_str(" -N, --task <tid>       use alternative namespace (/proc/<tid>/mountinfo file)\n");
    t.push_str(" -n, --noheadings       don't print column headings\n");
    t.push_str(" -O, --options <list>   limit the set of filesystems by mount options\n");
    t.push_str(" -o, --output <list>    the output columns to be shown\n");
    t.push_str("     --output-all       output all available columns\n");
    t.push_str(" -P, --pairs            use key=\"value\" output format\n");
    t.push_str("     --pseudo           print only pseudo-filesystems\n");
    t.push_str(
        "     --shadowed         print only filesystems over-mounted by another filesystem\n",
    );
    t.push_str(" -R, --submounts        print all submounts for the matching filesystems\n");
    t.push_str(" -r, --raw              use raw output format\n");
    t.push_str("     --real             print only real filesystems\n");
    t.push_str(" -S, --source <string>  the device to mount (by name, maj:min, \n                          LABEL=, UUID=, PARTUUID=, PARTLABEL=)\n");
    t.push_str(" -T, --target <path>    the path to the filesystem to use\n");
    t.push_str("     --tree             enable tree format output if possible\n");
    t.push_str(" -M, --mountpoint <dir> the mountpoint directory\n");
    t.push_str(" -t, --types <list>     limit the set of filesystems by FS types\n");
    t.push_str(" -U, --uniq             ignore filesystems with duplicate target\n");
    t.push_str(" -u, --notruncate       don't truncate text in columns\n");
    t.push_str(" -v, --nofsroot         don't print [/dir] for bind or btrfs mounts\n");
    t.push_str(
        " -y, --shell            use column names to be usable as shell variable identifiers\n",
    );
    t.push('\n');
    t.push_str(" -x, --verify           verify mount table content (default is fstab)\n");
    t.push_str("     --verbose          print more details\n");
    t.push_str("     --vfs-all          print all VFS options\n");
    t.push('\n');
    t.push_str(&format!(
        "{:<24}{}\n{:<24}{}\n",
        " -h, --help", "display this help", " -V, --version", "display version"
    ));
    t.push_str("\nAvailable output columns:\n");
    for i in &INFOS {
        t.push_str(&format!(" {:>11}  {}\n", i.name, i.help));
    }
    t.push_str("\nFor more details see findmnt(8).\n");
    t.into_bytes()
}

/// `sscanf(data, "%d:%d", &maj, &min) == 2`: `%d` is `strtol`, stored in
/// an `int`.
fn scan_majmin_signed(s: &[u8]) -> Option<(i32, i32)> {
    let int = |t: &[u8]| -> Option<(i32, usize)> {
        let sc = ulstrutils::scan_integer(t, 10)?;
        let limit = if sc.negative {
            1u128 << 63
        } else {
            (1u128 << 63) - 1
        };
        let v: i64 = if sc.saturated || sc.magnitude > limit {
            if sc.negative { i64::MIN } else { i64::MAX }
        } else {
            let m = i128::try_from(sc.magnitude).unwrap_or(0);
            i64::try_from(if sc.negative { m.saturating_neg() } else { m }).unwrap_or(0)
        };
        Some((v as i32, sc.end))
    };
    let (maj, end) = int(s)?;
    if s.get(end) != Some(&b':') {
        return None;
    }
    let (min, _) = int(s.get(end.saturating_add(1)..)?)?;
    Some((maj, min))
}

impl Findmnt {
    fn new(short: Vec<u8>) -> Self {
        let mut col_flags = [0u32; 27];
        for (slot, i) in col_flags.iter_mut().zip(INFOS.iter()) {
            *slot = i.flags;
        }
        Findmnt {
            flags: 0,
            parse_nerrors: 0,
            cache: None,
            matches: Matches::default(),
            columns: Vec::new(),
            col_flags,
            actions: Vec::new(),
            blk_cache: None,
            short,
            errno_left: smartcols::tty::setlocale_errno(),
        }
    }

    /// `set_source_match(data)`: `MAJ:MIN` matches the device number (and
    /// is never swapped for a target), anything else the source.
    fn set_source_match(&mut self, data: &[u8]) {
        if let Some((maj, min)) = scan_majmin_signed(data) {
            self.matches.majmin = Some(data.to_vec());
            self.matches.majmin_devno = Some(makedev(maj as u32, min as u32));
            self.flags |= FL_NOSWAPMATCH;
        } else {
            self.matches.source = Some(data.to_vec());
        }
    }

    /// `is_listall_mode()`: no filter at all.
    pub(crate) fn is_listall_mode(&self) -> bool {
        if (self.flags & (FL_DF | FL_REAL | FL_PSEUDO) != 0) && self.flags & FL_ALL == 0 {
            return false;
        }
        let m = &self.matches;
        m.source.is_none()
            && m.target.is_none()
            && m.fstype.is_none()
            && m.options.is_none()
            && m.majmin.is_none()
    }

    /// `has_poll_action(act)`.
    fn has_poll_action(&self, act: Change) -> bool {
        self.actions.is_empty() || self.actions.contains(&act)
    }

    /// `is_mount_compatible_mode()`: `findmnt -f <spec>`, looked up as
    /// mount(8) looks it up in fstab.
    fn is_mount_compatible_mode(&self) -> bool {
        self.matches.source.is_some()
            && self.matches.fstype.is_none()
            && self.matches.options.is_none()
            && self.flags & FL_FIRSTONLY != 0
    }

    /// The column's flags as `infos[]` holds them now (`-u` takes away
    /// the truncation).
    fn col_flag(&self, col: Col) -> u32 {
        self.col_flags
            .get(col as usize)
            .copied()
            .unwrap_or(info(col).flags)
    }

    /// `disable_columns_truncate()`.
    fn disable_columns_truncate(&mut self) {
        for f in &mut self.col_flags {
            *f &= !SCOLS_FL_TRUNC;
        }
    }

    /// `enable_extra_target_match(tb)`: a target that is not a mount point
    /// is replaced by the mount point it is on.
    fn enable_extra_target_match(&mut self, tb: &MntTable) {
        let Some(m) = self.matches.target.clone() else {
            return;
        };
        let tgt = if self.flags & FL_NOCACHE != 0 {
            m
        } else {
            let resolved = match self.cache.as_mut() {
                Some(c) => c.resolve_path(&m),
                None => ulmount::cache::canonicalize_path(&m),
            };
            let Some(cn) = resolved else {
                return;
            };
            cn
        };
        let found = tb.find_mountpoint(&tgt, Direction::Backward, self.cache.as_mut());
        if let Some(mnt) = found
            .and_then(|i| tb.ents.get(i))
            .and_then(|fs| fs.target.clone())
            && mnt != tgt
        {
            self.matches.target = Some(mnt);
        }
    }

    /// `match_func(fs, NULL)`: whether entry `i` of `tb` passes the filters.
    pub(crate) fn match_func(&mut self, tb: &MntTable, i: usize) -> bool {
        let rc = self.flags & FL_INVERT != 0;
        let Some(fs) = tb.ents.get(i) else {
            return rc;
        };
        if let Some(m) = &self.matches.fstype
            && !fs.match_fstype(Some(m))
        {
            return rc;
        }
        if let Some(m) = &self.matches.options
            && !fs.match_options(Some(m))
        {
            return rc;
        }
        if let Some(d) = self.matches.majmin_devno
            && fs.devno != d
        {
            return rc;
        }
        if let Some(m) = &self.matches.target
            && !fs_match_target(fs, m, self.cache.as_mut())
        {
            return rc;
        }
        if let Some(m) = &self.matches.source
            && !fs_match_source(fs, Some(m), self.cache.as_mut())
        {
            return rc;
        }
        if self.flags & FL_DF != 0 && self.flags & FL_ALL == 0 {
            if fs
                .fstype
                .as_deref()
                .is_some_and(|t| t.windows(5).any(|w| w == b"tmpfs"))
            {
                // tmpfs is wanted.
                return !rc;
            }
            if fs.is_pseudofs() {
                return rc;
            }
        }
        if self.flags & FL_REAL != 0 && fs.is_pseudofs() {
            return rc;
        }
        if self.flags & FL_PSEUDO != 0 && !fs.is_pseudofs() {
            return rc;
        }
        if self.flags & FL_SHADOWED != 0 && tb.over_fs(i).is_none() {
            return rc;
        }
        !rc
    }

    /// `get_next_fs(tb, itr)`.
    pub(crate) fn get_next_fs(&mut self, tb: &MntTable, itr: &mut Iter) -> Option<usize> {
        if self.is_listall_mode() {
            return tb.next_fs(itr);
        }
        if self.is_mount_compatible_mode() {
            let source = self.matches.source.clone()?;
            let fs = tb.find_source(&source, itr.direction, self.cache.as_mut());
            if fs.is_none() && self.flags & FL_NOSWAPMATCH == 0 {
                return tb.find_target(&source, itr.direction, self.cache.as_mut());
            }
            return fs;
        }
        loop {
            let fs = tb.find_next_fs(itr, |t, i| self.match_func(t, i));
            if fs.is_none()
                && self.flags & FL_NOSWAPMATCH == 0
                && self.matches.target.is_none()
                && self.matches.source.is_some()
            {
                // Swap 'spec' and target.
                self.matches.target = self.matches.source.take();
                itr.reset(None);
                continue;
            }
            return fs;
        }
    }

    /// `poll_match(fs)`: as `match_func`, and a lone operand tried as a
    /// target too.
    fn poll_match(&mut self, tb: &MntTable, i: usize) -> bool {
        let rc = self.match_func(tb, i);
        if !rc
            && self.flags & FL_NOSWAPMATCH == 0
            && self.matches.source.is_some()
            && self.matches.target.is_none()
        {
            let s = self.matches.source.take();
            self.matches.target.clone_from(&s);
            let rc = self.match_func(tb, i);
            self.matches.target = None;
            self.matches.source = s;
            return rc;
        }
        rc
    }

    /// `get_tag_from_udev(devname, col)`.
    fn get_tag_from_udev(devname: &[u8], col: Col) -> Option<Vec<u8>> {
        // libudev does not like /dev/mapper/ symlinks.
        let real = std::fs::canonicalize(quoting::os_from_bytes(devname))
            .ok()
            .map(|p| os_bytes(p.as_os_str()).into_owned());
        let name = real.as_deref().unwrap_or(devname);
        let name = name.strip_prefix(b"/dev/").unwrap_or(name);
        let key: &[u8] = match col {
            Col::Label => b"ID_FS_LABEL_ENC",
            Col::Uuid => b"ID_FS_UUID_ENC",
            Col::Partuuid => b"ID_PART_ENTRY_UUID",
            Col::Partlabel => b"ID_PART_ENTRY_NAME",
            _ => return None,
        };
        let data = ulmount::udev::property(name, key)?;
        Some(ulmount::mangle::unhexmangle(&data))
    }

    /// `get_tag(fs, tagname, col)`: LABEL, UUID, PARTUUID or PARTLABEL --
    /// the entry's own tag, else udev's, else the device's own, probed.
    fn get_tag(&mut self, fs: &Fs, tagname: &[u8], col: Col) -> Option<Vec<u8>> {
        if let Some((t, v)) = fs.tag()
            && t == tagname
        {
            return Some(v.to_vec());
        }
        let mut dev = fs.source.clone();
        if let Some(d) = &dev
            && self.flags & FL_NOCACHE == 0
        {
            dev = match self.cache.as_mut() {
                Some(c) => c.resolve_spec(d),
                None => None,
            };
        }
        if let Some(d) = &dev
            && let Some(res) = Self::get_tag_from_udev(d, col)
        {
            return Some(res);
        }
        let dev = dev?;
        self.cache.as_mut()?.find_tag_value(&dev, tagname)
    }

    /// `get_vfs_attr(fs, sizetype)`: SIZE, AVAIL, USED or USE% from
    /// `statvfs` of the mount point.
    fn get_vfs_attr(&self, fs: &Fs, col: Col) -> Option<Vec<u8>> {
        let target = fs.target.as_deref()?;
        let buf = sys::stat_vfs(target).ok()?;
        let vfs_attr = match col {
            Col::Size => buf.f_frsize.wrapping_mul(buf.f_blocks),
            Col::Avail => buf.f_frsize.wrapping_mul(buf.f_bavail),
            Col::Used => buf
                .f_frsize
                .wrapping_mul(buf.f_blocks.wrapping_sub(buf.f_bfree)),
            Col::Useperc => {
                if buf.f_blocks == 0 {
                    return Some(b"-".to_vec());
                }
                #[allow(
                    clippy::cast_precision_loss,
                    reason = "upstream's own double arithmetic"
                )]
                let pct =
                    (buf.f_blocks.wrapping_sub(buf.f_bfree)) as f64 / buf.f_blocks as f64 * 100.0;
                return Some(format!("{pct:.0}%").into_bytes());
            }
            _ => return None,
        };
        Some(if vfs_attr == 0 {
            b"0".to_vec()
        } else if self.flags & FL_BYTES != 0 {
            vfs_attr.to_string().into_bytes()
        } else {
            size_to_human_string(SIZE_SUFFIX_1LETTER, vfs_attr).into_bytes()
        })
    }

    /// `get_data_col_sources(fs, evaluate)`: every device with the entry's
    /// tag (with `--evaluate`), or with its device's UUID, one per line --
    /// from libblkid's cache.
    fn get_data_col_sources(&mut self, fs: &Fs, evaluate: bool) -> Option<Vec<u8>> {
        let (tag, val): (Vec<u8>, Option<Vec<u8>>) = if let Some((t, v)) = fs.tag() {
            // A LABEL= or UUID= entry without --evaluate names one device.
            if !evaluate {
                return None;
            }
            (t.to_vec(), Some(v.to_vec()))
        } else {
            match fs.srcpath() {
                Some(d) if d.starts_with(b"/dev/") => (b"UUID".to_vec(), None),
                _ => return None,
            }
        };
        let bc = self.blk_cache.get_or_insert_with(|| {
            let mut c = BlkCache::get_cache(None);
            let _ = c.probe_all();
            c
        });
        let val = match val {
            Some(v) => v,
            None => bc.get_tag_value(b"UUID", fs.srcpath()?)?,
        };
        // One device per line; none at all is no cell, and SOURCE's text is
        // shown instead.
        let mut out: Option<Vec<u8>> = None;
        let mut cursor = bc.dev_iter_begin();
        while let Some(dev) = bc.dev_next(&mut cursor, Some((&tag, &val))) {
            let Some(dev) = bc.verify(dev) else {
                continue;
            };
            let Some(name) = bc.devname(dev) else {
                continue;
            };
            match out.as_mut() {
                Some(o) => {
                    o.push(b'\n');
                    o.extend_from_slice(name);
                }
                None => out = Some(name.to_vec()),
            }
        }
        out
    }

    /// `get_data(fs, num)`: the text of column `col` for entry `fs`.
    fn get_data(&mut self, fs: &Fs, col: Col) -> Option<Vec<u8>> {
        match col {
            Col::Sources | Col::Source => {
                if col == Col::Sources
                    && let Some(s) = self.get_data_col_sources(fs, self.flags & FL_EVALUATE != 0)
                {
                    return Some(s);
                }
                let root = fs.root.clone();
                let mut spec = fs.srcpath().map(<[u8]>::to_vec);
                if let Some(s) = &spec
                    && self.flags & FL_CANONICALIZE != 0
                {
                    spec = match self.cache.as_mut() {
                        Some(c) => c.resolve_path(s),
                        None => ulmount::cache::canonicalize_path(s),
                    };
                }
                if spec.is_none() {
                    spec = fs.source.clone();
                    if let Some(s) = &spec
                        && self.flags & FL_EVALUATE != 0
                    {
                        spec = match self.cache.as_mut() {
                            Some(c) => c.resolve_spec(s),
                            None => Cache::new().resolve_spec(s),
                        };
                    }
                }
                let spec = spec?;
                match root {
                    Some(r) if self.flags & FL_NOFSROOT == 0 && r != b"/" => {
                        let mut s = spec;
                        s.push(b'[');
                        s.extend_from_slice(&r);
                        s.push(b']');
                        Some(s)
                    }
                    _ => Some(spec),
                }
            }
            Col::Target => fs.target.clone(),
            Col::Fstype => fs.fstype.clone(),
            Col::Options => fs.optstr.clone(),
            Col::VfsOptions => {
                if self.flags & FL_VFS_ALL != 0 {
                    fs.vfs_options_all()
                } else {
                    fs.vfs_optstr.clone()
                }
            }
            Col::FsOptions => fs.fs_optstr.clone(),
            Col::OptFields => fs.opt_fields.clone(),
            Col::Uuid => self.get_tag(fs, b"UUID", col),
            Col::Partuuid => self.get_tag(fs, b"PARTUUID", col),
            Col::Label => self.get_tag(fs, b"LABEL", col),
            Col::Partlabel => self.get_tag(fs, b"PARTLABEL", col),
            Col::Majmin => {
                if fs.devno == 0 {
                    return None;
                }
                let (ma, mi) = (major(fs.devno), minor(fs.devno));
                Some(if self.flags & (FL_RAW | FL_EXPORT | FL_JSON) != 0 {
                    format!("{ma}:{mi}").into_bytes()
                } else {
                    format!("{ma:3}:{mi:<3}").into_bytes()
                })
            }
            Col::Size | Col::Avail | Col::Used | Col::Useperc => self.get_vfs_attr(fs, col),
            Col::Fsroot => fs.root.clone(),
            Col::Tid => (fs.tid != 0).then(|| fs.tid.to_string().into_bytes()),
            Col::Id => (fs.id != 0).then(|| fs.id.to_string().into_bytes()),
            Col::Parent => (fs.parent != 0).then(|| fs.parent.to_string().into_bytes()),
            Col::Propagation => {
                if !fs.is_kernel() {
                    return None;
                }
                let fl = fs.propagation();
                let mut s = if fl & MS_SHARED != 0 {
                    "shared"
                } else {
                    "private"
                }
                .to_string();
                if fl & MS_SLAVE != 0 {
                    s.push_str(",slave");
                }
                if fl & MS_UNBINDABLE != 0 {
                    s.push_str(",unbindable");
                }
                Some(s.into_bytes())
            }
            Col::Freq => (!fs.is_kernel()).then(|| fs.freq.to_string().into_bytes()),
            Col::Passno => (!fs.is_kernel()).then(|| fs.passno.to_string().into_bytes()),
            Col::Action | Col::OldOptions | Col::OldTarget => None,
        }
    }

    /// `get_tabdiff_data(old_fs, new_fs, change, num)`.
    fn get_tabdiff_data(
        &mut self,
        old_fs: Option<&Fs>,
        new_fs: Option<&Fs>,
        change: Change,
        col: Col,
    ) -> Option<Vec<u8>> {
        match col {
            Col::Action => Some(
                match change {
                    Change::Mount => "mount",
                    Change::Umount => "umount",
                    Change::Remount => "remount",
                    Change::Move => "move",
                }
                .as_bytes()
                .to_vec(),
            ),
            Col::OldOptions => old_fs
                .filter(|_| matches!(change, Change::Remount | Change::Umount))
                .and_then(|f| f.optstr.clone()),
            Col::OldTarget => old_fs
                .filter(|_| matches!(change, Change::Move | Change::Umount))
                .and_then(|f| f.target.clone()),
            _ => match new_fs.or(old_fs) {
                Some(f) => self.get_data(f, col),
                None => None,
            },
        }
    }

    /// `add_line(table, fs, parent)`.
    fn add_line(
        &mut self,
        table: &mut Table,
        ids: &[ColumnId],
        tb: &MntTable,
        i: usize,
        parent: Option<LineId>,
        lines: &mut Vec<usize>,
    ) -> Option<LineId> {
        let fs = tb.ents.get(i)?.clone();
        let Ok(line) = table.new_line(parent) else {
            return None;
        };
        for (n, &col) in self.columns.clone().iter().enumerate() {
            if let Some(data) = self.get_data(&fs, col)
                && let Some(&id) = ids.get(n)
            {
                // The line and column are this table's own.
                let _ = table.line_set_data(line, id, &data);
            }
        }
        lines.push(i);
        Some(line)
    }

    /// `create_treenode(table, tb, fs, parent_line)`: the entry and its
    /// children -- and, at the top, any entry the tree did not reach.
    fn create_treenode(
        &mut self,
        table: &mut Table,
        ids: &[ColumnId],
        tb: &MntTable,
        fs: Option<usize>,
        parent_line: Option<LineId>,
        lines: &mut Vec<usize>,
    ) -> Result<(), ()> {
        let (fs, parent_line, first) = match fs {
            None => (tb.root_fs().ok_or(())?, None, true),
            Some(f) => {
                if self.flags & FL_SUBMOUNTS != 0 && lines.contains(&f) {
                    return Ok(());
                }
                (f, parent_line, false)
            }
        };
        let line = if self.flags & FL_SUBMOUNTS != 0 || self.match_func(tb, fs) {
            Some(
                self.add_line(table, ids, tb, fs, parent_line, lines)
                    .ok_or(())?,
            )
        } else {
            parent_line
        };
        let mut last = None;
        while let Some(chld) = tb.next_child_fs(fs, last, Direction::Forward) {
            last = Some(chld);
            self.create_treenode(table, ids, tb, Some(chld), line, lines)?;
        }
        if first && tb.nents() > table.nlines() {
            for i in 0..tb.nents() {
                if !lines.contains(&i) && self.match_func(tb, i) {
                    // Upstream ignores how each of these went.
                    let _ = self.create_treenode(table, ids, tb, Some(i), None, lines);
                }
            }
        }
        Ok(())
    }

    /// `add_matching_lines(tb, table, direction)`: `Err` when nothing
    /// matched.
    fn add_matching_lines(
        &mut self,
        tb: &MntTable,
        table: &mut Table,
        ids: &[ColumnId],
        direction: Direction,
        lines: &mut Vec<usize>,
    ) -> Result<(), ()> {
        let mut itr = Iter::new(direction);
        let mut nlines = 0usize;
        while let Some(fs) = self.get_next_fs(tb, &mut itr) {
            if self.flags & (FL_TREE | FL_SUBMOUNTS) != 0 {
                self.create_treenode(table, ids, tb, Some(fs), None, lines)?;
            } else {
                self.add_line(table, ids, tb, fs, None, lines).ok_or(())?;
            }
            nlines = nlines.saturating_add(1);
            if self.flags & FL_FIRSTONLY != 0 {
                break;
            }
            self.flags |= FL_NOSWAPMATCH;
        }
        if nlines > 0 { Ok(()) } else { Err(()) }
    }

    /// `parser_errcb`.
    fn parse_error(&mut self, filename: &[u8], line: usize) -> i32 {
        warnx(
            &self.short,
            &format!("{}: parse error at line {line} -- ignored", shown(filename)),
        );
        self.parse_nerrors = self.parse_nerrors.saturating_add(1);
        1
    }

    /// `parse_tabfiles(files, nfiles, tabtype)`: the tables, one after
    /// another into one; `None` after `can't read FILE`.
    fn parse_tabfiles(&mut self, files: &[Vec<u8>], tabtype: TabType) -> Option<MntTable> {
        let mut tb = MntTable::new();
        let mut list: Vec<Option<Vec<u8>>> = files.iter().cloned().map(Some).collect();
        if list.is_empty() {
            list.push(None);
        }
        for path in list {
            let mut errors: Vec<(Vec<u8>, usize)> = Vec::new();
            let mut cb = |f: &[u8], l: usize| {
                errors.push((f.to_vec(), l));
                1
            };
            // The path `warn` names: upstream's `%s` of a NULL is "(null)".
            let mut shown_path = path.clone();
            let rc = match tabtype {
                TabType::Fstab => tab_parse::parse_fstab(&mut tb, path.as_deref(), Some(&mut cb)),
                TabType::Mtab => tab_parse::parse_mtab(&mut tb, path.as_deref(), Some(&mut cb)),
                TabType::Kernel => {
                    let p = path.unwrap_or_else(|| {
                        if readable(tab_parse::PATH_PROC_MOUNTINFO) {
                            tab_parse::PATH_PROC_MOUNTINFO.to_vec()
                        } else {
                            tab_parse::PATH_PROC_MOUNTS.to_vec()
                        }
                    });
                    shown_path = Some(p.clone());
                    tab_parse::parse_file(&mut tb, &p, Some(&mut cb))
                }
            };
            // The callback's messages, in the order the parser met them.
            for (f, l) in errors {
                self.parse_error(&f, l);
            }
            if let Err(errno) = rc {
                let name = shown_path.map_or_else(|| "(null)".to_string(), |p| shown(&p));
                let errno = if errno == tab_parse::UNCHANGED {
                    self.errno_left
                } else {
                    errno
                };
                warn_errno(&self.short, &format!("can't read {name}"), errno);
                return None;
            }
        }
        Some(tb)
    }

    /// `cache_set_targets(cache)`: the kernel's table, for canonicalizing
    /// mount points of a table that is not the kernel's.
    fn cache_set_targets(&mut self) {
        let path = if readable(tab_parse::PATH_PROC_MOUNTINFO) {
            tab_parse::PATH_PROC_MOUNTINFO
        } else {
            tab_parse::PATH_PROC_MOUNTS
        };
        let mut tb = MntTable::new();
        if tab_parse::parse_file(&mut tb, path, None).is_ok()
            && let Some(c) = self.cache.as_mut()
        {
            c.mountinfo = Some(tb);
        }
    }
}

/// `access(path, R_OK) == 0`.
fn readable(path: &[u8]) -> bool {
    std::fs::File::open(quoting::os_from_bytes(path)).is_ok()
}

/// `warn(msg)` with `errno`: 0 is glibc's `Success`.
pub(crate) fn warn_errno(short: &[u8], msg: &str, errno: i32) {
    if errno == 0 {
        stderr_write(format!("{}: {msg}: Success\n", shown(short)).as_bytes());
    } else {
        warn(short, msg, &std::io::Error::from_raw_os_error(errno));
    }
}

/// `tab_is_tree(tb)`: the last entry came from the kernel and has a root.
fn tab_is_tree(tb: &MntTable) -> bool {
    tb.ents
        .last()
        .is_some_and(|fs| fs.is_kernel() && fs.root.is_some())
}

/// `tab_is_kernel(tb)`: every entry came from the kernel.
fn tab_is_kernel(tb: &MntTable) -> bool {
    tb.ents.iter().all(Fs::is_kernel)
}

/// `column_name_to_id(name, namesz)`: a column by its name, in any case.
/// Unknown, it is reported with the rest of the list after it, as
/// upstream's C string runs on to the list's end.
fn column_name_to_id(name: &[u8], rest: &[u8], short: &[u8]) -> Option<Col> {
    let found = INFOS
        .iter()
        .find(|i| i.name.as_bytes().eq_ignore_ascii_case(name))
        .map(|i| i.id);
    if found.is_none() {
        warnx(short, &format!("unknown column: {}", shown(rest)));
    }
    found
}

/// `poll_action_name_to_id(name, namesz)`.
fn poll_action_name_to_id(name: &[u8], rest: &[u8], short: &[u8]) -> Option<Change> {
    let found = [
        (&b"move"[..], Change::Move),
        (b"mount", Change::Mount),
        (b"umount", Change::Umount),
        (b"remount", Change::Remount),
    ]
    .into_iter()
    .find(|(n, _)| n.eq_ignore_ascii_case(name))
    .map(|(_, c)| c);
    if found.is_none() {
        warnx(short, &format!("unknown action: {}", shown(rest)));
    }
    found
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

stdfdguard::guard_std_fds!();

fn main() -> ExitCode {
    stdfdguard::restore();
    let argv: Vec<OsString> = std::env::args_os().collect();
    let short = short_name(
        argv.first()
            .map_or(OsStr::new("findmnt"), OsString::as_os_str),
    );
    let mut out = Stdout::new(1);
    let status = run(&argv, &short, &mut out);
    // `close_stdout`, which upstream registers with `atexit`.
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
        .map_or(OsStr::new("findmnt"), OsString::as_os_str);
    let mut f = Findmnt::new(short.to_vec());
    let mut tabfiles: Vec<Vec<u8>> = Vec::new();
    let mut direction = Direction::Forward;
    let mut verify = false;
    let mut timeout: i32 = -1;
    let mut tabtype: Option<TabType> = None;
    let mut outarg: Option<Vec<u8>> = None;
    let mut force_tree = false;
    let mut operands: Vec<Vec<u8>> = Vec::new();
    let mut excl_st = [0i32; 9];

    // The default output format.
    f.flags |= FL_TREE;

    let own = argv.get(1..).unwrap_or_default();
    for item in FINDMNT.parse(own, SHORTS, LONGS) {
        let opt = match item {
            Ok(opt) => opt,
            Err(e) => {
                // glibc names the program by argv[0] as given.
                stderr_write(format!("{}: {}\n", shown(&os_bytes(arg0)), e.sentence).as_bytes());
                return errtryhelp(short);
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
            return 1;
        }
        let arg = value.as_deref().map(|v| os_bytes(v).into_owned());
        match c {
            OPT_OUTPUT_ALL => {
                f.columns = INFOS
                    .iter()
                    .map(|i| i.id)
                    .filter(|&c| !is_tabdiff_column(c))
                    .collect();
            }
            OPT_VERBOSE => f.flags |= FL_VERBOSE,
            OPT_TREE => force_tree = true,
            OPT_PSEUDO => f.flags |= FL_PSEUDO,
            OPT_REAL => f.flags |= FL_REAL,
            OPT_VFS_ALL => f.flags |= FL_VFS_ALL,
            OPT_SHADOWED => f.flags |= FL_SHADOWED,
            _ => match u8::try_from(c).unwrap_or(0) {
                b'A' => f.flags |= FL_ALL,
                b'a' => f.flags |= FL_ASCII,
                b'b' => f.flags |= FL_BYTES,
                b'C' => f.flags |= FL_NOCACHE,
                b'c' => f.flags |= FL_CANONICALIZE,
                b'D' => {
                    f.flags &= !FL_TREE;
                    f.flags |= FL_DF;
                }
                b'd' => {
                    direction = match arg.as_deref() {
                        Some(b"forward") => Direction::Forward,
                        Some(b"backward") => Direction::Backward,
                        _ => {
                            warnx(
                                short,
                                &format!(
                                    "unknown direction {}",
                                    escaped_in_quotes(&arg.unwrap_or_default())
                                ),
                            );
                            return 1;
                        }
                    };
                }
                b'e' => f.flags |= FL_EVALUATE,
                b'i' => f.flags |= FL_INVERT,
                b'J' => f.flags |= FL_JSON,
                b'f' => f.flags |= FL_FIRSTONLY,
                b'F' => tabfiles.push(arg.unwrap_or_default()),
                b'u' => f.disable_columns_truncate(),
                b'o' => outarg = arg,
                b'O' => f.matches.options = arg,
                b'p' => {
                    if let Some(list) = arg {
                        f.actions.clear();
                        let parsed: Result<usize, IdListError> =
                            string_to_idarray(&list, &mut f.actions, NACTIONS, |name, rest| {
                                poll_action_name_to_id(name, rest, short)
                            });
                        if parsed.is_err() {
                            return 1;
                        }
                    }
                    f.flags |= FL_POLL;
                    f.flags &= !FL_TREE;
                }
                b'P' => {
                    f.flags |= FL_EXPORT;
                    f.flags &= !FL_TREE;
                }
                b'm' => {
                    tabtype = Some(TabType::Mtab);
                    f.flags &= !FL_TREE;
                }
                b's' => {
                    tabtype = Some(TabType::Fstab);
                    f.flags &= !FL_TREE;
                }
                b'k' => tabtype = Some(TabType::Kernel),
                b't' => f.matches.fstype = arg,
                b'r' => {
                    f.flags &= !FL_TREE;
                    f.flags |= FL_RAW;
                }
                b'l' => f.flags &= !FL_TREE,
                b'n' => f.flags |= FL_NOHEADINGS,
                b'N' => {
                    tabtype = Some(TabType::Kernel);
                    let value = value.unwrap_or_default();
                    let tid = match ulstrutils::ul_strtou64(&os_bytes(&value), 10) {
                        Ok(v) if u32::try_from(v).is_ok() => v,
                        Ok(_) => {
                            warnx(
                                short,
                                &num_error_message("invalid TID argument", &value, NumErr::Range),
                            );
                            return 1;
                        }
                        Err(e) => {
                            warnx(short, &num_error_message("invalid TID argument", &value, e));
                            return 1;
                        }
                    };
                    // `"/proc/%d/mountinfo"` of a `pid_t`.
                    tabfiles.push(format!("/proc/{}/mountinfo", tid as u32 as i32).into_bytes());
                }
                b'v' => f.flags |= FL_NOFSROOT,
                b'R' => f.flags |= FL_SUBMOUNTS,
                b'S' => {
                    f.set_source_match(&arg.unwrap_or_default());
                    f.flags |= FL_NOSWAPMATCH;
                }
                b'M' | b'T' => {
                    if c == i32::from(b'M') {
                        f.flags |= FL_STRICTTARGET;
                    }
                    f.matches.target = arg;
                    f.flags |= FL_NOSWAPMATCH;
                }
                b'U' => f.flags |= FL_UNIQ,
                b'w' => {
                    let value = value.unwrap_or_default();
                    match ulstrutils::ul_strtos32(&os_bytes(&value), 10) {
                        Ok(v) => timeout = v,
                        Err(e) => {
                            warnx(
                                short,
                                &num_error_message("invalid timeout argument", &value, e),
                            );
                            return 1;
                        }
                    }
                }
                b'x' => verify = true,
                b'y' => f.flags |= FL_SHELLVAR,
                b'h' => {
                    out.write(&usage(short));
                    return 0;
                }
                b'V' => {
                    let mut line = short.to_vec();
                    line.extend_from_slice(b" from util-linux 2.39.3\n");
                    out.write(&line);
                    return 0;
                }
                _ => return errtryhelp(short),
            },
        }
    }

    if f.columns.is_empty() && f.flags & FL_DF != 0 {
        f.columns = vec![
            Col::Source,
            Col::Fstype,
            Col::Size,
            Col::Used,
            Col::Avail,
            Col::Useperc,
            Col::Target,
        ];
    }
    // Default columns.
    if f.columns.is_empty() {
        if f.flags & FL_POLL != 0 {
            f.columns.push(Col::Action);
        }
        f.columns
            .extend([Col::Target, Col::Source, Col::Fstype, Col::Options]);
    }
    if let Some(list) = &outarg
        && string_add_to_idarray(list, &mut f.columns, MAX_COLUMNS, |name, rest| {
            column_name_to_id(name, rest, short)
        })
        .is_err()
    {
        return 1;
    }
    let tabtype = tabtype.unwrap_or(if verify {
        TabType::Fstab
    } else {
        TabType::Kernel
    });
    if f.flags & FL_POLL != 0 && tabfiles.len() > 1 {
        warnx(
            short,
            "--poll accepts only one file, but more specified by --tab-file",
        );
        return 1;
    }
    if !operands.is_empty() && (f.matches.source.is_some() || f.matches.target.is_some()) {
        warnx(
            short,
            "options --target and --source can't be used together with command line element that is not an option",
        );
        return 1;
    }
    let mut operands = operands.into_iter();
    if let Some(o) = operands.next() {
        // dev/tag/mountpoint/maj:min
        f.set_source_match(&o);
    }
    if let Some(o) = operands.next() {
        f.matches.target = Some(o);
    }
    if f.flags & FL_SUBMOUNTS != 0 && f.is_listall_mode() {
        // Don't care about submounts if list all mounts.
        f.flags &= !FL_SUBMOUNTS;
    }
    if f.flags & FL_SUBMOUNTS == 0
        && (f.flags & FL_FIRSTONLY != 0
            || f.matches.target.is_some()
            || f.matches.source.is_some()
            || f.matches.majmin.is_some())
    {
        f.flags &= !FL_TREE;
    }
    if f.flags & FL_NOSWAPMATCH == 0
        && f.matches.target.is_none()
        && let Some(x) = &f.matches.source
    {
        // A LABEL=/UUID= source cannot be swapped for a target.
        if [&b"LABEL="[..], b"UUID=", b"PARTLABEL=", b"PARTUUID="]
            .iter()
            .any(|p| x.starts_with(p))
        {
            f.flags |= FL_NOSWAPMATCH;
        }
    }

    let Some(mut tb) = f.parse_tabfiles(&tabfiles, tabtype) else {
        return 1;
    };
    let tabtype = if tabtype == TabType::Mtab && tab_is_kernel(&tb) {
        TabType::Kernel
    } else {
        tabtype
    };
    let istree = tab_is_tree(&tb);
    if istree && force_tree {
        f.flags |= FL_TREE;
    }
    if f.flags & FL_TREE != 0 && (tabfiles.len() > 1 || !istree) {
        f.flags &= !FL_TREE;
    }
    if f.flags & FL_NOCACHE == 0 {
        f.cache = Some(Cache::new());
        if tabtype != TabType::Kernel {
            f.cache_set_targets();
        }
    }
    if f.flags & FL_UNIQ != 0 {
        let cache = &mut f.cache;
        tb.uniq_fs(Direction::Backward, true, |a, b| {
            b.target
                .as_deref()
                .is_some_and(|t| fs_match_target(a, t, cache.as_mut()))
        });
    }
    if verify {
        let rc = verify::verify_table(&mut f, &tb, out);
        return u8::from(rc != 0);
    }

    // The output table.
    let mut table = Table::new();
    table.enable_raw(f.flags & FL_RAW != 0);
    table.enable_export(f.flags & FL_EXPORT != 0);
    table.enable_shellvar(f.flags & FL_SHELLVAR != 0);
    table.enable_json(f.flags & FL_JSON != 0);
    table.enable_ascii(f.flags & FL_ASCII != 0);
    table.enable_noheadings(f.flags & FL_NOHEADINGS != 0);
    if f.flags & FL_JSON != 0 {
        table.set_name(b"filesystems");
    }
    let mut ids: Vec<ColumnId> = Vec::with_capacity(f.columns.len());
    for &col in &f.columns.clone() {
        let mut fl = f.col_flag(col);
        if f.flags & FL_TREE == 0 {
            fl &= !SCOLS_FL_TREE;
        }
        if f.flags & FL_POLL == 0 && is_tabdiff_column(col) {
            warnx(
                short,
                &format!(
                    "{} column is requested, but --poll is not enabled",
                    info(col).name
                ),
            );
            return 1;
        }
        let cl = table.new_column(info(col).name.as_bytes(), info(col).whint, fl);
        if fl & SCOLS_FL_WRAP != 0 {
            // Multi-line cells (SOURCES).
            let _ = table.column_set_wrapnl(cl);
            let _ = table.column_set_safechars(cl, b"\n");
        }
        if f.flags & FL_JSON != 0 {
            let ty = match col {
                Col::Size | Col::Avail | Col::Used if f.flags & FL_BYTES != 0 => JsonType::Number,
                Col::Id | Col::Parent | Col::Freq | Col::Passno | Col::Tid => JsonType::Number,
                _ if fl & SCOLS_FL_WRAP != 0 => JsonType::ArrayString,
                _ => JsonType::String,
            };
            let _ = table.column_set_json_type(cl, ty);
        }
        ids.push(cl);
    }

    let mut lines: Vec<usize> = Vec::new();
    let rc = if f.flags & FL_POLL != 0 {
        // Poll mode: the first tab file only.
        let tabfile = tabfiles
            .first()
            .cloned()
            .unwrap_or_else(|| tab_parse::PATH_PROC_MOUNTINFO.to_vec());
        poll_table(
            &mut f, tb, &tabfile, timeout, &mut table, &ids, direction, out,
        )
    } else if f.flags & FL_TREE != 0 && f.flags & FL_SUBMOUNTS == 0 {
        // The whole tree.
        f.create_treenode(&mut table, &ids, &tb, None, None, &mut lines)
    } else {
        // The whole list, or subtrees.
        let mut rc = f.add_matching_lines(&tb, &mut table, &ids, direction, &mut lines);
        if rc.is_err()
            && tabtype == TabType::Kernel
            && f.flags & FL_NOSWAPMATCH != 0
            && f.flags & FL_STRICTTARGET == 0
            && f.matches.target.is_some()
        {
            // Found nothing: maybe --target is a regular file; try again
            // with the mount point it is on.
            f.enable_extra_target_match(&tb);
            rc = f.add_matching_lines(&tb, &mut table, &ids, direction, &mut lines);
        }
        rc
    };
    if rc.is_ok() && f.flags & FL_POLL == 0 {
        let mut text = Vec::new();
        // `scols_print_table`'s status is not looked at, as upstream does
        // not look at it: what it printed before any failure is the output.
        let _ = table.print_into(&mut text);
        out.write(&text);
    }
    // `mnt_unref_cache`: libblkid's cache, if a tag was scanned for, is
    // written back. The SOURCES cache upstream never puts.
    if let Some(bc) = f.blk_cache.take() {
        bc.abandon();
    }
    drop(f.cache.take());
    u8::from(rc.is_err())
}

/// `poll_table(tb, tabfile, timeout, table, direction)`: each change to the
/// table, as it happens, until the timeout (or the first, with
/// `--first-only`).
#[allow(
    clippy::too_many_arguments,
    reason = "upstream's function and its globals"
)]
fn poll_table(
    f: &mut Findmnt,
    tb: MntTable,
    tabfile: &[u8],
    timeout: i32,
    table: &mut Table,
    ids: &[ColumnId],
    direction: Direction,
    out: &mut Stdout,
) -> Result<(), ()> {
    let mut tb = tb;
    let mut tb_new = MntTable::new();
    let file = match std::fs::File::open(quoting::os_from_bytes(tabfile)) {
        Ok(file) => file,
        Err(e) => {
            warn(&f.short, &format!("cannot open {}", shown(tabfile)), &e);
            return Err(());
        }
    };
    loop {
        let count = match sys::poll_pri(&file, timeout) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) => {
                warn(&f.short, "poll() failed", &e);
                return Err(());
            }
        };
        let _ = count;
        // `rewind(f)`, then the table read again.
        let mut errors: Vec<(Vec<u8>, usize)> = Vec::new();
        let rc = {
            let mut cb = |name: &[u8], l: usize| {
                errors.push((name.to_vec(), l));
                1
            };
            tab_parse::parse_stream_from_start(&mut tb_new, &file, tabfile, Some(&mut cb))
        };
        for (name, l) in errors {
            f.parse_error(&name, l);
        }
        if rc.is_err() {
            return Err(());
        }
        let mut changes = tab_diff::diff_tables(&tb, &tb_new);
        if direction == Direction::Backward {
            changes.reverse();
        }
        let mut count = 0usize;
        for ch in changes {
            if !f.has_poll_action(ch.oper) {
                continue;
            }
            let matched = match (ch.new, ch.old) {
                (Some(n), _) => f.poll_match(&tb_new, n),
                (None, Some(o)) => f.poll_match(&tb, o),
                (None, None) => false,
            };
            if !matched {
                continue;
            }
            count = count.saturating_add(1);
            let old_fs = ch.old.and_then(|o| tb.ents.get(o)).cloned();
            let new_fs = ch.new.and_then(|n| tb_new.ents.get(n)).cloned();
            let Ok(line) = table.new_line(None) else {
                return Err(());
            };
            for (n, &col) in f.columns.clone().iter().enumerate() {
                if let Some(data) =
                    f.get_tabdiff_data(old_fs.as_ref(), new_fs.as_ref(), ch.oper, col)
                    && let Some(&id) = ids.get(n)
                {
                    let _ = table.line_set_data(line, id, &data);
                }
            }
            if f.flags & FL_FIRSTONLY != 0 {
                break;
            }
        }
        if count > 0 {
            let mut text = Vec::new();
            let printed = table.print_range_into(&mut text);
            if printed.is_ok() {
                text.push(b'\n');
            }
            out.write(&text);
            out.flush();
            if printed.is_err() {
                return Err(());
            }
        }
        // Swap the tables, and forget what was printed.
        std::mem::swap(&mut tb, &mut tb_new);
        table.remove_lines();
        tb_new.reset();
        if count > 0 && f.flags & FL_FIRSTONLY != 0 {
            break;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
