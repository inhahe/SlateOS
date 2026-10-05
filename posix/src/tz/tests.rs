//! glibc 2.39's answers, replayed: `tz_oracle.txt`, which
//! `posix/tools/oracle/tz_harness.py` records under WSL, scenario by
//! scenario, each from a fresh process's state -- through the C entry points
//! (`tzset`, `localtime_r`, `mktime`, `strftime`), so that what is compared
//! is what a C program sees.
//!
//! The zoneinfo files glibc read are in the oracle too, and stand in here for
//! the file system the host build cannot open (its file calls are `ENOSYS`):
//! [`file_id`] and [`load_zone_file`] are the test build's.

use std::sync::Mutex;
use std::vec::Vec;

use super::{FileId, State, TzFile, ZONE_FILE};
use crate::time::Tm;

// ---------------------------------------------------------------------------
// The files
// ---------------------------------------------------------------------------

/// The test build's file system: path, bytes.
static FILES: Mutex<Vec<(Vec<u8>, Vec<u8>)>> = Mutex::new(Vec::new());

/// `path` without its NUL, and with each run of `/` one `/`, as the kernel
/// reads it -- `TZDIR=/usr/share/zoneinfo/` makes glibc open `…//…`.
fn normalise(path: &[u8]) -> Vec<u8> {
    let path = path.split(|&b| b == 0).next().unwrap_or(&[]);
    let mut out = Vec::with_capacity(path.len());
    for &b in path {
        if !(b == b'/' && out.last() == Some(&b'/')) {
            out.push(b);
        }
    }
    out
}

/// Make these the only files there are.
fn install_files(files: &[(Vec<u8>, Vec<u8>)]) {
    let mut fs = FILES
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    fs.clear();
    fs.extend(files.iter().map(|(p, b)| (normalise(p), b.clone())));
}

/// The test build's `stat`: a file's identity -- its place in the table, and
/// a fixed time -- or `None` when there is no such file.
pub(super) fn file_id(path: &[u8]) -> Option<FileId> {
    let want = normalise(path);
    let fs = FILES
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let i = fs.iter().position(|(p, _)| *p == want)?;
    Some((1, u64::try_from(i).ok()?.checked_add(1)?, 0))
}

/// The test build's read of a zone file into [`ZONE_FILE`].
pub(super) fn load_zone_file(path: &[u8]) -> Option<(TzFile<'static>, FileId)> {
    let want = normalise(path);
    let id = file_id(path)?;
    let bytes = {
        let fs = FILES
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        fs.iter().find(|(p, _)| *p == want)?.1.clone()
    };
    // SAFETY: the caller holds the zone lock, which guards the buffer.
    let buf = unsafe { &mut *core::ptr::addr_of_mut!(ZONE_FILE) };
    buf.get_mut(..bytes.len())?.copy_from_slice(&bytes);
    Some((super::parse_zone_file(bytes.len())?, id))
}

// ---------------------------------------------------------------------------
// The replay
// ---------------------------------------------------------------------------

static ORACLE: &str = include_str!("../tz_oracle.txt");

/// Scenarios whose answer here is, on purpose, not glibc's -- each with the
/// scenario whose glibc answer it is held to instead (see the module docs of
/// [`super`]).
const DEVIATIONS: &[(&str, &str)] = &[
    // A `..` component: never read as a file here, so the value is parsed as
    // a rule, which it is not -- glibc's answer for a value that names no
    // file and is no rule either, `ES5`.
    ("dotdot", "rule two letter name"),
];

/// `hex` of the harness: `-` for NULL, `""` for the empty string.
fn hex(p: *const u8) -> String {
    if p.is_null() {
        return "-".into();
    }
    // SAFETY: every name the zone code hands out is a NUL-terminated string.
    let bytes = unsafe { core::slice::from_raw_parts(p, crate::string::strlen(p)) };
    if bytes.is_empty() {
        return "\"\"".into();
    }
    hex_bytes(bytes)
}

