//! `hostname` -- show or set the system's host name: Debian's `hostname`
//! 3.23, ported.
//!
//! Upstream is `hostname.c`, by Peter Tobias, Bernd Eckenfels, Graham Wilson
//! and Michael Meskes, the `hostname` Debian split out of net-tools; it also
//! shows and sets the NIS domain name.
//!
//! ```text
//! hostname [-a|-A|-d|-f|-i|-I|-s|-y]
//! hostname [-h|-V]
//! hostname [-b] {name|-F file}
//! dnsdomainname
//! domainname, nisdomainname, ypdomainname [name|-F file]
//! ```
//!
//! Every answer is the C library's, asked the way upstream asks it: the name
//! with `gethostname` and `sethostname`, the NIS domain with `getdomainname`
//! and `setdomainname`, `-f` and `-d` from `getaddrinfo`'s canonical name for
//! the host name, `-i` from the same lookup's addresses, `-a` from
//! `gethostbyname`'s aliases, and `-I` and `-A` from `getifaddrs` through
//! `getnameinfo` -- by way of `libcall` and its `netdb` module, so a script is
//! told what any C program on the same system is told. SlateOS's library
//! resolves as glibc does under `hosts: files dns`, so `-f` is the first name
//! on this host's `/etc/hosts` line when it has one.
//!
//! What this replaces read `/proc/sys/kernel/hostname`, `/etc/hosts` and
//! `/etc/resolv.conf` itself, because the library could not answer then: its
//! `gethostname` kept a process-local copy, and `AI_CANONNAME` echoed the
//! question back. Both are fixed, and a second resolver in here could only
//! ever disagree with the first. It also wrote `/etc/hostname`, which upstream
//! never does: `hostname NAME` renames the running system, and the name kept
//! across boots is `/etc/hostname`'s, which `hostnamectl set-hostname` writes.
//!
//! The name the program is run by picks its default, as upstream's does:
//! `dnsdomainname` is `hostname -d`; `domainname` shows or sets the NIS
//! domain; `nisdomainname` and `ypdomainname` are `hostname -y`, which
//! reports an unset domain as an error. Debian installs the four as links to
//! `hostname`, and SlateOS installs them as further names of this binary
//! (`scripts/rootfs-bin-manifest.txt`).
//!
//! Upstream's behaviour, kept: options are glibc `getopt_long`'s, and a bad
//! one prints getopt's message and then the usage on **standard output**,
//! exiting 255 (`usage (stdout)`, then `exit (-1)`), as `-h` and `--help` do;
//! an operand that cannot be used -- a second one, one beside `-F`, or one with
//! a display option -- prints the usage on standard error, exit 255; a host
//! name given to set is trimmed of white space and must pass RFC 1035's
//! letters, digits and hyphens rule, where a domain name is set exactly as
//! given; `-F FILE` takes the first line of the file that does not begin with
//! a newline or `#`, through upstream's `fgets` loop, and with `-b` a missing
//! or empty file sets the current name, or `localhost`; a failure to set
//! other than "not permitted" and "too long" is not reported, as upstream
//! tests for those two only; `-I` and `-A` print each address followed by a
//! space, then a newline; and a write that fails is not reported, as
//! upstream's `printf`s are unchecked. `scripts/hostname-diff.sh` holds all of
//! it to Ubuntu's `hostname`, under each of its five names.
//!
//! Upstream's notice is the `Debian hostname` entry of
//! `userspace/coreutils/licenses/notices.yaml`.

use std::ffi::{CString, OsString};
use std::fs::File;
use std::io::{self, BufRead, BufReader, Write};
use std::ops::ControlFlow;
use std::process::ExitCode;

use coreutils::errmsg::strerror;
use coreutils::getopt::{Opt, Program, Takes};
use libcall::netdb::{
    AF_INET, AF_INET6, AI_CANONNAME, EAI_NONAME, IFF_LOOPBACK, IFF_UP, INET6_ADDRSTRLEN,
    NI_NAMEREQD, NI_NUMERICHOST, SOCK_DGRAM,
};

#[cfg(test)]
mod tests;

/// The parser's name for this program. Its referral is never printed --
/// upstream answers a bad option with the usage itself -- and its status is
/// upstream's `exit (-1)`.
const HOSTNAME: Program = Program::new("hostname", 255);

