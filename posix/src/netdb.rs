// Offsets here are within one database line or one caller buffer, which
// bounds every sum made of them; the few sums that could pass the bound are
// checked.  Clippy cannot see the bounds.
#![allow(clippy::arithmetic_side_effects)]
//! `<netdb.h>`'s flat-file databases -- `/etc/services`, `/etc/protocols`,
//! `/etc/networks`, `/etc/ethers` -- and `<rpc/netdb.h>`'s `/etc/rpc`, read
//! as glibc 2.40's `nss_files` reads them.
//!
//! - **Lines** are `__nss_readline`'s ([`crate::nss_files::lines`]): leading
//!   white space skipped, empty lines and `#` comments not entries, and a
//!   `#` anywhere ends the line.
//! - **Fields** are `files-parse.c`'s macros ([`Line`]): a name runs to
//!   white space, which is swallowed; a number is `strtoul`'s, in the base
//!   each database's parser names, clamped to 32 bits, and must be followed
//!   by its terminator or the end; aliases are every white-space-separated
//!   word after that.  A line the parser refuses is not an entry.
//! - **Lookups** scan the whole file for the first match: services,
//!   protocols and RPC programs by name or alias exactly (`strcmp`),
//!   networks and ethers by name ignoring case (`strcasecmp`).
//! - **Answers** follow glibc's `getXXbyYY_r`: 0 whether found or not, with
//!   `*result` saying which, `ERANGE` when the caller's buffer is too small
//!   -- and the non-reentrant forms grow their block and retry, as
//!   glibc's `getXXbyYY` does.
//!
//! **A missing file** answers from a built-in copy ([`SERVICES`],
//! [`PROTOCOLS`], [`NETWORKS`], [`RPC`]), as `/etc/passwd`'s does from its
//! `root` entry (design-decisions.md section 1113): the booted system has
//! no `/etc` of its own yet, and "no service is called `http`" would be a
//! worse answer than the registry's.  The copies are this project's, from
//! the IANA registries, in the file's own format and read by the same
//! parser -- so a file, when there is one, simply takes their place.  There
//! is no built-in `/etc/ethers`.  (glibc, with no file, has nothing to
//! read: its lookups answer the open's error, `ENOENT`, and its
//! enumerations end at once with `errno` as it was -- as these do for a
//! file that exists and cannot be read.)
//!
//! **Per thread, not per process.**  The non-reentrant functions' results,
//! and the `set*ent`/`get*ent`/`end*ent` cursors, belong to the calling
//! thread ([`ThreadDb`], allocated on first use and freed when the thread
//! exits): two threads never overwrite each other's answer or consume each
//! other's entries.  glibc keeps one of each per process, behind a lock.

use crate::errno;
use crate::inet::EtherAddr;
use crate::nss_files::{self, Held, Room, Which};

// ---------------------------------------------------------------------------
// The entries
// ---------------------------------------------------------------------------

/// `struct servent`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Servent {
    /// Official service name.
    pub s_name: *const u8,
    /// Alias list (NULL-terminated).
    pub s_aliases: *const *const u8,
    /// Port number, network byte order.
    pub s_port: i32,
    /// Protocol name.
    pub s_proto: *const u8,
}

impl Servent {
    const EMPTY: Self = Self {
        s_name: core::ptr::null(),
        s_aliases: core::ptr::null(),
        s_port: 0,
        s_proto: core::ptr::null(),
    };
}

/// `struct protoent`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Protoent {
    /// Official protocol name.
    pub p_name: *const u8,
    /// Alias list (NULL-terminated).
    pub p_aliases: *const *const u8,
    /// Protocol number.
    pub p_proto: i32,
}

impl Protoent {
    const EMPTY: Self = Self {
        p_name: core::ptr::null(),
        p_aliases: core::ptr::null(),
        p_proto: 0,
    };
}

/// `struct netent`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Netent {
    /// Official network name.
    pub n_name: *const u8,
    /// Alias list (NULL-terminated).
    pub n_aliases: *const *const u8,
    /// Address family: `AF_INET`.
    pub n_addrtype: i32,
    /// Network number, host byte order.
    pub n_net: u32,
}

impl Netent {
    const EMPTY: Self = Self {
        n_name: core::ptr::null(),
        n_aliases: core::ptr::null(),
        n_addrtype: 0,
        n_net: 0,
    };
}

/// `struct rpcent` (`<rpc/netdb.h>`).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Rpcent {
    /// The program's official name.
    pub r_name: *const u8,
    /// Alias list (NULL-terminated).
    pub r_aliases: *const *const u8,
    /// The ONC RPC program number.
    pub r_number: i32,
}

impl Rpcent {
    const EMPTY: Self = Self {
        r_name: core::ptr::null(),
        r_aliases: core::ptr::null(),
        r_number: 0,
    };
}

// ---------------------------------------------------------------------------
// The built-in databases
// ---------------------------------------------------------------------------

/// `/etc/services` when there is none: the IANA Service Name and Transport
/// Protocol Port Number Registry's well-known entries, under the names Linux
/// systems give them.
pub(crate) const SERVICES: &[u8] = b"\
tcpmux 1/tcp
echo 7/tcp
echo 7/udp
discard 9/tcp sink null
discard 9/udp sink null
systat 11/tcp users
daytime 13/tcp
daytime 13/udp
netstat 15/tcp
qotd 17/tcp quote
chargen 19/tcp ttytst source
chargen 19/udp ttytst source
ftp-data 20/tcp
ftp 21/tcp
ssh 22/tcp
telnet 23/tcp
smtp 25/tcp mail
time 37/tcp timserver
time 37/udp timserver
whois 43/tcp nicname
tacacs 49/tcp
tacacs 49/udp
domain 53/tcp
domain 53/udp
bootps 67/udp
bootpc 68/udp
tftp 69/udp
gopher 70/tcp
finger 79/tcp
http 80/tcp www
kerberos 88/tcp kerberos5 krb5 kerberos-sec
kerberos 88/udp kerberos5 krb5 kerberos-sec
iso-tsap 102/tcp tsap
pop3 110/tcp pop-3
sunrpc 111/tcp portmapper
sunrpc 111/udp portmapper
auth 113/tcp authentication tap ident
nntp 119/tcp readnews untp
ntp 123/udp
epmap 135/tcp loc-srv
epmap 135/udp loc-srv
netbios-ns 137/udp
netbios-dgm 138/udp
netbios-ssn 139/tcp
imap2 143/tcp imap
snmp 161/tcp
snmp 161/udp
snmp-trap 162/tcp snmptrap
snmp-trap 162/udp snmptrap
xdmcp 177/udp
bgp 179/tcp
irc 194/tcp
ldap 389/tcp
ldap 389/udp
https 443/tcp
https 443/udp
microsoft-ds 445/tcp
kpasswd 464/tcp
kpasswd 464/udp
submissions 465/tcp ssmtp smtps urd
isakmp 500/udp
exec 512/tcp
biff 512/udp comsat
login 513/tcp
who 513/udp whod
shell 514/tcp cmd syslog
syslog 514/udp
printer 515/tcp spooler
talk 517/udp
ntalk 518/udp
route 520/udp router routed
uucp 540/tcp uucpd
klogin 543/tcp
kshell 544/tcp krcmd
dhcpv6-client 546/udp
dhcpv6-server 547/udp
afpovertcp 548/tcp
rtsp 554/tcp
rtsp 554/udp
nntps 563/tcp snntp
submission 587/tcp
ipp 631/tcp
ldaps 636/tcp
ldaps 636/udp
kerberos-adm 749/tcp
domain-s 853/tcp
domain-s 853/udp
rsync 873/tcp
ftps-data 989/tcp
ftps 990/tcp
telnets 992/tcp
imaps 993/tcp
pop3s 995/tcp
socks 1080/tcp
openvpn 1194/tcp
openvpn 1194/udp
ms-sql-s 1433/tcp
ms-sql-m 1434/udp
pptp 1723/tcp
radius 1812/tcp
radius 1812/udp
radius-acct 1813/tcp radacct
radius-acct 1813/udp radacct
nfs 2049/tcp
nfs 2049/udp
cvspserver 2401/tcp
mysql 3306/tcp
svn 3690/tcp subversion
sip 5060/tcp
sip 5060/udp
sip-tls 5061/tcp
sip-tls 5061/udp
xmpp-client 5222/tcp jabber-client
xmpp-server 5269/tcp jabber-server
mdns 5353/udp
postgresql 5432/tcp postgres
amqp 5672/tcp
x11 6000/tcp x11-0
ircs-u 6697/tcp
http-alt 8080/tcp webcache
git 9418/tcp
";

/// `/etc/protocols` when there is none: the IANA Assigned Internet Protocol
/// Numbers registry, each under its lower-case name with the registry's
/// keyword as its alias.
pub(crate) const PROTOCOLS: &[u8] = b"\
ip 0 IP
hopopt 0 HOPOPT
icmp 1 ICMP
igmp 2 IGMP
ggp 3 GGP
ipencap 4 IP-ENCAP
st 5 ST
tcp 6 TCP
egp 8 EGP
igp 9 IGP
pup 12 PUP
udp 17 UDP
hmp 20 HMP
xns-idp 22 XNS-IDP
rdp 27 RDP
iso-tp4 29 ISO-TP4
dccp 33 DCCP
xtp 36 XTP
ddp 37 DDP
idpr-cmtp 38 IDPR-CMTP
ipv6 41 IPv6
ipv6-route 43 IPv6-Route
ipv6-frag 44 IPv6-Frag
idrp 45 IDRP
rsvp 46 RSVP
gre 47 GRE
esp 50 IPSEC-ESP
ah 51 IPSEC-AH
skip 57 SKIP
ipv6-icmp 58 IPv6-ICMP
ipv6-nonxt 59 IPv6-NoNxt
ipv6-opts 60 IPv6-Opts
rspf 73 RSPF CPHB
vmtp 81 VMTP
eigrp 88 EIGRP
ospf 89 OSPFIGP
ax.25 93 AX.25
ipip 94 IPIP
etherip 97 ETHERIP
encap 98 ENCAP
pim 103 PIM
ipcomp 108 IPCOMP
vrrp 112 VRRP
l2tp 115 L2TP
isis 124 ISIS
sctp 132 SCTP
fc 133 FC
mobility-header 135 Mobility-Header
udplite 136 UDPLite
mpls-in-ip 137 MPLS-in-IP
manet 138
hip 139 HIP
shim6 140 Shim6
wesp 141 WESP
rohc 142 ROHC
ethernet 143 Ethernet
mptcp 262 MPTCP
";

/// `/etc/networks` when there is none.
pub(crate) const NETWORKS: &[u8] = b"\
link-local 169.254.0.0
";

/// `/etc/rpc` when there is none: the programs of the IANA Remote
/// Procedure Call Program Numbers registry that Linux systems name, under
/// those names -- Sun's assignments, and SGI's File Alteration Monitor.
/// (Linux's files also name three programs from the range the registry
/// leaves to users, 0x20000000 up, which no registry vouches for; they are
/// not here.)  `posix/tools/oracle/rpc_harness.py` reads it from this file:
/// no escapes, and nothing after the last line.
pub(crate) const RPC: &[u8] = b"\
portmapper 100000 portmap sunrpc rpcbind
rstatd 100001 rstat rstat_svc rup perfmeter
rusersd 100002 rusers
nfs 100003 nfsprog
ypserv 100004 ypprog
mountd 100005 mount showmount
ypbind 100007
walld 100008 rwall shutdown
yppasswdd 100009 yppasswd
etherstatd 100010 etherstat
rquotad 100011 rquotaprog quota rquota
sprayd 100012 spray
3270_mapper 100013
rje_mapper 100014
selection_svc 100015 selnsvc
database_svc 100016
rexd 100017 rex
alis 100018
sched 100019
llockmgr 100020
nlockmgr 100021
x25.inr 100022
statmon 100023
status 100024
bootparam 100026
ypupdated 100028 ypupdate
keyserv 100029 keyserver
tfsd 100037
nsed 100038
nsemntd 100039
ypxfrd 100069
nfs_acl 100227
pcnfsd 150001
amd 300019 amq
sgi_fam 391002
";

// ---------------------------------------------------------------------------
// Taking a line apart
// ---------------------------------------------------------------------------

