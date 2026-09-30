// Offsets and lengths in this file are within one buffer's length -- a
// database's text, or the caller's `_r` buffer -- which bounds every sum
// made of them; the few that could pass it are checked.  Clippy cannot see
// the bounds.
#![allow(clippy::arithmetic_side_effects)]
//! The flat-file databases -- `/etc/passwd`, `/etc/group`, `/etc/shadow` --
//! as glibc 2.39's `nss_files` reads them: the part [`crate::pwd`] and
//! [`crate::shadow`] share.
//!
//! - **Reading.** A database is read whole, into a `malloc` block
//!   ([`Text`]). A file that does not exist is [`Db::Missing`], which each
//!   database answers with its built-in `root` entry (design-decisions
//!   §1113); one that cannot be read is an error, passed on.
//! - **Lines** are `__nss_readline`'s: leading white space is skipped, and
//!   empty lines and `#` comments are not entries.
//! - **Fields** are `files-parse.c`'s: `:`-separated strings, a missing one
//!   empty; numbers are `strtoull` in base 10, and a line whose number is
//!   malformed, or past 32 bits as Debian's glibc has it (design-decisions
//!   §1136), is not an entry.
//!
//! Host tests read each database from a per-thread hook instead -- as text,
//! as missing, or as an error ([`set_test_text`], [`set_test_error`]) --
//! since the host has no filesystem behind `open`.

use crate::errno;

/// Which database.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Which {
    Passwd,
    Group,
    Shadow,
    /// `/etc/gshadow` -- the groups' shadow file.
    Gshadow,
    Services,
    Protocols,
    Networks,
    Ethers,
    Hosts,
    /// `/etc/host.conf` -- not a database, but read the same way.
    HostConf,
    /// `/etc/gai.conf` -- likewise.
    GaiConf,
    /// `/etc/shells` -- the login shells `getusershell` lists.
    Shells,
}

impl Which {
    /// How many there are: the host tests keep a slot for each.
    #[cfg(test)]
    const COUNT: usize = 12;
}

impl Which {
    /// The file, NUL-terminated.
    fn path(self) -> &'static [u8] {
        match self {
            Which::Passwd => b"/etc/passwd\0",
            Which::Group => b"/etc/group\0",
            Which::Shadow => b"/etc/shadow\0",
            Which::Gshadow => b"/etc/gshadow\0",
            Which::Services => b"/etc/services\0",
            Which::Protocols => b"/etc/protocols\0",
            Which::Networks => b"/etc/networks\0",
            Which::Ethers => b"/etc/ethers\0",
            Which::Hosts => b"/etc/hosts\0",
            Which::HostConf => b"/etc/host.conf\0",
            Which::GaiConf => b"/etc/gai.conf\0",
            Which::Shells => b"/etc/shells\0",
        }
    }
}

/// A database's bytes, owned: a `malloc` block freed when this goes.
pub(crate) struct Text {
    ptr: *mut u8,
    len: usize,
}

impl Text {
    pub(crate) fn bytes(&self) -> &[u8] {
        if self.ptr.is_null() {
            return &[];
        }
        // SAFETY: `ptr` holds `len` initialised bytes, owned by `self`.
        unsafe { core::slice::from_raw_parts(self.ptr, self.len) }
    }

    /// A copy of `bytes`, or `None` when memory runs out.
    #[cfg(test)]
    fn copy_of(bytes: &[u8]) -> Option<Self> {
        let ptr = crate::malloc::malloc(bytes.len().max(1));
        if ptr.is_null() {
            return None;
        }
        // SAFETY: `ptr` is a fresh block of at least `bytes.len()` bytes.
        unsafe { core::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr, bytes.len()) };
        Some(Self {
            ptr,
            len: bytes.len(),
        })
    }
}

impl Text {
    /// The block and its length, now the caller's to free.
    pub(crate) fn into_raw(self) -> (*mut u8, usize) {
        let t = core::mem::ManuallyDrop::new(self);
        (t.ptr, t.len)
    }
}

impl Drop for Text {
    fn drop(&mut self) {
        // SAFETY: `ptr` is null or this `Text`'s own `malloc` block.
        unsafe { crate::malloc::free(self.ptr) };
    }
}

/// What reading a database found.
pub(crate) enum Db {
    /// Its text.
    Text(Text),
    /// No such file: the database answers with its built-in entry.
    Missing,
}

/// Read database `which` whole: `Err` carries the `errno` for a file that
/// exists but cannot be read (`EACCES` for `/etc/shadow` without the
/// privilege, `ENOMEM`, `EIO`...).
pub(crate) fn read(which: Which) -> Result<Db, i32> {
    #[cfg(test)]
    {
        // SAFETY: this thread's slots, not borrowed elsewhere.
        let slots = unsafe { &*test_dbs() };
        match slots
            .get(which as usize)
            .copied()
            .unwrap_or(TestDb::Missing)
        {
            TestDb::Text(bytes) => Text::copy_of(bytes).map(Db::Text).ok_or(errno::ENOMEM),
            TestDb::Missing => Ok(Db::Missing),
            TestDb::Error(e) => Err(e),
        }
    }
    #[cfg(not(test))]
    {
        read_file(which.path())
    }
}

/// Read the file at `path` (NUL-terminated) whole, as [`read`] reads a
/// database: for a configuration file named at run time.
#[cfg(not(test))]
pub(crate) fn read_path(path: &[u8]) -> Result<Db, i32> {
    read_file(path)
}