/// `bytes` as lower-case hex digits.
fn hex_bytes(bytes: &[u8]) -> String {
    use core::fmt::Write;
    bytes.iter().fold(String::new(), |mut out, b| {
        // Writing to a `String` cannot fail.
        let _ = write!(out, "{b:02x}");
        out
    })
}

fn dehex(text: &str) -> Option<Vec<u8>> {
    match text {
        "unset" => None,
        "\"\"" => Some(Vec::new()),
        _ => Some(
            (0..text.len())
                .step_by(2)
                .map(|i| u8::from_str_radix(&text[i..i + 2], 16).expect("hex"))
                .collect(),
        ),
    }
}

/// `globals ()` of the harness.
fn globals() -> String {
    // SAFETY: plain reads of the C-visible variables; the caller holds the
    // environment lock, which serialises every writer.
    unsafe {
        let names = core::ptr::addr_of!(crate::time::tzname).read();
        format!(
            "{} {} {} {}",
            hex(names[0].0),
            hex(names[1].0),
            core::ptr::addr_of!(crate::time::timezone).read(),
            core::ptr::addr_of!(crate::time::daylight).read()
        )
    }
}

/// `marked ()` of the harness: every field recognisable.
fn marked() -> Tm {
    let mut tm = Tm::ZERO;
    tm.tm_sec = 77;
    tm.tm_min = 77;
    tm.tm_hour = 77;
    tm.tm_mday = 77;
    tm.tm_mon = 77;
    tm.tm_year = 77;
    tm.tm_wday = 77;
    tm.tm_yday = 777;
    tm.tm_isdst = 77;
    tm.tm_gmtoff = 77;
    tm.tm_zone = c"marked".as_ptr().cast();
    tm
}

/// `tmline ()` of the harness.
fn tmline(tm: &Tm) -> String {
    format!(
        "{} {} {} {} {} {} {} {} {} {} {}",
        tm.tm_sec,
        tm.tm_min,
        tm.tm_hour,
        tm.tm_mday,
        tm.tm_mon,
        tm.tm_year,
        tm.tm_wday,
        tm.tm_yday,
        tm.tm_isdst,
        tm.tm_gmtoff,
        hex(tm.tm_zone)
    )
}

fn set_env(name: &core::ffi::CStr, value: Option<&[u8]>) {
    match value {
        Some(v) => {
            let mut z = v.to_vec();
            z.push(0);
            // SAFETY: both strings are NUL-terminated and outlive the call.
            let rc = unsafe { crate::environ::setenv(name.as_ptr().cast(), z.as_ptr(), 1) };
            assert_eq!(rc, 0);
        }
        None => {
            // SAFETY: a NUL-terminated name.
            let rc = unsafe { crate::environ::unsetenv(name.as_ptr().cast()) };
            assert_eq!(rc, 0);
        }
    }
}

