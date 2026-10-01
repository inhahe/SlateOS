//! mountpoint -- see if a directory or file is a mount point.
//!
//! A port of util-linux 2.39.3's `sys-utils/mountpoint.c`, with upstream's
//! names, reading the kernel's mount table through `ulmount` (the port of
//! libmount's table code) as upstream reads it through libmount; measured
//! against `mountpoint from util-linux 2.39.3` by
//! `scripts/mountpoint-diff.sh`.
//!
//! It replaces the `mountpoint` personality the hand-written `findmnt`
//! carried, which compared the path with each mount point as text -- so
//! `/proc/` and a symlink to a mount point were not mount points, and `-d`
//! printed the mount's `MAJ:MIN` field rather than the device number.
//!
//! Upstream's behaviour is kept where it shows: the path is looked up in
//! `/proc/self/mountinfo` as libmount looks up a target -- as given, made
//! absolute, then canonicalized -- so a bind mount of a file counts; without
//! `/proc`, a directory is a mount point when its parent is on another
//! device or is itself (which a bind mount is not); `--nofollow` makes a
//! symlink never a mount point; and exit status 32 means "not a mount
//! point".
//!
//! # What is not upstream's
//!
//! * **A name in a diagnostic** has its unprintable bytes escaped
//!   (design-decisions §370).

use getoptlong::{Opt, Program, Takes};
use quoting::{escape_unprintable, os_bytes, os_from_bytes};
use std::ffi::{OsStr, OsString};
use std::process::ExitCode;
use ulclosestream::{Stdout, stderr_write, warn, warnx};
use ulmount::cache::Cache;
use ulmount::fs::{major, minor};
use ulmount::tab::{Direction, Fmt, Table};

/// `MOUNTPOINT_EXIT_NOMNT`.
const EXIT_NOMNT: u8 = 32;

/// Getopt's errors are only sentences here; the referral follows them.
const MOUNTPOINT: Program = Program::new("mountpoint", 1);

/// Upstream's option string.
const SHORTS: &str = "qdxhV";

/// `OPT_NOFOLLOW`: `CHAR_MAX + 1`.
const OPT_NOFOLLOW: i32 = 128;

/// Upstream's `longopts[]`, in its order, and each one's `val`.
const LONGS: &[(&str, Takes)] = &[
    ("quiet", Takes::Nothing),
    ("nofollow", Takes::Nothing),
    ("fs-devno", Takes::Nothing),
    ("devno", Takes::Nothing),
    ("help", Takes::Nothing),
    ("version", Takes::Nothing),
];
const LONG_VALS: [i32; 6] = [
    b'q' as i32,
    OPT_NOFOLLOW,
    b'd' as i32,
    b'x' as i32,
    b'h' as i32,
    b'V' as i32,
];

/// What `stat` or `lstat` said about the path.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Stat {
    dev: u64,
    ino: u64,
    rdev: u64,
    is_block: bool,
    is_link: bool,
}

/// `struct mountpoint_control`.
#[derive(Debug, Default)]
struct Control {
    path: Vec<u8>,
    dev: u64,
    st: Stat,
    dev_devno: bool,
    fs_devno: bool,
    nofollow: bool,
    quiet: bool,
}

