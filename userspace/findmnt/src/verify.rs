//! `findmnt --verify`: `findmnt-verify.c`, with upstream's names.
//!
//! Each entry of the table is checked -- its mount point exists (and is a
//! directory for a bind mount), its source exists or its tag resolves, its
//! type is known to the kernel and matches what is on the disk, and a root
//! filesystem is checked first by fsck -- and every finding is printed
//! under the entry's mount point, as `[E]` (an error), `[W]` (a warning) or,
//! with `--verbose`, `[ ]` (fine). A summary follows: the counts on stderr,
//! or `Success, no errors or warnings detected` on stdout.
//!
//! Upstream's quirks are kept: a swap area's unsupported `discard=` policy
//! is shown with the rest of the option string after it (upstream prints
//! from the value's start with `%s`), and any prefix of `once` or `pages`
//! is accepted; with `--nocanonicalize` no tag is ever resolved, so every
//! `LABEL=` source is "unreachable".

use crate::{FL_FIRSTONLY, FL_NOCACHE, FL_NOSWAPMATCH, FL_VERBOSE, Findmnt};
use ulclosestream::{Stdout, stderr_write};
use ulmount::blkid::ProbeFail;
use ulmount::cache::Cache;
use ulmount::fs::Fs;
use ulmount::tab::{Direction, Iter, Table as MntTable};

/// `struct verify_context`.
struct Verify {
    /// The entry being checked.
    fs: Option<usize>,
    /// Filesystem types the kernel knows (`/proc/filesystems`) or has a
    /// module for (`modules.dep`).
    fs_ary: Vec<Vec<u8>>,
    nwarnings: i32,
    nerrors: i32,
    target_printed: bool,
    no_fsck: bool,
}

/// `strerror(errno)` as glibc words it.
fn strerror(errno: i32) -> String {
    let e = std::io::Error::from_raw_os_error(errno);
    let s = e.to_string();
    // Rust appends " (os error N)"; glibc's text is what precedes it.
    match s.rfind(" (os error ") {
        Some(i) => s.get(..i).unwrap_or_default().to_string(),
        None => s,
    }
}

/// The `errno` an I/O error leaves.
fn errno_of(e: &std::io::Error) -> i32 {
    e.raw_os_error().unwrap_or(5)
}

impl Verify {
    /// `verify_mesg(vfy, type, fmt, ap)`: the entry's mount point the first
    /// time, then the finding.
    fn mesg(&mut self, tb: &MntTable, out: &mut Stdout, kind: char, text: &str) {
        if !self.target_printed
            && let Some(i) = self.fs
        {
            let mut line = tb
                .ents
                .get(i)
                .and_then(|fs| fs.target.clone())
                .unwrap_or_else(|| b"(null)".to_vec());
            line.push(b'\n');
            out.write(&line);
            self.target_printed = true;
        }
        out.write(format!("   [{kind}] {text}\n").as_bytes());
    }

    /// `verify_warn`.
    fn warn(&mut self, tb: &MntTable, out: &mut Stdout, text: &str) {
        self.nwarnings = self.nwarnings.saturating_add(1);
        self.mesg(tb, out, 'W', text);
    }

    /// `verify_err`.
    fn err(&mut self, tb: &MntTable, out: &mut Stdout, text: &str) {
        self.nerrors = self.nerrors.saturating_add(1);
        self.mesg(tb, out, 'E', text);
    }

    /// `verify_ok`: only with `--verbose`.
    fn ok(&mut self, f: &Findmnt, tb: &MntTable, out: &mut Stdout, text: &str) {
        if f.flags & FL_VERBOSE != 0 {
            self.mesg(tb, out, ' ', text);
        }
    }

    /// `is_supported_filesystem(vfy, name)`.
    fn is_supported_filesystem(&self, name: &[u8]) -> bool {
        self.fs_ary
            .iter()
            .any(|t| ulmount::utils::match_fstype(Some(t), Some(name)))
    }

    /// `add_filesystem(vfy, name)`.
    fn add_filesystem(&mut self, name: &[u8]) {
        if !self.is_supported_filesystem(name) {
            self.fs_ary.push(name.to_vec());
        }
    }
}