/// Upstream's option string, verbatim. `?` is a real option there, as `-h`
/// is, so `hostname -?` prints the usage without a complaint.
const SHORT_OPTIONS: &str = "aAdfbF:h?iIsVy";

/// Upstream's `long_options`, in declaration order, which getopt's ambiguity
/// message makes visible.
const LONG_OPTIONS: &[(&str, Takes)] = &[
    ("domain", Takes::Nothing),
    ("boot", Takes::Nothing),
    ("file", Takes::Required),
    ("fqdn", Takes::Nothing),
    ("all-fqdns", Takes::Nothing),
    ("help", Takes::Nothing),
    ("long", Takes::Nothing),
    ("short", Takes::Nothing),
    ("version", Takes::Nothing),
    ("alias", Takes::Nothing),
    ("ip-address", Takes::Nothing),
    ("all-ip-addresses", Takes::Nothing),
    ("nis", Takes::Nothing),
    ("yp", Takes::Nothing),
];

/// Spellings upstream gives one option, which getopt therefore does not call
/// ambiguous: `--long` is `--fqdn`, `--yp` is `--nis`.
const LONG_ALIASES: &[(&str, &str)] = &[("long", "fqdn"), ("yp", "nis")];

/// `usage ()`'s text, verbatim.
const USAGE: &[u8] = b"Usage: hostname [-b] {hostname|-F file}         set host name (from file)
       hostname [-a|-A|-d|-f|-i|-I|-s|-y]       display formatted name
       hostname                                 display host name

       {yp,nis,}domainname {nisdomain|-F file}  set NIS domain name (from file)
       {yp,nis,}domainname                      display NIS domain name

       dnsdomainname                            display dns domain name

       hostname -V|--version|-h|--help          print info and exit

Program name:
       {yp,nis,}domainname=hostname -y
       dnsdomainname=hostname -d

Program options:
    -a, --alias            alias names
    -A, --all-fqdns        all long host names (FQDNs)
    -b, --boot             set default hostname if none available
    -d, --domain           DNS domain name
    -f, --fqdn, --long     long host name (FQDN)
    -F, --file             read host name or NIS domain name from given file
    -i, --ip-address       addresses for the host name
    -I, --all-ip-addresses all addresses for the host
    -s, --short            short host name
    -y, --yp, --nis        NIS/YP domain name

Description:
   This command can get or set the host name or the NIS domain name. You can
   also get the DNS domain or the FQDN (fully qualified domain name).
   Unless you are using bind or NIS for host lookups you can change the
   FQDN (Fully Qualified Domain Name) and the DNS domain name (which is
   part of the FQDN) in the /etc/hosts file.
";

/// What to show or set: upstream's `enum type_t`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Type {
    /// The host name.
    Default,
    /// `-d`: the DNS domain -- the canonical name after its first dot.
    Dns,
    /// `-f`: the canonical name.
    Fqdn,
    /// `-s`: the host name up to its first dot.
    Short,
    /// `-a`: the host name's aliases.
    Alias,
    /// `-i`: the addresses the host name resolves to.
    Ip,
    /// `domainname`: the NIS domain, printed as the library gives it.
    Nis,
    /// `-y`: the NIS domain, an unset one being an error.
    NisDef,
    /// `-A`: the name of every configured interface address.
    AllFqdns,
    /// `-I`: every configured interface address.
    AllIps,
}

/// `getaddrinfo (host, NULL, {SOCK_DGRAM, AI_CANONNAME})`, as `hostname`
/// reads its answer.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Lookup {
    /// The first entry's `ai_canonname`. glibc always sets it when asked;
    /// were it missing, upstream would dereference NULL, and this reads it as
    /// empty instead.
    pub(crate) canonical: Option<Vec<u8>>,
    /// Each entry's address through `getnameinfo (NI_NUMERICHOST)` into
    /// `INET6_ADDRSTRLEN` bytes, in list order, or the `EAI_*` code it failed
    /// with.
    pub(crate) numeric: Vec<Result<Vec<u8>, i32>>,
}