/// `strtoul (s, &end, base)` in its full 64 bits: leading white space, a
/// sign (a negative number negated as unsigned), a `0x` prefix for bases 16
/// and 0, a leading `0` meaning octal for base 0; `u64::MAX` on overflow, as
/// `ULONG_MAX`.  The value and the bytes used, or `None` when there are no
/// digits.
pub(crate) fn strtou64(s: &[u8], base: u32) -> Option<(u64, usize)> {
    let mut i = s.iter().take_while(|&&b| nss_files::is_space(b)).count();
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
    let hex_prefix = matches!(s.get(i..i + 2), Some([b'0', b'x' | b'X']))
        && s.get(i + 2).is_some_and(u8::is_ascii_hexdigit);
    let base = match base {
        0 if hex_prefix => 16,
        0 if s.get(i) == Some(&b'0') => 8,
        0 => 10,
        b => b,
    };
    if base == 16 && hex_prefix {
        i += 2;
    }
    let digit = |b: u8| -> Option<u64> {
        let d = match b {
            b'0'..=b'9' => b - b'0',
            b'a'..=b'z' => b - b'a' + 10,
            b'A'..=b'Z' => b - b'A' + 10,
            _ => return None,
        };
        (u32::from(d) < base).then_some(u64::from(d))
    };
    let start = i;
    let mut value: u64 = 0;
    let mut overflow = false;
    while let Some(d) = s.get(i).copied().and_then(digit) {
        match value
            .checked_mul(u64::from(base))
            .and_then(|v| v.checked_add(d))
        {
            Some(v) => value = v,
            None => overflow = true,
        }
        i += 1;
    }
    if i == start {
        return None;
    }
    let value = if overflow {
        u64::MAX
    } else if negative {
        value.wrapping_neg()
    } else {
        value
    };
    Some((value, i))
}

/// `strtoull (s, &end, base)` as Debian's glibc takes a number field: leading
/// white space, a sign, a `0x` prefix for bases 16 and 0, a leading `0`
/// meaning octal for base 0.  The value and the bytes used; `None` when there
/// are no digits (`end == s`) or the value is past 32 bits -- a negative one
/// among them, which `strtoull` negates as unsigned, so anything but `-0` --
/// either of which makes the line no entry.  Upstream glibc clamps the
/// second to `0xffffffff` instead (design-decisions §1136).
pub(crate) fn strtou32(s: &[u8], base: u32) -> Option<(u32, usize)> {
    let mut i = s.iter().take_while(|&&b| nss_files::is_space(b)).count();
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
    let hex_prefix = matches!(s.get(i..i + 2), Some([b'0', b'x' | b'X']))
        && s.get(i + 2).is_some_and(u8::is_ascii_hexdigit);
    let base = match base {
        0 if hex_prefix => 16,
        0 if s.get(i) == Some(&b'0') => 8,
        0 => 10,
        b => b,
    };
    if base == 16 && hex_prefix {
        i += 2;
    }
    let digit = |b: u8| -> Option<u64> {
        let d = match b {
            b'0'..=b'9' => b - b'0',
            b'a'..=b'z' => b - b'a' + 10,
            b'A'..=b'Z' => b - b'A' + 10,
            _ => return None,
        };
        (u32::from(d) < base).then_some(u64::from(d))
    };
    let start = i;
    // `None` once the digits overflow 64 bits: strtoull then answers
    // ULLONG_MAX, unnegated -- past 32 bits whatever the sign.
    let mut value = Some(0u64);
    while let Some(d) = s.get(i).copied().and_then(digit) {
        value = value
            .and_then(|v| v.checked_mul(u64::from(base)))
            .and_then(|v| v.checked_add(d));
        i += 1;
    }
    if i == start {
        return None;
    }
    let value = if negative {
        value?.wrapping_neg()
    } else {
        value?
    };
    Some((u32::try_from(value).ok()?, i))
}

/// One database line, taken apart as `files-parse.c`'s `LINE_PARSER`
/// macros take it.
pub(crate) struct Line<'a> {
    rest: &'a [u8],
}

impl<'a> Line<'a> {
    /// The line up to its end: its first NUL (a C string ends there) or its
    /// first `#` (`strpbrk (line, "#\n")`).
    pub(crate) fn new(line: &'a [u8]) -> Self {
        let end = line
            .iter()
            .position(|&b| b == 0 || b == b'#' || b == b'\n')
            .unwrap_or(line.len());
        Self {
            rest: line.get(..end).unwrap_or(&[]),
        }
    }

    /// `STRING_FIELD (v, isspace, 1)`: up to white space, which is
    /// swallowed; the rest of the line when there is none.
    pub(crate) fn string(&mut self) -> &'a [u8] {
        let n = self
            .rest
            .iter()
            .position(|&b| nss_files::is_space(b))
            .unwrap_or(self.rest.len());
        let (field, tail) = self.rest.split_at(n);
        let skip = tail.iter().take_while(|&&b| nss_files::is_space(b)).count();
        self.rest = tail.get(skip..).unwrap_or(&[]);
        field
    }

    /// `INT_FIELD (v, term, swallow, base, ...)`: a number followed by its
    /// terminator (consumed, and every one after it when `swallow`) or by
    /// the line's end.  `None`: the line is no entry.
    pub(crate) fn int(&mut self, term: fn(u8) -> bool, swallow: bool, base: u32) -> Option<u32> {
        let (value, used) = strtou32(self.rest, base)?;
        let mut tail = self.rest.get(used..)?;
        match tail.first() {
            Some(&b) if term(b) => {
                let n = if swallow {
                    tail.iter().take_while(|&&b| term(b)).count()
                } else {
                    1
                };
                tail = tail.get(n..).unwrap_or(&[]);
            }
            Some(_) => return None,
            None => {}
        }
        self.rest = tail;
        Some(value)
    }

    /// What is left of the line.
    pub(crate) fn rest(&self) -> &'a [u8] {
        self.rest
    }

    /// A line of what `rest` gave back.
    pub(crate) fn from_rest(rest: &'a [u8]) -> Self {
        Self { rest }
    }

    /// `parse_list` with white space between the words: every word left.
    pub(crate) fn words(self) -> impl Iterator<Item = &'a [u8]> + Clone {
        self.rest
            .split(|&b| nss_files::is_space(b))
            .filter(|w| !w.is_empty())
    }
}

/// `isspace`, as a terminator for [`Line::int`].
fn space(b: u8) -> bool {
    nss_files::is_space(b)
}

/// Copy `words` into `room` as a NULL-terminated array of strings.
fn fill_list<'a>(
    room: &mut Room,
    words: impl Iterator<Item = &'a [u8]> + Clone,
) -> Result<*const *const u8, i32> {
    let n = words.clone().count();
    let list = room.pointers(n + 1)?;
    for (i, w) in words.enumerate() {
        let s = room.string(w)?;
        // SAFETY: `list` holds `n + 1` pointers and `i < n`.
        unsafe { list.add(i).write(s.cast_const()) };
    }
    Ok(list.cast_const())
}

// ---------------------------------------------------------------------------
// Each thread's results and cursors
// ---------------------------------------------------------------------------

/// Where one thread's enumeration of one database is: glibc's per-database
/// stream, as a copy of the file read whole.  All-zero is closed.
pub(crate) struct Cursor {
    /// The database's text: the file's `malloc` copy, or a built-in (not
    /// freed), or NULL.
    text: *mut u8,
    len: usize,
    /// Whether `text` is this cursor's own block.
    owned: bool,
    /// Whether the database is open (`text` meaningful).
    open: bool,
    /// The offset of the next line.
    at: usize,
}

impl Cursor {
    const CLOSED: Self = Self {
        text: core::ptr::null_mut(),
        len: 0,
        owned: false,
        open: false,
        at: 0,
    };

    /// Close: free the file's copy.  The next `get*ent` reads it afresh.
    fn close(&mut self) {
        if self.owned {
            // SAFETY: an owned `text` is this cursor's `malloc` block.
            unsafe { crate::malloc::free(self.text) };
        }
        *self = Self::CLOSED;
    }

    /// The database's text, opening it first if need be: `None` when the
    /// file exists and cannot be read.
    ///
    /// `errno` is left as it was then: glibc's `_nss_files_getXXent_r`
    /// saves it around the open it makes when nothing is open, and puts it
    /// back whatever the open did, so the enumeration ends (`ENOENT` from
    /// the `_r` form, NULL from the other) with the caller's `errno`.
    fn text(&mut self, which: Which, builtin: &'static [u8]) -> Option<&[u8]> {
        if !self.open {
            match nss_files::read(which) {
                Ok(nss_files::Db::Text(t)) => {
                    let (ptr, len) = t.into_raw();
                    self.text = ptr;
                    self.len = len;
                    self.owned = true;
                }
                Ok(nss_files::Db::Missing) => {
                    self.text = builtin.as_ptr().cast_mut();
                    self.len = builtin.len();
                    self.owned = false;
                }
                // The open's error is not the enumeration's answer.
                Err(_) => return None,
            }
            self.open = true;
            self.at = 0;
        }
        if self.text.is_null() {
            return Some(&[]);
        }
        // SAFETY: `text` holds `len` bytes: the file's block, owned by this
        // cursor until `close`, or a built-in `'static` slice.
        Some(unsafe { core::slice::from_raw_parts(self.text, self.len) })
    }
}

/// One thread's netdb state: the block each non-reentrant function answers
/// in, and each database's enumeration.
pub(crate) struct ThreadDb {
    serv: Held<Servent>,
    serv_ent: Held<Servent>,
    serv_cur: Cursor,
    proto: Held<Protoent>,
    proto_ent: Held<Protoent>,
    proto_cur: Cursor,
    net: Held<Netent>,
    net_ent: Held<Netent>,
    net_cur: Cursor,
    rpc: Held<Rpcent>,
    rpc_ent: Held<Rpcent>,
    rpc_cur: Cursor,
    host: Held<crate::socket::Hostent>,
    host_rev: Held<crate::socket::Hostent>,
    host_ent: Held<crate::socket::Hostent>,
    host_cur: Cursor,
    /// `inet_nsap_ntoa`'s answer when it is given no buffer
    /// ([`crate::inet`]): the resolver family's one other non-reentrant
    /// answer, kept with these rather than in every thread's block.
    nsap: [u8; crate::inet::NSAP_NTOA_MAX],
    /// `hostalias`'s answer ([`crate::resolv`]), glibc's static `abuf`.
    alias: [u8; crate::resolv::NS_MAXDNAME],
    /// The answers [`crate::res_debug`]'s printers keep in glibc's statics.
    res_debug: ResDebugBufs,
}

/// [`crate::res_debug`]'s buffers: `sym_ntos`'s and `sym_ntop`'s decimal
/// for a number in no table, `p_option`'s, `p_time`'s, and `loc_ntoa`'s
/// given none -- one each, as glibc has one static each.
struct ResDebugBufs {
    ntos: [u8; 20],
    ntop: [u8; 20],
    option: [u8; 40],
    time: [u8; 40],
    loc: [u8; crate::res_debug::LOC_NTOA_MAX],
}

/// Which of [`ResDebugBufs`]'s buffers.
#[derive(Clone, Copy)]
pub(crate) enum ResDebugBuf {
    /// `sym_ntos`'s (and `p_class`'s, `p_type`'s, `p_rcode`'s).
    Ntos,
    /// `sym_ntop`'s.
    Ntop,
    /// `p_option`'s.
    Option,
    /// `p_time`'s.
    Time,
    /// `loc_ntoa`'s.
    Loc,
}

impl ResDebugBuf {
    /// The buffer's size in bytes.
    pub(crate) const fn size(self) -> usize {
        match self {
            Self::Ntos | Self::Ntop => 20,
            Self::Option | Self::Time => 40,
            Self::Loc => crate::res_debug::LOC_NTOA_MAX,
        }
    }
}

impl ThreadDb {
    const NEW: Self = Self {
        serv: Held::new(Servent::EMPTY),
        serv_ent: Held::new(Servent::EMPTY),
        serv_cur: Cursor::CLOSED,
        proto: Held::new(Protoent::EMPTY),
        proto_ent: Held::new(Protoent::EMPTY),
        proto_cur: Cursor::CLOSED,
        net: Held::new(Netent::EMPTY),
        net_ent: Held::new(Netent::EMPTY),
        net_cur: Cursor::CLOSED,
        rpc: Held::new(Rpcent::EMPTY),
        rpc_ent: Held::new(Rpcent::EMPTY),
        rpc_cur: Cursor::CLOSED,
        host: Held::new(HOSTENT_EMPTY),
        host_rev: Held::new(HOSTENT_EMPTY),
        host_ent: Held::new(HOSTENT_EMPTY),
        host_cur: Cursor::CLOSED,
        nsap: [0; crate::inet::NSAP_NTOA_MAX],
        alias: [0; crate::resolv::NS_MAXDNAME],
        res_debug: ResDebugBufs {
            ntos: [0; 20],
            ntop: [0; 20],
            option: [0; 40],
            time: [0; 40],
            loc: [0; crate::res_debug::LOC_NTOA_MAX],
        },
    };
}

// The calling thread's blocks and cursors, by field: `fn` pointers for
// [`held`], [`next_entry`] and [`with_cursor`].
macro_rules! field {
    ($vis:vis $name:ident, $field:ident, $ty:ty) => {
        $vis fn $name(db: *mut ThreadDb) -> *mut $ty {
            // SAFETY: callers pass the calling thread's live `ThreadDb`.
            unsafe { &raw mut (*db).$field }
        }
    };
}