/// `fgets(buf, size, f)` over a whole file: its lines, each cut into
/// pieces of at most `size - 1` bytes.
fn fgets_lines(text: &[u8], size: usize) -> Vec<&[u8]> {
    let mut out = Vec::new();
    let mut rest = text;
    let max = size.saturating_sub(1).max(1);
    while !rest.is_empty() {
        let take = match rest.iter().position(|&b| b == b'\n') {
            Some(i) if i < max => i.saturating_add(1),
            _ => max.min(rest.len()),
        };
        out.push(rest.get(..take).unwrap_or_default());
        rest = rest.get(take..).unwrap_or_default();
    }
    out
}

/// C's `isspace`.
fn is_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}

/// `read_proc_filesystems(vfy)`: each line's name -- after `nodev` and the
/// white space, to the first newline, tab or blank.
fn read_proc_filesystems(vfy: &mut Verify) {
    let Ok(text) = std::fs::read("/proc/filesystems") else {
        return;
    };
    for line in fgets_lines(&text, 80) {
        let line = ulmount::c_str(line);
        let mut cp = 0usize;
        if line.first().is_some_and(|&b| !is_space(b)) {
            while line.get(cp).is_some_and(|&b| !is_space(b)) {
                cp = cp.saturating_add(1);
            }
        }
        while line.get(cp).is_some_and(|&b| is_space(b)) {
            cp = cp.saturating_add(1);
        }
        let name = line.get(cp..).unwrap_or_default();
        let end = name
            .iter()
            .position(|&b| b == b'\n' || b == b'\t' || b == b' ')
            .unwrap_or(name.len());
        vfy.add_filesystem(name.get(..end).unwrap_or_default());
    }
}

/// `read_kernel_filesystems(vfy)`: the filesystem modules of the running
/// kernel, from `/lib/modules/RELEASE/modules.dep` (none if it is not
/// there).
fn read_kernel_filesystems(vfy: &mut Verify) {
    let Some(release) = uname_release() else {
        return;
    };
    let mut path = b"/lib/modules/".to_vec();
    path.extend_from_slice(&release);
    path.extend_from_slice(b"/modules.dep");
    path.truncate(1023);
    let Ok(text) = std::fs::read(quoting::os_from_bytes(&path)) else {
        return;
    };
    for piece in fgets_lines(&text, 1024) {
        let buf = ulmount::c_str(piece);
        if !buf.starts_with(b"kernel/fs/") || buf.starts_with(b"kernel/fs/nls/") {
            continue;
        }
        let Some(colon) = buf.iter().position(|&b| b == b':') else {
            continue;
        };
        let head = buf.get(..colon).unwrap_or_default();
        let Some(slash) = head.iter().rposition(|&b| b == b'/') else {
            continue;
        };
        let name = head.get(slash.saturating_add(1)..).unwrap_or_default();
        let Some(ko) = name.windows(3).position(|w| w == b".ko") else {
            continue;
        };
        vfy.add_filesystem(name.get(..ko).unwrap_or_default());
    }
}

/// `uname(&uts)`'s release, from `/proc/sys/kernel/osrelease`.
fn uname_release() -> Option<Vec<u8>> {
    let t = std::fs::read("/proc/sys/kernel/osrelease").ok()?;
    let line = t.split(|&b| b == b'\n').next().unwrap_or_default();
    Some(line.to_vec())
}

/// `mnt_fs_get_option(fs, name, NULL, NULL) == 1`: the option is not there.
fn lacks(fs: &Fs, name: &[u8]) -> bool {
    matches!(fs.get_option(name), Ok(None))
}