/// Read a whole file into a growing block.
fn read_file(path: &[u8]) -> Result<Db, i32> {
    let fd = crate::file::open(
        path.as_ptr(),
        crate::fcntl::O_RDONLY | crate::fcntl::O_CLOEXEC,
        0,
    );
    if fd < 0 {
        let e = errno::get_errno();
        return if e == errno::ENOENT {
            Ok(Db::Missing)
        } else {
            Err(e)
        };
    }
    let mut text = Text {
        ptr: core::ptr::null_mut(),
        len: 0,
    };
    let mut cap = 0usize;
    let result = loop {
        if text.len == cap {
            let Some(bigger) = cap.checked_mul(2).map(|c| c.max(4096)) else {
                break Err(errno::ENOMEM);
            };
            // SAFETY: `ptr` is null or `text`'s block; realloc keeps it on
            // failure, where `text` still owns it.
            let p = unsafe { crate::malloc::realloc(text.ptr, bigger) };
            if p.is_null() {
                break Err(errno::ENOMEM);
            }
            text.ptr = p;
            cap = bigger;
        }
        // SAFETY: `ptr` holds `cap` bytes, of which `len` are read.
        let r = crate::file::read(fd, unsafe { text.ptr.add(text.len) }, cap - text.len);
        match usize::try_from(r) {
            Ok(0) => break Ok(()),
            Ok(n) => text.len += n,
            Err(_) => {
                let e = errno::get_errno();
                if e != errno::EINTR {
                    break Err(e);
                }
            }
        }
    };
    // A read-only descriptor's close cannot lose anything.
    let _ = crate::file::close(fd);
    result.map(|()| Db::Text(text))
}

/// `__nss_readline`'s entries: each line as glibc's parsers see it -- up to
/// its first NUL, since they read a C string, with its leading white space
/// skipped -- and neither empty nor a `#` comment; with the offset just past
/// it, so enumeration can resume there.
pub(crate) fn lines(text: &[u8], from: usize) -> impl Iterator<Item = (&[u8], usize)> + Clone {
    let mut at = from;
    core::iter::from_fn(move || {
        loop {
            let rest = text.get(at..)?;
            if rest.is_empty() {
                return None;
            }
            let (line, next) = match rest.iter().position(|&b| b == b'\n') {
                Some(i) => (rest.get(..i).unwrap_or(&[]), at + i + 1),
                None => (rest, text.len()),
            };
            at = next;
            if let Some(line) = entry_text(line) {
                return Some((line, next));
            }
        }
    })
}

/// C's `isspace` in the C locale.
pub(crate) fn is_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}

/// A line being taken apart field by field, as `files-parse.c`'s macros
/// take it.
pub(crate) struct Fields<'a> {
    rest: &'a [u8],
}

impl<'a> Fields<'a> {
    pub(crate) fn new(line: &'a [u8]) -> Self {
        Self { rest: line }
    }

    /// Whether the line has ended (`*line == '\0'`).
    pub(crate) fn at_end(&self) -> bool {
        self.rest.is_empty()
    }

    /// `STRING_FIELD`: up to the next `:`, which is consumed; the rest of
    /// the line when there is none, and "" once the line has ended.
    pub(crate) fn string(&mut self) -> &'a [u8] {
        match self.rest.iter().position(|&b| b == b':') {
            Some(i) => {
                let field = self.rest.get(..i).unwrap_or(&[]);
                self.rest = self.rest.get(i + 1..).unwrap_or(&[]);
                field
            }
            None => core::mem::take(&mut self.rest),
        }
    }

    /// The rest of the line (`pw_shell = line`).
    pub(crate) fn rest(&mut self) -> &'a [u8] {
        core::mem::take(&mut self.rest)
    }

    /// `INT_FIELD`: a number, which must be followed by `:` (consumed) or by
    /// the end of the line; `None` makes the line no entry.
    pub(crate) fn int(&mut self) -> Option<u32> {
        let Number::Fits(value, used) = strtou32(self.rest) else {
            return None;
        };
        self.after_number(used)?;
        Some(value)
    }

    /// `INT_FIELD_MAYBE_NULL`: as [`Fields::int`], but an empty field is
    /// `None` rather than an error -- and the line must not have ended
    /// before it.  `Err(())` makes the line no entry.
    pub(crate) fn int_or_empty(&mut self) -> Result<Option<u32>, ()> {
        if self.rest.is_empty() {
            return Err(());
        }
        match strtou32(self.rest) {
            Number::Fits(value, used) => {
                self.after_number(used).ok_or(())?;
                Ok(Some(value))
            }
            Number::Absent => {
                // No digits: the field is empty, and what follows it is
                // judged as after a number.
                self.after_number(0).ok_or(())?;
                Ok(None)
            }
            Number::PastU32 => Err(()),
        }
    }

    /// `INT_FIELD_MAYBE_NULL` with a terminator that never matches: the
    /// number must end the line.
    pub(crate) fn last_int_or_empty(&mut self) -> Result<Option<u32>, ()> {
        if self.rest.is_empty() {
            return Err(());
        }
        let (value, used) = match strtou32(self.rest) {
            Number::Fits(v, used) => (Some(v), used),
            Number::Absent => (None, 0),
            Number::PastU32 => return Err(()),
        };
        if self.rest.len() != used {
            return Err(());
        }
        self.rest = &[];
        Ok(value)
    }

    /// Skip leading white space (the shadow parser's "old form" test).
    pub(crate) fn skip_space(&mut self) {
        let n = self.rest.iter().take_while(|&&b| is_space(b)).count();
        self.rest = self.rest.get(n..).unwrap_or(&[]);
    }

    fn after_number(&mut self, used: usize) -> Option<()> {
        let tail = self.rest.get(used..)?;
        match tail.first() {
            Some(b':') => self.rest = tail.get(1..).unwrap_or(&[]),
            None => self.rest = &[],
            Some(_) => return None,
        }
        Some(())
    }
}

/// What a number field starts with, as [`strtou32`] reads it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Number {
    /// No digits (`endp == nptr`): an empty field, where one may be empty.
    Absent,
    /// A number that fits in 32 bits, and the bytes it used.
    Fits(u32, usize),
    /// A number past 32 bits -- a negative one among them, which `strtoull`
    /// negates as unsigned, so anything but `-0`. Debian's glibc makes the
    /// line no entry (design-decisions §1136).
    PastU32,
}