/// One entry of the library's interface list.
pub(crate) trait Interface {
    /// `ifa_flags`.
    fn flags(&self) -> u32;
    /// `ifa_addr->sa_family`, or `None` for an entry with no address.
    fn family(&self) -> Option<i32>;
    /// `IN6_IS_ADDR_LINKLOCAL || IN6_IS_ADDR_MC_LINKLOCAL`.
    fn is_ipv6_link_local(&self) -> bool;
    /// `getnameinfo` on the address into `NI_MAXHOST` bytes, with `flags`.
    fn name_info(&self, flags: i32) -> Result<Vec<u8>, i32>;
}

/// The C library, as `hostname` asks it. A trait so the tests can stand in a
/// library whose answers they choose. Errors are the library's numbers:
/// `errno` values, `EAI_*` codes, `h_errno`.
pub(crate) trait System {
    /// `localhost ()`: `gethostname` into a buffer grown until the name fits.
    fn hostname(&self) -> Result<Vec<u8>, i32>;
    /// `localdomain ()`: `getdomainname`, likewise.
    fn domainname(&self) -> Result<Vec<u8>, i32>;
    /// `sethostname (name, strlen (name))`.
    fn set_hostname(&self, name: &[u8]) -> Result<(), i32>;
    /// `setdomainname (name, strlen (name))`.
    fn set_domainname(&self, name: &[u8]) -> Result<(), i32>;
    /// `getaddrinfo` on `host`, as [`Lookup`] describes.
    fn lookup(&self, host: &[u8]) -> Result<Lookup, i32>;
    /// `gethostbyname (host)->h_aliases`, or `h_errno`.
    fn aliases(&self, host: &[u8]) -> Result<Vec<Vec<u8>>, i32>;
    /// `getifaddrs`, each entry handed to `visit` until it breaks; the
    /// `errno` when the list cannot be had.
    fn interfaces(
        &self,
        visit: &mut dyn FnMut(&dyn Interface) -> ControlFlow<()>,
    ) -> Result<(), i32>;
    /// `gai_strerror (code)`.
    fn gai_strerror(&self, code: i32) -> Vec<u8>;
    /// `hstrerror (code)`.
    fn hstrerror(&self, code: i32) -> Vec<u8>;
}

/// The library this program links.
struct Libc;

/// `localhost ()` and `localdomain ()`'s loop: 128 bytes to start with,
/// doubled while the library says the name does not fit -- by
/// `ENAMETOOLONG`, or by filling the buffer with no room for the NUL. A
/// buffer that cannot be had is `ENOMEM`, as upstream's `realloc` reports it.
fn grown(get: fn(&mut [u8]) -> Result<usize, i32>) -> Result<Vec<u8>, i32> {
    /// `ENOMEM`.
    const ENOMEM: i32 = 12;
    let mut len: usize = 128;
    loop {
        let mut buf = Vec::new();
        buf.try_reserve_exact(len).map_err(|_| ENOMEM)?;
        buf.resize(len, 0);
        match get(&mut buf) {
            Ok(n) if n < len => {
                buf.truncate(n);
                return Ok(buf);
            }
            Ok(_) | Err(libcall::ENAMETOOLONG) => {}
            Err(errno) => return Err(errno),
        }
        len = len.checked_mul(2).ok_or(ENOMEM)?;
    }
}

/// `bytes` as the C string a library call takes: up to its first NUL, which
/// none of the names handed over here can hold anyway.
fn c_string(bytes: &[u8]) -> CString {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    CString::new(bytes.get(..end).unwrap_or_default()).unwrap_or_default()
}

/// One entry of the library's list, as [`Interface`].
struct Entry<'a>(libcall::netdb::IfAddr<'a>);

impl Interface for Entry<'_> {
    fn flags(&self) -> u32 {
        self.0.flags()
    }
    fn family(&self) -> Option<i32> {
        self.0.family()
    }
    fn is_ipv6_link_local(&self) -> bool {
        self.0.is_ipv6_link_local()
    }
    fn name_info(&self, flags: i32) -> Result<Vec<u8>, i32> {
        self.0.name_info(flags).map(|t| t.as_bytes().to_vec())
    }
}

