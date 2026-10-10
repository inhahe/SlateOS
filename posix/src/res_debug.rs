// Every index here is into a table this module defines, a fixed buffer its
// text is bounded by, or a caller's buffer its size was checked against
// first; every sum is of such lengths, or BIND's 32-bit arithmetic, which
// wraps as C's unsigned arithmetic does and is written so. Clippy cannot see
// the bounds.
#![allow(clippy::arithmetic_side_effects, clippy::indexing_slicing)]
//! `<resolv.h>`'s symbol tables and printers -- `sym_ntos`, `sym_ntop`,
//! `sym_ston` and the tables `__p_class_syms` and `__p_type_syms`;
//! `p_class`, `p_type`, `p_rcode`, `p_option`, `p_time`; the LOC record's
//! text, `loc_aton` and `loc_ntoa`; and base64, `b64_ntop` and `b64_pton`.
//!
//! BIND's, in glibc 2.39's libresolv (`resolv/res_debug.c`, `base64.c`),
//! and here in libc.a, each under the `__` name glibc's `<resolv.h>`
//! renames its public one to, as `posix/include`'s does. Ported line for
//! line where a line decides an answer, and replayed against glibc's
//! (`resdebug_oracle.txt`), odd answers included:
//!
//! - **The tables** are glibc's, order and all: `sym_ntos` answers the
//!   first entry of a number (class 4 is `HS`, not `HESIOD`), `sym_ntop` an
//!   entry's human name -- which the class table has none of, so a class
//!   found is NULL -- and a number in none of them its decimal. `sym_ston`
//!   answers the table's end's number for a name in none (the class table's
//!   is `C_IN`); rcode 11 is `""`.
//! - **`loc_ntoa`** prints glibc's arithmetic as glibc's binary does: a
//!   latitude of -2^31 as 596 31 23.648, whose negation C leaves undefined
//!   and glibc's compiler made unsigned; an altitude over 2^31 cm as
//!   `-100000.-01m`, `%.2d` of a remainder of -1; a precision of 9e9 cm as
//!   `4100654.08m`, its multiply 32 bits wide.
//! - **`loc_aton`**'s scans are bounded by the text's end, where glibc's
//!   read on past the NUL until they meet white space; its answers are
//!   the same where glibc's are not left to that memory.
//!
//! The answers glibc keeps in static buffers -- `sym_ntos`'s and
//! `sym_ntop`'s for a number in no table, `p_option`'s, `p_time`'s,
//! `loc_ntoa`'s given no buffer -- are the calling thread's here
//! ([`crate::netdb`]'s block), so two threads never overwrite each other's;
//! when that block cannot be had, the answer is `"?"`, never NULL.

use crate::nameser::ns_format_ttl;

/// `struct res_sym`: a number, its name, and its human name.
#[repr(C)]
pub struct ResSym {
    /// The number (`T_MX`, `C_IN` ...).
    pub number: i32,
    /// Its symbolic name (`"MX"`); NULL ends a table.
    pub name: *const u8,
    /// Its fun name (`"mail exchanger"`), or NULL.
    pub humanname: *const u8,
}

/// A table of [`ResSym`], exported as data as glibc's are: its entries'
/// pointers are to string literals, which no thread writes.
#[repr(transparent)]
pub struct SymTable<const N: usize>(pub [ResSym; N]);

// SAFETY: the pointers are to `'static` string literals, only read.
unsafe impl<const N: usize> Sync for SymTable<N> {}

/// An entry.
const fn sym(
    number: i32,
    name: &'static core::ffi::CStr,
    human: Option<&'static core::ffi::CStr>,
) -> ResSym {
    ResSym {
        number,
        name: name.as_ptr().cast(),
        humanname: match human {
            Some(h) => h.as_ptr().cast(),
            None => core::ptr::null(),
        },
    }
}

/// A table's end: its number is `sym_ston`'s answer for a name in none.
const fn end(number: i32) -> ResSym {
    ResSym {
        number,
        name: core::ptr::null(),
        humanname: core::ptr::null(),
    }
}

/// The classes (`__p_class_syms`): `C_ANY` is a qclass but no class.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub static __p_class_syms: SymTable<7> = SymTable([
    sym(1, c"IN", None),
    sym(3, c"CHAOS", None),
    sym(4, c"HS", None),
    sym(4, c"HESIOD", None),
    sym(255, c"ANY", None),
    sym(254, c"NONE", None),
    end(1),
]);

/// The types (`__p_type_syms`), glibc's list and order, and its two
/// padding entries, "to preserve ABI".
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub static __p_type_syms: SymTable<46> = SymTable([
    sym(1, c"A", Some(c"address")),
    sym(2, c"NS", Some(c"name server")),
    sym(3, c"MD", Some(c"mail destination (deprecated)")),
    sym(4, c"MF", Some(c"mail forwarder (deprecated)")),
    sym(5, c"CNAME", Some(c"canonical name")),
    sym(6, c"SOA", Some(c"start of authority")),
    sym(7, c"MB", Some(c"mailbox")),
    sym(8, c"MG", Some(c"mail group member")),
    sym(9, c"MR", Some(c"mail rename")),
    sym(10, c"NULL", Some(c"null")),
    sym(11, c"WKS", Some(c"well-known service (deprecated)")),
    sym(12, c"PTR", Some(c"domain name pointer")),
    sym(13, c"HINFO", Some(c"host information")),
    sym(14, c"MINFO", Some(c"mailbox information")),
    sym(15, c"MX", Some(c"mail exchanger")),
    sym(16, c"TXT", Some(c"text")),
    sym(17, c"RP", Some(c"responsible person")),
    sym(18, c"AFSDB", Some(c"DCE or AFS server")),
    sym(19, c"X25", Some(c"X25 address")),
    sym(20, c"ISDN", Some(c"ISDN address")),
    sym(21, c"RT", Some(c"router")),
    sym(22, c"NSAP", Some(c"nsap address")),
    sym(23, c"NSAP_PTR", Some(c"domain name pointer")),
    sym(24, c"SIG", Some(c"signature")),
    sym(25, c"KEY", Some(c"key")),
    sym(26, c"PX", Some(c"mapping information")),
    sym(27, c"GPOS", Some(c"geographical position (withdrawn)")),
    sym(28, c"AAAA", Some(c"IPv6 address")),
    sym(29, c"LOC", Some(c"location")),
    sym(30, c"NXT", Some(c"next valid name (unimplemented)")),
    sym(31, c"EID", Some(c"endpoint identifier (unimplemented)")),
    sym(32, c"NIMLOC", Some(c"NIMROD locator (unimplemented)")),
    sym(33, c"SRV", Some(c"server selection")),
    sym(34, c"ATMA", Some(c"ATM address (unimplemented)")),
    sym(39, c"DNAME", Some(c"Non-terminal DNAME (for IPv6)")),
    sym(250, c"TSIG", Some(c"transaction signature")),
    sym(251, c"IXFR", Some(c"incremental zone transfer")),
    sym(252, c"AXFR", Some(c"zone transfer")),
    sym(253, c"MAILB", Some(c"mailbox-related data (deprecated)")),
    sym(254, c"MAILA", Some(c"mail agent (deprecated)")),
    sym(35, c"NAPTR", Some(c"URN Naming Authority")),
    sym(36, c"KX", Some(c"Key Exchange")),
    sym(37, c"CERT", Some(c"Certificate")),
    sym(255, c"ANY", Some(c"\"any\"")),
    end(0),
    end(0),
]);

