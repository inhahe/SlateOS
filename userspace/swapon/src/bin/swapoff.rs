//! swapoff -- disable devices and files for paging and swapping.
//!
//! A port of util-linux 2.39.3's `sys-utils/swapoff.c`, function by
//! function and with upstream's names, on `ulmount` (libmount: `/proc/swaps`,
//! fstab, `LABEL=`/`UUID=` resolution) and `ulblkid` (libblkid: a swap file's
//! label and UUID, which the device resolution cannot see). What it shares
//! with `swapon` is in the crate's library (swapon-common.c). Measured
//! against `swapoff from util-linux 2.39.3` by `scripts/swapon-diff.sh`.
//!
//! # What is not upstream's
//!
//! * **A name in a diagnostic** has its unprintable bytes escaped
//!   (design-decisions §370).

use std::ffi::{OsStr, OsString};
use std::process::ExitCode;

use getoptlong::{Opt, Program, Takes};
use swapon::{
    Common, Stdout, errtryhelp, get_swap_prober, match_swap, probe_value, short_name, shown,
    stderr_write, warn_errno, warnx,
};
use ulmount::tab::{Direction, Iter};

/// Getopt's errors are only sentences here; the referral follows them.
const SWAPOFF: Program = Program::new("swapoff", 1);

/// Upstream's option string.
const SHORTS: &str = "ahvVL:U:";

/// Upstream's `long_opts[]`, in its order; each is its short option.
const LONGS: &[(&str, Takes)] = &[
    ("all", Takes::Nothing),
    ("help", Takes::Nothing),
    ("verbose", Takes::Nothing),
    ("version", Takes::Nothing),
];
const LONG_VALS: [u8; 4] = *b"ahvV";

/// `SWAPOFF_EX_*`: the exit status bits.
const EX_OK: i32 = 0;
const EX_ENOMEM: i32 = 2;
const EX_FAILURE: i32 = 4;
// `SWAPOFF_EX_SYSERR` (8) is for a libmount iterator that could not be
// made, which cannot fail here.
const EX_USAGE: u8 = 16;
const EX_ALLERR: i32 = 32;
const EX_SOMEOK: i32 = 64;

/// `EPERM`, `ENOMEM`.
const EPERM: i32 = 1;
const ENOMEM: i32 = 12;

/// A fatal error: what `errx` printed, and the status to exit with.
struct Fatal(u8);

/// swapoff.c's statics.
struct Ctl {
    verbose: bool,
    all: bool,
}

/// `swapoff_resolve_tag(name, value, cache)`: as `mnt_resolve_tag`, which
/// finds block devices, and then among the swap *files* in `/proc/swaps`
/// by probing each.
fn swapoff_resolve_tag(common: &mut Common, name: &[u8], value: &[u8]) -> Option<Vec<u8>> {
    if let Some(path) = common.cache.resolve_tag(name, value) {
        return Some(path);
    }
    let files: Vec<Vec<u8>> = {
        let tb = common.get_swaps()?;
        let mut itr = Iter::new(Direction::Backward);
        let mut v = Vec::new();
        while let Some(i) = tb.next_fs(&mut itr) {
            let Some(fs) = tb.ents.get(i) else {
                continue;
            };
            if let (Some(src), Some(ty)) = (&fs.source, &fs.swaptype)
                && ty == b"file"
            {
                v.push(src.clone());
            }
        }
        v
    };
    let key = std::str::from_utf8(name).unwrap_or("");
    for src in files {
        let Some(pr) = get_swap_prober(&common.short, &src) else {
            continue;
        };
        if probe_value(&pr, key).as_deref() == Some(value) {
            return Some(src);
        }
    }
    None
}

