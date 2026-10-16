//! The issue file: what is shown above the login prompt -- `eval_issue_file`
//! reading `/etc/issue` and its `issue.d` (or the files `-f` names), and
//! `output_special_char` expanding the escapes in them.
//!
//! | escape | what |
//! |---|---|
//! | `\s` `\n` `\r` `\v` `\m` | `uname`'s system, node, release, version, machine |
//! | `\o` | the NIS domain, else `unknown_domain` |
//! | `\O` | the DNS domain, from the host name's canonical name |
//! | `\d` `\t` | the date (`Thu Oct  9  2026`, two spaces before the year) and the time |
//! | `\l` | the line |
//! | `\b` | the line's speed |
//! | `\S`, `\S{VAR}` | `PRETTY_NAME`, or `VAR`, from `os-release` (`ANSI_COLOR` as an escape) |
//! | `\u` `\U` | the users logged in: `2 `, `2 users` |
//! | `\4`, `\6`, `\4{iface}` | an IPv4 or IPv6 address of the best interface, or of `iface` |
//! | `\e`, `\e{name}` | an escape, or the escape for a colour name |
//!
//! Any other character after a backslash is that character, and a backslash
//! that ends the file is byte 255 -- `getc`'s `EOF` cut to an `unsigned
//! char`, as upstream passes it on.

use std::path::{Path, PathBuf};

use libcall::clock::Local;

use crate::Diag;
use crate::options::{F_ISSUE, Options};

/// `UL_COLORNAME_MAXSZ`: `\e{...}`'s buffer.
const COLORNAME_MAXSZ: usize = 32;
/// `\S{...}`'s buffer.
const VARNAME_MAXSZ: usize = 64;
/// `\4{...}`'s buffer.
const IFACE_MAXSZ: usize = 128;
/// `ISSUEDIR_EXT`.
const ISSUEDIR_EXT: &[u8] = b".issue";
/// The largest `os-release` read.
const OS_RELEASE_MAX: u64 = 4 * 1024 * 1024;

/// `uname`'s fields, as the escapes use them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Uts {
    /// `sysname`.
    pub sysname: Vec<u8>,
    /// `nodename`.
    pub nodename: Vec<u8>,
    /// `release`.
    pub release: Vec<u8>,
    /// `version`.
    pub version: Vec<u8>,
    /// `machine`.
    pub machine: Vec<u8>,
}

/// One entry of `getifaddrs` that has a name and an address.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Iface {
    /// `ifa_name`.
    pub name: Vec<u8>,
    /// `ifa_flags`.
    pub flags: u32,
    /// `ifa_addr`'s family.
    pub family: i32,
    /// The address, as `inet_ntop` takes it: 4 bytes or 16.
    pub addr: Vec<u8>,
}

/// What the escapes ask of the system: the C library, or a test's fixture.
pub trait Host {
    /// `uname (&uts)`.
    fn uname(&self) -> Uts;
    /// `xgethostname ()`.
    fn hostname(&self) -> Option<Vec<u8>>;
    /// `xgetdomainname ()`.
    fn domainname(&self) -> Option<Vec<u8>>;
    /// `getaddrinfo (host, NULL, {AI_CANONNAME})`'s `ai_canonname`.
    fn canonical_name(&self, host: &[u8]) -> Option<Vec<u8>>;
    /// `time` and `localtime_r`: now, in the local zone.
    fn local_now(&self) -> Option<Local>;
    /// `nl_langinfo (item)`.
    fn langinfo(&self, item: i32) -> Vec<u8>;
    /// How many `USER_PROCESS` records `utmp` holds.
    fn users(&self) -> usize;
    /// `getifaddrs`, `None` when it fails.
    fn interfaces(&self) -> Option<Vec<Iface>>;
    /// `inet_ntop (family, addr, ...)`.
    fn ntop(&self, family: i32, addr: &[u8]) -> Vec<u8>;
    /// The first address `getaddrinfo (host, NULL, {family, flags})` gives.
    fn host_address(&self, host: &[u8], family: i32, flags: i32) -> Option<Vec<u8>>;
}

/// Where the issue files and `os-release` are looked for: under `/` for the
/// program, under a scratch directory in the tests.
#[derive(Clone, Debug)]
pub struct Paths {
    root: PathBuf,
}

impl Paths {
    /// The paths under `root`.
    #[must_use]
    pub fn under(root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
        }
    }

    /// `path`, absolute, under the root.
    fn at(&self, path: &str) -> PathBuf {
        self.root.join(path.trim_start_matches('/'))
    }
}

impl Default for Paths {
    fn default() -> Self {
        Self::under(Path::new("/"))
    }
}

/// `struct issue`: the issue as last evaluated, and what printing it must do
/// to the line.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Issue {
    /// The memory stream's contents: `None` until a regular file was read.
    pub mem: Option<Vec<u8>>,
    /// `do_tcsetattr`: an `issue.d` held files, so the line's output must
    /// map a new line to carriage return and new line while it is printed --
    /// and keeps doing so after: upstream's `do_tcrestore`, which would put
    /// the modes back, is set by nothing.
    pub do_tcsetattr: bool,
    /// `op->osrelease`: `os-release`, once a read of it succeeded -- read
    /// once a process, as upstream keeps it.
    pub osrelease: Option<Vec<u8>>,
    /// `mem_old`: the issue as last printed, to tell whether a reload
    /// changed it.
    pub mem_old: Option<Vec<u8>>,
    /// `netlink_groups`: the address changes the last evaluation's `\4` and
    /// `\6` showed addresses for, to be listened for.
    pub netlink_groups: u32,
}