/// The rcodes (glibc's hidden `__p_rcode_syms`): 11, `ns_r_max`, is `""`.
static RCODE_SYMS: SymTable<16> = SymTable([
    sym(0, c"NOERROR", Some(c"no error")),
    sym(1, c"FORMERR", Some(c"format error")),
    sym(2, c"SERVFAIL", Some(c"server failed")),
    sym(3, c"NXDOMAIN", Some(c"no such domain name")),
    sym(4, c"NOTIMP", Some(c"not implemented")),
    sym(5, c"REFUSED", Some(c"refused")),
    sym(6, c"YXDOMAIN", Some(c"domain name exists")),
    sym(7, c"YXRRSET", Some(c"rrset exists")),
    sym(8, c"NXRRSET", Some(c"rrset doesn't exist")),
    sym(9, c"NOTAUTH", Some(c"not authoritative")),
    sym(10, c"NOTZONE", Some(c"Not in zone")),
    sym(11, c"", Some(c"")),
    sym(16, c"BADSIG", Some(c"bad signature")),
    sym(17, c"BADKEY", Some(c"bad key")),
    sym(18, c"BADTIME", Some(c"bad time")),
    end(0),
]);

/// The answer when the thread's buffer cannot be had.
const UNKNOWN: &core::ffi::CStr = c"?";

/// A NUL-terminated string's bytes, or `None` for NULL.
///
/// # Safety
///
/// `s` is NULL or a NUL-terminated string.
unsafe fn c_text<'a>(s: *const u8) -> Option<&'a [u8]> {
    if s.is_null() {
        return None;
    }
    // SAFETY: the caller's contract.
    Some(unsafe { core::slice::from_raw_parts(s, crate::string::strlen(s)) })
}

/// The entries of a table, up to the one whose name is NULL, and that end.
///
/// # Safety
///
/// `syms` is a table: entries up to one whose name is NULL.
unsafe fn entries<'a>(syms: *const ResSym) -> (&'a [ResSym], &'a ResSym) {
    let mut n = 0usize;
    // SAFETY: the caller's contract: every entry up to the end is readable.
    while !unsafe { &*syms.add(n) }.name.is_null() {
        n += 1;
    }
    // SAFETY: as above.
    unsafe { (core::slice::from_raw_parts(syms, n), &*syms.add(n)) }
}

/// The name `table` gives `number` -- its first entry's -- or `None` when
/// none does: how `ns_sprintrrf` prints a class or type (`CLASSn` or
/// `TYPEn` when `None`).
pub(crate) fn sym_name<const N: usize>(
    table: &'static SymTable<N>,
    number: i32,
) -> Option<&'static [u8]> {
    let s = table
        .0
        .iter()
        .take_while(|s| !s.name.is_null())
        .find(|s| s.number == number)?;
    // SAFETY: a table's names are NUL-terminated string literals.
    unsafe { c_text(s.name) }
}

/// `number` in decimal into the thread's buffer `which` holds, as a string:
/// glibc's `sprintf (unname, "%d", number)`.
fn decimal_into(which: crate::netdb::ResDebugBuf, number: i64) -> *const u8 {
    let buf = crate::netdb::res_debug_buffer(which);
    if buf.is_null() {
        return UNKNOWN.as_ptr().cast();
    }
    let mut w = Text::new();
    w.decimal(number);
    w.put(buf, which.size())
}

/// Text built in a fixed buffer, then copied out with its NUL.
struct Text {
    b: [u8; 128],
    n: usize,
}

impl Text {
    const fn new() -> Self {
        Self { b: [0; 128], n: 0 }
    }

    fn bytes(&mut self, s: &[u8]) {
        let k = s.len().min(self.b.len() - 1 - self.n);
        self.b[self.n..self.n + k].copy_from_slice(&s[..k]);
        self.n += k;
    }

    fn byte(&mut self, c: u8) {
        self.bytes(&[c]);
    }

    /// `%d` (or `%ld`).
    fn decimal(&mut self, v: i64) {
        self.padded(v, 0);
    }

    /// `%.Nd`: at least `prec` digits, a sign before them.
    fn padded(&mut self, v: i64, prec: usize) {
        if v < 0 {
            self.byte(b'-');
        }
        let mut digits = [0u8; 20];
        let mut k = digits.len();
        let mut m = v.unsigned_abs();
        loop {
            k -= 1;
            digits[k] = b'0' + (m % 10) as u8;
            m /= 10;
            if m == 0 {
                break;
            }
        }
        for _ in (digits.len() - k)..prec {
            self.byte(b'0');
        }
        let d = digits;
        self.bytes(&d[k..]);
    }

