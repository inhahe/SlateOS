// Offsets here are within one database line or one caller buffer, which
// bounds every sum made of them; the few that could pass the bound are
// checked.  Indices are into sixteen-byte addresses, their four- and
// twelve-byte parts, and `host.conf` words already measured.  Clippy cannot
// see the bounds.
#![allow(clippy::arithmetic_side_effects, clippy::indexing_slicing)]
//! The hosts database: `gethostbyname` and its kin, as glibc 2.40 answers
//! them with `/etc/nsswitch.conf` saying `hosts: files dns`.
//!
//! **Where answers come from**, in order -- the order every Linux
//! distribution configures:
//!
//! 1. **Numbers are not looked up.**  A name that is an address
//!    (`192.168.0.1`, or `::1` for `AF_INET6`) is answered as itself,
//!    glibc's `__nss_hostname_digits_dots`.
//! 2. **`/etc/hosts`**, read as glibc's `nss_files` reads it
//!    (`files-hosts.c`): one address and its names per line, matched
//!    ignoring case, the first line winning -- or, with `multi on` in
//!    `/etc/host.conf`, every line for the name merged into one answer.
//! 3. **The kernel's resolver** (`SYS_DNS_RESOLVE`), which is this system's
//!    DNS: it holds the cache, the kernel's own hosts table, the servers
//!    DHCP gave, and each container's names.  It is to this library what
//!    `systemd-resolved` is to glibc on a desktop distribution.  It answers
//!    one IPv4 address and no canonical name, so an IPv6 (`AAAA`) question
//!    is answered "no address of that kind" once the name is known to
//!    exist, and the canonical name is the name asked.
//!
//! **`/etc/host.conf`** is read once per process, as glibc reads it
//! (`resolv/res_hconf.c`): `multi`, `reorder` (put an address on a local
//! subnet first) and `trim` (cut these domains off the names
//! `gethostbyaddr` answers with), and the `RESOLV_*` environment variables
//! that override them.  A line it cannot use is reported on standard error,
//! as glibc reports it.
//!
//! **Answers** follow glibc's `getXXbyYY_r` exactly: 0 whether found or
//! not, with `*result` saying which and `*h_errnop` why not; `ERANGE` for a
//! buffer too small; `EAGAIN` for "try again".  The non-reentrant forms
//! answer in a block of the calling thread's, grown as needed
//! ([`crate::netdb`]).

use crate::errno;
use crate::netdb::{self, Line};
use crate::nss_files::{self, Room, Which};
use crate::socket::{
    AF_INET, AF_INET6, AF_UNSPEC, HOST_NOT_FOUND, Hostent, NO_DATA, NO_RECOVERY, TRY_AGAIN,
};

/// `NETDB_INTERNAL`: "see `errno`".
pub const NETDB_INTERNAL: i32 = -1;
/// `NETDB_SUCCESS`.
pub const NETDB_SUCCESS: i32 = 0;

/// `AI_V4MAPPED`, as `files-hosts.c` takes it.
const AI_V4MAPPED: i32 = 0x0008;

const INADDRSZ: usize = 4;
const IN6ADDRSZ: usize = 16;

/// An NSS module's answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Status {
    TryAgain,
    Unavail,
    NotFound,
    Success,
}

/// The two octets that open an IPv4-mapped IPv6 address, after ten zeros.
fn is_v4mapped(a: &[u8; 16]) -> bool {
    a[..10] == [0; 10] && a[10..12] == [0xff, 0xff]
}

fn is_loopback6(a: &[u8; 16]) -> bool {
    a[..15] == [0; 15] && a[15] == 1
}

/// The IPv4-mapped form of `v4`.
pub(crate) fn map_v4(v4: [u8; 4]) -> [u8; 16] {
    let mut a = [0u8; 16];
    a[10] = 0xff;
    a[11] = 0xff;
    a[12..].copy_from_slice(&v4);
    a
}

// ---------------------------------------------------------------------------
// /etc/host.conf
// ---------------------------------------------------------------------------

/// glibc's `TRIMDOMAINS_MAX`.
const TRIMDOMAINS_MAX: usize = 4;
/// Room for one trim domain: longer than any DNS name.
const TRIM_LEN: usize = 256;

/// What `/etc/host.conf` and the environment said: glibc's `_res_hconf`.
pub(crate) struct HostConf {
    pub(crate) multi: bool,
    pub(crate) reorder: bool,
    ntrim: usize,
    trim: [[u8; TRIM_LEN]; TRIMDOMAINS_MAX],
    trim_len: [usize; TRIMDOMAINS_MAX],
}

impl HostConf {
    const EMPTY: Self = Self {
        multi: false,
        reorder: false,
        ntrim: 0,
        trim: [[0; TRIM_LEN]; TRIMDOMAINS_MAX],
        trim_len: [0; TRIMDOMAINS_MAX],
    };

    fn trims(&self) -> impl Iterator<Item = &[u8]> {
        self.trim
            .iter()
            .zip(self.trim_len.iter())
            .take(self.ntrim)
            .map(|(t, &n)| t.get(..n).unwrap_or(&[]))
    }
}

/// The configuration, read on first use.
struct HostConfCell {
    /// 0 unread, 1 being read, 2 read.
    state: core::sync::atomic::AtomicU8,
    conf: core::cell::UnsafeCell<HostConf>,
}

impl HostConfCell {
    const fn new() -> Self {
        Self {
            state: core::sync::atomic::AtomicU8::new(0),
            conf: core::cell::UnsafeCell::new(HostConf::EMPTY),
        }
    }
}

// SAFETY: `conf` is written only by the one thread that wins `state` from 0
// to 1, and read only after `state` is 2 (release/acquire).
unsafe impl Sync for HostConfCell {}

crate::perprocess::process_global! {
    /// The process's `host.conf` (the thread's, on the host).
    fn hconf_cell() -> HostConfCell = HostConfCell::new();
}

/// The process's `host.conf` settings: glibc's `_res_hconf_init`, once.
pub(crate) fn host_conf() -> &'static HostConf {
    use core::sync::atomic::Ordering;
    // SAFETY: the process's cell, which lives forever.
    let cell = unsafe { &*hconf_cell() };
    if cell.state.load(Ordering::Acquire) != 2 {
        if cell
            .state
            .compare_exchange(0, 1, Ordering::Acquire, Ordering::Acquire)
            .is_ok()
        {
            let mut c = HostConf::EMPTY;
            load_host_conf(&mut c);
            // SAFETY: this thread won the right to write, and nobody reads
            // until `state` is 2.
            unsafe { *cell.conf.get() = c };
            cell.state.store(2, Ordering::Release);
        } else {
            while cell.state.load(Ordering::Acquire) != 2 {
                crate::pthread::sched_yield();
            }
        }
    }
    // SAFETY: `state` is 2: written once, never again.
    unsafe { &*cell.conf.get() }
}

/// Forget the configuration, so the next lookup reads it again.  Host
/// tests only: a process reads it once.
#[cfg(test)]
pub(crate) fn reset_host_conf() {
    // SAFETY: the thread's own cell on the host.
    unsafe {
        (*hconf_cell())
            .state
            .store(0, core::sync::atomic::Ordering::Release)
    };
}

/// Report a `host.conf` problem on standard error, as glibc's
/// `__fxprintf (NULL, ...)` does.
fn complain(parts: &[&[u8]]) {
    let mut msg = [0u8; 512];
    let mut n = 0usize;
    for p in parts {
        for &b in *p {
            if let Some(slot) = msg.get_mut(n) {
                *slot = b;
                n += 1;
            }
        }
    }
    #[cfg(not(test))]
    {
        // A diagnostic that cannot be written has nowhere else to go.
        let _ = crate::file::write(2, msg.as_ptr(), n);
    }
    #[cfg(test)]
    {
        // SAFETY: this thread's record of what would have been written.
        unsafe { (*complaints()).extend_from_slice(msg.get(..n).unwrap_or(&[])) };
    }
}

#[cfg(test)]
crate::perprocess::process_global! {
    /// What `complain` would have written to standard error, on the host.
    fn complaints() -> std::vec::Vec<u8> = std::vec::Vec::new();
}

/// A decimal line number, for [`complain`].
fn decimal(mut n: usize, buf: &mut [u8; 20]) -> &[u8] {
    let mut i = buf.len();
    loop {
        i -= 1;
        buf[i] = b'0' + (n % 10) as u8;
        n /= 10;
        if n == 0 {
            break;
        }
    }
    &buf[i..]
}

/// `skip_ws`.
fn skip_ws(s: &[u8]) -> &[u8] {
    let n = s.iter().take_while(|&&b| nss_files::is_space(b)).count();
    s.get(n..).unwrap_or(&[])
}

/// `skip_string`: to white space, `#`, `,` or the end.
fn skip_string(s: &[u8]) -> usize {
    s.iter()
        .position(|&b| nss_files::is_space(b) || b == b'#' || b == b',')
        .unwrap_or(s.len())
}

/// `arg_bool`: `on` or `off` as a prefix, case ignored.  The rest of the
/// line, or `None` (reported) for anything else.
fn arg_bool<'a>(fname: &[u8], line: usize, args: &'a [u8], flag: &mut bool) -> Option<&'a [u8]> {
    if args.len() >= 2 && args[..2].eq_ignore_ascii_case(b"on") {
        *flag = true;
        Some(&args[2..])
    } else if args.len() >= 3 && args[..3].eq_ignore_ascii_case(b"off") {
        *flag = false;
        Some(&args[3..])
    } else {
        let mut num = [0u8; 20];
        complain(&[
            fname,
            b": line ",
            decimal(line, &mut num),
            b": expected `on' or `off', found `",
            args,
            b"'\n",
        ]);
        None
    }
}

/// `arg_trimdomain_list`.
fn arg_trim<'a>(
    c: &mut HostConf,
    fname: &[u8],
    line: usize,
    mut args: &'a [u8],
) -> Option<&'a [u8]> {
    let mut num = [0u8; 20];
    loop {
        let len = skip_string(args);
        if c.ntrim >= TRIMDOMAINS_MAX {
            complain(&[
                fname,
                b": line ",
                decimal(line, &mut num),
                b": cannot specify more than 4 trim domains",
            ]);
            return None;
        }
        let d = &args[..len];
        let keep = d.len().min(TRIM_LEN);
        c.trim[c.ntrim][..keep].copy_from_slice(&d[..keep]);
        c.trim_len[c.ntrim] = keep;
        c.ntrim += 1;
        args = skip_ws(&args[len..]);
        if matches!(args.first(), Some(b',' | b';' | b':')) {
            args = skip_ws(&args[1..]);
            if args.is_empty() || args.first() == Some(&b'#') {
                complain(&[
                    fname,
                    b": line ",
                    decimal(line, &mut num),
                    b": list delimiter not followed by domain",
                ]);
                return None;
            }
        }
        if args.is_empty() || args.first() == Some(&b'#') {
            return Some(args);
        }
    }
}