/// The number at the start of a field, as Debian's glibc reads it
/// (`local-nss-overflow.diff`, in Debian and Ubuntu since 2009): `strtoull`
/// in base 10 -- leading white space, a sign, digits -- and a value past
/// `UINT_MAX` refuses the line. Upstream glibc's `strtou32` clamps it to
/// `0xffffffff` instead, which is `(uid_t) -1`: to `setresuid` that means
/// "leave unchanged", so a daemon dropping root to such a user would keep
/// root and be told it had succeeded (design-decisions §1136).
pub(crate) fn strtou32(s: &[u8]) -> Number {
    let mut i = s.iter().take_while(|&&b| is_space(b)).count();
    let negative = match s.get(i) {
        Some(b'-') => {
            i += 1;
            true
        }
        Some(b'+') => {
            i += 1;
            false
        }
        _ => false,
    };
    let digits = s
        .get(i..)
        .unwrap_or(&[])
        .iter()
        .take_while(|b| b.is_ascii_digit())
        .count();
    if digits == 0 {
        return Number::Absent;
    }
    // `None` once the digits overflow 64 bits: strtoull then answers
    // ULLONG_MAX, unnegated, which is past 32 bits whatever the sign.
    let mut value = Some(0u64);
    for &d in s.get(i..i + digits).unwrap_or(&[]) {
        value = value
            .and_then(|v| v.checked_mul(10))
            .and_then(|v| v.checked_add(u64::from(d - b'0')));
    }
    // A negative number is negated as unsigned: anything but -0 comes out
    // past 32 bits.
    let value = match value {
        Some(v) if negative => v.wrapping_neg(),
        Some(v) => v,
        None => return Number::PastU32,
    };
    match u32::try_from(value) {
        Ok(v) => Number::Fits(v, i + digits),
        Err(_) => Number::PastU32,
    }
}

/// Room for `need` bytes at `buf`, as the `_r` functions have it: `ERANGE`
/// otherwise.
pub(crate) struct Room {
    buf: *mut u8,
    len: usize,
    used: usize,
}

impl Room {
    /// # Safety
    ///
    /// `buf` is writable for `len` bytes, for as long as the `Room` and the
    /// pointers it hands out are used.
    pub(crate) unsafe fn new(buf: *mut u8, len: usize) -> Self {
        Self { buf, len, used: 0 }
    }

    /// Copy `s` in with a NUL after it; the copy's address.
    pub(crate) fn string(&mut self, s: &[u8]) -> Result<*mut u8, i32> {
        let end = self
            .used
            .checked_add(s.len())
            .and_then(|e| e.checked_add(1))
            .ok_or(errno::ERANGE)?;
        if end > self.len {
            return Err(errno::ERANGE);
        }
        // SAFETY: `used..end` lies inside the caller's `len` bytes.
        let at = unsafe { self.buf.add(self.used) };
        // SAFETY: as above; `s` cannot overlap a buffer we are filling from
        // a database we own.
        unsafe {
            core::ptr::copy_nonoverlapping(s.as_ptr(), at, s.len());
            at.add(s.len()).write(0);
        }
        self.used = end;
        Ok(at)
    }

    /// Copy `s` in as it is, with no NUL; the copy's address.
    pub(crate) fn bytes(&mut self, s: &[u8]) -> Result<*mut u8, i32> {
        let end = self.used.checked_add(s.len()).ok_or(errno::ERANGE)?;
        if end > self.len {
            return Err(errno::ERANGE);
        }
        // SAFETY: `used..end` lies inside the caller's `len` bytes.
        let at = unsafe { self.buf.add(self.used) };
        // SAFETY: as above; `s` is not the buffer being filled.
        unsafe { core::ptr::copy_nonoverlapping(s.as_ptr(), at, s.len()) };
        self.used = end;
        Ok(at)
    }

    /// The whole buffer's length.
    pub(crate) fn capacity(&self) -> usize {
        self.len
    }

    /// Room for `n` pointers, aligned for them by address (`parse_list`
    /// aligns the same way), zeroed.
    pub(crate) fn pointers(&mut self, n: usize) -> Result<*mut *const u8, i32> {
        let align = align_of::<*const u8>();
        // SAFETY: only an address is computed.
        let addr = unsafe { self.buf.add(self.used) } as usize;
        let pad = addr.next_multiple_of(align) - addr;
        let bytes = n.checked_mul(size_of::<*const u8>()).ok_or(errno::ERANGE)?;
        let start = self.used.checked_add(pad).ok_or(errno::ERANGE)?;
        let end = start.checked_add(bytes).ok_or(errno::ERANGE)?;
        if end > self.len {
            return Err(errno::ERANGE);
        }
        // SAFETY: `start..end` lies inside the buffer and is aligned.
        let at = unsafe { self.buf.add(start) }.cast::<*const u8>();
        // SAFETY: as above.
        unsafe { core::ptr::write_bytes(at, 0, n) };
        self.used = end;
        Ok(at)
    }
}

// ---------------------------------------------------------------------------
// What every database's functions share
// ---------------------------------------------------------------------------

/// Run `f` on database `which`'s text: the file's, or `builtin` when there
/// is no file. `Err` is the `errno` of a file that could not be read.
pub(crate) fn with_text<R>(
    which: Which,
    builtin: &'static [u8],
    f: impl FnOnce(&[u8]) -> R,
) -> Result<R, i32> {
    match read(which)? {
        Db::Text(t) => Ok(f(t.bytes())),
        Db::Missing => Ok(f(builtin)),
    }
}

/// A NIS compat name (`+` or `-`), which lookups skip.
pub(crate) fn is_compat(name: &[u8]) -> bool {
    matches!(name.first(), Some(b'+' | b'-'))
}

/// A NUL-terminated string's bytes.
///
/// # Safety
///
/// `name` is non-null and NUL-terminated.
pub(crate) unsafe fn c_bytes<'a>(name: *const u8) -> &'a [u8] {
    // SAFETY: the caller's contract.
    unsafe { core::slice::from_raw_parts(name, crate::string::strlen(name)) }
}

/// A string of the entry in `room`, or NULL for a field the line did not
/// have.
pub(crate) fn string_or_null(room: &mut Room, s: Option<&[u8]>) -> Result<*const u8, i32> {
    match s {
        Some(s) => room.string(s).map(<*mut u8>::cast_const),
        None => Ok(core::ptr::null()),
    }
}

