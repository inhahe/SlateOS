//! `hostname` against a library whose answers the tests choose.
//!
//! What the real library says on a real host is `scripts/hostname-diff.sh`'s
//! business; these pin upstream's logic over those answers -- which name is
//! shown, how a list is joined, which failure is reported how, and how a name
//! to set is read and checked.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::*;
use std::cell::RefCell;
use std::sync::atomic::{AtomicUsize, Ordering};

/// One interface entry, as the fake library lists it.
#[derive(Clone)]
struct FakeIf {
    flags: u32,
    family: Option<i32>,
    link_local: bool,
    name: Result<Vec<u8>, i32>,
}

impl Interface for FakeIf {
    fn flags(&self) -> u32 {
        self.flags
    }
    fn family(&self) -> Option<i32> {
        self.family
    }
    fn is_ipv6_link_local(&self) -> bool {
        self.link_local
    }
    fn name_info(&self, _flags: i32) -> Result<Vec<u8>, i32> {
        self.name.clone()
    }
}

/// An interface that is up, with an address of `family` named `name`.
fn up(family: i32, name: &[u8]) -> FakeIf {
    FakeIf {
        flags: IFF_UP,
        family: Some(family),
        link_local: false,
        name: Ok(name.to_vec()),
    }
}

struct Fake {
    hostname: Result<Vec<u8>, i32>,
    domainname: Result<Vec<u8>, i32>,
    set_result: Result<(), i32>,
    lookup: Result<Lookup, i32>,
    aliases: Result<Vec<Vec<u8>>, i32>,
    interfaces: Result<Vec<FakeIf>, i32>,
    /// Every set call: which, and the name it was given.
    sets: RefCell<Vec<(&'static str, Vec<u8>)>>,
    /// Every name `lookup` and `aliases` were asked about.
    asked: RefCell<Vec<Vec<u8>>>,
}

impl Default for Fake {
    fn default() -> Self {
        Fake {
            hostname: Ok(b"logo.example.org".to_vec()),
            domainname: Ok(b"(none)".to_vec()),
            set_result: Ok(()),
            lookup: Ok(Lookup {
                canonical: Some(b"logo.example.org".to_vec()),
                numeric: vec![Ok(b"192.0.2.7".to_vec()), Ok(b"2001:db8::7".to_vec())],
            }),
            aliases: Ok(vec![b"logo".to_vec(), b"www".to_vec()]),
            interfaces: Ok(vec![up(AF_INET, b"192.0.2.7")]),
            sets: RefCell::new(Vec::new()),
            asked: RefCell::new(Vec::new()),
        }
    }
}

impl System for Fake {
    fn hostname(&self) -> Result<Vec<u8>, i32> {
        self.hostname.clone()
    }
    fn domainname(&self) -> Result<Vec<u8>, i32> {
        self.domainname.clone()
    }
    fn set_hostname(&self, name: &[u8]) -> Result<(), i32> {
        self.sets.borrow_mut().push(("host", name.to_vec()));
        self.set_result
    }
    fn set_domainname(&self, name: &[u8]) -> Result<(), i32> {
        self.sets.borrow_mut().push(("domain", name.to_vec()));
        self.set_result
    }
    fn lookup(&self, host: &[u8]) -> Result<Lookup, i32> {
        self.asked.borrow_mut().push(host.to_vec());
        self.lookup.clone()
    }
    fn aliases(&self, host: &[u8]) -> Result<Vec<Vec<u8>>, i32> {
        self.asked.borrow_mut().push(host.to_vec());
        self.aliases.clone()
    }
    fn interfaces(
        &self,
        visit: &mut dyn FnMut(&dyn Interface) -> ControlFlow<()>,
    ) -> Result<(), i32> {
        let list = self.interfaces.clone()?;
        for entry in &list {
            if visit(entry).is_break() {
                break;
            }
        }
        Ok(())
    }
    fn gai_strerror(&self, code: i32) -> Vec<u8> {
        format!("gai error {code}").into_bytes()
    }
    fn hstrerror(&self, code: i32) -> Vec<u8> {
        format!("h error {code}").into_bytes()
    }
}

/// What one run printed and exited with.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Ran {
    out: String,
    err: String,
    status: u8,
}