/// `parse_line`: one `host.conf` line.
fn parse_hconf_line(c: &mut HostConf, fname: &[u8], line: usize, s: &[u8]) {
    let s = skip_ws(s);
    if s.is_empty() || s.first() == Some(&b'#') {
        return;
    }
    let len = skip_string(s);
    let cmd = &s[..len];
    let known = [b"order".as_slice(), b"trim", b"multi", b"reorder"]
        .into_iter()
        .position(|n| n.eq_ignore_ascii_case(cmd));
    let Some(which) = known else {
        let mut num = [0u8; 20];
        complain(&[
            fname,
            b": line ",
            decimal(line, &mut num),
            b": bad command `",
            s,
            b"'\n",
        ]);
        return;
    };
    let args = skip_ws(&s[len..]);
    let rest = match which {
        // `order`: read by old libraries, ignored by this one and glibc.
        0 => return,
        1 => arg_trim(c, fname, line, args),
        2 => arg_bool(fname, line, args, &mut c.multi),
        _ => arg_bool(fname, line, args, &mut c.reorder),
    };
    let Some(rest) = rest else {
        return;
    };
    // The rest of the line must be white space or a comment.
    let rest = skip_ws(rest);
    if !rest.is_empty() && rest.first() != Some(&b'#') {
        let mut num = [0u8; 20];
        complain(&[
            fname,
            b": line ",
            decimal(line, &mut num),
            b": ignoring trailing garbage `",
            rest,
            b"'\n",
        ]);
    }
}

/// `do_init`: the file named by `RESOLV_HOST_CONF` or `/etc/host.conf`,
/// then the `RESOLV_MULTI`, `RESOLV_REORDER`, `RESOLV_ADD_TRIM_DOMAINS` and
/// `RESOLV_OVERRIDE_TRIM_DOMAINS` variables.
fn load_host_conf(c: &mut HostConf) {
    let env = |name: &[u8]| -> Option<&'static [u8]> {
        // SAFETY: `name` is NUL-terminated; the value lives as long as the
        // environment.
        let v = unsafe { crate::environ::lookup(name.as_ptr()) };
        // SAFETY: a non-null value is a NUL-terminated string.
        (!v.is_null()).then(|| unsafe { nss_files::c_bytes(v) })
    };
    let fname: &[u8] = env(b"RESOLV_HOST_CONF\0").unwrap_or(b"/etc/host.conf");
    if let Some(text) = read_host_conf(fname) {
        // `fgets` into 256 bytes: a longer line is read, and numbered, as
        // several.
        let mut line_num = 0usize;
        let mut at = 0usize;
        while at < text.bytes().len() {
            let rest = &text.bytes()[at..];
            let upto = rest
                .iter()
                .position(|&b| b == b'\n')
                .map_or(rest.len(), |i| i + 1)
                .min(255);
            let chunk = &rest[..upto];
            at += upto;
            line_num += 1;
            // `*strchrnul (buf, '\n') = '\0'`, and the parser stops at a NUL.
            let end = chunk
                .iter()
                .position(|&b| b == b'\n' || b == 0)
                .unwrap_or(chunk.len());
            parse_hconf_line(c, fname, line_num, &chunk[..end]);
        }
    }
    if let Some(v) = env(b"RESOLV_MULTI\0") {
        let _ = arg_bool(b"RESOLV_MULTI", 1, v, &mut c.multi);
    }
    if let Some(v) = env(b"RESOLV_REORDER\0") {
        let _ = arg_bool(b"RESOLV_REORDER", 1, v, &mut c.reorder);
    }
    if let Some(v) = env(b"RESOLV_ADD_TRIM_DOMAINS\0") {
        let _ = arg_trim(c, b"RESOLV_ADD_TRIM_DOMAINS", 1, v);
    }
    if let Some(v) = env(b"RESOLV_OVERRIDE_TRIM_DOMAINS\0") {
        c.ntrim = 0;
        let _ = arg_trim(c, b"RESOLV_OVERRIDE_TRIM_DOMAINS", 1, v);
    }
}

/// `host.conf`'s text, or `None` when it cannot be opened (as glibc,
/// silently).
fn read_host_conf(fname: &[u8]) -> Option<nss_files::Text> {
    #[cfg(test)]
    {
        let _ = fname;
        match nss_files::read(Which::HostConf) {
            Ok(nss_files::Db::Text(t)) => Some(t),
            _ => None,
        }
    }
    #[cfg(not(test))]
    {
        let read = if fname == b"/etc/host.conf" {
            nss_files::read(Which::HostConf)
        } else {
            let mut path = [0u8; 4096];
            let p = path.get_mut(..fname.len())?;
            p.copy_from_slice(fname);
            nss_files::read_path(path.get(..=fname.len())?)
        };
        match read {
            Ok(nss_files::Db::Text(t)) => Some(t),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// /etc/hosts
// ---------------------------------------------------------------------------

/// One `/etc/hosts` line, as `files-hosts.c` reads it for a family.
pub(crate) struct HostLine<'a> {
    /// The family the address was read as: the one asked for, or, for
    /// `AF_UNSPEC`, the address's own.
    pub(crate) family: i32,
    /// The address: 4 bytes for `AF_INET`, 16 for `AF_INET6`.
    pub(crate) addr: [u8; 16],
    pub(crate) name: &'a [u8],
    aliases: Line<'a>,
}

impl<'a> HostLine<'a> {
    pub(crate) fn aliases(&self) -> impl Iterator<Item = &'a [u8]> + Clone + use<'a> {
        Line::from_rest(self.aliases.rest()).words()
    }

    fn len(&self) -> usize {
        if self.family == AF_INET {
            INADDRSZ
        } else {
            IN6ADDRSZ
        }
    }
}

/// `files-hosts.c`'s `LINE_PARSER`: the address read for `af`, or the line
/// skipped:
///
/// - `af` `AF_UNSPEC`: IPv4 if it reads as IPv4, else IPv6;
/// - `af` `AF_INET6` with `AI_V4MAPPED` in `flags`: an IPv4 address mapped;
/// - `af` `AF_INET`: an IPv6 address only if it is IPv4-mapped (its IPv4
///   part) or `::1` (as 127.0.0.1).
pub(crate) fn parse_host(line: &[u8], af: i32, flags: i32) -> Option<HostLine<'_>> {
    let mut l = Line::new(line);
    let text = l.string();
    let mut addr = [0u8; 16];
    let first = if af == AF_UNSPEC { AF_INET } else { af };
    let family = match (first, pton(first, text)) {
        (_, Some(a)) => {
            addr = a;
            first
        }
        (AF_INET6, None) if flags & AI_V4MAPPED != 0 => {
            let v4 = crate::inet::pton4(text)?;
            addr = map_v4(v4);
            AF_INET6
        }
        (AF_INET, None) if af == AF_INET => {
            let v6 = crate::inet::pton6(text)?;
            if is_v4mapped(&v6) {
                addr[..4].copy_from_slice(&v6[12..]);
            } else if is_loopback6(&v6) {
                addr[..4].copy_from_slice(&[127, 0, 0, 1]);
            } else {
                return None;
            }
            AF_INET
        }
        (AF_INET, None) => {
            // `af` was `AF_UNSPEC`.
            addr = crate::inet::pton6(text)?;
            AF_INET6
        }
        _ => return None,
    };
    let name = l.string();
    Some(HostLine {
        family,
        addr,
        name,
        aliases: l,
    })
}

/// `inet_pton` for `af` into 16 bytes.
fn pton(af: i32, text: &[u8]) -> Option<[u8; 16]> {
    let mut a = [0u8; 16];
    match af {
        AF_INET => a[..4].copy_from_slice(&crate::inet::pton4(text)?),
        AF_INET6 => a = crate::inet::pton6(text)?,
        _ => return None,
    }
    Some(a)
}

/// `LOOKUP_NAME_CASE`: the name, or one of the aliases, ignoring case.
fn line_names(line: &HostLine<'_>, name: &[u8]) -> bool {
    line.name.eq_ignore_ascii_case(name) || line.aliases().any(|a| a.eq_ignore_ascii_case(name))
}

/// The lines of `text` that name `name`, read for `af`: the first only,
/// unless `multi`.
fn named<'t>(
    text: &'t [u8],
    name: &'t [u8],
    af: i32,
    flags: i32,
    multi: bool,
) -> impl Iterator<Item = HostLine<'t>> + Clone {
    nss_files::lines(text, 0)
        .filter_map(move |(l, _)| parse_host(l, af, flags))
        .filter(move |h| line_names(h, name))
        .take(if multi { usize::MAX } else { 1 })
}

// ---------------------------------------------------------------------------
// Building a hostent in the caller's buffer
// ---------------------------------------------------------------------------

/// Copy an answer into `room`: the alias and address arrays (pointer
/// aligned), the addresses (`len` bytes each), then the strings.
fn fill_hostent<'a>(
    room: &mut Room,
    name: &[u8],
    family: i32,
    len: usize,
    addrs: impl Iterator<Item = [u8; 16]> + Clone,
    aliases: impl Iterator<Item = &'a [u8]> + Clone,
) -> Result<Hostent, i32> {
    let naliases = aliases.clone().count();
    let naddrs = addrs.clone().count();
    let alias_list = room.pointers(naliases + 1)?;
    let addr_list = room.pointers(naddrs + 1)?;
    for (i, a) in addrs.enumerate() {
        let p = room.bytes(a.get(..len).unwrap_or(&[]))?;
        // SAFETY: `addr_list` holds `naddrs + 1` pointers and `i < naddrs`.
        unsafe { addr_list.add(i).write(p.cast_const()) };
    }
    for (i, a) in aliases.enumerate() {
        let p = room.string(a)?;
        // SAFETY: as above, for the aliases.
        unsafe { alias_list.add(i).write(p.cast_const()) };
    }
    Ok(Hostent {
        h_name: room.string(name)?,
        h_aliases: alias_list.cast_const(),
        h_addrtype: family,
        h_length: len as i32,
        h_addr_list: addr_list.cast_const(),
    })
}

/// One module's answer: its status, and the entry when it found one.
struct Found {
    status: Status,
    entry: Option<Hostent>,
}

impl Found {
    fn miss(status: Status) -> Self {
        Self {
            status,
            entry: None,
        }
    }
}

/// A module's report through the caller's `h_errno` and `errno`.
struct Report<'r> {
    herr: &'r mut i32,
}

impl Report<'_> {
    fn set(&mut self, h: i32) {
        *self.herr = h;
    }

    /// A buffer too small: glibc's `TRYAGAIN`, `NETDB_INTERNAL`, `ERANGE`.
    fn erange(&mut self) -> Found {
        *self.herr = NETDB_INTERNAL;
        errno::set_errno(errno::ERANGE);
        Found::miss(Status::TryAgain)
    }
}

// ---------------------------------------------------------------------------
// The files module
// ---------------------------------------------------------------------------