/// Write `value` to `out` and `out` to `result`: the `_r` functions' success.
///
/// # Safety
///
/// `out` and `result` are valid for writes.
pub(crate) unsafe fn deliver<T>(value: T, out: *mut T, result: *mut *const T) {
    // SAFETY: the caller's contract.
    unsafe {
        out.write(value);
        result.write(out.cast_const());
    }
}

/// The `_r` lookups' common shape: `EFAULT` for a NULL output, `*result` NULL
/// until something is found, and `errno` set to what they return -- 0 for
/// found and for not found alike, as glibc's `getXXbyYY_r` does.
///
/// # Safety
///
/// Non-null pointers are the caller's: `out` and `result` writable, `buf`
/// writable for `buflen` bytes.
pub(crate) unsafe fn reentrant<T>(
    out: *mut T,
    buf: *mut u8,
    buflen: usize,
    result: *mut *const T,
    find: impl FnOnce(&mut Room) -> Result<Option<T>, i32>,
) -> i32 {
    if out.is_null() || buf.is_null() || result.is_null() {
        return errno::EFAULT;
    }
    // SAFETY: `result` is non-null and the caller's to write.
    unsafe { result.write(core::ptr::null()) };
    // SAFETY: the caller gives `buflen` writable bytes at `buf`.
    let mut room = unsafe { Room::new(buf, buflen) };
    let rc = match find(&mut room) {
        Ok(Some(value)) => {
            // SAFETY: both are the caller's, checked non-null above.
            unsafe { deliver(value, out, result) };
            0
        }
        Ok(None) => 0,
        Err(e) => e,
    };
    errno::set_errno(rc);
    rc
}

/// An entry a non-reentrant function returns, and the block its strings
/// live in -- this process's, reused by the next call, as POSIX allows.
pub(crate) struct Held<T> {
    entry: T,
    buf: *mut u8,
    cap: usize,
}

impl<T> Held<T> {
    pub(crate) const fn new(entry: T) -> Self {
        Self {
            entry,
            buf: core::ptr::null_mut(),
            cap: 0,
        }
    }

    /// Free the block: the entry it held is gone with it.
    pub(crate) fn release(&mut self) {
        // SAFETY: `buf` is NULL or this storage's `malloc` block.
        unsafe { crate::malloc::free(self.buf) };
        self.buf = core::ptr::null_mut();
        self.cap = 0;
    }
}

/// glibc's `NSS_BUFLEN_PASSWD`, `NSS_BUFLEN_GROUP` and `NSS_BUFLEN_SHADOW`:
/// where a [`Held`] block starts before it doubles.
const BUFLEN: usize = 1024;

/// Run a `_r` function into `held`, doubling the block while it answers
/// `ERANGE`, as glibc's `getXXbyYY` and `__nss_getent` do. The entry, or
/// NULL: `Err` carries the `_r` function's other answer.
pub(crate) fn hold<T>(
    held: *mut Held<T>,
    mut call: impl FnMut(*mut T, *mut u8, usize, *mut *const T) -> i32,
) -> Result<*const T, i32> {
    // SAFETY: this process's storage, used by one call at a time.
    let h = unsafe { &mut *held };
    loop {
        if h.buf.is_null() || h.cap == 0 {
            let p = crate::malloc::malloc(BUFLEN);
            if p.is_null() {
                return Err(errno::ENOMEM);
            }
            h.buf = p;
            h.cap = BUFLEN;
        }
        let mut result: *const T = core::ptr::null();
        let rc = call(&raw mut h.entry, h.buf, h.cap, &raw mut result);
        if rc != errno::ERANGE {
            return if rc == 0 { Ok(result) } else { Err(rc) };
        }
        let Some(cap) = h.cap.checked_mul(2) else {
            return Err(errno::ENOMEM);
        };
        // SAFETY: `buf` is this storage's block; on failure it is kept.
        let p = unsafe { crate::malloc::realloc(h.buf, cap) };
        if p.is_null() {
            return Err(errno::ENOMEM);
        }
        h.buf = p;
        h.cap = cap;
    }
}

/// A lookup's pointer, with `errno` as glibc leaves it: 0 when found or not
/// found, the error otherwise.
pub(crate) fn lookup_result<T>(r: Result<*const T, i32>) -> *const T {
    match r {
        Ok(p) => {
            errno::set_errno(0);
            p
        }
        Err(e) => {
            errno::set_errno(e);
            core::ptr::null()
        }
    }
}

/// An enumeration's pointer: NULL at the end with `errno` untouched, as
/// glibc's `getpwent` leaves it; `errno` set for a real error.
pub(crate) fn enumerated<T>(r: Result<*const T, i32>) -> *const T {
    match r {
        Ok(p) => p,
        Err(e) => {
            if e != errno::ENOENT {
                errno::set_errno(e);
            }
            core::ptr::null()
        }
    }
}

/// Where an enumeration is: its database's text once opened, and the offset
/// of the next line.
pub(crate) struct Cursor {
    open: Option<Opened>,
    at: usize,
}

enum Opened {
    File(Text),
    Builtin,
    /// The file could not be read: the call answers `ENOENT`, as glibc's
    /// does for an unavailable database.
    Unreadable,
}

impl Cursor {
    /// Not open: the next `get*ent` reads the database afresh
    /// (`set*ent`/`end*ent`).
    pub(crate) const CLOSED: Self = Self { open: None, at: 0 };

    fn open(which: Which) -> Opened {
        match read(which) {
            Ok(Db::Text(t)) => Opened::File(t),
            Ok(Db::Missing) => Opened::Builtin,
            Err(_) => Opened::Unreadable,
        }
    }
}