/// Run as `argv0` with `args` against `sys`.
fn run_as(argv0: &str, args: &[&str], sys: &Fake) -> Ran {
    run_os(argv0, args.iter().map(OsString::from).collect(), sys)
}

/// [`run_as`] for arguments that are not all text: a temporary file's path.
fn run_os(argv0: &str, args: Vec<OsString>, sys: &Fake) -> Ran {
    let mut out = Vec::new();
    let mut err = Vec::new();
    let mut io = Io {
        out: &mut out,
        err: &mut err,
        invocation: argv0.as_bytes().to_vec(),
    };
    let Exit(status) = run(&args, sys, &mut io);
    Ran {
        out: String::from_utf8(out).unwrap(),
        err: String::from_utf8(err).unwrap(),
        status,
    }
}

fn hostname(args: &[&str], sys: &Fake) -> Ran {
    run_as("hostname", args, sys)
}

fn ok(out: &str) -> Ran {
    Ran {
        out: out.to_string(),
        err: String::new(),
        status: 0,
    }
}

fn failed(err: &str) -> Ran {
    Ran {
        out: String::new(),
        err: err.to_string(),
        status: 1,
    }
}

fn usage_out(err: &str) -> Ran {
    Ran {
        out: String::from_utf8(USAGE.to_vec()).unwrap(),
        err: err.to_string(),
        status: 255,
    }
}

fn usage_err() -> Ran {
    Ran {
        out: String::new(),
        err: String::from_utf8(USAGE.to_vec()).unwrap(),
        status: 255,
    }
}

// ---------------------------------------------------------------- showing

#[test]
fn the_plain_name_and_the_short_one() {
    let sys = Fake::default();
    assert_eq!(hostname(&[], &sys), ok("logo.example.org\n"));
    assert_eq!(hostname(&["-s"], &sys), ok("logo\n"));
    assert_eq!(hostname(&["--short"], &sys), ok("logo\n"));
    let dotless = Fake {
        hostname: Ok(b"logo".to_vec()),
        ..Fake::default()
    };
    assert_eq!(hostname(&["-s"], &dotless), ok("logo\n"));
}

#[test]
fn the_canonical_name_and_its_domain_come_from_the_lookup() {
    let sys = Fake {
        hostname: Ok(b"logo".to_vec()),
        lookup: Ok(Lookup {
            canonical: Some(b"logo.local.example".to_vec()),
            numeric: vec![],
        }),
        ..Fake::default()
    };
    assert_eq!(hostname(&["-f"], &sys), ok("logo.local.example\n"));
    assert_eq!(hostname(&["--long"], &sys), ok("logo.local.example\n"));
    assert_eq!(hostname(&["-d"], &sys), ok("local.example\n"));
    // The lookup is of the host name, as `gethostname` gave it.
    assert!(sys.asked.borrow().iter().all(|h| h == b"logo"));
}

#[test]
fn a_canonical_name_without_a_dot_has_no_domain_and_no_newline() {
    let sys = Fake {
        lookup: Ok(Lookup {
            canonical: Some(b"logo".to_vec()),
            numeric: vec![],
        }),
        ..Fake::default()
    };
    assert_eq!(hostname(&["-d"], &sys), ok(""));
}

#[test]
fn the_addresses_are_joined_by_spaces() {
    let sys = Fake::default();
    assert_eq!(hostname(&["-i"], &sys), ok("192.0.2.7 2001:db8::7\n"));
}