/// An empty `struct hostent`.
const HOSTENT_EMPTY: crate::socket::Hostent = crate::socket::Hostent {
    h_name: core::ptr::null(),
    h_aliases: core::ptr::null(),
    h_addrtype: 0,
    h_length: 0,
    h_addr_list: core::ptr::null(),
};
field!(serv_held, serv, Held<Servent>);
field!(serv_ent_held, serv_ent, Held<Servent>);
field!(serv_cur, serv_cur, Cursor);
field!(proto_held, proto, Held<Protoent>);
field!(proto_ent_held, proto_ent, Held<Protoent>);
field!(proto_cur, proto_cur, Cursor);
field!(net_held_block, net, Held<Netent>);
field!(net_ent_held, net_ent, Held<Netent>);
field!(net_cur, net_cur, Cursor);
field!(rpc_held, rpc, Held<Rpcent>);
field!(rpc_ent_held, rpc_ent, Held<Rpcent>);
field!(rpc_cur, rpc_cur, Cursor);
field!(pub(crate) host_held_block, host, Held<crate::socket::Hostent>);
field!(pub(crate) host_rev_held, host_rev, Held<crate::socket::Hostent>);
field!(pub(crate) host_ent_held, host_ent, Held<crate::socket::Hostent>);
field!(pub(crate) host_cur, host_cur, Cursor);

/// The calling thread's [`ThreadDb`], allocated on its first use; NULL
/// when memory runs out.
fn thread_db() -> *mut ThreadDb {
    // SAFETY: the calling thread's block, touched by no other thread.
    let slot = unsafe { &raw mut (*crate::perthread::current()).netdb };
    // SAFETY: as above.
    let p = unsafe { *slot };
    if !p.is_null() {
        return p.cast();
    }
    let p = crate::malloc::malloc(size_of::<ThreadDb>()).cast::<ThreadDb>();
    if p.is_null() {
        return p;
    }
    // SAFETY: a fresh block of the right size; `malloc` aligns for any
    // type this size.
    unsafe {
        p.write(ThreadDb::NEW);
        *slot = p.cast();
    }
    p
}

/// The calling thread's buffer for `inet_nsap_ntoa` given none, which
/// holds [`crate::inet::NSAP_NTOA_MAX`] bytes; NULL when memory runs out.
pub(crate) fn nsap_ntoa_buffer() -> *mut u8 {
    let db = thread_db();
    if db.is_null() {
        return core::ptr::null_mut();
    }
    // SAFETY: the calling thread's live `ThreadDb`.
    unsafe { (&raw mut (*db).nsap).cast() }
}

/// The calling thread's buffer for `hostalias`'s answer, which holds
/// [`crate::resolv::NS_MAXDNAME`] bytes; NULL when memory runs out.
pub(crate) fn hostalias_buffer() -> *mut u8 {
    let db = thread_db();
    if db.is_null() {
        return core::ptr::null_mut();
    }
    // SAFETY: the calling thread's live `ThreadDb`.
    unsafe { (&raw mut (*db).alias).cast() }
}

/// The calling thread's buffer `which` for [`crate::res_debug`], which
/// holds `which.size()` bytes; NULL when memory runs out.
pub(crate) fn res_debug_buffer(which: ResDebugBuf) -> *mut u8 {
    let db = thread_db();
    if db.is_null() {
        return core::ptr::null_mut();
    }
    // SAFETY: the calling thread's live `ThreadDb`.
    let b = unsafe { &raw mut (*db).res_debug };
    // SAFETY: as above: a field of it.
    unsafe {
        match which {
            ResDebugBuf::Ntos => (&raw mut (*b).ntos).cast(),
            ResDebugBuf::Ntop => (&raw mut (*b).ntop).cast(),
            ResDebugBuf::Option => (&raw mut (*b).option).cast(),
            ResDebugBuf::Time => (&raw mut (*b).time).cast(),
            ResDebugBuf::Loc => (&raw mut (*b).loc).cast(),
        }
    }
}

/// Free the calling thread's netdb state: called as the thread exits.
pub(crate) fn thread_cleanup() {
    // SAFETY: the calling thread's block, touched by no other thread.
    let slot = unsafe { &raw mut (*crate::perthread::current()).netdb };
    // SAFETY: as above.
    let p = unsafe { core::ptr::replace(slot, core::ptr::null_mut()) }.cast::<ThreadDb>();
    if p.is_null() {
        return;
    }
    // SAFETY: `p` is this thread's `ThreadDb`, and nothing else refers to
    // it now that the slot is cleared.
    unsafe {
        let db = &mut *p;
        db.serv.release();
        db.serv_ent.release();
        db.serv_cur.close();
        db.proto.release();
        db.proto_ent.release();
        db.proto_cur.close();
        db.net.release();
        db.net_ent.release();
        db.net_cur.close();
        db.rpc.release();
        db.rpc_ent.release();
        db.rpc_cur.close();
        db.host.release();
        db.host_rev.release();
        db.host_ent.release();
        db.host_cur.close();
        crate::malloc::free(p.cast());
    }
}

/// Answer through one of the calling thread's result blocks: `field` picks
/// it, `call` is the `_r` function.  NULL when memory runs out, with
/// `errno` `ENOMEM`.
pub(crate) fn held<T>(
    field: fn(*mut ThreadDb) -> *mut Held<T>,
    call: impl FnMut(*mut T, *mut u8, usize, *mut *const T) -> i32,
) -> Result<*const T, i32> {
    let db = thread_db();
    if db.is_null() {
        return Err(errno::ENOMEM);
    }
    nss_files::hold(field(db), call)
}

/// The next entry of an enumeration, by the calling thread's cursor --
/// `take` parses and fills one line, `None` for a line that is no entry:
/// 0, `ENOENT` at the end, `ENOMEM`, or the fill's `ERANGE`, after which
/// the same entry comes again.  `errno` is glibc's: the fill's error, and
/// otherwise as it was -- at the end, and when the file cannot be read.
///
/// # Safety
///
/// Non-null pointers are the caller's: `out` and `result` writable, `buf`
/// writable for `buflen` bytes.
#[allow(clippy::too_many_arguments)] // the `_r` interface's four, and the database's four
pub(crate) unsafe fn next_entry<T>(
    cursor: fn(*mut ThreadDb) -> *mut Cursor,
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
    let db = thread_db();
    if db.is_null() {
        return errno::ENOMEM;
    }
    // SAFETY: the calling thread's cursor; nothing else refers to it during
    // this call.
    let c = unsafe { &mut *cursor(db) };
    let Some(text) = c.text(which, builtin) else {
        // glibc tries the file again on every call while it has none open.
        c.close();
        return errno::ENOENT;
    };
    // SAFETY: the text lives until the cursor is closed, which nothing in
    // this call does; the slice is only decoupled from the borrow of `c`
    // so that `c.at` can move while it is read.
    let text: &[u8] = unsafe { core::slice::from_raw_parts(text.as_ptr(), text.len()) };
    let from = c.at;
    for (line, next) in nss_files::lines(text, from) {
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
                unsafe { nss_files::deliver(value, out, result) };
                0
            }
            // In `errno` too, as glibc's backend reports it
            // (`*errnop = ERANGE`) -- where success and the end leave
            // `errno` as it was.
            Err(e) => {
                errno::set_errno(e);
                e
            }
        };
    }
    c.at = c.len;
    errno::ENOENT
}

/// A lookup over one database's entries: the first line `take` answers
/// `Some` for.  `Ok(None)`: not found.
fn scan<T>(
    which: Which,
    builtin: &'static [u8],
    mut take: impl FnMut(&[u8]) -> Option<Result<T, i32>>,
) -> Result<Option<T>, i32> {
    nss_files::with_text(which, builtin, |text| {
        for (line, _) in nss_files::lines(text, 0) {
            if let Some(r) = take(line) {
                return r.map(Some);
            }
        }
        Ok(None)
    })?
}

/// A NUL-terminated string's bytes.
///
/// # Safety
///
/// `s` is non-null and NUL-terminated.
unsafe fn bytes<'a>(s: *const u8) -> &'a [u8] {
    // SAFETY: the caller's contract.
    unsafe { nss_files::c_bytes(s) }
}

/// The byte-wise case-insensitive comparison `strcasecmp` makes in the C
/// locale.
fn eq_ignore_case(a: &[u8], b: &[u8]) -> bool {
    a.eq_ignore_ascii_case(b)
}

// ---------------------------------------------------------------------------
// Services
// ---------------------------------------------------------------------------

/// A `/etc/services` line: `name port/proto aliases...`.
struct ServLine<'a> {
    name: &'a [u8],
    /// Network byte order: `htons` of the number, truncated to 16 bits as
    /// `htons` truncates.
    port: i32,
    proto: &'a [u8],
    aliases: Line<'a>,
}

/// `files-service.c`'s parser.  The port is `INT_FIELD (s_port, ISSLASH,
/// 10, 0, htons)` -- which, read by the macro's argument order, is *base 0*
/// (so `0x1f` and `010` are numbers) with every `/` after it swallowed.
fn parse_serv(line: &[u8]) -> Option<ServLine<'_>> {
    let mut l = Line::new(line);
    let name = l.string();
    let port = l.int(|b| b == b'/', true, 0)?;
    let proto = l.string();
    Some(ServLine {
        name,
        port: i32::from((port as u16).to_be()),
        proto,
        aliases: l,
    })
}

/// A service's answer in `room`.
fn fill_serv(s: &ServLine<'_>, room: &mut Room) -> Result<Servent, i32> {
    Ok(Servent {
        s_name: room.string(s.name)?,
        s_proto: room.string(s.proto)?,
        s_port: s.port,
        s_aliases: fill_list(
            room,
            Line {
                rest: s.aliases.rest,
            }
            .words(),
        )?,
    })
}

/// Whether `name` is a service's name or one of its aliases (`strcmp`).
fn names_match(name: &[u8], official: &[u8], aliases: &Line<'_>) -> bool {
    official == name || Line { rest: aliases.rest }.words().any(|a| a == name)
}

/// Look up a service by name and protocol (any, when `proto` is NULL):
/// glibc's `getservbyname_r`.
///
/// # Safety
///
/// `name` is NUL-terminated; `proto` NULL or NUL-terminated; `result_buf`
/// and `result` writable; `buf` writable for `buflen` bytes.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn getservbyname_r(
    name: *const u8,
    proto: *const u8,
    result_buf: *mut Servent,
    buf: *mut u8,
    buflen: usize,
    result: *mut *const Servent,
) -> i32 {
    // SAFETY: the caller's pointers, checked non-null by `reentrant`.
    unsafe {
        nss_files::reentrant(result_buf, buf, buflen, result, |room| {
            if name.is_null() {
                return Ok(None);
            }
            let name = bytes(name);
            let proto = (!proto.is_null()).then(|| bytes(proto));
            scan(Which::Services, SERVICES, |line| {
                let s = parse_serv(line)?;
                if proto.is_some_and(|p| p != s.proto) || !names_match(name, s.name, &s.aliases) {
                    return None;
                }
                Some(fill_serv(&s, room))
            })
        })
    }
}

/// Look up a service by port (network byte order, as `s_port` holds it)
/// and protocol (any, when `proto` is NULL): glibc's `getservbyport_r`.
///
/// # Safety
///
/// As [`getservbyname_r`].
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn getservbyport_r(
    port: i32,
    proto: *const u8,
    result_buf: *mut Servent,
    buf: *mut u8,
    buflen: usize,
    result: *mut *const Servent,
) -> i32 {
    // SAFETY: as in `getservbyname_r`.
    unsafe {
        nss_files::reentrant(result_buf, buf, buflen, result, |room| {
            let proto = (!proto.is_null()).then(|| bytes(proto));
            scan(Which::Services, SERVICES, |line| {
                let s = parse_serv(line)?;
                if s.port != port || proto.is_some_and(|p| p != s.proto) {
                    return None;
                }
                Some(fill_serv(&s, room))
            })
        })
    }
}

/// [`getservbyname_r`] into the calling thread's block.
///
/// # Safety
///
/// `name` is NUL-terminated; `proto` NULL or NUL-terminated.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn getservbyname(name: *const u8, proto: *const u8) -> *const Servent {
    nss_files::lookup_result(held(
        serv_held,
        // SAFETY: the caller's strings; the thread's block.
        |p, b, l, r| unsafe { getservbyname_r(name, proto, p, b, l, r) },
    ))
}

/// [`getservbyport_r`] into the calling thread's block.
///
/// # Safety
///
/// `proto` is NULL or NUL-terminated.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn getservbyport(port: i32, proto: *const u8) -> *const Servent {
    nss_files::lookup_result(held(
        serv_held,
        // SAFETY: the caller's string; the thread's block.
        |p, b, l, r| unsafe { getservbyport_r(port, proto, p, b, l, r) },
    ))
}