/// The hosts file's text, or the module's answer when there is none:
/// `UNAVAIL` with `errno` saying why (`ENOENT` for a missing file).
fn hosts_text<R>(f: impl FnOnce(&[u8]) -> R) -> Result<R, Status> {
    match nss_files::read(Which::Hosts) {
        Ok(nss_files::Db::Text(t)) => Ok(f(t.bytes())),
        Ok(nss_files::Db::Missing) => {
            errno::set_errno(errno::ENOENT);
            Err(Status::Unavail)
        }
        Err(e) => {
            errno::set_errno(e);
            Err(if e == errno::EAGAIN {
                Status::TryAgain
            } else {
                Status::Unavail
            })
        }
    }
}

/// The least buffer `files-XXX.c`'s `internal_getent` reads a line into:
/// its `struct parser_data` (an address and two pointers) and two bytes.
const FILES_MIN_BUFLEN: usize = 16 + 2 * size_of::<*const u8>() + 2;

/// `_nss_files_gethostbyname3_r`: the first line naming `name`, read for
/// `af` -- and, with `multi`, every later one merged in: their addresses
/// after its, their aliases after its, and their own names as aliases when
/// they differ from its.  Reading on to the end leaves `HOST_NOT_FOUND`
/// behind even when something was found, as glibc's `multi` does.
///
/// glibc asserts that `multi` is never asked for `AF_UNSPEC`; here only
/// lines of the first one's family are merged.
fn files_byname3(name: &[u8], af: i32, room: &mut Room, rep: &mut Report<'_>) -> Found {
    if room.capacity() < FILES_MIN_BUFLEN {
        return rep.erange();
    }
    let multi = host_conf().multi;
    let r = hosts_text(|text| {
        let Some(first) = named(text, name, af, 0, multi).next() else {
            rep.set(HOST_NOT_FOUND);
            return Found::miss(Status::NotFound);
        };
        let family = first.family;
        let all = named(text, name, af, 0, multi).filter(move |h| h.family == family);
        if multi {
            rep.set(HOST_NOT_FOUND);
        }
        let rest = all.clone().skip(1);
        let aliases = first.aliases().chain(rest.flat_map({
            let first_name = first.name;
            move |h| {
                let own = (h.name != first_name).then_some(h.name);
                h.aliases().chain(own)
            }
        }));
        let addrs = all.map(|h| h.addr);
        match fill_hostent(room, first.name, first.family, first.len(), addrs, aliases) {
            Ok(e) => Found {
                status: Status::Success,
                entry: Some(e),
            },
            Err(_) => rep.erange(),
        }
    });
    r.unwrap_or_else(Found::miss)
}

/// `_nss_files_gethostbyaddr_r`: the first line whose address, read for
/// `af` (IPv4 lines mapped when the address is 16 bytes), is `addr`.
fn files_byaddr(addr: &[u8], af: i32, room: &mut Room, rep: &mut Report<'_>) -> Found {
    if room.capacity() < FILES_MIN_BUFLEN {
        return rep.erange();
    }
    let flags = if addr.len() == IN6ADDRSZ {
        AI_V4MAPPED
    } else {
        0
    };
    let r = hosts_text(|text| {
        let hit = nss_files::lines(text, 0)
            .filter_map(|(l, _)| parse_host(l, af, flags))
            .find(|h| h.len() == addr.len() && h.addr.get(..addr.len()) == Some(addr));
        let Some(h) = hit else {
            rep.set(HOST_NOT_FOUND);
            return Found::miss(Status::NotFound);
        };
        match fill_hostent(
            room,
            h.name,
            h.family,
            h.len(),
            core::iter::once(h.addr),
            h.aliases(),
        ) {
            Ok(e) => Found {
                status: Status::Success,
                entry: Some(e),
            },
            Err(_) => rep.erange(),
        }
    });
    r.unwrap_or_else(Found::miss)
}

/// One address found for a name, as `getaddrinfo` collects them: glibc's
/// `gaih_addrtuple`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Tuple {
    pub(crate) family: i32,
    pub(crate) addr: [u8; 16],
    pub(crate) scopeid: u32,
}

/// `_nss_files_gethostbyname4_r`, for `getaddrinfo` with `AF_UNSPEC`:
/// each line naming `name` (the first only, unless `multi`), IPv4 and IPv6
/// alike, handed to `each`; `canon` gets the first one's name.
pub(crate) fn files_byname4(
    name: &[u8],
    herr: &mut i32,
    canon: &mut dyn FnMut(&[u8]),
    each: &mut dyn FnMut(Tuple),
) -> Status {
    let multi = host_conf().multi;
    match nss_files::read(Which::Hosts) {
        Ok(nss_files::Db::Text(t)) => {
            let mut any = false;
            for h in named(t.bytes(), name, AF_UNSPEC, 0, multi) {
                if !any {
                    canon(h.name);
                }
                any = true;
                each(Tuple {
                    family: h.family,
                    addr: h.addr,
                    scopeid: 0,
                });
            }
            if any {
                Status::Success
            } else {
                *herr = HOST_NOT_FOUND;
                Status::NotFound
            }
        }
        // `gethostbyname4_r` says "no data" for a file it cannot open.
        Ok(nss_files::Db::Missing) => {
            errno::set_errno(errno::ENOENT);
            *herr = NO_DATA;
            Status::Unavail
        }
        Err(e) => {
            errno::set_errno(e);
            if e == errno::EAGAIN {
                *herr = TRY_AGAIN;
                Status::TryAgain
            } else {
                *herr = NO_DATA;
                Status::Unavail
            }
        }
    }
}

/// `_nss_files_gethostbyname3_r` for `getaddrinfo`: the addresses of
/// `af` for `name`, handed to `each`, and the answer's name to `canon`.
pub(crate) fn files_byname3_tuples(
    name: &[u8],
    af: i32,
    herr: &mut i32,
    canon: &mut dyn FnMut(&[u8]),
    each: &mut dyn FnMut(Tuple),
) -> Status {
    let multi = host_conf().multi;
    let r = hosts_text(|text| {
        let mut any = false;
        for h in named(text, name, af, 0, multi) {
            if !any {
                canon(h.name);
            }
            any = true;
            each(Tuple {
                family: h.family,
                addr: h.addr,
                scopeid: 0,
            });
        }
        if any {
            Status::Success
        } else {
            *herr = HOST_NOT_FOUND;
            Status::NotFound
        }
    });
    r.unwrap_or_else(|s| s)
}

// ---------------------------------------------------------------------------
// The kernel's resolver
// ---------------------------------------------------------------------------
//
// The kernel module answers as glibc's DNS module (`resolv/nss_dns/
// dns-host.c`) answers, with the kernel's `errno` standing in for the one
// `res_search` leaves: a name that is not a host name is not asked about;
// "no such name" is `NOTFOUND` with `HOST_NOT_FOUND`; a network that cannot
// be reached is "try again".

/// A forward lookup's failure, as `gethostbyname3_context` reports one.
fn forward_failure(e: i32, herr: &mut i32) -> Status {
    match e {
        errno::ENOENT | errno::EINVAL => {
            *herr = HOST_NOT_FOUND;
            Status::NotFound
        }
        // `res_search`'s ESRCH: no server answered.
        errno::EAGAIN | errno::ENETUNREACH | errno::EHOSTUNREACH => {
            *herr = TRY_AGAIN;
            errno::set_errno(errno::EAGAIN);
            Status::TryAgain
        }
        errno::ECONNREFUSED | errno::ETIMEDOUT => {
            *herr = TRY_AGAIN;
            errno::set_errno(errno::EAGAIN);
            Status::Unavail
        }
        // Out of descriptors, or not allowed to ask (no network capability):
        // the error is `errno`'s.
        errno::EMFILE | errno::ENFILE | errno::EPERM | errno::EACCES => {
            *herr = NETDB_INTERNAL;
            errno::set_errno(e);
            Status::Unavail
        }
        _ => {
            *herr = NO_RECOVERY;
            Status::NotFound
        }
    }
}

/// A reverse lookup's failure, as `_nss_dns_gethostbyaddr2_r` reports
/// one: `UNAVAIL` only for a refused query.
fn reverse_failure(e: i32, herr: &mut i32) -> Status {
    match e {
        errno::ENOENT | errno::EINVAL => {
            *herr = HOST_NOT_FOUND;
            Status::NotFound
        }
        errno::ECONNREFUSED => {
            *herr = TRY_AGAIN;
            Status::Unavail
        }
        errno::EAGAIN | errno::ENETUNREACH | errno::EHOSTUNREACH | errno::ETIMEDOUT => {
            *herr = TRY_AGAIN;
            Status::NotFound
        }
        errno::EMFILE | errno::ENFILE | errno::EPERM | errno::EACCES => {
            *herr = NETDB_INTERNAL;
            errno::set_errno(e);
            Status::Unavail
        }
        _ => {
            *herr = NO_RECOVERY;
            Status::NotFound
        }
    }
}

/// `SYS_DNS_RESOLVE`: `name`'s IPv4 address, or the kernel's `errno`.
fn kernel_resolve(name: &[u8]) -> Result<[u8; 4], i32> {
    // The kernel reads no more than a DNS name's 253 bytes; a longer or an
    // empty one names no host.
    if name.is_empty() || name.len() > 253 {
        return Err(errno::ENOENT);
    }
    #[cfg(test)]
    {
        test_kernel_resolve(name)
    }
    #[cfg(not(test))]
    {
        let mut out = [0u8; 4];
        let r = crate::syscall::syscall3(
            crate::syscall::SYS_DNS_RESOLVE,
            name.as_ptr() as u64,
            name.len() as u64,
            out.as_mut_ptr() as u64,
        );
        if r < 0 {
            Err(r
                .checked_neg()
                .and_then(|e| i32::try_from(e).ok())
                .unwrap_or(errno::EIO))
        } else {
            Ok(out)
        }
    }
}

/// `SYS_DNS_REVERSE_RESOLVE`: the name the kernel's resolver gives `addr`
/// (network order) into `out`, its length, or the kernel's `errno`.
fn kernel_reverse(addr: [u8; 4], out: &mut [u8; 256]) -> Result<usize, i32> {
    #[cfg(test)]
    {
        test_kernel_reverse(addr, out)
    }
    #[cfg(not(test))]
    {
        let r = crate::syscall::syscall3(
            crate::syscall::SYS_DNS_REVERSE_RESOLVE,
            u64::from(u32::from_ne_bytes(addr)),
            out.as_mut_ptr() as u64,
            out.len() as u64,
        );
        if r < 0 {
            Err(r
                .checked_neg()
                .and_then(|e| i32::try_from(e).ok())
                .unwrap_or(errno::EIO))
        } else {
            // The kernel reports the length; trust no more than it could
            // have written.
            Ok(usize::try_from(r).unwrap_or(0).min(out.len()))
        }
    }
}