#[test]
fn an_address_that_cannot_be_printed_ends_the_list_there() {
    let sys = Fake {
        lookup: Ok(Lookup {
            canonical: Some(b"logo".to_vec()),
            numeric: vec![Ok(b"192.0.2.7".to_vec()), Err(-12)],
        }),
        ..Fake::default()
    };
    assert_eq!(
        hostname(&["-i"], &sys),
        Ran {
            out: "192.0.2.7".to_string(),
            err: "hostname: gai error -12\n".to_string(),
            status: 1,
        }
    );
}

#[test]
fn a_failed_lookup_is_the_librarys_words() {
    let sys = Fake {
        lookup: Err(EAI_NONAME),
        ..Fake::default()
    };
    for flag in ["-d", "-f", "-i", "-a"] {
        assert_eq!(
            hostname(&[flag], &sys),
            failed("hostname: gai error -2\n"),
            "{flag}"
        );
    }
}

#[test]
fn the_aliases_are_joined_by_spaces() {
    let sys = Fake::default();
    assert_eq!(hostname(&["-a"], &sys), ok("logo www\n"));
    let none = Fake {
        aliases: Ok(vec![]),
        ..Fake::default()
    };
    assert_eq!(hostname(&["--alias"], &none), ok("\n"));
    let unknown = Fake {
        aliases: Err(1),
        ..Fake::default()
    };
    assert_eq!(hostname(&["-a"], &unknown), failed("hostname: h error 1\n"));
}

#[test]
fn a_host_name_the_library_cannot_give_is_strerror() {
    // 5 is EIO to the C library, and `strerror`'s words for it are POSIX's
    // only where the error table is: the host's maps 5 to a Win32 error. So
    // the shape is held everywhere and the words on Unix.
    let sys = Fake {
        hostname: Err(5),
        ..Fake::default()
    };
    for args in [&[][..], &["-s"], &["-f"], &["-a"]] {
        let ran = hostname(args, &sys);
        assert_eq!((ran.out.as_str(), ran.status), ("", 1), "{args:?}");
        assert!(ran.err.starts_with("hostname: "), "{args:?}: {}", ran.err);
        #[cfg(unix)]
        assert_eq!(ran.err, "hostname: Input/output error\n");
    }
}

#[test]
fn every_interface_address_but_the_ones_upstream_skips() {
    let sys = Fake {
        interfaces: Ok(vec![
            // The loopback.
            FakeIf {
                flags: IFF_UP | IFF_LOOPBACK,
                ..up(AF_INET, b"127.0.0.1")
            },
            up(AF_INET, b"192.0.2.7"),
            // An interface that is down.
            FakeIf {
                flags: 0,
                ..up(AF_INET, b"198.51.100.1")
            },
            // No address configured.
            FakeIf {
                family: None,
                ..up(AF_INET, b"never")
            },
            // A link-layer entry.
            up(17, b"packet"),
            // IPv6 link-local.
            FakeIf {
                link_local: true,
                ..up(AF_INET6, b"fe80::1")
            },
            up(AF_INET6, b"2001:db8::7"),
        ]),
        ..Fake::default()
    };
    assert_eq!(hostname(&["-I"], &sys), ok("192.0.2.7 2001:db8::7 \n"));
    assert_eq!(hostname(&["-A"], &sys), ok("192.0.2.7 2001:db8::7 \n"));
}

#[test]
fn no_interface_address_is_an_empty_line() {
    let sys = Fake {
        interfaces: Ok(vec![]),
        ..Fake::default()
    };
    assert_eq!(hostname(&["-I"], &sys), ok("\n"));
}

#[test]
fn an_address_without_a_name_is_skipped_by_both() {
    let sys = Fake {
        interfaces: Ok(vec![
            FakeIf {
                name: Err(EAI_NONAME),
                ..up(AF_INET, b"x")
            },
            up(AF_INET, b"192.0.2.7"),
        ]),
        ..Fake::default()
    };
    assert_eq!(hostname(&["-I"], &sys), ok("192.0.2.7 \n"));
    assert_eq!(hostname(&["-A"], &sys), ok("192.0.2.7 \n"));
}