/// One oracle step, run here; the line it should print.
fn run_step(line: &str) -> String {
    let (head, _) = line.split_once(" | ").expect("step");
    let words: Vec<&str> = head.split(' ').collect();
    match words[0] {
        "tz" => {
            set_env(c"TZDIR", dehex(words[3]).as_deref());
            set_env(c"TZ", dehex(words[1]).as_deref());
            crate::time::tzset();
            format!("{head} | {}", globals())
        }
        "local" => {
            let t: i64 = words[1].parse().expect("secs");
            let mut tm = marked();
            // SAFETY: both pointers are valid for the call.
            let r = unsafe { crate::time::localtime_r(&raw const t, &raw mut tm) };
            format!(
                "{head} | {} {} | {}",
                i32::from(!r.is_null()),
                tmline(&tm),
                globals()
            )
        }
        "mk" => {
            let n: Vec<i32> = words[1..]
                .iter()
                .map(|w| w.parse().expect("field"))
                .collect();
            let mut tm = marked();
            tm.tm_year = n[0] - 1900;
            tm.tm_mon = n[1];
            tm.tm_mday = n[2];
            tm.tm_hour = n[3];
            tm.tm_min = n[4];
            tm.tm_sec = n[5];
            tm.tm_isdst = n[6];
            crate::errno::set_errno(0);
            let r = crate::time::mktime(&raw mut tm);
            let e = crate::errno::get_errno();
            format!("{head} | {r} {e} {} | {}", tmline(&tm), globals())
        }
        "strf" => {
            let t: i64 = words[1].parse().expect("secs");
            let mut tm = marked();
            // SAFETY: both pointers are valid for the call.
            unsafe { crate::time::localtime_r(&raw const t, &raw mut tm) };
            let mut buf = [0u8; 256];
            // SAFETY: the buffer, the format and the `tm` are valid.
            let n = unsafe {
                crate::time::strftime(
                    buf.as_mut_ptr(),
                    buf.len(),
                    c"%Z %z".as_ptr().cast(),
                    &raw const tm,
                )
            };
            let out = &buf[..n];
            let text = if out.is_empty() {
                "\"\"".to_string()
            } else {
                hex_bytes(out)
            };
            format!("{head} | {text}")
        }
        other => panic!("unknown step {other}"),
    }
}

/// A file the oracle records: its path, and its bytes.
type File = (Vec<u8>, Vec<u8>);
/// A scenario: its name, and its steps' lines.
type Scenario = (String, Vec<String>);

/// The oracle: its files, and its scenarios.
fn oracle() -> (Vec<File>, Vec<Scenario>) {
    let mut files = Vec::new();
    let mut scenarios: Vec<(String, Vec<String>)> = Vec::new();
    for line in ORACLE.lines() {
        if let Some(rest) = line.strip_prefix("file ") {
            let (path, data) = rest.split_once(' ').expect("file line");
            files.push((path.as_bytes().to_vec(), dehex(data).expect("bytes")));
        } else if let Some(name) = line.strip_prefix("=== ") {
            scenarios.push((name.to_string(), Vec::new()));
        } else if let Some((_, lines)) = scenarios.last_mut() {
            lines.push(line.to_string());
        }
    }
    (files, scenarios)
}

