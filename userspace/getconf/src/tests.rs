//! `getconf`'s behaviour against a stand-in C library whose answers the tests
//! choose, so each of upstream's branches is reached on any host. What the
//! real library answers is `scripts/getconf-diff.sh`'s to measure.

// A failed expectation is the test's report; panicking on it is the point.
#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing
)]

use super::*;
use std::collections::HashMap;
use std::ffi::CString;

/// A library answering from tables: `sysconf` and `confstr` by constant,
/// `pathconf` by constant (any path), or failing with an errno for a path.
#[derive(Default)]
struct Fake {
    sysconf: HashMap<i32, i64>,
    confstr: HashMap<i32, &'static [u8]>,
    pathconf: HashMap<i32, i64>,
    path_errno: Option<(Vec<u8>, i32)>,
}

impl Library for Fake {
    fn sysconf(&self, name: i32) -> i64 {
        self.sysconf.get(&name).copied().unwrap_or(-1)
    }
    fn pathconf(&self, path: &CString, name: i32) -> Result<Option<i64>, i32> {
        if let Some((p, errno)) = &self.path_errno
            && p.as_slice() == path.as_bytes()
        {
            return Err(*errno);
        }
        Ok(self.pathconf.get(&name).copied())
    }
    fn confstr(&self, name: i32, buf: Option<&mut [u8]>) -> usize {
        let Some(value) = self.confstr.get(&name) else {
            return 0;
        };
        let need = value.len() + 1;
        if let Some(buf) = buf {
            let n = buf.len().min(need);
            for (i, slot) in buf.iter_mut().take(n).enumerate() {
                *slot = value.get(i).copied().unwrap_or(0);
            }
        }
        need
    }
}

/// `getconf ARGS...` against `lib`: (status, stdout, stderr).
fn getconf_as(argv0: &str, args: &[&str], lib: &Fake) -> (u8, String, String) {
    let args: Vec<Vec<u8>> = args.iter().map(|a| a.as_bytes().to_vec()).collect();
    let mut out = Vec::new();
    let mut err = Vec::new();
    let Exit(status) = {
        let mut io = Io {
            out: &mut out,
            err: &mut err,
            invocation: argv0.as_bytes().to_vec(),
        };
        run(&args, lib, &mut io)
    };
    (
        status,
        String::from_utf8(out).expect("utf-8 stdout"),
        String::from_utf8(err).expect("utf-8 stderr"),
    )
}

fn getconf(args: &[&str], lib: &Fake) -> (u8, String, String) {
    getconf_as("getconf", args, lib)
}

/// The constant a name is answered with, from the generated table.
fn constant(name: &str) -> Call {
    vars::VARS
        .iter()
        .find(|v| v.name == name)
        .map(|v| v.call)
        .expect("a name in the table")
}

fn sc(name: &str) -> i32 {
    match constant(name) {
        Call::Sysconf(c) => c,
        other => panic!("{name} is {other:?}"),
    }
}

#[test]
fn the_table_is_upstreams_whole_and_in_order() {
    // 320 rows -- `getconf -a | wc -l` on the reference system -- from
    // LINK_MAX to the last programming environment, as upstream lists them.
    assert_eq!(vars::VARS.len(), 320);
    assert_eq!(vars::VARS.first().map(|v| v.name), Some("LINK_MAX"));
    let names: Vec<&str> = vars::VARS.iter().map(|v| v.name).collect();
    let pos = |n: &str| names.iter().position(|&x| x == n).expect(n);
    assert!(pos("ARG_MAX") < pos("PAGESIZE"));
    assert_eq!(pos("_POSIX_LINK_MAX"), pos("LINK_MAX") + 1);
}

