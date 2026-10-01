//! Replays glibc 2.39's answers for the account-file functions -- the
//! stream and string readers, the writers, `getusershell`, `getpass` --
//! from `accounts_oracle.txt`, which `posix/tools/oracle/accounts_harness.py`
//! writes: the inputs and glibc's answers, one call a line (the harness's
//! docstring has the format). Where this library answers otherwise on
//! purpose, the test says so where it allows it.
//!
//! `lckpwdf` needs a file to lock, which the host tests have none of; its
//! lines in the oracle are what `shadow.rs`'s own tests reproduce by other
//! means (the clock, the states).

use crate::errno;
use crate::nss_files::{Which, c_bytes, set_test_text};
use crate::pwd::{
    Group, Passwd, fgetgrent, fgetgrent_r, fgetpwent, fgetpwent_r, putgrent, putpwent,
};
use crate::shadow::{Spwd, fgetspent, fgetspent_r, putspent, sgetspent, sgetspent_r};
use std::boxed::Box;
use std::collections::BTreeMap;
use std::format;
use std::string::{String, ToString};
use std::vec::Vec;

const ORACLE: &str = include_str!("accounts_oracle.txt");

/// The sentinel the harness puts in `errno` before every call.
const UNTOUCHED: i32 = 1234;

/// The harness's escaping: bytes outside `!`..`~`, and `|\,[]`, as `\xHH`.
fn escape(bytes: &[u8]) -> String {
    let mut out = String::new();
    for &b in bytes {
        if (0x21..=0x7e).contains(&b) && !b"|\\,[]".contains(&b) {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("\\x{b:02x}"));
        }
    }
    out
}

/// And back.
fn unescape(s: &str) -> Vec<u8> {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'\\' && b.get(i + 1) == Some(&b'x') {
            let hex = core::str::from_utf8(&b[i + 2..i + 4]).unwrap();
            out.push(u8::from_str_radix(hex, 16).unwrap());
            i += 4;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    out
}

/// A string field as the harness writes one: `=` and its bytes, `!` for NULL.
fn field(p: *const u8) -> String {
    if p.is_null() {
        "!".to_string()
    } else {
        // SAFETY: the library's C strings.
        format!("={}", escape(unsafe { c_bytes(p) }))
    }
}

/// A field written that way, back: `None` for NULL.
fn parse_field(s: &str) -> Option<Vec<u8>> {
    match s.strip_prefix('=') {
        Some(rest) => Some(unescape(rest)),
        None => {
            assert_eq!(s, "!", "not a field: {s}");
            None
        }
    }
}

fn show_pw(p: &Passwd) -> String {
    format!(
        "{}|{}|{}|{}|{}|{}|{}",
        field(p.pw_name),
        field(p.pw_passwd),
        p.pw_uid,
        p.pw_gid,
        field(p.pw_gecos),
        field(p.pw_dir),
        field(p.pw_shell)
    )
}

fn show_gr(g: &Group) -> String {
    let mut out = format!("{}|{}|{}|", field(g.gr_name), field(g.gr_passwd), g.gr_gid);
    if g.gr_mem.is_null() {
        out.push('!');
        return out;
    }
    out.push('[');
    let mut at = g.gr_mem;
    // SAFETY: the library's NULL-terminated list.
    while !unsafe { at.read() }.is_null() {
        if at != g.gr_mem {
            out.push(',');
        }
        // SAFETY: as above.
        out.push_str(&field(unsafe { at.read() }));
        // SAFETY: the terminator is still ahead.
        at = unsafe { at.add(1) };
    }
    out.push(']');
    out
}

fn show_sp(s: &Spwd) -> String {
    format!(
        "{}|{}|{}|{}|{}|{}|{}|{}|{}",
        field(s.sp_namp),
        field(s.sp_pwdp),
        s.sp_lstchg,
        s.sp_min,
        s.sp_max,
        s.sp_warn,
        s.sp_inact,
        s.sp_expire,
        s.sp_flag
    )
}

/// The oracle's input files, by kind and name: `None` for one not there.
type Texts = BTreeMap<(String, String), Option<Vec<u8>>>;

/// Its input records, by kind and case.
type Records = BTreeMap<(String, String), String>;

/// The oracle's inputs: `T <kind> <name> <text>` -- `None` for a file that
/// is not there -- and the records `C <kind> <case> <record>`.
fn inputs() -> (Texts, Records) {
    let mut texts = BTreeMap::new();
    let mut records = BTreeMap::new();
    for line in ORACLE.lines() {
        let mut w = line.splitn(4, ' ');
        match w.next() {
            Some("T") => {
                let (kind, name, text) = (w.next().unwrap(), w.next().unwrap(), w.next().unwrap());
                texts.insert((kind.to_string(), name.to_string()), parse_field(text));
            }
            Some("C") => {
                let (kind, name, rec) = (w.next().unwrap(), w.next().unwrap(), w.next().unwrap());
                records.insert((kind.to_string(), name.to_string()), rec.to_string());
            }
            _ => {}
        }
    }
    (texts, records)
}

/// A read-only stream over `text`, as the harness's `tmpfile` holding it.
fn stream_of(text: &[u8]) -> *mut u8 {
    let bytes: &'static mut [u8] = Box::leak(text.to_vec().into_boxed_slice());
    // SAFETY: a leaked buffer of the length given, which outlives the stream.
    let s = unsafe {
        crate::stdio_mem::fmemopen(bytes.as_mut_ptr().cast(), bytes.len(), c"r".as_ptr().cast())
    };
    assert!(!s.is_null(), "fmemopen");
    s
}

/// The kind of file each reader reads.
fn kind_of(f: &str) -> &'static str {
    match f {
        "fgetpwent_r" | "fgetpwent" => "pw",
        "fgetgrent_r" | "fgetgrent" => "gr",
        _ => "sp",
    }
}