/// `issue_is_changed (ie)`: whether the issue evaluated last differs from
/// the one printed last, compared as C strings. When it does not, the new
/// one is taken as printed. No issue at all is always a change.
pub fn issue_is_changed(ie: &mut Issue) -> bool {
    let c_str = |m: &[u8]| m.split(|&b| b == 0).next().unwrap_or_default().to_vec();
    match (&ie.mem_old, &ie.mem) {
        (Some(old), Some(new)) if c_str(old) == c_str(new) => {
            ie.mem_old = ie.mem.take();
            false
        }
        _ => true,
    }
}

/// One evaluation of the issue file: the options, the line's input speed,
/// where to look, the system's answers, and where warnings go.
pub struct Eval<'a> {
    op: &'a Options,
    speed: u32,
    paths: &'a Paths,
    host: &'a dyn Host,
    diag: &'a mut dyn Diag,
    /// `ie->output`: whether this evaluation opened the memory stream.
    output_open: bool,
}

impl<'a> Eval<'a> {
    /// An evaluation for `op`, of a line whose `cfgetispeed` is `speed`.
    pub fn new(
        op: &'a Options,
        speed: u32,
        paths: &'a Paths,
        host: &'a dyn Host,
        diag: &'a mut dyn Diag,
    ) -> Self {
        Self {
            op,
            speed,
            paths,
            host,
            diag,
            output_open: false,
        }
    }

    /// `eval_issue_file (ie, op, tp)`: the issue, into `ie.mem`.
    pub fn eval(&mut self, ie: &mut Issue) {
        self.output_open = false;
        ie.netlink_groups = 0;
        if self.op.flags & F_ISSUE == 0 {
            return;
        }
        // "The custom issue file or directory list specified by -f. Note
        // that nothing is printed if the file/dir does not exist."
        if let Some(list) = self.op.issue.clone() {
            // `strtok (list, ":")`: empty fields are skipped.
            for file in list.split(|&b| b == b':').filter(|f| !f.is_empty()) {
                let path = PathBuf::from(quoting::os_from_bytes(file));
                let Ok(meta) = std::fs::metadata(&path) else {
                    continue;
                };
                if meta.is_dir() {
                    self.issuedir_read(ie, &path);
                } else {
                    self.issuefile_read(ie, &path);
                }
            }
            return;
        }
        // "The default /etc/issue and optional /etc/issue.d directory as
        // extension to the file. The /etc/issue.d directory is ignored if
        // there is no /etc/issue file." -- `access (F_OK|R_OK)`, which an
        // open for reading answers.
        let issue = self.paths.at("/etc/issue");
        if std::fs::File::open(&issue).is_ok() {
            self.issuefile_read(ie, &issue);
            self.issuedir_read(ie, &self.paths.at("/etc/issue.d"));
            return;
        }
        // "Fallback @runstatedir (usually /run) -- the file is not required
        // to read the dir."
        let file = self.issuefile_read(ie, &self.paths.at("/run/issue"));
        let dir = self.issuedir_read(ie, &self.paths.at("/run/issue.d"));
        if file || dir {
            return;
        }
        // "Fallback @sysconfstaticdir (usually /usr/lib)".
        self.issuefile_read(ie, &self.paths.at("/usr/lib/issue"));
        self.issuedir_read(ie, &self.paths.at("/usr/lib/issue.d"));
    }

    /// `issuefile_read (ie, filename, op, tp)`: whether the file opened and
    /// was a regular one (upstream's 0).
    fn issuefile_read(&mut self, ie: &mut Issue, path: &Path) -> bool {
        let Ok(mut f) = std::fs::File::open(path) else {
            return false;
        };
        self.issuefile_read_stream(ie, &mut f)
    }

    /// `issuefile_read_stream (ie, f, op, tp)`: a regular file's bytes, its
    /// escapes expanded, onto the memory stream -- which the first file of
    /// an evaluation starts afresh.
    fn issuefile_read_stream(&mut self, ie: &mut Issue, f: &mut std::fs::File) -> bool {
        use std::io::Read;
        if !f.metadata().is_ok_and(|m| m.is_file()) {
            return false;
        }
        if !self.output_open {
            ie.mem = Some(Vec::new());
            self.output_open = true;
        }
        let mut data = Vec::new();
        // A read that fails part way is upstream's `getc` meeting `EOF`:
        // what was read is kept, and the rest is not there.
        let _ = f.read_to_end(&mut data);
        let mut out = ie.mem.take().unwrap_or_default();
        let mut r = Reader {
            data: &data,
            pos: 0,
        };
        while let Some(c) = r.getc() {
            if c == b'\\' {
                // `getc (f)` past the end is `EOF`, which reaches the switch
                // as `(unsigned char) -1`.
                let next = r.getc().unwrap_or(0xff);
                self.output_special_char(ie, &mut out, next, &mut r);
            } else {
                out.push(c);
            }
        }
        ie.mem = Some(out);
        true
    }

