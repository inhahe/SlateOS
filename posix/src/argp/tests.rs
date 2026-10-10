//! argp against glibc 2.39's answers: the parsers and scenarios of
//! `posix/tools/oracle/argp_harness.py`, built here from the descriptions
//! in `argp_oracle.txt` and run, every line of each compared -- the calls
//! the parsers and filters heard, what went to standard output and to
//! standard error, and how it ended.
//!
//! An exit is a panic carrying its status (`argp::exit_now`), caught here
//! as the harness's parent catches its child's.

use super::*;
use std::boxed::Box;
use std::cell::RefCell;
use std::ffi::CString;
use std::format;
use std::string::{String, ToString};
use std::vec::Vec;

const ORACLE: &str = include_str!("../argp_oracle.txt");

/// What argp's exit is in a test.
pub(crate) struct Exited(pub(crate) i32);

/// The oracle's escapes undone.
fn unesc(s: &str) -> Vec<u8> {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'\\' && i + 1 < b.len() {
            match b[i + 1] {
                b'n' => out.push(b'\n'),
                b't' => out.push(b'\t'),
                b'v' => out.push(0x0B),
                b'\\' => out.push(b'\\'),
                b'x' => {
                    out.push(u8::from_str_radix(&s[i + 2..i + 4], 16).unwrap());
                    i += 4;
                    continue;
                }
                c => panic!("bad escape {c}"),
            }
            i += 2;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    out
}

/// Bytes escaped as the oracle has them (the harness's `esc`).
fn esc(b: &[u8]) -> String {
    let mut s = String::new();
    for &c in b {
        match c {
            b'\\' => s.push_str("\\\\"),
            b'\n' => s.push_str("\\n"),
            b'\t' => s.push_str("\\t"),
            0x0B => s.push_str("\\v"),
            c if c < 0x20 || c >= 0x7f => s.push_str(&format!("\\x{c:02x}")),
            c => s.push(char::from(c)),
        }
    }
    s
}

/// `-` or a C string's bytes, escaped.
fn esc_ptr(p: *const u8) -> String {
    if p.is_null() {
        "-".to_string()
    } else {
        // SAFETY: a C string.
        esc(unsafe { text(p) })
    }
}

/// A key as the oracle writes it.
fn key_name(key: i32) -> String {
    let named = [
        (ARGP_KEY_ARG, "ARG"),
        (ARGP_KEY_ARGS, "ARGS"),
        (ARGP_KEY_END, "END"),
        (ARGP_KEY_NO_ARGS, "NO_ARGS"),
        (ARGP_KEY_INIT, "INIT"),
        (ARGP_KEY_SUCCESS, "SUCCESS"),
        (ARGP_KEY_ERROR, "ERROR"),
        (ARGP_KEY_FINI, "FINI"),
        (ARGP_KEY_HELP_PRE_DOC, "PRE_DOC"),
        (ARGP_KEY_HELP_POST_DOC, "POST_DOC"),
        (ARGP_KEY_HELP_HEADER, "HEADER"),
        (ARGP_KEY_HELP_EXTRA, "EXTRA"),
        (ARGP_KEY_HELP_DUP_ARGS_NOTE, "DUP_ARGS_NOTE"),
        (ARGP_KEY_HELP_ARGS_DOC, "ARGS_DOC"),
    ];
    if let Some((_, n)) = named.iter().find(|(k, _)| *k == key) {
        return (*n).to_string();
    }
    if key > 0x20 && key < 0x7f {
        format!("'{}'", char::from(u8::try_from(key).unwrap()))
    } else {
        key.to_string()
    }
}

/// A key from the oracle's text.
fn parse_key(s: &str) -> i32 {
    let named = [
        ("ARG", ARGP_KEY_ARG),
        ("ARGS", ARGP_KEY_ARGS),
        ("END", ARGP_KEY_END),
        ("NO_ARGS", ARGP_KEY_NO_ARGS),
        ("INIT", ARGP_KEY_INIT),
        ("SUCCESS", ARGP_KEY_SUCCESS),
        ("ERROR", ARGP_KEY_ERROR),
        ("FINI", ARGP_KEY_FINI),
        ("PRE_DOC", ARGP_KEY_HELP_PRE_DOC),
        ("POST_DOC", ARGP_KEY_HELP_POST_DOC),
        ("HEADER", ARGP_KEY_HELP_HEADER),
        ("EXTRA", ARGP_KEY_HELP_EXTRA),
        ("DUP_ARGS_NOTE", ARGP_KEY_HELP_DUP_ARGS_NOTE),
        ("ARGS_DOC", ARGP_KEY_HELP_ARGS_DOC),
    ];
    if let Some((_, k)) = named.iter().find(|(n, _)| *n == s) {
        return *k;
    }
    if s.len() == 3 && s.starts_with('\'') {
        return i32::from(s.as_bytes()[1]);
    }
    s.parse().unwrap()
}

/// `None` for `-`, else the text, as a C string.
fn opt_cstr(s: &str) -> Option<CString> {
    (s != "-").then(|| CString::new(unesc(s)).unwrap())
}

fn ptr_of(c: Option<&CString>) -> *const u8 {
    c.map_or(core::ptr::null(), |c| c.as_ptr().cast())
}

/// An option's description: name, key, arg, flags, doc, group.
type OptSpec = (
    Option<CString>,
    i32,
    Option<CString>,
    i32,
    Option<CString>,
    i32,
);

/// One argp's description.
struct Spec {
    id: String,
    args_doc: Option<CString>,
    doc: Option<CString>,
    /// name, key, arg, flags, doc, group.
    options: Vec<OptSpec>,
    /// child id, flags, header, group.
    children: Vec<(String, i32, Option<CString>, i32)>,
    react: Vec<(i32, String)>,
    filter: Option<Vec<(i32, String)>>,
}

/// A scenario: its name, argp, flags, how, argv, environment, globals, and
/// glibc's lines after them.
struct Scenario {
    name: String,
    argp: String,
    flags: u32,
    how: String,
    argv: Vec<Vec<u8>>,
    env: Vec<(String, Option<Vec<u8>>)>,
    globs: Vec<(String, Vec<u8>)>,
    want: Vec<String>,
}

fn parse_flags(s: &str) -> u32 {
    if s == "0" {
        return 0;
    }
    s.split(',')
        .map(|f| match f {
            "ARGP_PARSE_ARGV0" => ARGP_PARSE_ARGV0,
            "ARGP_NO_ERRS" => ARGP_NO_ERRS,
            "ARGP_NO_ARGS" => ARGP_NO_ARGS,
            "ARGP_IN_ORDER" => ARGP_IN_ORDER,
            "ARGP_NO_HELP" => ARGP_NO_HELP,
            "ARGP_NO_EXIT" => ARGP_NO_EXIT,
            "ARGP_LONG_ONLY" => ARGP_LONG_ONLY,
            "ARGP_SILENT" => ARGP_NO_EXIT | ARGP_NO_ERRS | ARGP_NO_HELP,
            f => panic!("flag {f}"),
        })
        .fold(0, |a, b| a | b)
}

fn option_flags(s: &str) -> i32 {
    if s == "0" {
        return 0;
    }
    s.split(',')
        .map(|f| match f {
            "ALIAS" => OPTION_ALIAS,
            "HIDDEN" => OPTION_HIDDEN,
            "DOC" => OPTION_DOC,
            "NO_USAGE" => OPTION_NO_USAGE,
            "ARG_OPTIONAL" => OPTION_ARG_OPTIONAL,
            f => panic!("option flag {f}"),
        })
        .fold(0, |a, b| a | b)
}

/// The oracle read: the argps' descriptions and the scenarios.
fn read_oracle() -> (Vec<Spec>, Vec<Scenario>) {
    let mut specs: Vec<Spec> = Vec::new();
    let mut scens: Vec<Scenario> = Vec::new();
    let find = |specs: &mut Vec<Spec>, id: &str| specs.iter().position(|s| s.id == id).unwrap();
    for line in ORACLE
        .lines()
        .filter(|l| !l.starts_with('#') && !l.is_empty())
    {
        let f: Vec<&str> = line.split('\t').collect();
        match f[0] {
            "A" => specs.push(Spec {
                id: f[1].to_string(),
                args_doc: opt_cstr(f[2]),
                doc: opt_cstr(f[3]),
                options: Vec::new(),
                children: Vec::new(),
                react: Vec::new(),
                filter: None,
            }),
            "o" => {
                let i = find(&mut specs, f[1]);
                let key = if f[3] == "0" { 0 } else { parse_key(f[3]) };
                specs[i].options.push((
                    opt_cstr(f[2]),
                    key,
                    opt_cstr(f[4]),
                    option_flags(f[5]),
                    opt_cstr(f[6]),
                    f[7].parse().unwrap(),
                ));
            }
            "c" => {
                let i = find(&mut specs, f[1]);
                specs[i].children.push((
                    f[2].to_string(),
                    f[3].parse().unwrap(),
                    opt_cstr(f[4]),
                    f[5].parse().unwrap(),
                ));
            }
            "r" => {
                let i = find(&mut specs, f[1]);
                specs[i]
                    .react
                    .push((parse_key(f[2]), String::from_utf8(unesc(f[3])).unwrap()));
            }
            "f" => {
                let i = find(&mut specs, f[1]);
                let list = specs[i].filter.get_or_insert_with(Vec::new);
                if f[2] != "-" {
                    list.push((parse_key(f[2]), String::from_utf8(unesc(f[3])).unwrap()));
                }
            }
            "S" => scens.push(Scenario {
                name: f[1].to_string(),
                argp: f[2].to_string(),
                flags: parse_flags(f[3]),
                how: f[4].to_string(),
                argv: Vec::new(),
                env: Vec::new(),
                globs: Vec::new(),
                want: Vec::new(),
            }),
            "a" => {
                let s = scens.last_mut().unwrap();
                s.argv = f[1..].iter().map(|a| unesc(a)).collect();
            }
            "e" => {
                let s = scens.last_mut().unwrap();
                s.env
                    .push((f[1].to_string(), (f[2] != "-").then(|| unesc(f[2]))));
            }
            "g" => {
                let s = scens.last_mut().unwrap();
                s.globs.push((f[1].to_string(), unesc(f[2])));
            }
            _ => scens.last_mut().unwrap().want.push(line.to_string()),
        }
    }
    (specs, scens)
}

/// The tree in memory: every argp's tables, at addresses that do not move
/// while it is parsed.
struct Built {
    argps: Vec<Argp>,
    _options: Vec<Vec<ArgpOption>>,
    _children: Vec<Vec<ArgpChild>>,
}

fn build(specs: &[Spec]) -> Built {
    assert!(
        specs.len() <= PARSERS.len(),
        "more argps than parser functions"
    );
    let mut argps: Vec<Argp> = specs
        .iter()
        .enumerate()
        .map(|(i, s)| Argp {
            options: core::ptr::null(),
            parser: Some(PARSERS[i]),
            args_doc: ptr_of(s.args_doc.as_ref()),
            doc: ptr_of(s.doc.as_ref()),
            children: core::ptr::null(),
            help_filter: s.filter.as_ref().map(|_| FILTERS[i]),
            argp_domain: core::ptr::null(),
        })
        .collect();
    let mut options = Vec::new();
    let mut children = Vec::new();
    for (i, s) in specs.iter().enumerate() {
        let mut table: Vec<ArgpOption> = s
            .options
            .iter()
            .map(|(n, k, a, fl, d, g)| ArgpOption {
                name: ptr_of(n.as_ref()),
                key: *k,
                arg: ptr_of(a.as_ref()),
                flags: *fl,
                doc: ptr_of(d.as_ref()),
                group: *g,
            })
            .collect();
        table.push(ArgpOption {
            name: core::ptr::null(),
            key: 0,
            arg: core::ptr::null(),
            flags: 0,
            doc: core::ptr::null(),
            group: 0,
        });
        if !s.options.is_empty() {
            argps[i].options = table.as_ptr();
        }
        options.push(table);
        if !s.children.is_empty() {
            let mut kids: Vec<ArgpChild> = s
                .children
                .iter()
                .map(|(cid, cfl, hdr, grp)| {
                    let ci = specs.iter().position(|x| &x.id == cid).unwrap();
                    ArgpChild {
                        argp: &raw const argps[ci],
                        flags: *cfl,
                        header: ptr_of(hdr.as_ref()),
                        group: *grp,
                    }
                })
                .collect();
            kids.push(ArgpChild {
                argp: core::ptr::null(),
                flags: 0,
                header: core::ptr::null(),
                group: 0,
            });
            argps[i].children = kids.as_ptr();
            children.push(kids);
        }
    }
    Built {
        argps,
        _options: options,
        _children: children,
    }
}

/// What the parsers and filters of the running scenario are: their ids,
/// reactions and children's counts, and the log of their calls.
struct Run {
    ids: Vec<String>,
    react: Vec<Vec<(i32, String)>>,
    filter: Vec<Option<Vec<(i32, String)>>>,
    nchildren: Vec<usize>,
    log: Vec<String>,
    /// The children's inputs handed out, kept alive.
    tags: Vec<CString>,
    /// The filters' strings and the messages, kept alive.
    keep: Vec<CString>,
}

std::thread_local! {
    static RUN: RefCell<Option<Run>> = const { RefCell::new(None) };
}

fn with_run<R>(f: impl FnOnce(&mut Run) -> R) -> R {
    RUN.with(|r| f(r.borrow_mut().as_mut().expect("no scenario running")))
}

/// A C string the run keeps alive.
fn kept(s: &[u8]) -> *const u8 {
    with_run(|r| {
        r.keep.push(CString::new(s).unwrap());
        r.keep.last().unwrap().as_ptr().cast()
    })
}

/// The harness's parser: log the call, do what the table says.
fn run_parser(i: usize, key: i32, arg: *mut u8, st: *mut ArgpState) -> i32 {
    // SAFETY: the parse's state.
    let s = unsafe { &mut *st };
    let (id, action, nkids) = with_run(|r| {
        (
            r.ids[i].clone(),
            r.react[i]
                .iter()
                .find(|(k, _)| *k == key)
                .map(|(_, w)| w.clone()),
            r.nchildren[i],
        )
    });
    let flags = if s.flags == 0 {
        "0".to_string()
    } else {
        format!("{:#x}", s.flags)
    };
    let input = if s.input.is_null() {
        "(null)".to_string()
    } else {
        esc_ptr(s.input.cast())
    };
    with_run(|r| {
        r.log.push(format!(
            "C {id}\t{}\t{}\tnext={} num={} argc={} in={input} flags={flags} quoted={}",
            key_name(key),
            esc_ptr(arg),
            s.next,
            s.arg_num,
            s.argc,
            s.quoted
        ));
    });
    if key == ARGP_KEY_INIT && nkids > 0 {
        let parent = if s.input.is_null() {
            "(null)".to_string()
        } else {
            esc_ptr(s.input.cast())
        };
        for c in 0..nkids {
            let tag = with_run(|r| {
                r.tags.push(CString::new(format!("{parent}.{c}")).unwrap());
                r.tags.last().unwrap().as_ptr()
            });
            // SAFETY: the parent's children's inputs, one each.
            unsafe { s.child_inputs.add(c).write(tag.cast_mut().cast()) };
        }
    }
    let Some(w) = action else {
        return ARGP_ERR_UNKNOWN;
    };
    let num_arg = |p: &str| -> u32 { w[p.len()..].trim().parse().unwrap() };
    match w.as_str() {
        "unknown" => ARGP_ERR_UNKNOWN,
        "ok" => 0,
        "einval" => crate::errno::EINVAL,
        "usage" => {
            // SAFETY: the parse's state.
            unsafe { usage(st) };
            0
        }
        "rest" | "args" => {
            let mut line = format!("C {id}\t{w}");
            for k in s.next..s.argc {
                // SAFETY: the parse's argv, within its count.
                line.push_str(&format!(
                    "\t{}",
                    esc_ptr(unsafe { s.argv.add(usize::try_from(k).unwrap()).read() })
                ));
            }
            with_run(|r| r.log.push(line));
            s.next = s.argc;
            0
        }
        w if w.starts_with("error ") => {
            let msg = kept(&w.as_bytes()[6..]);
            crate::printf::tests::with_valist(&[msg as u64], &[], |ap| {
                // SAFETY: the state, a format and its argument.
                unsafe { verror(st, c"%s".as_ptr().cast(), ap) }
            });
            0
        }
        w if w.starts_with("failure ") => {
            let mut it = w[8..].splitn(3, ' ');
            let status: i32 = it.next().unwrap().parse().unwrap();
            let errnum: i32 = it.next().unwrap().parse().unwrap();
            let msg = kept(it.next().unwrap().as_bytes());
            crate::printf::tests::with_valist(&[msg as u64], &[], |ap| {
                // SAFETY: as above.
                unsafe { vfailure(st, status, errnum, c"%s".as_ptr().cast(), ap) }
            });
            0
        }
        w if w.starts_with("help ") => {
            let f = u32::from_str_radix(w[5..].trim_start_matches("0x"), 16).unwrap();
            // SAFETY: the parse's state and its stream.
            unsafe { state_help(st, s.out_stream, f) };
            0
        }
        w if w.starts_with("argmax ") => {
            if s.arg_num >= num_arg("argmax ") {
                // SAFETY: as above.
                unsafe { usage(st) };
            }
            0
        }
        w if w.starts_with("argmin ") => {
            if s.arg_num < num_arg("argmin ") {
                // SAFETY: as above.
                unsafe { usage(st) };
            }
            0
        }
        w if w.starts_with("take ") => {
            if s.arg_num < num_arg("take ") {
                0
            } else {
                ARGP_ERR_UNKNOWN
            }
        }
        w => panic!("bad action {w}"),
    }
}

/// The harness's help filter.
fn run_filter(i: usize, key: i32, txt: *const u8, input: *mut c_void) -> *mut u8 {
    let (id, action) = with_run(|r| {
        (
            r.ids[i].clone(),
            r.filter[i]
                .as_ref()
                .and_then(|f| f.iter().find(|(k, _)| *k == key).map(|(_, w)| w.clone())),
        )
    });
    let inp = if input.is_null() {
        "(null)".to_string()
    } else {
        esc_ptr(input.cast())
    };
    with_run(|r| {
        r.log.push(format!(
            "F {id}\t{}\t{}\tin={inp}",
            key_name(key),
            esc_ptr(txt)
        ))
    });
    let malloced = |b: &[u8]| -> *mut u8 {
        let p = crate::malloc::malloc(b.len() + 1);
        // SAFETY: a fresh block of len + 1.
        unsafe {
            core::ptr::copy_nonoverlapping(b.as_ptr(), p, b.len());
            p.add(b.len()).write(0);
        }
        p
    };
    match action.as_deref() {
        None | Some("keep") => txt.cast_mut(),
        Some("drop") => core::ptr::null_mut(),
        Some(w) if w.starts_with("replace ") => malloced(&w.as_bytes()[8..]),
        Some(w) if w.starts_with("append ") => {
            // SAFETY: NULL or a C string.
            let mut b = if txt.is_null() {
                Vec::new()
            } else {
                unsafe { text(txt) }.to_vec()
            };
            b.extend_from_slice(&w.as_bytes()[7..]);
            malloced(&b)
        }
        Some(w) => panic!("bad filter action {w}"),
    }
}

macro_rules! pool {
    ($($i:literal),*) => {
        const PARSERS: [ArgpParserFn; 40] = [$({
            unsafe extern "C-unwind" fn p(key: i32, arg: *mut u8, st: *mut ArgpState) -> i32 {
                run_parser($i, key, arg, st)
            }
            p
        }),*];
        const FILTERS: [ArgpHelpFilterFn; 40] = [$({
            unsafe extern "C-unwind" fn f(key: i32, t: *const u8, input: *mut c_void) -> *mut u8 {
                run_filter($i, key, t, input)
            }
            f
        }),*];
    };
}
pool!(
    0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25,
    26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39
);

/// The harness's version hook.
unsafe extern "C-unwind" fn hook(stream: *mut u8, st: *mut ArgpState) {
    let msg: &[u8] = if st.is_null() {
        b"version from the hook, without one\n"
    } else {
        b"version from the hook, with a state\n"
    };
    put(stream, msg);
}

/// Serialises the scenarios: argp's globals, the environment and the
/// program's names are the process's.
static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Exits printed by nothing: the panic hook says nothing of an `Exited`.
fn quiet_exits() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let prev = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            if info.payload().downcast_ref::<Exited>().is_none() {
                prev(info);
            }
        }));
    });
}