/// One reader call on `stream` as the oracle names it: its answer, and what
/// it read, shown as the harness shows it (`-` for none).
fn read_one(f: &str, stream: *mut u8, buflen: usize) -> (i32, String) {
    let mut buf = std::vec![0u8; buflen];
    // SAFETY: an open stream; `buf` holds `buflen` bytes; the structures
    // and result pointers are this frame's.
    unsafe {
        match f {
            "fgetpwent_r" => {
                let mut e = Passwd::EMPTY;
                let mut r: *const Passwd = core::ptr::null();
                let rc = fgetpwent_r(stream, &raw mut e, buf.as_mut_ptr(), buflen, &raw mut r);
                (
                    rc,
                    if r.is_null() {
                        "-".into()
                    } else {
                        show_pw(&*r)
                    },
                )
            }
            "fgetgrent_r" => {
                let mut e = Group::EMPTY;
                let mut r: *const Group = core::ptr::null();
                let rc = fgetgrent_r(stream, &raw mut e, buf.as_mut_ptr(), buflen, &raw mut r);
                (
                    rc,
                    if r.is_null() {
                        "-".into()
                    } else {
                        show_gr(&*r)
                    },
                )
            }
            "fgetspent_r" => {
                let mut e = Spwd::EMPTY;
                let mut r: *const Spwd = core::ptr::null();
                let rc = fgetspent_r(stream, &raw mut e, buf.as_mut_ptr(), buflen, &raw mut r);
                (
                    rc,
                    if r.is_null() {
                        "-".into()
                    } else {
                        show_sp(&*r)
                    },
                )
            }
            "fgetpwent" => {
                let p = fgetpwent(stream);
                (
                    0,
                    if p.is_null() {
                        "-".into()
                    } else {
                        show_pw(&*p)
                    },
                )
            }
            "fgetgrent" => {
                let p = fgetgrent(stream);
                (
                    0,
                    if p.is_null() {
                        "-".into()
                    } else {
                        show_gr(&*p)
                    },
                )
            }
            "fgetspent" => {
                let p = fgetspent(stream);
                (
                    0,
                    if p.is_null() {
                        "-".into()
                    } else {
                        show_sp(&*p)
                    },
                )
            }
            other => panic!("no reader {other}"),
        }
    }
}

