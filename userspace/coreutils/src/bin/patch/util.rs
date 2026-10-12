//! `util.c`: talking to the user, ending in failure, and the file operations
//! everything else is built on -- backups, moves, copies, temporary files,
//! the names in patch headers -- plus the signal handling around them.

use std::ffi::CString;
use std::io::{Read, Write};
use std::sync::Mutex;
use std::sync::atomic::{AtomicPtr, Ordering};

use crate::backupfile::BackupType;
use crate::sys::{self, Stat, Timespec, errno, oflag};
use crate::{Ctx, FileId, FileIdType, Verbosity};

/// What `version_controller` found: the system's name, the command that
/// gets the file from it, and the one that compares the file with it.
pub type VersionControl = (&'static str, Vec<u8>, Option<Vec<u8>>);

/// `enum file_attributes`.
pub const FA_TIMES: u32 = 1;
pub const FA_IDS: u32 = 2;
pub const FA_MODE: u32 = 4;
pub const FA_XATTRS: u32 = 8;

/// `PATH_MAX`.
const PATH_MAX: usize = 4096;

/// Write all of `bytes` to descriptor `fd`. A failure is not reported:
/// upstream checks neither `printf` nor `fprintf (stderr, ...)`.
fn write_fd(fd: i32, bytes: &[u8]) {
    let _ = coreutils::stdfd::write_all(fd, bytes);
}

/// To standard output, at once: upstream's `printf` followed by `fflush`.
pub fn print_stdout(bytes: &[u8]) {
    write_fd(1, bytes);
}

/// To standard error.
pub fn print_stderr(bytes: &[u8]) {
    write_fd(2, bytes);
}

/// `say`.
pub fn say(bytes: &[u8]) {
    print_stdout(bytes);
}

/// A descriptor this program owns, as a `File` that closes it.
pub fn file_from_fd(fd: i32) -> std::fs::File {
    #[cfg(unix)]
    {
        use std::os::fd::FromRawFd;
        // SAFETY: every caller hands over a descriptor it opened and owns
        // alone; the `File` closes it once.
        unsafe { std::fs::File::from_raw_fd(fd) }
    }
    #[cfg(not(unix))]
    {
        let _ = fd;
        // Off unix nothing opens a descriptor to hand over; this is never
        // reached, and a null device stands in rather than a panic.
        std::fs::File::open("NUL").unwrap_or_else(|_| std::process::exit(2))
    }
}

/// Close `file`, saying whether the close succeeded: `close (fd) == 0`.
pub fn close_file(file: std::fs::File) -> bool {
    coreutils::stdfd::close(file).is_ok()
}

/// `fstat` of an open file.
pub fn fstat_file(file: &std::fs::File) -> Option<Stat> {
    #[cfg(unix)]
    {
        use std::os::fd::AsRawFd;
        sys::fstat_fd(file.as_raw_fd()).ok()
    }
    #[cfg(not(unix))]
    {
        let _ = file;
        None
    }
}

/// `gettime`.
pub fn now() -> Timespec {
    let d = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    Timespec {
        sec: i64::try_from(d.as_secs()).unwrap_or(i64::MAX),
        nsec: i64::from(d.subsec_nanos()),
    }
}

/// `argmatch (word, quoting_style_args, ...)`: the style, or whether the
/// word was ambiguous rather than unknown.
pub fn quoting_style(word: &[u8]) -> Result<quoting::Style, bool> {
    let mut found: Option<quoting::Style> = None;
    let mut ambiguous = false;
    for (name, style) in quoting::Style::WORDS {
        if name.as_bytes() == word {
            return Ok(*style);
        }
        if name.as_bytes().starts_with(word) {
            match found {
                None => found = Some(*style),
                Some(f) if f != *style => ambiguous = true,
                Some(_) => {}
            }
        }
    }
    match found {
        Some(s) if !ambiguous => Ok(s),
        _ => Err(ambiguous),
    }
}

/// `invalid_arg (context, value, problem)`: gnulib's `argmatch_invalid`,
/// under the program's name.
pub fn invalid_arg(program_name: &[u8], context: &str, value: &[u8], ambiguous: bool) {
    let mut m = program_name.to_vec();
    m.extend_from_slice(if ambiguous {
        b": ambiguous argument "
    } else {
        b": invalid argument "
    });
    m.extend_from_slice(&crate::backupfile::quote_c_locale(value));
    m.extend_from_slice(b" for ");
    m.extend_from_slice(&crate::backupfile::quote_c_locale(context.as_bytes()));
    m.push(b'\n');
    print_stderr(&m);
}

// ------------------------------------------------------------- signals ----

/// `sigs[]`: the signals upstream's `fatal_exit` handles.
const SIGS: [i32; 6] = [1, 13, 15, 24, 25, 2];
/// `SIGPIPE`.
const SIGPIPE: i32 = 13;
/// `SIGCHLD`.
const SIGCHLD: i32 = 17;

/// What `set_signals (false)` found to handle (`signals_to_block`), and the
/// mask `ignore_signals` replaced (`initial_signal_mask`).
struct SignalState {
    to_block: Vec<i32>,
    initial_mask: Option<libcall::signal::SigSet>,
}

static SIGNALS: Mutex<SignalState> = Mutex::new(SignalState {
    to_block: Vec::new(),
    initial_mask: None,
});

/// The temporary files a fatal signal must remove, published for the
/// handler as an immutable snapshot it can read without a lock.
struct Registry {
    temps: [Option<CString>; 5],
    queued: Vec<CString>,
}

static REGISTRY: Mutex<Registry> = Mutex::new(Registry {
    temps: [None, None, None, None, None],
    queued: Vec::new(),
});
static SNAPSHOT: AtomicPtr<Vec<CString>> = AtomicPtr::new(std::ptr::null_mut());

fn publish(r: &Registry) {
    let all: Vec<CString> = r
        .temps
        .iter()
        .flatten()
        .chain(r.queued.iter())
        .cloned()
        .collect();
    // Leaked on purpose: the handler may be reading the previous snapshot
    // at this moment, so none is ever freed. Each is a few names.
    let snap = Box::leak(Box::new(all));
    SNAPSHOT.store(snap, Ordering::SeqCst);
}

/// Temporary file `slot` (`crate::TMP_*`) is `name` and must be removed
/// if a signal ends the program.
pub fn register_temp(slot: usize, name: &[u8]) {
    if let Ok(mut r) = REGISTRY.lock()
        && let Some(t) = r.temps.get_mut(slot)
    {
        *t = CString::new(name).ok();
        publish(&r);
    }
}

/// Temporary file `slot` no longer needs removing.
pub fn unregister_temp(slot: usize) {
    if let Ok(mut r) = REGISTRY.lock()
        && let Some(t) = r.temps.get_mut(slot)
    {
        *t = None;
        publish(&r);
    }
}

/// A finished output, queued to be moved into place later.
pub fn register_queued(name: &[u8]) {
    if let Ok(mut r) = REGISTRY.lock()
        && let Ok(c) = CString::new(name)
    {
        r.queued.push(c);
        publish(&r);
    }
}

/// A queued output moved into place.
pub fn unregister_queued(name: &[u8]) {
    if let Ok(mut r) = REGISTRY.lock() {
        r.queued.retain(|c| c.as_bytes() != name);
        publish(&r);
    }
}

/// `fatal_exit` as a signal handler: the temporary files removed, then the
/// signal raised again (`exit_with_signal`).
extern "C" fn fatal_handler(sig: i32) {
    #[cfg(unix)]
    {
        unsafe extern "C" {
            fn unlink(path: *const std::ffi::c_char) -> i32;
        }
        let p = SNAPSHOT.load(Ordering::SeqCst);
        if !p.is_null() {
            // SAFETY: every snapshot is leaked, so `p` stays valid, and none
            // is changed after it is published.
            for name in unsafe { &*p } {
                // SAFETY: a live NUL-terminated string; `unlink` is
                // async-signal-safe.
                unsafe { unlink(name.as_ptr()) };
            }
        }
    }
    exit_with_signal(sig);
}

/// `exit_with_signal`.
pub fn exit_with_signal(sig: i32) -> ! {
    let _ = libcall::signal::set_default(sig);
    let mut s = libcall::signal::SigSet::empty();
    if s.add(sig).is_ok() {
        let _ = libcall::signal::unblock(&s);
    }
    let _ = libcall::signal::raise(sig);
    #[cfg(unix)]
    {
        libcall::process::exit_immediately(2)
    }
    #[cfg(not(unix))]
    {
        std::process::exit(2)
    }
}

/// `set_signals`.
pub fn set_signals(reset: bool) {
    let Ok(mut st) = SIGNALS.lock() else {
        return;
    };
    if !reset {
        let _ = libcall::signal::set_default(SIGCHLD);
        st.to_block.clear();
        for sig in SIGS {
            let ignoring = if sig == SIGPIPE {
                // The runtime ignored it before `main`; what matters is what
                // this program was given.
                coreutils::stdfd::sigpipe_ignored_at_startup()
            } else {
                match libcall::signal::is_ignored(sig) {
                    Ok(i) => i,
                    Err(_) => continue,
                }
            };
            if !ignoring {
                st.to_block.push(sig);
                let _ = libcall::signal::set_handler(sig, fatal_handler, false);
            }
        }
    } else if let Some(mask) = st.initial_mask.take() {
        // Undo the effect of ignore_signals.
        let _ = libcall::signal::set_mask(&mask);
    }
}

/// `ignore_signals`: the handled signals held back through the critical
/// region of putting files in place.
pub fn ignore_signals() {
    let Ok(mut st) = SIGNALS.lock() else {
        return;
    };
    let mut set = libcall::signal::SigSet::empty();
    for &sig in &st.to_block {
        let _ = set.add(sig);
    }
    if let Ok(old) = libcall::signal::block(&set) {
        st.initial_mask = Some(old);
    }
}

// --------------------------------------------------------- file names ----

fn is_space(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}

/// The bytes of `s` up to its first NUL: what C's string functions see.
pub fn cstr(s: &[u8]) -> &[u8] {
    s.iter()
        .position(|&c| c == 0)
        .map_or(s, |n| s.get(..n).unwrap_or(s))
}

/// gnulib's `last_component`: where the last component of `name` starts.
pub fn last_component(name: &[u8]) -> usize {
    let mut base = name.iter().take_while(|&&c| c == b'/').count();
    let mut saw_slash = false;
    let mut p = base;
    while let Some(&c) = name.get(p) {
        if c == b'/' {
            saw_slash = true;
        } else if saw_slash {
            base = p;
            saw_slash = false;
        }
        p = p.saturating_add(1);
    }
    base
}

/// gnulib's `base_len`: `name`'s length without trailing slashes.
pub fn base_len(name: &[u8]) -> usize {
    let mut len = name.len();
    while len > 1 && name.get(len.saturating_sub(1)) == Some(&b'/') {
        len = len.saturating_sub(1);
    }
    len
}

/// gnulib's `dir_len`.
fn dir_len(file: &[u8]) -> usize {
    let prefix = usize::from(file.first() == Some(&b'/'));
    let mut length = last_component(file);
    while prefix < length {
        if file.get(length.saturating_sub(1)) != Some(&b'/') {
            break;
        }
        length = length.saturating_sub(1);
    }
    length
}

/// gnulib's `dir_name`.
pub fn dir_name(file: &[u8]) -> Vec<u8> {
    let length = dir_len(file);
    let mut dir = file.get(..length).unwrap_or_default().to_vec();
    if length == 0 {
        dir.push(b'.');
    }
    dir
}

/// gnulib's `base_name`.
pub fn base_name(name: &[u8]) -> Vec<u8> {
    let base = last_component(name);
    let tail = name.get(base..).unwrap_or_default();
    if tail.is_empty() {
        return name.get(..base_len(name)).unwrap_or_default().to_vec();
    }
    let mut length = base_len(tail);
    if tail.get(length) == Some(&b'/') {
        length = length.saturating_add(1);
    }
    tail.get(..length).unwrap_or_default().to_vec()
}

/// `parse_c_string`: a C-quoted name at `s[0] == '"'`; the name, and where
/// it ended -- or `None` when it is not a valid C string.
fn parse_c_string(s: &[u8]) -> (Option<Vec<u8>>, usize) {
    let mut out = Vec::new();
    let mut i = 1usize;
    loop {
        let c = s.get(i).copied().unwrap_or(0);
        i = i.saturating_add(1);
        match c {
            0 => return (None, i),
            b'"' => return (Some(out), i),
            b'\\' => {}
            c => {
                out.push(c);
                continue;
            }
        }
        let c = s.get(i).copied().unwrap_or(0);
        i = i.saturating_add(1);
        let decoded = match c {
            b'a' => 0x07,
            b'b' => 0x08,
            b'f' => 0x0c,
            b'n' => b'\n',
            b'r' => b'\r',
            b't' => b'\t',
            b'v' => 0x0b,
            b'\\' | b'"' => c,
            b'0'..=b'3' => {
                let d1 = s.get(i).copied().unwrap_or(0);
                i = i.saturating_add(1);
                if !(b'0'..=b'7').contains(&d1) {
                    return (None, i);
                }
                let d2 = s.get(i).copied().unwrap_or(0);
                i = i.saturating_add(1);
                if !(b'0'..=b'7').contains(&d2) {
                    return (None, i);
                }
                (c.wrapping_sub(b'0') << 6) | (d1.wrapping_sub(b'0') << 3) | d2.wrapping_sub(b'0')
            }
            _ => return (None, i),
        };
        out.push(decoded);
    }
}

/// `strip_leading_slashes`: `name` with up to `strip` leading components
/// removed (all of them when `strip` is negative), or `None` when it has too
/// few.
fn strip_leading_slashes(name: &[u8], strip: i32) -> Option<Vec<u8>> {
    let mut s = strip;
    let mut n = 0usize;
    let mut p = 0usize;
    while p < name.len() {
        if name.get(p) == Some(&b'/') {
            while name.get(p.saturating_add(1)) == Some(&b'/') {
                p = p.saturating_add(1);
            }
            if strip < 0 || {
                s = s.saturating_sub(1);
                s >= 0
            } {
                n = p.saturating_add(1);
            }
        }
        p = p.saturating_add(1);
    }
    let rest = name.get(n..).unwrap_or_default();
    if (strip < 0 || s <= 0) && !rest.is_empty() {
        Some(rest.to_vec())
    } else {
        None
    }
}

/// `filename_is_safe`: relative, and with no `..` component.
pub fn filename_is_safe(name: &[u8]) -> bool {
    if name.first() == Some(&b'/') {
        return false;
    }
    let mut i = 0usize;
    while i < name.len() {
        if name.get(i) == Some(&b'.') {
            i = i.saturating_add(1);
            if name.get(i) == Some(&b'.') {
                i = i.saturating_add(1);
                if i >= name.len() || name.get(i) == Some(&b'/') {
                    return false;
                }
            }
        }
        while name.get(i).is_some_and(|&c| c != b'/') {
            i = i.saturating_add(1);
        }
        while name.get(i) == Some(&b'/') {
            i = i.saturating_add(1);
        }
    }
    true
}

/// `contains_slash`.
fn contains_slash(s: &[u8]) -> bool {
    s.contains(&b'/')
}

impl Ctx {
    // ---------------------------------------------------------- dying ----

    /// `fatal`.
    pub fn fatal(&mut self, msg: &[u8]) -> ! {
        let mut m = self.program_name.clone();
        m.extend_from_slice(b": **** ");
        m.extend_from_slice(msg);
        m.push(b'\n');
        print_stderr(&m);
        self.fatal_exit();
    }

    /// `pfatal`: the message, then `perror (" ")`'s ` : <error>`.
    pub fn pfatal(&mut self, msg: &[u8], errnum: i32) -> ! {
        let mut m = self.program_name.clone();
        m.extend_from_slice(b": **** ");
        m.extend_from_slice(msg);
        m.extend_from_slice(b" : ");
        m.extend_from_slice(
            coreutils::errmsg::strerror(&std::io::Error::from_raw_os_error(errnum)).as_bytes(),
        );
        m.push(b'\n');
        print_stderr(&m);
        self.fatal_exit();
    }

    /// `read_fatal`.
    pub fn read_fatal(&mut self) -> ! {
        let e = sys::last_errno();
        self.pfatal(b"read error", e);
    }

    /// `write_fatal`.
    pub fn write_fatal(&mut self) -> ! {
        let e = sys::last_errno();
        self.pfatal(b"write error", e);
    }

    // ------------------------------------------------------- talking ----

    /// `ask`: the prompt, then a line from the terminal into `buf` -- or,
    /// with no terminal, a newline, as if the default had been typed.
    pub fn ask(&mut self, prompt: &[u8]) {
        print_stdout(prompt);
        if self.ttyfd == -2 {
            self.ttyfd = if self.posixly_correct || coreutils::stdfd::is_tty(1) {
                sys::open_at(sys::AT_FDCWD, b"/dev/tty", oflag::RDONLY, 0).unwrap_or(-1)
            } else {
                -1
            };
        }
        if self.ttyfd < 0 {
            // No terminal at all -- default it.
            print_stdout(b"\n");
            self.buf.clear();
            self.buf.push(b'\n');
            return;
        }
        // Read what fits in `buf`; read again, into a buffer twice the
        // size, only when that filled it without ending a line.
        self.buf.clear();
        loop {
            let want = self
                .bufsize
                .saturating_sub(1)
                .saturating_sub(self.buf.len());
            let mut chunk = vec![0u8; want];
            match coreutils::stdfd::read(self.ttyfd, &mut chunk) {
                Ok(n) => {
                    self.buf
                        .extend_from_slice(chunk.get(..n).unwrap_or_default());
                    if n == want && self.buf.get(self.bufsize.saturating_sub(2)) != Some(&b'\n') {
                        self.bufsize = self.bufsize.saturating_mul(2);
                        continue;
                    }
                    if n == 0 {
                        print_stdout(b"EOF\n");
                    }
                }
                Err(e) => {
                    let mut m = self.program_name.clone();
                    m.extend_from_slice(b": tty read failed: ");
                    m.extend_from_slice(coreutils::errmsg::strerror(&e).as_bytes());
                    m.push(b'\n');
                    print_stderr(&m);
                    // The terminal is given up on; what closing it says
                    // changes nothing.
                    let _ = sys::close_fd(self.ttyfd);
                    self.ttyfd = -1;
                }
            }
            break;
        }
    }

    /// The first byte of the answer, `*buf`.
    pub fn answer(&self) -> u8 {
        self.buf.first().copied().unwrap_or(0)
    }

    /// `ok_to_reverse`: whether to reverse the patch `msg` describes.
    pub fn ok_to_reverse(&mut self, msg: &[u8]) -> bool {
        let mut r = false;
        if self.noreverse || !(self.force && self.verbosity == Verbosity::Silent) {
            say(msg);
        }
        if self.noreverse {
            say(b"  Skipping patch.\n");
            self.skip_rest_of_patch = true;
        } else if self.force {
            if self.verbosity != Verbosity::Silent {
                say(b"  Applying it anyway.\n");
            }
        } else if self.batch {
            say(if self.reverse {
                b"  Ignoring -R.\n"
            } else {
                b"  Assuming -R.\n"
            });
            r = true;
        } else {
            self.ask(if self.reverse {
                b"  Ignore -R? [n] "
            } else {
                b"  Assume -R? [n] "
            });
            r = self.answer() == b'y';
            if !r {
                self.ask(b"Apply anyway? [n] ");
                if self.answer() != b'y' {
                    if self.verbosity != Verbosity::Silent {
                        say(b"Skipping patch.\n");
                    }
                    self.skip_rest_of_patch = true;
                }
            }
        }
        r
    }

    // -------------------------------------------------------- file ids ----

    /// `insert_file_id`.
    pub fn insert_file_id(&mut self, st: &Stat, kind: FileIdType) {
        let e = self.file_ids.entry((st.dev, st.ino)).or_insert(FileId {
            kind,
            queued_output: false,
        });
        e.kind = kind;
    }

    /// `lookup_file_id`.
    pub fn lookup_file_id(&self, st: &Stat) -> FileIdType {
        self.file_ids
            .get(&(st.dev, st.ino))
            .map_or(FileIdType::Unknown, |f| f.kind)
    }

    /// `set_queued_output`.
    pub fn set_queued_output(&mut self, st: &Stat, queued: bool) {
        let e = self.file_ids.entry((st.dev, st.ino)).or_insert(FileId {
            kind: FileIdType::Unknown,
            queued_output: false,
        });
        e.queued_output = queued;
    }

    /// `has_queued_output`.
    pub fn has_queued_output(&self, st: &Stat) -> bool {
        self.file_ids
            .get(&(st.dev, st.ino))
            .is_some_and(|f| f.queued_output)
    }

    // ------------------------------------------------- file attributes ----

    fn kind_word(mode: u32) -> &'static str {
        if mode & sys::S_IFMT == sys::S_IFLNK {
            "symbolic link"
        } else {
            "file"
        }
    }

    /// `set_file_attributes`.
    pub fn set_file_attributes(
        &mut self,
        to: &[u8],
        attr: u32,
        from: Option<&[u8]>,
        st: Option<&Stat>,
        mode: u32,
        new_time: Option<Timespec>,
    ) {
        let st = st.copied().unwrap_or_default();
        let _ = from;
        if attr & FA_TIMES != 0 {
            let times = match new_time {
                Some(t) => [t, t],
                None => [st.atime, st.mtime],
            };
            if let Err(e) = self.safe.lutimens(to, times) {
                let mut m = format!("Failed to set the timestamps of {} ", Self::kind_word(mode))
                    .into_bytes();
                m.extend_from_slice(&self.q(to));
                self.pfatal(&m, e);
            }
        }
        if attr & FA_IDS != 0 {
            let (euid, egid) = *self.effective_ids.get_or_insert_with(sys::effective_ids);
            let mut uid = if euid == st.uid { u32::MAX } else { st.uid };
            let gid = if egid == st.gid { u32::MAX } else { st.gid };
            // May fail if we are not privileged to set the file owner, or we
            // are not in group instat.st_gid. Ignore those errors.
            if uid != u32::MAX || gid != u32::MAX {
                let mut failure = self.safe.lchown(to, uid, gid).err();
                if failure == Some(errno::EPERM) {
                    failure = if uid != u32::MAX {
                        uid = u32::MAX;
                        match self.safe.lchown(to, uid, gid) {
                            Ok(()) | Err(errno::EPERM) => None,
                            Err(e) => Some(e),
                        }
                    } else {
                        None
                    };
                }
                if let Some(e) = failure {
                    let mut m = format!(
                        "Failed to set the {} of {} ",
                        if uid == u32::MAX {
                            "owner"
                        } else {
                            "owning group"
                        },
                        Self::kind_word(mode)
                    )
                    .into_bytes();
                    m.extend_from_slice(&self.q(to));
                    self.pfatal(&m, e);
                }
            }
        }
        // FA_XATTRS: Ubuntu's patch is built without libattr, so there is
        // nothing to copy.
        if attr & FA_MODE != 0
            && mode & sys::S_IFMT != sys::S_IFLNK
            && let Err(e) = self.safe.chmod(to, mode & 0o7777)
        {
            let mut m = format!(
                "Failed to set the permissions of {} ",
                Self::kind_word(mode)
            )
            .into_bytes();
            m.extend_from_slice(&self.q(to));
            self.pfatal(&m, e);
        }
    }

    // --------------------------------------------------------- backups ----

    /// `create_backup_copy`.
    fn create_backup_copy(
        &mut self,
        from: &[u8],
        to: &[u8],
        st: &Stat,
        to_dir_known_to_exist: bool,
    ) {
        self.copy_file(from, to, None, 0, st.mode, to_dir_known_to_exist);
        self.set_file_attributes(
            to,
            FA_TIMES | FA_IDS | FA_MODE,
            Some(from),
            Some(st),
            st.mode,
            None,
        );
    }

    /// `create_backup`.
    pub fn create_backup(&mut self, to: &[u8], to_st: Option<&Stat>, leave_original: bool) {
        if let Some(st) = to_st
            && !(st.is_reg() || st.is_lnk())
        {
            let mut m = b"File ".to_vec();
            m.extend_from_slice(to);
            m.extend_from_slice(b" is not a ");
            m.extend_from_slice(if st.is_lnk() {
                b"symbolic link"
            } else {
                b"regular file"
            });
            m.extend_from_slice(b" -- refusing to create backup");
            self.fatal(&m);
        }

        if let Some(st) = to_st
            && self.lookup_file_id(st) == FileIdType::Created
        {
            if self.debug & 4 != 0 {
                let mut m = b"File ".to_vec();
                m.extend_from_slice(&self.q(to));
                m.extend_from_slice(b" already seen\n");
                say(&m);
            }
            return;
        }

        let mut try_makedirs_errno = 0;
        let bakname =
            if self.origprae.is_some() || self.origbase.is_some() || self.origsuff.is_some() {
                let p = self.origprae.clone().unwrap_or_default();
                let b = self.origbase.clone().unwrap_or_default();
                let s = self.origsuff.clone().unwrap_or_default();
                let o = to
                    .iter()
                    .rposition(|&c| c == b'/')
                    .map_or(0, |i| i.saturating_add(1));
                let mut bak = p.clone();
                bak.extend_from_slice(to.get(..o).unwrap_or_default());
                bak.extend_from_slice(&b);
                bak.extend_from_slice(to.get(o..).unwrap_or_default());
                bak.extend_from_slice(&s);
                if (self.origprae.is_some() && (contains_slash(&p) || contains_slash(to)))
                    || (self.origbase.is_some() && contains_slash(&b))
                {
                    try_makedirs_errno = errno::ENOENT;
                }
                bak
            } else {
                let bt = self.backup_type;
                self.find_backup_file_name(to, bt)
            };

        match to_st {
            None => {
                if self.debug & 4 != 0 {
                    let mut m = b"Creating empty file ".to_vec();
                    m.extend_from_slice(&self.q(&bakname));
                    m.push(b'\n');
                    say(&m);
                }
                try_makedirs_errno = errno::ENOENT;
                let _ = self.safe.unlink(&bakname);
                let fd = loop {
                    match self.safe.open(
                        &bakname,
                        oflag::CREAT | oflag::EXCL | oflag::WRONLY | oflag::TRUNC,
                        0o666,
                    ) {
                        Ok(fd) => break fd,
                        Err(e) => {
                            if e != try_makedirs_errno {
                                let mut m = b"Can't create file ".to_vec();
                                m.extend_from_slice(&self.q(&bakname));
                                self.pfatal(&m, e);
                            }
                            self.makedirs(&bakname);
                            try_makedirs_errno = 0;
                        }
                    }
                };
                if let Err(e) = sys::close_fd(fd) {
                    let mut m = b"Can't close file ".to_vec();
                    m.extend_from_slice(&self.q(&bakname));
                    self.pfatal(&m, e);
                }
            }
            Some(st) if leave_original => {
                let st = *st;
                self.create_backup_copy(to, &bakname, &st, try_makedirs_errno == 0);
            }
            Some(st) => {
                let st = *st;
                if self.debug & 4 != 0 {
                    let mut m = b"Renaming file ".to_vec();
                    m.extend_from_slice(&self.q(to));
                    m.extend_from_slice(b" to ");
                    m.extend_from_slice(&self.q(&bakname));
                    m.push(b'\n');
                    say(&m);
                }
                loop {
                    match self.safe.rename(to, &bakname) {
                        Ok(()) => break,
                        Err(e) if e == try_makedirs_errno => {
                            self.makedirs(&bakname);
                            try_makedirs_errno = 0;
                        }
                        Err(errno::EXDEV) => {
                            self.create_backup_copy(to, &bakname, &st, try_makedirs_errno == 0);
                            let _ = self.safe.unlink(to);
                            break;
                        }
                        Err(e) => {
                            let mut m = b"Can't rename file ".to_vec();
                            m.extend_from_slice(&self.q(to));
                            m.extend_from_slice(b" to ");
                            m.extend_from_slice(&self.q(&bakname));
                            self.pfatal(&m, e);
                        }
                    }
                }
            }
        }
    }

    /// `move_file`: `from` (or, with none, nothing -- the file is removed)
    /// put at `to`, renamed if possible and copied if not; `to` backed up
    /// first if `backup`.
    pub fn move_file(
        &mut self,
        from: Option<&[u8]>,
        from_needs_removal: Option<&mut bool>,
        fromst: Option<&Stat>,
        to: &[u8],
        mode: u32,
        backup: bool,
    ) {
        let (to_errno, mut to_st) = match self.stat_file(to) {
            Ok(s) => (0, s),
            Err(e) => (e, Stat::default()),
        };
        if backup {
            let st = (to_errno == 0).then_some(to_st);
            self.create_backup(to, st.as_ref(), false);
        }
        if to_errno == 0 {
            self.insert_file_id(&to_st, FileIdType::Overwritten);
        }

        let Some(from) = from else {
            if !backup {
                if self.debug & 4 != 0 {
                    let mut m = b"Removing file ".to_vec();
                    m.extend_from_slice(&self.q(to));
                    m.push(b'\n');
                    say(&m);
                }
                if let Err(e) = self.safe.unlink(to)
                    && e != errno::ENOENT
                {
                    let mut m = b"Can't remove file ".to_vec();
                    m.extend_from_slice(&self.q(to));
                    self.pfatal(&m, e);
                }
            }
            return;
        };

        if mode & sys::S_IFMT == sys::S_IFLNK {
            let mut to_dir_known_to_exist = false;
            // FROM contains the contents of the symlink we have patched;
            // need to convert that back into a symlink.
            let fd = match self.safe.open(from, oflag::RDONLY, 0) {
                Ok(fd) => fd,
                Err(e) => {
                    let mut m = b"Can't reopen file ".to_vec();
                    m.extend_from_slice(&self.q(from));
                    self.pfatal(&m, e);
                }
            };
            let mut f = file_from_fd(fd);
            let mut target = Vec::new();
            // Up to PATH_MAX bytes, as upstream reads into its buffer.
            let read_ok = Read::take(&mut f, u64::try_from(PATH_MAX).unwrap_or(4096))
                .read_to_end(&mut target)
                .is_ok();
            if !read_ok || !close_file(f) {
                self.read_fatal();
            }
            let target = cstr(&target).to_vec();
            if !backup && self.safe.unlink(to).is_ok() {
                to_dir_known_to_exist = true;
            }
            if let Err(e) = self.safe.symlink(&target, to) {
                if e == errno::ENOENT && !to_dir_known_to_exist {
                    self.makedirs(to);
                }
                if let Err(e) = self.safe.symlink(&target, to) {
                    let mut m = b"Can't create symbolic link ".to_vec();
                    m.extend_from_slice(to);
                    self.pfatal(&m, e);
                }
            }
            match self.safe.lstat(to) {
                Ok(s) => to_st = s,
                Err(e) => {
                    let mut m = b"Can't get file attributes of symbolic link ".to_vec();
                    m.extend_from_slice(to);
                    self.pfatal(&m, e);
                }
            }
            self.insert_file_id(&to_st, FileIdType::Created);
            return;
        }

        if self.debug & 4 != 0 {
            let mut m = b"Renaming file ".to_vec();
            m.extend_from_slice(&self.q(from));
            m.extend_from_slice(b" to ");
            m.extend_from_slice(&self.q(to));
            m.push(b'\n');
            say(&m);
        }

        if let Err(mut e) = self.safe.rename(from, to) {
            let mut to_dir_known_to_exist = false;
            let mut renamed = false;
            if e == errno::ENOENT && (to_errno == -1 || to_errno == errno::ENOENT) {
                self.makedirs(to);
                to_dir_known_to_exist = true;
                match self.safe.rename(from, to) {
                    Ok(()) => renamed = true,
                    Err(e2) => e = e2,
                }
            }
            if !renamed {
                if e == errno::EXDEV {
                    if !backup {
                        match self.safe.unlink(to) {
                            Ok(()) => to_dir_known_to_exist = true,
                            Err(errno::ENOENT) => {}
                            Err(e) => {
                                let mut m = b"Can't remove file ".to_vec();
                                m.extend_from_slice(&self.q(to));
                                self.pfatal(&m, e);
                            }
                        }
                    }
                    let tost = self.copy_file(from, to, Some(()), 0, mode, to_dir_known_to_exist);
                    if let Some(tost) = tost {
                        self.insert_file_id(&tost, FileIdType::Created);
                    }
                    return;
                }
                let mut m = b"Can't rename file ".to_vec();
                m.extend_from_slice(&self.q(from));
                m.extend_from_slice(b" to ");
                m.extend_from_slice(&self.q(to));
                self.pfatal(&m, e);
            }
        }

        // rename_succeeded:
        if let Some(fs) = fromst {
            self.insert_file_id(fs, FileIdType::Created);
        }
        // Do not clear *FROM_NEEDS_REMOVAL if it's possible that the rename
        // returned zero because FROM and TO are hard links to the same file.
        if (0 < to_errno || (to_errno == 0 && to_st.nlink <= 1))
            && let Some(r) = from_needs_removal
        {
            *r = false;
        }
    }

    /// `create_file`: `file` created and truncated, its mode readable and
    /// writable by the owner and executable by nobody; directories made as
    /// needed.
    pub fn create_file(
        &mut self,
        file: &[u8],
        open_flags: i32,
        mode: u32,
        to_dir_known_to_exist: bool,
    ) -> i32 {
        let mut try_makedirs_errno = if to_dir_known_to_exist {
            0
        } else {
            errno::ENOENT
        };
        let mode = (mode | 0o600) & !0o111 & 0o7777;
        loop {
            match self
                .safe
                .open(file, oflag::CREAT | oflag::TRUNC | open_flags, mode)
            {
                Ok(fd) => return fd,
                Err(e) => {
                    if e != try_makedirs_errno {
                        let mut m = b"Can't create file ".to_vec();
                        m.extend_from_slice(&self.q(file));
                        self.pfatal(&m, e);
                    }
                    self.makedirs(file);
                    try_makedirs_errno = 0;
                }
            }
        }
    }

    /// `copy_to_fd`.
    fn copy_to_fd(&mut self, from: &[u8], tofd: i32) {
        let flags = oflag::RDONLY | self.nofollow();
        let fromfd = match self.safe.open(from, flags, 0) {
            Ok(fd) => fd,
            Err(e) => {
                let mut m = b"Can't reopen file ".to_vec();
                m.extend_from_slice(&self.q(from));
                self.pfatal(&m, e);
            }
        };
        let mut chunk = vec![0u8; 8 * 1024];
        loop {
            match coreutils::stdfd::read(fromfd, &mut chunk) {
                Ok(0) => break,
                Ok(n) => {
                    if coreutils::stdfd::write_all(tofd, chunk.get(..n).unwrap_or_default())
                        .is_err()
                    {
                        self.write_fatal();
                    }
                }
                Err(_) => self.read_fatal(),
            }
        }
        if sys::close_fd(fromfd).is_err() {
            self.read_fatal();
        }
    }

    /// `copy_file`: `from` copied to `to`; `to`'s status, when `want_st`.
    pub fn copy_file(
        &mut self,
        from: &[u8],
        to: &[u8],
        want_st: Option<()>,
        to_flags: i32,
        mode: u32,
        to_dir_known_to_exist: bool,
    ) -> Option<Stat> {
        if self.debug & 4 != 0 {
            let mut m = format!("Copying {} ", Self::kind_word(mode)).into_bytes();
            m.extend_from_slice(&self.q(from));
            m.extend_from_slice(b" to ");
            m.extend_from_slice(&self.q(to));
            m.push(b'\n');
            say(&m);
        }
        if mode & sys::S_IFMT == sys::S_IFLNK {
            let target = match self.safe.readlink(from, PATH_MAX) {
                Ok(t) => t,
                Err(e) => {
                    let mut m = b"Can't read symbolic link ".to_vec();
                    m.extend_from_slice(from);
                    self.pfatal(&m, e);
                }
            };
            if let Err(e) = self.safe.symlink(&target, to) {
                let mut m = b"Can't create symbolic link ".to_vec();
                m.extend_from_slice(to);
                self.pfatal(&m, e);
            }
            if want_st.is_some() {
                return match self.safe.lstat(to) {
                    Ok(s) => Some(s),
                    Err(e) => {
                        let mut m = b"Can't get file attributes of symbolic link ".to_vec();
                        m.extend_from_slice(to);
                        self.pfatal(&m, e);
                    }
                };
            }
            None
        } else {
            let flags = oflag::WRONLY | to_flags | self.nofollow();
            let tofd = self.create_file(to, flags, mode, to_dir_known_to_exist);
            self.copy_to_fd(from, tofd);
            let st = if want_st.is_some() {
                match sys::fstat_fd(tofd) {
                    Ok(s) => Some(s),
                    Err(e) => {
                        let mut m = b"Can't get file attributes of file ".to_vec();
                        m.extend_from_slice(to);
                        self.pfatal(&m, e);
                    }
                }
            } else {
                None
            };
            if sys::close_fd(tofd).is_err() {
                self.write_fatal();
            }
            st
        }
    }

    /// `append_to_file`.
    pub fn append_to_file(&mut self, from: &[u8], to: &[u8]) {
        let flags = oflag::WRONLY | oflag::APPEND | self.nofollow();
        let tofd = match self.safe.open(to, flags, 0) {
            Ok(fd) => fd,
            Err(e) => {
                let mut m = b"Can't reopen file ".to_vec();
                m.extend_from_slice(&self.q(to));
                self.pfatal(&m, e);
            }
        };
        self.copy_to_fd(from, tofd);
        if sys::close_fd(tofd).is_err() {
            self.write_fatal();
        }
    }

    // ------------------------------------------------- version control ----

    /// `quote_system_arg`: shell-quoted, always in the shell style.
    fn quote_system_arg(arg: &[u8]) -> Vec<u8> {
        quoting::Style::Shell.quote_in(arg, quoting::Charset::Ascii)
    }

    /// `version_controller`: "RCS", "SCCS", "ClearCase" or "Perforce" when
    /// `filename` is under one, with the commands that get and diff it.
    pub fn version_controller(
        &mut self,
        filename: &[u8],
        readonly: bool,
        filestat: Option<&Stat>,
    ) -> Option<VersionControl> {
        let dir = dir_name(filename);
        let filebase = base_name(filename);
        let dotslash: &[u8] = if filename.first() == Some(&b'-') {
            b"./"
        } else {
            b""
        };
        let try_path = |this: &mut Self, rest: &[u8]| -> Option<Stat> {
            let mut p = dir.clone();
            p.push(b'/');
            p.extend_from_slice(rest);
            this.safe.stat(&p).ok()
        };
        let mut rcs = b"RCS/".to_vec();
        rcs.extend_from_slice(&filebase);
        let mut rcs_v = rcs.clone();
        rcs_v.extend_from_slice(b",v");
        let mut base_v = filebase.clone();
        base_v.extend_from_slice(b",v");
        let found = try_path(self, &rcs_v)
            .or_else(|| try_path(self, &rcs))
            .or_else(|| try_path(self, &base_v));
        if let Some(c) = found
            && !filestat.is_some_and(|f| f.dev == c.dev && f.ino == c.ino)
        {
            let mut get = if readonly {
                b"co ".to_vec()
            } else {
                b"co -l ".to_vec()
            };
            get.extend_from_slice(dotslash);
            get.extend_from_slice(&Self::quote_system_arg(filename));
            let mut diff = b"rcsdiff ".to_vec();
            diff.extend_from_slice(dotslash);
            diff.extend_from_slice(&Self::quote_system_arg(filename));
            diff.extend_from_slice(b">/dev/null");
            return Some(("RCS", get, Some(diff)));
        }
        let mut sccs1 = b"SCCS/s.".to_vec();
        sccs1.extend_from_slice(&filebase);
        let mut sccs2 = b"s.".to_vec();
        sccs2.extend_from_slice(&filebase);
        let sccs = if try_path(self, &sccs1).is_some() {
            Some(sccs1)
        } else if try_path(self, &sccs2).is_some() {
            Some(sccs2)
        } else {
            None
        };
        if let Some(rest) = sccs {
            let mut trybuf = dir.clone();
            trybuf.push(b'/');
            trybuf.extend_from_slice(&rest);
            let mut get = if readonly {
                b"get ".to_vec()
            } else {
                b"get -e ".to_vec()
            };
            get.extend_from_slice(&Self::quote_system_arg(&trybuf));
            let mut diff = b"get -p ".to_vec();
            diff.extend_from_slice(&Self::quote_system_arg(&trybuf));
            diff.extend_from_slice(b"|diff - ");
            diff.extend_from_slice(dotslash);
            diff.extend_from_slice(&Self::quote_system_arg(filename));
            diff.extend_from_slice(b">/dev/null");
            return Some(("SCCS", get, Some(diff)));
        }
        let mut at = filebase.clone();
        at.extend_from_slice(b"@@");
        if !readonly && filestat.is_some() && try_path(self, &at).is_some_and(|s| s.is_dir()) {
            let mut get = b"cleartool co -unr -nc ".to_vec();
            get.extend_from_slice(&Self::quote_system_arg(filename));
            return Some(("ClearCase", get, None));
        }
        if !readonly
            && filestat.is_some()
            && (std::env::var_os("P4PORT").is_some()
                || std::env::var_os("P4USER").is_some()
                || std::env::var_os("P4CONFIG").is_some())
        {
            let mut get = b"p4 edit ".to_vec();
            get.extend_from_slice(&Self::quote_system_arg(filename));
            return Some(("Perforce", get, None));
        }
        None
    }

    /// `version_get`: whether `filename` was got from `cs`, its status in
    /// `filestat`.
    pub fn version_get(
        &mut self,
        filename: &[u8],
        cs: &str,
        exists: bool,
        readonly: bool,
        getbuf: &[u8],
        filestat: &mut Stat,
    ) -> bool {
        if self.patch_get < 0 {
            let mut m = b"Get file ".to_vec();
            m.extend_from_slice(&self.q(filename));
            m.extend_from_slice(
                format!(
                    " from {cs}{}? [y] ",
                    if readonly { "" } else { " with lock" }
                )
                .as_bytes(),
            );
            self.ask(&m);
            if self.answer() == b'n' {
                return false;
            }
        }
        if self.dry_run {
            if !exists {
                let mut m = b"can't do dry run on nonexistent version-controlled file ".to_vec();
                m.extend_from_slice(&self.q(filename));
                m.extend_from_slice(b"; invoke '");
                m.extend_from_slice(getbuf);
                m.extend_from_slice(b"' and try again");
                self.fatal(&m);
            }
        } else {
            if self.verbosity == Verbosity::Verbose {
                let mut m = b"Getting file ".to_vec();
                m.extend_from_slice(&self.q(filename));
                m.extend_from_slice(
                    format!(
                        " from {cs}{}...\n",
                        if readonly { "" } else { " with lock" }
                    )
                    .as_bytes(),
                );
                say(&m);
            }
            if self.systemic(getbuf) != 0 {
                let mut m = b"Can't get file ".to_vec();
                m.extend_from_slice(&self.q(filename));
                m.extend_from_slice(format!(" from {cs}").as_bytes());
                self.fatal(&m);
            }
            match self.safe.stat(filename) {
                Ok(s) => *filestat = s,
                Err(e) => {
                    let m = self.q(filename);
                    self.pfatal(&m, e);
                }
            }
        }
        true
    }

    /// `systemic`: `command` run by the shell; its wait status.
    pub fn systemic(&mut self, command: &[u8]) -> i32 {
        if self.debug & 8 != 0 {
            let mut m = b"+ ".to_vec();
            m.extend_from_slice(command);
            m.push(b'\n');
            say(&m);
        }
        match coreutils::shell::shell_bytes(command).status() {
            Ok(status) => {
                #[cfg(unix)]
                {
                    use std::os::unix::process::ExitStatusExt;
                    status.into_raw()
                }
                #[cfg(not(unix))]
                {
                    status.code().unwrap_or(1)
                }
            }
            Err(_) => 127 << 8,
        }
    }

    // ----------------------------------------------------- directories ----

    /// `makedirs`: the directories `name` needs, made; errors ignored.
    pub fn makedirs(&mut self, name: &[u8]) {
        // `replace_slashes`: the slashes that end a component worth testing.
        let mut f = name.iter().take_while(|&&c| c == b'/').count();
        let mut component_start = f;
        let mut marks: Vec<usize> = Vec::new();
        while f < name.len() {
            if name.get(f) == Some(&b'/') {
                let slash = f;
                while name.get(f.saturating_add(1)) == Some(&b'/') {
                    f = f.saturating_add(1);
                }
                if f.saturating_add(1) >= name.len() {
                    break;
                }
                let dot_or_dotdot = slash.saturating_sub(component_start) <= 2
                    && name.get(component_start) == Some(&b'.')
                    && slash.checked_sub(1).and_then(|i| name.get(i)) == Some(&b'.');
                if !dot_or_dotdot {
                    marks.push(slash);
                }
                component_start = f.saturating_add(1);
            }
            f = f.saturating_add(1);
        }
        for m in marks {
            let prefix = name.get(..m).unwrap_or_default().to_vec();
            let _ = self.safe.mkdir(&prefix, 0o777);
        }
    }

    /// `removedirs`: `name`'s empty ancestor directories removed.
    pub fn removedirs(&mut self, name: &[u8]) {
        let mut i = name.len();
        while i != 0 {
            let at = |k: usize| name.get(k).copied();
            let prev = |n: usize| i.checked_sub(n).and_then(at);
            if at(i) == Some(b'/')
                && !(prev(1) == Some(b'/')
                    || (prev(1) == Some(b'.')
                        && (i == 1
                            || prev(2) == Some(b'/')
                            || (prev(2) == Some(b'.') && (i == 2 || prev(3) == Some(b'/'))))))
            {
                let dir = name.get(..i).unwrap_or_default().to_vec();
                if self.safe.rmdir(&dir).is_ok() && self.verbosity == Verbosity::Verbose {
                    let mut m = b"Removed empty directory ".to_vec();
                    m.extend_from_slice(&self.q(&dir));
                    m.push(b'\n');
                    say(&m);
                }
            }
            i = i.saturating_sub(1);
        }
    }

    // ----------------------------------------------- names in headers ----

    /// `fetchname`: the name in a header line at `at`, stripped; with
    /// `ptimestr`, what follows it; with `pstamp`, that read as a time.
    /// Nothing is set when the name is missing, malformed, `/dev/null`, or
    /// has too few components to strip.
    pub fn fetchname(
        &mut self,
        at: &[u8],
        strip_leading: i32,
        pname: &mut Option<Vec<u8>>,
        ptimestr: Option<&mut Option<Vec<u8>>>,
        pstamp: Option<&mut Timespec>,
    ) {
        let at = cstr(at);
        let skip = at.iter().take_while(|&&c| is_space(c)).count();
        let at = at.get(skip..).unwrap_or_default();
        let mut stamp = Timespec { sec: -1, nsec: 0 };
        if self.debug & 128 != 0 {
            let mut m = b"fetchname ".to_vec();
            m.extend_from_slice(at);
            m.extend_from_slice(format!(" {strip_leading}\n").as_bytes());
            say(&m);
        }
        let wants_stamp = pstamp.is_some();
        let (name, t) = if at.first() == Some(&b'"') {
            match parse_c_string(at) {
                (Some(n), end) => (cstr(&n).to_vec(), end),
                (None, _) => {
                    if self.debug & 128 != 0 {
                        let mut m = b"ignoring malformed filename ".to_vec();
                        m.extend_from_slice(&self.q(at));
                        m.push(b'\n');
                        say(&m);
                    }
                    return;
                }
            }
        } else {
            let mut t = 0usize;
            while let Some(&c) = at.get(t) {
                if is_space(c) {
                    // Allow file names with internal spaces, but only if a
                    // tab separates the file name from the date.
                    let mut u = t;
                    while at.get(u) != Some(&b'\t')
                        && at.get(u.saturating_add(1)).is_some_and(|&c| is_space(c))
                    {
                        u = u.saturating_add(1);
                    }
                    let want = if wants_stamp { b'\t' } else { b'\n' };
                    if at.get(u) != Some(&b'\t')
                        && at
                            .get(u.saturating_add(1)..)
                            .unwrap_or_default()
                            .contains(&want)
                    {
                        t = t.saturating_add(1);
                        continue;
                    }
                    break;
                }
                t = t.saturating_add(1);
            }
            (at.get(..t).unwrap_or_default().to_vec(), t)
        };

        // If the name is "/dev/null", ignore the name and mark the file as
        // being nonexistent.
        if name == b"/dev/null" {
            if let Some(p) = pstamp {
                *p = Timespec { sec: 0, nsec: 0 };
            }
            return;
        }

        // Ignore the name if it doesn't have enough slashes to strip off.
        let Some(name) = strip_leading_slashes(&name, strip_leading) else {
            return;
        };

        let rest = at.get(t..).unwrap_or_default();
        let timestr = ptimestr.is_some().then(|| {
            let mut u = rest.len();
            if u != 0 && rest.get(u.saturating_sub(1)) == Some(&b'\n') {
                u = u.saturating_sub(1);
            }
            if u != 0 && rest.get(u.saturating_sub(1)) == Some(&b'\r') {
                u = u.saturating_sub(1);
            }
            rest.get(..u).unwrap_or_default().to_vec()
        });

        if rest.first().is_some_and(|&c| c != b'\n') || rest.is_empty() {
            if rest.first() != Some(&b'\n') {
                let Some(_) = pstamp else {
                    return;
                };
                let parsed = coreutils::parse_datetime::parse_datetime(
                    rest,
                    Some(coreutils::parse_datetime::Timespec {
                        tv_sec: self.initial_time.sec,
                        tv_nsec: i32::try_from(self.initial_time.nsec).unwrap_or(0),
                    }),
                );
                if let Some(p) = parsed {
                    stamp = Timespec {
                        sec: p.tv_sec,
                        nsec: i64::from(p.tv_nsec),
                    };
                    if !(self.set_time || self.set_utc) {
                        // The header says the file is nonexistent if the
                        // timestamp is the epoch; but the listed time is
                        // local time, not UTC, and POSIX.1 allows local
                        // time offset anywhere in the range -25:00 < offset
                        // < +26:00. Match any time in that range.
                        let lower = Timespec {
                            sec: -25 * 60 * 60,
                            nsec: 0,
                        };
                        let upper = Timespec {
                            sec: 26 * 60 * 60,
                            nsec: 0,
                        };
                        if stamp > lower && stamp < upper {
                            stamp = Timespec { sec: 0, nsec: 0 };
                        }
                    }
                }
            }
        }

        *pname = Some(name);
        if let Some(pt) = ptimestr {
            *pt = timestr;
        }
        if let Some(p) = pstamp {
            *p = stamp;
        }
    }

    /// `parse_name`: a name in `diff --git`'s header, from `s`, stripped;
    /// and where it ended.
    pub fn parse_name(s: &[u8], strip_leading: i32) -> (Option<Vec<u8>>, usize) {
        let s = cstr(s);
        let skip = s.iter().take_while(|&&c| is_space(c)).count();
        let rest = s.get(skip..).unwrap_or_default();
        let (name, end) = if rest.first() == Some(&b'"') {
            match parse_c_string(rest) {
                (Some(n), e) => (cstr(&n).to_vec(), e),
                (None, e) => return (None, skip.saturating_add(e)),
            }
        } else {
            let t = rest.iter().take_while(|&&c| !is_space(c)).count();
            (rest.get(..t).unwrap_or_default().to_vec(), t)
        };
        (
            strip_leading_slashes(&name, strip_leading),
            skip.saturating_add(end),
        )
    }

    // ------------------------------------------------- temporary files ----

    /// `make_tempfile`: a new file, created exclusively, named after
    /// `real_name` beside it -- `DIR/BASE.<letter>XXXXXX` -- or with no real
    /// name, or on a dry run, in `$TMPDIR`. Its name, and its descriptor or
    /// the error.
    pub fn make_tempfile(
        &mut self,
        letter: u8,
        real_name: Option<&[u8]>,
        flags: i32,
        mode: u32,
    ) -> (Vec<u8>, Result<i32, i32>) {
        let mut template = match real_name {
            Some(r) if !self.dry_run => {
                let mut t = dir_name(r);
                t.push(b'/');
                t.extend_from_slice(&base_name(r));
                t.push(b'.');
                t.push(letter);
                t
            }
            _ => {
                let tmpdir = std::env::var_os("TMPDIR")
                    .or_else(|| std::env::var_os("TMP"))
                    .or_else(|| std::env::var_os("TEMP"))
                    .map_or_else(
                        || b"/tmp".to_vec(),
                        |d| coreutils::quote::os_bytes(&d).into_owned(),
                    );
                let mut t = tmpdir;
                t.extend_from_slice(b"/p");
                t.push(letter);
                t
            }
        };
        let stem = template.len();
        template.extend_from_slice(b"XXXXXX");
        // `try_tempname`: six letters from a value seeded by the clock and
        // the process, stepped by 7777 each time a name is taken.
        const LETTERS: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
        let t = now();
        let mut value = (u64::try_from(t.nsec / 1000).unwrap_or(0) << 16)
            ^ u64::try_from(t.sec).unwrap_or(0)
            ^ u64::from(std::process::id());
        for _ in 0..(62u32 * 62 * 62) {
            let mut v = value;
            for k in 0..6 {
                if let Some(slot) = template.get_mut(stem.saturating_add(k)) {
                    *slot = LETTERS.get((v % 62) as usize).copied().unwrap_or(b'X');
                }
                v /= 62;
            }
            // `try_safe_open`.
            let mut try_makedirs_errno = errno::ENOENT;
            let r = loop {
                match self
                    .safe
                    .open(&template, oflag::CREAT | oflag::EXCL | flags, mode)
                {
                    Err(e) if e == try_makedirs_errno => {
                        let t = template.clone();
                        self.makedirs(&t);
                        try_makedirs_errno = 0;
                    }
                    other => break other,
                }
            };
            match r {
                Ok(fd) => return (template, Ok(fd)),
                Err(errno::EEXIST) => value = value.wrapping_add(7777),
                Err(e) => return (template, Err(e)),
            }
        }
        (template, Err(errno::EEXIST))
    }

    /// `O_NOFOLLOW` unless `--follow-symlinks`: Debian's fix for
    /// CVE-2019-13636, wherever a file is opened to be read or written.
    pub fn nofollow(&self) -> i32 {
        if self.follow_symlinks {
            0
        } else {
            oflag::NOFOLLOW
        }
    }

    /// `stat_file`: `lstat`, or with `--follow-symlinks` `stat`, through
    /// the safe walk; the `errno` on failure.
    pub fn stat_file(&mut self, filename: &[u8]) -> Result<Stat, i32> {
        if self.follow_symlinks {
            self.safe.stat(filename)
        } else {
            self.safe.lstat(filename)
        }
    }

    /// `cwd_is_root`: whether the working directory is `/`.
    pub fn cwd_is_root(&mut self, _name: &[u8]) -> bool {
        match (
            sys::stat_at(sys::AT_FDCWD, b"/", 0),
            sys::stat_at(sys::AT_FDCWD, b".", 0),
        ) {
            (Ok(r), Ok(c)) => r.dev == c.dev && r.ino == c.ino,
            _ => false,
        }
    }

    /// `find_backup_file_name`, gnulib's with `simple_backup_suffix` as
    /// `patch` sets it.
    pub fn find_backup_file_name(&mut self, file: &[u8], backup_type: BackupType) -> Vec<u8> {
        crate::backupfile::find_backup_file_name(file, backup_type, &self.simple_backup_suffix)
    }

    /// `get_version`: the backup type `version` names, or the default.
    pub fn get_version(&mut self, context: &str, version: Option<&[u8]>) -> BackupType {
        match crate::backupfile::get_version(version) {
            Ok(t) => t,
            Err(ambiguous) => {
                // `XARGMATCH`: the complaint, the list, and `exit
                // (exit_failure)` -- no "Try --help".
                let name = self.program_name.clone();
                crate::backupfile::complain(&name, context, version.unwrap_or_default(), ambiguous);
                std::process::exit(2);
            }
        }
    }

    /// `usage (stderr, 2)`, for the callers outside `main.rs`.
    pub fn usage_failure(&mut self) -> ! {
        let mut m = self.program_name.clone();
        m.extend_from_slice(b": Try '");
        m.extend_from_slice(&self.program_name);
        m.extend_from_slice(b" --help' for more information.\n");
        print_stderr(&m);
        std::process::exit(2);
    }

    /// Written to the output: `fwrite`, failure fatal.
    pub fn write_or_die(&mut self, w: &mut dyn Write, bytes: &[u8]) {
        if w.write_all(bytes).is_err() {
            self.write_fatal();
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn leading_components_are_stripped_as_strip_leading_slashes_does() {
        assert_eq!(strip_leading_slashes(b"a/b/c", -1).unwrap(), b"c");
        assert_eq!(strip_leading_slashes(b"a/b/c", 0).unwrap(), b"a/b/c");
        assert_eq!(strip_leading_slashes(b"a/b/c", 1).unwrap(), b"b/c");
        assert_eq!(strip_leading_slashes(b"a//b/c", 2).unwrap(), b"c");
        assert!(strip_leading_slashes(b"a/b", 2).is_none());
        assert!(strip_leading_slashes(b"a/", 1).is_none());
        assert_eq!(strip_leading_slashes(b"/a/b", 1).unwrap(), b"a/b");
    }

    #[test]
    fn c_strings_decode_as_parse_c_string_does() {
        assert_eq!(parse_c_string(b"\"a\\tb\" rest").0.unwrap(), b"a\tb");
        assert_eq!(parse_c_string(b"\"\\303\\251\"").0.unwrap(), b"\xc3\xa9");
        assert!(parse_c_string(b"\"unterminated").0.is_none());
        assert!(parse_c_string(b"\"bad\\q\"").0.is_none());
        assert!(parse_c_string(b"\"\\4ab\"").0.is_none());
    }

    #[test]
    fn unsafe_names_are_those_filename_is_safe_refuses() {
        assert!(filename_is_safe(b"a/b"));
        assert!(filename_is_safe(b"a/..b"));
        assert!(filename_is_safe(b".hidden"));
        assert!(!filename_is_safe(b"/etc/passwd"));
        assert!(!filename_is_safe(b"../x"));
        assert!(!filename_is_safe(b"a/../x"));
        assert!(!filename_is_safe(b"a/.."));
    }

    #[test]
    fn dir_and_base_names_are_gnulibs() {
        assert_eq!(dir_name(b"file"), b".");
        assert_eq!(dir_name(b"a/b"), b"a");
        assert_eq!(dir_name(b"/x"), b"/");
        assert_eq!(dir_name(b"a//b"), b"a");
        assert_eq!(base_name(b"a/b"), b"b");
        assert_eq!(base_name(b"a/b/"), b"b/");
        assert_eq!(base_name(b"/"), b"/");
        assert_eq!(base_len(b"ab//"), 2);
    }

    #[test]
    fn quoting_styles_resolve_by_prefix() {
        assert_eq!(quoting_style(b"c").unwrap(), quoting::Style::C);
        assert_eq!(quoting_style(b"lit").unwrap(), quoting::Style::Literal);
        assert_eq!(quoting_style(b"s"), Err(true));
        assert_eq!(quoting_style(b"zzz"), Err(false));
    }
}