/// `mnt_fs_get_option(fs, name, &arg, &argsz) == 0 && arg`: the option's
/// value, and -- what upstream's `%s` of `arg` prints -- the rest of the
/// option string it sits in, from the value on.
fn option_value_and_rest(fs: &Fs, name: &[u8]) -> Option<(Vec<u8>, Vec<u8>)> {
    for s in [&fs.fs_optstr, &fs.vfs_optstr, &fs.user_optstr]
        .into_iter()
        .flatten()
    {
        match ulmount::optstr::locate_option(s, name) {
            Ok(Some(loc)) => {
                let (start, len) = loc.value?;
                let value = s.get(start..start.saturating_add(len))?.to_vec();
                let rest = s.get(start..).unwrap_or_default().to_vec();
                return Some((value, rest));
            }
            Ok(None) => {}
            Err(_) => return None,
        }
    }
    None
}

/// `verify_order(vfy)`: a mount point listed again, or one listed after a
/// mount point below it.
fn verify_order(vfy: &mut Verify, f: &mut Findmnt, tb: &MntTable, out: &mut Stdout) {
    let Some(i) = vfy.fs else {
        return;
    };
    let Some(target) = tb.ents.get(i).and_then(|fs| fs.target.clone()) else {
        return;
    };
    let resolve = |f: &mut Findmnt, t: &[u8]| -> Option<Vec<u8>> {
        if f.flags & FL_NOCACHE != 0 {
            return Some(t.to_vec());
        }
        match f.cache.as_mut() {
            Some(c) => c.resolve_target(t),
            None => Some(t.to_vec()),
        }
    };
    let Some(tgt) = resolve(f, &target) else {
        return;
    };
    let mut itr = Iter::new(Direction::Forward);
    tb.set_iter(&mut itr, i);
    // The entry itself.
    if tb.next_fs(&mut itr).is_none() {
        return;
    }
    while let Some(n) = tb.next_fs(&mut itr) {
        let Some(nt) = tb.ents.get(n).and_then(|fs| fs.target.clone()) else {
            continue;
        };
        let Some(n_tgt) = resolve(f, &nt) else {
            continue;
        };
        let len = n_tgt.len();
        if tgt.starts_with(&n_tgt) {
            match tgt.get(len) {
                None => vfy.warn(tb, out, "target specified more than once"),
                Some(b'/') => {
                    let text = format!(
                        "wrong order: {} specified before {}",
                        crate::shown(&tgt),
                        crate::shown(&n_tgt)
                    );
                    vfy.err(tb, out, &text);
                }
                Some(_) => {}
            }
        }
    }
}

/// `verify_target(vfy)`.
fn verify_target(vfy: &mut Verify, f: &mut Findmnt, tb: &MntTable, fs: &Fs, out: &mut Stdout) {
    let Some(mut tgt) = fs.target.clone() else {
        vfy.err(tb, out, "undefined target (fs_file)");
        return;
    };
    if f.flags & FL_NOCACHE == 0
        && let Some(c) = f.cache.as_mut()
    {
        let Some(cn) = c.resolve_target(&tgt) else {
            return;
        };
        if cn != tgt {
            vfy.warn(
                tb,
                out,
                &format!("non-canonical target path (real: {})", crate::shown(&cn)),
            );
        }
        tgt = cn;
    }
    match std::fs::metadata(quoting::os_from_bytes(&tgt)) {
        Err(e) => {
            let m = strerror(errno_of(&e));
            if lacks(fs, b"noauto") {
                vfy.err(
                    tb,
                    out,
                    &format!("unreachable on boot required target: {m}"),
                );
            } else {
                vfy.warn(tb, out, &format!("unreachable target: {m}"));
            }
        }
        Ok(sb) if !sb.is_dir() && lacks(fs, b"bind") => {
            vfy.err(tb, out, "target is not a directory");
        }
        Ok(_) => vfy.ok(f, tb, out, "target exists"),
    }
}