#[test]
fn the_stream_readers_answer_as_glibc_does() {
    let (texts, _) = inputs();
    let mut bad = Vec::new();
    let mut calls = 0;
    let mut stream: *mut u8 = core::ptr::null_mut();
    for line in ORACLE
        .lines()
        .filter(|l| l.starts_with("R ") || l.starts_with("N "))
    {
        let (lhs, rhs) = line.split_once(" = ").unwrap();
        let w: Vec<&str> = lhs.split(' ').collect();
        let (f, name, i) = (w[1], w[2], w[3].parse::<usize>().unwrap());
        // `_r` lines name the buffer; the others grow their own.
        let buflen = w.get(4).map_or(0, |b| b.parse().unwrap());
        if i == 0 {
            if !stream.is_null() {
                crate::stdio::fclose(stream);
            }
            let text = texts[&(kind_of(f).to_string(), name.to_string())]
                .clone()
                .unwrap();
            stream = stream_of(&text);
        }
        errno::set_errno(UNTOUCHED);
        let (rc, entry) = read_one(f, stream, buflen);
        let err = errno::get_errno();
        let got = if w[0] == "R" {
            format!("{rc} {err} {entry}")
        } else {
            format!("{err} {entry}")
        };
        calls += 1;
        if got != rhs {
            bad.push(format!("{line}\n    ours {got}"));
        }
    }
    if !stream.is_null() {
        crate::stdio::fclose(stream);
    }
    assert_eq!(calls, 142, "the oracle's reader calls");
    assert!(
        bad.is_empty(),
        "{} of {calls} differ:\n{}",
        bad.len(),
        bad.join("\n")
    );
}

#[test]
fn sgetspent_answers_as_glibc_does_but_for_a_string_that_is_no_entry() {
    let (_, records) = inputs();
    let mut bad = Vec::new();
    let mut calls = 0;
    for line in ORACLE.lines().filter(|l| l.starts_with("S ")) {
        let (lhs, rhs) = line.split_once(" = ").unwrap();
        let w: Vec<&str> = lhs.split(' ').collect();
        let (f, name) = (w[1], w[2]);
        let mut string = parse_field(&records[&("sget".to_string(), name.to_string())]).unwrap();
        string.push(0);
        let want: Vec<&str> = rhs.splitn(3, ' ').collect();
        let (mut want_rc, mut want_err) = (
            want[0].parse::<i32>().unwrap(),
            want[1].parse::<i32>().unwrap(),
        );
        let want_entry = want[2];
        errno::set_errno(UNTOUCHED);
        let (rc, entry) = if f == "sgetspent_r" {
            let buflen: usize = w[3].parse().unwrap();
            // A string that is no entry: glibc returns whatever `errno`
            // held -- here the sentinel -- and leaves it; this says
            // EINVAL, both ways (design-decisions §1137).
            if want_entry == "-" && want_rc == UNTOUCHED {
                (want_rc, want_err) = (errno::EINVAL, errno::EINVAL);
            }
            let mut buf = std::vec![0u8; buflen];
            let mut e = Spwd::EMPTY;
            let mut r: *const Spwd = core::ptr::null();
            // SAFETY: a C string; `buf` holds `buflen` bytes; the rest is
            // this frame's.
            let rc = unsafe {
                sgetspent_r(
                    string.as_ptr(),
                    &raw mut e,
                    buf.as_mut_ptr(),
                    buflen,
                    &raw mut r,
                )
            };
            // SAFETY: the entry, when there is one.
            (
                rc,
                if r.is_null() {
                    "-".into()
                } else {
                    show_sp(unsafe { &*r })
                },
            )
        } else {
            // The same, for the non-reentrant form: NULL with EINVAL.
            if want_entry == "-" && want_err == UNTOUCHED {
                want_err = errno::EINVAL;
            }
            // SAFETY: a C string.
            let p = unsafe { sgetspent(string.as_ptr()) };
            // SAFETY: the entry, when there is one.
            (
                0,
                if p.is_null() {
                    "-".into()
                } else {
                    show_sp(unsafe { &*p })
                },
            )
        };
        let err = errno::get_errno();
        calls += 1;
        let got = format!("{rc} {err} {entry}");
        let want = format!("{want_rc} {want_err} {want_entry}");
        if got != want {
            bad.push(format!("{line}\n    want {want}\n    ours {got}"));
        }
    }
    assert_eq!(calls, 79, "the oracle's sgetspent calls");
    assert!(
        bad.is_empty(),
        "{} of {calls} differ:\n{}",
        bad.len(),
        bad.join("\n")
    );
}