#[test]
fn any_other_failure_ends_dash_i_and_is_skipped_by_dash_a() {
    let sys = Fake {
        interfaces: Ok(vec![
            up(AF_INET, b"192.0.2.7"),
            FakeIf {
                name: Err(-3),
                ..up(AF_INET, b"x")
            },
            up(AF_INET, b"192.0.2.8"),
        ]),
        ..Fake::default()
    };
    assert_eq!(
        hostname(&["-I"], &sys),
        Ran {
            out: "192.0.2.7 ".to_string(),
            err: "hostname: gai error -3\n".to_string(),
            status: 1,
        }
    );
    assert_eq!(hostname(&["-A"], &sys), ok("192.0.2.7 192.0.2.8 \n"));
}

#[test]
fn no_interface_list_is_strerror() {
    let sys = Fake {
        interfaces: Err(5),
        ..Fake::default()
    };
    let ran = hostname(&["-I"], &sys);
    assert_eq!((ran.out.as_str(), ran.status), ("", 1));
    #[cfg(unix)]
    assert_eq!(ran.err, "hostname: Input/output error\n");
}

#[test]
fn the_nis_domain_two_ways() {
    let unset = Fake::default();
    // `domainname` prints what the library says.
    assert_eq!(run_as("domainname", &[], &unset), ok("(none)\n"));
    // `-y`, `nisdomainname` and `ypdomainname` call it unset -- on standard
    // output, under the name the program was run by.
    assert_eq!(
        hostname(&["-y"], &unset),
        Ran {
            out: "hostname: Local domain name not set\n".to_string(),
            err: String::new(),
            status: 1,
        }
    );
    assert_eq!(
        run_as("/usr/bin/ypdomainname", &[], &unset).out,
        "ypdomainname: Local domain name not set\n"
    );
    let set = Fake {
        domainname: Ok(b"lab".to_vec()),
        ..Fake::default()
    };
    assert_eq!(run_as("nisdomainname", &[], &set), ok("lab\n"));
    assert_eq!(hostname(&["--nis"], &set), ok("lab\n"));
    assert_eq!(hostname(&["--yp"], &set), ok("lab\n"));
    assert_eq!(run_as("domainname", &[], &set), ok("lab\n"));
}

#[test]
fn dnsdomainname_is_dash_d() {
    let sys = Fake::default();
    assert_eq!(run_as("dnsdomainname", &[], &sys), ok("example.org\n"));
    assert_eq!(run_as("/bin/dnsdomainname", &[], &sys), ok("example.org\n"));
    // An option still wins over the name it was run by.
    assert_eq!(run_as("dnsdomainname", &["-s"], &sys), ok("logo\n"));
}

#[test]
fn the_last_display_option_wins() {
    let sys = Fake::default();
    assert_eq!(hostname(&["-s", "-f"], &sys), ok("logo.example.org\n"));
    assert_eq!(hostname(&["-fs"], &sys), ok("logo\n"));
    assert_eq!(hostname(&["-d", "-s"], &sys), ok("logo\n"));
}

#[test]
fn boot_alone_shows_the_name() {
    assert_eq!(
        hostname(&["-b"], &Fake::default()),
        ok("logo.example.org\n")
    );
}

// ---------------------------------------------------------------- setting

#[test]
fn a_name_is_set_through_the_library() {
    let sys = Fake::default();
    assert_eq!(hostname(&["newname"], &sys), ok(""));
    assert_eq!(*sys.sets.borrow(), vec![("host", b"newname".to_vec())]);
}