    /// The text and its NUL at `dst`, which holds `cap` bytes; `dst`.
    fn put(&self, dst: *mut u8, cap: usize) -> *const u8 {
        let n = self.n.min(cap - 1);
        // SAFETY: `dst` holds `cap` bytes, the caller's contract.
        unsafe {
            core::ptr::copy_nonoverlapping(self.b.as_ptr(), dst, n);
            dst.add(n).write(0);
        }
        dst.cast_const()
    }
}

// ---------------------------------------------------------------------------
// The tables
// ---------------------------------------------------------------------------

/// `sym_ntos` -- the name of `number` in the table `syms`, with `*success`
/// 1; or its decimal, in the calling thread's buffer, with `*success` 0
/// (`success` may be NULL). Deprecated by glibc.
///
/// # Safety
///
/// `syms` is a table (entries up to one whose name is NULL); `success` is
/// NULL or writable.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn __sym_ntos(
    syms: *const ResSym,
    number: i32,
    success: *mut i32,
) -> *const u8 {
    // SAFETY: the caller's contract.
    let (list, _) = unsafe { entries(syms) };
    let found = list.iter().find(|s| s.number == number);
    if !success.is_null() {
        // SAFETY: writable, the caller's contract.
        unsafe { success.write(i32::from(found.is_some())) };
    }
    match found {
        Some(s) => s.name,
        None => decimal_into(crate::netdb::ResDebugBuf::Ntos, i64::from(number)),
    }
}

/// `sym_ntop` -- as [`__sym_ntos`], but the entry's human name, which may
/// be NULL (the class table has none). Deprecated by glibc.
///
/// # Safety
///
/// As [`__sym_ntos`].
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn __sym_ntop(
    syms: *const ResSym,
    number: i32,
    success: *mut i32,
) -> *const u8 {
    // SAFETY: the caller's contract.
    let (list, _) = unsafe { entries(syms) };
    let found = list.iter().find(|s| s.number == number);
    if !success.is_null() {
        // SAFETY: writable, the caller's contract.
        unsafe { success.write(i32::from(found.is_some())) };
    }
    match found {
        Some(s) => s.humanname,
        None => decimal_into(crate::netdb::ResDebugBuf::Ntop, i64::from(number)),
    }
}

/// `sym_ston` -- the number of the name `name` in the table `syms`, in
/// ASCII case alone, with `*success` 1; or the number of the table's end,
/// its default, with `*success` 0. A NULL name is in no table. Deprecated
/// by glibc.
///
/// # Safety
///
/// As [`__sym_ntos`]; `name` is NULL or a NUL-terminated string.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn __sym_ston(
    syms: *const ResSym,
    name: *const u8,
    success: *mut i32,
) -> i32 {
    // SAFETY: the caller's contract.
    let (list, end) = unsafe { entries(syms) };
    // SAFETY: the caller's contract.
    let name = unsafe { c_text(name) };
    let found = name.and_then(|name| {
        // SAFETY: every entry's name is a NUL-terminated string.
        list.iter()
            .find(|s| unsafe { c_text(s.name) }.is_some_and(|n| n.eq_ignore_ascii_case(name)))
    });
    if !success.is_null() {
        // SAFETY: writable, the caller's contract.
        unsafe { success.write(i32::from(found.is_some())) };
    }
    found.map_or(end.number, |s| s.number)
}

/// `p_class` -- a class's name: [`__sym_ntos`] on [`__p_class_syms`].
/// Deprecated by glibc.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn __p_class(class: i32) -> *const u8 {
    // SAFETY: a table of this module's; no success asked for.
    unsafe { __sym_ntos(__p_class_syms.0.as_ptr(), class, core::ptr::null_mut()) }
}

/// `p_type` -- a type's name: [`__sym_ntos`] on [`__p_type_syms`].
/// Deprecated by glibc.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn __p_type(type_: i32) -> *const u8 {
    // SAFETY: as above.
    unsafe { __sym_ntos(__p_type_syms.0.as_ptr(), type_, core::ptr::null_mut()) }
}

/// `p_rcode` -- a response code's name (11, `ns_r_max`, is `""`).
/// Deprecated by glibc.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn __p_rcode(rcode: i32) -> *const u8 {
    // SAFETY: as above.
    unsafe { __sym_ntos(RCODE_SYMS.0.as_ptr(), rcode, core::ptr::null_mut()) }
}

/// The options `p_option` has names for, glibc 2.39's values -- those
/// [`crate::resolv`] keeps no constant of are here alone.
const OPTION_NAMES: [(u64, &core::ffi::CStr); 18] = [
    (crate::resolv::RES_INIT, c"init"),
    (crate::resolv::RES_DEBUG, c"debug"),
    (crate::resolv::RES_USEVC, c"use-vc"),
    (crate::resolv::RES_IGNTC, c"igntc"),
    (crate::resolv::RES_RECURSE, c"recurs"),
    (crate::resolv::RES_DEFNAMES, c"defnam"),
    (0x0000_0100, c"styopn"),
    (crate::resolv::RES_DNSRCH, c"dnsrch"),
    (crate::resolv::RES_NOALIASES, c"noaliases"),
    (crate::resolv::RES_ROTATE, c"rotate"),
    (0x0010_0000, c"edns0"),
    (0x0020_0000, c"single-request"),
    (0x0040_0000, c"single-request-reopen"),
    (0x0080_0000, c"dnssec"),
    (crate::resolv::RES_NOTLDQUERY, c"no-tld-query"),
    (0x0200_0000, c"no-reload"),
    (crate::resolv::RES_TRUSTAD, c"trust-ad"),
    (0x0800_0000, c"no-aaaa"),
];

