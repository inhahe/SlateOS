//! `<resolv.h>`'s message and name printers: `fp_nquery`, `fp_query`,
//! `p_query`, `p_cdnname`, `p_cdname`, `p_fqnname`, `p_fqname` and
//! `fp_resstat` -- BIND's, in glibc 2.39's libresolv (`resolv/res_debug.c`),
//! and here in libc.a, each under the `__` name glibc's `<resolv.h>` renames
//! its public one to, as `posix/include`'s does.
//!
//! A message is printed as dig's ancestors printed it: the header (`;;
//! ->>HEADER<<- opcode: QUERY, status: NOERROR, id: 4660`), its flags and
//! section counts, then each section's records -- a question as its name,
//! type and class, anything else as [`crate::nameser::ns_sprintrr`] prints
//! it. `_res.pfcode`'s `RES_PRF_*` bits choose which parts are printed, 0
//! meaning all; a section header is printed only when its bit and
//! `RES_PRF_HEAD1` both are. Each answer is glibc's, replayed against its
//! output (`posix/tools/oracle/resprint_harness.py`,
//! `resprint_oracle.txt`), down to the blank lines.
//!
//! glibc grows the buffer a record is printed into, 2048 bytes at first and
//! by 1024 to 131072, in a `static` the next call starts from; so does this,
//! so that a record too big for 131072 is refused the same way.

use core::sync::atomic::{AtomicUsize, Ordering};

use crate::list::List;
use crate::resolv::{NsMsg, NsRr};

/// The length `fp_query`, `p_query` and `p_cdname` take a message to have.
const PACKETSZ: i32 = 512;
/// The most bytes of a name as text, its NUL counted.
const MAXDNAME: usize = 1025;
/// The most bytes of a compressed name.
const MAXCDNAME: i32 = 255;

/// `RES_PRF_*`: which parts of a message `fp_nquery` prints.
const PRF_QUES: i32 = 0x10;
const PRF_ANS: i32 = 0x20;
const PRF_AUTH: i32 = 0x40;
const PRF_ADD: i32 = 0x80;
const PRF_HEAD1: i32 = 0x100;
const PRF_HEAD2: i32 = 0x200;
const PRF_HEADX: i32 = 0x800;

/// glibc's `res_opcodes`: each opcode's name.
const OPCODES: [&[u8]; 16] = [
    b"QUERY",
    b"IQUERY",
    b"CQUERYM",
    b"CQUERYU",
    b"NOTIFY",
    b"UPDATE",
    b"6",
    b"7",
    b"8",
    b"9",
    b"10",
    b"11",
    b"12",
    b"13",
    b"ZONEINIT",
    b"ZONEREF",
];

/// The sections' names: `__p_default_section_syms`, and
/// `__p_update_section_syms` for an UPDATE message.
const SECTIONS: [&[u8]; 4] = [b"QUERY", b"ANSWER", b"AUTHORITY", b"ADDITIONAL"];
const UPDATE_SECTIONS: [&[u8]; 4] = [b"ZONE", b"PREREQUISITE", b"UPDATE", b"ADDITIONAL"];
const NS_O_UPDATE: u16 = 5;

/// The size of the buffer a record is printed into: glibc's `static int
/// buflen`, which only grows, and which every call after starts from.
static BUFLEN: AtomicUsize = AtomicUsize::new(2048);

/// A stream written to; whatever it fails to take is lost, as glibc's
/// `fprintf` calls lose it (they are not checked).
struct Out(*mut u8);

impl Out {
    fn put(&mut self, b: &[u8]) {
        if !b.is_empty() {
            // SAFETY: the caller's stream (each printer's contract), and
            // `b`'s bytes.
            unsafe { crate::stdio::fwrite(b.as_ptr(), 1, b.len(), self.0) };
        }
    }

    fn num(&mut self, n: i64) {
        let mut buf = [0u8; 24];
        let mut i = buf.len();
        let neg = n < 0;
        let mut v = n.unsigned_abs();
        loop {
            i = i.saturating_sub(1);
            if let Some(slot) = buf.get_mut(i) {
                *slot = b'0'.wrapping_add((v % 10) as u8);
            }
            v /= 10;
            if v == 0 {
                break;
            }
        }
        if neg {
            i = i.saturating_sub(1);
            if let Some(slot) = buf.get_mut(i) {
                *slot = b'-';
            }
        }
        self.put(buf.get(i..).unwrap_or(&[]));
    }