/// `stat(path)` (`follow`) or `lstat(path)`.
fn stat(path: &[u8], follow: bool) -> std::io::Result<Stat> {
    let p = os_from_bytes(path);
    let m = if follow {
        std::fs::metadata(&p)?
    } else {
        std::fs::symlink_metadata(&p)?
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::{FileTypeExt, MetadataExt};
        Ok(Stat {
            dev: m.dev(),
            ino: m.ino(),
            rdev: m.rdev(),
            is_block: m.file_type().is_block_device(),
            is_link: m.file_type().is_symlink(),
        })
    }
    #[cfg(not(unix))]
    {
        Ok(Stat {
            is_link: m.file_type().is_symlink(),
            ..Stat::default()
        })
    }
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
fn errtryhelp(short: &[u8]) -> u8 {
    stderr_write(format!("Try '{} --help' for more information.\n", shown(short)).as_bytes());
    1
}

/// `usage()`.
fn usage(short: &[u8]) -> Vec<u8> {
    let s = shown(short);
    let mut t = String::from("\nUsage:\n");
    t.push_str(&format!(
        " {s} [-qd] /path/to/directory\n {s} -x /dev/device\n"
    ));
    t.push_str("\nCheck whether a directory or file is a mountpoint.\n");
    t.push_str("\nOptions:\n");
    t.push_str(" -q, --quiet        quiet mode - don't print anything\n");
    t.push_str("     --nofollow     do not follow symlink\n");
    t.push_str(" -d, --fs-devno     print maj:min device number of the filesystem\n");
    t.push_str(" -x, --devno        print maj:min device number of the block device\n");
    t.push('\n');
    t.push_str(&format!(
        "{:<20}{}\n{:<20}{}\n",
        " -h, --help", "display this help", " -V, --version", "display version"
    ));
    t.push_str("\nFor more details see mountpoint(1).\n");
    t.into_bytes()
}

/// `dir_to_device(ctl)`: the device number of the filesystem mounted at the
/// path, from the kernel's table; without it (no `/proc`), from comparing
/// the path with its parent.
fn dir_to_device(ctl: &mut Control) -> Result<(), ()> {
    // `mnt_new_table_from_file(_PATH_PROC_MOUNTINFO)`: the kind guessed, a
    // bad line skipped in silence.
    let mut tb = Table {
        fmt: Fmt::Guess,
        ents: Vec::new(),
    };
    let path = ulmount::tab_parse::PATH_PROC_MOUNTINFO;
    let parsed = std::fs::metadata(os_from_bytes(path)).is_ok()
        && ulmount::tab_parse::parse_file(&mut tb, path, None).is_ok();
    if !parsed {
        // The traditional way, independent of /proc but blind to bind
        // mounts: a directory whose parent is on another device, or is the
        // directory itself.
        let cn = ulmount::cache::canonicalize_path(&ctl.path);
        let mut buf = cn.unwrap_or_else(|| ctl.path.clone());
        buf.extend_from_slice(b"/..");
        if buf.len() >= 4096 {
            return Err(());
        }
        let pst = stat(&buf, true).map_err(|_| ())?;
        if ctl.st.dev != pst.dev || ctl.st.ino == pst.ino {
            ctl.dev = ctl.st.dev;
            return Ok(());
        }
        return Err(());
    }
    // A cache, to canonicalize every path needed.
    let mut cache = Cache::new();
    match tb
        .find_target(&ctl.path, Direction::Backward, Some(&mut cache))
        .and_then(|i| tb.ents.get(i))
    {
        Some(fs) if fs.target.is_some() => {
            ctl.dev = fs.devno;
            Ok(())
        }
        _ => Err(()),
    }
}

/// `print_devno(ctl)`: the block device's own number.
fn print_devno(ctl: &Control, short: &[u8], out: &mut Stdout) -> Result<(), ()> {
    if !ctl.st.is_block {
        if !ctl.quiet {
            warnx(short, &format!("{}: not a block device", shown(&ctl.path)));
        }
        return Err(());
    }
    out.write(format!("{}:{}\n", major(ctl.st.rdev), minor(ctl.st.rdev)).as_bytes());
    Ok(())
}

/// The option each parsed item stands for, as upstream's switch sees it.
fn option_code(opt: &Opt<'_>) -> Option<i32> {
    match opt {
        Opt::Short(c, _) => Some(i32::from(*c)),
        Opt::Long(name, _) => {
            let i = LONGS.iter().position(|&(n, _)| n == *name)?;
            LONG_VALS.get(i).copied()
        }
        Opt::Operand(_) => None,
    }
}

stdfdguard::guard_std_fds!();

fn main() -> ExitCode {
    stdfdguard::restore();
    let argv: Vec<OsString> = std::env::args_os().collect();
    let short = short_name(
        argv.first()
            .map_or(OsStr::new("mountpoint"), OsString::as_os_str),
    );
    let mut out = Stdout::new(1);
    let status = run(&argv, &short, &mut out);
    // `close_stdout`, which upstream registers with `atexit`.
    ExitCode::from(out.close(status, &short))
}

/// `main()`.
fn run(argv: &[OsString], short: &[u8], out: &mut Stdout) -> u8 {
    let arg0 = argv
        .first()
        .map_or(OsStr::new("mountpoint"), OsString::as_os_str);
    let mut ctl = Control::default();
    let mut operands: Vec<Vec<u8>> = Vec::new();
    let own = argv.get(1..).unwrap_or_default();
    for item in MOUNTPOINT.parse(own, SHORTS, LONGS) {
        let opt = match item {
            Ok(opt) => opt,
            Err(e) => {
                // glibc names the program by argv[0] as given.
                stderr_write(format!("{}: {}\n", shown(&os_bytes(arg0)), e.sentence).as_bytes());
                return errtryhelp(short);
            }
        };
        let Some(c) = option_code(&opt) else {
            if let Opt::Operand(o) = &opt {
                operands.push(os_bytes(o).into_owned());
            }
            continue;
        };
        match c {
            OPT_NOFOLLOW => ctl.nofollow = true,
            _ => match u8::try_from(c).unwrap_or(0) {
                b'q' => ctl.quiet = true,
                b'd' => ctl.fs_devno = true,
                b'x' => ctl.dev_devno = true,
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
    let path = match operands.as_slice() {
        [p] => p.clone(),
        _ => {
            warnx(short, "bad usage");
            return errtryhelp(short);
        }
    };
    if ctl.nofollow && ctl.dev_devno {
        warnx(short, "--devno and --nofollow are mutually exclusive");
        return 1;
    }
    ctl.path = path;
    match stat(&ctl.path, !ctl.nofollow) {
        Ok(st) => ctl.st = st,
        Err(e) => {
            if !ctl.quiet {
                warn(short, &shown(&ctl.path), &e);
            }
            return 1;
        }
    }
    if ctl.dev_devno {
        return if print_devno(&ctl, short, out).is_ok() {
            0
        } else {
            EXIT_NOMNT
        };
    }
    if (ctl.nofollow && ctl.st.is_link) || dir_to_device(&mut ctl).is_err() {
        if !ctl.quiet {
            let mut line = ctl.path.clone();
            line.extend_from_slice(b" is not a mountpoint\n");
            out.write(&line);
        }
        return EXIT_NOMNT;
    }
    if ctl.fs_devno {
        out.write(format!("{}:{}\n", major(ctl.dev), minor(ctl.dev)).as_bytes());
    } else if !ctl.quiet {
        let mut line = ctl.path.clone();
        line.extend_from_slice(b" is a mountpoint\n");
        out.write(&line);
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn help_is_upstreams() {
        let text = String::from_utf8(usage(b"mountpoint")).unwrap_or_default();
        assert!(text.starts_with(
            "\nUsage:\n mountpoint [-qd] /path/to/directory\n mountpoint -x /dev/device\n"
        ));
        // `USAGE_HELP_OPTIONS(20)`: `%-20s`.
        assert!(text.contains(" -h, --help         display this help\n"));
        assert!(text.ends_with("\nFor more details see mountpoint(1).\n"));
    }

    #[test]
    fn long_options_map_to_upstreams_values() {
        assert_eq!(
            option_code(&Opt::Long("nofollow", None)),
            Some(OPT_NOFOLLOW)
        );
        assert_eq!(
            option_code(&Opt::Long("fs-devno", None)),
            Some(i32::from(b'd'))
        );
        assert_eq!(
            option_code(&Opt::Long("devno", None)),
            Some(i32::from(b'x'))
        );
    }
}