/// `p_option` -- a resolver option's name (`"recurs"`, `"edns0"` ...), one
/// bit at a time: anything else is `?0x...?`, in the calling thread's
/// buffer. Deprecated by glibc.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn __p_option(option: u64) -> *const u8 {
    if let Some((_, name)) = OPTION_NAMES.iter().find(|(bit, _)| *bit == option) {
        return name.as_ptr().cast();
    }
    let which = crate::netdb::ResDebugBuf::Option;
    let buf = crate::netdb::res_debug_buffer(which);
    if buf.is_null() {
        return UNKNOWN.as_ptr().cast();
    }
    let mut w = Text::new();
    w.bytes(b"?0x");
    let mut digits = [0u8; 16];
    let mut k = digits.len();
    let mut m = option;
    loop {
        k -= 1;
        digits[k] = b"0123456789abcdef"[(m & 0xf) as usize];
        m >>= 4;
        if m == 0 {
            break;
        }
    }
    w.bytes(&digits[k..]);
    w.byte(b'?');
    w.put(buf, which.size())
}

/// `p_time` -- a TTL as `ns_format_ttl` writes it (`1d1h1m1s`, `1W`), or
/// in decimal if it does not fit -- it always does -- in the calling
/// thread's buffer. Deprecated by glibc.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn __p_time(value: u32) -> *const u8 {
    let which = crate::netdb::ResDebugBuf::Time;
    let buf = crate::netdb::res_debug_buffer(which);
    if buf.is_null() {
        return UNKNOWN.as_ptr().cast();
    }
    // SAFETY: the thread's buffer holds `which.size()` bytes.
    if unsafe { ns_format_ttl(u64::from(value), buf, which.size()) } < 0 {
        let mut w = Text::new();
        w.decimal(i64::from(value));
        return w.put(buf, which.size());
    }
    buf.cast_const()
}

// ---------------------------------------------------------------------------
// LOC records
// ---------------------------------------------------------------------------

/// Powers of ten, `unsigned int`s.
const POWEROFTEN: [u32; 10] = [
    1,
    10,
    100,
    1000,
    10000,
    100_000,
    1_000_000,
    10_000_000,
    100_000_000,
    1_000_000_000,
];

/// C's `isdigit` in the C locale; the end of the text is none.
fn at(s: &[u8], i: usize) -> u8 {
    s.get(i).copied().unwrap_or(0)
}

/// C's `isspace` in the C locale.
fn is_space(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}

/// glibc's `precsize_ntoa`: a size or precision byte, mantissa and power of
/// ten in centimetres, as metres to two places -- the multiply 32 bits
/// wide, as glibc's `int * unsigned int` is.
fn precsize_ntoa(prec: u8, w: &mut Text) {
    let mantissa = u32::from(prec >> 4) % 10;
    let exponent = usize::from(prec & 0x0f) % 10;
    let val = u64::from(mantissa.wrapping_mul(POWEROFTEN[exponent]));
    // Under 2^32: each converts.
    w.decimal(i64::try_from(val / 100).unwrap_or(i64::MAX));
    w.byte(b'.');
    w.padded(i64::try_from(val % 100).unwrap_or(0), 2);
}

/// glibc's `precsize_aton`: `X.YY` metres at `*cp` as a size byte,
/// mantissa and exponent, and `*cp` past what it read.
fn precsize_aton(s: &[u8], cp: &mut usize) -> u8 {
    let mut mval: u32 = 0;
    let mut cmval: u32 = 0;
    let mut i = *cp;
    while at(s, i).is_ascii_digit() {
        mval = mval
            .wrapping_mul(10)
            .wrapping_add(u32::from(at(s, i) - b'0'));
        i += 1;
    }
    if at(s, i) == b'.' {
        i += 1;
        if at(s, i).is_ascii_digit() {
            cmval = u32::from(at(s, i) - b'0') * 10;
            i += 1;
            if at(s, i).is_ascii_digit() {
                cmval += u32::from(at(s, i) - b'0');
                i += 1;
            }
        }
    }
    cmval = mval.wrapping_mul(100).wrapping_add(cmval);
    let mut exponent = 0usize;
    while exponent < 9 {
        if cmval < POWEROFTEN[exponent + 1] {
            break;
        }
        exponent += 1;
    }
    let mantissa = (cmval / POWEROFTEN[exponent]).min(9);
    *cp = i;
    ((mantissa << 4) as u8) | exponent as u8
}

/// glibc's `latlon2ul`: a latitude or longitude, `deg [min [sec[.frac]]]`
/// then a hemisphere, as its unsigned 32-bit encoding, and which it was --
/// 1 latitude, 2 longitude, 0 neither -- with `*cp` at the next field.
fn latlon2ul(s: &[u8], cp: &mut usize) -> (u32, i32) {
    let mut i = *cp;
    let (mut deg, mut min, mut secs, mut secsfrac) = (0i32, 0i32, 0i32, 0i32);
    let digits = |i: &mut usize, v: &mut i32| {
        while at(s, *i).is_ascii_digit() {
            *v = v.wrapping_mul(10).wrapping_add(i32::from(at(s, *i) - b'0'));
            *i += 1;
        }
    };
    let spaces = |i: &mut usize| {
        while is_space(at(s, *i)) {
            *i += 1;
        }
    };
    // The rest of a field, to white space or the text's end.
    let rest = |i: &mut usize| {
        while *i < s.len() && !is_space(at(s, *i)) {
            *i += 1;
        }
    };
    digits(&mut i, &mut deg);
    spaces(&mut i);
    'hemi: {
        if !at(s, i).is_ascii_digit() {
            break 'hemi;
        }
        digits(&mut i, &mut min);
        spaces(&mut i);
        if !at(s, i).is_ascii_digit() {
            break 'hemi;
        }
        digits(&mut i, &mut secs);
        if at(s, i) == b'.' {
            i += 1;
            if at(s, i).is_ascii_digit() {
                secsfrac = i32::from(at(s, i) - b'0') * 100;
                i += 1;
                if at(s, i).is_ascii_digit() {
                    secsfrac += i32::from(at(s, i) - b'0') * 10;
                    i += 1;
                    if at(s, i).is_ascii_digit() {
                        secsfrac += i32::from(at(s, i) - b'0');
                        i += 1;
                    }
                }
            }
        }
        rest(&mut i);
        spaces(&mut i);
    }
    let magnitude = deg
        .wrapping_mul(60)
        .wrapping_add(min)
        .wrapping_mul(60)
        .wrapping_add(secs)
        .wrapping_mul(1000)
        .cast_unsigned();
    let half = 1u32 << 31;
    let hemi = at(s, i);
    let retval = match hemi {
        b'N' | b'n' | b'E' | b'e' => half
            .wrapping_add(magnitude)
            .wrapping_add(secsfrac.cast_unsigned()),
        b'S' | b's' | b'W' | b'w' => half
            .wrapping_sub(magnitude)
            .wrapping_sub(secsfrac.cast_unsigned()),
        _ => 0,
    };
    let which = match hemi {
        b'N' | b'n' | b'S' | b's' => 1,
        b'E' | b'e' | b'W' | b'w' => 2,
        _ => 0,
    };
    // Past the hemisphere, and anything stuck to it, to the next field.
    if i < s.len() {
        i += 1;
    }
    rest(&mut i);
    spaces(&mut i);
    *cp = i;
    (retval, which)
}