    /// `issuedir_read (ie, dirname, op, tp)`: every `*.issue` in the
    /// directory, in `versionsort` order. Whether the directory opened
    /// (upstream's 0).
    fn issuedir_read(&mut self, ie: &mut Issue, dir: &Path) -> bool {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return false;
        };
        let mut names: Vec<Vec<u8>> = entries
            .flatten()
            // `issuedir_filter`: a regular file or a symbolic link, named
            // `*.issue` and not hidden.
            .filter(|e| e.file_type().is_ok_and(|t| t.is_file() || t.is_symlink()))
            .map(|e| quoting::os_bytes(&e.file_name()).into_owned())
            .filter(|n| {
                n.first() != Some(&b'.')
                    && n.len() > ISSUEDIR_EXT.len()
                    && n.ends_with(ISSUEDIR_EXT)
            })
            .collect();
        if names.is_empty() {
            return true;
        }
        names.sort_by(|a, b| ulstrutils::strverscmp(a, b));
        ie.do_tcsetattr = true;
        for name in names {
            if let Ok(mut f) = std::fs::File::open(dir.join(quoting::os_from_bytes(&name))) {
                self.issuefile_read_stream(ie, &mut f);
            }
        }
        true
    }

    /// `output_special_char (ie, c, op, tp, fp)`: the escape `\c`, onto
    /// `out`; `r` is the file, for an escape's `{argument}`.
    fn output_special_char(
        &mut self,
        ie: &mut Issue,
        out: &mut Vec<u8>,
        c: u8,
        r: &mut Reader<'_>,
    ) {
        match c {
            b'e' => match r.argument(COLORNAME_MAXSZ) {
                Some(name) => out.extend_from_slice(&ulcolors::get_sequence(&name)),
                None => out.push(0x1b),
            },
            b's' => out.extend_from_slice(&self.host.uname().sysname),
            b'n' => out.extend_from_slice(&self.host.uname().nodename),
            b'r' => out.extend_from_slice(&self.host.uname().release),
            b'v' => out.extend_from_slice(&self.host.uname().version),
            b'm' => out.extend_from_slice(&self.host.uname().machine),
            b'o' => {
                let dom = self.host.domainname();
                out.extend_from_slice(dom.as_deref().unwrap_or(b"unknown_domain"));
            }
            b'O' => {
                // The DNS domain: what follows the first dot of the host
                // name's canonical name.
                let dom = self
                    .host
                    .hostname()
                    .and_then(|h| self.host.canonical_name(&h))
                    .and_then(|canon| {
                        let dot = canon.iter().position(|&b| b == b'.')?;
                        canon.get(dot.saturating_add(1)..).map(<[u8]>::to_vec)
                    });
                out.extend_from_slice(dom.as_deref().unwrap_or(b"unknown_domain"));
            }
            b'd' | b't' => {
                // Upstream prints whatever `localtime_r` left, failed or
                // not; a failure here is the zeroed time.
                let tm = self.host.local_now().unwrap_or_default();
                if c == b'd' {
                    let day = self
                        .host
                        .langinfo(libcall::locale::ABDAY_1.saturating_add(tm.wday));
                    let mon = self
                        .host
                        .langinfo(libcall::locale::ABMON_1.saturating_add(tm.mon));
                    let year = if tm.year < 70 {
                        tm.year.saturating_add(2000)
                    } else {
                        tm.year.saturating_add(1900)
                    };
                    out.extend_from_slice(&day);
                    out.push(b' ');
                    out.extend_from_slice(&mon);
                    out.extend_from_slice(format!(" {:2}  {year}", tm.mday).as_bytes());
                } else {
                    out.extend_from_slice(
                        format!("{:02}:{:02}:{:02}", tm.hour, tm.min, tm.sec).as_bytes(),
                    );
                }
            }
            b'l' => out.extend_from_slice(&self.op.tty),
            b'b' => {
                if let Some(bps) = crate::speeds::bps_of(self.speed) {
                    out.extend_from_slice(bps.to_string().as_bytes());
                }
            }
            b'S' => match r.argument(VARNAME_MAXSZ) {
                // `\S{VAR}`: the variable, or nothing.
                Some(var) => {
                    if let Some(value) = self.read_os_release(ie, &var) {
                        if var == b"ANSI_COLOR" {
                            out.extend_from_slice(b"\x1b[");
                            out.extend_from_slice(&value);
                            out.push(b'm');
                        } else {
                            out.extend_from_slice(&value);
                        }
                    }
                }
                // `\S`: the pretty name, or the system's.
                None => match self.read_os_release(ie, b"PRETTY_NAME") {
                    Some(name) => out.extend_from_slice(&name),
                    None => out.extend_from_slice(&self.host.uname().sysname),
                },
            },
            b'u' => out.extend_from_slice(format!("{} ", self.host.users()).as_bytes()),
            b'U' => {
                let users = self.host.users();
                let noun = if users == 1 { "user" } else { "users" };
                out.extend_from_slice(format!("{users} {noun}").as_bytes());
            }
            b'4' | b'6' => {
                let family = if c == b'4' {
                    libcall::netdb::AF_INET
                } else {
                    libcall::netdb::AF_INET6
                };
                // A failed `getifaddrs` ends the escape before its argument
                // is read: `{iface}` is then printed as it stands.
                let Some(addrs) = self.host.interfaces() else {
                    return;
                };
                let iface = r.argument(IFACE_MAXSZ);
                self.output_iface_ip(out, &addrs, iface.as_deref(), family);
                // The address shown may change: listen for it to.
                ie.netlink_groups |= if c == b'4' {
                    libcall::netlink::RTMGRP_IPV4_IFADDR
                } else {
                    libcall::netlink::RTMGRP_IPV6_IFADDR
                };
            }
            other => out.push(other),
        }
    }

    /// `output_iface_ip (ie, addrs, iface, family)`: the address of
    /// `iface`, or of the first interface that is up, running and not a
    /// loopback -- and, failing that, of the host name.
    fn output_iface_ip(
        &self,
        out: &mut Vec<u8>,
        addrs: &[Iface],
        iface: Option<&[u8]>,
        family: i32,
    ) {
        use libcall::netdb::{AF_INET6, AI_V4MAPPED, IFF_LOOPBACK, IFF_RUNNING, IFF_UP};
        let best = addrs
            .iter()
            .filter(|p| p.family == family)
            .find(|p| match iface {
                Some(name) => p.name == name,
                None => {
                    p.flags & IFF_LOOPBACK == 0
                        && p.flags & IFF_UP != 0
                        && p.flags & IFF_RUNNING != 0
                }
            });
        if let Some(p) = best {
            out.extend_from_slice(&self.host.ntop(family, &p.addr));
            return;
        }
        if iface.is_some() {
            return;
        }
        // "Hmm.. not found the best interface, print host IP at least".
        let flags = if family == AF_INET6 { AI_V4MAPPED } else { 0 };
        if let Some(addr) = self
            .host
            .hostname()
            .and_then(|h| self.host.host_address(&h, family, flags))
        {
            out.extend_from_slice(&self.host.ntop(family, &addr));
        }
    }

    /// `read_os_release (op, varname)`: the variable's value in
    /// `/etc/os-release`, or in `/usr/lib/os-release` when that does not
    /// open. A file that opens neither way is warned of every time it is
    /// asked for; one too big or unreadable is quietly nothing.
    fn read_os_release(&mut self, ie: &mut Issue, varname: &[u8]) -> Option<Vec<u8>> {
        if ie.osrelease.is_none() {
            let file = std::fs::File::open(self.paths.at("/etc/os-release"))
                .or_else(|_| std::fs::File::open(self.paths.at("/usr/lib/os-release")));
            let Ok(mut file) = file else {
                self.diag.log_warn(b"cannot open os-release file");
                return None;
            };
            let size = file.metadata().ok()?.len();
            if size > OS_RELEASE_MAX {
                return None;
            }
            let mut data = Vec::new();
            std::io::Read::read_to_end(&mut file, &mut data).ok()?;
            // `read_all` of `st_size` bytes: a file that changed size
            // between the two is not read.
            if u64::try_from(data.len()).ok()? != size {
                return None;
            }
            ie.osrelease = Some(data);
        }
        os_release_value(ie.osrelease.as_deref()?, varname)
    }
}