#[test]
fn every_constant_slateos_defines_has_the_value_the_table_gives_it() {
    // The table's values are glibc's headers'; SlateOS's C library must give
    // each constant it defines the same number, or getconf would ask it for
    // the wrong thing. Every `_SC_`, `_PC_` and `_CS_` constant `posix`
    // declares is checked against every row that names it.
    // Named by full path: a `use` of `posix::unistd` imports a stateful
    // module, which the one-libc gate refuses even in a test; its
    // constants are plain integers and may be read where they live.
    let ours: &[(&str, i32)] = &[
        ("_SC_ARG_MAX", posix::unistd::_SC_ARG_MAX),
        ("_SC_CHILD_MAX", posix::unistd::_SC_CHILD_MAX),
        ("_SC_CLK_TCK", posix::unistd::_SC_CLK_TCK),
        ("_SC_NGROUPS_MAX", posix::unistd::_SC_NGROUPS_MAX),
        ("_SC_OPEN_MAX", posix::unistd::_SC_OPEN_MAX),
        ("_SC_STREAM_MAX", posix::unistd::_SC_STREAM_MAX),
        ("_SC_TZNAME_MAX", posix::unistd::_SC_TZNAME_MAX),
        ("_SC_MQ_OPEN_MAX", posix::unistd::_SC_MQ_OPEN_MAX),
        ("_SC_MQ_PRIO_MAX", posix::unistd::_SC_MQ_PRIO_MAX),
        ("_SC_VERSION", posix::unistd::_SC_VERSION),
        ("_SC_PAGESIZE", posix::unistd::_SC_PAGESIZE),
        ("_SC_SEM_VALUE_MAX", posix::unistd::_SC_SEM_VALUE_MAX),
        ("_SC_TIMER_MAX", posix::unistd::_SC_TIMER_MAX),
        ("_SC_BC_BASE_MAX", posix::unistd::_SC_BC_BASE_MAX),
        ("_SC_BC_DIM_MAX", posix::unistd::_SC_BC_DIM_MAX),
        ("_SC_BC_SCALE_MAX", posix::unistd::_SC_BC_SCALE_MAX),
        ("_SC_BC_STRING_MAX", posix::unistd::_SC_BC_STRING_MAX),
        ("_SC_COLL_WEIGHTS_MAX", posix::unistd::_SC_COLL_WEIGHTS_MAX),
        ("_SC_EXPR_NEST_MAX", posix::unistd::_SC_EXPR_NEST_MAX),
        ("_SC_LINE_MAX", posix::unistd::_SC_LINE_MAX),
        ("_SC_RE_DUP_MAX", posix::unistd::_SC_RE_DUP_MAX),
        ("_SC_2_VERSION", posix::unistd::_SC_2_VERSION),
        ("_SC_2_C_BIND", posix::unistd::_SC_2_C_BIND),
        ("_SC_IOV_MAX", posix::unistd::_SC_IOV_MAX),
        ("_SC_THREADS", posix::unistd::_SC_THREADS),
        ("_SC_GETGR_R_SIZE_MAX", posix::unistd::_SC_GETGR_R_SIZE_MAX),
        ("_SC_GETPW_R_SIZE_MAX", posix::unistd::_SC_GETPW_R_SIZE_MAX),
        ("_SC_LOGIN_NAME_MAX", posix::unistd::_SC_LOGIN_NAME_MAX),
        ("_SC_TTY_NAME_MAX", posix::unistd::_SC_TTY_NAME_MAX),
        (
            "_SC_THREAD_DESTRUCTOR_ITERATIONS",
            posix::unistd::_SC_THREAD_DESTRUCTOR_ITERATIONS,
        ),
        ("_SC_THREAD_KEYS_MAX", posix::unistd::_SC_THREAD_KEYS_MAX),
        ("_SC_THREAD_STACK_MIN", posix::unistd::_SC_THREAD_STACK_MIN),
        (
            "_SC_THREAD_THREADS_MAX",
            posix::unistd::_SC_THREAD_THREADS_MAX,
        ),
        ("_SC_NPROCESSORS_CONF", posix::unistd::_SC_NPROCESSORS_CONF),
        ("_SC_NPROCESSORS_ONLN", posix::unistd::_SC_NPROCESSORS_ONLN),
        ("_SC_PHYS_PAGES", posix::unistd::_SC_PHYS_PAGES),
        ("_SC_AVPHYS_PAGES", posix::unistd::_SC_AVPHYS_PAGES),
        ("_SC_SYMLOOP_MAX", posix::unistd::_SC_SYMLOOP_MAX),
        ("_SC_HOST_NAME_MAX", posix::unistd::_SC_HOST_NAME_MAX),
        ("_PC_LINK_MAX", posix::unistd::_PC_LINK_MAX),
        ("_PC_MAX_CANON", posix::unistd::_PC_MAX_CANON),
        ("_PC_MAX_INPUT", posix::unistd::_PC_MAX_INPUT),
        ("_PC_NAME_MAX", posix::unistd::_PC_NAME_MAX),
        ("_PC_PATH_MAX", posix::unistd::_PC_PATH_MAX),
        ("_PC_PIPE_BUF", posix::unistd::_PC_PIPE_BUF),
        ("_PC_CHOWN_RESTRICTED", posix::unistd::_PC_CHOWN_RESTRICTED),
        ("_PC_NO_TRUNC", posix::unistd::_PC_NO_TRUNC),
        ("_PC_VDISABLE", posix::unistd::_PC_VDISABLE),
        ("_PC_FILESIZEBITS", posix::unistd::_PC_FILESIZEBITS),
        ("_PC_SYMLINK_MAX", posix::unistd::_PC_SYMLINK_MAX),
        ("_CS_PATH", posix::unistd::_CS_PATH),
    ];
    // glibc spells one of these by its older name in getconf's table:
    // `<bits/confname.h>` defines `_SC_IOV_MAX` as `_SC_UIO_MAXIOV`.
    let spelt = |name: &'static str| match name {
        "_SC_IOV_MAX" => "_SC_UIO_MAXIOV",
        other => other,
    };
    let mut checked = 0;
    for &(name, value) in ours {
        let rows: Vec<&Var> = vars::VARS
            .iter()
            .filter(|v| v.constant == spelt(name))
            .collect();
        assert!(!rows.is_empty(), "{name} is in no row of getconf's table");
        for row in rows {
            let got = match row.call {
                Call::Sysconf(c) | Call::Confstr(c) | Call::Pathconf(c) => c,
            };
            assert_eq!(got, value, "{} ({name})", row.name);
            checked += 1;
        }
    }
    assert!(checked >= ours.len());
}