/// `do_swapoff(orig_special, quiet, canonic)`: `SWAPOFF_EX_*`, or -1 for a
/// device that cannot be found; a caller that is not root stops the
/// program.
fn do_swapoff(
    ctl: &Ctl,
    common: &mut Common,
    orig_special: &[u8],
    quiet: bool,
    canonic: bool,
    out: &mut Stdout,
) -> Result<i32, Fatal> {
    if ctl.verbose {
        let mut line = b"swapoff ".to_vec();
        line.extend_from_slice(orig_special);
        line.push(b'\n');
        out.write(&line);
    }
    let special = if canonic {
        orig_special.to_vec()
    } else {
        let resolved = common.cache.resolve_spec(orig_special).or_else(|| {
            let (n, v) = ulblkid::parse_tag_string(orig_special)?;
            swapoff_resolve_tag(common, &n, &v)
        });
        match resolved {
            Some(s) => s,
            None => return Ok(common.cannot_find(orig_special)),
        }
    };
    let Ok(cpath) = std::ffi::CString::new(special) else {
        return Ok(common.cannot_find(orig_special));
    };
    match libcall::swapoff(&cpath) {
        Ok(()) => Ok(EX_OK),
        Err(EPERM) => {
            warnx(&common.short, "Not superuser.");
            Err(Fatal(EX_USAGE))
        }
        Err(ENOMEM) => {
            warn_errno(
                &common.short,
                &format!("{}: swapoff failed", shown(orig_special)),
                ENOMEM,
            );
            Ok(EX_ENOMEM)
        }
        Err(e) => {
            if !quiet {
                warn_errno(
                    &common.short,
                    &format!("{}: swapoff failed", shown(orig_special)),
                    e,
                );
            }
            Ok(EX_FAILURE)
        }
    }
}

/// `swapoff_by(name, value, quiet)`.
fn swapoff_by(
    ctl: &Ctl,
    common: &mut Common,
    name: &[u8],
    value: &[u8],
    quiet: bool,
    out: &mut Stdout,
) -> Result<i32, Fatal> {
    match swapoff_resolve_tag(common, name, value) {
        Some(special) => do_swapoff(ctl, common, &special, quiet, true, out),
        None => Ok(common.cannot_find(value)),
    }
}

/// `usage()`.
fn usage(short: &[u8]) -> Vec<u8> {
    let mut t = String::from("\nUsage:\n");
    t.push_str(&format!(" {} [options] [<spec>]\n", shown(short)));
    t.push('\n');
    t.push_str("Disable devices and files for paging and swapping.\n");
    t.push_str("\nOptions:\n");
    t.push_str(" -a, --all              disable all swaps from /proc/swaps\n");
    t.push_str(" -v, --verbose          verbose mode\n");
    t.push('\n');
    t.push_str(&format!(
        "{:<24}{}\n{:<24}{}\n",
        " -h, --help", "display this help", " -V, --version", "display version"
    ));
    t.push_str(
        "\nThe <spec> parameter:\n -L <label>             LABEL of device to be used\n -U <uuid>              UUID of device to be used\n LABEL=<label>          LABEL of device to be used\n UUID=<uuid>            UUID of device to be used\n <device>               name of device to be used\n <file>                 name of file to be used\n",
    );
    t.push_str("\nFor more details see swapoff(8).\n");
    t.into_bytes()
}

/// `swapoff_all()`: everything in `/proc/swaps` (quietly), then the swap
/// areas in fstab that are not active (so that `swapoff -a` twice says
/// nothing the second time).
fn swapoff_all(ctl: &Ctl, common: &mut Common, out: &mut Stdout) -> Result<i32, Fatal> {
    let (mut nerrs, mut nsucc) = (0usize, 0usize);
    let active: Vec<Vec<u8>> = match common.get_swaps() {
        Some(tb) => {
            let mut itr = Iter::new(Direction::Backward);
            let mut v = Vec::new();
            while let Some(i) = tb.find_next_fs(&mut itr, match_swap) {
                if let Some(src) = tb.ents.get(i).and_then(|f| f.source.clone()) {
                    v.push(src);
                }
            }
            v
        }
        None => Vec::new(),
    };
    for src in &active {
        if do_swapoff(ctl, common, src, true, true, out)? == EX_OK {
            nsucc = nsucc.saturating_add(1);
        } else {
            nerrs = nerrs.saturating_add(1);
        }
    }
    let fstab: Vec<Vec<u8>> = match common.get_fstab(None) {
        Some(tb) => {
            let mut itr = Iter::new(Direction::Forward);
            let mut v = Vec::new();
            while let Some(i) = tb.find_next_fs(&mut itr, match_swap) {
                if let Some(src) = tb.ents.get(i).and_then(|f| f.source.clone()) {
                    v.push(src);
                }
            }
            v
        }
        None => Vec::new(),
    };
    for src in &fstab {
        if !common.is_active_swap(src) {
            // Its answer is not looked at: probably unmounted already.
            do_swapoff(ctl, common, src, true, false, out)?;
        }
    }
    Ok(if nerrs == 0 {
        EX_OK
    } else if nsucc == 0 {
        EX_ALLERR
    } else {
        EX_SOMEOK
    })
}