impl System for Libc {
    fn hostname(&self) -> Result<Vec<u8>, i32> {
        grown(libcall::hostname_into)
    }
    fn domainname(&self) -> Result<Vec<u8>, i32> {
        grown(libcall::domainname_into)
    }
    fn set_hostname(&self, name: &[u8]) -> Result<(), i32> {
        libcall::sethostname(name)
    }
    fn set_domainname(&self, name: &[u8]) -> Result<(), i32> {
        libcall::setdomainname(name)
    }
    fn lookup(&self, host: &[u8]) -> Result<Lookup, i32> {
        let list = libcall::netdb::AddrInfo::lookup(&c_string(host), SOCK_DGRAM, AI_CANONNAME)?;
        Ok(Lookup {
            canonical: list.canonical_name().map(<[u8]>::to_vec),
            numeric: list
                .names(NI_NUMERICHOST, INET6_ADDRSTRLEN)
                .map(|name| name.map(|t| t.as_bytes().to_vec()))
                .collect(),
        })
    }
    fn aliases(&self, host: &[u8]) -> Result<Vec<Vec<u8>>, i32> {
        let mut found = Vec::new();
        libcall::netdb::host_aliases(&c_string(host), |alias| found.push(alias.to_vec()))?;
        Ok(found)
    }
    fn interfaces(
        &self,
        visit: &mut dyn FnMut(&dyn Interface) -> ControlFlow<()>,
    ) -> Result<(), i32> {
        let list = libcall::netdb::IfAddrs::get()?;
        for entry in &list {
            if visit(&Entry(entry)).is_break() {
                break;
            }
        }
        Ok(())
    }
    fn gai_strerror(&self, code: i32) -> Vec<u8> {
        libcall::netdb::gai_strerror(code).to_bytes().to_vec()
    }
    fn hstrerror(&self, code: i32) -> Vec<u8> {
        libcall::netdb::hstrerror(code).to_bytes().to_vec()
    }
}

/// How `run` ends: the status upstream's `exit`, `err` or `errx` gives.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Exit(pub(crate) u8);

/// Which stream `usage` writes to.
#[derive(Clone, Copy)]
enum Stream {
    Out,
    Err,
}

/// The two streams, and the name the program was run by.
pub(crate) struct Io<'a> {
    pub(crate) out: &'a mut dyn Write,
    pub(crate) err: &'a mut dyn Write,
    /// `argv[0]` as given: the prefix getopt's messages carry.
    pub(crate) invocation: Vec<u8>,
}

impl Io<'_> {
    /// The last component of `argv[0]`: upstream's `progname`, and the
    /// `__progname` that `err` and `errx` print.
    fn short_name(&self) -> &[u8] {
        let s = &self.invocation;
        s.iter()
            .rposition(|&b| b == b'/')
            .map_or(s.as_slice(), |i| {
                s.get(i.saturating_add(1)..).unwrap_or_default()
            })
    }

    /// Standard output, unchecked as upstream's `printf`s are.
    fn put(&mut self, bytes: &[u8]) {
        // Ignored: upstream never looks at what `printf` returns, so a failed
        // write does not change what it exits with, and neither does ours.
        let _ = self.out.write_all(bytes);
    }

    /// `usage (stream)`: the text, then `exit (-1)`.
    fn usage(&mut self, stream: Stream) -> Exit {
        let to: &mut dyn Write = match stream {
            Stream::Out => &mut *self.out,
            Stream::Err => &mut *self.err,
        };
        // Ignored, as `fprintf`'s result is upstream.
        let _ = to.write_all(USAGE);
        Exit(255)
    }

    /// getopt's own complaint, `argv[0]: sentence`, before the usage.
    fn getopt_error(&mut self, sentence: &str) {
        let mut text = self.invocation.clone();
        text.extend_from_slice(b": ");
        text.extend_from_slice(sentence.as_bytes());
        text.push(b'\n');
        // Ignored: getopt does not check its `fprintf` either.
        let _ = self.err.write_all(&text);
    }

    /// `errx (1, "%s", message)`.
    fn errx(&mut self, message: &[u8]) -> Exit {
        let mut text = self.short_name().to_vec();
        text.extend_from_slice(b": ");
        text.extend_from_slice(message);
        text.push(b'\n');
        // Ignored, as `errx`'s own writes are: it is about to exit anyway.
        let _ = self.err.write_all(&text);
        Exit(1)
    }

    /// `err (1, NULL)`: the program's name and `strerror (errno)`.
    fn err(&mut self, e: &io::Error) -> Exit {
        self.errx(strerror(e).as_bytes())
    }

    /// [`Io::err`] for an `errno` the library returned.
    fn err_errno(&mut self, errno: i32) -> Exit {
        self.err(&io::Error::from_raw_os_error(errno))
    }

    /// `printf ("%s\n", text)`.
    fn line(&mut self, text: &[u8]) -> Exit {
        self.put(text);
        self.put(b"\n");
        Exit(0)
    }
}