/// The forward lookup the three `byname` forms share: `name`'s IPv4
/// address, after glibc's `check_name`.
fn kernel_forward(name: &[u8], herr: &mut i32) -> Result<[u8; 4], Status> {
    if !crate::resolv::hnok(name) {
        *herr = HOST_NOT_FOUND;
        return Err(Status::NotFound);
    }
    kernel_resolve(name).map_err(|e| forward_failure(e, herr))
}

/// The kernel module's `gethostbyname3_r`.  An `AF_INET6` question has no
/// answer the kernel can give; asking for the IPv4 address says whether the
/// name exists ("no address of that kind") or not.
fn kernel_byname3(name: &[u8], af: i32, room: &mut Room, rep: &mut Report<'_>) -> Found {
    if af != AF_INET && af != AF_INET6 {
        rep.set(NO_DATA);
        errno::set_errno(errno::EAFNOSUPPORT);
        return Found::miss(Status::Unavail);
    }
    match kernel_forward(name, rep.herr) {
        Ok(v4) if af == AF_INET => {
            let mut a = [0u8; 16];
            a[..4].copy_from_slice(&v4);
            match fill_hostent(
                room,
                name,
                AF_INET,
                INADDRSZ,
                core::iter::once(a),
                core::iter::empty(),
            ) {
                Ok(e) => {
                    rep.set(NETDB_SUCCESS);
                    Found {
                        status: Status::Success,
                        entry: Some(e),
                    }
                }
                Err(_) => rep.erange(),
            }
        }
        Ok(_) => {
            rep.set(NO_DATA);
            Found::miss(Status::NotFound)
        }
        Err(s) => Found::miss(s),
    }
}

/// The kernel module's `gethostbyname4_r`: the IPv4 address.
pub(crate) fn kernel_byname4(
    name: &[u8],
    herr: &mut i32,
    canon: &mut dyn FnMut(&[u8]),
    each: &mut dyn FnMut(Tuple),
) -> Status {
    kernel_byname3_tuples(name, AF_INET, herr, canon, each)
}

/// The kernel module's `gethostbyname3_r` for `getaddrinfo`.
pub(crate) fn kernel_byname3_tuples(
    name: &[u8],
    af: i32,
    herr: &mut i32,
    canon: &mut dyn FnMut(&[u8]),
    each: &mut dyn FnMut(Tuple),
) -> Status {
    if af != AF_INET && af != AF_INET6 {
        *herr = NO_DATA;
        errno::set_errno(errno::EAFNOSUPPORT);
        return Status::Unavail;
    }
    match kernel_forward(name, herr) {
        Ok(v4) if af == AF_INET => {
            let mut a = [0u8; 16];
            a[..4].copy_from_slice(&v4);
            canon(name);
            each(Tuple {
                family: AF_INET,
                addr: a,
                scopeid: 0,
            });
            *herr = NETDB_SUCCESS;
            Status::Success
        }
        Ok(_) => {
            *herr = NO_DATA;
            Status::NotFound
        }
        Err(s) => s,
    }
}

/// The kernel module's `gethostbyaddr2_r`, as glibc's DNS module: an
/// IPv4-mapped or -compatible IPv6 address is asked as its IPv4 address and
/// answered as one; an address shorter than its family's is `EAFNOSUPPORT`;
/// the answer keeps the length it was asked with.  Another IPv6 address has
/// no reverse name the kernel can find.
fn kernel_byaddr(addr: &[u8], af: i32, room: &mut Room, rep: &mut Report<'_>) -> Found {
    let (mut af, mut a) = (af, addr);
    if af == AF_INET6 && a.len() == IN6ADDRSZ {
        let mapped = a[..12] == [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0xff, 0xff];
        let tunnelled = a[..12] == [0; 12] && a[12..] != [0, 0, 0, 1];
        if mapped || tunnelled {
            af = AF_INET;
            a = &a[12..];
        }
    }
    let size = match af {
        AF_INET => INADDRSZ,
        AF_INET6 => IN6ADDRSZ,
        _ => 0,
    };
    if size == 0 || size > a.len() {
        errno::set_errno(errno::EAFNOSUPPORT);
        rep.set(NETDB_INTERNAL);
        return Found::miss(Status::Unavail);
    }
    if af == AF_INET6 {
        rep.set(HOST_NOT_FOUND);
        return Found::miss(Status::NotFound);
    }
    let v4 = [a[0], a[1], a[2], a[3]];
    let mut out = [0u8; 256];
    match kernel_reverse(v4, &mut out) {
        Ok(n) => {
            let mut a16 = [0u8; 16];
            let keep = a.len().min(16);
            a16[..keep].copy_from_slice(&a[..keep]);
            let name = out.get(..n).unwrap_or(&[]);
            match fill_hostent(
                room,
                name,
                AF_INET,
                keep,
                core::iter::once(a16),
                core::iter::empty(),
            ) {
                Ok(e) => {
                    rep.set(NETDB_SUCCESS);
                    Found {
                        status: Status::Success,
                        entry: Some(e),
                    }
                }
                Err(_) => rep.erange(),
            }
        }
        Err(e) => Found::miss(reverse_failure(e, rep.herr)),
    }
}

#[cfg(test)]
crate::perprocess::process_global! {
    /// The host's stand-in for the kernel's resolver: names and addresses
    /// it knows, and the error it answers for any other.
    fn test_resolver() -> TestResolver = TestResolver::EMPTY;
}

/// Names the host tests' kernel resolver knows, and what it answers.
#[cfg(test)]
pub(crate) type TestNames = &'static [(&'static [u8], Result<[u8; 4], i32>)];

/// Addresses the host tests' kernel resolver knows names for.
#[cfg(test)]
pub(crate) type TestReverse = &'static [([u8; 4], Result<&'static [u8], i32>)];

/// What the host tests' kernel resolver knows.
#[cfg(test)]
pub(crate) struct TestResolver {
    pub(crate) names: TestNames,
    pub(crate) reverse: TestReverse,
    pub(crate) otherwise: i32,
}

#[cfg(test)]
impl TestResolver {
    const EMPTY: Self = Self {
        names: &[],
        reverse: &[],
        otherwise: errno::ENOENT,
    };
}

/// Make the host tests' kernel resolver answer as `r` on this thread.
#[cfg(test)]
pub(crate) fn set_test_resolver(r: TestResolver) {
    // SAFETY: this thread's stand-in.
    unsafe { *test_resolver() = r };
}

#[cfg(test)]
fn test_kernel_resolve(name: &[u8]) -> Result<[u8; 4], i32> {
    // SAFETY: this thread's stand-in.
    let r = unsafe { &*test_resolver() };
    r.names
        .iter()
        .find(|(n, _)| *n == name)
        .map_or(Err(r.otherwise), |(_, a)| *a)
}

#[cfg(test)]
fn test_kernel_reverse(addr: [u8; 4], out: &mut [u8; 256]) -> Result<usize, i32> {
    // SAFETY: this thread's stand-in.
    let r = unsafe { &*test_resolver() };
    let name = r
        .reverse
        .iter()
        .find(|(a, _)| *a == addr)
        .map_or(Err(r.otherwise), |(_, n)| *n)?;
    out[..name.len()].copy_from_slice(name);
    Ok(name.len())
}

// ---------------------------------------------------------------------------
// The lookups: numbers, then files, then the kernel
// ---------------------------------------------------------------------------

/// `__nss_hostname_digits_dots_context` for the `_r` functions: a name that
/// is an address answered as itself.  `None`: not a number, look it up.
fn digits_dots(name: &[u8], af: i32, room: &mut Room, rep: &mut Report<'_>) -> Option<Found> {
    let c0 = *name.first()?;
    if !(c0.is_ascii_hexdigit() || c0 == b':') {
        return None;
    }
    // Any family but the two is read as IPv4 (glibc: unless RES_USE_INET6,
    // which it no longer honours).
    let af = if af == AF_INET6 { AF_INET6 } else { AF_INET };
    // glibc sizes its block before it knows which form the name has.
    let answer = |room: &mut Room, family: i32, len: usize, a: [u8; 16], rep: &mut Report<'_>| {
        match fill_hostent(
            room,
            name,
            family,
            len,
            core::iter::once(a),
            core::iter::empty(),
        ) {
            Ok(e) => {
                rep.set(NETDB_SUCCESS);
                Found {
                    status: Status::Success,
                    entry: Some(e),
                }
            }
            Err(_) => rep.erange(),
        }
    };
    let not_found = |rep: &mut Report<'_>| {
        rep.set(HOST_NOT_FOUND);
        Found::miss(Status::NotFound)
    };
    if c0.is_ascii_digit() && name.iter().all(|&b| b.is_ascii_digit() || b == b'.') {
        // Digits and dots, not ending in a dot: an address or nothing.
        if name.last() == Some(&b'.') {
            return None;
        }
        let parsed = if af == AF_INET {
            crate::inet::aton_exact(name).map(|v4| {
                let mut a = [0u8; 16];
                a[..4].copy_from_slice(&v4);
                a
            })
        } else {
            crate::inet::pton6(name)
        };
        return Some(match parsed {
            Some(a) => answer(
                room,
                af,
                if af == AF_INET { INADDRSZ } else { IN6ADDRSZ },
                a,
                rep,
            ),
            None => not_found(rep),
        });
    }
    let v6ish = (c0.is_ascii_hexdigit() && name.contains(&b':')) || c0 == b':';
    if v6ish
        && name
            .iter()
            .all(|&b| b.is_ascii_hexdigit() || b == b':' || b == b'.')
    {
        if name.last() == Some(&b'.') {
            return None;
        }
        // An IPv6 address cannot be an `AF_INET` answer.
        if af == AF_INET {
            return Some(not_found(rep));
        }
        return Some(match crate::inet::pton6(name) {
            Some(a) => answer(room, AF_INET6, IN6ADDRSZ, a, rep),
            None => not_found(rep),
        });
    }
    if v6ish && af == AF_INET {
        // glibc refuses an IPv6-looking name for `AF_INET` before it reads
        // the rest of it.
        return Some(not_found(rep));
    }
    None
}

