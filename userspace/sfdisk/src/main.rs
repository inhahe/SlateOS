//! sfdisk -- display or manipulate a disk partition table from a script.
//!
//! A port of util-linux 2.39.3's `disk-utils/sfdisk.c`, function by
//! function and with upstream's names, on the libfdisk port, listing
//! through `fdisk::list` (`fdisk-list.c`, which upstream links into both
//! programs); measured against Ubuntu's `sfdisk` by `scripts/sfdisk-diff.sh`.
//!
//! # What is upstream's, and easy to get wrong
//!
//! * **The script is read a line at a time** from standard input, through
//!   libfdisk's script parser; a line it cannot parse is tried as one of
//!   the shell's commands (`print`, `help`, `quit`, `write`, `abort`).
//! * **A reply's leading blanks** are moved over by `memmove` of only as
//!   many bytes as there were blanks, so `  yes` reads as `yeyes`: upstream
//!   copies `p - buf` bytes where it means the rest of the line. Kept.
//! * **`--activate`'s failure** names the argument's position, not the
//!   partition it asked for (`devname, i + 1`).
//!
//! # What is not upstream's
//!
//! * **No readline**: on a terminal the prompt is written and the line read
//!   as from a pipe (known-issues `TD-B-FDISK-HAS-NO-READLINE`).
//! * **Table headers are not coloured** (known-issues
//!   `TD-B-SMARTCOLS-DRAWS-NO-COLOUR`).

stdfdguard::guard_std_fds!();

use std::cell::{Cell, RefCell};
use std::ffi::{OsStr, OsString};
use std::io::{IsTerminal, Read, Seek, SeekFrom, Write};
use std::rc::Rc;

use fdisk::list::{self, Fields, WIPEMODE_ALWAYS, WIPEMODE_AUTO, WIPEMODE_NEVER};
use fdisk::{StdinInput, Term};
use getoptlong::{LongVals, Opt, Program, Takes};
use libfdisk::errno::{EINVAL, EIO, ENOTSUP};
use libfdisk::parttype::{PARSE_ALIAS, PARSE_DATA, PARSE_NAME, PARSE_SHORTCUT};
use libfdisk::script::{BUFSIZ, Script, ScriptRef};
use libfdisk::utils::partname;
use libfdisk::{Ask, AskType, Context, LabelType, Partition, fieldtype};
use quoting::{escape_unprintable, os_bytes};
use ulclosestream::{stderr_write, warn, warnx};

/// `PACKAGE_STRING`.
const PACKAGE_STRING: &str = "util-linux 2.39.3";

/// `SFDISK_PROMPT`.
const SFDISK_PROMPT: &[u8] = b">>> ";

/// `SFDISK_DONE_*`: how the script loop ended.
const SFDISK_DONE_NONE: i32 = 0;
const SFDISK_DONE_EOF: i32 = 1;
const SFDISK_DONE_ABORT: i32 = 2;
const SFDISK_DONE_WRITE: i32 = 3;
const SFDISK_DONE_ASK: i32 = 4;

/// What the dialog callback shares with the program.
struct Shared {
    /// Standard output, its colours, the program's name.
    term: Term,
    /// `sf->quiet`: no information lines.
    quiet: Cell<bool>,
}

/// `get_user_reply (prompt, buf, bufsz)`: `prompt` written and a line read;
/// none at the end of the input. Upstream moves the reply over its leading
/// non-graphic bytes with `memmove (buf, p, p - buf)` -- as many bytes as
/// it skipped, not the rest of the line -- and that is kept.
fn get_user_reply(sh: &Shared, prompt: &[u8], bufsz: usize) -> Option<Vec<u8>> {
    sh.term.write(prompt);
    sh.term.flush();
    let got = fdisk::fgets_stdin(bufsz);
    if got.line.is_empty() {
        return None;
    }
    Some(reply_from_line(&got.line))
}

/// The reply in a line `fgets` read, as `get_user_reply` leaves it: its
/// leading non-graphic bytes moved over -- by only as many bytes as were
/// skipped -- and its newline taken off.
fn reply_from_line(line: &[u8]) -> Vec<u8> {
    // The C string fgets left.
    let mut buf = libfdisk::utils::c_str(line).to_vec();
    let skip = buf.iter().take_while(|c| !c.is_ascii_graphic()).count();
    if skip > 0 {
        let end = skip.saturating_mul(2).min(buf.len());
        let moved: Vec<u8> = buf.get(skip..end).unwrap_or_default().to_vec();
        if let Some(d) = buf.get_mut(..moved.len()) {
            d.copy_from_slice(&moved);
        }
        if moved.len() < skip {
            // The bytes moved included the string's end.
            buf.truncate(moved.len());
        }
    }
    if buf.last() == Some(&b'\n') {
        buf.pop();
    }
    buf
}

/// `rpmatch (s)`: 1 for yes, 0 for no, -1 for neither.
fn rpmatch(s: &[u8]) -> i32 {
    match s.first() {
        Some(b'y' | b'Y') => 1,
        Some(b'n' | b'N') => 0,
        _ => -1,
    }
}

/// `ask_callback (cxt, ask, sf)`: messages printed, yes-or-no questions
/// answered from standard input; libfdisk asks sfdisk nothing else.
fn ask_callback(sh: &Shared, ask: &mut Ask) -> i32 {
    match ask.get_type() {
        AskType::Info => {
            if !sh.quiet.get() {
                sh.term.write(ask.print_get_mesg().unwrap_or_default());
                sh.term.write(b"\n");
            }
        }
        AskType::Warnx => {
            let msg = ask.print_get_mesg().unwrap_or_default().to_vec();
            sh.term.warning(&msg);
            stderr_write(b"\n");
        }
        AskType::Warn => {
            let mut msg = ask.print_get_mesg().unwrap_or_default().to_vec();
            let e = std::io::Error::from_raw_os_error(ask.print_get_errno());
            msg.extend_from_slice(format!(": {}\n", errmsg::strerror(&e)).as_bytes());
            sh.term.warning(&msg);
        }
        AskType::YesNo => {
            sh.term.write(b"\n");
            loop {
                sh.term.write(ask.get_query().unwrap_or_default());
                let Some(buf) = get_user_reply(sh, b" [Y]es/[N]o: ", BUFSIZ) else {
                    return 1;
                };
                let x = rpmatch(&buf);
                if x == 1 || x == 0 {
                    ask.yesno_set_result(x);
                    return 0;
                }
            }
        }
        _ => {}
    }
    0
}

/// `ACT_*`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Act {
    None,
    Fdisk,
    Activate,
    Dump,
    List,
    ListFree,
    ListTypes,
    Reorder,
    Relocate,
    ShowSize,
    ShowGeom,
    Verify,
    PartType,
    PartUuid,
    PartLabel,
    PartAttrs,
    DiskId,
    Delete,
    BackupSectors,
}

/// `struct sfdisk`.
struct Sfdisk {
    sh: Rc<Shared>,
    act: Act,
    /// `-N`, counted from 0; -1 without it.
    partno: i32,
    wipemode: i32,
    pwipemode: i32,
    lockmode: Option<Vec<u8>>,
    label: Option<Vec<u8>>,
    label_nested: Option<Vec<u8>>,
    backup_file: Option<Vec<u8>>,
    move_typescript: Option<Vec<u8>>,
    prompt: Option<Vec<u8>>,
    cxt: Context,
    /// `-N`'s partition before the change, for `--move-data`.
    orig_pa: Option<Partition>,
    verify: bool,
    interactive: bool,
    noreread: bool,
    force: bool,
    backup: bool,
    container: bool,
    unused: bool,
    append: bool,
    json: bool,
    movedata: bool,
    movefsync: bool,
    notell: bool,
    noact: bool,
    fields: Fields,
}

/// An argument as a C string's bytes.
fn arg_bytes(arg: &OsStr) -> Vec<u8> {
    os_bytes(arg).into_owned()
}

/// `basename (path)` (libgen's): the last component, trailing slashes
/// aside.
fn basename(path: &[u8]) -> &[u8] {
    let trimmed = {
        let n = path
            .iter()
            .rposition(|&c| c != b'/')
            .map_or(0, |i| i.saturating_add(1));
        path.get(..n).unwrap_or_default()
    };
    if trimmed.is_empty() {
        return if path.is_empty() { b"." } else { b"/" };
    }
    let start = trimmed
        .iter()
        .rposition(|&c| c == b'/')
        .map_or(0, |i| i.saturating_add(1));
    trimmed.get(start..).unwrap_or(trimmed)
}

impl Sfdisk {
    fn short(&self) -> &[u8] {
        &self.sh.term.short
    }

    /// `errx (EXIT_FAILURE, msg)`.
    fn errx(&self, msg: &str) -> ! {
        warnx(self.short(), msg);
        self.sh.term.exit(1)
    }

    /// `err (EXIT_FAILURE, msg)`, with `errnum` as `errno`.
    fn err(&self, msg: &str, errnum: i32) -> ! {
        warn(
            self.short(),
            msg,
            &std::io::Error::from_raw_os_error(errnum),
        );
        self.sh.term.exit(1)
    }

    /// `strtou32_or_err (arg, msg)`.
    fn strtou32_or_err(&self, arg: &[u8], msg: &str) -> u32 {
        ulstrutils::ul_strtou32(arg, 10).unwrap_or_else(|e| {
            self.errx(&ulstrutils::num_error_message(
                msg,
                &quoting::os_from_bytes(arg),
                e,
            ))
        })
    }

    /// `fdisk_info (cxt, ...)` of text built from pieces.
    fn info(&mut self, parts: &[&[u8]]) {
        self.cxt.info(&parts.concat());
    }

    /// `sfdisk_init (sf)`: the context, the callback, and the nested
    /// context `--label-nested` asks for.
    fn init(&mut self) {
        let sh = Rc::clone(&self.sh);
        self.cxt
            .set_ask(Some(Rc::new(move |_: &mut Context, ask: &mut Ask| {
                ask_callback(&sh, ask)
            })));
        if self.wipemode != WIPEMODE_ALWAYS {
            self.cxt.enable_bootbits_protection(true);
        }
        if let Some(nested) = self.label_nested.clone() {
            // The original context is the nested one's parent.
            let parent = std::mem::replace(&mut self.cxt, Context::empty());
            self.cxt = Context::new_nested(parent, Some(&nested));
        }
    }

    /// `get_partition (cxt, partno)`: the partition numbered `partno`, as
    /// the table reads it.
    fn get_partition(&mut self, partno: usize) -> Option<Partition> {
        let mut tb = libfdisk::Table::new();
        if self.cxt.get_partitions(&mut tb) != 0 {
            return None;
        }
        let i = tb.get_partition_by_partno(partno)?;
        tb.get_partition(i).cloned()
    }