/// The next entry of an enumeration, which `take` parses and fills -- `None`
/// for a line that is no entry: 0, `ENOENT` at the end, or the fill's
/// `ERANGE`, after which the same entry comes again.  `errno` is glibc's:
/// the fill's error, and otherwise as it was -- at the end, and when the
/// file cannot be read.
///
/// # Safety
///
/// As [`reentrant`]; `cursor` is this process's.
#[allow(clippy::too_many_arguments)] // the `_r` interface's four, plus the database's four
pub(crate) unsafe fn next_entry<T>(
    cursor: *mut Cursor,
    which: Which,
    builtin: &'static [u8],
    take: impl Fn(&[u8], &mut Room) -> Option<Result<T, i32>>,
    out: *mut T,
    buf: *mut u8,
    buflen: usize,
    result: *mut *const T,
) -> i32 {
    if out.is_null() || buf.is_null() || result.is_null() {
        return errno::EFAULT;
    }
    // SAFETY: `result` is the caller's, non-null.
    unsafe { result.write(core::ptr::null()) };
    // SAFETY: this process's cursor, used by one call at a time.
    let c = unsafe { &mut *cursor };
    let opened = c.open.get_or_insert_with(|| Cursor::open(which));
    let text: &[u8] = match opened {
        Opened::File(t) => t.bytes(),
        Opened::Builtin => builtin,
        Opened::Unreadable => {
            // Tried again next time, as glibc opens the file again on every
            // call while it has none open.
            c.open = None;
            return errno::ENOENT;
        }
    };
    for (line, next) in lines(text, c.at) {
        // SAFETY: the caller gives `buflen` writable bytes at `buf`.
        let mut room = unsafe { Room::new(buf, buflen) };
        let Some(filled) = take(line, &mut room) else {
            c.at = next;
            continue;
        };
        return match filled {
            Ok(value) => {
                c.at = next;
                // SAFETY: the caller's, checked non-null above.
                unsafe { deliver(value, out, result) };
                0
            }
            // In `errno` too, as glibc's backend reports it
            // (`*errnop = ERANGE`) -- where success and the end leave
            // `errno` as it was.
            Err(err) => {
                errno::set_errno(err);
                err
            }
        };
    }
    c.at = text.len();
    errno::ENOENT
}

// ---------------------------------------------------------------------------
// The caller's stream: the fgetXXent readers and the putXXent writers
// ---------------------------------------------------------------------------

/// A line as glibc's parsers see it: a C string, so it ends at its first NUL
/// -- or at its newline -- with its leading white space skipped; `None` for
/// a line that is empty then, or a `#` comment (`__nss_readline`).
fn entry_text(raw: &[u8]) -> Option<&[u8]> {
    let end = raw
        .iter()
        .position(|&b| b == 0 || b == b'\n')
        .unwrap_or(raw.len());
    let line = raw.get(..end).unwrap_or(&[]);
    let from = line
        .iter()
        .position(|&b| !is_space(b))
        .unwrap_or(line.len());
    let line = line.get(from..).unwrap_or(&[]);
    (!line.is_empty() && line.first() != Some(&b'#')).then_some(line)
}

/// The next entry of `stream`, read as glibc's `__nss_fgetent_r` reads it:
/// line by line -- blank lines and `#` comments passed over, and a line
/// `place` finds no entry in skipped, with `errno` set to `EINVAL` as
/// glibc's `__nss_parse_line_result` sets it -- until `place` makes an entry
/// of one. `Ok` with what `place` made; `Err` with `ENOENT` at the end of the
/// stream, the stream's own error, or `place`'s.
///
/// `place` is also given the line's length as read, newline and all: glibc
/// reads a line into the caller's buffer, which is too small for it unless
/// it has two bytes to spare, whatever the line holds.
///
/// `place`'s `ERANGE` -- the caller's buffer is too small -- puts the stream
/// back at the start of the line, so that the next call, with a bigger
/// buffer, reads the same entry; a stream that cannot seek back is marked in
/// error and the answer is `ESPIPE`, as glibc's is, since a retry would miss
/// the entry. The stream is held (`flockfile`) for the whole call.
///
/// # Safety
///
/// `stream` is an open stream.
pub(crate) unsafe fn fget_entry<R>(
    stream: *mut u8,
    mut place: impl FnMut(&[u8], usize) -> Option<Result<R, i32>>,
) -> Result<R, i32> {
    crate::stdio::flockfile(stream.cast());
    let mut line: *mut u8 = core::ptr::null_mut();
    let mut cap = 0usize;
    let result = loop {
        let start = crate::stdio::ftello(stream);
        // SAFETY: `line`/`cap` are this call's buffer; the stream the caller's.
        let got = unsafe { crate::stdio::read_line_as_fgets(&raw mut line, &raw mut cap, stream) };
        let n = match got {
            Ok(0) => break Err(errno::ENOENT),
            Ok(n) => n,
            // An ERANGE would read as "buffer too small" to the caller, who
            // would retry for ever: glibc makes it EINVAL.
            Err(errno::ERANGE) => break Err(errno::EINVAL),
            Err(e) => break Err(e),
        };
        // SAFETY: the read wrote `n` bytes at `line`.
        let raw = unsafe { core::slice::from_raw_parts(line, n) };
        let placed = match entry_text(raw) {
            Some(text) => place(text, n),
            // Blank or a comment -- but read into the caller's buffer first,
            // so a long one is too big for a small buffer all the same.
            None => place(&[], n).filter(|r| matches!(r, Err(e) if *e == errno::ERANGE)),
        };
        match placed {
            None => {
                if entry_text(raw).is_some() {
                    errno::set_errno(errno::EINVAL);
                }
            }
            Some(Err(errno::ERANGE)) => {
                break Err(
                    if start >= 0
                        && crate::stdio::fseeko(stream, start, crate::stdio::SEEK_SET) == 0
                    {
                        errno::ERANGE
                    } else {
                        crate::stdio::set_stream_error(stream);
                        errno::ESPIPE
                    },
                );
            }
            Some(other) => break other,
        }
    };
    // SAFETY: the read's block, or NULL.
    unsafe { crate::malloc::free(line) };
    crate::stdio::funlockfile(stream.cast());
    result
}

/// Whether a buffer of `buflen` bytes holds a line of `len` bytes as glibc's
/// `__nss_readline` reads it: `fgets` with a marker in the last byte, so the
/// line needs two to spare.
fn line_fits(len: usize, buflen: usize) -> bool {
    len.checked_add(2).is_some_and(|need| need <= buflen)
}