#[test]
fn every_scenario_is_glibcs() {
    let _env = crate::environ::lock_env_for_test();
    let saved_tz = crate::environ::getenv_bytes(b"TZ").map(<[u8]>::to_vec);
    let saved_dir = crate::environ::getenv_bytes(b"TZDIR").map(<[u8]>::to_vec);
    let (files, scenarios) = oracle();
    assert!(
        scenarios.len() > 50,
        "the oracle holds {} scenarios",
        scenarios.len()
    );
    install_files(&files);

    let mut failures = Vec::new();
    let mut compared = 0usize;
    for (name, lines) in &scenarios {
        let expected: Vec<String> = match DEVIATIONS.iter().find(|(d, _)| d == name) {
            None => lines.clone(),
            Some((_, instead)) => {
                let (_, theirs) = scenarios
                    .iter()
                    .find(|(n, _)| n == instead)
                    .expect("reference");
                // The reference's answers, under this scenario's own `TZ`.
                let own = lines[0].split_once(" | ").expect("tz line").0;
                let mut out = theirs.clone();
                let rest = out[0].split_once(" | ").expect("tz line").1.to_string();
                out[0] = format!("{own} | {rest}");
                out
            }
        };
        super::reset_for_test();
        assert_eq!(
            lines.len(),
            expected.len(),
            "{name}: the reference has as many steps"
        );
        for (line, want) in lines.iter().zip(&expected) {
            let got = run_step(line);
            compared += 1;
            if got != *want {
                failures.push(format!("{name}:\n    glibc: {want}\n    here:  {got}"));
            }
        }
    }
    // Every step of every scenario, not a parse that found none.
    let steps: usize = scenarios.iter().map(|(_, l)| l.len()).sum();
    assert_eq!(compared, steps);
    assert!(compared > 1000, "only {compared} lines compared");

    install_files(&[]);
    set_env(c"TZ", saved_tz.as_deref());
    set_env(c"TZDIR", saved_dir.as_deref());
    super::reset_for_test();
    assert!(
        failures.is_empty(),
        "{} of the oracle's lines differ:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn a_name_is_interned_once_and_shares_a_tail() {
    let _env = crate::environ::lock_env_for_test();
    super::reset_for_test();
    let lock = super::Locked::take();
    let edt = super::tzstring(b"EDT").expect("room");
    let again = super::tzstring(b"EDT").expect("room");
    let dt = super::tzstring(b"DT").expect("room");
    assert_eq!(edt, again);
    // "If this string is the same as the end of an already-allocated string,
    // it can share space."
    // SAFETY: both point into the arena, at most 3 bytes apart.
    assert_eq!(unsafe { edt.ptr().add(1) }, dt.ptr());
    drop(lock);
    super::reset_for_test();
}

#[test]
fn a_full_arena_refuses_the_name() {
    let _env = crate::environ::lock_env_for_test();
    super::reset_for_test();
    let lock = super::Locked::take();
    let big = [b'A'; super::ARENA_CAP];
    assert!(
        super::tzstring(&big).is_none(),
        "a name the arena cannot hold is refused"
    );
    assert!(
        super::tzstring(b"FIT").is_some(),
        "and refusing it costs nothing"
    );
    drop(lock);
    super::reset_for_test();
}

/// `tzname[0]` and `timezone` as they stand.
fn std_name_and_west() -> (Vec<u8>, i64) {
    // SAFETY: reads of the C-visible variables under the environment lock.
    unsafe {
        let names = core::ptr::addr_of!(crate::time::tzname).read();
        let p = names[0].0;
        let name = core::slice::from_raw_parts(p, crate::string::strlen(p)).to_vec();
        (name, core::ptr::addr_of!(crate::time::timezone).read())
    }
}

#[test]
fn a_name_too_long_for_a_path_is_read_as_a_rule() {
    let _env = crate::environ::lock_env_for_test();
    let long = vec![b'A'; crate::unistd::PATH_MAX];
    assert!(super::zone_path(&long).is_none(), "no path could hold it");
    // glibc's open fails with ENAMETOOLONG, and the value is parsed as a
    // rule; here the path is never built, with the same result.
    super::reset_for_test();
    let mut tz = long.clone();
    tz.extend_from_slice(b"5");
    set_env(c"TZ", Some(&tz));
    crate::time::tzset();
    let (name, west) = std_name_and_west();
    assert_eq!((name.len(), west), (long.len(), 5 * 3600));
    set_env(c"TZ", None);
    super::reset_for_test();
}

#[test]
fn a_file_that_is_not_tzif_is_no_zone() {
    let _env = crate::environ::lock_env_for_test();
    super::reset_for_test();
    install_files(&[(
        b"/usr/share/zoneinfo/Bad".to_vec(),
        b"not a zone file".to_vec(),
    )]);
    set_env(c"TZ", Some(b"Bad"));
    crate::time::tzset();
    // glibc's `lose:`: no file, so the name is parsed as a rule -- a name
    // with no offset, which is UTC under that name.
    assert_eq!(std_name_and_west(), (b"Bad".to_vec(), 0));
    install_files(&[]);
    set_env(c"TZ", None);
    super::reset_for_test();
}

#[test]
fn a_tz_too_long_to_remember_is_read_each_time() {
    let _env = crate::environ::lock_env_for_test();
    super::reset_for_test();
    let mut long = vec![b'A'; super::OLD_TZ_CAP + 10];
    long.extend_from_slice(b"5");
    set_env(c"TZ", Some(&long));
    crate::time::tzset();
    {
        let mut lock = super::Locked::take();
        let state: &mut State = lock.state();
        assert!(state.old_tz_len.is_none(), "too long to keep");
        assert_eq!(state.rules[0].offset, -5 * 3600);
    }
    set_env(c"TZ", None);
    super::reset_for_test();
}