fn set_env(name: &str, v: Option<&[u8]>) {
    let n = CString::new(name).unwrap();
    match v {
        Some(v) => {
            let v = CString::new(v).unwrap();
            // SAFETY: C strings.
            assert_eq!(
                unsafe { crate::environ::setenv(n.as_ptr().cast(), v.as_ptr().cast(), 1) },
                0
            );
        }
        None => {
            // SAFETY: a C string.
            unsafe { crate::environ::unsetenv(n.as_ptr().cast()) };
        }
    }
}

/// One scenario run: its lines, as the oracle's after its S, a, e and g.
fn run_scenario(sc: &Scenario, specs: &[Spec], built: &Built) -> Vec<String> {
    let ai = specs.iter().position(|s| s.id == sc.argp).unwrap();
    RUN.with(|r| {
        *r.borrow_mut() = Some(Run {
            ids: specs.iter().map(|s| s.id.clone()).collect(),
            react: specs.iter().map(|s| s.react.clone()).collect(),
            filter: specs.iter().map(|s| s.filter.clone()).collect(),
            nchildren: specs.iter().map(|s| s.children.len()).collect(),
            log: Vec::new(),
            tags: Vec::new(),
            keep: Vec::new(),
        });
    });
    for v in ["ARGP_HELP_FMT", "COLUMNS", "POSIXLY_CORRECT"] {
        set_env(v, None);
    }
    for (k, v) in &sc.env {
        set_env(k, v.as_deref());
    }
    let version = sc
        .globs
        .iter()
        .find(|(k, _)| k == "version")
        .map(|(_, v)| CString::new(v.clone()).unwrap());
    let bug = sc
        .globs
        .iter()
        .find(|(k, _)| k == "bug")
        .map(|(_, v)| CString::new(v.clone()).unwrap());
    let err_exit: i32 = sc
        .globs
        .iter()
        .find(|(k, _)| k == "err_exit")
        .map_or(64, |(_, v)| {
            std::str::from_utf8(v).unwrap().parse().unwrap()
        });
    let has_hook = sc.globs.iter().any(|(k, _)| k == "hook");
    let full = c"/usr/bin/prog";
    let short = c"prog";
    // SAFETY: plain writes of the C variables (the program's names are this
    // thread's own), the scenarios taking turns.
    let saved = unsafe {
        let saved = (
            crate::crt::progname_full_slot().read(),
            crate::crt::progname_slot().read(),
        );
        crate::crt::progname_full_slot().write(full.as_ptr().cast());
        crate::crt::progname_slot().write(short.as_ptr().cast());
        (&raw mut argp_program_version).write(ptr_of(version.as_ref()));
        (&raw mut argp_program_version_hook).write(if has_hook { Some(hook) } else { None });
        (&raw mut argp_program_bug_address).write(ptr_of(bug.as_ref()));
        (&raw mut argp_err_exit_status).write(err_exit);
        saved
    };
    let mut argv_store: Vec<CString> = sc
        .argv
        .iter()
        .map(|a| CString::new(a.clone()).unwrap())
        .collect();
    let mut argv: Vec<*mut u8> = argv_store
        .iter_mut()
        .map(|c| c.as_ptr().cast_mut().cast())
        .collect();
    argv.push(core::ptr::null_mut());
    let argc = i32::try_from(sc.argv.len()).unwrap();
    let root = c"root";
    let mut ended = String::new();
    let (out, err) = crate::stdio::capture_std_streams(|| {
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            if let Some(rest) = sc.how.strip_prefix("help ") {
                let mut it = rest.split(' ');
                let f =
                    u32::from_str_radix(it.next().unwrap().trim_start_matches("0x"), 16).unwrap();
                let n = it.next().unwrap();
                let name = (n != "-").then(|| CString::new(n).unwrap());
                // SAFETY: the tree, the stream, the name.
                unsafe {
                    help::help(
                        &raw const built.argps[ai],
                        core::ptr::null(),
                        crate::stdio::stdout_stream(),
                        f,
                        ptr_of(name.as_ref()).cast_mut(),
                    );
                }
                "X\thelp".to_string()
            } else {
                let mut idx = -12345;
                let av = if argc == 0 {
                    core::ptr::null_mut()
                } else {
                    argv.as_mut_ptr()
                };
                // SAFETY: the tree and the argument vector.
                let r = unsafe {
                    parse::parse(
                        &raw const built.argps[ai],
                        argc,
                        av,
                        sc.flags,
                        &mut idx,
                        root.as_ptr().cast_mut().cast(),
                    )
                };
                format!("X\treturn {r} {idx}")
            }
        }));
        ended = match r {
            Ok(s) => s,
            Err(e) => match e.downcast::<Exited>() {
                Ok(x) => format!("X\texit {}", x.0),
                Err(e) => std::panic::resume_unwind(e),
            },
        };
    });
    // SAFETY: as above.
    unsafe {
        crate::crt::progname_full_slot().write(saved.0);
        crate::crt::progname_slot().write(saved.1);
        (&raw mut argp_program_version).write(core::ptr::null());
        (&raw mut argp_program_version_hook).write(None);
        (&raw mut argp_program_bug_address).write(core::ptr::null());
        (&raw mut argp_err_exit_status).write(64);
    }
    for (k, _) in &sc.env {
        set_env(k, None);
    }
    let mut lines = with_run(|r| core::mem::take(&mut r.log));
    lines.push(format!("O\t{}", esc(&out)));
    lines.push(format!("E\t{}", esc(&err)));
    lines.push(ended);
    RUN.with(|r| *r.borrow_mut() = None);
    drop(argv_store);
    lines
}