/// The value of `varname` in `os-release` text, by upstream's own scan: the
/// last line that starts with the name (blanks, even new lines, allowed
/// around the `=`), opening blanks and quotes taken off, and a closing quote
/// with the blanks after it.
fn os_release_value(data: &[u8], varname: &[u8]) -> Option<Vec<u8>> {
    // An empty name matches at every position, and a line that does not
    // open with `=` is then retried where it stands, forever: upstream hangs
    // on `\S{}`. Here it names nothing (lib.rs, deliberate differences).
    if varname.is_empty() {
        return None;
    }
    // The buffer is a C string: a NUL in the file ends it.
    let buf = data.split(|&b| b == 0).next().unwrap_or_default();
    let tail = |p: usize| buf.get(p..).unwrap_or_default();
    let spn = |p: usize, set: &[u8]| tail(p).iter().take_while(|b| set.contains(b)).count();
    let cspn = |p: usize, set: &[u8]| tail(p).iter().take_while(|b| !set.contains(b)).count();
    let at = |p: usize| buf.get(p).copied().unwrap_or(0);
    let mut ret = None;
    let mut p = 0usize;
    loop {
        p = p.saturating_add(spn(p, b"\n\r"));
        p = p.saturating_add(spn(p, b" \t\n\r"));
        if at(p) == 0 {
            break;
        }
        if spn(p, b"#;\n") != 0 || !tail(p).starts_with(varname) {
            p = p.saturating_add(cspn(p, b"\n\r"));
            continue;
        }
        p = p.saturating_add(varname.len());
        p = p.saturating_add(spn(p, b" \t\n\r"));
        if at(p) != b'=' {
            continue;
        }
        p = p.saturating_add(spn(p, b" \t\n\r=\""));
        let eol = p.saturating_add(cspn(p, b"\n\r"));
        // Back from the line's end over blanks: a quote there ends the
        // value; anything else leaves the line whole, blanks and all.
        let mut end = eol;
        let mut eon = eol.saturating_sub(1);
        while eon > p {
            match at(eon) {
                b'\t' | b' ' => eon = eon.saturating_sub(1),
                b'"' => {
                    end = eon;
                    break;
                }
                _ => break,
            }
        }
        ret = Some(buf.get(p..end).unwrap_or_default().to_vec());
        // Upstream steps past the line's NUL -- past its copy's end when
        // the value ended the file, and reads on from there; here the scan
        // ends.
        if eol >= buf.len() {
            break;
        }
        p = eol.saturating_add(1);
    }
    ret
}