    /// `backup_sectors (sf, tpl, name, devname, offset, size)`: `size`
    /// bytes of the device from `offset`, copied to a file named by the
    /// template and the offset.
    fn backup_sectors(&mut self, tpl: &[u8], name: &str, devname: &[u8], offset: u64, size: usize) {
        let mut fname = tpl.to_vec();
        fname.extend_from_slice(format!("0x{offset:08x}.bak").as_bytes());
        let shown_dev = escape_unprintable(devname);
        let fail = |s: &Self| -> ! { s.errx(&format!("{shown_dev}: failed to create a backup")) };

        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let Ok(mut out) = opts.open(ulsysfs::path_of(&fname)) else {
            fail(self)
        };
        let Some(dev) = self.cxt.get_dev().cloned() else {
            fail(self)
        };
        let mut f = &*dev;
        if let Err(e) = f.seek(SeekFrom::Start(offset)) {
            let mut m = b"cannot seek ".to_vec();
            m.extend_from_slice(devname);
            self.cxt.warn(e.raw_os_error().unwrap_or(0), &m);
            fail(self);
        }
        let mut buf = vec![0u8; size];
        if let Err(e) = f.read_exact(&mut buf) {
            let mut m = b"cannot read ".to_vec();
            m.extend_from_slice(devname);
            self.cxt.warn(e.raw_os_error().unwrap_or(0), &m);
            fail(self);
        }
        if let Err(e) = out.write_all(&buf) {
            let mut m = b"cannot write ".to_vec();
            m.extend_from_slice(&fname);
            self.cxt.warn(e.raw_os_error().unwrap_or(0), &m);
            fail(self);
        }
        let mut m = format!("{name:>12} (offset {offset:5}, size {size:5}): ").into_bytes();
        m.extend_from_slice(&fname);
        self.cxt.info(&m);
    }

    /// `mk_backup_filename_tpl (filename, devname, suffix)`: `$HOME/sfdisk-
    /// <dev><suffix>` by default (or for `@default`), else `<filename>-
    /// <dev><suffix>`.
    fn mk_backup_filename_tpl(
        &self,
        filename: Option<&[u8]>,
        devname: &[u8],
        suffix: &[u8],
    ) -> Vec<u8> {
        let name = basename(devname);
        match filename.filter(|f| *f != b"@default") {
            None => {
                let Some(home) = std::env::var_os("HOME") else {
                    self.errx("failed to create a backup file, $HOME undefined");
                };
                [&os_bytes(&home)[..], b"/sfdisk-", name, suffix].concat()
            }
            Some(f) => [f, b"-", name, suffix].concat(),
        }
    }

    /// `backup_partition_table (sf, devname)`: every on-disk part of the
    /// label copied to a file.
    fn backup_partition_table(&mut self, devname: &[u8]) {
        if !self.cxt.has_label() {
            return;
        }
        let tpl = self.mk_backup_filename_tpl(self.backup_file.clone().as_deref(), devname, b"-");
        self.sh.term.color_scheme_enable(b"header", ulcolors::BOLD);
        self.cxt.info(b"Backup files:");
        self.sh.term.color_disable();
        let mut i = 0;
        while let Ok((name, offset, size)) = self.cxt.locate_disklabel(i) {
            if size == 0 {
                break;
            }
            self.backup_sectors(&tpl, name, devname, offset, size);
            i = i.wrapping_add(1);
        }
        if !self.sh.quiet.get() {
            self.sh.term.write(b"\n");
        }
    }

    /// `assign_device (sf, devname, rdonly)`: the device opened, locked and,
    /// with `--backup`, its table saved -- or the program ends.
    fn assign_device(&mut self, devname: &[u8], rdonly: bool) {
        let rc = self.cxt.assign_device(devname, rdonly);
        if rc != 0 {
            self.err(
                &format!("cannot open {}", escape_unprintable(devname)),
                rc.wrapping_neg(),
            );
        }
        if !self.cxt.is_readonly() {
            let locked = self.cxt.get_dev().cloned().map_or(0, |dev| {
                ulblkid::blkdev::lock(&dev, devname, self.lockmode.as_deref(), self.short())
            });
            if locked != 0 {
                // Read-write but untouched: nothing to sync.
                let _ = self.cxt.deassign_device(true);
                self.sh.term.exit(1);
            }
            if self.backup {
                self.backup_partition_table(devname);
            }
        }
    }