#[test]
fn the_librarys_refusals_in_upstreams_words() {
    let denied = Fake {
        set_result: Err(libcall::EPERM),
        ..Fake::default()
    };
    assert_eq!(
        hostname(&["newname"], &denied),
        failed("hostname: you must be root to change the host name\n")
    );
    assert_eq!(
        run_as("domainname", &["lab"], &denied),
        failed("domainname: you must be root to change the domain name\n")
    );
    let long = Fake {
        set_result: Err(libcall::EINVAL),
        ..Fake::default()
    };
    assert_eq!(
        hostname(&["newname"], &long),
        failed("hostname: name too long\n")
    );
    assert_eq!(
        run_as("domainname", &["lab"], &long),
        failed("domainname: name too long\n")
    );
    // Any other failure is not reported: upstream tests for those two only.
    let other = Fake {
        set_result: Err(libcall::ENOSYS),
        ..Fake::default()
    };
    assert_eq!(hostname(&["newname"], &other), ok(""));
}

#[test]
fn a_host_name_is_trimmed_and_checked_before_it_is_set() {
    let sys = Fake::default();
    assert_eq!(hostname(&[" \t name.example \n\x0b"], &sys), ok(""));
    assert_eq!(*sys.sets.borrow(), vec![("host", b"name.example".to_vec())]);
    for bad in [
        "",
        "   ",
        "-lead",
        "trail-",
        ".lead",
        "trail.",
        "a..b",
        "a-.b",
        "a.-b",
        "a_b",
        "a b",
        "caf\u{e9}",
    ] {
        let sys = Fake::default();
        assert_eq!(
            hostname(&["--", bad], &sys),
            failed("hostname: the specified hostname is invalid\n"),
            "{bad:?}"
        );
        assert!(sys.sets.borrow().is_empty(), "{bad:?} reached the library");
    }
    for good in ["a", "a-b", "a--b", "a.b", "1.2.3.4", "x-1.y-2"] {
        let sys = Fake::default();
        assert_eq!(hostname(&[good], &sys), ok(""), "{good:?}");
    }
}

#[test]
fn a_domain_name_is_set_as_given() {
    let sys = Fake::default();
    assert_eq!(run_as("domainname", &[" odd_name. "], &sys), ok(""));
    assert_eq!(run_as("ypdomainname", &["lab"], &sys), ok(""));
    assert_eq!(hostname(&["-y", "lab2"], &sys), ok(""));
    assert_eq!(
        *sys.sets.borrow(),
        vec![
            ("domain", b" odd_name. ".to_vec()),
            ("domain", b"lab".to_vec()),
            ("domain", b"lab2".to_vec()),
        ]
    );
}

#[test]
fn only_the_host_and_nis_names_can_be_set() {
    for flag in ["-s", "-f", "-d", "-a", "-i", "-I", "-A"] {
        let sys = Fake::default();
        assert_eq!(hostname(&[flag, "newname"], &sys), usage_err(), "{flag}");
        assert!(sys.sets.borrow().is_empty());
    }
    assert_eq!(
        run_as("dnsdomainname", &["x"], &Fake::default()),
        usage_err()
    );
}

#[test]
fn a_second_operand_is_a_usage_error() {
    let sys = Fake::default();
    assert_eq!(hostname(&["one", "two"], &sys), usage_err());
    assert!(sys.sets.borrow().is_empty());
}

// ---------------------------------------------------------------- the command line

#[test]
fn version_and_help() {
    let sys = Fake::default();
    assert_eq!(hostname(&["-V"], &sys), ok("hostname 3.23\n"));
    assert_eq!(hostname(&["--version"], &sys), ok("hostname 3.23\n"));
    assert_eq!(hostname(&["-h"], &sys), usage_out(""));
    assert_eq!(hostname(&["--help"], &sys), usage_out(""));
    assert_eq!(hostname(&["-?"], &sys), usage_out(""));
    // Each acts where it is met: a bad option after `-V` is never read.
    assert_eq!(hostname(&["-V", "-Q"], &sys), ok("hostname 3.23\n"));
}