/// The option each parsed item stands for, as upstream's switch sees it.
fn option_code(opt: &Opt<'_>) -> Option<(u8, Option<OsString>)> {
    match opt {
        Opt::Short(c, value) => Some((*c, value.clone())),
        Opt::Long(name, value) => {
            let i = LONGS.iter().position(|&(n, _)| n == *name)?;
            Some((*LONG_VALS.get(i)?, value.clone()))
        }
        Opt::Operand(_) => None,
    }
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
            .map_or(OsStr::new("swapoff"), OsString::as_os_str),
    );
    let mut out = Stdout::new(1);
    let status = match run(&argv, &short, &mut out) {
        Ok(s) | Err(Fatal(s)) => s,
    };
    ExitCode::from(out.close(status, &short))
}

/// `main()`.
fn run(argv: &[OsString], short: &[u8], out: &mut Stdout) -> Result<u8, Fatal> {
    let arg0 = argv
        .first()
        .map_or(OsStr::new("swapoff"), OsString::as_os_str);
    let mut ctl = Ctl {
        verbose: false,
        all: false,
    };
    let mut common = Common::new(short.to_vec());
    let mut operands: Vec<Vec<u8>> = Vec::new();

    let own = argv.get(1..).unwrap_or_default();
    for item in SWAPOFF.parse(own, SHORTS, LONGS) {
        let opt = match item {
            Ok(opt) => opt,
            Err(e) => {
                stderr_write(
                    format!("{}: {}\n", shown(&quoting::os_bytes(arg0)), e.sentence).as_bytes(),
                );
                return Err(Fatal(errtryhelp(short, EX_USAGE)));
            }
        };
        let Some((c, value)) = option_code(&opt) else {
            if let Opt::Operand(o) = &opt {
                operands.push(quoting::os_bytes(o).into_owned());
            }
            continue;
        };
        let arg = value.as_deref().map(|v| quoting::os_bytes(v).into_owned());
        match c {
            b'a' => ctl.all = true,
            b'v' => ctl.verbose = true,
            b'L' => common.labels.push(arg.unwrap_or_default()),
            b'U' => common.uuids.push(arg.unwrap_or_default()),
            b'h' => {
                out.write(&usage(short));
                return Ok(0);
            }
            b'V' => {
                out.write(format!("{} from util-linux 2.39.3\n", shown(short)).as_bytes());
                return Ok(0);
            }
            _ => return Err(Fatal(errtryhelp(short, EX_USAGE))),
        }
    }

    if !ctl.all && common.labels.is_empty() && common.uuids.is_empty() && operands.is_empty() {
        warnx(short, "bad usage");
        return Err(Fatal(errtryhelp(short, EX_USAGE)));
    }

    let mut status = 0i32;
    for label in common.labels.clone() {
        status |= swapoff_by(&ctl, &mut common, b"LABEL", &label, false, out)?;
    }
    for uuid in common.uuids.clone() {
        status |= swapoff_by(&ctl, &mut common, b"UUID", &uuid, false, out)?;
    }
    for spec in &operands {
        status |= do_swapoff(&ctl, &mut common, spec, false, false, out)?;
    }
    if ctl.all {
        status |= swapoff_all(&ctl, &mut common, out)?;
    }
    Ok(exit_byte(status))
}