/// NUL-terminated copies of a record's strings, kept alive for a call.
#[derive(Default)]
struct Strings(Vec<Vec<u8>>);

impl Strings {
    fn c(&mut self, s: Option<Vec<u8>>) -> *const u8 {
        match s {
            None => core::ptr::null(),
            Some(mut v) => {
                v.push(0);
                self.0.push(v);
                self.0.last().unwrap().as_ptr()
            }
        }
    }
}

/// `f` writing into a memory stream: its answer, `errno`, and the bytes.
fn written(f: impl FnOnce(*mut u8) -> i32) -> (i32, i32, Vec<u8>) {
    let mut out: *mut u8 = core::ptr::null_mut();
    let mut len = 0usize;
    // SAFETY: a pair of out-pointers.
    let stream = unsafe { crate::stdio_mem::open_memstream(&raw mut out, &raw mut len) };
    assert!(!stream.is_null());
    errno::set_errno(UNTOUCHED);
    let rc = f(stream);
    let err = errno::get_errno();
    assert_eq!(crate::stdio::fclose(stream), 0);
    // SAFETY: open_memstream's block of `len` bytes.
    let bytes = unsafe { core::slice::from_raw_parts(out, len) }.to_vec();
    // SAFETY: the stream's block, the caller's to free.
    unsafe { crate::malloc::free(out) };
    (rc, err, bytes)
}

#[test]
fn the_writers_write_what_glibc_writes() {
    let (_, records) = inputs();
    let mut bad = Vec::new();
    let mut calls = 0;
    for line in ORACLE.lines().filter(|l| l.starts_with("P ")) {
        let (lhs, rhs) = line.split_once(" = ").unwrap();
        let w: Vec<&str> = lhs.split(' ').collect();
        let (f, name) = (w[1], w[2]);
        let kind = match f {
            "putpwent" => "pw",
            "putgrent" => "gr",
            _ => "sp",
        };
        let rec = &records[&(kind.to_string(), name.to_string())];
        let parts: Vec<&str> = rec.split('|').collect();
        let mut keep = Strings::default();
        let (rc, err, bytes) = match kind {
            "pw" => {
                let p = Passwd {
                    pw_name: keep.c(parse_field(parts[0])),
                    pw_passwd: keep.c(parse_field(parts[1])),
                    pw_uid: parts[2].parse().unwrap(),
                    pw_gid: parts[3].parse().unwrap(),
                    pw_gecos: keep.c(parse_field(parts[4])),
                    pw_dir: keep.c(parse_field(parts[5])),
                    pw_shell: keep.c(parse_field(parts[6])),
                };
                // SAFETY: a record of C strings; an open stream.
                written(|s| unsafe { putpwent(&raw const p, s) })
            }
            "gr" => {
                let members: Option<Vec<*const u8>> = (parts[3] != "!").then(|| {
                    let inner = parts[3]
                        .strip_prefix('[')
                        .unwrap()
                        .strip_suffix(']')
                        .unwrap();
                    let mut v: Vec<*const u8> = inner
                        .split(',')
                        .filter(|m| !m.is_empty())
                        .map(|m| keep.c(parse_field(m)))
                        .collect();
                    v.push(core::ptr::null());
                    v
                });
                let g = Group {
                    gr_name: keep.c(parse_field(parts[0])),
                    gr_passwd: keep.c(parse_field(parts[1])),
                    gr_gid: parts[2].parse().unwrap(),
                    gr_mem: members.as_ref().map_or(core::ptr::null(), |v| v.as_ptr()),
                };
                // SAFETY: as above, the list NULL-terminated.
                written(|s| unsafe { putgrent(&raw const g, s) })
            }
            _ => {
                let n = |i: usize| parts[i].parse::<i64>().unwrap();
                let sp = Spwd {
                    sp_namp: keep.c(parse_field(parts[0])),
                    sp_pwdp: keep.c(parse_field(parts[1])),
                    sp_lstchg: n(2),
                    sp_min: n(3),
                    sp_max: n(4),
                    sp_warn: n(5),
                    sp_inact: n(6),
                    sp_expire: n(7),
                    sp_flag: parts[8].parse().unwrap(),
                };
                // SAFETY: as above.
                written(|s| unsafe { putspent(&raw const sp, s) })
            }
        };
        calls += 1;
        let got = format!("{rc} {err} ={}", escape(&bytes));
        if got != rhs {
            bad.push(format!("{line}\n    ours {got}"));
        }
    }
    assert_eq!(calls, 35, "the oracle's writer calls");
    assert!(
        bad.is_empty(),
        "{} of {calls} differ:\n{}",
        bad.len(),
        bad.join("\n")
    );
}