/// The words of a message, its layout aside.
fn words(line: &str) -> Vec<String> {
    let t = unesc(line.split_once('\t').unwrap().1);
    String::from_utf8(t)
        .unwrap()
        .split_whitespace()
        .map(ToString::to_string)
        .collect()
}

/// Where this library answers otherwise, each for its reason
/// (design-decisions §1163).
const DEVIATIONS: &[(&str, &str)] = &[
    (
        "basic-no-argv",
        "argc 0 and a NULL argv: glibc's reads argv[0] and faults",
    ),
    (
        "help-fmt rmargin=abc",
        "a right margin of 0: glibc's help loops for ever",
    ),
    ("usage-fmt rmargin=abc", "as above, for the usage"),
    (
        "help-fmt rmargin=20",
        "the text's column past the right margin: glibc's help loops for ever",
    ),
    (
        "usage-docs",
        "an OPTION_DOC entry is no option: not in the usage line, where glibc lists it",
    ),
    (
        "usage-fmt usage-indent=40,rmargin=50",
        "a usage item that fills its line exactly: glibc's scan reads past it into its stale buffer",
    ),
];

#[test]
fn every_scenario_is_glibcs() {
    let _g = SERIAL
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    quiet_exits();
    let (specs, scens) = read_oracle();
    assert!(scens.len() > 170, "{}", scens.len());
    let built = build(&specs);
    let by_name = |n: &str| scens.iter().find(|s| s.name == n).unwrap();
    let mut wrong = Vec::new();
    for sc in &scens {
        let got = run_scenario(sc, &specs, &built);
        let deviation = DEVIATIONS.iter().find(|(n, _)| *n == sc.name);
        let want = &sc.want;
        let ok = match deviation.map(|(n, _)| *n) {
            None => &got == want,
            Some("basic-no-argv") => {
                // No arguments, so the parser's END finds too few and calls
                // argp_usage, as it would for an empty argv.
                got == [
                    "C basic\tINIT\t-\tnext=0 num=0 argc=0 in=root flags=0 quoted=0",
                    "C basic\tNO_ARGS\t-\tnext=0 num=0 argc=0 in=root flags=0 quoted=0",
                    "C basic\tEND\t-\tnext=0 num=0 argc=0 in=root flags=0 quoted=0",
                    "O\t",
                    "E\tUsage: prog [OPTION...] ARG1 ARG2\\nTry `prog --help' or `prog --usage' for more information.\\n",
                    "X\texit 64",
                ]
                .map(String::from)
            }
            Some("usage-docs") => {
                // glibc's own answer with the entry marked OPTION_NO_USAGE,
                // its parser's name aside.
                let nu = by_name("usage-docs-nu");
                let fix = |l: &String| l.replace("C docs_nu\t", "C docs\t");
                got == nu.want.iter().map(fix).collect::<Vec<_>>()
            }
            Some(_) => {
                // It ends, says what glibc's said, and says it all: the
                // words of the help at the default margin.
                let plain = by_name(if sc.name.starts_with("help") {
                    "help"
                } else {
                    "usage"
                });
                let o = |v: &[String]| {
                    v.iter()
                        .find(|l| l.starts_with("O\t"))
                        .map(|l| words(l))
                        .unwrap_or_default()
                };
                let e = |v: &[String]| v.iter().find(|l| l.starts_with("E\t")).cloned();
                got.last().map(String::as_str) == Some("X\texit 0")
                    && e(&got) == e(want)
                    && o(&got) == o(&plain.want)
            }
        };
        if !ok {
            wrong.push(format!(
                "{}:\n  want {}\n  got  {}",
                sc.name,
                want.join("\n       "),
                got.join("\n       ")
            ));
        }
    }
    for w in wrong.iter().take(6) {
        std::eprintln!("{w}");
    }
    assert!(
        wrong.is_empty(),
        "{} of {} scenarios are not glibc's",
        wrong.len(),
        scens.len()
    );
}