/// `verify_tag(vfy, name, value)`: the device the tag names, if it can be
/// found (never with `--nocanonicalize`).
fn verify_tag(
    vfy: &mut Verify,
    f: &mut Findmnt,
    tb: &MntTable,
    fs: &Fs,
    name: &[u8],
    value: &[u8],
    out: &mut Stdout,
) -> Option<Vec<u8>> {
    let src = if f.flags & FL_NOCACHE == 0 {
        f.cache.as_mut().and_then(|c| c.resolve_tag(name, value))
    } else {
        None
    };
    let (n, v) = (crate::shown(name), crate::shown(value));
    match &src {
        None if lacks(fs, b"noauto") => vfy.err(
            tb,
            out,
            &format!("unreachable on boot required source: {n}={v}"),
        ),
        None => vfy.warn(tb, out, &format!("unreachable: {n}={v}")),
        Some(s) => vfy.ok(
            f,
            tb,
            out,
            &format!("{n}={v} translated to {}", crate::shown(s)),
        ),
    }
    src
}

/// `verify_source(vfy)`: a tag must resolve; a path is checked for being
/// there and being a block device, unless the filesystem is a pseudo or a
/// network one, or bound.
fn verify_source(vfy: &mut Verify, f: &mut Findmnt, tb: &MntTable, fs: &Fs, out: &mut Stdout) {
    let src = match fs.srcpath() {
        None => {
            let Some((t, v)) = fs.tag() else {
                vfy.err(tb, out, "undefined source (fs_spec)");
                return;
            };
            let (t, v) = (t.to_vec(), v.to_vec());
            match verify_tag(vfy, f, tb, fs, &t, &v, out) {
                Some(s) => s,
                None => return,
            }
        }
        Some(s) => {
            // A tag blkid can parse but libmount does not know.
            if ulmount::utils::parse_tag_string(s).is_some()
                && std::fs::metadata(quoting::os_from_bytes(s)).is_err()
            {
                vfy.err(
                    tb,
                    out,
                    &format!("unsupported source tag: {}", crate::shown(s)),
                );
                return;
            }
            s.to_vec()
        }
    };
    let isbind = matches!(fs.get_option(b"bind"), Ok(Some(_)));
    let shown = crate::shown(&src);
    if fs.is_pseudofs() || fs.is_netfs() {
        vfy.ok(
            f,
            tb,
            out,
            &format!("do not check {shown} source (pseudo/net)"),
        );
        return;
    }
    match std::fs::metadata(quoting::os_from_bytes(&src)) {
        Err(e) => vfy.warn(
            tb,
            out,
            &format!("unreachable source: {shown}: {}", strerror(errno_of(&e))),
        ),
        Ok(sb) if (sb.is_dir() || sb.is_file()) && !isbind => {
            vfy.warn(
                tb,
                out,
                &format!("non-bind mount source {shown} is a directory or regular file"),
            );
        }
        Ok(sb) if !is_block(&sb) && !isbind => {
            vfy.warn(tb, out, &format!("source {shown} is not a block device"));
        }
        Ok(_) => vfy.ok(f, tb, out, &format!("source {shown} exists")),
    }
}

/// `S_ISBLK`.
fn is_block(m: &std::fs::Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::FileTypeExt;
        m.file_type().is_block_device()
    }
    #[cfg(not(unix))]
    {
        let _ = m;
        false
    }
}

/// `verify_options(vfy)`: with `--verbose`, the three kinds of options.
fn verify_options(vfy: &mut Verify, f: &Findmnt, tb: &MntTable, fs: &Fs, out: &mut Stdout) {
    if let Some(o) = &fs.vfs_optstr {
        vfy.ok(f, tb, out, &format!("VFS options: {}", crate::shown(o)));
    }
    if let Some(o) = &fs.fs_optstr {
        vfy.ok(f, tb, out, &format!("FS options: {}", crate::shown(o)));
    }
    if let Some(o) = &fs.user_optstr {
        vfy.ok(
            f,
            tb,
            out,
            &format!("userspace options: {}", crate::shown(o)),
        );
    }
}