/// The `fgetXXent_r` functions, over [`fget_entry`]: the entry `take` makes
/// of the next line, into the caller's structure and buffer. 0 with
/// `*result` set; `ENOENT` at the end; `ERANGE` (the same entry comes next
/// time); `ESPIPE` if it cannot; the stream's error. `errno` is set to what
/// is returned, when that is not 0, as glibc's is.
///
/// A NULL stream is `EBADF` (design-decisions §1120) and a NULL `out`, `buf`
/// or `result` `EFAULT` (§303), where glibc would fault; a `buflen` below
/// 3 -- a character, its newline and the NUL -- is `ERANGE`, before
/// anything is read, as glibc's.
///
/// # Safety
///
/// `stream` is NULL or an open stream; non-null `out` and `result` are
/// writable, and `buf` writable for `buflen` bytes.
pub(crate) unsafe fn fget_reentrant<T>(
    stream: *mut u8,
    take: impl Fn(&[u8], &mut Room) -> Option<Result<T, i32>>,
    out: *mut T,
    buf: *mut u8,
    buflen: usize,
    result: *mut *const T,
) -> i32 {
    if !result.is_null() {
        // SAFETY: the caller's, non-null.
        unsafe { result.write(core::ptr::null()) };
    }
    let rc = if stream.is_null() {
        errno::EBADF
    } else if buflen < 3 {
        errno::ERANGE
    } else if out.is_null() || buf.is_null() || result.is_null() {
        errno::EFAULT
    } else {
        let place = |text: &[u8], len: usize| {
            if !line_fits(len, buflen) {
                return Some(Err(errno::ERANGE));
            }
            if text.is_empty() {
                return None;
            }
            // SAFETY: the caller gives `buflen` writable bytes at `buf`.
            let mut room = unsafe { Room::new(buf, buflen) };
            take(text, &mut room)
        };
        // SAFETY: an open stream, the caller's.
        let got = unsafe { fget_entry(stream, place) };
        match got {
            Ok(value) => {
                // SAFETY: the caller's, checked non-null above.
                unsafe { deliver(value, out, result) };
                return 0;
            }
            Err(e) => e,
        }
    };
    errno::set_errno(rc);
    rc
}

/// The `fgetXXent` functions: the next entry of `stream`, in this process's
/// `held` storage -- which grows until the entry fits, in `BUFLEN` steps as
/// glibc's buffer does, with `errno` set to `ERANGE` when it had to, as
/// glibc's retry leaves it. The line is read once, so unlike glibc's, which
/// re-reads it after growing and so refuses a stream it cannot seek
/// (`fgetpos` first), this reads a pipe too. NULL at the end, `errno`
/// `ENOENT`, or on an error, with its `errno`; a NULL stream is `EBADF`.
///
/// # Safety
///
/// `stream` is NULL or an open stream; `held` is this process's storage,
/// used by one call at a time.
pub(crate) unsafe fn fget_held<T>(
    stream: *mut u8,
    held: *mut Held<T>,
    take: impl Fn(&[u8], &mut Room) -> Option<Result<T, i32>>,
) -> *const T {
    if stream.is_null() {
        errno::set_errno(errno::EBADF);
        return core::ptr::null();
    }
    // SAFETY: this process's storage, used by one call at a time.
    let h = unsafe { &mut *held };
    // SAFETY: an open stream, the caller's.
    let got = unsafe {
        fget_entry(stream, |text, len| {
            // The buffer glibc would have read the line into: grown to hold
            // it, in the steps glibc's retries take.
            if let Err(e) = h.reserve(len.saturating_add(2)) {
                return Some(Err(e));
            }
            if text.is_empty() {
                return None;
            }
            h.make(|room| take(text, room))
        })
    };
    match got {
        Ok(p) => p,
        Err(e) => {
            errno::set_errno(e);
            core::ptr::null()
        }
    }
}

impl<T> Held<T> {
    /// Grow the block to at least `need` bytes, in [`BUFLEN`] steps, with
    /// `errno` set to `ERANGE` if it grew -- what glibc's retry loop leaves.
    fn reserve(&mut self, need: usize) -> Result<(), i32> {
        if !self.buf.is_null() && self.cap >= need {
            return Ok(());
        }
        let steps = need.div_ceil(BUFLEN).max(1);
        let cap = steps.checked_mul(BUFLEN).ok_or(errno::ENOMEM)?;
        let first = self.buf.is_null();
        // SAFETY: `buf` is NULL or this storage's block; kept on failure.
        let p = unsafe { crate::malloc::realloc(self.buf, cap) };
        if p.is_null() {
            return Err(errno::ENOMEM);
        }
        self.buf = p;
        self.cap = cap;
        if !first || cap > BUFLEN {
            errno::set_errno(errno::ERANGE);
        }
        Ok(())
    }

    /// The entry `make` fills into this storage's block, growing it a step
    /// at a time while `make` answers `ERANGE`: a pointer to it, or `None`
    /// when `make` finds no entry.
    fn make(
        &mut self,
        make: impl Fn(&mut Room) -> Option<Result<T, i32>>,
    ) -> Option<Result<*const T, i32>> {
        loop {
            // SAFETY: `buf` is this storage's block of `cap` bytes.
            let mut room = unsafe { Room::new(self.buf, self.cap) };
            match make(&mut room)? {
                Ok(entry) => {
                    self.entry = entry;
                    return Some(Ok(&raw const self.entry));
                }
                Err(errno::ERANGE) => {
                    let Some(more) = self.cap.checked_add(BUFLEN) else {
                        return Some(Err(errno::ENOMEM));
                    };
                    if let Err(e) = self.reserve(more) {
                        return Some(Err(e));
                    }
                }
                Err(e) => return Some(Err(e)),
            }
        }
    }
}

/// glibc's `__nss_valid_field`: a field a database line can hold -- NULL,
/// or without `:` and newline, which would end it early.
pub(crate) fn valid_field(s: *const u8) -> bool {
    // SAFETY: NULL or the caller's C string.
    s.is_null()
        || !unsafe { c_bytes(s) }
            .iter()
            .any(|&b| b == b':' || b == b'\n')
}