/// `_option_is_short` and `_option_is_end`, as glibc's inline ones.
#[test]
fn option_kinds() {
    let o = |key: i32, flags: i32, name: *const u8, doc: *const u8, group: i32| ArgpOption {
        name,
        key,
        arg: core::ptr::null(),
        flags,
        doc,
        group,
    };
    let n = c"x".as_ptr().cast::<u8>();
    // SAFETY: options.
    unsafe {
        assert_eq!(_option_is_short(&o(i32::from(b'a'), 0, n, n, 0)), 1);
        assert_eq!(
            _option_is_short(&o(i32::from(b'a'), OPTION_DOC, n, n, 0)),
            0
        );
        assert_eq!(_option_is_short(&o(1000, 0, n, n, 0)), 0);
        assert_eq!(_option_is_short(&o(0, 0, n, n, 0)), 0);
        assert_eq!(_option_is_short(&o(0x7f, 0, n, n, 0)), 0);
        assert_eq!(
            _option_is_end(&o(0, 0, core::ptr::null(), core::ptr::null(), 0)),
            1
        );
        assert_eq!(
            _option_is_end(&o(0, 0, core::ptr::null(), core::ptr::null(), 1)),
            0
        );
        assert_eq!(_option_is_end(&o(0, 0, core::ptr::null(), n, 0)), 0);
        assert_eq!(
            _option_is_end(&o(1, 0, core::ptr::null(), core::ptr::null(), 0)),
            0
        );
        assert_eq!(_option_is_end(core::ptr::null()), 1);
    }
}