/// The calling thread's next service: 0, `ENOENT` at the end, `ERANGE`.
///
/// # Safety
///
/// `result_buf` and `result` writable; `buf` writable for `buflen` bytes.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn getservent_r(
    result_buf: *mut Servent,
    buf: *mut u8,
    buflen: usize,
    result: *mut *const Servent,
) -> i32 {
    // SAFETY: the caller's pointers; `db` is the thread's live block.
    unsafe {
        next_entry(
            serv_cur,
            Which::Services,
            SERVICES,
            |line, room| parse_serv(line).map(|s| fill_serv(&s, room)),
            result_buf,
            buf,
            buflen,
            result,
        )
    }
}

/// The calling thread's next service, or NULL at the end.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn getservent() -> *const Servent {
    nss_files::enumerated(held(
        serv_ent_held,
        // SAFETY: the thread's block.
        |p, b, l, r| unsafe { getservent_r(p, b, l, r) },
    ))
}

/// Start the calling thread's enumeration of the services again.
/// `stayopen` changes nothing: every lookup reads the file afresh, as
/// glibc's `nss_files` does.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn setservent(_stayopen: i32) {
    close_cursor(serv_cur);
}

/// End the calling thread's enumeration of the services.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn endservent() {
    close_cursor(serv_cur);
}

/// Close one of the calling thread's cursors: `set*ent` and `end*ent` both,
/// since the next `get*ent` reads the file again from its start.
pub(crate) fn close_cursor(cursor: fn(*mut ThreadDb) -> *mut Cursor) {
    // SAFETY: the calling thread's block, touched by no other thread.
    let p = unsafe { (*crate::perthread::current()).netdb }.cast::<ThreadDb>();
    if p.is_null() {
        // Nothing opened, nothing to close.
        return;
    }
    // SAFETY: `p` is the thread's live `ThreadDb`.
    unsafe { (*cursor(p)).close() };
}

// ---------------------------------------------------------------------------
// Protocols
// ---------------------------------------------------------------------------

/// A `/etc/protocols` or `/etc/rpc` line: `name number aliases...`.
struct NumberedLine<'a> {
    name: &'a [u8],
    number: i32,
    aliases: Line<'a>,
}

/// `files-proto.c`'s parser, and `files-rpc.c`'s, which is the same: the
/// number is decimal and followed by white space or the end.
fn parse_numbered(line: &[u8]) -> Option<NumberedLine<'_>> {
    let mut l = Line::new(line);
    let name = l.string();
    // `p_proto` and `r_number` are `int`s: 32 bits, as the file's number
    // clamped.
    let number = l.int(space, true, 10)? as i32;
    Some(NumberedLine {
        name,
        number,
        aliases: l,
    })
}

fn fill_proto(p: &NumberedLine<'_>, room: &mut Room) -> Result<Protoent, i32> {
    Ok(Protoent {
        p_name: room.string(p.name)?,
        p_proto: p.number,
        p_aliases: fill_list(
            room,
            Line {
                rest: p.aliases.rest,
            }
            .words(),
        )?,
    })
}

/// Look up a protocol by name or alias: glibc's `getprotobyname_r`.
///
/// # Safety
///
/// `name` is NUL-terminated; `result_buf` and `result` writable; `buf`
/// writable for `buflen` bytes.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn getprotobyname_r(
    name: *const u8,
    result_buf: *mut Protoent,
    buf: *mut u8,
    buflen: usize,
    result: *mut *const Protoent,
) -> i32 {
    // SAFETY: the caller's pointers, checked non-null by `reentrant`.
    unsafe {
        nss_files::reentrant(result_buf, buf, buflen, result, |room| {
            if name.is_null() {
                return Ok(None);
            }
            let name = bytes(name);
            scan(Which::Protocols, PROTOCOLS, |line| {
                let p = parse_numbered(line)?;
                names_match(name, p.name, &p.aliases).then(|| fill_proto(&p, room))
            })
        })
    }
}

/// Look up a protocol by number: glibc's `getprotobynumber_r`.
///
/// # Safety
///
/// As [`getprotobyname_r`].
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn getprotobynumber_r(
    proto: i32,
    result_buf: *mut Protoent,
    buf: *mut u8,
    buflen: usize,
    result: *mut *const Protoent,
) -> i32 {
    // SAFETY: the caller's pointers, checked non-null by `reentrant`.
    unsafe {
        nss_files::reentrant(result_buf, buf, buflen, result, |room| {
            scan(Which::Protocols, PROTOCOLS, |line| {
                let p = parse_numbered(line)?;
                (p.number == proto).then(|| fill_proto(&p, room))
            })
        })
    }
}

/// [`getprotobyname_r`] into the calling thread's block.
///
/// # Safety
///
/// `name` is NUL-terminated.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn getprotobyname(name: *const u8) -> *const Protoent {
    nss_files::lookup_result(held(
        proto_held,
        // SAFETY: the caller's string; the thread's block.
        |p, b, l, r| unsafe { getprotobyname_r(name, p, b, l, r) },
    ))
}

/// [`getprotobynumber_r`] into the calling thread's block.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn getprotobynumber(proto: i32) -> *const Protoent {
    nss_files::lookup_result(held(
        proto_held,
        // SAFETY: the thread's block.
        |p, b, l, r| unsafe { getprotobynumber_r(proto, p, b, l, r) },
    ))
}

/// The calling thread's next protocol: 0, `ENOENT` at the end, `ERANGE`.
///
/// # Safety
///
/// `result_buf` and `result` writable; `buf` writable for `buflen` bytes.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn getprotoent_r(
    result_buf: *mut Protoent,
    buf: *mut u8,
    buflen: usize,
    result: *mut *const Protoent,
) -> i32 {
    // SAFETY: the caller's pointers; `db` is the thread's live block.
    unsafe {
        next_entry(
            proto_cur,
            Which::Protocols,
            PROTOCOLS,
            |line, room| parse_numbered(line).map(|p| fill_proto(&p, room)),
            result_buf,
            buf,
            buflen,
            result,
        )
    }
}

/// The calling thread's next protocol, or NULL at the end.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn getprotoent() -> *const Protoent {
    nss_files::enumerated(held(
        proto_ent_held,
        // SAFETY: the thread's block.
        |p, b, l, r| unsafe { getprotoent_r(p, b, l, r) },
    ))
}

/// Start the calling thread's enumeration of the protocols again.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn setprotoent(_stayopen: i32) {
    close_cursor(proto_cur);
}

/// End the calling thread's enumeration of the protocols.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn endprotoent() {
    close_cursor(proto_cur);
}

// ---------------------------------------------------------------------------
// RPC programs: <rpc/netdb.h>
// ---------------------------------------------------------------------------

fn fill_rpc(p: &NumberedLine<'_>, room: &mut Room) -> Result<Rpcent, i32> {
    Ok(Rpcent {
        r_name: room.string(p.name)?,
        r_aliases: fill_list(
            room,
            Line {
                rest: p.aliases.rest,
            }
            .words(),
        )?,
        r_number: p.number,
    })
}

/// Look up an RPC program by name or alias: glibc's `getrpcbyname_r`.
///
/// # Safety
///
/// `name` is NUL-terminated; `result_buf` and `result` writable; `buf`
/// writable for `buflen` bytes.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn getrpcbyname_r(
    name: *const u8,
    result_buf: *mut Rpcent,
    buf: *mut u8,
    buflen: usize,
    result: *mut *const Rpcent,
) -> i32 {
    // SAFETY: the caller's pointers, checked non-null by `reentrant`.
    unsafe {
        nss_files::reentrant(result_buf, buf, buflen, result, |room| {
            if name.is_null() {
                return Ok(None);
            }
            let name = bytes(name);
            scan(Which::Rpc, RPC, |line| {
                let p = parse_numbered(line)?;
                names_match(name, p.name, &p.aliases).then(|| fill_rpc(&p, room))
            })
        })
    }
}

/// Look up an RPC program by number: glibc's `getrpcbynumber_r`.
///
/// # Safety
///
/// As [`getrpcbyname_r`].
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn getrpcbynumber_r(
    number: i32,
    result_buf: *mut Rpcent,
    buf: *mut u8,
    buflen: usize,
    result: *mut *const Rpcent,
) -> i32 {
    // SAFETY: the caller's pointers, checked non-null by `reentrant`.
    unsafe {
        nss_files::reentrant(result_buf, buf, buflen, result, |room| {
            scan(Which::Rpc, RPC, |line| {
                let p = parse_numbered(line)?;
                (p.number == number).then(|| fill_rpc(&p, room))
            })
        })
    }
}

/// [`getrpcbyname_r`] into the calling thread's block.
///
/// # Safety
///
/// `name` is NUL-terminated.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn getrpcbyname(name: *const u8) -> *const Rpcent {
    nss_files::lookup_result(held(
        rpc_held,
        // SAFETY: the caller's string; the thread's block.
        |p, b, l, r| unsafe { getrpcbyname_r(name, p, b, l, r) },
    ))
}

/// [`getrpcbynumber_r`] into the calling thread's block.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn getrpcbynumber(number: i32) -> *const Rpcent {
    nss_files::lookup_result(held(
        rpc_held,
        // SAFETY: the thread's block.
        |p, b, l, r| unsafe { getrpcbynumber_r(number, p, b, l, r) },
    ))
}

/// The calling thread's next RPC program: 0, `ENOENT` at the end,
/// `ERANGE`.
///
/// # Safety
///
/// `result_buf` and `result` writable; `buf` writable for `buflen` bytes.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn getrpcent_r(
    result_buf: *mut Rpcent,
    buf: *mut u8,
    buflen: usize,
    result: *mut *const Rpcent,
) -> i32 {
    // SAFETY: the caller's pointers; `db` is the thread's live block.
    unsafe {
        next_entry(
            rpc_cur,
            Which::Rpc,
            RPC,
            |line, room| parse_numbered(line).map(|p| fill_rpc(&p, room)),
            result_buf,
            buf,
            buflen,
            result,
        )
    }
}

/// The calling thread's next RPC program, or NULL at the end.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn getrpcent() -> *const Rpcent {
    nss_files::enumerated(held(
        rpc_ent_held,
        // SAFETY: the thread's block.
        |p, b, l, r| unsafe { getrpcent_r(p, b, l, r) },
    ))
}

/// Start the calling thread's enumeration of the RPC programs again.
/// `stayopen` changes nothing, as for the other databases.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn setrpcent(_stayopen: i32) {
    close_cursor(rpc_cur);
}

/// End the calling thread's enumeration of the RPC programs.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn endrpcent() {
    close_cursor(rpc_cur);
}

// ---------------------------------------------------------------------------
// Networks
// ---------------------------------------------------------------------------

/// A `/etc/networks` line: `name number aliases...`.
struct NetLine<'a> {
    name: &'a [u8],
    net: u32,
    aliases: Line<'a>,
}

/// `files-network.c`'s parser: the number is padded with `.0` to four
/// parts and read by `inet_network` -- which may answer `INADDR_NONE`,
/// still an entry.
fn parse_net(line: &[u8]) -> NetLine<'_> {
    let mut l = Line::new(line);
    let name = l.string();
    let addr = l.string();
    let dots = addr
        .iter()
        .fold(0usize, |n, &b| n + usize::from(b == b'.'))
        .min(3);
    let mut padded = [0u8; 64];
    let mut n = addr.len();
    let net = match padded.get_mut(..n) {
        Some(head) => {
            head.copy_from_slice(addr);
            let mut fits = true;
            for _ in dots..3 {
                match padded.get_mut(n..n + 2) {
                    Some(two) => two.copy_from_slice(b".0"),
                    None => fits = false,
                }
                n += 2;
            }
            if fits {
                crate::inet::network(padded.get(..n).unwrap_or(&[]))
            } else {
                crate::inet::INADDR_NONE
            }
        }
        // Too long to be a network number at all.
        None => crate::inet::INADDR_NONE,
    };
    NetLine {
        name,
        net,
        aliases: l,
    }
}

fn fill_net(n: &NetLine<'_>, room: &mut Room) -> Result<Netent, i32> {
    Ok(Netent {
        n_name: room.string(n.name)?,
        n_aliases: fill_list(
            room,
            Line {
                rest: n.aliases.rest,
            }
            .words(),
        )?,
        n_addrtype: crate::socket::AF_INET,
        n_net: n.net,
    })
}