#[test]
fn a_bad_option_is_getopts_message_then_the_usage_on_standard_output() {
    let sys = Fake::default();
    assert_eq!(
        hostname(&["-Q"], &sys),
        usage_out("hostname: invalid option -- 'Q'\n")
    );
    assert_eq!(
        hostname(&["--nosuch"], &sys),
        usage_out("hostname: unrecognized option '--nosuch'\n")
    );
    assert_eq!(
        hostname(&["-F"], &sys),
        usage_out("hostname: option requires an argument -- 'F'\n")
    );
    assert_eq!(
        hostname(&["--file"], &sys),
        usage_out("hostname: option '--file' requires an argument\n")
    );
    assert_eq!(
        hostname(&["--short=x"], &sys),
        usage_out("hostname: option '--short' doesn't allow an argument\n")
    );
    // getopt names the program as it was run, whole path and all.
    assert_eq!(
        run_as("/bin/dnsdomainname", &["-Q"], &sys),
        usage_out("/bin/dnsdomainname: invalid option -- 'Q'\n")
    );
}

#[test]
fn long_options_abbreviate_as_getopt_has_them() {
    let sys = Fake::default();
    assert_eq!(hostname(&["--sh"], &sys), ok("logo\n"));
    assert_eq!(hostname(&["--l"], &sys), ok("logo.example.org\n"));
    assert_eq!(
        hostname(&["--a"], &sys),
        usage_out(
            "hostname: option '--a' is ambiguous; possibilities: '--all-fqdns' '--alias' '--all-ip-addresses'\n"
        )
    );
    assert_eq!(
        hostname(&["--f"], &sys),
        usage_out("hostname: option '--f' is ambiguous; possibilities: '--file' '--fqdn'\n")
    );
}

// ---------------------------------------------------------------- -F

/// A file in the temporary directory holding `content`, removed on drop.
struct TempFile(std::path::PathBuf);

impl TempFile {
    fn new(content: &[u8]) -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "hostname-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::write(&path, content).unwrap();
        TempFile(path)
    }
    /// The path as an argument, bytes and all: a temporary directory's name
    /// need not be text.
    fn arg(&self) -> OsString {
        self.0.clone().into_os_string()
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        // Ignored: a leftover temporary file is no test's failure.
        let _ = std::fs::remove_file(&self.0);
    }
}