/// A file read a byte at a time, as `getc` reads it.
struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl Reader<'_> {
    /// `getc`: the next byte, `None` at the end.
    fn getc(&mut self) -> Option<u8> {
        let c = self.data.get(self.pos).copied()?;
        self.pos = self.pos.saturating_add(1);
        Some(c)
    }

    /// `get_escape_argument (fd, buf, bufsz)`: `{argument}` after an escape
    /// -- at most `bufsz - 1` bytes of it kept, the rest read and dropped.
    /// `None`, and nothing read, when the next byte is no `{`; `None`, and
    /// the rest of the file read, when it ends before the `}`.
    fn argument(&mut self, bufsz: usize) -> Option<Vec<u8>> {
        if self.data.get(self.pos) != Some(&b'{') {
            return None;
        }
        self.pos = self.pos.saturating_add(1);
        let mut buf = Vec::new();
        loop {
            let c = self.getc()?;
            if c == b'}' {
                return Some(buf);
            }
            if buf.len() < bufsz.saturating_sub(1) {
                buf.push(c);
            }
        }
    }
}

/// The real system, through the C library.
pub struct SystemHost;

impl Host for SystemHost {
    fn uname(&self) -> Uts {
        // A failed `uname` leaves upstream's buffer as the stack had it;
        // here, empty.
        libcall::uts::uname()
            .map(|u| Uts {
                sysname: u.sysname().to_vec(),
                nodename: u.nodename().to_vec(),
                release: u.release().to_vec(),
                version: u.version().to_vec(),
                machine: u.machine().to_vec(),
            })
            .unwrap_or_default()
    }

    fn hostname(&self) -> Option<Vec<u8>> {
        let mut buf = [0u8; libcall::XGETHOSTNAME_BUF];
        libcall::xgethostname(&mut buf).map(<[u8]>::to_vec)
    }

    fn domainname(&self) -> Option<Vec<u8>> {
        // glibc's `getdomainname` is `uname`'s domain field, which Linux
        // sets to `(none)` until someone names one.
        libcall::uts::uname().ok().map(|u| u.domainname().to_vec())
    }

    fn canonical_name(&self, host: &[u8]) -> Option<Vec<u8>> {
        let host = std::ffi::CString::new(host).ok()?;
        let info = libcall::netdb::AddrInfo::lookup(&host, 0, libcall::netdb::AI_CANONNAME).ok()?;
        info.canonical_name().map(<[u8]>::to_vec)
    }

    fn local_now(&self) -> Option<Local> {
        libcall::clock::local_now().ok()
    }

    fn langinfo(&self, item: i32) -> Vec<u8> {
        let mut buf = [0u8; 256];
        let n = libcall::locale::langinfo(item, &mut buf).unwrap_or(0);
        buf.get(..n).unwrap_or_default().to_vec()
    }

    fn users(&self) -> usize {
        // `getutxent` over glibc's default file.
        std::fs::read(quoting::os_from_bytes(libcall::utmp::UTMP_FILE.to_bytes()))
            .map_or(0, |data| utmp_users(&data))
    }

    fn interfaces(&self) -> Option<Vec<Iface>> {
        let list = libcall::netdb::IfAddrs::get().ok()?;
        Some(
            list.iter()
                .filter_map(|e| {
                    Some(Iface {
                        name: e.name()?.to_vec(),
                        flags: e.flags(),
                        family: e.family()?,
                        addr: e.address()?.to_vec(),
                    })
                })
                .collect(),
        )
    }

    fn ntop(&self, family: i32, addr: &[u8]) -> Vec<u8> {
        // `INET6_ADDRSTRLEN + 1`.
        let mut buf = [0u8; 47];
        let n = libcall::netdb::inet_ntop(family, addr, &mut buf).unwrap_or(0);
        buf.get(..n).unwrap_or_default().to_vec()
    }

    fn host_address(&self, host: &[u8], family: i32, flags: i32) -> Option<Vec<u8>> {
        let host = std::ffi::CString::new(host).ok()?;
        let info = libcall::netdb::AddrInfo::lookup_family(&host, family, 0, flags).ok()?;
        let (got, addr) = info.first_address()?;
        // Upstream prints the first answer as `family` whatever family it
        // is; asked for one family, `getaddrinfo` answers in it.
        (got == family).then(|| addr.to_vec())
    }
}

/// The `USER_PROCESS` records in `utmp` data.
fn utmp_users(data: &[u8]) -> usize {
    utmpfile::parse(data)
        .iter()
        .filter(|r| r.is_user_session())
        .count()
}

/// `show_issue (op)`'s evaluation: the issue file for a line whose
/// `cfgetispeed` is `speed`.
pub fn show_issue(
    op: &Options,
    speed: u32,
    paths: &Paths,
    host: &dyn Host,
    diag: &mut dyn Diag,
) -> Vec<u8> {
    let mut ie = Issue::default();
    Eval::new(op, speed, paths, host, diag).eval(&mut ie);
    ie.mem.unwrap_or_default()
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]
mod tests {
    use super::*;
    use crate::tests::Said;
    use libcall::locale::{ABDAY_1, ABMON_1};