#[test]
fn a_sysconf_name_prints_the_librarys_value() {
    let mut lib = Fake::default();
    lib.sysconf.insert(sc("PAGESIZE"), 16384);
    assert_eq!(
        getconf(&["PAGESIZE"], &lib),
        (0, "16384\n".into(), String::new())
    );
    // `_POSIX_` may be left off a name that has it.
    lib.sysconf.insert(sc("_POSIX_ARG_MAX"), 262_144);
    assert_eq!(
        getconf(&["POSIX_ARG_MAX"], &lib).0,
        2,
        "only the _POSIX_ prefix is optional"
    );
    assert_eq!(getconf(&["_POSIX_ARG_MAX"], &lib).1, "262144\n");
}

#[test]
fn a_name_the_library_cannot_answer_is_undefined() {
    let lib = Fake::default();
    assert_eq!(
        getconf(&["PAGESIZE"], &lib),
        (0, "undefined\n".into(), String::new())
    );
}

#[test]
fn the_all_ones_limits_print_unsigned_not_undefined() {
    let mut lib = Fake::default();
    lib.sysconf.insert(sc("ULONG_MAX"), -1);
    assert_eq!(getconf(&["ULONG_MAX"], &lib).1, "18446744073709551615\n");
    assert_eq!(getconf(&["UINT_MAX"], &lib).1, "18446744073709551615\n");
}

#[test]
fn a_path_variable_needs_its_path_and_reports_the_failure_to_read_it() {
    let mut lib = Fake::default();
    let Call::Pathconf(name_max) = constant("NAME_MAX") else {
        panic!("NAME_MAX is a pathconf name");
    };
    lib.pathconf.insert(name_max, 255);
    assert_eq!(
        getconf(&["NAME_MAX", "/"], &lib),
        (0, "255\n".into(), String::new())
    );
    // No path: usage, exit 2.
    let (status, out, err) = getconf(&["NAME_MAX"], &lib);
    assert_eq!((status, out.as_str()), (2, ""));
    assert!(err.starts_with("Usage: getconf [-v specification] variable_name [pathname]\n"));
    // A limit the file system does not have.
    assert_eq!(getconf(&["LINK_MAX", "/"], &lib).1, "undefined\n");
    // A path that cannot be examined: `error (3, errno, "pathconf: %s", path)`.
    lib.path_errno = Some((b"/nope".to_vec(), 2));
    assert_eq!(
        getconf(&["NAME_MAX", "/nope"], &lib),
        (
            3,
            String::new(),
            "getconf: pathconf: /nope: No such file or directory\n".into()
        )
    );
}