/// Every shell `getusershell` hands out from a fresh start, shown as the
/// harness shows the list.
fn shells_listed() -> String {
    crate::usershell::endusershell();
    let mut out = Vec::new();
    loop {
        let p = crate::usershell::getusershell();
        if p.is_null() {
            break;
        }
        out.push(field(p));
    }
    format!("[{}]", out.join(","))
}

/// The next shell alone, as `[<shell>]` or `[]`.
fn one_shell() -> String {
    let p = crate::usershell::getusershell();
    if p.is_null() {
        "[]".into()
    } else {
        format!("[{}]", field(p))
    }
}

#[test]
fn getusershell_lists_what_glibc_lists() {
    let (texts, _) = inputs();
    let mut want = BTreeMap::new();
    for line in ORACLE.lines().filter(|l| l.starts_with("U ")) {
        let (lhs, rhs) = line.split_once(" = ").unwrap();
        want.insert(lhs[2..].to_string(), rhs.to_string());
    }
    for ((kind, name), text) in &texts {
        if kind != "sh" {
            continue;
        }
        let leaked: Option<&'static [u8]> = text.clone().map(|t| &*Box::leak(t.into_boxed_slice()));
        set_test_text(Which::Shells, leaked);
        assert_eq!(&shells_listed(), &want[name.as_str()], "/etc/shells {name}");
    }
    // The state machine, as the harness drives it: two reads from a fresh
    // start, two past the end, a rewind, an end.
    let two = texts[&("sh".to_string(), "two".to_string())]
        .clone()
        .unwrap();
    set_test_text(Which::Shells, Some(Box::leak(two.into_boxed_slice())));
    crate::usershell::endusershell();
    let mut got = Vec::new();
    for _ in 0..4 {
        got.push(one_shell());
    }
    crate::usershell::setusershell();
    got.push(one_shell());
    crate::usershell::endusershell();
    got.push(one_shell());
    let expect: Vec<String> = ["a", "b", "c", "d", "e", "f"]
        .iter()
        .map(|s| want[&format!("seq-{s}")].clone())
        .collect();
    assert_eq!(got, expect);
    crate::usershell::endusershell();
}

#[test]
fn getpass_reads_what_glibc_reads_and_prompts_as_it_prompts() {
    let mut answers = Vec::new();
    let mut prompts = String::new();
    for line in ORACLE.lines().filter(|l| l.starts_with("G ")) {
        let (lhs, rhs) = line.split_once(" = ").unwrap();
        if lhs == "G prompts" {
            prompts = rhs.to_string();
        } else {
            answers.push(rhs.to_string());
        }
    }
    assert_eq!(answers.len(), 3);
    // The harness's input, and its prompts `P0:`..`P2:`.
    let input = stream_of(b"secret\nline2");
    let mut got = Vec::new();
    let (_, _, out) = written(|output| {
        for i in 0..3 {
            let prompt = format!("P{i}:\0");
            // SAFETY: open streams, a C string.
            let r = unsafe { crate::getpass::getpass_on(prompt.as_ptr(), input, output) };
            got.push(field(r));
        }
        0
    });
    crate::stdio::fclose(input);
    assert_eq!(got, answers);
    assert_eq!(format!("={}", escape(&out)), prompts);
}