/// The networks' `_r` shape: [`nss_files::reentrant`], with `*h_errnop`
/// `HOST_NOT_FOUND` when nothing is found, as `files-network.c` reports it.
///
/// # Safety
///
/// As [`nss_files::reentrant`]; `h_errnop` NULL or writable.
unsafe fn net_reentrant(
    result_buf: *mut Netent,
    buf: *mut u8,
    buflen: usize,
    result: *mut *const Netent,
    h_errnop: *mut i32,
    find: impl FnOnce(&mut Room) -> Result<Option<Netent>, i32>,
) -> i32 {
    let mut missed = false;
    // SAFETY: the caller's contract.
    let rc = unsafe {
        nss_files::reentrant(result_buf, buf, buflen, result, |room| {
            let r = find(room);
            missed = matches!(r, Ok(None));
            r
        })
    };
    if missed && !h_errnop.is_null() {
        // SAFETY: non-null, and writable by contract.
        unsafe { h_errnop.write(crate::socket::HOST_NOT_FOUND) };
    }
    rc
}

/// Look up a network by name or alias, ignoring case: glibc's
/// `getnetbyname_r`.
///
/// # Safety
///
/// `name` is NUL-terminated; `result_buf` and `result` writable; `buf`
/// writable for `buflen` bytes; `h_errnop` NULL or writable.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn getnetbyname_r(
    name: *const u8,
    result_buf: *mut Netent,
    buf: *mut u8,
    buflen: usize,
    result: *mut *const Netent,
    h_errnop: *mut i32,
) -> i32 {
    // SAFETY: the caller's pointers.
    unsafe {
        net_reentrant(result_buf, buf, buflen, result, h_errnop, |room| {
            if name.is_null() {
                return Ok(None);
            }
            let name = bytes(name);
            scan(Which::Networks, NETWORKS, |line| {
                let n = parse_net(line);
                let hit = eq_ignore_case(n.name, name)
                    || Line {
                        rest: n.aliases.rest,
                    }
                    .words()
                    .any(|a| eq_ignore_case(a, name));
                hit.then(|| fill_net(&n, room))
            })
        })
    }
}

/// Look up a network by number (host byte order) and family (`AF_UNSPEC`
/// for any): glibc's `getnetbyaddr_r`.
///
/// # Safety
///
/// As [`getnetbyname_r`].
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn getnetbyaddr_r(
    net: u32,
    ty: i32,
    result_buf: *mut Netent,
    buf: *mut u8,
    buflen: usize,
    result: *mut *const Netent,
    h_errnop: *mut i32,
) -> i32 {
    // SAFETY: the caller's pointers.
    unsafe {
        net_reentrant(result_buf, buf, buflen, result, h_errnop, |room| {
            scan(Which::Networks, NETWORKS, |line| {
                let n = parse_net(line);
                let family_ok = ty == crate::socket::AF_UNSPEC || ty == crate::socket::AF_INET;
                (family_ok && n.net == net).then(|| fill_net(&n, room))
            })
        })
    }
}

/// A network lookup into the calling thread's block, reporting "not
/// found" through `h_errno` as glibc's `getnetbyname` does.
fn net_held(
    call: impl Fn(*mut Netent, *mut u8, usize, *mut *const Netent, *mut i32) -> i32,
) -> *const Netent {
    let mut herr = 0i32;
    let r = held(net_held_block, |p, b, l, r| call(p, b, l, r, &raw mut herr));
    if matches!(r, Ok(p) if p.is_null()) {
        crate::socket::set_h_errno(herr);
    }
    nss_files::lookup_result(r)
}

/// [`getnetbyname_r`] into the calling thread's block.
///
/// # Safety
///
/// `name` is NUL-terminated.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn getnetbyname(name: *const u8) -> *const Netent {
    // SAFETY: the caller's string.
    net_held(|p, b, l, r, h| unsafe { getnetbyname_r(name, p, b, l, r, h) })
}

/// [`getnetbyaddr_r`] into the calling thread's block.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn getnetbyaddr(net: u32, ty: i32) -> *const Netent {
    // SAFETY: the thread's block.
    net_held(|p, b, l, r, h| unsafe { getnetbyaddr_r(net, ty, p, b, l, r, h) })
}

/// The calling thread's next network: 0, `ENOENT` at the end (with
/// `*h_errnop` `HOST_NOT_FOUND`), `ERANGE`.
///
/// # Safety
///
/// `result_buf` and `result` writable; `buf` writable for `buflen` bytes;
/// `h_errnop` NULL or writable.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn getnetent_r(
    result_buf: *mut Netent,
    buf: *mut u8,
    buflen: usize,
    result: *mut *const Netent,
    h_errnop: *mut i32,
) -> i32 {
    // SAFETY: the caller's pointers; `db` is the thread's live block.
    let rc = unsafe {
        next_entry(
            net_cur,
            Which::Networks,
            NETWORKS,
            |line, room| Some(fill_net(&parse_net(line), room)),
            result_buf,
            buf,
            buflen,
            result,
        )
    };
    if rc == errno::ENOENT && !h_errnop.is_null() {
        // SAFETY: non-null, and writable by contract.
        unsafe { h_errnop.write(crate::socket::HOST_NOT_FOUND) };
    }
    rc
}

/// The calling thread's next network, or NULL at the end.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn getnetent() -> *const Netent {
    let mut herr = 0i32;
    let r = held(
        net_ent_held,
        // SAFETY: the thread's block.
        |p, b, l, r| unsafe { getnetent_r(p, b, l, r, &raw mut herr) },
    );
    if matches!(r, Err(e) if e == errno::ENOENT) {
        crate::socket::set_h_errno(herr);
    }
    nss_files::enumerated(r)
}

/// Start the calling thread's enumeration of the networks again.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn setnetent(_stayopen: i32) {
    close_cursor(net_cur);
}

/// End the calling thread's enumeration of the networks.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn endnetent() {
    close_cursor(net_cur);
}

// ---------------------------------------------------------------------------
// Ethers
// ---------------------------------------------------------------------------

/// An `/etc/ethers` line: `xx:xx:xx:xx:xx:xx name`.
struct EtherLine<'a> {
    addr: [u8; 6],
    name: &'a [u8],
}

/// `files-ethers.c`'s parser: six hexadecimal numbers of at most `ff`, the
/// first five each followed by one `:`, the sixth by white space or the
/// end; then the name.
fn parse_ether(line: &[u8]) -> Option<EtherLine<'_>> {
    let mut l = Line::new(line);
    let mut addr = [0u8; 6];
    for (i, slot) in addr.iter_mut().enumerate() {
        let n = if i < 5 {
            l.int(|b| b == b':', false, 16)?
        } else {
            l.int(space, true, 16)?
        };
        *slot = u8::try_from(n).ok()?;
    }
    Some(EtherLine {
        addr,
        name: l.string(),
    })
}

/// The Ethernet address `/etc/ethers` gives `hostname` (ignoring case):
/// 0, or -1 when there is none.
///
/// # Safety
///
/// `hostname` is NUL-terminated; `addr` writable.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn ether_hostton(hostname: *const u8, addr: *mut EtherAddr) -> i32 {
    if hostname.is_null() || addr.is_null() {
        return -1;
    }
    // SAFETY: the caller's string.
    let name = unsafe { bytes(hostname) };
    let found = scan(Which::Ethers, b"", |line| {
        let e = parse_ether(line)?;
        eq_ignore_case(e.name, name).then_some(Ok(e.addr))
    });
    match found {
        Ok(Some(a)) => {
            // SAFETY: `addr` is writable by contract.
            unsafe { (*addr).ether_addr_octet = a };
            0
        }
        _ => -1,
    }
}

