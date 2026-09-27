//! findfs -- find a filesystem by label or UUID.
//!
//! A port of util-linux 2.39.3's `misc-utils/findfs.c`: one operand, a tag
//! (`LABEL=`, `UUID=`, `PARTUUID=`, `PARTLABEL=`), evaluated by `ulblkid`
//! -- the port of libblkid -- through udev's links and then its device
//! cache, as upstream's is. Measured against `findfs from util-linux
//! 2.39.3` by `scripts/blkid-cli-diff.sh`.
//!
//! Upstream's quirks are kept where they show:
//!
//! * Anything but exactly one argument is "bad usage", status 2 -- the
//!   status e2fsprogs' findfs used -- before options are looked at.
//! * An operand without `=` is not a tag, and is printed back as it is;
//!   `--` is such an operand.
//!
//! This replaces the `findfs` personality of the old hand-written `blkid`.
//!
//! # What is not upstream's
//!
//! * **A name in a diagnostic** has its unprintable bytes escaped
//!   (design-decisions §370).

use getoptlong::{Opt, Program, Takes};
use quoting::{escape_unprintable, os_bytes};
use std::ffi::{OsStr, OsString};
use std::process::ExitCode;
use ulclosestream::{Stdout, stderr_write, warnx};

/// `FINDFS_SUCCESS`.
const FINDFS_SUCCESS: u8 = 0;
/// `FINDFS_NOT_FOUND`: the label or UUID cannot be found.
const FINDFS_NOT_FOUND: u8 = 1;
/// `FINDFS_USAGE_ERROR`.
const FINDFS_USAGE_ERROR: u8 = 2;

/// Getopt's errors are only sentences here; the referral follows them.
const FINDFS: Program = Program::new("findfs", 2);

/// Upstream's `longopts[]`.
const LONGS: &[(&str, Takes)] = &[("version", Takes::Nothing), ("help", Takes::Nothing)];

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

/// `usage()`.
fn usage(short: &[u8]) -> Vec<u8> {
    let s = shown(short);
    let mut t = String::from("\nUsage:\n");
    t.push_str(&format!(
        " {s} [options] {{LABEL,UUID,PARTUUID,PARTLABEL}}=<value>\n"
    ));
    t.push('\n');
    t.push_str("Find a filesystem by label or UUID.\n");
    t.push_str("\nOptions:\n");
    t.push_str(&format!(
        "{:<16}{}\n{:<16}{}\n",
        " -h, --help", "display this help", " -V, --version", "display version"
    ));
    t.push_str("\nFor more details see findfs(8).\n");
    t.into_bytes()
}

stdfdguard::guard_std_fds!();

fn main() -> ExitCode {
    stdfdguard::restore();
    let argv: Vec<OsString> = std::env::args_os().collect();
    let short = short_name(
        argv.first()
            .map_or(OsStr::new("findfs"), OsString::as_os_str),
    );
    // `close_stdout`, with the default CLOSE_EXIT_CODE.
    let mut out = Stdout::new(1);
    let status = run(&argv, &short, &mut out);
    ExitCode::from(out.close(status, &short))
}

/// `main()`.
fn run(argv: &[OsString], short: &[u8], out: &mut Stdout) -> u8 {
    let arg0 = argv
        .first()
        .map_or(OsStr::new("findfs"), OsString::as_os_str);
    if argv.len() != 2 {
        // 2, for backward compatibility with e2fsprogs' findfs.
        warnx(short, "bad usage");
        return errtryhelp(short, FINDFS_USAGE_ERROR);
    }
    let own = argv.get(1..).unwrap_or_default();
    for item in FINDFS.parse(own, "Vh", LONGS) {
        match item {
            Err(e) => {
                // glibc names the program by argv[0] as given.
                stderr_write(format!("{}: {}\n", shown(&os_bytes(arg0)), e.sentence).as_bytes());
                return errtryhelp(short, FINDFS_USAGE_ERROR);
            }
            Ok(Opt::Short(b'V', _)) | Ok(Opt::Long("version", _)) => {
                out.write(format!("{} from util-linux 2.39.3\n", shown(short)).as_bytes());
                return FINDFS_SUCCESS;
            }
            Ok(Opt::Short(b'h', _)) | Ok(Opt::Long("help", _)) => {
                out.write(&usage(short));
                return FINDFS_SUCCESS;
            }
            Ok(Opt::Operand(_)) => {}
            Ok(_) => return errtryhelp(short, FINDFS_USAGE_ERROR),
        }
    }
    // `argv[1]`, whatever getopt made of it.
    let spec = argv
        .get(1)
        .map(|a| os_bytes(a).into_owned())
        .unwrap_or_default();
    let Some(dev) = ulblkid::evaluate::evaluate_tag(&spec, None, None) else {
        warnx(short, &format!("unable to resolve '{}'", shown(&spec)));
        return FINDFS_NOT_FOUND;
    };
    // `puts(dev)`.
    out.write(&dev);
    out.write(b"\n");
    FINDFS_SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_usage_names_the_program() {
        let u = String::from_utf8(usage(b"findfs")).unwrap_or_default();
        assert!(
            u.starts_with("\nUsage:\n findfs [options] {LABEL,UUID,PARTUUID,PARTLABEL}=<value>\n")
        );
        assert!(u.contains(" -h, --help     display this help\n"));
        assert!(u.ends_with("\nFor more details see findfs(8).\n"));
    }

    #[test]
    fn names_are_argv0_past_its_slash() {
        assert_eq!(short_name(OsStr::new("/sbin/findfs")), b"findfs");
        assert_eq!(short_name(OsStr::new("findfs")), b"findfs");
    }
}