    /// A C string's bytes.
    fn cstr(&mut self, s: *const u8) {
        if s.is_null() {
            return;
        }
        // SAFETY: a NUL-terminated string, the function that gave it says.
        let n = unsafe { crate::string::strlen(s) };
        // SAFETY: as above, `n` bytes before the NUL.
        self.put(unsafe { core::slice::from_raw_parts(s, n) });
    }

    /// `strerror(errno)`, as glibc's `%s` of it.
    fn errno_text(&mut self) {
        self.cstr(crate::string::strerror(crate::errno::get_errno()));
    }
}

/// A header flag of `h`, as `ns_msg_getflag` reads it.
fn flag(h: &NsMsg, mask: u16, shift: u16) -> u16 {
    (h.flags & mask) >> shift
}

/// The text before a NUL in `b`.
fn until_nul(b: &[u8]) -> &[u8] {
    let n = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    b.get(..n).unwrap_or(&[])
}

/// `fp_resstat` -- `statp`'s options, by name, on one line of `file`: `;;
/// res options: init recurs ...`.
///
/// # Safety
///
/// `statp` is a resolver state; `file` a stream.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn __fp_resstat(statp: *const crate::resolv::ResState, file: *mut u8) {
    let mut out = Out(file);
    out.put(b";; res options:");
    // SAFETY: a resolver state, the caller's contract.
    let options = unsafe { (*statp).options };
    let mut mask: u64 = 1;
    while mask != 0 {
        if options & mask != 0 {
            out.put(b" ");
            out.cstr(crate::res_debug::__p_option(mask));
        }
        mask = mask.wrapping_shl(1);
    }
    out.put(b"\n");
}

/// One section of a message: glibc's `do_section`.
fn section(out: &mut Out, pfcode: i32, h: &mut NsMsg, sect: i32, pflag: i32) {
    let sflag = pfcode & pflag;
    if pfcode != 0 && sflag == 0 {
        return;
    }
    let mut buf: List<u8> = List::new();
    if buf.resize(BUFLEN.load(Ordering::Relaxed), 0).is_err() {
        out.put(b";; memory allocation failure\n");
        return;
    }
    let opcode = flag(h, 0x7800, 11);
    let names = if opcode == NS_O_UPDATE {
        &UPDATE_SECTIONS
    } else {
        &SECTIONS
    };
    let sect_name = names.get(sect as usize).copied().unwrap_or(b"?");
    // SAFETY: a zeroed record is a valid one for `ns_parserr` to fill.
    let mut rr: NsRr = unsafe { core::mem::zeroed() };
    let mut rrnum = 0i32;
    loop {
        // SAFETY: a handle `ns_initparse` filled, over the caller's message.
        if unsafe { crate::resolv::ns_parserr(h, sect, rrnum, &raw mut rr) } != 0 {
            if crate::errno::get_errno() != crate::errno::ENODEV {
                out.put(b";; ns_parserr: ");
                out.errno_text();
                out.put(b"\n");
            } else if rrnum > 0 && sflag != 0 && pfcode & PRF_HEAD1 != 0 {
                out.put(b"\n");
            }
            return;
        }
        if rrnum == 0 && sflag != 0 && pfcode & PRF_HEAD1 != 0 {
            out.put(b";; ");
            out.put(sect_name);
            out.put(b" SECTION:\n");
        }
        if sect == 0 {
            out.put(b";;\t");
            out.put(until_nul(&rr.name));
            out.put(b", type = ");
            out.cstr(crate::res_debug::__p_type(i32::from(rr.type_)));
            out.put(b", class = ");
            out.cstr(crate::res_debug::__p_class(i32::from(rr.rr_class)));
            out.put(b"\n");
        } else {
            // SAFETY: the handle and the record `ns_parserr` filled from it,
            // and `buf`'s bytes.
            let n = unsafe {
                crate::nameser::ns_sprintrr(
                    h,
                    &raw const rr,
                    core::ptr::null(),
                    core::ptr::null(),
                    buf.as_mut_ptr(),
                    buf.len(),
                )
            };
            if n < 0 {
                if crate::errno::get_errno() == crate::errno::ENOSPC {
                    // A bigger buffer, as glibc's: 1024 more, to 131072.
                    let len = BUFLEN.load(Ordering::Relaxed);
                    buf.clear();
                    if len >= 131_072 {
                        out.put(b";; memory allocation failure\n");
                        return;
                    }
                    let len = len.saturating_add(1024);
                    BUFLEN.store(len, Ordering::Relaxed);
                    if buf.resize(len, 0).is_err() {
                        out.put(b";; memory allocation failure\n");
                        return;
                    }
                    continue;
                }
                out.put(b";; ns_sprintrr: ");
                out.errno_text();
                out.put(b"\n");
                return;
            }
            out.put(until_nul(&buf));
            out.put(b"\n");
        }
        rrnum = rrnum.saturating_add(1);
    }
}