/// `verify_swaparea(vfy)`: `discard=` and `pri=`.
fn verify_swaparea(vfy: &mut Verify, tb: &MntTable, fs: &Fs, out: &mut Stdout) {
    if let Some((arg, rest)) = option_value_and_rest(fs, b"discard") {
        // `strncmp(arg, "once", argsz)`: a prefix of either is accepted.
        let prefix_of = |word: &[u8]| word.starts_with(&arg);
        if !prefix_of(b"once") && !prefix_of(b"pages") {
            vfy.err(
                tb,
                out,
                &format!(
                    "unsupported swaparea discard policy: {}",
                    crate::shown(&rest)
                ),
            );
        }
    }
    if let Some((arg, _)) = option_value_and_rest(fs, b"pri") {
        let digits = arg.strip_prefix(b"-").unwrap_or(&arg);
        if !digits.iter().all(u8::is_ascii_digit) {
            vfy.err(tb, out, "failed to parse swaparea priority option");
        }
    }
}

/// `verify_fstype(vfy)`: the type is one the kernel knows, and matches what
/// is on the device.
fn verify_fstype(vfy: &mut Verify, f: &mut Findmnt, tb: &MntTable, fs: &Fs, out: &mut Stdout) {
    let Some(source) = fs.source.clone() else {
        return;
    };
    let src = match f.cache.as_mut() {
        Some(c) => c.resolve_spec(&source),
        None => Cache::new().resolve_spec(&source),
    };
    let Some(src) = src else {
        return;
    };
    let shown_src = crate::shown(&src);
    if fs.is_pseudofs() || fs.is_netfs() {
        vfy.ok(
            f,
            tb,
            out,
            &format!("do not check {shown_src} FS type (pseudo/net)"),
        );
        return;
    }
    let ty = fs.fstype.clone();
    let mut isauto = false;
    if let Some(t) = &ty {
        let none = t == b"none";
        if none && lacks(fs, b"bind") && lacks(fs, b"move") {
            vfy.warn(
                tb,
                out,
                "\"none\" FS type is recommended for bind or move oprations only",
            );
            return;
        }
        let mut isswap = false;
        match t.as_slice() {
            b"auto" => isauto = true,
            b"swap" => isswap = true,
            b"xfs" | b"btrfs" => vfy.no_fsck = true,
            _ => {}
        }
        if !isswap && !isauto && !none && !vfy.is_supported_filesystem(t) {
            vfy.warn(
                tb,
                out,
                &format!(
                    "{} seems unsupported by the current kernel",
                    crate::shown(t)
                ),
            );
        }
    }
    // `errno = 0; realtype = mnt_get_fstype(src, &ambi, cache)`.
    let realtype = match f.cache.as_mut() {
        Some(c) => c.get_fstype(&src),
        None => probe_type(&src),
    };
    let realtype = match realtype {
        Ok(r) => r,
        Err(fail) => {
            let reason = match fail.errno() {
                Some(e) if e != 0 => strerror(e),
                _ => "reason unknown".to_string(),
            };
            let text = format!("cannot detect on-disk filesystem type ({reason})");
            if isauto {
                vfy.err(tb, out, &text);
            } else {
                vfy.warn(tb, out, &text);
            }
            return;
        }
    };
    let isswap = realtype == b"swap";
    vfy.no_fsck = realtype == b"xfs" || realtype == b"btrfs";
    let shown_real = crate::shown(&realtype);
    if let Some(t) = &ty
        && !isauto
        && *t != realtype
    {
        vfy.warn(
            tb,
            out,
            &format!(
                "{} does not match with on-disk {shown_real}",
                crate::shown(t)
            ),
        );
        return;
    }
    if !isswap && !vfy.is_supported_filesystem(&realtype) {
        vfy.warn(
            tb,
            out,
            &format!("on-disk {shown_real} seems unsupported by the current kernel"),
        );
        return;
    }
    vfy.ok(f, tb, out, &format!("FS type is {shown_real}"));
}

/// `mnt_get_fstype(devname, &ambi, NULL)`: probed directly, TYPE only.
fn probe_type(devname: &[u8]) -> Result<Vec<u8>, ProbeFail> {
    let file = ulmount::blkid::open_nonblock(devname)?;
    let size = ulmount::blkid::device_size(&file)?;
    let values = ulmount::blkid::probe_file(&file, size, ulmount::blkid::SUBLKS_TYPE)?;
    values
        .into_iter()
        .find(|(n, _)| *n == b"TYPE")
        .map(|(_, v)| v)
        .ok_or(ProbeFail::Nothing)
}