/// glibc's `__nss_valid_list_field`: a NULL list, or members none of which
/// holds `:`, newline or the `,` that separates them.
///
/// # Safety
///
/// `list` is NULL or a NULL-terminated array of C strings.
pub(crate) unsafe fn valid_list_field(list: *const *const u8) -> bool {
    if list.is_null() {
        return true;
    }
    let mut at = list;
    loop {
        // SAFETY: the array is NULL-terminated, and `at` has not passed it.
        let member = unsafe { at.read() };
        if member.is_null() {
            return true;
        }
        // SAFETY: a member: the caller's C string.
        if unsafe { c_bytes(member) }
            .iter()
            .any(|&b| matches!(b, b':' | b'\n' | b','))
        {
            return false;
        }
        // SAFETY: not past the terminator, which is still ahead.
        at = unsafe { at.add(1) };
    }
}

/// Writes one entry's fields to a stream, under its lock, remembering
/// whether any write failed: the `putXXent` functions' `fprintf`s.
pub(crate) struct EntryWriter {
    stream: *mut u8,
    failed: bool,
}

impl EntryWriter {
    /// Hold `stream` for the whole entry, so that another thread's writes
    /// cannot land inside it.
    pub(crate) fn new(stream: *mut u8) -> Self {
        crate::stdio::flockfile(stream.cast());
        Self {
            stream,
            failed: false,
        }
    }

    /// Write `bytes`.
    pub(crate) fn bytes(&mut self, bytes: &[u8]) {
        if bytes.is_empty() || self.failed {
            return;
        }
        // SAFETY: `bytes` is readable; the stream is the caller's, held.
        let n =
            unsafe { crate::stdio::fwrite_unlocked(bytes.as_ptr(), 1, bytes.len(), self.stream) };
        if n != bytes.len() {
            self.failed = true;
        }
    }

    /// Write C string `s`, nothing for NULL: glibc's `_S(x)`.
    pub(crate) fn c_str_or_empty(&mut self, s: *const u8) {
        if !s.is_null() {
            // SAFETY: the caller's C string.
            self.bytes(unsafe { c_bytes(s) });
        }
    }

    /// Write C string `s` with each `:` and newline in it as a space, and
    /// nothing for NULL: glibc's `__nss_rewrite_field`, which `putpwent`
    /// applies to the GECOS field rather than refuse it.
    pub(crate) fn rewritten(&mut self, s: *const u8) {
        if s.is_null() {
            return;
        }
        // SAFETY: the caller's C string.
        let text = unsafe { c_bytes(s) };
        for (i, part) in text.split(|&b| b == b':' || b == b'\n').enumerate() {
            if i > 0 {
                self.bytes(b" ");
            }
            self.bytes(part);
        }
    }

    /// Write `v` in decimal, as `%lu` or `%ld` does.
    pub(crate) fn int(&mut self, v: i128) {
        let mut digits = [0u8; 40];
        let mut at = digits.len();
        let mut m = v.unsigned_abs();
        loop {
            at = at.saturating_sub(1);
            if let Some(slot) = digits.get_mut(at) {
                // `m % 10` is a digit.
                #[allow(clippy::cast_possible_truncation)]
                let d = (m % 10) as u8;
                *slot = b'0' + d;
            }
            m /= 10;
            if m == 0 {
                break;
            }
        }
        if v < 0 {
            self.bytes(b"-");
        }
        self.bytes(digits.get(at..).unwrap_or(&[]));
    }

    /// Release the stream, and say whether everything was written: 0, or -1.
    pub(crate) fn finish(self) -> i32 {
        crate::stdio::funlockfile(self.stream.cast());
        if self.failed { -1 } else { 0 }
    }
}

// ---------------------------------------------------------------------------
// Host tests: a database's text, per thread
// ---------------------------------------------------------------------------

#[cfg(test)]
use crate::perprocess::process_global;