/// glibc's `loc_aton` on text: the record's 16 bytes, or `None` (0).
fn loc_aton(s: &[u8]) -> Option<[u8; 16]> {
    let maxcp = s.len();
    let mut cp = 0usize;
    let (l1, w1) = latlon2ul(s, &mut cp);
    let (l2, w2) = latlon2ul(s, &mut cp);
    let (latit, longit) = match (w1, w2) {
        (1, 2) => (l1, l2),
        (2, 1) => (l2, l1),
        _ => return None,
    };
    let mut altsign: i32 = 1;
    if at(s, cp) == b'-' {
        altsign = -1;
        cp += 1;
    }
    if at(s, cp) == b'+' {
        cp += 1;
    }
    let mut altmeters: i32 = 0;
    let mut altfrac: i32 = 0;
    while at(s, cp).is_ascii_digit() {
        altmeters = altmeters
            .wrapping_mul(10)
            .wrapping_add(i32::from(at(s, cp) - b'0'));
        cp += 1;
    }
    if at(s, cp) == b'.' {
        cp += 1;
        if at(s, cp).is_ascii_digit() {
            altfrac = i32::from(at(s, cp) - b'0') * 10;
            cp += 1;
            if at(s, cp).is_ascii_digit() {
                altfrac += i32::from(at(s, cp) - b'0');
                cp += 1;
            }
        }
    }
    let alt = 10_000_000i32
        .wrapping_add(altsign.wrapping_mul(altmeters.wrapping_mul(100).wrapping_add(altfrac)));
    // Size, horizontal and vertical precision: each optional, glibc's
    // defaults 1 m, 10 km, 10 m.
    let (mut siz, mut hp, mut vp) = (0x12u8, 0x16u8, 0x13u8);
    let next_field = |cp: &mut usize| -> bool {
        while *cp < maxcp && !is_space(at(s, *cp)) {
            *cp += 1;
        }
        while *cp < maxcp && is_space(at(s, *cp)) {
            *cp += 1;
        }
        *cp < maxcp
    };
    if next_field(&mut cp) {
        siz = precsize_aton(s, &mut cp);
        if next_field(&mut cp) {
            hp = precsize_aton(s, &mut cp);
            if next_field(&mut cp) {
                vp = precsize_aton(s, &mut cp);
            }
        }
    }
    let mut out = [0u8; 16];
    out[1] = siz;
    out[2] = hp;
    out[3] = vp;
    out[4..8].copy_from_slice(&latit.to_be_bytes());
    out[8..12].copy_from_slice(&longit.to_be_bytes());
    out[12..16].copy_from_slice(&alt.cast_unsigned().to_be_bytes());
    Some(out)
}

/// `loc_aton` -- a LOC record's text (`42 21 54 N 71 06 18 W -24m 30m`:
/// latitude, longitude, altitude, then size and precisions, each optional)
/// as its 16 bytes of RDATA in `binary`: 16, or 0 for text without one
/// latitude and one longitude. A NULL text is 0; a NULL `binary`, which
/// glibc writes through, is 0 as well. Deprecated by glibc.
///
/// # Safety
///
/// `ascii` is NULL or a NUL-terminated string; `binary` is NULL or holds
/// 16 bytes.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn __loc_aton(ascii: *const u8, binary: *mut u8) -> i32 {
    // SAFETY: the caller's contract.
    let Some(s) = (unsafe { c_text(ascii) }) else {
        return 0;
    };
    match loc_aton(s) {
        Some(out) if !binary.is_null() => {
            // SAFETY: `binary` holds 16 bytes, the caller's contract.
            unsafe { core::ptr::copy_nonoverlapping(out.as_ptr(), binary, 16) };
            16
        }
        _ => 0,
    }
}

/// The longest text `loc_ntoa` writes, its NUL counted: glibc's `tmpbuf`.
pub(crate) const LOC_NTOA_MAX: usize =
    b"1000 60 60.000 N 1000 60 60.000 W -12345678.00m 90000000.00m 90000000.00m 90000000.00m\0"
        .len();