/// The modules in `nsswitch.conf`'s order, until one answers: glibc's
/// `getXXbyYY_r` loop, stopping early when a buffer was too small.
fn chain(
    room: &mut Room,
    rep: &mut Report<'_>,
    mut files: impl FnMut(&mut Room, &mut Report<'_>) -> Found,
    mut kernel: impl FnMut(&mut Room, &mut Report<'_>) -> Found,
) -> Found {
    let f = files(room, rep);
    if f.status == Status::Success {
        return f;
    }
    if f.status == Status::TryAgain
        && *rep.herr == NETDB_INTERNAL
        && errno::get_errno() == errno::ERANGE
    {
        return f;
    }
    kernel(room, rep)
}

/// `_res_hconf_reorder_addrs`: with `reorder on`, an IPv4 address on a
/// local subnet goes first.
fn reorder(e: &mut Hostent) {
    if !host_conf().reorder || e.h_addrtype != AF_INET {
        return;
    }
    let nets = local_subnets();
    if nets.is_empty() {
        return;
    }
    let list = e.h_addr_list.cast_mut();
    let mut i = 0usize;
    // SAFETY: `h_addr_list` is this answer's NULL-terminated array of
    // pointers to four-byte addresses, in the caller's buffer.
    unsafe {
        while !(*list.add(i)).is_null() {
            let a = *list.add(i);
            let v = [*a, *a.add(1), *a.add(2), *a.add(3)];
            let v = u32::from_ne_bytes(v);
            if nets.iter().any(|&(addr, mask)| (v ^ addr) & mask == 0) {
                let first = *list;
                *list = a;
                *list.add(i) = first;
                return;
            }
            i += 1;
        }
    }
}

/// The IPv4 subnets of this machine's interfaces, as `(address, mask)` in
/// network byte order: what `reorder` compares against.  Read once and kept,
/// as glibc keeps its list, once there is one.
fn local_subnets() -> &'static [(u32, u32)] {
    crate::perprocess::process_global! {
        fn subnets() -> ([(u32, u32); 16], usize) = ([(0, 0); 16], 0);
    }
    // SAFETY: the process's list; written only while empty.
    let s = unsafe { &mut *subnets() };
    if s.1 == 0 {
        let mut n = 0usize;
        crate::socket::for_each_ipv4_interface(|addr, mask| {
            if let Some(slot) = s.0.get_mut(n) {
                *slot = (addr, mask);
                n += 1;
            }
        });
        s.1 = n;
    }
    s.0.get(..s.1).unwrap_or(&[])
}

/// `_res_hconf_trim_domains`: the first trim domain that ends a name (and
/// is shorter than it), ignoring case, is cut off it.
fn trim_domains(e: &mut Hostent) {
    let conf = host_conf();
    if conf.ntrim == 0 {
        return;
    }
    let trim_one = |name: *const u8| {
        if name.is_null() {
            return;
        }
        // SAFETY: a NUL-terminated name in the caller's buffer, which this
        // answer owns.
        let s = unsafe { nss_files::c_bytes(name) };
        for t in conf.trims() {
            if s.len() > t.len() && s[s.len() - t.len()..].eq_ignore_ascii_case(t) {
                // SAFETY: inside the name just measured.
                unsafe { name.cast_mut().add(s.len() - t.len()).write(0) };
                break;
            }
        }
    };
    trim_one(e.h_name);
    let mut i = 0usize;
    // SAFETY: `h_aliases` is this answer's NULL-terminated array.
    unsafe {
        while !(*e.h_aliases.add(i)).is_null() {
            trim_one(*e.h_aliases.add(i));
            i += 1;
        }
    }
}

/// glibc's `getXXbyYY_r` ending: `*result`, `*h_errnop`, `errno` and the
/// return value from the last module's status.
///
/// # Safety
///
/// `out` and `result` writable.
unsafe fn finish(
    found: Found,
    out: *mut Hostent,
    result: *mut *const Hostent,
    herr: i32,
    h_errnop: *mut i32,
) -> i32 {
    if !h_errnop.is_null() {
        // SAFETY: writable by contract.
        unsafe { h_errnop.write(herr) };
    }
    crate::socket::set_h_errno(herr);
    let res = match found.status {
        Status::Success | Status::NotFound => 0,
        s if errno::get_errno() == errno::ERANGE && s != Status::TryAgain => errno::EINVAL,
        Status::TryAgain if herr != NETDB_INTERNAL => errno::EAGAIN,
        _ => return errno::get_errno(),
    };
    if let (Status::Success, Some(e)) = (found.status, found.entry) {
        // SAFETY: the caller's, writable by contract.
        unsafe { nss_files::deliver(e, out, result) };
    }
    errno::set_errno(res);
    res
}

// ---------------------------------------------------------------------------
// gethostbyname and its kin
// ---------------------------------------------------------------------------

/// `gethostbyname3_r`'s body, for [`gethostbyname2_r`] and the rest: the
/// numeric shortcut, then the modules, then `reorder`.
///
/// # Safety
///
/// As [`gethostbyname2_r`].
unsafe fn byname_r(
    name: *const u8,
    af: i32,
    out: *mut Hostent,
    buf: *mut u8,
    buflen: usize,
    result: *mut *const Hostent,
    h_errnop: *mut i32,
) -> i32 {
    if out.is_null() || buf.is_null() || result.is_null() {
        return errno::EFAULT;
    }
    // SAFETY: non-null, the caller's to write.
    unsafe { result.write(core::ptr::null()) };
    // A module that does not report leaves the caller's value, as glibc's
    // modules write through the caller's pointer.
    let mut herr = if h_errnop.is_null() {
        crate::socket::get_h_errno()
    } else {
        // SAFETY: readable by contract.
        unsafe { h_errnop.read() }
    };
    if name.is_null() {
        herr = HOST_NOT_FOUND;
        // SAFETY: the caller's pointers.
        return unsafe { finish(Found::miss(Status::NotFound), out, result, herr, h_errnop) };
    }
    // SAFETY: the caller's string.
    let name = unsafe { nss_files::c_bytes(name) };
    // SAFETY: the caller gives `buflen` writable bytes at `buf`.
    let mut room = unsafe { Room::new(buf, buflen) };
    let mut rep = Report { herr: &mut herr };
    let mut found = match digits_dots_sized(name, af, buflen, &mut room, &mut rep) {
        Some(f) => f,
        None => chain(
            &mut room,
            &mut rep,
            |room, rep| files_byname3(name, af, room, rep),
            |room, rep| kernel_byname3(name, af, room, rep),
        ),
    };
    if let Some(e) = found.entry.as_mut() {
        reorder(e);
    }
    // SAFETY: the caller's pointers.
    unsafe { finish(found, out, result, herr, h_errnop) }
}

/// [`digits_dots`], after glibc's size check -- which it makes for any name
/// that starts like a number, before reading the rest of it: a buffer too
/// small for an address, three pointers and the name is `ERANGE` even for
/// `example.com`.
fn digits_dots_sized(
    name: &[u8],
    af: i32,
    buflen: usize,
    room: &mut Room,
    rep: &mut Report<'_>,
) -> Option<Found> {
    let c0 = *name.first()?;
    if !(c0.is_ascii_hexdigit() || c0 == b':') {
        return None;
    }
    let need = 16 + 2 * size_of::<*const u8>() + size_of::<*const u8>() + name.len() + 1;
    if buflen < need {
        return Some(rep.erange());
    }
    digits_dots(name, af, room, rep)
}

/// Look up `name` as an `af` address: glibc's `gethostbyname2_r`.
///
/// 0 when the lookup was made, whatever it found: `*result` is `ret` when
/// there is an answer and NULL when not, with `*h_errnop` saying why
/// (`HOST_NOT_FOUND`, `NO_DATA`, `TRY_AGAIN`...).  `ERANGE` when `buflen`
/// cannot hold the answer, `EAGAIN` for a temporary failure, or the
/// `errno` of another.
///
/// # Safety
///
/// `name` NULL or NUL-terminated; `ret` and `result` writable; `buf`
/// writable for `buflen` bytes; `h_errnop` NULL or writable.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn gethostbyname2_r(
    name: *const u8,
    af: i32,
    ret: *mut Hostent,
    buf: *mut u8,
    buflen: usize,
    result: *mut *const Hostent,
    h_errnop: *mut i32,
) -> i32 {
    // SAFETY: the caller's contract.
    unsafe { byname_r(name, af, ret, buf, buflen, result, h_errnop) }
}

/// [`gethostbyname2_r`] for `AF_INET`.
///
/// # Safety
///
/// As [`gethostbyname2_r`].
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn gethostbyname_r(
    name: *const u8,
    ret: *mut Hostent,
    buf: *mut u8,
    buflen: usize,
    result: *mut *const Hostent,
    h_errnop: *mut i32,
) -> i32 {
    // SAFETY: the caller's contract.
    unsafe { byname_r(name, AF_INET, ret, buf, buflen, result, h_errnop) }
}

/// Look up the name of an address: glibc's `gethostbyaddr_r`, answering
/// as [`gethostbyname2_r`] does.  The unspecified IPv6 address (`::`) is
/// `ENOENT` with `HOST_NOT_FOUND`, never looked up; with `trim` in
/// `host.conf`, the listed domains are cut off the names.
///
/// # Safety
///
/// `addr` readable for `len` bytes; the rest as [`gethostbyname2_r`].
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn gethostbyaddr_r(
    addr: *const u8,
    len: u32,
    ty: i32,
    ret: *mut Hostent,
    buf: *mut u8,
    buflen: usize,
    result: *mut *const Hostent,
    h_errnop: *mut i32,
) -> i32 {
    if ret.is_null() || buf.is_null() || result.is_null() {
        return errno::EFAULT;
    }
    // SAFETY: non-null, the caller's to write.
    unsafe { result.write(core::ptr::null()) };
    let len = len as usize;
    let a: &[u8] = if addr.is_null() {
        &[]
    } else {
        // SAFETY: readable for `len` bytes by contract.
        unsafe { core::slice::from_raw_parts(addr, len) }
    };
    if len == IN6ADDRSZ && a == [0u8; 16] {
        if !h_errnop.is_null() {
            // SAFETY: writable by contract.
            unsafe { h_errnop.write(HOST_NOT_FOUND) };
        }
        crate::socket::set_h_errno(HOST_NOT_FOUND);
        return errno::ENOENT;
    }
    let mut herr = if h_errnop.is_null() {
        crate::socket::get_h_errno()
    } else {
        // SAFETY: readable by contract.
        unsafe { h_errnop.read() }
    };
    // SAFETY: the caller gives `buflen` writable bytes at `buf`.
    let mut room = unsafe { Room::new(buf, buflen) };
    let mut rep = Report { herr: &mut herr };
    let mut found = if a.len() == len {
        chain(
            &mut room,
            &mut rep,
            |room, rep| files_byaddr(a, ty, room, rep),
            |room, rep| kernel_byaddr(a, ty, room, rep),
        )
    } else {
        rep.set(HOST_NOT_FOUND);
        Found::miss(Status::NotFound)
    };
    if let Some(e) = found.entry.as_mut() {
        reorder(e);
        trim_domains(e);
    }
    // SAFETY: the caller's pointers.
    unsafe { finish(found, ret, result, herr, h_errnop) }
}

/// A lookup into one of the calling thread's blocks, `h_errno` set as the
/// lookup set it.
fn host_held(
    field: fn(*mut netdb::ThreadDb) -> *mut nss_files::Held<Hostent>,
    call: impl Fn(*mut Hostent, *mut u8, usize, *mut *const Hostent, *mut i32) -> i32,
) -> *const Hostent {
    let mut herr = crate::socket::get_h_errno();
    let r = netdb::held(field, |p, b, l, r| call(p, b, l, r, &raw mut herr));
    crate::socket::set_h_errno(herr);
    match r {
        Ok(p) => {
            errno::set_errno(0);
            p
        }
        Err(e) => {
            errno::set_errno(e);
            if e == errno::ENOMEM {
                crate::socket::set_h_errno(NETDB_INTERNAL);
            }
            core::ptr::null()
        }
    }
}