#[test]
fn a_confstr_name_prints_its_string() {
    let mut lib = Fake::default();
    lib.confstr.insert(0, b"/bin:/usr/bin");
    assert_eq!(
        getconf(&["PATH"], &lib),
        (0, "/bin:/usr/bin\n".into(), String::new())
    );
    // One the library has no value for prints an empty line, as upstream's
    // `printf ("%.*s\n", 0, ...)` does.
    assert_eq!(
        getconf(&["GNU_LIBC_VERSION"], &lib),
        (0, "\n".into(), String::new())
    );
}

#[test]
fn too_many_or_too_few_operands_is_usage() {
    let lib = Fake::default();
    for args in [&[][..], &["PAGESIZE", "/", "x"][..], &["PAGESIZE", "/"][..]] {
        let (status, out, err) = getconf(args, &lib);
        assert_eq!((status, out.as_str()), (2, ""), "{args:?}");
        assert_eq!(
            err,
            "Usage: getconf [-v specification] variable_name [pathname]\n       getconf -a [pathname]\n"
        );
    }
}

#[test]
fn an_unknown_name_is_refused_naming_the_program_as_invoked() {
    let lib = Fake::default();
    assert_eq!(
        getconf_as("/usr/bin/getconf", &["NOPE"], &lib),
        (
            2,
            String::new(),
            "/usr/bin/getconf: Unrecognized variable `NOPE'\n".into()
        )
    );
    // ...while the usage line names only the program.
    let (_, _, err) = getconf_as("/usr/bin/getconf", &[], &lib);
    assert!(err.starts_with("Usage: getconf [-v"));
}

#[test]
fn a_spec_is_accepted_and_ignored() {
    let mut lib = Fake::default();
    lib.sysconf.insert(sc("LONG_BIT"), 64);
    for args in [
        &["-v", "POSIX_V7_LP64_OFF64", "LONG_BIT"][..],
        &["-vPOSIX_V7_LP64_OFF64", "LONG_BIT"][..],
        &["-v", "anything", "LONG_BIT"][..],
    ] {
        assert_eq!(
            getconf(args, &lib),
            (0, "64\n".into(), String::new()),
            "{args:?}"
        );
    }
    // `-v` with nothing after it is usage.
    assert_eq!(getconf(&["-v"], &lib).0, 2);
}

#[test]
fn a_double_dash_ends_the_options() {
    let mut lib = Fake::default();
    lib.sysconf.insert(sc("LONG_BIT"), 64);
    assert_eq!(getconf(&["--", "LONG_BIT"], &lib).1, "64\n");
}

#[test]
fn dash_a_lists_every_name_padded_with_blanks_for_what_is_missing() {
    let mut lib = Fake::default();
    lib.sysconf.insert(sc("PAGESIZE"), 16384);
    let (status, out, err) = getconf(&["-a"], &lib);
    assert_eq!((status, err.as_str()), (0, ""));
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines.len(), vars::VARS.len());
    assert_eq!(
        lines.first().copied(),
        Some("LINK_MAX                           ")
    );
    assert!(lines.contains(&"PAGESIZE                           16384"));
    // A name longer than the column is not cut.
    let long = vars::VARS
        .iter()
        .find(|v| v.name.len() > 35)
        .map(|v| v.name);
    if let Some(long) = long {
        assert!(lines.iter().any(|l| l.starts_with(long)));
    }
    // `-a` takes one optional path, no more.
    assert_eq!(getconf(&["-a", "/", "x"], &lib).0, 2);
}

#[test]
fn version_and_help_are_upstreams_text() {
    let lib = Fake::default();
    let (status, out, _) = getconf(&["--version"], &lib);
    assert_eq!(status, 0);
    assert!(out.starts_with("getconf (GNU libc) 2.39\nCopyright (C) 2024"));
    assert!(out.ends_with("Written by Roland McGrath.\n"));
    let (status, out, _) = getconf(&["--help"], &lib);
    assert_eq!(status, 0);
    assert!(
        out.starts_with("Usage: getconf [-v SPEC] VAR\n  or:  getconf [-v SPEC] PATH_VAR PATH\n")
    );
    // Only as the first argument, as upstream checks only argv[1].
    assert_eq!(getconf(&["LONG_BIT", "--help"], &lib).0, 2);
}