/// What a database reads as on the host.
#[cfg(test)]
#[derive(Clone, Copy)]
enum TestDb {
    Missing,
    Text(&'static [u8]),
    /// A file that exists and cannot be read, with this `errno`.
    Error(i32),
}

#[cfg(test)]
process_global! {
    /// What each database reads as on this host thread.
    fn test_dbs() -> [TestDb; Which::COUNT] = [TestDb::Missing; Which::COUNT];
}

#[cfg(test)]
fn set_test_db(which: Which, db: TestDb) {
    // SAFETY: this thread's slots, not borrowed elsewhere.
    if let Some(slot) = unsafe { &mut *test_dbs() }.get_mut(which as usize) {
        *slot = db;
    }
}

/// Make database `which` read as `text` on this host test thread (`None`:
/// as a missing file).
#[cfg(test)]
pub(crate) fn set_test_text(which: Which, text: Option<&'static [u8]>) {
    set_test_db(which, text.map_or(TestDb::Missing, TestDb::Text));
}

/// Make database `which` a file that cannot be read, with `errno`, on this
/// host test thread.
#[cfg(test)]
pub(crate) fn set_test_error(which: Which, errno: i32) {
    set_test_db(which, TestDb::Error(errno));
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::vec::Vec;

    fn all(text: &[u8]) -> Vec<&[u8]> {
        lines(text, 0).map(|(l, _)| l).collect()
    }

    #[test]
    fn lines_skip_blanks_comments_and_leading_space() {
        let text = b"root:x:0\n\n   \n# a comment\n  \t# indented comment\n  bin:x:1\nlast";
        assert_eq!(all(text), [&b"root:x:0"[..], b"bin:x:1", b"last"]);
    }

    #[test]
    fn lines_resume_where_they_stopped() {
        let text = b"a\nb\nc\n";
        let mut it = lines(text, 0);
        let (first, next) = it.next().unwrap();
        assert_eq!(first, b"a");
        assert_eq!(
            lines(text, next).map(|(l, _)| l).collect::<Vec<_>>(),
            [&b"b"[..], b"c"]
        );
        assert_eq!(lines(text, text.len()).count(), 0);
    }

    #[test]
    fn strtou32_is_debians_glibcs() {
        assert_eq!(strtou32(b"42:"), Number::Fits(42, 2));
        assert_eq!(
            strtou32(b"  7x"),
            Number::Fits(7, 3),
            "leading space is skipped"
        );
        assert_eq!(strtou32(b"+5"), Number::Fits(5, 2));
        assert_eq!(strtou32(b"05"), Number::Fits(5, 2), "base 10, not octal");
        assert_eq!(strtou32(b"-0"), Number::Fits(0, 2));
        assert_eq!(strtou32(b"4294967295"), Number::Fits(u32::MAX, 10));
        // Past 32 bits: the line is no entry, where upstream would clamp.
        assert_eq!(strtou32(b"4294967296"), Number::PastU32);
        assert_eq!(strtou32(b"-1"), Number::PastU32, "negated as unsigned");
        assert_eq!(
            strtou32(b"-4294967295"),
            Number::PastU32,
            "strtoull makes it 18446744069414584321, not 1"
        );
        assert_eq!(strtou32(b"18446744073709551615"), Number::PastU32);
        assert_eq!(
            strtou32(b"-99999999999999999999999"),
            Number::PastU32,
            "an overflow is ULLONG_MAX whatever the sign -- not negated to 1"
        );
        assert_eq!(strtou32(b""), Number::Absent);
        assert_eq!(strtou32(b":"), Number::Absent);
        assert_eq!(strtou32(b" -"), Number::Absent);
        assert_eq!(
            strtou32(b"0x10"),
            Number::Fits(0, 1),
            "the 0, then x ends it"
        );
    }

    /// A number past 32 bits makes the line no entry, in every kind of number
    /// field (glibc 2.39 as Debian and Ubuntu build it, asked: the oracle
    /// files carry such lines, and these are its answers).
    #[test]
    fn a_number_past_32_bits_makes_no_entry() {
        let mut f = Fields::new(b"4294967296:1");
        assert_eq!(f.int(), None);
        let mut f = Fields::new(b"-1:1");
        assert_eq!(f.int(), None);
        let mut f = Fields::new(b"4294967296:1");
        assert_eq!(f.int_or_empty(), Err(()));
        let mut f = Fields::new(b"-2");
        assert_eq!(f.last_int_or_empty(), Err(()));
        let mut f = Fields::new(b":1");
        assert_eq!(f.int_or_empty(), Ok(None), "an empty field is still empty");
        let mut f = Fields::new(b"4294967295");
        assert_eq!(f.last_int_or_empty(), Ok(Some(u32::MAX)));
    }

    #[test]
    fn fields_follow_the_macros() {
        let mut f = Fields::new(b"name:pw:12:34:rest:of:it");
        assert_eq!(f.string(), b"name");
        assert_eq!(f.string(), b"pw");
        assert_eq!(f.int(), Some(12));
        assert_eq!(f.int(), Some(34));
        assert_eq!(f.rest(), b"rest:of:it");
        assert!(f.at_end());

        // A missing field is empty, and so is every one after it.
        let mut f = Fields::new(b"name");
        assert_eq!(
            (f.string(), f.string(), f.string()),
            (&b"name"[..], &b""[..], &b""[..])
        );

        // A number followed by anything but ':' or the end is no entry.
        assert_eq!(Fields::new(b"12a:").int(), None);
        assert_eq!(Fields::new(b":").int(), None, "an empty number is no entry");
        assert_eq!(Fields::new(b"12").int(), Some(12), "the end may follow it");
    }

    #[test]
    fn maybe_null_fields() {
        let mut f = Fields::new(b"::7:");
        assert_eq!(f.int_or_empty(), Ok(None));
        assert_eq!(f.int_or_empty(), Ok(None));
        assert_eq!(f.int_or_empty(), Ok(Some(7)));
        assert_eq!(f.int_or_empty(), Err(()), "the line ended before the field");
        assert_eq!(Fields::new(b"5x").int_or_empty(), Err(()));
        assert_eq!(Fields::new(b"9").last_int_or_empty(), Ok(Some(9)));
        assert_eq!(
            Fields::new(b"9:").last_int_or_empty(),
            Err(()),
            "must end the line"
        );
        assert_eq!(Fields::new(b"x").last_int_or_empty(), Err(()));
    }

    #[test]
    fn room_copies_strings_and_aligns_pointers() {
        let mut buf = [0xAAu8; 64];
        // SAFETY: `buf` outlives `room`.
        let mut room = unsafe { Room::new(buf.as_mut_ptr().wrapping_add(1), 63) };
        let s = room.string(b"abc").unwrap();
        // SAFETY: `s` points at the 4 bytes just written.
        assert_eq!(unsafe { core::slice::from_raw_parts(s, 4) }, b"abc\0");
        let p = room.pointers(2).unwrap();
        assert_eq!(
            p as usize % align_of::<*const u8>(),
            0,
            "aligned by address"
        );
        // SAFETY: two pointers were zeroed there.
        assert!(unsafe { (*p).is_null() && (*p.add(1)).is_null() });
        assert_eq!(room.pointers(8), Err(errno::ERANGE));
        assert_eq!(room.string(&[b'x'; 64]), Err(errno::ERANGE));
    }

    #[test]
    fn a_test_database_reads_as_its_text_and_none_as_missing() {
        set_test_text(Which::Group, Some(b"g:x:5:"));
        let Ok(Db::Text(t)) = read(Which::Group) else {
            panic!("expected text");
        };
        assert_eq!(t.bytes(), b"g:x:5:");
        set_test_text(Which::Group, None);
        assert!(matches!(read(Which::Group), Ok(Db::Missing)));
        assert!(matches!(read(Which::Shadow), Ok(Db::Missing)));
    }

    #[test]
    fn a_test_error_reads_as_that_error() {
        set_test_error(Which::Shadow, errno::EACCES);
        assert!(matches!(read(Which::Shadow), Err(e) if e == errno::EACCES));
        set_test_text(Which::Shadow, None);
    }

    #[test]
    fn a_host_file_that_cannot_be_opened_is_not_missing() {
        // The host has no filesystem behind `open`: whatever it answers, it
        // is not ENOENT, so it is an error rather than a missing database.
        match read_file(Which::Passwd.path()) {
            Err(e) => assert_ne!(e, errno::ENOENT),
            Ok(Db::Missing) => {}
            Ok(Db::Text(_)) => panic!("the host has no files to read"),
        }
    }
}