    /// A system whose every answer is fixed.
    struct Fixed;

    impl Host for Fixed {
        fn uname(&self) -> Uts {
            Uts {
                sysname: b"Linux".to_vec(),
                nodename: b"box".to_vec(),
                release: b"6.6.0".to_vec(),
                version: b"#1 SMP".to_vec(),
                machine: b"x86_64".to_vec(),
            }
        }
        fn hostname(&self) -> Option<Vec<u8>> {
            Some(b"box".to_vec())
        }
        fn domainname(&self) -> Option<Vec<u8>> {
            Some(b"(none)".to_vec())
        }
        fn canonical_name(&self, _host: &[u8]) -> Option<Vec<u8>> {
            Some(b"box.example.org".to_vec())
        }
        fn local_now(&self) -> Option<Local> {
            Some(Local {
                sec: 5,
                min: 4,
                hour: 3,
                mday: 9,
                mon: 9,
                year: 126,
                wday: 4,
            })
        }
        fn langinfo(&self, item: i32) -> Vec<u8> {
            if item == ABDAY_1 + 4 {
                b"Thu".to_vec()
            } else if item == ABMON_1 + 9 {
                b"Oct".to_vec()
            } else {
                b"?".to_vec()
            }
        }
        fn users(&self) -> usize {
            2
        }
        fn interfaces(&self) -> Option<Vec<Iface>> {
            use libcall::netdb::{AF_INET, IFF_LOOPBACK, IFF_RUNNING, IFF_UP};
            let iface = |name: &[u8], flags, last| Iface {
                name: name.to_vec(),
                flags,
                family: AF_INET,
                addr: vec![10, 0, 0, last],
            };
            Some(vec![
                iface(b"lo", IFF_UP | IFF_RUNNING | IFF_LOOPBACK, 1),
                iface(b"eth0", IFF_UP, 2),
                iface(b"eth1", IFF_RUNNING, 3),
                iface(b"eth2", IFF_UP | IFF_RUNNING, 4),
            ])
        }
        fn ntop(&self, _family: i32, addr: &[u8]) -> Vec<u8> {
            let parts: Vec<String> = addr.iter().map(u8::to_string).collect();
            parts.join(".").into_bytes()
        }
        fn host_address(&self, _host: &[u8], _family: i32, flags: i32) -> Option<Vec<u8>> {
            Some(vec![192, 0, 2, u8::try_from(flags).unwrap()])
        }
    }

    /// `text` evaluated as `/etc/issue`, for line `tty1` at 9600 baud. (Not
    /// through `-f`: its list is split at `:`, which a Windows host's
    /// scratch path holds.)
    fn expand(text: &[u8]) -> Vec<u8> {
        let scratch = scratchdir::ScratchDir::new("agetty-issue");
        let root = scratch.path("root");
        std::fs::create_dir_all(root.join("etc")).unwrap();
        std::fs::write(root.join("etc/issue"), text).unwrap();
        let paths = Paths::under(&root);
        show_issue(
            &Options::default(),
            0o15,
            &paths,
            &Fixed,
            &mut Said::default(),
        )
    }

    #[test]
    fn the_files_f_names_are_a_colon_separated_list() {
        let scratch = scratchdir::ScratchDir::new("agetty-issue-f");
        let dir = scratch.path("");
        std::fs::write(dir.join("one"), b"1\n").unwrap();
        std::fs::write(dir.join("two"), b"2\n").unwrap();
        // Relative names, since a Windows host's absolute ones hold a `:`.
        let cwd = std::env::current_dir().unwrap();
        let rel = |name: &str| {
            let path = dir.join(name);
            let path = path
                .strip_prefix(&cwd)
                .map(Path::to_path_buf)
                .unwrap_or(path);
            quoting::os_bytes(path.as_os_str()).into_owned()
        };
        if rel("one").contains(&b':') {
            // The scratch directory is on another drive than the tests run
            // from: no name for it without a colon.
            return;
        }
        let list = [
            rel("one"),
            b"::".to_vec(),
            rel("nope"),
            b":".to_vec(),
            rel("two"),
        ]
        .concat();
        let op = Options {
            issue: Some(list),
            ..Options::default()
        };
        let text = show_issue(&op, 0, &Paths::default(), &Fixed, &mut Said::default());
        assert_eq!(text, b"1\n2\n");
    }

    #[test]
    fn the_uname_escapes_the_line_and_its_speed() {
        assert_eq!(
            expand(b"\\s \\n \\r \\v \\m on \\l at \\b\n"),
            b"Linux box 6.6.0 #1 SMP x86_64 on tty1 at 9600\n"
        );
    }

    #[test]
    fn the_domains_and_the_users() {
        assert_eq!(expand(b"\\o|\\O|\\u|\\U"), b"(none)|example.org|2 |2 users");
    }

    #[test]
    fn the_date_has_two_spaces_before_the_year_and_the_time_two_digits() {
        assert_eq!(expand(b"\\d \\t"), b"Thu Oct  9  2026 03:04:05");
    }

    #[test]
    fn an_address_is_the_best_interface_s_or_the_one_named() {
        assert_eq!(expand(b"\\4"), b"10.0.0.4");
        assert_eq!(expand(b"\\4{eth0}"), b"10.0.0.2");
        assert_eq!(expand(b"\\4{nope}"), b"");
        // No IPv6 interface: the host's address, asked for mapped ones.
        assert_eq!(expand(b"\\6"), b"192.0.2.8");
    }