/// glibc's `fgets (buf, buf.len (), stream)`; `false` where it returns NULL.
///
/// Up to `buf.len () - 1` bytes are read, stopping after a newline, and
/// terminated. At end of file with nothing read, or on a read error, the
/// answer is `false` and `buf` keeps what it held -- after an error, with the
/// bytes already copied over it, unterminated, as glibc's `_IO_getline`
/// leaves them. A one-byte buffer is given its terminator and nothing is
/// read, glibc's special case; a zero-byte one is refused.
fn fgets(stream: &mut impl BufRead, buf: &mut [u8]) -> bool {
    let room = match buf.len() {
        0 => return false,
        1 => {
            if let Some(b) = buf.first_mut() {
                *b = 0;
            }
            return true;
        }
        n => n.saturating_sub(1),
    };
    let mut count = 0usize;
    while count < room {
        let chunk = match stream.fill_buf() {
            Ok(chunk) => chunk,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) => return false,
        };
        if chunk.is_empty() {
            break;
        }
        let window = chunk
            .get(..room.saturating_sub(count).min(chunk.len()))
            .unwrap_or_default();
        let (take, ends_line) = match window.iter().position(|&b| b == b'\n') {
            Some(i) => (i.saturating_add(1), true),
            None => (window.len(), false),
        };
        let end = count.saturating_add(take);
        if let (Some(to), Some(from)) = (buf.get_mut(count..end), window.get(..take)) {
            to.copy_from_slice(from);
        }
        stream.consume(take);
        count = end;
        if ends_line {
            break;
        }
    }
    if count == 0 {
        return false;
    }
    if let Some(b) = buf.get_mut(count) {
        *b = 0;
    }
    true
}

/// `read_file (filename, boot)`: the first line of the file that does not
/// begin with a newline or `#`, without its newline. `Ok(None)` is the file
/// that could not be opened under `-b`; any other failure is `err (1, NULL)`'s.
///
/// Upstream's loop, with its edges: the buffer is the file's size plus one
/// (so `fgets` reads whole lines, and a file whose size says 0 -- empty, or a
/// `/proc` file -- gives the empty name); a file whose every line is skipped
/// leaves the last skipped line in the buffer, newline and all, and that is
/// the name; a directory opens, fails to read, and gives the empty name.
fn read_file(path: &OsString, boot: bool) -> io::Result<Option<Vec<u8>>> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(_) if boot => return Ok(None),
        Err(e) => return Err(e),
    };
    let size = file.metadata()?.len();
    // `calloc (st_size + 1, sizeof (char))`.
    let len = usize::try_from(size)
        .ok()
        .and_then(|s| s.checked_add(1))
        .ok_or_else(|| io::Error::from(io::ErrorKind::OutOfMemory))?;
    let mut buf = Vec::new();
    buf.try_reserve_exact(len)
        .map_err(|_| io::Error::from(io::ErrorKind::OutOfMemory))?;
    buf.resize(len, 0);
    // `fgets (buf, st_size + 1, fp)`: the size is an `int` in C, so a file
    // of 2 GiB or more wraps it; zero or less makes `fgets` refuse.
    #[allow(clippy::cast_possible_truncation)]
    let n = usize::try_from(size.wrapping_add(1) as i32)
        .unwrap_or(0)
        .min(len);
    let mut stream = BufReader::new(file);
    let mut cut = false;
    while let Some(window) = buf.get_mut(..n) {
        if !fgets(&mut stream, window) {
            break;
        }
        if matches!(buf.first(), Some(b'\n' | b'#')) {
            continue;
        }
        // `if ((p = strchr (buf, '\n')) != NULL) *p = '\0'; break;`
        cut = true;
        break;
    }
    let end = buf
        .iter()
        .position(|&b| b == 0 || (cut && b == b'\n'))
        .unwrap_or(buf.len());
    buf.truncate(end);
    Ok(Some(buf))
}