/// [`gethostbyname2_r`] into the calling thread's block, `h_errno` saying
/// why when it answers NULL.
///
/// # Safety
///
/// `name` is NULL or NUL-terminated.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn gethostbyname2(name: *const u8, af: i32) -> *const Hostent {
    host_held(netdb::host_held_block, |p, b, l, r, h| {
        // SAFETY: the caller's string; the thread's block.
        unsafe { byname_r(name, af, p, b, l, r, h) }
    })
}

/// [`gethostbyname2`] for `AF_INET`.
///
/// # Safety
///
/// `name` is NULL or NUL-terminated.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn gethostbyname(name: *const u8) -> *const Hostent {
    // SAFETY: the caller's contract.
    unsafe { gethostbyname2(name, AF_INET) }
}

/// [`gethostbyaddr_r`] into the calling thread's block (another than
/// `gethostbyname`'s, so a reverse lookup does not overwrite a forward one).
///
/// # Safety
///
/// `addr` readable for `len` bytes.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn gethostbyaddr(addr: *const u8, len: u32, ty: i32) -> *const Hostent {
    host_held(netdb::host_rev_held, |p, b, l, r, h| {
        // SAFETY: the caller's address; the thread's block.
        unsafe { gethostbyaddr_r(addr, len, ty, p, b, l, r, h) }
    })
}

// ---------------------------------------------------------------------------
// sethostent, gethostent, endhostent
// ---------------------------------------------------------------------------

/// The calling thread's next `/etc/hosts` entry, read as IPv4 (so IPv6
/// lines appear only when IPv4-mapped, or `::1` as 127.0.0.1): 0,
/// `ENOENT` at the end with `*h_errnop` `HOST_NOT_FOUND`, `ERANGE`.
///
/// # Safety
///
/// `ret` and `result` writable; `buf` writable for `buflen` bytes;
/// `h_errnop` NULL or writable.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn gethostent_r(
    ret: *mut Hostent,
    buf: *mut u8,
    buflen: usize,
    result: *mut *const Hostent,
    h_errnop: *mut i32,
) -> i32 {
    // SAFETY: the caller's pointers; the thread's cursor.
    let rc = unsafe {
        netdb::next_entry(
            netdb::host_cur,
            Which::Hosts,
            b"",
            |line, room| {
                let h = parse_host(line, AF_INET, 0)?;
                Some(
                    fill_hostent(
                        room,
                        h.name,
                        AF_INET,
                        INADDRSZ,
                        core::iter::once(h.addr),
                        h.aliases(),
                    )
                    .map_err(|_| errno::ERANGE),
                )
            },
            ret,
            buf,
            buflen,
            result,
        )
    };
    let herr = match rc {
        0 => None,
        errno::ENOENT => Some(HOST_NOT_FOUND),
        errno::ERANGE => Some(NETDB_INTERNAL),
        _ => Some(NETDB_INTERNAL),
    };
    if let (Some(h), false) = (herr, h_errnop.is_null()) {
        // SAFETY: writable by contract.
        unsafe { h_errnop.write(h) };
    }
    rc
}

/// The calling thread's next `/etc/hosts` entry, or NULL at the end.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn gethostent() -> *const Hostent {
    let mut herr = crate::socket::get_h_errno();
    let r = netdb::held(netdb::host_ent_held, |p, b, l, r| {
        // SAFETY: the thread's block.
        unsafe { gethostent_r(p, b, l, r, &raw mut herr) }
    });
    crate::socket::set_h_errno(herr);
    nss_files::enumerated(r)
}

/// Start the calling thread's enumeration of `/etc/hosts` again.  `stayopen`
/// changes nothing here.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn sethostent(_stayopen: i32) {
    netdb::close_cursor(netdb::host_cur);
}