/// glibc's `loc_ntoa` on a record's 16 bytes, into `w`.
fn loc_ntoa(b: &[u8; 16], w: &mut Text) {
    if b[0] != 0 {
        w.bytes(b"; error: unknown LOC RR version");
        return;
    }
    let long_of = |k: usize| u32::from_be_bytes([b[k], b[k + 1], b[k + 2], b[k + 3]]);
    let half = 1u32 << 31;
    // glibc's `latval = templ - (1<<31)`, and its negation of a negative
    // one: -2^31 negated is C's undefined behaviour, and glibc's compiler
    // made it 2^31 -- unsigned arithmetic from there on, as here.
    let coord = |templ: u32, pos: u8, neg: u8| -> (u32, u8) {
        let val = templ.wrapping_sub(half).cast_signed();
        if val < 0 {
            (val.unsigned_abs(), neg)
        } else {
            (val.cast_unsigned(), pos)
        }
    };
    let (lat, ns) = coord(long_of(4), b'N', b'S');
    let (long, ew) = coord(long_of(8), b'E', b'W');
    let referencealt: u32 = 100_000 * 100;
    let templ = long_of(12);
    let (altval, altsign) = if templ < referencealt {
        ((referencealt - templ).cast_signed(), -1i32)
    } else {
        (templ.wrapping_sub(referencealt).cast_signed(), 1i32)
    };
    let part = |w: &mut Text, v: u32, hemi: u8| {
        let (secfrac, v) = (v % 1000, v / 1000);
        let (sec, v) = (v % 60, v / 60);
        let (min, deg) = (v % 60, v / 60);
        w.decimal(i64::from(deg));
        w.byte(b' ');
        w.padded(i64::from(min), 2);
        w.byte(b' ');
        w.padded(i64::from(sec), 2);
        w.byte(b'.');
        w.padded(i64::from(secfrac), 3);
        w.byte(b' ');
        w.byte(hemi);
    };
    part(w, lat, ns);
    w.byte(b' ');
    part(w, long, ew);
    w.byte(b' ');
    // C's `%` and `/` truncate, as Rust's do: -10000001 is -100000 m and
    // -1 cm, which `%.2d` prints `-01`.
    let altfrac = altval % 100;
    let altmeters = (altval / 100).wrapping_mul(altsign);
    w.decimal(i64::from(altmeters));
    w.byte(b'.');
    w.padded(i64::from(altfrac), 2);
    w.byte(b'm');
    for p in [b[1], b[2], b[3]] {
        w.byte(b' ');
        precsize_ntoa(p, w);
        w.byte(b'm');
    }
}

/// `loc_ntoa` -- a LOC record's 16 bytes of RDATA as its text, into
/// `ascii` (which must hold [`LOC_NTOA_MAX`] bytes), or with NULL into the
/// calling thread's buffer: that pointer; `"?"` when the thread's buffer
/// cannot be had, and for a NULL record, where glibc reads through it. A
/// version but 0 is `"; error: unknown LOC RR version"`. Deprecated by
/// glibc.
///
/// # Safety
///
/// `binary` is NULL or holds 16 bytes; `ascii` is NULL or holds
/// [`LOC_NTOA_MAX`] bytes.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn __loc_ntoa(binary: *const u8, ascii: *mut u8) -> *const u8 {
    let which = crate::netdb::ResDebugBuf::Loc;
    let dst = if ascii.is_null() {
        crate::netdb::res_debug_buffer(which)
    } else {
        ascii
    };
    if dst.is_null() || binary.is_null() {
        return UNKNOWN.as_ptr().cast();
    }
    let mut b = [0u8; 16];
    // SAFETY: `binary` holds 16 bytes, the caller's contract.
    unsafe { core::ptr::copy_nonoverlapping(binary, b.as_mut_ptr(), 16) };
    let mut w = Text::new();
    loc_ntoa(&b, &mut w);
    w.put(dst, LOC_NTOA_MAX)
}

// ---------------------------------------------------------------------------
// Base64
// ---------------------------------------------------------------------------

/// RFC 4648's base64 alphabet.
const BASE64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// glibc's `b64_ntop` on slices: the text's length, or `None` when
/// `target` cannot hold it and its NUL -- what fit written all the same.
fn b64_ntop(src: &[u8], target: &mut [u8]) -> Option<usize> {
    let mut n = 0usize;
    let (groups, rest) = src.as_chunks::<3>();
    for c in groups {
        if n + 4 > target.len() {
            return None;
        }
        target[n] = BASE64[usize::from(c[0] >> 2)];
        target[n + 1] = BASE64[usize::from(((c[0] & 0x03) << 4) + (c[1] >> 4))];
        target[n + 2] = BASE64[usize::from(((c[1] & 0x0f) << 2) + (c[2] >> 6))];
        target[n + 3] = BASE64[usize::from(c[2] & 0x3f)];
        n += 4;
    }
    if !rest.is_empty() {
        let mut input = [0u8; 3];
        input[..rest.len()].copy_from_slice(rest);
        if n + 4 > target.len() {
            return None;
        }
        target[n] = BASE64[usize::from(input[0] >> 2)];
        target[n + 1] = BASE64[usize::from(((input[0] & 0x03) << 4) + (input[1] >> 4))];
        target[n + 2] = if rest.len() == 1 {
            b'='
        } else {
            BASE64[usize::from(((input[1] & 0x0f) << 2) + (input[2] >> 6))]
        };
        target[n + 3] = b'=';
        n += 4;
    }
    if n >= target.len() {
        return None;
    }
    target[n] = 0;
    Some(n)
}

/// `b64_ntop` -- `srclength` bytes as base64 text, `=` padded, in `target`
/// (`targsize` bytes, the NUL counted): the text's length, or -1 when it
/// does not fit -- the groups before the one that did not written. A NULL
/// `src` or `target` is -1. Not deprecated.
///
/// # Safety
///
/// `src` is NULL or holds `srclength` bytes; `target` is NULL or holds
/// `targsize` bytes.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn __b64_ntop(
    src: *const u8,
    srclength: usize,
    target: *mut u8,
    targsize: usize,
) -> i32 {
    if (src.is_null() && srclength > 0) || target.is_null() {
        return -1;
    }
    let src: &[u8] = if srclength == 0 {
        &[]
    } else {
        // SAFETY: `src` holds `srclength` bytes, the caller's contract.
        unsafe { core::slice::from_raw_parts(src, srclength) }
    };
    // SAFETY: `target` holds `targsize` bytes, the caller's contract.
    let target = unsafe { core::slice::from_raw_parts_mut(target, targsize) };
    b64_ntop(src, target).map_or(-1, |n| i32::try_from(n).unwrap_or(i32::MAX))
}