    /// `move_partition_data (sf, partno, orig_pa)`: the data of a partition
    /// whose start moved copied to the new start, step by step, in the
    /// order an overlap needs; with a typescript of the steps if asked.
    #[allow(
        clippy::too_many_lines,
        reason = "upstream's function, kept in one piece so it can be read against it"
    )]
    fn move_partition_data(&mut self, partno: usize, orig_pa: &Partition) -> i32 {
        let pa = self.get_partition(partno);
        let refusal = match &pa {
            None => Some("failed to read new partition from device; ignoring --move-data"),
            Some(p) if !p.has_size() => {
                Some("failed to get size of the new partition; ignoring --move-data")
            }
            Some(p) if !p.has_start() => {
                Some("failed to get start of the new partition; ignoring --move-data")
            }
            Some(_) if !orig_pa.has_size() => {
                Some("failed to get size of the old partition; ignoring --move-data")
            }
            Some(_) if !orig_pa.has_start() => {
                Some("failed to get start of the old partition; ignoring --move-data")
            }
            Some(p) if p.start == orig_pa.start => {
                Some("start of the partition has not been moved; ignoring --move-data")
            }
            Some(p) if orig_pa.size > p.size => {
                Some("new partition is smaller than original; ignoring --move-data")
            }
            Some(_) => None,
        };
        if let Some(m) = refusal {
            warnx(self.short(), m);
            return -EINVAL;
        }
        let Some(pa) = pa else { return -EINVAL };

        // The direction, for an overlap.
        let nsectors = orig_pa.size;
        let from = orig_pa.start;
        let to = pa.start;
        let mut backward = false;
        if (to >= from && from.wrapping_add(nsectors) >= to)
            || (from >= to && to.wrapping_add(nsectors) >= from)
        {
            // Copied from the end of the source when the target is after it.
            backward = from < to;
        }

        // The step: nearest to 1 MiB, in optimal I/O units.
        let ss = self.cxt.get_sector_size();
        let mut io = self.cxt.get_optimal_iosize();
        if io == 0 {
            io = ss;
        }
        let mut step_bytes: u64 = if io < 1024 * 1024 {
            (1024 * 1024u64)
                .wrapping_add(io / 2)
                .checked_div(io)
                .unwrap_or(0)
                .wrapping_mul(io)
        } else {
            io
        };
        let step = step_bytes.checked_div(ss).unwrap_or(0);
        let mut nbytes = nsectors.wrapping_mul(ss);

        let devname = partname(self.cxt.get_devname(), partno.wrapping_add(1)).unwrap_or_default();
        let typescript = self
            .move_typescript
            .clone()
            .map(|t| self.mk_backup_filename_tpl(Some(&t), &devname, b".move"));

        let mut progress = false;
        if !self.sh.quiet.get() {
            self.cxt.info(b"");
            self.sh.term.color_scheme_enable(b"header", ulcolors::BOLD);
            self.cxt.info(if self.noact {
                b"Data move: (--no-act)".as_slice()
            } else {
                b"Data move:"
            });
            self.sh.term.color_disable();
            if let Some(t) = &typescript {
                self.info(&[b" typescript file: ", t]);
            }
            self.sh
                .term
                .write(format!("  start sector: (from/to) {from} / {to}\n").as_bytes());
            self.sh
                .term
                .write(format!("  sectors: {nsectors}\n").as_bytes());
            self.sh
                .term
                .write(format!("  step size: {step_bytes} bytes\n").as_bytes());
            self.sh.term.write(b"\n");
            self.sh.term.flush();
            progress = std::io::stdout().is_terminal();
        }
        if self.interactive
            && !self
                .cxt
                .ask_yesno(b"Do you want to move partition data?")
                .unwrap_or(false)
        {
            self.cxt.info(b"Leaving.");
            return 0;
        }

        let mut rc = 0;
        let mut ioerr: usize = 0;
        let mut log: Option<std::io::BufWriter<std::fs::File>> = None;
        'done: {
            if let Some(t) = &typescript {
                match std::fs::File::create(ulsysfs::path_of(t)) {
                    Ok(f) => log = Some(std::io::BufWriter::new(f)),
                    Err(e) => {
                        let errno = e.raw_os_error().unwrap_or(0);
                        rc = errno.wrapping_neg();
                        let mut m = b"cannot open ".to_vec();
                        m.extend_from_slice(t);
                        self.cxt.warn(errno, &m);
                        break 'done;
                    }
                }
                if let Some(f) = log.as_mut() {
                    let mut head = format!("# sfdisk: {PACKAGE_STRING}\n# Disk: ").into_bytes();
                    head.extend_from_slice(&devname);
                    head.extend_from_slice(
                        format!(
                            "\n# Partition: {}\n# Operation: move data\n# Sector size: {ss}\n# Original start offset \
                             (sectors/bytes): {from}/{}\n# New start offset (sectors/bytes): {to}/{}\n# Area size \
                             (sectors/bytes): {nsectors}/{}\n# Step size (sectors/bytes): {step}/{step_bytes}\n# \
                             Steps: {}\n#\n# <step>: <from> <to> (step offsets in bytes)\n",
                            partno.wrapping_add(1),
                            from.wrapping_mul(ss),
                            to.wrapping_mul(ss),
                            nsectors.wrapping_mul(ss),
                            nsectors.checked_div(step).unwrap_or(0).wrapping_add(1),
                        )
                        .as_bytes(),
                    );
                    // The typescript's own write errors are not looked at
                    // until it is closed, and not then either.
                    let _ = f.write_all(&head);
                }
            }

            let Some(dev) = self.cxt.get_dev().cloned() else {
                break 'done;
            };
            let mut fd = &*dev;
            let mut src = (if backward {
                from.wrapping_add(nsectors)
            } else {
                from
            })
            .wrapping_mul(ss);
            let mut dst = (if backward {
                to.wrapping_add(nsectors)
            } else {
                to
            })
            .wrapping_mul(ss);
            let mut buf = vec![0u8; usize::try_from(step_bytes).unwrap_or(0)];
            let mut prev_time = std::time::Instant::now();
            let mut prev: u64 = 0;
            let mut bytes_per_sec: u64 = 0;
            let mut i: u64 = 0;
            let mut cc: usize = 1;
            while i < nsectors && nbytes > 0 {
                if nbytes < step_bytes {
                    step_bytes = nbytes;
                    buf.truncate(usize::try_from(step_bytes).unwrap_or(0));
                }
                nbytes = nbytes.wrapping_sub(step_bytes);
                if backward {
                    src = src.wrapping_sub(step_bytes);
                    dst = dst.wrapping_sub(step_bytes);
                }
                let mut failed = false;
                if !self.noact {
                    // Read the source.
                    if fd.seek(SeekFrom::Start(src)).is_err() || fd.read_exact(&mut buf).is_err() {
                        if let Some(f) = log.as_mut() {
                            let _ = writeln!(f, "{cc:05}: read error {src:12} {dst:12}");
                        }
                        self.cxt.warn(
                            std::io::Error::last_os_error().raw_os_error().unwrap_or(0),
                            format!("cannot read at offset: {src}; continue").as_bytes(),
                        );
                        ioerr = ioerr.wrapping_add(1);
                        failed = true;
                    } else if fd.seek(SeekFrom::Start(dst)).is_err() || fd.write_all(&buf).is_err()
                    {
                        // Write the target.
                        if let Some(f) = log.as_mut() {
                            let _ = writeln!(f, "{cc:05}: write error {src:12} {dst:12}");
                        }
                        self.cxt.warn(
                            std::io::Error::last_os_error().raw_os_error().unwrap_or(0),
                            format!("cannot write at offset: {dst}; continue").as_bytes(),
                        );
                        ioerr = ioerr.wrapping_add(1);
                        failed = true;
                    } else if self.movefsync && dev.sync_all().is_err() {
                        self.cxt.warn(
                            std::io::Error::last_os_error().raw_os_error().unwrap_or(0),
                            format!("cannot fsync at offset: {dst}; continue").as_bytes(),
                        );
                    }
                }
                if !failed {
                    if let Some(f) = log.as_mut() {
                        let _ = writeln!(f, "{cc:05}: {src:12} {dst:12}");
                    }
                    if progress && i.is_multiple_of(10) {
                        let elapsed = prev_time.elapsed();
                        if elapsed.as_secs() > 1 {
                            let usec = u64::try_from(elapsed.as_micros())
                                .unwrap_or(u64::MAX)
                                .max(1);
                            bytes_per_sec = i
                                .wrapping_sub(prev)
                                .wrapping_mul(ss)
                                .checked_div(usec)
                                .unwrap_or(0)
                                .wrapping_mul(1_000_000);
                            prev_time = std::time::Instant::now();
                            prev = i;
                        }
                        #[allow(
                            clippy::cast_precision_loss,
                            reason = "a percentage and a rate on a progress line"
                        )]
                        let pct = 100.0 / (nsectors as f64 / (i.wrapping_add(1)) as f64);
                        let line = if bytes_per_sec != 0 {
                            #[allow(
                                clippy::cast_precision_loss,
                                reason = "a rate on a progress line"
                            )]
                            let mib = (bytes_per_sec / (1024 * 1024)) as f64;
                            format!(
                                "Moved {} from {nsectors} sectors ({pct:.3}%, {mib:.1} MiB/s).",
                                i.wrapping_add(1)
                            )
                        } else {
                            format!(
                                "Moved {} from {nsectors} sectors ({pct:.3}%).",
                                i.wrapping_add(1)
                            )
                        };
                        self.sh.term.write(line.as_bytes());
                        self.sh.term.flush();
                        self.sh.term.write(b"\r");
                    }
                }
                if !backward {
                    src = src.wrapping_add(step_bytes);
                    dst = dst.wrapping_add(step_bytes);
                }
                i = i.wrapping_add(step);
                cc = cc.wrapping_add(1);
                if step == 0 {
                    break;
                }
            }
            if progress && nsectors != 0 {
                let x = smartcols::tty::terminal_dimension()
                    .0
                    .filter(|&w| w > 0)
                    .unwrap_or(80);
                self.sh.term.write(&vec![b' '; x]);
                self.sh.term.flush();
                self.sh.term.write(b"\r");
                // `i` is past `nsectors` on success, by the last step.
                let i = i.min(nsectors);
                #[allow(
                    clippy::cast_precision_loss,
                    reason = "a percentage on a progress line"
                )]
                let pct = 100.0 / (nsectors as f64 / (i.wrapping_add(1)) as f64);
                self.sh
                    .term
                    .write(format!("Moved {i} from {nsectors} sectors ({pct:.0}%).\n").as_bytes());
            }
            rc = 0;
        }
        if let Some(mut f) = log {
            // `fclose`'s status is not looked at.
            let _ = f.flush();
        }
        if self.noact {
            self.cxt.info(b"Your data has not been moved (--no-act).");
        }
        if ioerr != 0 {
            self.cxt
                .info(format!("{ioerr} I/O errors detected!").as_bytes());
            rc = -EIO;
        } else if rc != 0 {
            warn(
                self.short(),
                &format!("{}: failed to move data", escape_unprintable(&devname)),
                &std::io::Error::from_raw_os_error(rc.wrapping_neg()),
            );
        }
        rc
    }

    /// `write_changes (sf)`: the table written (unless `--no-act`), the data
    /// moved if asked, the kernel told, the device closed.
    fn write_changes(&mut self) -> i32 {
        let mut rc = 0;
        if self.noact {
            self.cxt
                .info(b"The partition table is unchanged (--no-act).");
        } else {
            rc = self.cxt.write_disklabel();
        }
        if rc == 0 && self.movedata {
            if let Some(orig) = self.orig_pa.clone() {
                rc = self
                    .move_partition_data(usize::try_from(self.partno).unwrap_or(usize::MAX), &orig);
            }
        }
        if !self.noact && rc == 0 {
            self.cxt.info(b"\nThe partition table has been altered.");
            if !self.notell {
                // The system may still be busy with an earlier re-read.
                std::thread::sleep(std::time::Duration::from_millis(250));
                // Upstream does not look at the result either.
                let _ = self.cxt.reread_partition_table();
            }
        }
        if rc == 0 {
            rc = self.cxt.deassign_device(self.noact || self.notell);
        }
        rc
    }

    /// `command_list_partitions (sf, argc, argv)`: `--list`.
    fn command_list_partitions(&mut self, args: &[Vec<u8>]) -> i32 {
        self.cxt.enable_listonly(true);
        if args.is_empty() {
            list::print_all_devices_pt(&mut self.cxt, &self.sh.term, &self.fields, self.verify);
            return 0;
        }
        let mut fail: i32 = 0;
        for (i, dev) in args.iter().enumerate() {
            if list::print_device_pt(
                &mut self.cxt,
                &self.sh.term,
                &self.fields,
                dev,
                true,
                self.verify,
                i != 0,
            ) != 0
            {
                fail = fail.wrapping_add(1);
            }
        }
        fail
    }

    /// `command_list_freespace (sf, argc, argv)`: `--list-free`.
    fn command_list_freespace(&mut self, args: &[Vec<u8>]) -> i32 {
        self.cxt.enable_listonly(true);
        if args.is_empty() {
            list::print_all_devices_freespace(&mut self.cxt, &self.sh.term);
            return 0;
        }
        let mut fail: i32 = 0;
        for (i, dev) in args.iter().enumerate() {
            if list::print_device_freespace(&mut self.cxt, &self.sh.term, dev, true, i != 0) != 0 {
                fail = fail.wrapping_add(1);
            }
        }
        fail
    }

    /// `command_list_types (sf)`: `--list-types`, of `--label`'s label or
    /// the MBR's.
    fn command_list_types(&mut self) -> i32 {
        let name = self.label.clone().unwrap_or_else(|| b"dos".to_vec());
        let Some(lb) = self
            .cxt
            .get_label(Some(&name))
            .and_then(|i| self.cxt.label_at(i))
        else {
            self.errx(&format!(
                "unsupported label '{}'",
                escape_unprintable(&name)
            ));
        };
        let codes = lb.has_code_parttypes();
        let mut out = b"Id  Name\n\n".to_vec();
        let mut i = 0;
        while let Some(t) = lb.get_parttype(i) {
            if codes {
                out.extend_from_slice(format!("{:2x}  ", t.code).as_bytes());
            } else {
                out.extend_from_slice(t.get_string().unwrap_or(b"(null)"));
                out.extend_from_slice(b"  ");
            }
            out.extend_from_slice(t.get_name().unwrap_or(b"(null)"));
            out.push(b'\n');
            i = i.wrapping_add(1);
        }
        self.sh.term.write(&out);
        0
    }

    /// `verify_device (sf, devname)`: the device's table checked; 1 when
    /// it has none.
    fn verify_device(&mut self, devname: &[u8]) -> i32 {
        self.cxt.enable_listonly(true);
        self.assign_device(devname, true);
        self.sh.term.color_scheme_enable(b"header", ulcolors::BOLD);
        self.info(&[devname, b":"]);
        self.sh.term.color_disable();
        let rc = if self.cxt.has_label() {
            self.cxt.verify_disklabel()
        } else {
            self.cxt.info(b"unrecognized partition table type");
            1
        };
        // Read-only: nothing to sync.
        let _ = self.cxt.deassign_device(true);
        rc
    }

    /// `command_verify (sf, argc, argv)`: `--verify`.
    fn command_verify(&mut self, args: &[Vec<u8>]) -> i32 {
        let devs = if args.is_empty() {
            list::proc_partitions(self.short())
        } else {
            args.to_vec()
        };
        let mut nfails: i32 = 0;
        for (i, dev) in devs.iter().enumerate() {
            if i != 0 {
                self.cxt.info(b" ");
            }
            if self.verify_device(dev) < 0 {
                nfails = nfails.wrapping_add(1);
            }
        }
        nfails
    }

    /// `get_size (dev, silent, &sz)`: the device's size in 512-byte sectors.
    fn get_size(&self, dev: &[u8], silent: bool) -> Result<u64, i32> {
        let shown = escape_unprintable(dev);
        let f = match std::fs::File::open(ulsysfs::path_of(dev)) {
            Ok(f) => f,
            Err(e) => {
                if !silent {
                    warn(self.short(), &format!("cannot open {shown}"), &e);
                }
                return Err(e.raw_os_error().unwrap_or(0).wrapping_neg());
            }
        };
        ulblkid::blkdev::get_sectors(&f).map_err(|e| {
            if !silent {
                warn(
                    self.short(),
                    &format!("Cannot get size of {shown}"),
                    &std::io::Error::from_raw_os_error(e),
                );
            }
            e.wrapping_neg()
        })
    }

    /// `command_show_size (sf, argc, argv)`: `--show-size`, in 1 KiB blocks.
    fn command_show_size(&mut self, args: &[Vec<u8>]) -> i32 {
        if !args.is_empty() {
            for dev in args {
                if let Ok(sz) = self.get_size(dev, false) {
                    self.sh.term.write(format!("{}\n", sz / 2).as_bytes());
                }
            }
            return 0;
        }
        let mut total: u64 = 0;
        for dev in list::proc_partitions(self.short()) {
            if let Ok(sz) = self.get_size(&dev, true) {
                let mut line = dev.clone();
                line.extend_from_slice(format!(": {:9}\n", sz / 2).as_bytes());
                self.sh.term.write(&line);
                total = total.wrapping_add(sz / 2);
            }
        }
        if total != 0 {
            self.sh
                .term
                .write(format!("total: {total} blocks\n").as_bytes());
        }
        0
    }

    /// `print_geom (sf, devname)`.
    fn print_geom(&mut self, devname: &[u8]) -> i32 {
        self.cxt.enable_listonly(true);
        self.assign_device(devname, true);
        let m = format!(
            ": {} cylinders, {} heads, {} sectors/track",
            self.cxt.get_geom_cylinders(),
            self.cxt.get_geom_heads(),
            self.cxt.get_geom_sectors()
        );
        self.info(&[devname, m.as_bytes()]);
        // Read-only: nothing to sync.
        let _ = self.cxt.deassign_device(true);
        0
    }

    /// `command_show_geometry (sf, argc, argv)`: `--show-geometry`.
    fn command_show_geometry(&mut self, args: &[Vec<u8>]) -> i32 {
        let devs = if args.is_empty() {
            list::proc_partitions(self.short())
        } else {
            args.to_vec()
        };
        let mut nfails: i32 = 0;
        for dev in &devs {
            if self.print_geom(dev) < 0 {
                nfails = nfails.wrapping_add(1);
            }
        }
        nfails
    }

    /// `command_activate (sf, argc, argv)`: `--activate <dev> [<part> ...]`
    /// -- the bootable partitions listed, or made the given ones.
    fn command_activate(&mut self, args: &[Vec<u8>]) -> i32 {
        let Some(devname) = args.first().cloned() else {
            self.errx("no disk device specified")
        };
        let listonly = args.len() == 1;
        self.assign_device(&devname, listonly);

        if self.cxt.is_labeltype(LabelType::Gpt) {
            if libfdisk::gpt::is_hybrid(&mut self.cxt) {
                self.errx("toggle boot flags is unsupported for Hybrid GPT/MBR");
            }
            // From the GPT to its protective MBR.
            let parent = std::mem::replace(&mut self.cxt, Context::empty());
            self.cxt = Context::new_nested(parent, Some(b"dos"));
            self.cxt
                .info(b"Activation is unsupported for GPT -- entering nested PMBR.");
        } else if !self.cxt.is_labeltype(LabelType::Dos) {
            self.errx("toggle boot flags is supported for MBR or PMBR only");
        }

        let nparts = self.cxt.get_npartitions();
        for i in 0..nparts {
            let Ok(mut pa) = self.cxt.get_partition(i) else {
                continue;
            };
            if listonly {
                if !pa.is_bootable() {
                    continue;
                }
                if let Ok(Some(data)) = self.cxt.partition_to_string(&mut pa, fieldtype::DEVICE) {
                    let mut line = data;
                    line.push(b'\n');
                    self.sh.term.write(&line);
                }
            } else if pa.is_bootable() {
                // Every active one made inactive first.
                self.cxt.toggle_partition_flag(i, libfdisk::DOS_FLAG_ACTIVE);
            }
        }

        for (i, arg) in args.iter().enumerate().skip(1) {
            if i == 1 && arg.as_slice() == b"-" {
                break;
            }
            let n = self.strtou32_or_err(arg, "failed to parse partition number");
            let idx = usize::try_from(n).unwrap_or(0).wrapping_sub(1);
            if self
                .cxt
                .toggle_partition_flag(idx, libfdisk::DOS_FLAG_ACTIVE)
                != 0
            {
                // Upstream names the argument's position, not the partition.
                self.errx(&format!(
                    "{}: partition {}: failed to toggle bootable flag",
                    escape_unprintable(&devname),
                    i.wrapping_add(1)
                ));
            }
        }
        if listonly {
            self.cxt.deassign_device(true)
        } else {
            self.write_changes()
        }
    }

    /// `command_delete (sf, argc, argv)`: `--delete <dev> [<part> ...]`.
    fn command_delete(&mut self, args: &[Vec<u8>]) -> i32 {
        let Some(devname) = args.first().cloned() else {
            self.errx("no disk device specified")
        };
        self.assign_device(&devname, false);
        let shown = escape_unprintable(&devname);
        if args.len() == 1 {
            // All of them.
            let nparts = self.cxt.get_npartitions();
            for i in 0..nparts {
                if self.cxt.is_partition_used(i) != 0 && self.cxt.delete_partition(i) != 0 {
                    self.errx(&format!(
                        "{shown}: partition {}: failed to delete",
                        i.wrapping_add(1)
                    ));
                }
            }
        } else {
            for arg in args.iter().skip(1) {
                let n =
                    usize::try_from(self.strtou32_or_err(arg, "failed to parse partition number"))
                        .unwrap_or(0);
                if self.cxt.delete_partition(n.wrapping_sub(1)) != 0 {
                    self.errx(&format!("{shown}: partition {n}: failed to delete"));
                }
            }
        }
        self.write_changes()
    }

    /// `command_reorder (sf, argc, argv)`: `--reorder`.
    fn command_reorder(&mut self, args: &[Vec<u8>]) -> i32 {
        let Some(devname) = args.first().cloned() else {
            self.errx("no disk device specified")
        };
        self.assign_device(&devname, false);
        if self.cxt.reorder_partitions() == 1 {
            // Unchanged.
            self.cxt.deassign_device(true)
        } else {
            self.write_changes()
        }
    }

    /// `command_dump (sf, argc, argv)`: `--dump` and `--json`.
    fn command_dump(&mut self, args: &[Vec<u8>]) -> i32 {
        let Some(devname) = args.first().cloned() else {
            self.errx("no disk device specified")
        };
        self.assign_device(&devname, true);
        let shown = escape_unprintable(&devname);
        if !self.cxt.has_label() {
            self.errx(&format!(
                "{shown}: does not contain a recognized partition table"
            ));
        }
        let mut dp = Script::new();
        if dp.read_context(&mut self.cxt) != 0 {
            self.errx(&format!("{shown}: failed to dump partition table"));
        }
        if self.json {
            dp.enable_json(true);
        }
        let mut out = Vec::new();
        dp.write_file(&self.cxt, &mut out);
        self.sh.term.write(&out);
        // Read-only: nothing to sync.
        let _ = self.cxt.deassign_device(true);
        0
    }

    /// `command_backup_sectors (sf, argc, argv)`: `--backup-pt-sectors`.
    fn command_backup_sectors(&mut self, args: &[Vec<u8>]) -> i32 {
        let Some(devname) = args.first().cloned() else {
            self.errx("no disk device specified")
        };
        self.assign_device(&devname, true);
        if !self.cxt.has_label() {
            self.errx(&format!(
                "{}: does not contain a recognized partition table",
                escape_unprintable(&devname)
            ));
        }
        self.backup_partition_table(&devname);
        // Read-only: nothing to sync.
        let _ = self.cxt.deassign_device(true);
        0
    }

    /// `assign_device_partition (sf, devname, partno, rdonly)`: the device
    /// opened, and partition `partno` (from 1) checked to be there.
    fn assign_device_partition(&mut self, devname: &[u8], partno: usize, rdonly: bool) {
        let shown = escape_unprintable(devname);
        let rc = self.cxt.assign_device(devname, rdonly);
        if rc != 0 {
            self.err(&format!("cannot open {shown}"), rc.wrapping_neg());
        }
        if !self.cxt.is_readonly() {
            let locked = self.cxt.get_dev().cloned().map_or(0, |dev| {
                ulblkid::blkdev::lock(&dev, devname, self.lockmode.as_deref(), self.short())
            });
            if locked != 0 {
                // Upstream goes on with the device closed.
                let _ = self.cxt.deassign_device(true);
                return;
            }
        }
        if self.cxt.label_ref().is_none() {
            self.errx(&format!("{shown}: no partition table found"));
        }
        let n = self.cxt.get_npartitions();
        if partno > n {
            self.errx(&format!(
                "{shown}: partition {partno}: partition table contains only {n} partitions"
            ));
        }
        if self.cxt.is_partition_used(partno.wrapping_sub(1)) == 0 {
            self.errx(&format!("{shown}: partition {partno}: partition is unused"));
        }
    }

    /// The device and partition number of `--part-*`: `<dev> <part>
    /// [<value>]`.
    fn part_args(&self, args: &[Vec<u8>]) -> (Vec<u8>, usize, Option<Vec<u8>>) {
        let Some(devname) = args.first().cloned() else {
            self.errx("no disk device specified")
        };
        let Some(p) = args.get(1) else {
            self.errx("no partition number specified")
        };
        let partno = usize::try_from(self.strtou32_or_err(p, "failed to parse partition number"))
            .unwrap_or(0);
        if args.len() > 3 {
            self.errx("unexpected arguments");
        }
        (devname, partno, args.get(2).cloned())
    }

    /// `command_parttype (sf, argc, argv)`: `--part-type`.
    fn command_parttype(&mut self, args: &[Vec<u8>]) -> i32 {
        let (devname, partno, typestr) = self.part_args(args);
        let shown = escape_unprintable(&devname);
        // Read-only without a new type.
        self.assign_device_partition(&devname, partno, typestr.is_none());

        let Some(typestr) = typestr else {
            let t = self
                .cxt
                .get_partition(partno.wrapping_sub(1))
                .ok()
                .and_then(|pa| pa.ty);
            let Some(t) = t else {
                self.errx(&format!(
                    "{shown}: partition {partno}: failed to get partition type"
                ));
            };
            let codes = self
                .cxt
                .label_ref()
                .is_some_and(libfdisk::Label::has_code_parttypes);
            if codes {
                self.sh.term.write(format!("{:2x}\n", t.code).as_bytes());
            } else {
                self.sh.term.write(t.get_string().unwrap_or(b"(null)"));
                self.sh.term.write(b"\n");
            }
            // Read-only: nothing to sync.
            let _ = self.cxt.deassign_device(true);
            return 0;
        };
        if self.backup {
            self.backup_partition_table(&devname);
        }
        let ty = self.cxt.label_ref().and_then(|lb| {
            lb.advparse_parttype(
                &typestr,
                PARSE_DATA | PARSE_ALIAS | PARSE_NAME | PARSE_SHORTCUT,
            )
        });
        let Some(ty) = ty else {
            let lname = self.cxt.label_ref().map_or("", |l| l.get_name());
            self.errx(&format!(
                "failed to parse {lname} partition type '{}'",
                escape_unprintable(&typestr)
            ));
        };
        if self.cxt.set_partition_type(partno.wrapping_sub(1), &ty) != 0 {
            self.errx(&format!(
                "{shown}: partition {partno}: failed to set partition type"
            ));
        }
        self.write_changes()
    }

    /// `command_partuuid`, `command_partlabel` and `command_partattrs`:
    /// `--part-uuid`, `--part-label` and `--part-attrs`, which print a
    /// partition's field or set it.
    fn command_partfield(&mut self, args: &[Vec<u8>], what: Act) -> i32 {
        let (devname, partno, value) = self.part_args(args);
        let shown = escape_unprintable(&devname);
        let noun = match what {
            Act::PartUuid => "partition UUID",
            Act::PartLabel => "partition name",
            _ => "partition attributes",
        };
        self.assign_device_partition(&devname, partno, value.is_none());

        let Some(value) = value else {
            let pa = self.cxt.get_partition(partno.wrapping_sub(1)).ok();
            let field = pa.as_ref().and_then(|pa| match what {
                Act::PartUuid => pa.get_uuid(),
                Act::PartLabel => pa.get_name(),
                _ => pa.get_attrs(),
            });
            match field {
                Some(s) => {
                    let mut line = s.to_vec();
                    line.push(b'\n');
                    self.sh.term.write(&line);
                }
                // No attributes is nothing to print; no UUID or name is a
                // failure.
                None if what == Act::PartAttrs => {}
                None => self.errx(&format!(
                    "{shown}: partition {partno}: failed to get {noun}"
                )),
            }
            // Read-only: nothing to sync.
            let _ = self.cxt.deassign_device(true);
            return 0;
        };
        if self.backup {
            self.backup_partition_table(&devname);
        }
        let mut pa = Partition::new();
        match what {
            Act::PartUuid => pa.set_uuid(Some(&value)),
            Act::PartLabel => pa.set_name(Some(&value)),
            _ => pa.set_attrs(Some(&value)),
        }
        if self.cxt.set_partition(partno.wrapping_sub(1), &mut pa) != 0 {
            self.errx(&format!(
                "{shown}: partition {partno}: failed to set {noun}"
            ));
        }
        self.write_changes()
    }

    /// `command_diskid (sf, argc, argv)`: `--disk-id`.
    fn command_diskid(&mut self, args: &[Vec<u8>]) -> i32 {
        let Some(devname) = args.first().cloned() else {
            self.errx("no disk device specified")
        };
        if args.len() > 2 {
            self.errx("unexpected arguments");
        }
        let id = args.get(1).cloned();
        self.assign_device(&devname, id.is_none());
        let Some(id) = id else {
            if let Ok(Some(s)) = self.cxt.get_disklabel_id() {
                let mut line = s;
                line.push(b'\n');
                self.sh.term.write(&line);
            }
            // Read-only: nothing to sync.
            let _ = self.cxt.deassign_device(true);
            return 0;
        };
        if self.cxt.set_disklabel_id_from_string(&id) != 0 {
            self.errx(&format!(
                "{}: failed to set disklabel ID",
                escape_unprintable(&devname)
            ));
        }
        self.write_changes()
    }

    /// `command_relocate (sf, argc, argv)`: `--relocate gpt-bak-std|gpt-bak-mini`.
    fn command_relocate(&mut self, args: &[Vec<u8>]) -> i32 {
        let [oper, devname] = args else {
            match args.len() {
                0 => self.errx("no relocate operation specified"),
                1 => self.errx("no disk device specified"),
                _ => self.errx("unexpected arguments"),
            }
        };
        let gpt = self.cxt.get_label(Some(b"gpt"));
        match oper.as_slice() {
            b"gpt-bak-mini" => {
                if let Some(lb) = gpt.and_then(|i| self.cxt.label_at_mut(i)) {
                    libfdisk::gpt::enable_minimize(lb, true);
                }
            }
            b"gpt-bak-std" => {}
            _ => self.errx("unsupported relocation operation"),
        }
        let devname = devname.clone();
        self.assign_device(&devname, false);
        if let Some(lb) = gpt.and_then(|i| self.cxt.label_at_mut(i)) {
            lb.set_changed(true);
        }
        self.write_changes()
    }

    /// `sfdisk_print_partition (sf, n)`: a partition just added, a line.
    fn sfdisk_print_partition(&mut self, n: usize) {
        if self.sh.quiet.get() {
            return;
        }
        let Ok(mut pa) = self.cxt.get_partition(n) else {
            return;
        };
        let mut field = |cxt: &mut Context, id: i32| -> String {
            let d = cxt
                .partition_to_string(&mut pa, id)
                .ok()
                .flatten()
                .unwrap_or_else(|| b"(null)".to_vec());
            String::from_utf8_lossy(&d).into_owned()
        };
        let line = format!(
            "{:>12} : {:>12} {:>12} ({}) {}\n",
            field(&mut self.cxt, fieldtype::DEVICE),
            field(&mut self.cxt, fieldtype::START),
            field(&mut self.cxt, fieldtype::END),
            field(&mut self.cxt, fieldtype::SIZE),
            field(&mut self.cxt, fieldtype::TYPE)
        );
        self.sh.term.write(line.as_bytes());
    }

    /// `command_fdisk_help ()`: the shell's commands and input format.
    fn command_fdisk_help(&self) {
        let t = &self.sh.term;
        let title = |s: &[u8]| {
            t.write(b"\n");
            t.color_scheme_enable(b"help-title", ulcolors::BOLD);
            t.write(s);
            t.color_disable();
        };
        t.write(b"\nHelp:\n");
        title(b" Commands:\n");
        t.write(
            b"   write    write table to disk and exit\n   quit     show new situation and wait for user's feedback \
              before write\n   abort    exit sfdisk shell\n   print    display the partition table\n   help     \
              show this help text\n\n   Ctrl-D   the same as 'quit'\n",
        );
        title(b" Input format:\n");
        t.write(
            b"   <start>, <size>, <type>, <bootable>\n\n   <start>  Beginning of the partition in sectors, or bytes \
              if\n            specified in the format <number>{K,M,G,T,P,E,Z,Y}.\n            The default is the \
              first free space.\n\n   <size>   Size of the partition in sectors, or bytes if\n            specified \
              in the format <number>{K,M,G,T,P,E,Z,Y}.\n            The default is all available space.\n\n   \
              <type>   The partition type.  Default is a Linux data partition.\n            MBR: hex or \
              L,S,Ex,X,U,R,V shortcuts.\n            GPT: UUID or L,S,H,U,R,V shortcuts.\n\n   <bootable>  Use '*' \
              to mark an MBR partition as bootable.\n",
        );
        title(b" Example:\n");
        t.write(b"   , 4G     Creates a 4GiB partition at default start offset.\n\n");
    }

    /// `loop_control_commands (sf, dp, buf)`: a line the script parser
    /// refused, as a shell command -- `SFDISK_DONE_*`, or `-EINVAL` for an
    /// unknown one in a script.
    fn loop_control_commands(&mut self, nlines: usize, buf: &[u8]) -> i32 {
        let line = libfdisk::utils::c_str(buf);
        let p = line
            .get(
                line.iter()
                    .take_while(|&&c| c == b' ' || c == b'\t')
                    .count()..,
            )
            .unwrap_or_default();
        match p {
            b"print" => {
                list::list_disklabel(&mut self.cxt, &self.sh.term, &self.fields);
                SFDISK_DONE_NONE
            }
            b"help" => {
                self.command_fdisk_help();
                SFDISK_DONE_NONE
            }
            b"quit" => SFDISK_DONE_ASK,
            b"write" => SFDISK_DONE_WRITE,
            b"abort" => SFDISK_DONE_ABORT,
            _ if self.interactive => {
                self.cxt.warnx(b"unsupported command");
                SFDISK_DONE_NONE
            }
            _ => {
                self.cxt.warnx(
                    format!(
                        "line {}: unsupported command",
                        i32::try_from(nlines).unwrap_or(i32::MAX)
                    )
                    .as_bytes(),
                );
                -EINVAL
            }
        }
    }

    /// `has_container_or_unused (sf)`: whether the table has an extended
    /// partition or an unused slot -- room for one more.
    fn has_container_or_unused(&mut self) -> bool {
        if self.container || self.unused {
            return true;
        }
        let nparts = self.cxt.get_npartitions();
        for i in 0..nparts {
            if self.cxt.is_partition_used(i) == 0 {
                self.unused = true;
                continue;
            }
            if self.cxt.get_partition(i).is_ok_and(|pa| pa.is_container()) {
                self.container = true;
            }
        }
        self.container || self.unused
    }

    /// `last_pt_partno (sf)`: the highest partition number in use.
    fn last_pt_partno(&mut self) -> usize {
        let nparts = self.cxt.get_npartitions();
        (0..nparts)
            .filter_map(|i| self.cxt.get_partition(i).ok())
            .filter(|pa| pa.used)
            .map(|pa| pa.partno)
            .max()
            .unwrap_or(0)
    }

    /// `follow_wipe_mode (sf)`.
    fn follow_wipe_mode(&mut self) {
        let mut dowipe = self.wipemode == WIPEMODE_ALWAYS;
        if self.interactive && self.wipemode == WIPEMODE_AUTO {
            // In interactive mode.
            dowipe = true;
        }
        if self.cxt.is_ptcollision() && self.wipemode != WIPEMODE_NEVER {
            // An old partition table always goes.
            dowipe = true;
        }
        self.cxt.enable_wipe(dowipe);
        if self.sh.quiet.get() {
            return;
        }
        let collision = self.cxt.get_collision().unwrap_or(b"(null)").to_vec();
        if dowipe {
            if !self.cxt.is_ptcollision() {
                let mut m = b"The device contains '".to_vec();
                m.extend_from_slice(&collision);
                m.extend_from_slice(
                    b"' signature and it may be removed by a write command. See sfdisk(8) man page and --wipe option \
                      for more details.",
                );
                self.cxt.warnx(&m);
                self.sh.term.write(b"\n");
            }
        } else {
            let mut m = b"The device contains '".to_vec();
            m.extend_from_slice(&collision);
            m.extend_from_slice(
                b"' signature and it may remain on the device. It is recommended to wipe the device with wipefs(8) \
                  or sfdisk --wipe, in order to avoid possible collisions.",
            );
            self.cxt.warnx(&m);
            stderr_write(b"\n");
        }
    }

    /// `wipe_partition (sf, partno)`: a new partition's old signature, wiped
    /// by the write if the user (or `--wipe-partitions`) says so.
    fn wipe_partition(&mut self, partno: usize) -> i32 {
        let mut tmp = match self.cxt.get_partition(partno) {
            Ok(pa) => pa,
            Err(rc) => return rc,
        };
        let fstype = match self.cxt.partition_to_string(&mut tmp, fieldtype::FSTYPE) {
            Ok(Some(t)) => t,
            Ok(None) => return 0,
            Err(rc) => return rc,
        };
        let mut m = format!("Partition #{} contains a ", partno.wrapping_add(1)).into_bytes();
        m.extend_from_slice(&fstype);
        m.extend_from_slice(b" signature.");
        self.cxt.warnx(&m);
        let yes = if self.pwipemode == WIPEMODE_AUTO && std::io::stdin().is_terminal() {
            self.cxt
                .ask_yesno(b"Do you want to remove the signature?")
                .unwrap_or(false)
        } else {
            self.pwipemode == WIPEMODE_ALWAYS
        };
        if yes {
            self.cxt
                .info(b"The signature will be removed by a write command.");
            return self.cxt.wipe_partition(partno, true);
        }
        0
    }

    /// `refresh_prompt_buffer (sf, devname, next_partno, created)`: the
    /// next partition's name once a table exists, `>>> ` before.
    fn refresh_prompt_buffer(&mut self, devname: &[u8], next_partno: usize, created: bool) {
        if created {
            let Some(pname) = partname(Some(devname), next_partno.wrapping_add(1)) else {
                self.err("failed to allocate partition name", libfdisk::errno::ENOMEM);
            };
            if !self
                .prompt
                .as_deref()
                .is_some_and(|p| p.starts_with(&pname))
            {
                self.prompt = Some([&pname[..], b": "].concat());
            }
        } else if !self
            .prompt
            .as_deref()
            .is_some_and(|p| p.starts_with(SFDISK_PROMPT))
        {
            self.prompt = Some(SFDISK_PROMPT.to_vec());
        }
    }

    /// `command_fdisk (sf, argc, argv)`: `sfdisk <dev> [[-N] <part>]` -- a
    /// table, or one partition, made from a script read line by line, shown
    /// and written.
    #[allow(
        clippy::too_many_lines,
        reason = "upstream's function, kept in one piece so it can be read against it"
    )]
    fn command_fdisk(&mut self, args: &[Vec<u8>]) -> i32 {
        let mut partno = self.partno;
        if partno < 0 && args.len() > 1 {
            let n = self.strtou32_or_err(
                args.get(1).map_or(&[][..], Vec::as_slice),
                "failed to parse partition number",
            );
            partno = i32::from_ne_bytes(n.to_ne_bytes());
        }
        let Some(devname) = args.first().cloned() else {
            self.errx("no disk device specified")
        };
        let shown = escape_unprintable(&devname);
        self.assign_device(&devname, false);

        let dp: ScriptRef = Rc::new(RefCell::new(Script::new()));
        self.cxt.set_script(Some(Rc::clone(&dp)));
        let mut input = StdinInput::default();

        let mut created = false;
        let mut unused = false;
        let mut ignored: usize = 0;
        let mut next_partno: Option<usize> = None;
        // `-N <part>` changes that partition of the table that is there.
        if let Ok(p) = usize::try_from(partno) {
            if !self.cxt.has_label() {
                self.errx(&format!(
                    "{shown}: cannot modify partition {}: no partition table was found",
                    partno.wrapping_add(1)
                ));
            }
            let n = self.cxt.get_npartitions();
            if p > n {
                self.errx(&format!(
                    "{shown}: cannot modify partition {}: partition table contains only {n} partitions",
                    partno.wrapping_add(1)
                ));
            }
            if self.cxt.is_partition_used(p) == 0 {
                let m = format!(
                    "warning: {}: partition {} is not defined yet",
                    escape_unprintable(&devname),
                    partno.wrapping_add(1)
                );
                self.cxt.warnx(m.as_bytes());
                unused = true;
            }
            created = true;
            next_partno = Some(p);
            if self.movedata {
                self.orig_pa = self.get_partition(p);
            }
        }
        if self.append {
            created = true;
            next_partno = Some(self.last_pt_partno().wrapping_add(1));
        }

        let quiet = self.sh.quiet.get();
        if !quiet && self.interactive {
            self.sh
                .term
                .color_scheme_enable(b"welcome", ulcolors::GREEN);
            self.cxt
                .info(format!("\nWelcome to sfdisk ({PACKAGE_STRING}).").as_bytes());
            self.sh.term.color_disable();
            self.cxt.info(
                b"Changes will remain in memory only, until you decide to write them.\nBe careful before using the \
                  write command.\n",
            );
        }

        if !self.noact && !self.noreread {
            if !quiet {
                self.sh
                    .term
                    .write(b"Checking that no-one is using this disk right now ...");
            }
            if self.cxt.device_is_used() {
                if !quiet {
                    self.sh.term.write(b" FAILED\n\n");
                }
                self.cxt.warnx(
                    b"This disk is currently in use - repartitioning is probably a bad idea.\nUmount all file \
                      systems, and swapoff all swap partitions on this disk.\nUse the --no-reread flag to suppress \
                      this check.\n",
                );
                if !self.force {
                    self.errx("Use the --force flag to overrule all checks.");
                }
            } else if !quiet {
                self.sh.term.write(b" OK\n\n");
            }
        }

        if self.cxt.get_collision().is_some() {
            self.follow_wipe_mode();
        }
        if !quiet {
            list::list_disk_geometry(&mut self.cxt, &self.sh.term);
            if self.cxt.has_label() {
                self.cxt.info(b"\nOld situation:");
                list::list_disklabel(&mut self.cxt, &self.sh.term, &self.fields);
            }
        }

        let label: Vec<u8> = match &self.label {
            Some(l) => l.clone(),
            None if self.cxt.has_label() => self
                .cxt
                .label_ref()
                .map_or("", |l| l.get_name())
                .as_bytes()
                .to_vec(),
            // Just for backward compatibility.
            None => b"dos".to_vec(),
        };
        if dp.borrow_mut().set_header(b"label", Some(&label)) != 0 {
            self.errx("failed to set script header");
        }

        if !quiet && self.interactive {
            if !self.cxt.has_label() && self.label.is_none() {
                let mut m = b"\nsfdisk is going to create a new '".to_vec();
                m.extend_from_slice(&label);
                m.extend_from_slice(
                    b"' disk label.\nUse 'label: <name>' before you define a first partition\nto override the default.",
                );
                self.cxt.info(&m);
            }
            self.cxt.info(b"\nType 'help' to get more information.\n");
        } else if !quiet {
            self.sh.term.write(b"\n");
        }

        let mut buf = vec![0u8; BUFSIZ];
        let mut rc;
        loop {
            let nents = dp.borrow_mut().get_table().get_nents();
            let next = *next_partno.get_or_insert(nents);
            if created
                && partno < 0
                && next == self.cxt.get_npartitions()
                && !self.has_container_or_unused()
            {
                self.cxt.info(b"All partitions used.");
                rc = SFDISK_DONE_ASK;
                break;
            }

            self.refresh_prompt_buffer(&devname, next, created);
            if let Some(prompt) = self.prompt.clone()
                && (self.interactive || !quiet)
            {
                // Upstream's readline writes the prompt on a terminal; with
                // no readline, it is written here as on a pipe.
                self.sh.term.write(&prompt);
            }

            rc = dp
                .borrow_mut()
                .read_line(&mut self.cxt, &mut input, &mut buf);
            if rc == -ENOTSUP {
                if let Some(last) = buf.last_mut() {
                    *last = 0;
                }
                let mut m = b"Unknown script header '".to_vec();
                m.extend_from_slice(libfdisk::utils::c_str(&buf));
                m.extend_from_slice(b"' -- ignore.");
                self.cxt.warnx(&m);
                continue;
            }
            if rc < 0 {
                // The parser refused it: one of the shell's commands?
                if let Some(last) = buf.last_mut() {
                    *last = 0;
                }
                let nlines = dp.borrow().get_nlines();
                rc = self.loop_control_commands(nlines, &buf);
                if rc != 0 {
                    break;
                }
                continue;
            }
            if rc == 1 {
                rc = SFDISK_DONE_EOF;
                if !quiet {
                    self.sh.term.write(b"Done.\n");
                }
                break;
            }

            let nparts = dp.borrow_mut().get_table().get_nents();
            if nparts == 0 {
                self.cxt.info(b"Script header accepted.");
            } else {
                let mut cur_partno: usize = usize::MAX;
                let Some(mut pa) = dp
                    .borrow_mut()
                    .get_table()
                    .get_partition(nparts.wrapping_sub(1))
                    .cloned()
                else {
                    rc = SFDISK_DONE_ABORT;
                    break;
                };
                if ignore_partition(&pa) {
                    self.cxt.info(b"Ignoring partition.");
                    next_partno = Some(next.wrapping_add(1));
                    ignored = ignored.wrapping_add(1);
                    continue;
                }
                if !created {
                    // A new label.
                    rc = self.cxt.apply_script_headers(&dp);
                    created = rc == 0;
                    if rc != 0 {
                        self.cxt.warn(
                            rc.wrapping_neg(),
                            b"Failed to apply script headers, disk label not created",
                        );
                    }
                    if rc == 0 && self.cxt.get_collision().is_some() {
                        self.follow_wipe_mode();
                    }
                }
                if rc == 0 {
                    if let Ok(p) = usize::try_from(partno) {
                        // `-N <part>`: that partition changed.
                        rc = self.cxt.set_partition(p, &mut pa);
                        rc = if rc == 0 {
                            SFDISK_DONE_ASK
                        } else {
                            SFDISK_DONE_ABORT
                        };
                        break;
                    }
                    // A partition added.
                    if !self.interactive
                        && !quiet
                        && self
                            .prompt
                            .as_deref()
                            .is_none_or(|p| p.starts_with(SFDISK_PROMPT))
                    {
                        self.refresh_prompt_buffer(&devname, next, created);
                        if let Some(prompt) = self.prompt.clone() {
                            self.sh.term.write(&prompt);
                        }
                    }
                    rc = self.cxt.add_partition(Some(&mut pa), Some(&mut cur_partno));
                    if rc != 0 {
                        let m = format!("Failed to add #{} partition", next.wrapping_add(1));
                        self.cxt.warn(rc.wrapping_neg(), m.as_bytes());
                    }
                }
                // A new partition's signature (or, with `-N`, an unused
                // one's) checked.
                if rc == 0 && (unused || partno < 0) {
                    let at = if unused {
                        usize::try_from(partno).unwrap_or(usize::MAX)
                    } else {
                        cur_partno
                    };
                    rc = self.wipe_partition(at);
                }
                if rc == 0 {
                    // Its result shown.
                    if self.interactive {
                        self.sfdisk_print_partition(cur_partno);
                    }
                    next_partno = Some(cur_partno.wrapping_add(1));
                } else {
                    // Dropped from the script.
                    dp.borrow_mut()
                        .get_table()
                        .remove_partition(nparts.wrapping_sub(1));
                }
            }
            if rc != 0 && !self.interactive {
                rc = SFDISK_DONE_ABORT;
                break;
            }
        }

        // An empty label, for a script that names one and adds nothing.
        if (rc == SFDISK_DONE_EOF || rc == SFDISK_DONE_WRITE)
            && !created
            && dp.borrow().has_force_label()
            && dp.borrow_mut().get_table().get_nents() == ignored
            && dp.borrow().get_header(b"label").is_some()
            && self.cxt.apply_script_headers(&dp) != 0
        {
            self.cxt
                .warnx(b"Failed to apply script headers, disk label not created.");
            rc = SFDISK_DONE_ABORT;
        }

        if !quiet && rc != SFDISK_DONE_ABORT {
            self.cxt.info(b"\nNew situation:");
            list::list_disk_identifier(&mut self.cxt);
            list::list_disklabel(&mut self.cxt, &self.sh.term, &self.fields);
        }

        match rc {
            SFDISK_DONE_ASK | SFDISK_DONE_EOF | SFDISK_DONE_WRITE => {
                let leave = rc != SFDISK_DONE_WRITE
                    && self.interactive
                    && !self
                        .cxt
                        .ask_yesno(b"Do you want to write this to disk?")
                        .unwrap_or(false);
                if leave {
                    self.cxt.info(b"Leaving.");
                    rc = 0;
                } else {
                    rc = self.write_changes();
                }
            }
            // Aborted, or an error.
            _ => {
                self.cxt.info(b"Leaving.\n");
            }
        }
        self.cxt.set_script(None);
        rc
    }
}