    /// The groups of address changes an evaluation asks to hear.
    fn groups_for(text: &[u8]) -> u32 {
        let scratch = scratchdir::ScratchDir::new("agetty-issue-groups");
        let root = scratch.path("root");
        std::fs::create_dir_all(root.join("etc")).unwrap();
        std::fs::write(root.join("etc/issue"), text).unwrap();
        let paths = Paths::under(&root);
        let op = Options::default();
        let mut ie = Issue {
            netlink_groups: 0xffff,
            ..Issue::default()
        };
        Eval::new(&op, 0, &paths, &Fixed, &mut Said::default()).eval(&mut ie);
        ie.netlink_groups
    }

    #[test]
    fn an_address_shown_is_listened_for() {
        use libcall::netlink::{RTMGRP_IPV4_IFADDR, RTMGRP_IPV6_IFADDR};
        assert_eq!(groups_for(b"plain"), 0);
        assert_eq!(groups_for(b"\\4"), RTMGRP_IPV4_IFADDR);
        assert_eq!(groups_for(b"\\6{eth0}"), RTMGRP_IPV6_IFADDR);
        assert_eq!(
            groups_for(b"\\4 \\6"),
            RTMGRP_IPV4_IFADDR | RTMGRP_IPV6_IFADDR
        );
    }

    #[test]
    fn an_issue_is_changed_unless_it_reads_as_the_last_printed() {
        let mut ie = Issue {
            mem_old: Some(b"one\n".to_vec()),
            mem: Some(b"one\n".to_vec()),
            ..Issue::default()
        };
        assert!(!issue_is_changed(&mut ie));
        // The new one is taken as printed.
        assert_eq!(
            (ie.mem.as_deref(), ie.mem_old.as_deref()),
            (None, Some(&b"one\n"[..]))
        );
        let mut ie = Issue {
            mem_old: Some(b"one\n".to_vec()),
            mem: Some(b"two\n".to_vec()),
            ..Issue::default()
        };
        assert!(issue_is_changed(&mut ie));
        assert_eq!(ie.mem.as_deref(), Some(&b"two\n"[..]));
        // As C strings: what follows a NUL is not compared.
        let mut ie = Issue {
            mem_old: Some(b"one\0x".to_vec()),
            mem: Some(b"one\0y".to_vec()),
            ..Issue::default()
        };
        assert!(!issue_is_changed(&mut ie));
        // No issue at all, before or now, is always a change.
        assert!(issue_is_changed(&mut Issue::default()));
        let mut ie = Issue {
            mem_old: Some(b"one\n".to_vec()),
            ..Issue::default()
        };
        assert!(issue_is_changed(&mut ie));
    }

    #[test]
    fn colours_escapes_and_whatever_else_follows_a_backslash() {
        assert_eq!(expand(b"\\e{red}x\\e{reset}"), b"\x1b[31mx\x1b[0m");
        assert_eq!(expand(b"\\e[1m"), b"\x1b[1m");
        assert_eq!(expand(b"\\\\ \\q"), b"\\ q");
        // A backslash that ends the file is `EOF` cut to a byte.
        assert_eq!(expand(b"end\\"), b"end\xff");
        // An argument the file ends inside is no argument: the escape is
        // the plain one, and the rest of the file is gone.
        assert_eq!(expand(b"\\e{red"), b"\x1b");
        assert_eq!(expand(b"\\4{eth0"), b"10.0.0.4");
    }

    #[test]
    fn an_argument_keeps_one_byte_less_than_its_buffer() {
        let mut r = Reader {
            data: b"{abcdef}rest",
            pos: 0,
        };
        assert_eq!(r.argument(4).unwrap(), b"abc");
        assert_eq!(r.getc(), Some(b'r'));
        let mut r = Reader {
            data: b"x{",
            pos: 0,
        };
        assert_eq!(r.argument(4), None);
        assert_eq!(r.getc(), Some(b'x'));
    }

    #[test]
    fn os_release_values_are_scanned_as_upstream_scans_them() {
        let text = b"NAME=\"Ubuntu\"\nPRETTY_NAME=\"Ubuntu 24.04 LTS\"  \n# PRETTY_NAME=x\nANSI_COLOR=0;31\nID = ubuntu \nNAME_X=1\n";
        assert_eq!(
            os_release_value(text, b"PRETTY_NAME").unwrap(),
            b"Ubuntu 24.04 LTS"
        );
        assert_eq!(os_release_value(text, b"NAME").unwrap(), b"Ubuntu");
        assert_eq!(os_release_value(text, b"ANSI_COLOR").unwrap(), b"0;31");
        // Blanks after a value with no closing quote stay.
        assert_eq!(os_release_value(text, b"ID").unwrap(), b"ubuntu ");
        assert_eq!(os_release_value(text, b"VERSION"), None);
        // The last assignment wins, and a value may end the file.
        assert_eq!(os_release_value(b"A=1\nA=2", b"A").unwrap(), b"2");
        // The `=` and the value may be on later lines.
        assert_eq!(os_release_value(b"A\n=\n\"x\"\n", b"A").unwrap(), b"x");
        // A NUL ends the text.
        assert_eq!(os_release_value(b"A=1\0\nA=2", b"A").unwrap(), b"1");
        // No name names nothing, where upstream's scan never ends.
        assert_eq!(os_release_value(text, b""), None);
        assert_eq!(os_release_value(b"=x\n", b""), None);
    }