/// glibc's `b64_pton` on text, into `target` or, with none, only counted:
/// the bytes, or `None` (-1).
fn b64_pton(src: &[u8], mut target: Option<&mut [u8]>) -> Option<usize> {
    let at = |i: usize| src.get(i).copied().unwrap_or(0);
    let mut i = 0usize;
    let mut tarindex = 0usize;
    let mut state = 0;
    let mut ch;
    loop {
        ch = at(i);
        i += 1;
        if ch == 0 {
            break;
        }
        if is_space(ch) {
            continue;
        }
        if ch == b'=' {
            break;
        }
        let pos = BASE64.iter().position(|&c| c == ch)? as u8;
        match state {
            0 => {
                if let Some(t) = target.as_deref_mut() {
                    *t.get_mut(tarindex)? = pos << 2;
                }
                state = 1;
            }
            1 => {
                if let Some(t) = target.as_deref_mut() {
                    if tarindex + 1 >= t.len() {
                        return None;
                    }
                    t[tarindex] |= pos >> 4;
                    t[tarindex + 1] = (pos & 0x0f) << 4;
                }
                tarindex += 1;
                state = 2;
            }
            2 => {
                if let Some(t) = target.as_deref_mut() {
                    if tarindex + 1 >= t.len() {
                        return None;
                    }
                    t[tarindex] |= pos >> 2;
                    t[tarindex + 1] = (pos & 0x03) << 6;
                }
                tarindex += 1;
                state = 3;
            }
            _ => {
                if let Some(t) = target.as_deref_mut() {
                    *t.get_mut(tarindex)? |= pos;
                }
                tarindex += 1;
                state = 0;
            }
        }
    }
    if ch == b'=' {
        ch = at(i);
        i += 1;
        match state {
            0 | 1 => return None,
            2 => {
                // One byte of information: white space, then another `=`.
                while ch != 0 && is_space(ch) {
                    ch = at(i);
                    i += 1;
                }
                if ch != b'=' {
                    return None;
                }
                ch = at(i);
                i += 1;
            }
            _ => {}
        }
        // Nothing but white space after the padding.
        while ch != 0 {
            if !is_space(ch) {
                return None;
            }
            ch = at(i);
            i += 1;
        }
        // The bits past the last whole byte must be zero, or they would be
        // a channel of their own.
        if let Some(t) = target.as_deref() {
            if t.get(tarindex).is_some_and(|&b| b != 0) {
                return None;
            }
        }
    } else if state != 0 {
        return None;
    }
    Some(tarindex)
}