/// `ignore_partition (pa)`: a script line that sets no start or no size
/// and no default for it -- or an old sfdisk's `start=0 size=0`.
fn ignore_partition(pa: &Partition) -> bool {
    (!pa.has_start() && !pa.start_is_default())
        || (!pa.has_size() && !pa.end_is_default())
        || (pa.has_start() && pa.start == 0 && pa.has_size() && pa.size == 0)
}

/// `usage ()`.
fn usage(short: &[u8]) -> Vec<u8> {
    let p = String::from_utf8_lossy(short).into_owned();
    let mut out = format!(
        "\nUsage:\n {p} [options] <dev> [[-N] <part>]\n {p} [options] <command>\n\nDisplay or manipulate a disk \
         partition table.\n\nCommands:\n"
    );
    out.push_str(
        " -A, --activate <dev> [<part> ...] list or set bootable (P)MBR partitions\n -d, --dump <dev>                  \
         dump partition table (usable for later input)\n -J, --json <dev>                  dump partition table in \
         JSON format\n -B, --backup-pt-sectors <dev>     binary partition table backup (see -b and -O)\n -g, \
         --show-geometry [<dev> ...]   list geometry of all or specified devices\n -l, --list [<dev> ...]            \
         list partitions of each device\n -F, --list-free [<dev> ...]       list unpartitioned free areas of each \
         device\n -r, --reorder <dev>               fix partitions order (by start offset)\n -s, --show-size [<dev> \
         ...]       list sizes of all or specified devices\n -T, --list-types                  print the recognized \
         types (see -X)\n -V, --verify [<dev> ...]          test whether partitions seem correct\n     --delete \
         <dev> [<part> ...]   delete all or specified partitions\n\n --part-label <dev> <part> [<str>] print or \
         change partition label\n --part-type <dev> <part> [<type>] print or change partition type\n --part-uuid \
         <dev> <part> [<uuid>] print or change partition uuid\n --part-attrs <dev> <part> [<str>] print or change \
         partition attributes\n\n --disk-id <dev> [<str>]           print or change disk label ID (UUID)\n \
         --relocate <oper> <dev>           move partition header\n\nArguments:\n <dev>                     device \
         (usually disk) path\n <part>                    partition number\n <type>                    partition \
         type, GUID for GPT, hex for MBR\n\nOptions:\n -a, --append              append partitions to existing \
         partition table\n -b, --backup              backup partition table sectors (see -O)\n     --bytes        \
         \x20      print SIZE in bytes rather than in human readable format\n     --move-data[=<typescript>] move \
         partition data after relocation (requires -N)\n     --move-use-fsync      use fsync after each write when \
         move data\n -f, --force               disable all consistency checking\n     --color[=<when>]      \
         colorize output (auto, always or never)\n",
    );
    out.push_str(&format!(
        "                             {}\n",
        ulcolors::usage_colors_default()
    ));
    out.push_str(
        "     --lock[=<mode>]       use exclusive device lock (yes, no or nonblock)\n -N, --partno <num>        \
         specify partition number\n -n, --no-act              do everything except write to device\n     \
         --no-reread           do not check whether the device is in use\n     --no-tell-kernel      do not tell \
         kernel about changes\n -O, --backup-file <path>  override default backup file name\n -o, --output <list> \
         \x20     output columns\n -q, --quiet               suppress extra info messages\n -w, --wipe <mode>         \
         wipe signatures (auto, always or never)\n -W, --wipe-partitions <mode>  wipe signatures from new \
         partitions (auto, always or never)\n -X, --label <name>        specify label type (dos, gpt, ...)\n -Y, \
         --label-nested <name> specify nested label type (dos, bsd)\n\n -G, --show-pt-geometry    deprecated, alias \
         to --show-geometry\n -L, --Linux               deprecated, only for backward compatibility\n -u, --unit S  \
         \x20           deprecated, only sector unit is supported\n\n -h, --help                display this help\n \
         -v, --version             display version\n",
    );
    let mut out = out.into_bytes();
    list::list_available_columns(&mut out);
    out.extend_from_slice(b"\nFor more details see sfdisk(8).\n");
    out
}