/// C's `isspace` in the "C" locale, which upstream never leaves.
fn is_c_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}

/// `check_name`: RFC 1035 section 2.3.1 -- a letter or digit first and last,
/// only letters, digits, hyphens and dots between, no hyphen beside a dot
/// and no two dots together. Letters and digits are ASCII, as the "C"
/// locale's `isalnum` has them.
fn check_name(name: &[u8]) -> bool {
    let (Some(first), Some(last)) = (name.first(), name.last()) else {
        return false;
    };
    if !first.is_ascii_alphanumeric() || !last.is_ascii_alphanumeric() {
        return false;
    }
    for (i, &c) in name.iter().enumerate() {
        if !c.is_ascii_alphanumeric() && c != b'-' && c != b'.' {
            return false;
        }
        let before = i.checked_sub(1).and_then(|j| name.get(j)).copied();
        let after = name.get(i.saturating_add(1)).copied();
        if c == b'-' && (before == Some(b'.') || after == Some(b'.')) {
            return false;
        }
        if c == b'.' && before == Some(b'.') {
            return false;
        }
    }
    true
}

/// `set_name (type, name)`.
fn set_name(sys: &dyn System, io: &mut Io<'_>, kind: Type, name: &[u8]) -> Exit {
    match kind {
        Type::Default => {
            // White space cannot be in a host name, so it is trimmed off both
            // ends first; what is left must pass `check_name`.
            let start = name
                .iter()
                .position(|&b| !is_c_space(b))
                .unwrap_or(name.len());
            let end = name
                .iter()
                .rposition(|&b| !is_c_space(b))
                .map_or(start, |i| i.saturating_add(1));
            let name = name.get(start..end).unwrap_or_default();
            if !check_name(name) {
                return io.errx(b"the specified hostname is invalid");
            }
            match sys.set_hostname(name) {
                Err(libcall::EPERM) => io.errx(b"you must be root to change the host name"),
                Err(libcall::EINVAL) => io.errx(b"name too long"),
                // Any other failure goes unreported and the status stays 0:
                // upstream tests `errno` for these two values and no others.
                _ => Exit(0),
            }
        }
        Type::Nis | Type::NisDef => match sys.set_domainname(name) {
            Err(libcall::EPERM) => io.errx(b"you must be root to change the domain name"),
            Err(libcall::EINVAL) => io.errx(b"name too long"),
            _ => Exit(0),
        },
        // Only the host name and the domain name can be set.
        _ => io.usage(Stream::Err),
    }
}

/// `-I` and `-A`: every address of every interface that is up, but the
/// loopback's and IPv6's link-local ones, each as `getnameinfo` gives it and
/// followed by a space, then a newline.
fn all_addresses(sys: &dyn System, io: &mut Io<'_>, kind: Type) -> Exit {
    let flags = if kind == Type::AllIps {
        NI_NUMERICHOST
    } else {
        NI_NAMEREQD
    };
    let mut failed: Option<i32> = None;
    let listed = sys.interfaces(&mut |entry| {
        // An entry with no configured address, the loopback, an interface
        // that is down, and an address that is neither IPv4 nor IPv6 are
        // skipped, as are IPv6 link-local addresses.
        let Some(family) = entry.family() else {
            return ControlFlow::Continue(());
        };
        if entry.flags() & IFF_LOOPBACK != 0 || entry.flags() & IFF_UP == 0 {
            return ControlFlow::Continue(());
        }
        if family != AF_INET && family != AF_INET6 {
            return ControlFlow::Continue(());
        }
        if family == AF_INET6 && entry.is_ipv6_link_local() {
            return ControlFlow::Continue(());
        }
        match entry.name_info(flags) {
            Ok(name) => {
                io.put(&name);
                io.put(b" ");
            }
            // An address with no name is skipped; for `-I` any other failure
            // ends the program, for `-A` none does.
            Err(code) if kind != Type::AllFqdns && code != EAI_NONAME => {
                failed = Some(code);
                return ControlFlow::Break(());
            }
            Err(_) => {}
        }
        ControlFlow::Continue(())
    });
    if let Err(errno) = listed {
        // `errx (1, "%s", strerror (errno))`.
        let message = strerror(&io::Error::from_raw_os_error(errno));
        return io.errx(message.as_bytes());
    }
    if let Some(code) = failed {
        return io.errx(&sys.gai_strerror(code));
    }
    io.put(b"\n");
    Exit(0)
}