    #[test]
    fn an_issue_d_is_read_in_version_order_after_the_file() {
        let scratch = scratchdir::ScratchDir::new("agetty-issue-d");
        let root = scratch.path("root");
        std::fs::create_dir_all(root.join("etc/issue.d")).unwrap();
        std::fs::write(root.join("etc/issue"), b"top\n").unwrap();
        for (name, body) in [
            ("10-b.issue", "ten\n"),
            ("9-a.issue", "nine\n"),
            (".hidden.issue", "no\n"),
            ("readme", "no\n"),
        ] {
            std::fs::write(root.join("etc/issue.d").join(name), body).unwrap();
        }
        std::fs::create_dir(root.join("etc/issue.d/dir.issue")).unwrap();
        let op = Options::default();
        let paths = Paths::under(&root);
        let mut said = Said::default();
        let mut ie = Issue::default();
        Eval::new(&op, 0, &paths, &Fixed, &mut said).eval(&mut ie);
        assert_eq!(ie.mem.unwrap(), b"top\nnine\nten\n");
        assert!(ie.do_tcsetattr);
    }

    #[test]
    fn without_etc_issue_run_and_then_usr_lib_are_read() {
        let scratch = scratchdir::ScratchDir::new("agetty-issue-run");
        let root = scratch.path("root");
        std::fs::create_dir_all(root.join("usr/lib/issue.d")).unwrap();
        std::fs::write(root.join("usr/lib/issue"), b"static\n").unwrap();
        let paths = Paths::under(&root);
        let show = |op: &Options| show_issue(op, 0, &paths, &Fixed, &mut Said::default());
        assert_eq!(show(&Options::default()), b"static\n");
        // An empty `/run/issue.d` is enough to stop there.
        std::fs::create_dir_all(root.join("run/issue.d")).unwrap();
        assert_eq!(show(&Options::default()), b"");
        std::fs::write(root.join("run/issue"), b"run\n").unwrap();
        assert_eq!(show(&Options::default()), b"run\n");
        // `-i`: no issue at all.
        let quiet = Options {
            flags: 0,
            ..Options::default()
        };
        assert_eq!(show(&quiet), b"");
    }

    #[test]
    fn each_evaluation_starts_afresh_and_one_with_no_file_keeps_the_last() {
        let scratch = scratchdir::ScratchDir::new("agetty-issue-again");
        let root = scratch.path("root");
        std::fs::create_dir_all(root.join("etc")).unwrap();
        std::fs::write(root.join("etc/issue"), b"one\n").unwrap();
        let op = Options::default();
        let paths = Paths::under(&root);
        let mut said = Said::default();
        let mut ie = Issue::default();
        Eval::new(&op, 0, &paths, &Fixed, &mut said).eval(&mut ie);
        Eval::new(&op, 0, &paths, &Fixed, &mut said).eval(&mut ie);
        assert_eq!(ie.mem.as_deref().unwrap(), b"one\n");
        std::fs::remove_file(root.join("etc/issue")).unwrap();
        Eval::new(&op, 0, &paths, &Fixed, &mut said).eval(&mut ie);
        assert_eq!(ie.mem.unwrap(), b"one\n");
    }

    #[test]
    fn the_os_release_name_falls_back_to_the_system_s_with_a_warning_each_time() {
        let scratch = scratchdir::ScratchDir::new("agetty-issue-os");
        let root = scratch.path("root");
        std::fs::create_dir_all(root.join("etc")).unwrap();
        std::fs::write(root.join("etc/issue"), b"\\S|\\S{ANSI_COLOR}|\\S{NOPE}").unwrap();
        let op = Options::default();
        let paths = Paths::under(&root);
        let mut said = Said::default();
        assert_eq!(show_issue(&op, 0, &paths, &Fixed, &mut said), b"Linux||");
        assert_eq!(said.syslog.len(), 3);
        assert_eq!(said.syslog[0], b"cannot open os-release file");
        std::fs::create_dir_all(root.join("usr/lib")).unwrap();
        std::fs::write(root.join("usr/lib/os-release"), b"PRETTY_NAME=Lib\n").unwrap();
        let mut said = Said::default();
        assert_eq!(show_issue(&op, 0, &paths, &Fixed, &mut said), b"Lib||");
        std::fs::write(
            root.join("etc/os-release"),
            b"PRETTY_NAME=\"Slate\"\nANSI_COLOR=\"1;34\"\n",
        )
        .unwrap();
        assert_eq!(
            show_issue(&op, 0, &paths, &Fixed, &mut said),
            b"Slate|\x1b[1;34m|"
        );
        assert!(said.syslog.is_empty());
    }

    #[test]
    fn utmp_users_count_user_processes_only() {
        let record = |kind: u8| {
            let mut r = vec![0u8; utmpfile::RECORD_SIZE];
            r[0] = kind;
            r
        };
        let data = [record(7), record(6), record(7), record(8)].concat();
        assert_eq!(utmp_users(&data), 2);
        assert_eq!(utmp_users(&[]), 0);
    }
}