/// The program, as invoked by `sfdisk`.
const SFDISK: Program = Program::new("sfdisk", 1);

/// Upstream's option string.
const SHORTS: &str = "aAbBcdfFgGhJlLo:O:nN:qrsTu:vVX:Y:w:W:";

/// `OPT_CHANGE_ID` and the rest: `CHAR_MAX + 1` on.
const OPT_CHANGE_ID: i32 = 128;
const OPT_PRINT_ID: i32 = 129;
const OPT_NOREREAD: i32 = 131;
const OPT_PARTUUID: i32 = 132;
const OPT_PARTLABEL: i32 = 133;
const OPT_PARTTYPE: i32 = 134;
const OPT_PARTATTRS: i32 = 135;
const OPT_DISKID: i32 = 136;
const OPT_BYTES: i32 = 137;
const OPT_COLOR: i32 = 138;
const OPT_MOVEDATA: i32 = 139;
const OPT_MOVEFSYNC: i32 = 140;
const OPT_DELETE: i32 = 141;
const OPT_NOTELL: i32 = 142;
const OPT_RELOCATE: i32 = 143;
const OPT_LOCK: i32 = 144;

/// Upstream's `longopts[]`, in its order.
const LONGS: &[(&str, Takes)] = &[
    ("activate", Takes::Nothing),
    ("append", Takes::Nothing),
    ("backup-pt-sectors", Takes::Nothing),
    ("backup", Takes::Nothing),
    ("backup-file", Takes::Required),
    ("bytes", Takes::Nothing),
    ("color", Takes::Optional),
    ("lock", Takes::Optional),
    ("delete", Takes::Nothing),
    ("dump", Takes::Nothing),
    ("help", Takes::Nothing),
    ("force", Takes::Nothing),
    ("json", Takes::Nothing),
    ("label", Takes::Required),
    ("label-nested", Takes::Required),
    ("list", Takes::Nothing),
    ("list-free", Takes::Nothing),
    ("list-types", Takes::Nothing),
    ("no-act", Takes::Nothing),
    ("no-reread", Takes::Nothing),
    ("no-tell-kernel", Takes::Nothing),
    ("move-data", Takes::Optional),
    ("move-use-fsync", Takes::Nothing),
    ("output", Takes::Required),
    ("partno", Takes::Required),
    ("reorder", Takes::Nothing),
    ("show-geometry", Takes::Nothing),
    ("quiet", Takes::Nothing),
    ("verify", Takes::Nothing),
    ("version", Takes::Nothing),
    ("wipe", Takes::Required),
    ("wipe-partitions", Takes::Required),
    ("relocate", Takes::Nothing),
    ("part-uuid", Takes::Nothing),
    ("part-label", Takes::Nothing),
    ("part-type", Takes::Nothing),
    ("part-attrs", Takes::Nothing),
    ("disk-id", Takes::Nothing),
    ("show-pt-geometry", Takes::Nothing),
    ("unit", Takes::Required),
    ("Linux", Takes::Nothing),
    ("show-size", Takes::Nothing),
    ("change-id", Takes::Nothing),
    ("id", Takes::Nothing),
    ("print-id", Takes::Nothing),
];
/// Each one's `val`.
const LONG_VALS: [i32; 45] = [
    b'A' as i32,
    b'a' as i32,
    b'B' as i32,
    b'b' as i32,
    b'O' as i32,
    OPT_BYTES,
    OPT_COLOR,
    OPT_LOCK,
    OPT_DELETE,
    b'd' as i32,
    b'h' as i32,
    b'f' as i32,
    b'J' as i32,
    b'X' as i32,
    b'Y' as i32,
    b'l' as i32,
    b'F' as i32,
    b'T' as i32,
    b'n' as i32,
    OPT_NOREREAD,
    OPT_NOTELL,
    OPT_MOVEDATA,
    OPT_MOVEFSYNC,
    b'o' as i32,
    b'N' as i32,
    b'r' as i32,
    b'g' as i32,
    b'q' as i32,
    b'V' as i32,
    b'v' as i32,
    b'w' as i32,
    b'W' as i32,
    OPT_RELOCATE,
    OPT_PARTUUID,
    OPT_PARTLABEL,
    OPT_PARTTYPE,
    OPT_PARTATTRS,
    OPT_DISKID,
    b'G' as i32,
    b'u' as i32,
    b'L' as i32,
    b's' as i32,
    OPT_CHANGE_ID,
    b'c' as i32,
    OPT_PRINT_ID,
];
/// [`LONGS`] and [`LONG_VALS`] as one table.
const OPTIONS: LongVals<'static, i32> = LongVals::new(LONGS, &LONG_VALS);