/// End the calling thread's enumeration of `/etc/hosts`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn endhostent() {
    netdb::close_cursor(netdb::host_cur);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nss_files::set_test_text;
    use std::format;
    use std::string::String;
    use std::vec::Vec;

    fn text(p: *const u8) -> String {
        // SAFETY: NUL-terminated strings from the functions under test.
        String::from_utf8_lossy(unsafe { nss_files::c_bytes(p) }).into_owned()
    }

    fn ntop(af: i32, p: *const u8) -> String {
        let mut out = [0u8; 64];
        let r = crate::inet::inet_ntop(af, p, out.as_mut_ptr(), 64);
        text(r)
    }

    fn show(h: *const Hostent, rc: i32, herr: i32) -> String {
        if h.is_null() {
            return format!("NULL rc={rc} herr={herr}");
        }
        // SAFETY: a non-null answer.
        let h = unsafe { &*h };
        let mut addrs = Vec::new();
        let mut i = 0;
        // SAFETY: NULL-terminated arrays in the answer.
        unsafe {
            while !(*h.h_addr_list.add(i)).is_null() {
                addrs.push(ntop(h.h_addrtype, *h.h_addr_list.add(i)));
                i += 1;
            }
        }
        let mut aliases = Vec::new();
        i = 0;
        // SAFETY: as above.
        unsafe {
            while !(*h.h_aliases.add(i)).is_null() {
                aliases.push(text(*h.h_aliases.add(i)));
                i += 1;
            }
        }
        format!(
            "name={} type={} len={} addrs=[{}] aliases=[{}]",
            text(h.h_name),
            h.h_addrtype,
            h.h_length,
            addrs.join(","),
            aliases.join(",")
        )
    }

    fn cz(s: &str) -> Vec<u8> {
        let mut v = if s == "-" {
            Vec::new()
        } else {
            s.as_bytes().to_vec()
        };
        v.push(0);
        v
    }

    /// What `hosts_oracle.c` prints for `cmd`, from this crate's functions.
    fn run(cmd: &str, out: &mut Vec<String>) {
        let a: Vec<&str> = cmd.split(' ').collect();
        let mut buf = std::vec![0u8; 8192];
        let b = buf.as_mut_ptr();
        let empty = Hostent {
            h_name: core::ptr::null(),
            h_aliases: core::ptr::null(),
            h_addrtype: 0,
            h_length: 0,
            h_addr_list: core::ptr::null(),
        };
        // SAFETY (every call): NUL-terminated names, and outputs and a
        // buffer this function owns.
        unsafe {
            match a[0] {
                "byname" | "byname2" => {
                    let n = cz(a[1]);
                    let mut hb = empty;
                    let mut r: *const Hostent = core::ptr::null();
                    let mut herr = -99;
                    let rc = if a[0] == "byname" {
                        gethostbyname_r(n.as_ptr(), &mut hb, b, 8192, &mut r, &mut herr)
                    } else {
                        gethostbyname2_r(
                            n.as_ptr(),
                            a[2].parse().unwrap(),
                            &mut hb,
                            b,
                            8192,
                            &mut r,
                            &mut herr,
                        )
                    };
                    out.push(show(r, rc, herr));
                }
                "byaddr" => {
                    let af: i32 = a[2].parse().unwrap();
                    let t = cz(a[1]);
                    let mut addr = [0u8; 16];
                    crate::inet::inet_pton(
                        if af == 10 { AF_INET6 } else { AF_INET },
                        t.as_ptr(),
                        addr.as_mut_ptr(),
                    );
                    let mut hb = empty;
                    let mut r: *const Hostent = core::ptr::null();
                    let mut herr = -99;
                    let rc = gethostbyaddr_r(
                        addr.as_ptr(),
                        a[3].parse().unwrap(),
                        af,
                        &mut hb,
                        b,
                        8192,
                        &mut r,
                        &mut herr,
                    );
                    out.push(show(r, rc, herr));
                }
                "small" => {
                    let n = cz(a[1]);
                    let mut hb = empty;
                    let mut r: *const Hostent = core::ptr::null();
                    let mut herr = -99;
                    let rc = gethostbyname_r(
                        n.as_ptr(),
                        &mut hb,
                        b,
                        a[2].parse().unwrap(),
                        &mut r,
                        &mut herr,
                    );
                    out.push(format!(
                        "rc={rc} herr={herr} found={}",
                        i32::from(!r.is_null())
                    ));
                }
                "nr" | "nr2" => {
                    let n = cz(a[1]);
                    crate::socket::set_h_errno(-99);
                    let h = if a[0] == "nr" {
                        gethostbyname(n.as_ptr())
                    } else {
                        gethostbyname2(n.as_ptr(), a[2].parse().unwrap())
                    };
                    out.push(show(h, 0, crate::socket::get_h_errno()));
                }
                "ent" => {
                    sethostent(0);
                    loop {
                        let h = gethostent();
                        if h.is_null() {
                            break;
                        }
                        out.push(show(h, 0, 0));
                    }
                    out.push(format!("end herr={}", crate::socket::get_h_errno()));
                    endhostent();
                }
                other => panic!("unknown command {other}"),
            }
        }
    }

    /// Commands whose answer turns on exactly where glibc's `nss_files`
    /// lays a line out in the caller's buffer: a buffer it finds too small
    /// may be enough here.  `ERANGE`'s threshold is not the interface.
    const LAYOUT_DEPENDENT: &[&str] = &["small localhost 60", "small multi.example 60"];

    /// The network is down: every name the kernel is asked is unreachable.
    fn no_network() {
        set_test_resolver(TestResolver {
            names: &[],
            reverse: &[],
            otherwise: errno::ENETUNREACH,
        });
    }

    fn compare(conf: &'static [u8], want: &str, want_stderr: &str) {
        set_test_text(Which::Hosts, Some(HOSTS_FILE));
        set_test_text(Which::HostConf, Some(conf));
        reset_host_conf();
        // SAFETY: this thread's record.
        unsafe { (*complaints()).clear() };
        no_network();
        let mut ours = Vec::new();
        let mut wanted = Vec::new();
        // Enumerations print several lines: compare the whole transcripts,
        // leaving out the layout-dependent commands' lines.
        let mut glibc = want.lines();
        for cmd in COMMANDS.lines() {
            let before = ours.len();
            run(cmd, &mut ours);
            let n = ours.len() - before;
            let theirs: Vec<&str> = (0..n).filter_map(|_| glibc.next()).collect();
            if LAYOUT_DEPENDENT.contains(&cmd) {
                ours.truncate(before);
                continue;
            }
            wanted.extend(theirs.into_iter().map(String::from));
        }
        let mut wrong = Vec::new();
        for i in 0..wanted.len().max(ours.len()) {
            let (w, o) = (wanted.get(i), ours.get(i));
            if w != o {
                wrong.push(format!("line {}\n  glibc: {w:?}\n  ours:  {o:?}", i + 1));
            }
        }
        assert!(
            wrong.is_empty(),
            "{} differ:\n{}",
            wrong.len(),
            wrong.join("\n")
        );
        // SAFETY: this thread's record.
        let said = String::from_utf8(unsafe { (*complaints()).clone() }).unwrap();
        assert_eq!(said, want_stderr);
    }

    #[test]
    fn every_answer_is_glibcs_without_multi() {
        compare(CONF_PLAIN, GLIBC_PLAIN, GLIBC_PLAIN_STDERR);
    }

    #[test]
    fn every_answer_is_glibcs_with_multi_reorder_and_trim() {
        compare(CONF_MULTI, GLIBC_MULTI, GLIBC_MULTI_STDERR);
    }

    #[test]
    fn the_kernel_answers_what_the_file_does_not() {
        set_test_text(Which::Hosts, Some(b"10.0.0.9 filed\n"));
        set_test_text(Which::HostConf, None);
        reset_host_conf();
        set_test_resolver(TestResolver {
            names: &[
                (b"example.com", Ok([93, 184, 216, 34])),
                (b"slow.example", Err(errno::ETIMEDOUT)),
                (b"denied.example", Err(errno::EACCES)),
            ],
            reverse: &[([93, 184, 216, 34], Ok(b"example.com"))],
            otherwise: errno::ENOENT,
        });
        let mut out = Vec::new();
        for cmd in [
            "byname filed",
            "byname example.com",
            "byname2 example.com 10",
            "byname nosuch.example",
            "byname2 nosuch.example 10",
            "byname slow.example",
            "byname denied.example",
            "byname bad_name!",
            "byaddr 93.184.216.34 2 4",
            "byaddr ::ffff:93.184.216.34 10 16",
            "byaddr 2001:db8::1 10 16",
            "byaddr 10.1.1.1 2 4",
        ] {
            run(cmd, &mut out);
        }
        assert_eq!(
            out,
            [
                "name=filed type=2 len=4 addrs=[10.0.0.9] aliases=[]",
                "name=example.com type=2 len=4 addrs=[93.184.216.34] aliases=[]",
                // The name exists; the kernel has no IPv6 address for it.
                "NULL rc=0 herr=4",
                "NULL rc=0 herr=1",
                "NULL rc=0 herr=1",
                // No answer in time: "unavailable", errno EAGAIN.
                "NULL rc=11 herr=2",
                // Not allowed to ask: the error is errno's.
                "NULL rc=13 herr=-1",
                // Not a host name: never asked.
                "NULL rc=0 herr=1",
                "name=example.com type=2 len=4 addrs=[93.184.216.34] aliases=[]",
                // A mapped address is asked, and answered, as IPv4.
                "name=example.com type=2 len=4 addrs=[93.184.216.34] aliases=[]",
                "NULL rc=0 herr=1",
                "NULL rc=0 herr=1",
            ]
        );
    }

    #[test]
    fn host_conf_complaints_are_glibcs() {
        set_test_text(
            Which::HostConf,
            Some(b"bogus on\nmulti maybe\nmulti only\nreorder off junk\ntrim a, b, c, d, e\ntrim x,\norder hosts,bind\n"),
        );
        reset_host_conf();
        // SAFETY: this thread's record.
        unsafe { (*complaints()).clear() };
        let c = host_conf();
        assert!(c.multi, "`only` begins with `on`");
        assert!(!c.reorder);
        assert_eq!(c.trims().collect::<Vec<_>>(), [&b"a"[..], b"b", b"c", b"d"]);
        // SAFETY: this thread's record.
        let said = String::from_utf8(unsafe { (*complaints()).clone() }).unwrap();
        assert_eq!(
            said,
            "/etc/host.conf: line 1: bad command `bogus on'\n\
             /etc/host.conf: line 2: expected `on' or `off', found `maybe'\n\
             /etc/host.conf: line 3: ignoring trailing garbage `ly'\n\
             /etc/host.conf: line 4: ignoring trailing garbage `junk'\n\
             /etc/host.conf: line 5: cannot specify more than 4 trim domains\
             /etc/host.conf: line 6: cannot specify more than 4 trim domains"
        );
        set_test_text(Which::HostConf, None);
        reset_host_conf();
    }

    #[test]
    fn a_missing_hosts_file_leaves_the_kernel() {
        set_test_text(Which::Hosts, None);
        set_test_text(Which::HostConf, None);
        reset_host_conf();
        set_test_resolver(TestResolver {
            names: &[(b"localhost", Ok([127, 0, 0, 1]))],
            reverse: &[],
            otherwise: errno::ENOENT,
        });
        let mut out = Vec::new();
        run("byname localhost", &mut out);
        run("ent", &mut out);
        assert_eq!(
            out,
            [
                "name=localhost type=2 len=4 addrs=[127.0.0.1] aliases=[]",
                "end herr=1",
            ]
        );
    }

    #[test]
    fn the_non_reentrant_forms_use_separate_blocks() {
        set_test_text(Which::Hosts, Some(b"10.0.0.1 one\n10.0.0.2 two\n"));
        set_test_text(Which::HostConf, None);
        reset_host_conf();
        // SAFETY: NUL-terminated names and a readable address.
        unsafe {
            let fwd = gethostbyname(c"one".as_ptr().cast());
            let rev = gethostbyaddr([10u8, 0, 0, 2].as_ptr(), 4, AF_INET);
            assert!(!fwd.is_null() && !rev.is_null());
            assert_eq!(
                text((*fwd).h_name),
                "one",
                "a reverse lookup keeps the forward answer"
            );
            assert_eq!(text((*rev).h_name), "two");
        }
        netdb::thread_cleanup();
    }

    #[test]
    fn res_hnok_is_glibcs() {
        use crate::resolv::hnok as res_hnok;
        for ok in [
            &b"example.com"[..],
            b"a-b.c_d",
            b"x",
            b"example.com.",
            b"1.2.3.4",
            b"",
        ] {
            assert!(res_hnok(ok), "{:?}", String::from_utf8_lossy(ok));
        }
        for bad in [
            &b"-lead.example"[..],
            b"a b",
            b"a:b",
            b"caf\xc3\xa9",
            b"a..b",
            b"x!",
        ] {
            assert!(!res_hnok(bad), "{:?}", String::from_utf8_lossy(bad));
        }
    }

    // Generated by posix/tools/oracle/hosts_harness.py: the files, the commands, and
    // what glibc 2.39 printed for them (hosts_oracle.c), with no network.
    const HOSTS_FILE: &[u8] = b"# The test's hosts file\n127.0.0.1\tlocalhost\n127.0.1.1\tmyhost.example.com\tmyhost\n::1\tlocalhost ip6-localhost ip6-loopback\n10.0.0.1 multi.example multi\n10.0.0.2 multi.example\nfe80::1 multi.example multi6\n127.0.0.5 Multi.Example local5\n::ffff:192.168.9.9 mapped.example mapped\n192.168.1.10 Case.Example ALIAS\nbad.address badline\n1.2.3.4\n1.2.3.5 twice\n1.2.3.6 twice  # comment\n0x7f000002 hexaddr\n010.0.0.1 octaddr\nfe80::2%eth0 scoped\n2001:db8::5 v6only v6alias\n10.9.9.9 trim.example.com trimalias.example.com other.example.org\n  172.16.0.1   spaced   sp2\t\n172.16.0.2 nul\x00hidden\n";
    const CONF_PLAIN: &[u8] = b"# nothing\n";
    const CONF_MULTI: &[u8] = b"multi on\nreorder on\ntrim example.com\n";
    const COMMANDS: &str = "\
byname localhost\n\
byname LOCALHOST\n\
byname myhost\n\
byname myhost.example.com\n\
byname ip6-localhost\n\
byname2 ip6-localhost 10\n\
byname2 localhost 10\n\
byname multi.example\n\
byname multi\n\
byname2 multi.example 10\n\
byname multi6\n\
byname2 multi6 10\n\
byname mapped\n\
byname2 mapped 10\n\
byname case.example\n\
byname alias\n\
byname badline\n\
byname twice\n\
byname hexaddr\n\
byname octaddr\n\
byname scoped\n\
byname2 v6only 10\n\
byname2 v6alias 10\n\
byname v6only\n\
byname spaced\n\
byname sp2\n\
byname nul\n\
byname hidden\n\
byname 1.2.3.4\n\
byname 1.2.3.4.\n\
byname 127.1\n\
byname 0x7f000001\n\
byname2 ::1 10\n\
byname ::1\n\
byname2 1.2.3.4 10\n\
byname2 ::ffff:1.2.3.4 10\n\
byname abc:def\n\
byname2 abc:def 10\n\
byname2 abc:def. 10\n\
byname nothere\n\
byname -\n\
byname2 localhost 99\n\
byaddr 127.0.0.1 2 4\n\
byaddr 10.0.0.2 2 4\n\
byaddr ::1 10 16\n\
byaddr ::ffff:127.0.0.1 10 16\n\
byaddr 10.9.9.9 2 4\n\
byaddr 1.1.1.1 2 4\n\
byaddr :: 10 16\n\
byaddr 127.0.0.1 10 4\n\
byaddr 127.0.0.1 2 16\n\
byaddr 2001:db8::5 10 16\n\
small localhost 20\n\
small localhost 60\n\
small localhost 200\n\
small 1.2.3.4 40\n\
small 1.2.3.4 60\n\
small example.com 10\n\
small zzz 10\n\
small multi.example 60\n\
nr localhost\n\
nr nothere\n\
nr2 localhost 10\n\
nr2 1.2.3.4 10\n\
ent\n\
";
    const GLIBC_PLAIN: &str = "\
name=localhost type=2 len=4 addrs=[127.0.0.1] aliases=[]\n\
name=localhost type=2 len=4 addrs=[127.0.0.1] aliases=[]\n\
name=myhost.example.com type=2 len=4 addrs=[127.0.1.1] aliases=[myhost]\n\
name=myhost.example.com type=2 len=4 addrs=[127.0.1.1] aliases=[myhost]\n\
name=localhost type=2 len=4 addrs=[127.0.0.1] aliases=[ip6-localhost,ip6-loopback]\n\
name=localhost type=10 len=16 addrs=[::1] aliases=[ip6-localhost,ip6-loopback]\n\
name=localhost type=10 len=16 addrs=[::1] aliases=[ip6-localhost,ip6-loopback]\n\
name=multi.example type=2 len=4 addrs=[10.0.0.1] aliases=[multi]\n\
name=multi.example type=2 len=4 addrs=[10.0.0.1] aliases=[multi]\n\
name=multi.example type=10 len=16 addrs=[fe80::1] aliases=[multi6]\n\
NULL rc=11 herr=2\n\
name=multi.example type=10 len=16 addrs=[fe80::1] aliases=[multi6]\n\
name=mapped.example type=2 len=4 addrs=[192.168.9.9] aliases=[mapped]\n\
name=mapped.example type=10 len=16 addrs=[::ffff:192.168.9.9] aliases=[mapped]\n\
name=Case.Example type=2 len=4 addrs=[192.168.1.10] aliases=[ALIAS]\n\
name=Case.Example type=2 len=4 addrs=[192.168.1.10] aliases=[ALIAS]\n\
NULL rc=11 herr=2\n\
name=twice type=2 len=4 addrs=[1.2.3.5] aliases=[]\n\
NULL rc=11 herr=2\n\
NULL rc=11 herr=2\n\
NULL rc=11 herr=2\n\
name=v6only type=10 len=16 addrs=[2001:db8::5] aliases=[v6alias]\n\
name=v6only type=10 len=16 addrs=[2001:db8::5] aliases=[v6alias]\n\
NULL rc=11 herr=2\n\
name=spaced type=2 len=4 addrs=[172.16.0.1] aliases=[sp2]\n\
name=spaced type=2 len=4 addrs=[172.16.0.1] aliases=[sp2]\n\
name=nul type=2 len=4 addrs=[172.16.0.2] aliases=[]\n\
NULL rc=11 herr=2\n\
name=1.2.3.4 type=2 len=4 addrs=[1.2.3.4] aliases=[]\n\
NULL rc=11 herr=2\n\
name=127.1 type=2 len=4 addrs=[127.0.0.1] aliases=[]\n\
NULL rc=11 herr=2\n\
name=::1 type=10 len=16 addrs=[::1] aliases=[]\n\
NULL rc=0 herr=1\n\
NULL rc=0 herr=1\n\
name=::ffff:1.2.3.4 type=10 len=16 addrs=[::ffff:1.2.3.4] aliases=[]\n\
NULL rc=0 herr=1\n\
NULL rc=0 herr=1\n\
NULL rc=0 herr=1\n\
NULL rc=11 herr=2\n\
name= type=2 len=4 addrs=[1.2.3.4] aliases=[]\n\
NULL rc=97 herr=4\n\
name=localhost type=2 len=4 addrs=[127.0.0.1] aliases=[]\n\
name=multi.example type=2 len=4 addrs=[10.0.0.2] aliases=[]\n\
name=localhost type=10 len=16 addrs=[::1] aliases=[ip6-localhost,ip6-loopback]\n\
name=localhost type=10 len=16 addrs=[::ffff:127.0.0.1] aliases=[]\n\
name=trim.example.com type=2 len=4 addrs=[10.9.9.9] aliases=[trimalias.example.com,other.example.org]\n\
NULL rc=0 herr=2\n\
NULL rc=2 herr=1\n\
NULL rc=97 herr=-1\n\
NULL rc=0 herr=2\n\
name=v6only type=10 len=16 addrs=[2001:db8::5] aliases=[v6alias]\n\
rc=34 herr=-1 found=0\n\
rc=34 herr=-1 found=0\n\
rc=0 herr=-99 found=1\n\
rc=34 herr=-1 found=0\n\
rc=0 herr=0 found=1\n\
rc=34 herr=-1 found=0\n\
rc=34 herr=-1 found=0\n\
rc=34 herr=-1 found=0\n\
name=localhost type=2 len=4 addrs=[127.0.0.1] aliases=[]\n\
NULL rc=0 herr=2\n\
name=localhost type=10 len=16 addrs=[::1] aliases=[ip6-localhost,ip6-loopback]\n\
NULL rc=0 herr=1\n\
name=localhost type=2 len=4 addrs=[127.0.0.1] aliases=[]\n\
name=myhost.example.com type=2 len=4 addrs=[127.0.1.1] aliases=[myhost]\n\
name=localhost type=2 len=4 addrs=[127.0.0.1] aliases=[ip6-localhost,ip6-loopback]\n\
name=multi.example type=2 len=4 addrs=[10.0.0.1] aliases=[multi]\n\
name=multi.example type=2 len=4 addrs=[10.0.0.2] aliases=[]\n\
name=Multi.Example type=2 len=4 addrs=[127.0.0.5] aliases=[local5]\n\
name=mapped.example type=2 len=4 addrs=[192.168.9.9] aliases=[mapped]\n\
name=Case.Example type=2 len=4 addrs=[192.168.1.10] aliases=[ALIAS]\n\
name= type=2 len=4 addrs=[1.2.3.4] aliases=[]\n\
name=twice type=2 len=4 addrs=[1.2.3.5] aliases=[]\n\
name=twice type=2 len=4 addrs=[1.2.3.6] aliases=[]\n\
name=trim.example.com type=2 len=4 addrs=[10.9.9.9] aliases=[trimalias.example.com,other.example.org]\n\
name=spaced type=2 len=4 addrs=[172.16.0.1] aliases=[sp2]\n\
name=nul type=2 len=4 addrs=[172.16.0.2] aliases=[]\n\
end herr=1\n\
";
    const GLIBC_PLAIN_STDERR: &str = "\
";
    const GLIBC_MULTI: &str = "\
name=localhost type=2 len=4 addrs=[127.0.0.1,127.0.0.1] aliases=[ip6-localhost,ip6-loopback]\n\
name=localhost type=2 len=4 addrs=[127.0.0.1,127.0.0.1] aliases=[ip6-localhost,ip6-loopback]\n\
name=myhost.example.com type=2 len=4 addrs=[127.0.1.1] aliases=[myhost]\n\
name=myhost.example.com type=2 len=4 addrs=[127.0.1.1] aliases=[myhost]\n\
name=localhost type=2 len=4 addrs=[127.0.0.1] aliases=[ip6-localhost,ip6-loopback]\n\
name=localhost type=10 len=16 addrs=[::1] aliases=[ip6-localhost,ip6-loopback]\n\
name=localhost type=10 len=16 addrs=[::1] aliases=[ip6-localhost,ip6-loopback]\n\
name=multi.example type=2 len=4 addrs=[127.0.0.5,10.0.0.2,10.0.0.1] aliases=[multi,local5,Multi.Example]\n\
name=multi.example type=2 len=4 addrs=[10.0.0.1] aliases=[multi]\n\
name=multi.example type=10 len=16 addrs=[fe80::1] aliases=[multi6]\n\
NULL rc=11 herr=2\n\
name=multi.example type=10 len=16 addrs=[fe80::1] aliases=[multi6]\n\
name=mapped.example type=2 len=4 addrs=[192.168.9.9] aliases=[mapped]\n\
name=mapped.example type=10 len=16 addrs=[::ffff:192.168.9.9] aliases=[mapped]\n\
name=Case.Example type=2 len=4 addrs=[192.168.1.10] aliases=[ALIAS]\n\
name=Case.Example type=2 len=4 addrs=[192.168.1.10] aliases=[ALIAS]\n\
NULL rc=11 herr=2\n\
name=twice type=2 len=4 addrs=[1.2.3.5,1.2.3.6] aliases=[]\n\
NULL rc=11 herr=2\n\
NULL rc=11 herr=2\n\
NULL rc=11 herr=2\n\
name=v6only type=10 len=16 addrs=[2001:db8::5] aliases=[v6alias]\n\
name=v6only type=10 len=16 addrs=[2001:db8::5] aliases=[v6alias]\n\
NULL rc=11 herr=2\n\
name=spaced type=2 len=4 addrs=[172.16.0.1] aliases=[sp2]\n\
name=spaced type=2 len=4 addrs=[172.16.0.1] aliases=[sp2]\n\
name=nul type=2 len=4 addrs=[172.16.0.2] aliases=[]\n\
NULL rc=11 herr=2\n\
name=1.2.3.4 type=2 len=4 addrs=[1.2.3.4] aliases=[]\n\
NULL rc=11 herr=2\n\
name=127.1 type=2 len=4 addrs=[127.0.0.1] aliases=[]\n\
NULL rc=11 herr=2\n\
name=::1 type=10 len=16 addrs=[::1] aliases=[]\n\
NULL rc=0 herr=1\n\
NULL rc=0 herr=1\n\
name=::ffff:1.2.3.4 type=10 len=16 addrs=[::ffff:1.2.3.4] aliases=[]\n\
NULL rc=0 herr=1\n\
NULL rc=0 herr=1\n\
NULL rc=0 herr=1\n\
NULL rc=11 herr=2\n\
name= type=2 len=4 addrs=[1.2.3.4] aliases=[]\n\
NULL rc=97 herr=4\n\
name=localhost type=2 len=4 addrs=[127.0.0.1] aliases=[]\n\
name=multi.example type=2 len=4 addrs=[10.0.0.2] aliases=[]\n\
name=localhost type=10 len=16 addrs=[::1] aliases=[ip6-localhost,ip6-loopback]\n\
name=localhost type=10 len=16 addrs=[::ffff:127.0.0.1] aliases=[]\n\
name=trim. type=2 len=4 addrs=[10.9.9.9] aliases=[trimalias.,other.example.org]\n\
NULL rc=0 herr=2\n\
NULL rc=2 herr=1\n\
NULL rc=97 herr=-1\n\
NULL rc=0 herr=2\n\
name=v6only type=10 len=16 addrs=[2001:db8::5] aliases=[v6alias]\n\
rc=34 herr=-1 found=0\n\
rc=34 herr=-1 found=0\n\
rc=0 herr=1 found=1\n\
rc=34 herr=-1 found=0\n\
rc=0 herr=0 found=1\n\
rc=34 herr=-1 found=0\n\
rc=34 herr=-1 found=0\n\
rc=34 herr=-1 found=0\n\
name=localhost type=2 len=4 addrs=[127.0.0.1,127.0.0.1] aliases=[ip6-localhost,ip6-loopback]\n\
NULL rc=0 herr=2\n\
name=localhost type=10 len=16 addrs=[::1] aliases=[ip6-localhost,ip6-loopback]\n\
NULL rc=0 herr=1\n\
name=localhost type=2 len=4 addrs=[127.0.0.1] aliases=[]\n\
name=myhost.example.com type=2 len=4 addrs=[127.0.1.1] aliases=[myhost]\n\
name=localhost type=2 len=4 addrs=[127.0.0.1] aliases=[ip6-localhost,ip6-loopback]\n\
name=multi.example type=2 len=4 addrs=[10.0.0.1] aliases=[multi]\n\
name=multi.example type=2 len=4 addrs=[10.0.0.2] aliases=[]\n\
name=Multi.Example type=2 len=4 addrs=[127.0.0.5] aliases=[local5]\n\
name=mapped.example type=2 len=4 addrs=[192.168.9.9] aliases=[mapped]\n\
name=Case.Example type=2 len=4 addrs=[192.168.1.10] aliases=[ALIAS]\n\
name= type=2 len=4 addrs=[1.2.3.4] aliases=[]\n\
name=twice type=2 len=4 addrs=[1.2.3.5] aliases=[]\n\
name=twice type=2 len=4 addrs=[1.2.3.6] aliases=[]\n\
name=trim.example.com type=2 len=4 addrs=[10.9.9.9] aliases=[trimalias.example.com,other.example.org]\n\
name=spaced type=2 len=4 addrs=[172.16.0.1] aliases=[sp2]\n\
name=nul type=2 len=4 addrs=[172.16.0.2] aliases=[]\n\
end herr=1\n\
";
    const GLIBC_MULTI_STDERR: &str = "\
";
}