/// `fp_nquery` -- the message `msg`, `len` bytes, printed to `file`: as
/// much of it as `_res.pfcode` asks for (all, when it is 0).
///
/// # Safety
///
/// `msg` holds `len` bytes; `file` is a stream.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn __fp_nquery(msg: *const u8, len: i32, file: *mut u8) {
    // glibc reads `_res.pfcode`, an unsigned long, as an int.
    // SAFETY: the process's resolver state.
    let pfcode = unsafe { (*crate::resolv::__res_state()).pfcode } as i32;
    // SAFETY: the caller's contract.
    unsafe { nquery(msg, len, file, pfcode) };
}

/// [`__fp_nquery`]'s body, given the print flags rather than reading
/// `_res`'s: the tests' way in, which no other thread's `res_init` can
/// reach.
///
/// # Safety
///
/// As [`__fp_nquery`].
unsafe fn nquery(msg: *const u8, len: i32, file: *mut u8, pfcode: i32) {
    let mut out = Out(file);
    // SAFETY: a zeroed handle is a valid one for `ns_initparse` to fill.
    let mut h: NsMsg = unsafe { core::mem::zeroed() };
    // SAFETY: `len` bytes at `msg`, the caller's contract.
    if unsafe { crate::resolv::ns_initparse(msg, len, &raw mut h) } < 0 {
        out.put(b";; ns_initparse: ");
        out.errno_text();
        out.put(b"\n");
        return;
    }
    let opcode = flag(&h, 0x7800, 11);
    let rcode = flag(&h, 0x000F, 0);
    let names = if opcode == NS_O_UPDATE {
        &UPDATE_SECTIONS
    } else {
        &SECTIONS
    };
    let counts = h.counts;
    if pfcode == 0 || pfcode & PRF_HEADX != 0 || rcode != 0 {
        out.put(b";; ->>HEADER<<- opcode: ");
        out.put(OPCODES.get(usize::from(opcode)).copied().unwrap_or(b"?"));
        out.put(b", status: ");
        out.cstr(crate::res_debug::__p_rcode(i32::from(rcode)));
        out.put(b", id: ");
        out.num(i64::from(h.id));
        out.put(b"\n");
    }
    if pfcode == 0 || pfcode & PRF_HEADX != 0 {
        out.put(b";");
    }
    if pfcode == 0 || pfcode & PRF_HEAD2 != 0 {
        out.put(b"; flags:");
        for (mask, shift, name) in [
            (0x8000, 15, &b" qr"[..]),
            (0x0400, 10, b" aa"),
            (0x0200, 9, b" tc"),
            (0x0100, 8, b" rd"),
            (0x0080, 7, b" ra"),
            (0x0040, 6, b" ??"),
            (0x0020, 5, b" ad"),
            (0x0010, 4, b" cd"),
        ] {
            if flag(&h, mask, shift) != 0 {
                out.put(name);
            }
        }
    }
    if pfcode == 0 || pfcode & PRF_HEAD1 != 0 {
        for (k, (name, count)) in names.iter().zip(counts).enumerate() {
            out.put(if k == 0 { b"; " } else { b", " });
            out.put(name);
            out.put(b": ");
            out.num(i64::from(count));
        }
    }
    if pfcode == 0 || pfcode & (PRF_HEADX | PRF_HEAD2 | PRF_HEAD1) != 0 {
        out.put(b"\n");
    }
    for (sect, pflag) in [(0, PRF_QUES), (1, PRF_ANS), (2, PRF_AUTH), (3, PRF_ADD)] {
        section(&mut out, pfcode, &mut h, sect, pflag);
    }
    if counts.iter().all(|&c| c == 0) {
        out.put(b"\n");
    }
}