/// `-d`, `-f`, `-a` and `-i`: each starts from the library's lookup of the
/// host name, which must succeed even for `-a`, which then asks
/// `gethostbyname` instead.
fn resolved(sys: &dyn System, io: &mut Io<'_>, kind: Type) -> Exit {
    let host = match sys.hostname() {
        Ok(host) => host,
        Err(errno) => return io.err_errno(errno),
    };
    let found = match sys.lookup(&host) {
        Ok(found) => found,
        Err(code) => return io.errx(&sys.gai_strerror(code)),
    };
    let canonical = found.canonical.unwrap_or_default();
    match kind {
        Type::Alias => {
            // `gethostbyname (localhost ())`: upstream asks for the name again.
            let host = match sys.hostname() {
                Ok(host) => host,
                Err(errno) => return io.err_errno(errno),
            };
            match sys.aliases(&host) {
                Ok(aliases) => io.line(&aliases.join(&b' ')),
                Err(h_errno) => io.errx(&sys.hstrerror(h_errno)),
            }
        }
        Type::Ip => {
            for (i, address) in found.numeric.iter().enumerate() {
                match address {
                    Ok(text) => {
                        if i > 0 {
                            io.put(b" ");
                        }
                        io.put(text);
                    }
                    Err(code) => return io.errx(&sys.gai_strerror(*code)),
                }
            }
            io.line(b"")
        }
        // Everything after the canonical name's first dot -- and nothing at
        // all, not even a newline, when it has none.
        Type::Dns => match canonical.iter().position(|&b| b == b'.') {
            Some(dot) => io.line(canonical.get(dot.saturating_add(1)..).unwrap_or_default()),
            None => Exit(0),
        },
        Type::Fqdn => io.line(&canonical),
        _ => Exit(0),
    }
}

/// `show_name (type)`.
fn show_name(sys: &dyn System, io: &mut Io<'_>, kind: Type) -> Exit {
    match kind {
        Type::Default => match sys.hostname() {
            Ok(name) => io.line(&name),
            Err(errno) => io.err_errno(errno),
        },
        Type::Short => match sys.hostname() {
            Ok(name) => {
                let end = name.iter().position(|&b| b == b'.').unwrap_or(name.len());
                io.line(name.get(..end).unwrap_or_default())
            }
            Err(errno) => io.err_errno(errno),
        },
        // `domainname` prints what the library says, `(none)` included.
        Type::Nis => match sys.domainname() {
            Ok(domain) => io.line(&domain),
            Err(errno) => io.err_errno(errno),
        },
        // `localnisdomain ()`: an unset domain is a failure, reported on
        // standard output.
        Type::NisDef => match sys.domainname() {
            Ok(domain) if domain != b"(none)" => io.line(&domain),
            _ => {
                let mut text = io.short_name().to_vec();
                text.extend_from_slice(b": Local domain name not set\n");
                io.put(&text);
                Exit(1)
            }
        },
        Type::AllIps | Type::AllFqdns => all_addresses(sys, io, kind),
        Type::Dns | Type::Fqdn | Type::Alias | Type::Ip => resolved(sys, io, kind),
    }
}

/// An argument's bytes. `as_encoded_bytes` is the bytes themselves on Unix,
/// where this program runs; elsewhere it is the platform's own encoding,
/// which only the host's tests ever see.
fn bytes(arg: &OsString) -> Vec<u8> {
    arg.as_encoded_bytes().to_vec()
}