/// `verify_passno(vfy)`: the root filesystem should be checked first.
fn verify_passno(vfy: &mut Verify, tb: &MntTable, fs: &Fs, out: &mut Stdout) {
    if fs.target.as_deref() == Some(b"/") && fs.passno != 1 && !vfy.no_fsck {
        vfy.warn(
            tb,
            out,
            &format!("recommended root FS passno is 1 (current is {})", fs.passno),
        );
    }
}

/// `verify_filesystem(vfy)`.
fn verify_filesystem(vfy: &mut Verify, f: &mut Findmnt, tb: &MntTable, out: &mut Stdout) {
    let Some(fs) = vfy.fs.and_then(|i| tb.ents.get(i)).cloned() else {
        return;
    };
    if fs.is_swaparea() {
        verify_swaparea(vfy, tb, &fs, out);
    } else {
        verify_target(vfy, f, tb, &fs, out);
        verify_options(vfy, f, tb, &fs, out);
    }
    verify_source(vfy, f, tb, &fs, out);
    verify_fstype(vfy, f, tb, &fs, out);
    verify_passno(vfy, tb, &fs, out);
}

/// `mtime` as seconds and nanoseconds.
fn mtime_of(path: &str) -> Option<(i64, i64)> {
    let m = std::fs::metadata(path).ok()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        Some((m.mtime(), m.mtime_nsec()))
    }
    #[cfg(not(unix))]
    {
        let _ = m;
        None
    }
}

/// `verify_table(tb)`: every entry checked, then the summary. The number of
/// errors (parse errors included), which decides the exit status.
pub(crate) fn verify_table(f: &mut Findmnt, tb: &MntTable, out: &mut Stdout) -> i32 {
    let mut vfy = Verify {
        fs: None,
        fs_ary: Vec::new(),
        nwarnings: 0,
        nerrors: 0,
        target_printed: false,
        no_fsck: false,
    };
    let check_order = f.is_listall_mode();
    let mut itr = Iter::new(Direction::Forward);
    read_proc_filesystems(&mut vfy);
    read_kernel_filesystems(&mut vfy);
    loop {
        vfy.fs = f.get_next_fs(tb, &mut itr);
        if vfy.fs.is_none() {
            break;
        }
        vfy.target_printed = false;
        vfy.no_fsck = false;
        if check_order {
            verify_order(&mut vfy, f, tb, out);
        }
        verify_filesystem(&mut vfy, f, tb, out);
        if f.flags & FL_FIRSTONLY != 0 {
            break;
        }
        f.flags |= FL_NOSWAPMATCH;
    }
    // systemd reads fstab into units once; an fstab newer than that has not
    // been read.
    if let (Some(a), Some(b)) = (
        mtime_of("/run/systemd/systemd-units-load"),
        mtime_of("/etc/fstab"),
    ) && a < b
    {
        vfy.warn(
            tb,
            out,
            "your fstab has been modified, but systemd still uses the old version;\n       use 'systemctl daemon-reload' to reload",
        );
    }
    let plural = |n: i32, one: &str, many: &str| {
        if n == 1 {
            one.replace("%d", &n.to_string())
        } else {
            many.replace("%d", &n.to_string())
        }
    };
    if vfy.nerrors != 0 || f.parse_nerrors != 0 || vfy.nwarnings != 0 {
        let text = format!(
            "\n{}{}{}\n",
            plural(f.parse_nerrors, "%d parse error", "%d parse errors"),
            plural(vfy.nerrors, ", %d error", ", %d errors"),
            plural(vfy.nwarnings, ", %d warning", ", %d warnings"),
        );
        stderr_write(text.as_bytes());
    } else {
        out.write(b"Success, no errors or warnings detected\n");
    }
    vfy.nerrors.saturating_add(f.parse_nerrors)
}

/// `fgets_lines`, for the crate's tests.
#[cfg(test)]
pub(crate) fn fgets_lines_for_tests(text: &[u8], size: usize) -> Vec<&[u8]> {
    fgets_lines(text, size)
}