/// `excl[]`: the options that exclude each other, rows and columns in
/// ASCII order.
const EXCL: [&[i32]; 3] = [
    &[b'F' as i32, b'd' as i32],
    &[b'F' as i32, b'J' as i32],
    &[b's' as i32, b'u' as i32],
];

/// The program's name, as `program_invocation_short_name`.
fn short_name(arg0: &OsStr) -> Vec<u8> {
    basename(&os_bytes(arg0)).to_vec()
}

impl Sfdisk {
    /// `main ()`, from its options on: the exit status.
    #[allow(
        clippy::too_many_lines,
        reason = "upstream's main, kept in one piece so it can be read against it"
    )]
    fn run(&mut self, argv: &[OsString]) -> u8 {
        let arg0 = argv
            .first()
            .map_or(OsStr::new("sfdisk"), OsString::as_os_str);
        let mut colormode = ulcolors::ColorMode::Undef;
        let mut outarg: Option<Vec<u8>> = None;
        let mut bytes = false;
        let mut excl_st = [0i32; 3];
        let mut operands: Vec<Vec<u8>> = Vec::new();

        let own = argv.get(1..).unwrap_or_default();
        for item in SFDISK.parse(own, SHORTS, LONGS) {
            let opt = match item {
                Ok(opt) => opt,
                Err(e) => {
                    // glibc names the program by argv[0] as given.
                    stderr_write(
                        format!("{}: {}\n", escape_unprintable(&os_bytes(arg0)), e.sentence)
                            .as_bytes(),
                    );
                    stderr_write(
                        format!(
                            "Try '{} --help' for more information.\n",
                            escape_unprintable(self.short())
                        )
                        .as_bytes(),
                    );
                    return 1;
                }
            };
            let Some((c, value)) = OPTIONS.code::<i32>(&opt) else {
                if let Opt::Operand(w) = opt {
                    operands.push(arg_bytes(w));
                }
                continue;
            };
            if let Some(msg) = ulstrutils::err_exclusive_options(
                c,
                &EXCL,
                &mut excl_st,
                |c| OPTIONS.name_of(c),
                self.short(),
            ) {
                stderr_write(msg.as_bytes());
                return 1;
            }
            let argb = value.as_deref().map(arg_bytes).unwrap_or_default();
            let ch = |b: u8| i32::from(b);
            match c {
                c if c == ch(b'A') => self.act = Act::Activate,
                c if c == ch(b'a') => self.append = true,
                c if c == ch(b'b') => self.backup = true,
                c if c == ch(b'B') => self.act = Act::BackupSectors,
                OPT_CHANGE_ID | OPT_PRINT_ID => {
                    let name = OPTIONS.name_of(c).unwrap_or("");
                    warnx(
                        self.short(),
                        &format!("{name} is deprecated in favour of --part-type"),
                    );
                    self.act = Act::PartType;
                }
                c if c == ch(b'c') => {
                    warnx(self.short(), "--id is deprecated in favour of --part-type");
                    self.act = Act::PartType;
                }
                c if c == ch(b'J') || c == ch(b'd') => {
                    if c == ch(b'J') {
                        self.json = true;
                    }
                    self.act = Act::Dump;
                }
                c if c == ch(b'F') => self.act = Act::ListFree,
                c if c == ch(b'f') => self.force = true,
                c if c == ch(b'G') || c == ch(b'g') => {
                    if c == ch(b'G') {
                        warnx(
                            self.short(),
                            "--show-pt-geometry is no more implemented. Using --show-geometry.",
                        );
                    }
                    self.act = Act::ShowGeom;
                }
                c if c == ch(b'h') => {
                    let text = usage(self.short());
                    self.sh.term.write(&text);
                    return 0;
                }
                c if c == ch(b'l') => self.act = Act::List,
                c if c == ch(b'L') => {
                    warnx(self.short(), "--Linux option is unnecessary and deprecated");
                }
                c if c == ch(b'o') => outarg = Some(argb),
                c if c == ch(b'O') => {
                    self.backup = true;
                    self.backup_file = Some(argb);
                }
                c if c == ch(b'n') => self.noact = true,
                c if c == ch(b'N') => {
                    let n = self.strtou32_or_err(&argb, "failed to parse partition number");
                    self.partno = i32::from_ne_bytes(n.wrapping_sub(1).to_ne_bytes());
                }
                c if c == ch(b'q') => self.sh.quiet.set(true),
                c if c == ch(b'r') => self.act = Act::Reorder,
                c if c == ch(b's') => self.act = Act::ShowSize,
                c if c == ch(b'T') => self.act = Act::ListTypes,
                c if c == ch(b'u') => {
                    let unit = argb.first().copied().unwrap_or(0);
                    if unit != b'S' {
                        self.errx(&format!(
                            "unsupported unit '{}'",
                            escape_unprintable(&[unit])
                        ));
                    }
                }
                c if c == ch(b'v') => {
                    let short = String::from_utf8_lossy(self.short()).into_owned();
                    self.sh
                        .term
                        .write(format!("{short} from {PACKAGE_STRING}\n").as_bytes());
                    return 0;
                }
                c if c == ch(b'V') => self.verify = true,
                c if c == ch(b'w') || c == ch(b'W') => {
                    let Some(mode) = list::wipemode_from_string(&argb) else {
                        self.errx("unsupported wipe mode");
                    };
                    if c == ch(b'w') {
                        self.wipemode = mode;
                    } else {
                        self.pwipemode = mode;
                    }
                }
                c if c == ch(b'X') => self.label = Some(argb),
                c if c == ch(b'Y') => self.label_nested = Some(argb),
                OPT_PARTUUID => self.act = Act::PartUuid,
                OPT_PARTTYPE => self.act = Act::PartType,
                OPT_PARTLABEL => self.act = Act::PartLabel,
                OPT_PARTATTRS => self.act = Act::PartAttrs,
                OPT_DISKID => self.act = Act::DiskId,
                OPT_NOREREAD => self.noreread = true,
                OPT_BYTES => bytes = true,
                OPT_COLOR => {
                    colormode = ulcolors::ColorMode::Auto;
                    if value.is_some() {
                        colormode = ulcolors::colormode_or_err(&argb).unwrap_or_else(|bad| {
                            self.errx(&format!(
                                "unsupported color mode: {}",
                                quoting::escaped_in_quotes(&bad)
                            ))
                        });
                    }
                }
                OPT_MOVEDATA => {
                    self.movedata = true;
                    self.move_typescript = value.is_some().then_some(argb);
                }
                OPT_MOVEFSYNC => self.movefsync = true,
                OPT_DELETE => self.act = Act::Delete,
                OPT_NOTELL => self.notell = true,
                OPT_RELOCATE => self.act = Act::Relocate,
                OPT_LOCK => {
                    self.lockmode = Some(match value {
                        Some(_) => argb.strip_prefix(b"=").unwrap_or(&argb).to_vec(),
                        None => b"1".to_vec(),
                    });
                }
                _ => {
                    stderr_write(
                        format!(
                            "Try '{} --help' for more information.\n",
                            escape_unprintable(self.short())
                        )
                        .as_bytes(),
                    );
                    return 1;
                }
            }
        }

        *self.sh.term.colors.borrow_mut() = Some(ulcolors::Colors::init(
            colormode,
            b"sfdisk",
            smartcols::tty::stdout_is_tty(),
        ));
        self.init();
        if bytes {
            self.cxt.set_size_unit(libfdisk::SIZEUNIT_BYTES);
        }
        if outarg.is_some() {
            self.fields.string = outarg;
        }
        if self.verify && self.act == Act::None {
            // --verify may be used with --list too.
            self.act = Act::Verify;
        } else if self.act == Act::None {
            self.act = Act::Fdisk;
        }
        if self.movedata && !(self.act == Act::Fdisk && self.partno >= 0) {
            self.errx("--movedata requires -N");
        }

        let args = operands;
        let rc = match self.act {
            Act::Activate => self.command_activate(&args),
            Act::BackupSectors => self.command_backup_sectors(&args),
            Act::Delete => self.command_delete(&args),
            Act::List => self.command_list_partitions(&args),
            Act::ListTypes => self.command_list_types(),
            Act::ListFree => self.command_list_freespace(&args),
            Act::Fdisk | Act::None => self.command_fdisk(&args),
            Act::Dump => self.command_dump(&args),
            Act::ShowSize => self.command_show_size(&args),
            Act::ShowGeom => self.command_show_geometry(&args),
            Act::Verify => self.command_verify(&args),
            Act::PartType => self.command_parttype(&args),
            Act::PartUuid | Act::PartLabel | Act::PartAttrs => {
                self.command_partfield(&args, self.act)
            }
            Act::DiskId => self.command_diskid(&args),
            Act::Reorder => self.command_reorder(&args),
            Act::Relocate => self.command_relocate(&args),
        };
        u8::from(rc != 0)
    }
}

fn main() {
    // Before anything touches standard I/O: a descriptor the process was
    // started without stays closed, as upstream would find it.
    stdfdguard::restore();
    let argv: Vec<OsString> = std::env::args_os().collect();
    let short = short_name(
        argv.first()
            .map_or(OsStr::new("sfdisk"), OsString::as_os_str),
    );
    let sh = Rc::new(Shared {
        term: Term::new(short),
        quiet: Cell::new(false),
    });
    let mut sf = Sfdisk {
        sh: Rc::clone(&sh),
        act: Act::None,
        partno: -1,
        wipemode: WIPEMODE_AUTO,
        pwipemode: WIPEMODE_AUTO,
        lockmode: None,
        label: None,
        label_nested: None,
        backup_file: None,
        move_typescript: None,
        prompt: None,
        cxt: Context::new(),
        orig_pa: None,
        verify: false,
        interactive: std::io::stdin().is_terminal(),
        noreread: false,
        force: false,
        backup: false,
        container: false,
        unused: false,
        append: false,
        json: false,
        movedata: false,
        movefsync: false,
        notell: false,
        noact: false,
        fields: Fields::default(),
    };
    let status = sf.run(&argv);
    sh.term.exit(status)
}

#[cfg(test)]
mod tests;