/// What `hostname -F` (with `extra` flags first) sets from a file holding
/// `content`, or how it fails.
fn set_from(extra: &[&str], content: &[u8]) -> (Ran, Vec<(&'static str, Vec<u8>)>) {
    let file = TempFile::new(content);
    let sys = Fake::default();
    let mut args: Vec<OsString> = extra.iter().map(OsString::from).collect();
    args.push("-F".into());
    args.push(file.arg());
    let ran = run_os("hostname", args, &sys);
    let sets = sys.sets.borrow().clone();
    (ran, sets)
}

#[test]
fn the_first_line_that_is_not_blank_or_a_comment() {
    let host = |name: &[u8]| vec![("host", name.to_vec())];
    assert_eq!(set_from(&[], b"name\n"), (ok(""), host(b"name")));
    assert_eq!(set_from(&[], b"first\nsecond\n"), (ok(""), host(b"first")));
    assert_eq!(
        set_from(&[], b"# a comment\n\nafter\n"),
        (ok(""), host(b"after"))
    );
    assert_eq!(set_from(&[], b"no-newline"), (ok(""), host(b"no-newline")));
    // White space is trimmed by setting, not by reading.
    assert_eq!(set_from(&[], b"  spaced  \n"), (ok(""), host(b"spaced")));
    // A NUL ends the name, as it ends a C string.
    assert_eq!(set_from(&[], b"ab\0cd\n"), (ok(""), host(b"ab")));
}

#[test]
fn a_file_with_no_usable_line_gives_an_invalid_name() {
    let invalid = failed("hostname: the specified hostname is invalid\n");
    // An empty file; a file of one newline, which is the last line read and so
    // the name; a file of comments, likewise; a line of spaces.
    for content in [&b""[..], b"\n", b"# only\n", b"   \n"] {
        assert_eq!(
            set_from(&[], content),
            (invalid.clone(), vec![]),
            "{content:?}"
        );
    }
}

#[test]
fn boot_stands_the_current_name_in_for_a_missing_or_empty_file() {
    let current = vec![("host", b"logo.example.org".to_vec())];
    assert_eq!(set_from(&["-b"], b""), (ok(""), current.clone()));
    let sys = Fake::default();
    assert_eq!(
        hostname(&["-b", "-F", "/nonexistent/hostname-test"], &sys),
        ok("")
    );
    assert_eq!(*sys.sets.borrow(), current);
    // A file that holds only a newline is not empty: its name is the newline.
    assert_eq!(
        set_from(&["-b"], b"\n").0,
        failed("hostname: the specified hostname is invalid\n")
    );
    // No current name either: `localhost`.
    for current in [&b""[..], b"(none)"] {
        let sys = Fake {
            hostname: Ok(current.to_vec()),
            ..Fake::default()
        };
        assert_eq!(
            hostname(&["-b", "-F", "/nonexistent/hostname-test"], &sys),
            ok("")
        );
        assert_eq!(*sys.sets.borrow(), vec![("host", b"localhost".to_vec())]);
    }
}

#[test]
fn a_missing_file_without_boot_is_err() {
    let sys = Fake::default();
    assert_eq!(
        hostname(&["-F", "/nonexistent/hostname-test"], &sys),
        failed("hostname: No such file or directory\n")
    );
}

#[test]
fn a_name_from_a_file_and_an_operand_is_a_usage_error() {
    let file = TempFile::new(b"name\n");
    let sys = Fake::default();
    assert_eq!(
        run_os(
            "hostname",
            vec!["-F".into(), file.arg(), "other".into()],
            &sys
        ),
        usage_err()
    );
    assert!(sys.sets.borrow().is_empty());
}

#[test]
fn a_domain_from_a_file_keeps_what_a_host_name_would_lose() {
    let file = TempFile::new(b"# only a comment\n");
    let sys = Fake::default();
    assert_eq!(
        run_os("domainname", vec!["-F".into(), file.arg()], &sys),
        ok("")
    );
    // The last line read, newline and all: nothing trims a domain name.
    assert_eq!(
        *sys.sets.borrow(),
        vec![("domain", b"# only a comment\n".to_vec())]
    );
}

#[cfg(unix)]
#[test]
fn a_directory_opens_and_reads_as_nothing() {
    let sys = Fake::default();
    assert_eq!(
        hostname(&["-F", "/"], &sys),
        failed("hostname: the specified hostname is invalid\n")
    );
}

#[test]
fn fgets_is_glibcs() {
    let mut stream: &[u8] = b"one\ntwo";
    let mut buf = [0xAAu8; 8];
    assert!(fgets(&mut stream, &mut buf));
    assert_eq!(&buf[..5], b"one\n\0");
    assert!(fgets(&mut stream, &mut buf));
    assert_eq!(&buf[..4], b"two\0");
    // At the end: refused, and the buffer keeps the last line.
    assert!(!fgets(&mut stream, &mut buf));
    assert_eq!(&buf[..4], b"two\0");
    // A line longer than the room is split.
    let mut stream: &[u8] = b"abcdef\n";
    let mut small = [0u8; 4];
    assert!(fgets(&mut stream, &mut small));
    assert_eq!(&small, b"abc\0");
    // One byte: the terminator, and nothing read.
    let mut stream: &[u8] = b"abc";
    let mut one = [0xAAu8; 1];
    assert!(fgets(&mut stream, &mut one));
    assert_eq!(one, [0]);
    assert_eq!(stream, b"abc");
    // None: refused.
    assert!(!fgets(&mut stream, &mut []));
}

#[test]
fn check_name_is_rfc_1035s() {
    assert!(check_name(b"a"));
    assert!(check_name(b"a1-b2.c3"));
    assert!(!check_name(b""));
    assert!(!check_name(b"a-.b"));
    assert!(!check_name(b"a.-b"));
    assert!(!check_name(b"a..b"));
    assert!(!check_name(b"a\xffb"));
}