/// `fp_query` -- [`__fp_nquery`] of a message taken to be `PACKETSZ`
/// (512) bytes long. `ns_initparse` refuses bytes after a message's last
/// record, so of a shorter one this prints only `;; ns_initparse: Message
/// too long`, as glibc's does.
///
/// # Safety
///
/// `msg` holds 512 bytes; `file` is a stream.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn __fp_query(msg: *const u8, file: *mut u8) {
    // SAFETY: the caller's contract.
    unsafe { __fp_nquery(msg, PACKETSZ, file) };
}

/// `p_query` -- [`__fp_query`] on standard output: of a message shorter
/// than 512 bytes, the same one line.
///
/// # Safety
///
/// `msg` holds 512 bytes.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn __p_query(msg: *const u8) {
    // SAFETY: the caller's contract; standard output is a stream.
    unsafe { __fp_query(msg, crate::stdio::stdout_stream()) };
}

/// `p_cdnname` -- the compressed name at `cp`, in the message `msg` of
/// `len` bytes, printed to `file` (the root as `.`): the byte after the
/// name, or NULL -- printing nothing -- for one that does not expand.
///
/// # Safety
///
/// `msg` holds `len` bytes, and `cp` points into them; `file` is a stream.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn __p_cdnname(
    cp: *const u8,
    msg: *const u8,
    len: i32,
    file: *mut u8,
) -> *const u8 {
    let mut name = [0u8; MAXDNAME];
    let eom = msg.wrapping_add(usize::try_from(len).unwrap_or(0));
    let n = crate::resolv::dn_expand(msg, eom, cp, name.as_mut_ptr(), MAXDNAME as i32);
    if n < 0 {
        return core::ptr::null();
    }
    let mut out = Out(file);
    let text = until_nul(&name);
    out.put(if text.is_empty() { b"." } else { text });
    cp.wrapping_add(n as usize)
}

/// `p_cdname` -- [`__p_cdnname`] in a message taken to be `PACKETSZ` (512)
/// bytes long.
///
/// # Safety
///
/// `msg` holds 512 bytes, and `cp` points into them; `file` is a stream.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn __p_cdname(cp: *const u8, msg: *const u8, file: *mut u8) -> *const u8 {
    // SAFETY: the caller's contract.
    unsafe { __p_cdnname(cp, msg, PACKETSZ, file) }
}

/// `p_fqnname` -- the compressed name at `cp` expanded into `name`
/// (`namelen` bytes), with a final dot: the byte after the name, or NULL
/// for one that does not expand or leaves no room for the dot. The message
/// ends `msglen` bytes past `cp`, not past `msg` -- glibc's reading.
///
/// # Safety
///
/// `msg` holds the message, which runs at least to `cp + msglen`; `name`
/// holds `namelen` bytes.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn __p_fqnname(
    cp: *const u8,
    msg: *const u8,
    msglen: i32,
    name: *mut u8,
    namelen: i32,
) -> *const u8 {
    let eom = cp.wrapping_add(usize::try_from(msglen).unwrap_or(0));
    let n = crate::resolv::dn_expand(msg, eom, cp, name, namelen);
    if n < 0 {
        return core::ptr::null();
    }
    // SAFETY: `dn_expand` wrote a NUL-terminated name into `name`.
    let newlen = unsafe { crate::string::strlen(name) };
    // SAFETY: `newlen` bytes before the NUL, inside `name`.
    let last = (newlen > 0).then(|| unsafe { name.add(newlen.wrapping_sub(1)).read() });
    if last != Some(b'.') {
        if newlen.saturating_add(1) >= usize::try_from(namelen).unwrap_or(0) {
            return core::ptr::null();
        }
        // SAFETY: `newlen + 2 <= namelen`: room for the dot and a NUL.
        unsafe {
            name.add(newlen).write(b'.');
            name.add(newlen.wrapping_add(1)).write(0);
        }
    }
    cp.wrapping_add(n as usize)
}