/// `b64_pton` -- base64 text as bytes in `target` (`targsize` bytes): the
/// bytes, or -1 for a character outside the alphabet, bad padding, bits
/// past the last byte that are not zero, or too small a `target`. White
/// space anywhere is skipped. A NULL `target` counts the bytes and writes
/// nothing; a NULL `src` is -1. Not deprecated.
///
/// # Safety
///
/// `src` is NULL or a NUL-terminated string; `target` is NULL or holds
/// `targsize` bytes.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn __b64_pton(src: *const u8, target: *mut u8, targsize: usize) -> i32 {
    // SAFETY: the caller's contract.
    let Some(s) = (unsafe { c_text(src) }) else {
        return -1;
    };
    let target = if target.is_null() {
        None
    } else {
        // SAFETY: `target` holds `targsize` bytes, the caller's contract.
        Some(unsafe { core::slice::from_raw_parts_mut(target, targsize) })
    };
    b64_pton(s, target).map_or(-1, |n| i32::try_from(n).unwrap_or(i32::MAX))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::format;
    use std::string::String;
    use std::vec::Vec;

    /// glibc 2.39's answers, one line a call
    /// (`posix/tools/oracle/resdebug_harness.py`, which says the forms).
    const ORACLE: &str = include_str!("resdebug_oracle.txt");

    /// A text as the harness writes it: `\xHH` for a byte, `\x` for none.
    fn untoken(t: &str) -> Vec<u8> {
        if t == "\\x" {
            return Vec::new();
        }
        let b = t.as_bytes();
        let mut out = Vec::new();
        let mut i = 0;
        while i < b.len() {
            if b[i] == b'\\' && b.get(i + 1) == Some(&b'x') {
                out.push(u8::from_str_radix(&t[i + 2..i + 4], 16).unwrap());
                i += 4;
            } else {
                out.push(b[i]);
                i += 1;
            }
        }
        out
    }

    /// An answer as the harness's C writes it: `NULL`, `\x` for empty,
    /// `\xHH` for a byte outside `!`..`~` or a backslash.
    fn answer(p: *const u8) -> String {
        use core::fmt::Write;
        if p.is_null() {
            return "NULL".into();
        }
        // SAFETY: every answer is a NUL-terminated string.
        let s = unsafe { c_text(p) }.unwrap();
        if s.is_empty() {
            return "\\x".into();
        }
        s.iter().fold(String::new(), |mut out, &c| {
            if (0x21..=0x7e).contains(&c) && c != b'\\' {
                out.push(c as char);
            } else {
                // Writing into a String cannot fail.
                let _ = write!(out, "\\x{c:02x}");
            }
            out
        })
    }

    fn hex(b: &[u8]) -> String {
        b.iter()
            .map(|x| format!("{x:02x}"))
            .collect::<Vec<_>>()
            .concat()
    }

    fn unhex(h: &str) -> Vec<u8> {
        (0..h.len() / 2)
            .map(|i| u8::from_str_radix(&h[i * 2..i * 2 + 2], 16).unwrap())
            .collect()
    }

    fn cstring(b: &[u8]) -> Vec<u8> {
        let mut v = b.to_vec();
        v.push(0);
        v
    }

    fn table(which: &str) -> *const ResSym {
        if which == "class" {
            __p_class_syms.0.as_ptr()
        } else {
            __p_type_syms.0.as_ptr()
        }
    }

    /// What the harness prints after ` = ` for `call`, from this module.
    fn ours(call: &str) -> String {
        let f: Vec<&str> = call.split(' ').collect();
        match f[0] {
            "B" => {
                let src = if f[1] == "-" { Vec::new() } else { unhex(f[1]) };
                let mut out = [0xaau8; 48];
                // SAFETY: `src` holds its length; `out` every size asked.
                let r = unsafe {
                    __b64_ntop(
                        src.as_ptr(),
                        src.len(),
                        out.as_mut_ptr(),
                        f[2].parse().unwrap(),
                    )
                };
                format!("{r} {}", hex(&out))
            }
            "D" => {
                let s = cstring(&untoken(f[1]));
                if f[2] == "null" {
                    // SAFETY: NUL-terminated text; no target.
                    let r = unsafe { __b64_pton(s.as_ptr(), core::ptr::null_mut(), 0) };
                    return format!("{r} -");
                }
                let mut out = [0xaau8; 48];
                // SAFETY: as above; `out` holds every size asked.
                let r = unsafe { __b64_pton(s.as_ptr(), out.as_mut_ptr(), f[2].parse().unwrap()) };
                format!("{r} {}", hex(&out))
            }
            "N" | "H" => {
                let mut ok = 7;
                let n: i32 = f[2].parse().unwrap();
                // SAFETY: a table of this module's; `ok` is writable.
                let r = unsafe {
                    if f[0] == "N" {
                        __sym_ntos(table(f[1]), n, &mut ok)
                    } else {
                        __sym_ntop(table(f[1]), n, &mut ok)
                    }
                };
                format!("{} {ok}", answer(r))
            }
            "S" => {
                let mut ok = 7;
                let s = cstring(&untoken(f[2]));
                // SAFETY: as above; NUL-terminated text.
                let r = unsafe { __sym_ston(table(f[1]), s.as_ptr(), &mut ok) };
                format!("{r} {ok}")
            }
            "C" => answer(__p_class(f[1].parse().unwrap())),
            "T" => answer(__p_type(f[1].parse().unwrap())),
            "R" => answer(__p_rcode(f[1].parse().unwrap())),
            "O" => answer(__p_option(u64::from_str_radix(f[1], 16).unwrap())),
            "M" => answer(__p_time(f[1].parse().unwrap())),
            "A" => {
                let s = cstring(&untoken(f[1]));
                let mut out = [0xaau8; 16];
                // SAFETY: NUL-terminated text; `out` holds 16 bytes.
                let r = unsafe { __loc_aton(s.as_ptr(), out.as_mut_ptr()) };
                format!("{r} {}", hex(&out))
            }
            "Z" => {
                let b = unhex(f[1]);
                let mut out = [0u8; LOC_NTOA_MAX];
                // SAFETY: `b` holds 16 bytes; `out` the longest text.
                answer(unsafe { __loc_ntoa(b.as_ptr(), out.as_mut_ptr()) })
            }
            other => panic!("unknown line kind {other}"),
        }
    }

    #[test]
    fn the_symbols_and_printers_answer_as_glibcs() {
        let mut wrong = Vec::new();
        let mut n = 0;
        for line in ORACLE
            .lines()
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
        {
            let (call, want) = line.split_once(" = ").unwrap();
            n += 1;
            let got = ours(call);
            if got != want {
                wrong.push(format!("{call}\n  glibc: {want}\n  ours:  {got}"));
            }
        }
        assert!(n > 450, "the oracle has {n} lines");
        assert!(
            wrong.is_empty(),
            "{} of {n}:\n{}",
            wrong.len(),
            wrong.join("\n")
        );
    }

    /// The thread's buffers are its own, and `loc_ntoa` given none uses one.
    #[test]
    fn the_static_answers_are_the_threads_own() {
        let a = __p_class(1000);
        let other = std::thread::spawn(|| {
            let p = __p_class(2000);
            (p as usize, answer(p))
        })
        .join()
        .unwrap();
        assert_ne!(a as usize, other.0);
        assert_eq!(other.1, "2000");
        assert_eq!(answer(a), "1000", "not overwritten by the other thread");
        let b = unhex("00121613800000008000000000989680");
        // SAFETY: 16 bytes; no buffer asks for the thread's.
        let t = unsafe { __loc_ntoa(b.as_ptr(), core::ptr::null_mut()) };
        assert_eq!(
            answer(t),
            "0\\x2000\\x2000.000\\x20N\\x200\\x2000\\x2000.000\\x20E\\x200.00m\\x201.00m\\x2010000.00m\\x2010.00m"
        );
    }

    /// Where glibc would read or write through NULL, these refuse.
    #[test]
    fn null_is_refused_where_glibc_would_fault() {
        let mut out = [0u8; 16];
        // SAFETY: NULL is refused before it is read.
        assert_eq!(
            unsafe { __b64_ntop(core::ptr::null(), 3, out.as_mut_ptr(), 16) },
            -1
        );
        // SAFETY: as above.
        assert_eq!(
            unsafe { __b64_ntop(c"x".as_ptr().cast(), 1, core::ptr::null_mut(), 16) },
            -1
        );
        // SAFETY: as above.
        assert_eq!(
            unsafe { __b64_pton(core::ptr::null(), out.as_mut_ptr(), 16) },
            -1
        );
        // SAFETY: as above.
        assert_eq!(
            unsafe { __loc_aton(core::ptr::null(), out.as_mut_ptr()) },
            0
        );
        // SAFETY: as above.
        assert_eq!(
            unsafe { __loc_aton(c"45 N 90 E 1m".as_ptr().cast(), core::ptr::null_mut()) },
            0
        );
        // SAFETY: as above.
        assert_eq!(
            answer(unsafe { __loc_ntoa(core::ptr::null(), out.as_mut_ptr()) }),
            "?"
        );
        let mut ok = 7;
        // SAFETY: a NULL name is in no table.
        assert_eq!(
            unsafe { __sym_ston(__p_class_syms.0.as_ptr(), core::ptr::null(), &mut ok) },
            1
        );
        assert_eq!(ok, 0);
    }
}