/// The host name `/etc/ethers` gives `addr`, copied into `hostname`, which
/// must be large enough -- the interface gives no size, as glibc's
/// comment laments: 0, or -1 when there is none.
///
/// # Safety
///
/// `addr` readable; `hostname` writable for the name and its NUL.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn ether_ntohost(hostname: *mut u8, addr: *const EtherAddr) -> i32 {
    if hostname.is_null() || addr.is_null() {
        return -1;
    }
    // SAFETY: `addr` is readable by contract.
    let want = unsafe { (*addr).ether_addr_octet };
    let mut out = hostname;
    let found = scan(Which::Ethers, b"", |line| {
        let e = parse_ether(line)?;
        if e.addr != want {
            return None;
        }
        // SAFETY: `hostname` holds the name and its NUL by contract.
        unsafe {
            core::ptr::copy_nonoverlapping(e.name.as_ptr(), out, e.name.len());
            out = out.add(e.name.len());
            out.write(0);
        }
        Some(Ok(()))
    });
    if matches!(found, Ok(Some(()))) { 0 } else { -1 }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nss_files::{set_test_error, set_test_text};
    use std::format;
    use std::string::String;
    use std::vec::Vec;

    fn text(p: *const u8) -> String {
        // SAFETY: the functions under test hand back NUL-terminated strings.
        String::from_utf8_lossy(unsafe { bytes(p) }).into_owned()
    }

    fn list(l: *const *const u8) -> String {
        let mut v = Vec::new();
        let mut i = 0;
        // SAFETY: a NULL-terminated array of strings, or NULL.
        while !l.is_null() && !unsafe { *l.add(i) }.is_null() {
            v.push(text(unsafe { *l.add(i) }));
            i += 1;
        }
        format!(" aliases=[{}]", v.join(","))
    }

    fn serv(s: *const Servent, rc: i32) -> String {
        if s.is_null() {
            return format!("NULL rc={rc}");
        }
        // SAFETY: a non-null answer from the functions under test.
        let s = unsafe { &*s };
        format!(
            "name={} port={} proto={}{}",
            text(s.s_name),
            u16::from_be(s.s_port as u16),
            text(s.s_proto),
            list(s.s_aliases)
        )
    }

    fn proto(p: *const Protoent, rc: i32) -> String {
        if p.is_null() {
            return format!("NULL rc={rc}");
        }
        // SAFETY: as in `serv`.
        let p = unsafe { &*p };
        format!(
            "name={} proto={}{}",
            text(p.p_name),
            p.p_proto,
            list(p.p_aliases)
        )
    }

    fn net(n: *const Netent, rc: i32, herr: i32) -> String {
        if n.is_null() {
            return format!("NULL rc={rc} herr={herr}");
        }
        // SAFETY: as in `serv`.
        let n = unsafe { &*n };
        format!(
            "name={} type={} net={:08x}{}",
            text(n.n_name),
            n.n_addrtype,
            n.n_net,
            list(n.n_aliases)
        )
    }

    /// NUL-terminated copy, or NULL for "-".
    fn arg(s: &str) -> Option<Vec<u8>> {
        (s != "-").then(|| {
            let mut v = s.as_bytes().to_vec();
            v.push(0);
            v
        })
    }

    fn ptr(a: Option<&Vec<u8>>) -> *const u8 {
        a.map_or(core::ptr::null(), |v| v.as_ptr())
    }

    fn c(s: &str) -> Vec<u8> {
        arg(s).unwrap()
    }

    /// `strtol (s, NULL, 0)`.
    fn strtol0(s: &str) -> i64 {
        let (neg, t) = s.strip_prefix('-').map_or((false, s), |t| (true, t));
        let v = if let Some(h) = t.strip_prefix("0x") {
            i64::from_str_radix(h, 16).unwrap()
        } else {
            t.parse().unwrap()
        };
        if neg { -v } else { v }
    }

    /// What `netdb_oracle.c` prints for `cmd`, from this crate's functions.
    #[allow(clippy::too_many_lines)] // one arm per command, as the oracle has
    fn run(cmd: &str, out: &mut Vec<String>) {
        let a: Vec<&str> = cmd.split(' ').collect();
        let mut buf = std::vec![0u8; 8192];
        let b = buf.as_mut_ptr();
        // SAFETY (every call below): NUL-terminated strings, and outputs and
        // a buffer this function owns.
        unsafe {
            match a[0] {
                "serv" => {
                    let (n, p) = (c(a[1]), arg(a[2]));
                    let mut sb = Servent::EMPTY;
                    let mut r: *const Servent = core::ptr::null();
                    let rc = getservbyname_r(n.as_ptr(), ptr(p.as_ref()), &mut sb, b, 8192, &mut r);
                    out.push(serv(r, rc));
                }
                "port" | "rawport" => {
                    let p = arg(a[2]);
                    let port = if a[0] == "port" {
                        i32::from((a[1].parse::<u16>().unwrap()).to_be())
                    } else {
                        strtol0(a[1]) as i32
                    };
                    let mut sb = Servent::EMPTY;
                    let mut r: *const Servent = core::ptr::null();
                    let rc = getservbyport_r(port, ptr(p.as_ref()), &mut sb, b, 8192, &mut r);
                    out.push(serv(r, rc));
                }
                "servsmall" => {
                    let (n, p) = (c(a[1]), arg(a[2]));
                    let mut sb = Servent::EMPTY;
                    let mut r: *const Servent = core::ptr::null();
                    let size = a[3].parse().unwrap();
                    let rc = getservbyname_r(n.as_ptr(), ptr(p.as_ref()), &mut sb, b, size, &mut r);
                    out.push(format!("rc={rc} found={}", i32::from(!r.is_null())));
                }
                "servent" => {
                    setservent(0);
                    loop {
                        let s = getservent();
                        if s.is_null() {
                            break;
                        }
                        out.push(serv(s, 0));
                    }
                    out.push(String::from("end"));
                    endservent();
                }
                "mix" => {
                    let n = c(a[1]);
                    setservent(0);
                    out.push(serv(getservent(), 0));
                    out.push(serv(getservent(), 0));
                    out.push(serv(getservbyname(n.as_ptr(), core::ptr::null()), 0));
                    out.push(serv(getservent(), 0));
                    endservent();
                }
                "proto" => {
                    let n = c(a[1]);
                    let mut pb = Protoent::EMPTY;
                    let mut r: *const Protoent = core::ptr::null();
                    let rc = getprotobyname_r(n.as_ptr(), &mut pb, b, 8192, &mut r);
                    out.push(proto(r, rc));
                }
                "protonum" => {
                    let mut pb = Protoent::EMPTY;
                    let mut r: *const Protoent = core::ptr::null();
                    let rc = getprotobynumber_r(strtol0(a[1]) as i32, &mut pb, b, 8192, &mut r);
                    out.push(proto(r, rc));
                }
                "protoent" => {
                    setprotoent(0);
                    loop {
                        let p = getprotoent();
                        if p.is_null() {
                            break;
                        }
                        out.push(proto(p, 0));
                    }
                    out.push(String::from("end"));
                    endprotoent();
                }
                "net" => {
                    let n = c(a[1]);
                    let mut nb = Netent::EMPTY;
                    let mut r: *const Netent = core::ptr::null();
                    let mut herr = -99;
                    let rc = getnetbyname_r(n.as_ptr(), &mut nb, b, 8192, &mut r, &mut herr);
                    out.push(net(r, rc, herr));
                }
                "netaddr" => {
                    let mut nb = Netent::EMPTY;
                    let mut r: *const Netent = core::ptr::null();
                    let mut herr = -99;
                    let n = u32::from_str_radix(a[1], 16).unwrap();
                    let ty = a[2].parse().unwrap();
                    let rc = getnetbyaddr_r(n, ty, &mut nb, b, 8192, &mut r, &mut herr);
                    out.push(net(r, rc, herr));
                }
                "netent" => {
                    setnetent(0);
                    loop {
                        let n = getnetent();
                        if n.is_null() {
                            break;
                        }
                        out.push(net(n, 0, 0));
                    }
                    out.push(String::from("end"));
                    endnetent();
                }
                "ethh" => {
                    let n = c(a[1]);
                    let mut e = EtherAddr {
                        ether_addr_octet: [0xaa; 6],
                    };
                    let rc = ether_hostton(n.as_ptr(), &mut e);
                    let shown = if rc == 0 {
                        text(crate::inet::ether_ntoa(&e))
                    } else {
                        String::from("-")
                    };
                    out.push(format!("rc={rc} addr={shown}"));
                }
                "ethn" => {
                    let n = c(a[1]);
                    let mut e = EtherAddr::ZERO;
                    let p = crate::inet::ether_aton_r(n.as_ptr(), &mut e);
                    let mut host = std::vec![0u8; 1100];
                    host[0] = b'-';
                    let rc = if p.is_null() {
                        -2
                    } else {
                        ether_ntohost(host.as_mut_ptr(), &e)
                    };
                    out.push(format!("rc={rc} host={}", text(host.as_ptr())));
                }
                other => panic!("unknown command {other}"),
            }
        }
    }

    fn with_files() {
        set_test_text(Which::Services, Some(SERVICES_FILE));
        set_test_text(Which::Protocols, Some(PROTOCOLS_FILE));
        set_test_text(Which::Networks, Some(NETWORKS_FILE));
        set_test_text(Which::Ethers, Some(ETHERS_FILE));
    }

    /// Commands whose answer turns on exactly where glibc's `nss_files`
    /// lays a line out in the caller's buffer -- it copies the whole line
    /// there -- so a buffer it finds too small may be enough here.
    /// `ERANGE`'s threshold is not the interface.
    const LAYOUT_DEPENDENT: &[&str] = &["servsmall http tcp 60"];

    #[test]
    fn every_answer_is_glibcs() {
        with_files();
        let mut ours = Vec::new();
        let mut glibc = GLIBC_OUTPUT.lines();
        let mut want: Vec<&str> = Vec::new();
        for cmd in COMMANDS.lines() {
            let before = ours.len();
            run(cmd, &mut ours);
            let theirs: Vec<&str> = (before..ours.len()).filter_map(|_| glibc.next()).collect();
            if LAYOUT_DEPENDENT.contains(&cmd) {
                ours.truncate(before);
            } else {
                want.extend(theirs);
            }
        }
        let mut wrong = Vec::new();
        for i in 0..want.len().max(ours.len()) {
            let (w, o) = (want.get(i).copied(), ours.get(i).map(String::as_str));
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
    }

    /// A number past 32 bits -- a negative one among them, which `strtoull`
    /// negates as unsigned -- makes the line no entry, as Debian's glibc has
    /// it (its `local-nss-overflow.diff`, carried by the oracle's WSL glibc);
    /// upstream glibc would clamp it to `0xffffffff` (design-decisions
    /// §1136).
    #[test]
    fn a_number_past_32_bits_makes_no_entry_as_debians_glibc_has_it() {
        set_test_text(
            Which::Services,
            Some(b"neg -1/tcp\nbig 99999999999/tcp\nok 7/tcp\n"),
        );
        set_test_text(
            Which::Protocols,
            Some(b"neg -1 NEG\nbig 99999999999 BIG\nok 7 OK\n"),
        );
        let mut out = Vec::new();
        run("serv neg -", &mut out);
        run("serv big -", &mut out);
        run("proto neg", &mut out);
        run("protonum -1", &mut out);
        run("proto big", &mut out);
        for (i, line) in out.iter().enumerate() {
            assert!(line.starts_with("NULL"), "{i}: {line}");
        }
        let mut out = Vec::new();
        run("serv ok -", &mut out);
        run("proto ok", &mut out);
        assert_eq!(
            out,
            [
                "name=ok port=7 proto=tcp aliases=[]",
                "name=ok proto=7 aliases=[OK]"
            ]
        );
        assert_eq!(strtou32(b"-0", 10), Some((0, 2)));
        assert_eq!(strtou32(b"4294967295", 10), Some((u32::MAX, 10)));
        assert_eq!(strtou32(b"4294967296", 10), None);
        assert_eq!(strtou32(b"-1", 10), None);
        assert_eq!(strtou32(b"-99999999999999999999999", 10), None);
        assert_eq!(strtou32(b"0xffffffff", 0), Some((u32::MAX, 10)));
        assert_eq!(strtou32(b"0x100000000", 0), None);
        assert_eq!(strtou32(b"0x1f/", 0), Some((31, 4)));
        assert_eq!(strtou32(b"010", 0), Some((8, 3)));
        assert_eq!(
            strtou32(b"0x", 16),
            Some((0, 1)),
            "no hex digit: the 0 alone"
        );
        assert_eq!(strtou32(b"  +7", 10), Some((7, 4)));
        assert_eq!(strtou32(b"x", 10), None);
    }

    #[test]
    fn a_missing_file_answers_from_the_built_in_copy() {
        for w in [
            Which::Services,
            Which::Protocols,
            Which::Networks,
            Which::Ethers,
        ] {
            set_test_text(w, None);
        }
        let mut out = Vec::new();
        for cmd in [
            "serv http tcp",
            "serv www -",
            "port 443 tcp",
            "serv domain udp",
            "proto tcp",
            "protonum 17",
            "net link-local",
            "ethh anything",
        ] {
            run(cmd, &mut out);
        }
        assert_eq!(
            out,
            [
                "name=http port=80 proto=tcp aliases=[www]",
                "name=http port=80 proto=tcp aliases=[www]",
                "name=https port=443 proto=tcp aliases=[]",
                "name=domain port=53 proto=udp aliases=[]",
                "name=tcp proto=6 aliases=[TCP]",
                "name=udp proto=17 aliases=[UDP]",
                "name=link-local type=2 net=a9fe0000 aliases=[]",
                "rc=-1 addr=-",
            ]
        );
    }

    #[test]
    fn every_built_in_line_parses() {
        let serv_ok: fn(&[u8]) -> bool = |l| parse_serv(l).is_some();
        let numbered_ok: fn(&[u8]) -> bool = |l| parse_numbered(l).is_some();
        for (name, text, parse) in [
            ("services", SERVICES, serv_ok),
            ("protocols", PROTOCOLS, numbered_ok),
            ("rpc", RPC, numbered_ok),
        ] {
            for (line, _) in nss_files::lines(text, 0) {
                assert!(parse(line), "{name}: {:?}", String::from_utf8_lossy(line));
            }
        }
        assert_eq!(parse_net(b"link-local 169.254.0.0").net, 0xa9fe_0000);
    }

    #[test]
    fn an_unreadable_file_is_its_error_not_a_miss() {
        set_test_error(Which::Services, errno::EACCES);
        let mut sb = Servent::EMPTY;
        let mut r: *const Servent = core::ptr::null();
        let mut buf = [0u8; 256];
        // SAFETY: NUL-terminated name; outputs this test owns.
        let rc = unsafe {
            getservbyname_r(
                c"http".as_ptr().cast(),
                core::ptr::null(),
                &mut sb,
                buf.as_mut_ptr(),
                256,
                &mut r,
            )
        };
        assert_eq!(rc, errno::EACCES);
        assert!(r.is_null());
        // SAFETY: NUL-terminated name.
        assert!(unsafe { getservbyname(c"http".as_ptr().cast(), core::ptr::null()) }.is_null());
        assert_eq!(errno::get_errno(), errno::EACCES);
        // An enumeration just ends, with `errno` as it was: glibc's
        // `_nss_files_getservent_r` puts it back after the failed open.
        endservent();
        errno::set_errno(12345);
        assert!(getservent().is_null());
        assert_eq!(errno::get_errno(), 12345);
        // SAFETY: outputs this test owns.
        let rc = unsafe { getservent_r(&mut sb, buf.as_mut_ptr(), 256, &mut r) };
        assert_eq!(rc, errno::ENOENT);
        assert!(r.is_null());
        assert_eq!(errno::get_errno(), 12345);
        // The file is tried again: readable now, it is enumerated.
        set_test_text(Which::Services, Some(b"echo 7/tcp\n"));
        // SAFETY: outputs this test owns.
        let rc = unsafe { getservent_r(&mut sb, buf.as_mut_ptr(), 256, &mut r) };
        assert_eq!(serv(r, rc), "name=echo port=7 proto=tcp aliases=[]");
        endservent();
        set_test_text(Which::Services, None);
    }

    /// A buffer too small for the next entry is `ERANGE` in `errno` as well,
    /// as glibc's `getservent_r` and `getprotoent_r` have it; the entry comes
    /// again, and success and the end leave `errno` alone.
    #[test]
    fn an_enumeration_reports_erange_in_errno_too() {
        with_files();
        let mut sb = Servent::EMPTY;
        let mut sr: *const Servent = core::ptr::null();
        let mut pb = Protoent::EMPTY;
        let mut pr: *const Protoent = core::ptr::null();
        let mut small = [0u8; 4];
        let mut big = [0u8; 256];
        setservent(0);
        setprotoent(0);
        // SAFETY: outputs this test owns; each buffer's own size.
        unsafe {
            errno::set_errno(12345);
            let rc = getservent_r(&mut sb, small.as_mut_ptr(), 4, &mut sr);
            assert_eq!(
                (rc, sr.is_null(), errno::get_errno()),
                (errno::ERANGE, true, errno::ERANGE)
            );
            errno::set_errno(12345);
            let rc = getservent_r(&mut sb, big.as_mut_ptr(), 256, &mut sr);
            assert_eq!(
                serv(sr, rc),
                "name=http port=80 proto=tcp aliases=[www,www-http]"
            );
            assert_eq!(errno::get_errno(), 12345);
            errno::set_errno(12345);
            let rc = getprotoent_r(&mut pb, small.as_mut_ptr(), 4, &mut pr);
            assert_eq!(
                (rc, pr.is_null(), errno::get_errno()),
                (errno::ERANGE, true, errno::ERANGE)
            );
            errno::set_errno(12345);
            let rc = getprotoent_r(&mut pb, big.as_mut_ptr(), 256, &mut pr);
            assert_eq!(proto(pr, rc), "name=ip proto=0 aliases=[IP]");
            assert_eq!(errno::get_errno(), 12345);
            while getprotoent_r(&mut pb, big.as_mut_ptr(), 256, &mut pr) == 0 {}
            errno::set_errno(12345);
            let rc = getprotoent_r(&mut pb, big.as_mut_ptr(), 256, &mut pr);
            assert_eq!((rc, errno::get_errno()), (errno::ENOENT, 12345), "the end");
        }
        endservent();
        endprotoent();
    }

    #[test]
    fn the_non_reentrant_block_grows_for_a_long_entry() {
        let mut line = b"long 9/tcp".to_vec();
        for i in 0..400 {
            line.extend_from_slice(format!(" alias{i}").as_bytes());
        }
        line.push(b'\n');
        let leaked: &'static [u8] = Vec::leak(line);
        set_test_text(Which::Services, Some(leaked));
        // SAFETY: NUL-terminated name.
        let s = unsafe { getservbyname(c"alias399".as_ptr().cast(), core::ptr::null()) };
        assert!(
            !s.is_null(),
            "a 3 KiB entry fits once the block has doubled"
        );
        // SAFETY: a non-null answer.
        assert_eq!(text(unsafe { (*s).s_name }), "long");
        set_test_text(Which::Services, None);
    }

    #[test]
    fn enumerations_are_per_thread() {
        with_files();
        // This thread takes the first two services...
        setservent(0);
        let first = serv(getservent(), 0);
        let _ = getservent();
        // ...and another thread starts from the beginning regardless.
        let other = std::thread::spawn(|| {
            set_test_text(Which::Services, Some(SERVICES_FILE));
            setservent(0);
            let s = serv(getservent(), 0);
            endservent();
            thread_cleanup();
            s
        })
        .join()
        .unwrap();
        assert_eq!(other, first);
        // And this thread's third is the third.
        let third = serv(getservent(), 0);
        assert_eq!(third, "name=indented port=81 proto=tcp aliases=[]");
        endservent();
    }

    #[test]
    fn thread_cleanup_frees_and_forgets_the_block() {
        with_files();
        // SAFETY: NUL-terminated name.
        assert!(!unsafe { getprotobyname(c"tcp".as_ptr().cast()) }.is_null());
        setnetent(0);
        assert!(!getnetent().is_null());
        // SAFETY: the calling thread's block.
        assert!(!unsafe { (*crate::perthread::current()).netdb }.is_null());
        thread_cleanup();
        // SAFETY: as above.
        assert!(unsafe { (*crate::perthread::current()).netdb }.is_null());
        // A second cleanup, and a lookup after it, are both fine.
        thread_cleanup();
        // SAFETY: NUL-terminated name.
        assert!(!unsafe { getprotobyname(c"udp".as_ptr().cast()) }.is_null());
        thread_cleanup();
    }

    #[test]
    fn null_names_are_not_found() {
        let mut sb = Servent::EMPTY;
        let mut r: *const Servent = core::ptr::null();
        let mut buf = [0u8; 64];
        // SAFETY: NULL is the input under test; the outputs are this test's.
        unsafe {
            let rc = getservbyname_r(
                core::ptr::null(),
                core::ptr::null(),
                &mut sb,
                buf.as_mut_ptr(),
                64,
                &mut r,
            );
            assert_eq!((rc, r.is_null()), (0, true));
            assert!(getprotobyname(core::ptr::null()).is_null());
            assert!(getnetbyname(core::ptr::null()).is_null());
            assert!(getrpcbyname(core::ptr::null()).is_null());
            let mut e = EtherAddr::ZERO;
            assert_eq!(ether_hostton(core::ptr::null(), &mut e), -1);
        }
    }

    // <rpc/netdb.h>, replayed against posix/tools/oracle/rpc_harness.py's
    // program: its lines for each of its three runs.

    /// glibc 2.39's answers, and the files it read.
    const RPC_ORACLE: &str = include_str!("rpc_oracle.txt");

    /// The harness's names and numbers, in its order.
    const RPC_NAMES: &[&core::ffi::CStr] = &[
        c"portmapper",
        c"sunrpc",
        c"rpcbind",
        c"nfsprog",
        c"ypserv",
        c"ypprog",
        c"mountd",
        c"showmount",
        c"bad100006",
        c"nonum",
        c"hex",
        c"neg",
        c"trailing",
        c"big",
        c"wrapped",
        c"dup",
        c"again",
        c"nlockmgr",
        c"tabs",
        c"status",
        c"last",
        c"comment",
        c"a",
        c"missing",
        c"",
        c"rquota",
        c"keyserver",
        c"x25.inr",
        c"sgi_fam",
        c"amq",
        c"NFS",
    ];
    const RPC_NUMBERS: &[i32] = &[
        100_000,
        100_001,
        100_003,
        100_004,
        100_005,
        100_006,
        100_007,
        0,
        31,
        -5,
        1,
        100_021,
        100_024,
        100_099,
        42,
        100_227,
        150_001,
        391_002,
        545_580_417,
    ];

    /// The oracle's input `name`, its escapes undone.
    fn rpc_input(name: &str) -> Vec<u8> {
        let prefix = format!("input {name} = ");
        let line = RPC_ORACLE
            .lines()
            .find_map(|l| l.strip_prefix(prefix.as_str()))
            .unwrap();
        let mut out = Vec::new();
        let mut bytes = line.bytes();
        while let Some(b) = bytes.next() {
            if b != b'\\' {
                out.push(b);
                continue;
            }
            out.push(match bytes.next() {
                Some(b'n') => b'\n',
                Some(b't') => b'\t',
                Some(b'r') => b'\r',
                Some(b'\\') => b'\\',
                other => panic!("{name}: escape {other:?}"),
            });
        }
        out
    }

    /// The harness's `en`: errno's name, `kept` for its marker.
    fn rpc_en(e: i32) -> String {
        match e {
            0 => "0".into(),
            12345 => "kept".into(),
            errno::ENOENT => "ENOENT".into(),
            errno::ERANGE => "ERANGE".into(),
            errno::EINVAL => "EINVAL".into(),
            e => format!("e{e}"),
        }
    }

    /// The harness's `entry`.
    fn rpc_entry(r: &Rpcent) -> String {
        let mut aliases = Vec::new();
        for i in 0.. {
            // SAFETY: an answer's NULL-terminated array of strings, read up
            // to its NULL.
            let a = unsafe { *r.r_aliases.add(i) };
            if a.is_null() {
                break;
            }
            aliases.push(text(a));
        }
        format!("{}|{}|[{}]", text(r.r_name), r.r_number, aliases.join(","))
    }

    /// The harness's `show`, `errno` read now.
    fn rpc_show(run: &str, what: &str, r: *const Rpcent) -> String {
        let answer = if r.is_null() {
            "NULL".into()
        } else {
            // SAFETY: a non-null answer from the functions under test.
            rpc_entry(unsafe { &*r })
        };
        format!(
            "{run} {what} = {answer} errno={}",
            rpc_en(errno::get_errno())
        )
    }

    /// A `_r` form's answer as the harness prints it: the entry when it
    /// was delivered into `r`, else where `res` was left -- NULL, or
    /// `untouched` (still `unset`), or elsewhere.
    fn rpc_delivered(rc: i32, r: &Rpcent, res: *const Rpcent, unset: *const Rpcent) -> String {
        if rc == 0 && core::ptr::eq(res, r) {
            rpc_entry(r)
        } else if res.is_null() {
            "NULL".into()
        } else if core::ptr::eq(res, unset) {
            "untouched".into()
        } else {
            "other".into()
        }
    }

    /// The harness's `main`, call for call: the lines it prints as `run`.
    fn rpc_probes(run: &str) -> Vec<String> {
        let mut out = Vec::new();
        for n in 0..64 {
            errno::set_errno(12345);
            let r = getrpcent();
            out.push(rpc_show(run, &format!("getrpcent #{n}"), r));
            if r.is_null() {
                break;
            }
        }
        setrpcent(0);
        errno::set_errno(12345);
        out.push(rpc_show(run, "getrpcent after setrpcent(0)", getrpcent()));
        setrpcent(1);
        errno::set_errno(12345);
        out.push(rpc_show(run, "getrpcent after setrpcent(1)", getrpcent()));
        errno::set_errno(12345);
        out.push(rpc_show(run, "getrpcent after that", getrpcent()));
        endrpcent();
        errno::set_errno(12345);
        out.push(rpc_show(run, "getrpcent after endrpcent", getrpcent()));
        endrpcent();
        for name in RPC_NAMES {
            errno::set_errno(12345);
            // SAFETY: a NUL-terminated name.
            let r = unsafe { getrpcbyname(name.as_ptr().cast()) };
            let what = format!("getrpcbyname({})", name.to_str().unwrap());
            out.push(rpc_show(run, &what, r));
        }
        for &number in RPC_NUMBERS {
            errno::set_errno(12345);
            let r = getrpcbynumber(number);
            out.push(rpc_show(run, &format!("getrpcbynumber({number})"), r));
        }
        let unset = core::ptr::dangling::<Rpcent>();
        let mut buf = [0u8; 1024];
        for size in (0..=96).step_by(8) {
            for by_name in [true, false] {
                let mut r = Rpcent::EMPTY;
                let mut res = unset;
                errno::set_errno(12345);
                let b = buf.as_mut_ptr();
                // SAFETY: a NUL-terminated name; `size` bytes of `buf`,
                // which has 1024; outputs this test owns.
                let (what, rc) = unsafe {
                    if by_name {
                        let name = c"portmapper".as_ptr().cast();
                        let rc = getrpcbyname_r(name, &mut r, b, size, &mut res);
                        ("getrpcbyname_r(portmapper", rc)
                    } else {
                        let rc = getrpcbynumber_r(100_001, &mut r, b, size, &mut res);
                        ("getrpcbynumber_r(100001", rc)
                    }
                };
                out.push(format!(
                    "{run} {what}, {size}) = {} {} errno={}",
                    rpc_en(rc),
                    rpc_delivered(rc, &r, res, unset),
                    rpc_en(errno::get_errno())
                ));
            }
        }
        let mut r = Rpcent::EMPTY;
        let mut res = unset;
        errno::set_errno(12345);
        // SAFETY: a NUL-terminated name; the buffer's size; outputs this
        // test owns.
        let rc = unsafe {
            getrpcbyname_r(
                c"missing".as_ptr().cast(),
                &mut r,
                buf.as_mut_ptr(),
                buf.len(),
                &mut res,
            )
        };
        out.push(format!(
            "{run} getrpcbyname_r(missing) = {} {} errno={}",
            rpc_en(rc),
            if res.is_null() { "NULL" } else { "other" },
            rpc_en(errno::get_errno())
        ));
        setrpcent(0);
        for n in 0..64 {
            let mut r = Rpcent::EMPTY;
            let mut res = unset;
            errno::set_errno(12345);
            let len = if n == 1 { 8 } else { buf.len() };
            // SAFETY: `len` bytes of `buf`; outputs this test owns.
            let rc = unsafe { getrpcent_r(&mut r, buf.as_mut_ptr(), len, &mut res) };
            let answer = match rpc_delivered(rc, &r, res, unset) {
                a if a == "untouched" => "other".into(),
                a => a,
            };
            out.push(format!(
                "{run} getrpcent_r #{n} = {} {answer} errno={}",
                rpc_en(rc),
                rpc_en(errno::get_errno())
            ));
            if rc != 0 && rc != errno::ERANGE {
                break;
            }
        }
        endrpcent();
        out
    }

    /// Every probe of every run answers as glibc's did: `files` over the
    /// harness's test file; `builtin` with no file, so from [`RPC`] -- which
    /// glibc read as `/etc/rpc`; `none` with a file that cannot be opened
    /// (`ENOENT`), which glibc answers as it does no file.
    ///
    /// A `_r` lookup that glibc found a buffer too small for may fit here:
    /// `nss_files` copies the whole line into the buffer and takes it apart
    /// there, so where its `ERANGE` starts is its layout's, not the
    /// interface -- as `LAYOUT_DEPENDENT` above.  Such a probe must then give
    /// the entry glibc gives with room; and wherever glibc's fits, ours must.
    #[test]
    fn every_rpc_answer_is_glibcs() {
        let file: &'static [u8] = Vec::leak(rpc_input("rpc"));
        assert_eq!(
            String::from_utf8_lossy(&rpc_input("builtin")),
            String::from_utf8_lossy(RPC),
            "RPC is not the copy the oracle was made from: rerun rpc_harness.py"
        );
        let mut wrong = Vec::new();
        for run in ["files", "builtin", "none"] {
            match run {
                "files" => set_test_text(Which::Rpc, Some(file)),
                "builtin" => set_test_text(Which::Rpc, None),
                _ => set_test_error(Which::Rpc, errno::ENOENT),
            }
            let want: Vec<&str> = RPC_ORACLE
                .lines()
                .filter(|l| l.strip_prefix(run).is_some_and(|r| r.starts_with(' ')))
                .collect();
            let ours = rpc_probes(run);
            // glibc's answer to each sized lookup at the largest size.
            let roomy = |line: &str| -> Option<String> {
                let (probe, _) = line.split_once(", ")?;
                let last = format!("{probe}, 96) = ");
                want.iter()
                    .find_map(|w| w.strip_prefix(last.as_str()).map(String::from))
            };
            for i in 0..want.len().max(ours.len()) {
                let (w, o) = (want.get(i).copied(), ours.get(i).map(String::as_str));
                let layout = match (w, o) {
                    (Some(w), Some(o)) if w.ends_with("= ERANGE NULL errno=ERANGE") => {
                        let (probe, _) = w.split_once(" = ").unwrap();
                        roomy(w)
                            .is_some_and(|a| a.starts_with("0 ") && o == format!("{probe} = {a}"))
                    }
                    _ => false,
                };
                if w != o && !layout {
                    wrong.push(format!(
                        "{run} line {}\n  glibc: {w:?}\n  ours:  {o:?}",
                        i + 1
                    ));
                }
            }
        }
        set_test_text(Which::Rpc, None);
        assert!(
            wrong.is_empty(),
            "{} differ:\n{}",
            wrong.len(),
            wrong.join("\n")
        );
    }

    // Generated by posix/tools/oracle/netdb_harness.py: the database files, the
    // commands, and what glibc 2.39 printed for them (netdb_oracle.c).
    const SERVICES_FILE: &[u8] = b"# comment line\nhttp\t\t80/tcp\t\twww www-http\t# the web\nhttp\t\t80/udp\n  indented\t81/tcp\nftp 21/tcp\nftp 21/udp fsp\nnoproto 99\noctal 010/tcp\nhex 0x1f/tcp\nbig 70000/tcp\nslashes 85//tcp\nbad 8x/tcp\nempty /tcp\ndup 7/tcp first\ndup 7/tcp second\nCase 55/tcp\ncrlf 56/tcp alias\r\nnul 57/tcp\x00hidden\ntabs\t58/tcp\tt1\tt2\nhash#x 59/tcp\ntrailing 60/tcp   \nspaceport  61 /tcp\nsp2 62/ tcp\n\n   \nlast 63/tcp";
    const PROTOCOLS_FILE: &[u8] = b"ip 0 IP\ntcp 6 TCP\nudp\t17\tUDP\t# user datagram\nhex 0x10 HEX\nbad 12x BAD\nnoalias 20\ndup 6 DUP\nCase 30\nendnum 31";
    const NETWORKS_FILE: &[u8] = b"loopback 127\nlink-local 169.254.0.0 ll\nten 10.0 tennet\nfull 192.168.1.0\nUpper 172.16\nhex 0x0a.1\nbad x.y\ntoomany 1.2.3.4.5\nnoaddr\n";
    const ETHERS_FILE: &[u8] = b"00:11:22:33:44:55 host1\n0:1:2:3:4:5 Host2 # comment\naa:bb:cc:dd:ee:ff\thost3\n1:2:3:4:5 short\n100:1:2:3:4:5 toobig\n0x1:2:3:4:5:6 hexpfx\n 1:2:3:4:5:7 indented\n1:2:3:4:5:8\n1:2:3:4:5:9 twice\n1:2:3:4:5:9 again\n";
    const COMMANDS: &str = "\
serv http tcp\n\
serv http -\n\
serv www tcp\n\
serv www-http udp\n\
serv http udp\n\
serv indented -\n\
serv ftp udp\n\
serv fsp -\n\
serv fsp tcp\n\
serv noproto -\n\
serv noproto tcp\n\
serv octal -\n\
port 8 -\n\
serv hex -\n\
port 31 tcp\n\
serv big -\n\
port 4464 tcp\n\
serv slashes -\n\
serv bad -\n\
serv empty -\n\
serv dup -\n\
port 7 tcp\n\
serv case -\n\
serv Case -\n\
serv crlf -\n\
serv alias -\n\
serv nul -\n\
serv hidden -\n\
serv tabs -\n\
serv t2 -\n\
serv hash -\n\
serv hash#x -\n\
serv trailing tcp\n\
serv spaceport -\n\
serv sp2 -\n\
serv last -\n\
port 80 -\n\
port 80 udp\n\
port 81 tcp\n\
port 999 -\n\
port 63 tcp\n\
rawport 0x7fffffff -\n\
servsmall http tcp 10\n\
servsmall http tcp 60\n\
servsmall http tcp 200\n\
servent\n\
mix ftp\n\
proto tcp\n\
proto TCP\n\
proto udp\n\
proto UDP\n\
protonum 17\n\
protonum 6\n\
proto hex\n\
protonum 16\n\
protonum 0\n\
proto bad\n\
proto noalias\n\
proto case\n\
proto Case\n\
proto endnum\n\
protonum 99\n\
protoent\n\
net loopback\n\
net LOOPBACK\n\
net ll\n\
net LL\n\
net ten\n\
net full\n\
net upper\n\
net hex\n\
net bad\n\
net toomany\n\
net noaddr\n\
net nothere\n\
netaddr 7f000000 2\n\
netaddr 7f000000 0\n\
netaddr 7f000000 10\n\
netaddr a000000 2\n\
netaddr ffffffff 2\n\
netaddr 0 2\n\
netent\n\
ethh host1\n\
ethh HOST1\n\
ethh host2\n\
ethh host3\n\
ethh short\n\
ethh toobig\n\
ethh hexpfx\n\
ethh indented\n\
ethh twice\n\
ethh nothere\n\
ethn 0:11:22:33:44:55\n\
ethn aa:bb:cc:dd:ee:ff\n\
ethn 1:2:3:4:5:6\n\
ethn 1:2:3:4:5:7\n\
ethn 1:2:3:4:5:8\n\
ethn 1:2:3:4:5:9\n\
ethn 9:9:9:9:9:9\n\
";
    const GLIBC_OUTPUT: &str = "\
name=http port=80 proto=tcp aliases=[www,www-http]\n\
name=http port=80 proto=tcp aliases=[www,www-http]\n\
name=http port=80 proto=tcp aliases=[www,www-http]\n\
NULL rc=0\n\
name=http port=80 proto=udp aliases=[]\n\
name=indented port=81 proto=tcp aliases=[]\n\
name=ftp port=21 proto=udp aliases=[fsp]\n\
name=ftp port=21 proto=udp aliases=[fsp]\n\
NULL rc=0\n\
name=noproto port=99 proto= aliases=[]\n\
NULL rc=0\n\
name=octal port=8 proto=tcp aliases=[]\n\
name=octal port=8 proto=tcp aliases=[]\n\
name=hex port=31 proto=tcp aliases=[]\n\
name=hex port=31 proto=tcp aliases=[]\n\
name=big port=4464 proto=tcp aliases=[]\n\
name=big port=4464 proto=tcp aliases=[]\n\
name=slashes port=85 proto=tcp aliases=[]\n\
NULL rc=0\n\
NULL rc=0\n\
name=dup port=7 proto=tcp aliases=[first]\n\
name=dup port=7 proto=tcp aliases=[first]\n\
NULL rc=0\n\
name=Case port=55 proto=tcp aliases=[]\n\
name=crlf port=56 proto=tcp aliases=[alias]\n\
name=crlf port=56 proto=tcp aliases=[alias]\n\
name=nul port=57 proto=tcp aliases=[]\n\
NULL rc=0\n\
name=tabs port=58 proto=tcp aliases=[t1,t2]\n\
name=tabs port=58 proto=tcp aliases=[t1,t2]\n\
NULL rc=0\n\
NULL rc=0\n\
name=trailing port=60 proto=tcp aliases=[]\n\
NULL rc=0\n\
name=sp2 port=62 proto= aliases=[tcp]\n\
name=last port=63 proto=tcp aliases=[]\n\
name=http port=80 proto=tcp aliases=[www,www-http]\n\
name=http port=80 proto=udp aliases=[]\n\
name=indented port=81 proto=tcp aliases=[]\n\
NULL rc=0\n\
name=last port=63 proto=tcp aliases=[]\n\
NULL rc=0\n\
rc=34 found=0\n\
rc=34 found=0\n\
rc=0 found=1\n\
name=http port=80 proto=tcp aliases=[www,www-http]\n\
name=http port=80 proto=udp aliases=[]\n\
name=indented port=81 proto=tcp aliases=[]\n\
name=ftp port=21 proto=tcp aliases=[]\n\
name=ftp port=21 proto=udp aliases=[fsp]\n\
name=noproto port=99 proto= aliases=[]\n\
name=octal port=8 proto=tcp aliases=[]\n\
name=hex port=31 proto=tcp aliases=[]\n\
name=big port=4464 proto=tcp aliases=[]\n\
name=slashes port=85 proto=tcp aliases=[]\n\
name=dup port=7 proto=tcp aliases=[first]\n\
name=dup port=7 proto=tcp aliases=[second]\n\
name=Case port=55 proto=tcp aliases=[]\n\
name=crlf port=56 proto=tcp aliases=[alias]\n\
name=nul port=57 proto=tcp aliases=[]\n\
name=tabs port=58 proto=tcp aliases=[t1,t2]\n\
name=trailing port=60 proto=tcp aliases=[]\n\
name=sp2 port=62 proto= aliases=[tcp]\n\
name=last port=63 proto=tcp aliases=[]\n\
end\n\
name=http port=80 proto=tcp aliases=[www,www-http]\n\
name=http port=80 proto=udp aliases=[]\n\
name=ftp port=21 proto=tcp aliases=[]\n\
name=indented port=81 proto=tcp aliases=[]\n\
name=tcp proto=6 aliases=[TCP]\n\
name=tcp proto=6 aliases=[TCP]\n\
name=udp proto=17 aliases=[UDP]\n\
name=udp proto=17 aliases=[UDP]\n\
name=udp proto=17 aliases=[UDP]\n\
name=tcp proto=6 aliases=[TCP]\n\
NULL rc=0\n\
NULL rc=0\n\
name=ip proto=0 aliases=[IP]\n\
NULL rc=0\n\
name=noalias proto=20 aliases=[]\n\
NULL rc=0\n\
name=Case proto=30 aliases=[]\n\
name=endnum proto=31 aliases=[]\n\
NULL rc=0\n\
name=ip proto=0 aliases=[IP]\n\
name=tcp proto=6 aliases=[TCP]\n\
name=udp proto=17 aliases=[UDP]\n\
name=noalias proto=20 aliases=[]\n\
name=dup proto=6 aliases=[DUP]\n\
name=Case proto=30 aliases=[]\n\
name=endnum proto=31 aliases=[]\n\
end\n\
name=loopback type=2 net=7f000000 aliases=[]\n\
name=loopback type=2 net=7f000000 aliases=[]\n\
name=link-local type=2 net=a9fe0000 aliases=[ll]\n\
name=link-local type=2 net=a9fe0000 aliases=[ll]\n\
name=ten type=2 net=0a000000 aliases=[tennet]\n\
name=full type=2 net=c0a80100 aliases=[]\n\
name=Upper type=2 net=ac100000 aliases=[]\n\
name=hex type=2 net=0a010000 aliases=[]\n\
name=bad type=2 net=ffffffff aliases=[]\n\
name=toomany type=2 net=ffffffff aliases=[]\n\
name=noaddr type=2 net=ffffffff aliases=[]\n\
NULL rc=0 herr=1\n\
name=loopback type=2 net=7f000000 aliases=[]\n\
name=loopback type=2 net=7f000000 aliases=[]\n\
NULL rc=0 herr=1\n\
name=ten type=2 net=0a000000 aliases=[tennet]\n\
name=bad type=2 net=ffffffff aliases=[]\n\
NULL rc=0 herr=1\n\
name=loopback type=2 net=7f000000 aliases=[]\n\
name=link-local type=2 net=a9fe0000 aliases=[ll]\n\
name=ten type=2 net=0a000000 aliases=[tennet]\n\
name=full type=2 net=c0a80100 aliases=[]\n\
name=Upper type=2 net=ac100000 aliases=[]\n\
name=hex type=2 net=0a010000 aliases=[]\n\
name=bad type=2 net=ffffffff aliases=[]\n\
name=toomany type=2 net=ffffffff aliases=[]\n\
name=noaddr type=2 net=ffffffff aliases=[]\n\
end\n\
rc=0 addr=0:11:22:33:44:55\n\
rc=0 addr=0:11:22:33:44:55\n\
rc=0 addr=0:1:2:3:4:5\n\
rc=0 addr=aa:bb:cc:dd:ee:ff\n\
rc=-1 addr=-\n\
rc=-1 addr=-\n\
rc=0 addr=1:2:3:4:5:6\n\
rc=0 addr=1:2:3:4:5:7\n\
rc=0 addr=1:2:3:4:5:9\n\
rc=-1 addr=-\n\
rc=0 host=host1\n\
rc=0 host=host3\n\
rc=0 host=hexpfx\n\
rc=0 host=indented\n\
rc=0 host=\n\
rc=0 host=twice\n\
rc=-1 host=-\n\
";
}