/// `p_fqname` -- [`__p_fqnname`] of the name at `cp`, the message taken to
/// run `MAXCDNAME` (255) bytes past it, printed to `file`.
///
/// # Safety
///
/// `msg` holds the message, which runs at least 255 bytes past `cp`;
/// `file` is a stream.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn __p_fqname(cp: *const u8, msg: *const u8, file: *mut u8) -> *const u8 {
    let mut name = [0u8; MAXDNAME];
    // SAFETY: the caller's contract; `name` holds MAXDNAME bytes.
    let n = unsafe { __p_fqnname(cp, msg, MAXCDNAME, name.as_mut_ptr(), MAXDNAME as i32) };
    if n.is_null() {
        return n;
    }
    Out(file).put(until_nul(&name));
    n
}

#[cfg(test)]
mod tests {
    use super::*;
    extern crate std;
    use std::format;
    use std::string::String;
    use std::vec;
    use std::vec::Vec;

    /// glibc 2.39's answers, one line a call
    /// (`posix/tools/oracle/resprint_harness.py`, which says the forms).
    const ORACLE: &str = include_str!("resprint_oracle.txt");

    fn token(b: &[u8]) -> String {
        if b.is_empty() {
            return String::from("\\x");
        }
        let mut s = String::new();
        for &c in b {
            if (0x21..=0x7E).contains(&c) && c != b'\\' {
                s.push(char::from(c));
            } else {
                s.push_str(&format!("\\x{c:02x}"));
            }
        }
        s
    }

    fn unhex(s: &str) -> Vec<u8> {
        if s == "-" {
            return Vec::new();
        }
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }

    /// What `f` writes to a stream, caught with `open_memstream`.
    fn capture(f: impl FnOnce(*mut u8)) -> Vec<u8> {
        let mut buf: *mut u8 = core::ptr::null_mut();
        let mut size = 0usize;
        // SAFETY: two writable locations for the stream's buffer and size.
        let s = unsafe { crate::stdio_mem::open_memstream(&raw mut buf, &raw mut size) };
        assert!(!s.is_null());
        f(s);
        assert_eq!(crate::stdio::fclose(s), 0);
        // SAFETY: the buffer the closed stream left, `size` bytes.
        let out = unsafe { core::slice::from_raw_parts(buf, size) }.to_vec();
        // SAFETY: the stream's buffer, now the caller's to free.
        unsafe { crate::malloc::free(buf) };
        out
    }

    fn ret(r: *const u8, base: *const u8) -> String {
        if r.is_null() {
            String::from("-")
        } else {
            format!("{}", r as usize - base as usize)
        }
    }