/// `main`, after `argv[0]`.
pub(crate) fn run(args: &[OsString], sys: &dyn System, io: &mut Io<'_>) -> Exit {
    // If called as `dnsdomainname`, by default show the DNS domain name --
    // and so on for the other three, as upstream decides by `progname`.
    let progname = io.short_name();
    let mut kind = match progname {
        b"dnsdomainname" => Type::Dns,
        b"domainname" => Type::Nis,
        b"ypdomainname" | b"nisdomainname" => Type::NisDef,
        _ => Type::Default,
    };
    let mut file: Option<OsString> = None;
    let mut boot = false;
    let mut operands: Vec<&OsString> = Vec::new();

    for item in HOSTNAME.parse_aliased(args, SHORT_OPTIONS, LONG_OPTIONS, LONG_ALIASES) {
        let item = match item {
            Ok(item) => item,
            // getopt's complaint, then `case '?': usage (stdout)`.
            Err(e) => {
                io.getopt_error(&e.sentence);
                return io.usage(Stream::Out);
            }
        };
        match item {
            Opt::Operand(word) => operands.push(word),
            Opt::Short(b'd', _) | Opt::Long("domain", _) => kind = Type::Dns,
            Opt::Short(b'a', _) | Opt::Long("alias", _) => kind = Type::Alias,
            Opt::Short(b'f', _) | Opt::Long("fqdn" | "long", _) => kind = Type::Fqdn,
            Opt::Short(b'A', _) | Opt::Long("all-fqdns", _) => kind = Type::AllFqdns,
            Opt::Short(b'i', _) | Opt::Long("ip-address", _) => kind = Type::Ip,
            Opt::Short(b'I', _) | Opt::Long("all-ip-addresses", _) => kind = Type::AllIps,
            Opt::Short(b's', _) | Opt::Long("short", _) => kind = Type::Short,
            Opt::Short(b'y', _) | Opt::Long("nis" | "yp", _) => kind = Type::NisDef,
            Opt::Short(b'b', _) | Opt::Long("boot", _) => boot = true,
            Opt::Short(b'F', value) | Opt::Long("file", value) => file = value,
            Opt::Short(b'V', _) | Opt::Long("version", _) => {
                io.put(b"hostname 3.23\n");
                return Exit(0);
            }
            Opt::Short(b'h' | b'?', _) | Opt::Long("help", _) => return io.usage(Stream::Out),
            // `default: usage (stderr)`. Unreachable: every option in the
            // tables above has its case.
            Opt::Short(..) | Opt::Long(..) => return io.usage(Stream::Err),
        }
    }

    // A name may come from a file, which under `-b` may be missing or empty,
    // the current name -- or `localhost`, when there is none -- standing in.
    let mut name: Option<Vec<u8>> = None;
    if let Some(path) = &file {
        name = match read_file(path, boot) {
            Ok(name) => name,
            Err(e) => return io.err(&e),
        };
        if boot && name.as_ref().is_none_or(Vec::is_empty) {
            let mut current = match sys.hostname() {
                Ok(current) => current,
                Err(errno) => return io.err_errno(errno),
            };
            if current.is_empty() || current == b"(none)" {
                current = b"localhost".to_vec();
            }
            name = Some(current);
        }
    }

    // Otherwise the name is the operand -- an error beside a file, and more
    // than one is an error too.
    let mut rest = operands.into_iter();
    if let Some(first) = rest.next() {
        if name.is_some() {
            return io.usage(Stream::Err);
        }
        name = Some(bytes(first));
    }
    if rest.next().is_some() {
        return io.usage(Stream::Err);
    }

    match name {
        // `name` is a C string from here on: up to its first NUL.
        Some(name) => {
            let end = name.iter().position(|&b| b == 0).unwrap_or(name.len());
            set_name(sys, io, kind, name.get(..end).unwrap_or_default())
        }
        None => show_name(sys, io, kind),
    }
}

fn main() -> ExitCode {
    // argv[0] as given, and the arguments after it.
    let invocation = std::env::args_os()
        .next()
        .map_or_else(|| b"hostname".to_vec(), |a| bytes(&a));
    let rest: Vec<OsString> = std::env::args_os().skip(1).collect();
    let stdout = io::stdout();
    let mut out = io::BufWriter::new(stdout.lock());
    let stderr = io::stderr();
    let mut err = stderr.lock();
    let mut io = Io {
        out: &mut out,
        err: &mut err,
        invocation,
    };
    let Exit(status) = run(&rest, &Libc, &mut io);
    // `exit`'s flush of standard output, whose failure upstream never sees.
    let _ = io.out.flush();
    ExitCode::from(status)
}