    /// Every printer's every call in the oracle, answered as glibc's.
    #[test]
    fn every_print_is_glibcs() {
        let mut wrong = Vec::new();
        let mut calls = 0;
        for (i, line) in ORACLE.lines().enumerate() {
            if line.starts_with('#') || line.is_empty() {
                continue;
            }
            let (lhs, want) = line.split_once(" = ").unwrap();
            let f: Vec<&str> = lhs.split(' ').collect();
            calls += 1;
            let got = match f[0] {
                "Q" => {
                    let pf = i32::from_str_radix(f[1], 16).unwrap();
                    let mut m = unhex(f[2]);
                    let len: i32 = f[3].parse().unwrap();
                    // A call may read one byte past: glibc's had the
                    // literal's NUL there.
                    m.push(0);
                    // SAFETY: `len` bytes at most of `m`; a stream.
                    token(&capture(|s| unsafe { nquery(m.as_ptr(), len, s, pf) }))
                }
                "Y" => {
                    let pf = i32::from_str_radix(f[1], 16).unwrap();
                    let mut m = unhex(f[2]);
                    m.resize(512, 0);
                    // SAFETY: 512 bytes, as fp_query reads; a stream.
                    token(&capture(|s| unsafe { nquery(m.as_ptr(), 512, s, pf) }))
                }
                "N" => {
                    let mut m = unhex(f[1]);
                    let off: usize = f[2].parse().unwrap();
                    let len: i32 = f[3].parse().unwrap();
                    m.push(0);
                    let mut r = core::ptr::null();
                    // SAFETY: `len` bytes of `m`, `off` inside them or at
                    // their end; a stream.
                    let text = capture(|s| unsafe {
                        r = __p_cdnname(m.as_ptr().add(off), m.as_ptr(), len, s);
                    });
                    format!("{} {}", ret(r, m.as_ptr()), token(&text))
                }
                "D" | "G" => {
                    let mut m = unhex(f[1]);
                    let off: usize = f[2].parse().unwrap();
                    m.resize(512, 0);
                    let mut r = core::ptr::null();
                    // SAFETY: 512 bytes, as the two read; a stream.
                    let text = capture(|s| unsafe {
                        let cp = m.as_ptr().add(off);
                        r = if f[0] == "D" {
                            __p_cdname(cp, m.as_ptr(), s)
                        } else {
                            __p_fqname(cp, m.as_ptr(), s)
                        };
                    });
                    format!("{} {}", ret(r, m.as_ptr()), token(&text))
                }
                "F" => {
                    let mut m = unhex(f[1]);
                    let off: usize = f[2].parse().unwrap();
                    let msglen: i32 = f[3].parse().unwrap();
                    let namelen: i32 = f[4].parse().unwrap();
                    m.resize(1024, 0);
                    let mut name = vec![0u8; 2048];
                    // SAFETY: the message runs to 1024 bytes, past `off +
                    // msglen`; `name` holds more than `namelen` bytes.
                    let r = unsafe {
                        __p_fqnname(
                            m.as_ptr().add(off),
                            m.as_ptr(),
                            msglen,
                            name.as_mut_ptr(),
                            namelen,
                        )
                    };
                    let n = name.iter().position(|&c| c == 0).unwrap_or(name.len());
                    format!("{} {}", ret(r, m.as_ptr()), token(&name[..n]))
                }
                "O" => {
                    let options = u64::from_str_radix(f[1], 16).unwrap();
                    // SAFETY: every field of a resolver state may be zero.
                    let mut st: crate::resolv::ResState = unsafe { core::mem::zeroed() };
                    st.options = options;
                    // SAFETY: a state and a stream.
                    token(&capture(|s| unsafe { __fp_resstat(&raw const st, s) }))
                }
                other => panic!("line {}: {other}", i + 1),
            };
            if got != want {
                wrong.push(format!(
                    "line {}: {lhs:.80}: {got:.300}, want {want:.300}",
                    i + 1
                ));
            }
        }
        for w in wrong.iter().take(20) {
            std::eprintln!("{w}");
        }
        assert!(
            wrong.is_empty(),
            "{} of {calls} calls are not glibc's",
            wrong.len()
        );
        assert!(calls > 900, "the oracle read as {calls} calls");
    }

    /// `fp_nquery` of a message `ns_initparse` refuses says why, and only
    /// that.
    #[test]
    fn a_message_that_does_not_parse_is_said_so() {
        let short = [0u8; 5];
        // SAFETY: 5 bytes; a stream.
        let text = capture(|s| unsafe { nquery(short.as_ptr(), 5, s, 0) });
        assert_eq!(text, b";; ns_initparse: Message too long\n");
    }

    /// `p_query` prints `fp_query`'s text, all of it, on standard output.
    #[test]
    fn p_query_prints_on_standard_output() {
        // An answer for www.example.org whose one TXT record fills it to
        // the 512 bytes p_query reads: ns_initparse refuses bytes past the
        // last record, so a shorter message would print only that.
        let mut m = unhex(concat!(
            "beef81800001000100000000",
            "03777777076578616d706c65036f726700",
            "00010001",
            "c00c001000010000003c01d3"
        ));
        m.push(255);
        m.extend(core::iter::repeat_n(b'x', 255));
        m.push(210);
        m.extend(core::iter::repeat_n(b'y', 210));
        assert_eq!(m.len(), 512);
        // SAFETY: 512 bytes; a stream.
        let want = capture(|s| unsafe { nquery(m.as_ptr(), 512, s, 0) });
        assert!(
            want.starts_with(b";; ->>HEADER<<- opcode: QUERY"),
            "{}",
            token(&want)
        );
        assert!(want.windows(4).any(|w| w == b"TXT\t"), "{}", token(&want));
        let printed = crate::stdio::capture_std_stream(crate::stdio::stdout_stream(), || {
            // SAFETY: 512 bytes. `_res.pfcode` is 0: no test sets it.
            unsafe { __p_query(m.as_ptr()) };
        });
        assert_eq!(token(&printed), token(&want));
    }
}
