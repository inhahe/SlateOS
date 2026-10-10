//! glibc's answers replayed through `rcmd.rs`, in the world of
//! `rcmd/world.rs`: `rcmd_oracle.txt`, from
//! `posix/tools/oracle/rcmd_harness.py`.

use super::sys::{self, FileSpec, Server, World};
use super::*;
use std::format;
use std::string::{String, ToString};
use std::vec::Vec;

const ORACLE: &str = include_str!("../rcmd_oracle.txt");

/// The harness's hex fields: `NULL`, `~` an empty string, `-` nothing, or
/// bytes in hex.
fn field(tok: &str) -> Option<Vec<u8>> {
    match tok {
        "NULL" => None,
        "~" | "-" => Some(Vec::new()),
        h => Some(
            (0..h.len())
                .step_by(2)
                .map(|i| u8::from_str_radix(&h[i..i + 2], 16).expect("hex"))
                .collect(),
        ),
    }
}

/// The harness's `hexs`: a C string as it prints one.
fn hexs(p: *const u8) -> String {
    if p.is_null() {
        return "NULL".into();
    }
    // SAFETY: a C string from the code under test.
    let b = unsafe { core::slice::from_raw_parts(p, crate::string::strlen(p)) };
    if b.is_empty() { "~".into() } else { hex(b) }
}

/// The harness's `hex`: `-` for nothing.
fn hex(b: &[u8]) -> String {
    if b.is_empty() {
        return "-".into();
    }
    b.iter()
        .map(|x| format!("{x:02x}"))
        .collect::<Vec<_>>()
        .concat()
}

/// A field as a C string the call can be given: NUL-terminated, or NULL.
struct CStr(Option<Vec<u8>>);

impl CStr {
    fn new(tok: &str) -> Self {
        Self(field(tok).map(|mut v| {
            v.push(0);
            v
        }))
    }

    fn ptr(&self) -> *const u8 {
        self.0.as_ref().map_or(core::ptr::null(), |v| v.as_ptr())
    }
}

/// The harness's file spec: `-`, or `kind:mode:links:text`.
fn spec(tok: &str) -> Option<FileSpec> {
    if tok == "-" {
        return None;
    }
    let mut parts = tok.splitn(4, ':');
    let kind = parts.next().expect("kind").as_bytes()[0];
    let mode = u32::from_str_radix(parts.next().expect("mode"), 8).expect("octal");
    let links = parts.next().expect("links").parse().expect("links");
    let text = field(parts.next().expect("text")).expect("text");
    Some(FileSpec {
        text,
        mode,
        kind,
        links,
        uid: 0,
    })
}

fn files(pairs: &[(&str, Option<FileSpec>)]) -> Vec<(Vec<u8>, FileSpec)> {
    pairs
        .iter()
        .filter_map(|(p, s)| s.clone().map(|s| (p.as_bytes().to_vec(), s)))
        .collect()
}

/// The oracle's databases, from its header, as this thread's.
fn databases() {
    use crate::nss_files::{Which, set_test_text};
    for line in ORACLE.lines().take_while(|l| l.starts_with('#')) {
        let Some((name, h)) = line.trim_start_matches("# ").split_once(": ") else {
            continue;
        };
        let which = match name {
            "passwd" => Which::Passwd,
            "hosts" => Which::Hosts,
            "netgroup" => Which::Netgroup,
            "host.conf" => Which::HostConf,
            _ => continue,
        };
        let bytes = field(h).expect("hex");
        set_test_text(which, Some(Vec::leak(bytes)));
    }
}

fn world(files: Vec<(Vec<u8>, FileSpec)>) -> World {
    World {
        files,
        hostname: b"probe.example.com".to_vec(),
        privileged: true,
        ..World::default()
    }
}

fn set_home(home: Option<&[u8]>) {
    match home {
        Some(h) => {
            let mut v = h.to_vec();
            v.push(0);
            // SAFETY: NUL-terminated strings, alive for the call.
            let rc = unsafe { crate::environ::setenv(c"HOME".as_ptr().cast(), v.as_ptr(), 1) };
            assert_eq!(rc, 0);
        }
        None => {
            // SAFETY: a NUL-terminated name.
            let rc = unsafe { crate::environ::unsetenv(c"HOME".as_ptr().cast()) };
            assert_eq!(rc, 0);
        }
    }
}

/// Read `fd` to its end, as the harness's `drain`.
fn drain(fd: i32) -> Vec<u8> {
    let mut out = Vec::new();
    let mut buf = [0u8; 256];
    loop {
        let n = sys::read(fd, &mut buf);
        if n <= 0 {
            return out;
        }
        out.extend_from_slice(&buf[..usize::try_from(n).expect("n")]);
    }
}

fn server(l: &[&str]) -> Option<Server> {
    let family: i32 = l[0].parse().expect("family");
    if family == 0 {
        return None;
    }
    Some(Server {
        family,
        port: l[1].parse().expect("port"),
        back: l[2].parse().expect("back"),
        strings: l[3] == "1",
        reply: field(l[4]).expect("reply"),
        data: field(l[5]).expect("data"),
        errdata: field(l[6]).expect("errdata"),
    })
}

fn busy(tok: &str) -> Vec<u16> {
    if tok == "-" {
        return Vec::new();
    }
    tok.split(',').map(|p| p.parse().expect("port")).collect()
}

/// `errno` as glibc's `getaddrinfo` leaves it in the harness's sandbox:
/// `ENOENT`, from its resolver's open of the `/etc/resolv.conf` that the
/// sandbox's `/etc` lacks -- even for a lookup `/etc/hosts` answers. A path
/// of `rcmd` or `rexec` that sets no `errno` of its own after the lookup (a
/// refusal; an end of file, which `perror` then reports) shows it; one that
/// sets its own (`EAFNOSUPPORT`, the `0` before `poll`, a failed `connect`)
/// does not. This library's lookup opens no such file, so the replay starts
/// each call where glibc's lookup would have left it.
fn sandbox_errno() {
    errno::set_errno(errno::ENOENT);
}

/// After a protocol call: what the harness's `call_line` prints.
fn call_line(s: i32, e: i32, ahost: *mut u8, fd2: i32) -> String {
    let (mut got, mut goterr) = (Vec::new(), Vec::new());
    if s >= 0 {
        if fd2 >= 0 {
            goterr = drain(fd2);
            sys::close(fd2);
        }
        got = drain(s);
        sys::close(s);
    }
    let (stderr, recv, cport) = sys::with(|w| (w.stderr.clone(), w.recv.clone(), w.cport));
    let recv = if cport == 0 && recv.is_empty() {
        "-".to_string()
    } else {
        hex(&recv)
    };
    format!(
        "{} {} {} {} {} {} recv={} cport={}",
        if s >= 0 { "fd" } else { "-1" },
        if s >= 0 { 0 } else { e },
        hexs(ahost),
        hex(&stderr),
        hex(&got),
        hex(&goterr),
        recv,
        cport
    )
}

fn replay(line: &str) -> Option<String> {
    let (left, right) = line.split_once(" | ").expect("line");
    let l: Vec<&str> = left.split(' ').collect();
    let got = match l[0] {
        "ruserok" => {
            let (rhost, ruser, luser) = (CStr::new(l[2]), CStr::new(l[4]), CStr::new(l[5]));
            let rhosts = spec(l[7]);
            sys::set(world(files(&[
                ("/etc/hosts.equiv", spec(l[6])),
                ("/tmp/rc/h/alice/.rhosts", rhosts.clone()),
                ("/tmp/rc/h/root/.rhosts", rhosts),
            ])));
            state::set_errstr(core::ptr::null());
            // SAFETY: C strings.
            let r = unsafe {
                ruserok_af(
                    rhost.ptr(),
                    l[3].parse().expect("su"),
                    ruser.ptr(),
                    luser.ptr(),
                    l[1].parse().expect("af"),
                )
            };
            format!("{r} {}", hexs(state::errstr()))
        }
        "iruserok" => {
            let addr = field(l[2]).expect("addr");
            let (ruser, luser) = (CStr::new(l[4]), CStr::new(l[5]));
            sys::set(world(files(&[
                ("/etc/hosts.equiv", spec(l[6])),
                ("/tmp/rc/h/alice/.rhosts", spec(l[7])),
            ])));
            state::set_errstr(core::ptr::null());
            // SAFETY: an address of its family's size; C strings.
            let r = unsafe {
                iruserok_af(
                    addr.as_ptr().cast(),
                    l[3].parse().expect("su"),
                    ruser.ptr(),
                    luser.ptr(),
                    l[1].parse().expect("af"),
                )
            };
            format!("{r} {}", hexs(state::errstr()))
        }
        "netrc" => {
            let host = field(l[2]).expect("host");
            let (name, pass) = (CStr::new(l[3]), CStr::new(l[4]));
            sys::set(world(files(&[("/tmp/rc/h/carol/.netrc", spec(l[5]))])));
            set_home((l[1] == "1").then_some(&b"/tmp/rc/h/carol"[..]));
            let mut found = Found::default();
            let r = ruserpass(&host, name.ptr(), pass.ptr(), &mut found);
            let n = found
                .name
                .as_ref()
                .map_or(name.ptr(), |b| b.ptr.cast_const());
            let p = found
                .pass
                .as_ref()
                .map_or(pass.ptr(), |b| b.ptr.cast_const());
            let stderr = sys::with(|w| w.stderr.clone());
            set_home(Some(b"/tmp/rc/h/carol"));
            format!("{r} {} {} {}", hexs(n), hexs(p), hex(&stderr))
        }
        "rcmd" => {
            let host = CStr::new(l[3]);
            let (loc, rem, cmd) = (CStr::new(l[5]), CStr::new(l[6]), CStr::new(l[7]));
            let want_fd2 = l[8] == "1";
            sys::set(World {
                server: server(&l[9..16]),
                busy: busy(l[16]),
                ..world(Vec::new())
            });
            let mut ahost = host.ptr().cast_mut();
            let mut fd2 = -1;
            sandbox_errno();
            let port: u16 = l[4].parse().expect("port");
            // SAFETY: C strings, live locals.
            let s = unsafe {
                rcmd_af(
                    &raw mut ahost,
                    port.to_be(),
                    loc.ptr(),
                    rem.ptr(),
                    cmd.ptr(),
                    if want_fd2 {
                        &raw mut fd2
                    } else {
                        core::ptr::null_mut()
                    },
                    l[2].parse().expect("af"),
                )
            };
            let e = errno::get_errno();
            call_line(s, e, ahost, if want_fd2 { fd2 } else { -1 })
        }
        "rexec" => {
            let host = CStr::new(l[3]);
            let (user, pass, cmd) = (CStr::new(l[5]), CStr::new(l[6]), CStr::new(l[7]));
            let want_fd2 = l[8] == "1";
            sys::set(World {
                server: server(&l[9..16]),
                ..world(files(&[("/tmp/rc/h/carol/.netrc", spec(l[16]))]))
            });
            set_home(Some(b"/tmp/rc/h/carol"));
            let mut ahost = host.ptr().cast_mut();
            let mut fd2 = -1;
            sandbox_errno();
            let port: u16 = l[4].parse().expect("port");
            // SAFETY: C strings, live locals.
            let s = unsafe {
                rexec_af(
                    &raw mut ahost,
                    i32::from(port.to_be()),
                    user.ptr(),
                    pass.ptr(),
                    cmd.ptr(),
                    if want_fd2 {
                        &raw mut fd2
                    } else {
                        core::ptr::null_mut()
                    },
                    l[2].parse().expect("af"),
                )
            };
            let e = errno::get_errno();
            call_line(s, e, ahost, if want_fd2 { fd2 } else { -1 })
        }
        "rresvport" | "rresvport-unprivileged" => {
            sys::set(World {
                busy: busy(l[3]),
                privileged: l[0] == "rresvport",
                ..World::default()
            });
            let mut port: i32 = l[2].parse().expect("start");
            errno::set_errno(0);
            // SAFETY: a live local.
            let s = unsafe { rresvport_af(&raw mut port, l[1].parse().expect("af")) };
            let e = errno::get_errno();
            let mut bound = 0;
            if s >= 0 {
                let mut ss = Storage::ZERO;
                let mut len = 128;
                assert_eq!(sys::getsockname(s, &mut ss, &mut len), 0);
                bound = ss.port();
                sys::close(s);
            }
            format!(
                "{} {} {port} {bound}",
                if s >= 0 { "fd" } else { "-1" },
                if s >= 0 { 0 } else { e }
            )
        }
        other => panic!("an oracle line of no kind known: {other}"),
    };
    let mut want: Vec<String> = right.split(' ').map(String::from).collect();
    if l[0] == "rcmd" && l[1] == "unspec-v4-only" {
        // `dual` is `::1` and `127.0.0.1`. glibc sorts `::1` first, finds it
        // refused, says so and tries `127.0.0.1`; this library's
        // `getaddrinfo` sorts an IPv6 answer after an IPv4 one, there being
        // no IPv6 socket on this system to reach it (gai.rs's module doc), so
        // the first address answers and nothing is said. The fallback itself
        // is `the_next_address_is_tried_after_a_refusal`'s.
        want[3] = "-".into();
    }
    let want = want.join(" ");
    (got != want).then(|| format!("{left}\n    glibc: {right}\n    ours:  {got}"))
}

/// Every line of glibc's answers, replayed.
#[test]
fn rcmd_is_glibcs() {
    databases();
    let lines: Vec<&str> = ORACLE.lines().filter(|l| !l.starts_with('#')).collect();
    assert!(lines.len() > 1000, "{} lines", lines.len());
    let failures: Vec<String> = lines.iter().filter_map(|l| replay(l)).collect();
    assert!(
        failures.is_empty(),
        "{} of {} differ from glibc's:\n{}",
        failures.len(),
        lines.len(),
        failures
            .iter()
            .take(30)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
}

/// A refused connection is tried again after 1, 2, 4, 8 and 16 seconds --
/// the oracle sees only the time it took.
#[test]
fn a_refused_connection_backs_off_as_glibcs_does() {
    databases();
    sys::set(world(Vec::new()));
    let mut ahost = c"localhost".as_ptr().cast_mut().cast::<u8>();
    // SAFETY: C strings, live locals.
    let s = unsafe {
        rcmd_af(
            &raw mut ahost,
            514u16.to_be(),
            c"alice".as_ptr().cast(),
            c"bob".as_ptr().cast(),
            c"x".as_ptr().cast(),
            core::ptr::null_mut(),
            AF_INET as SaFamilyT,
        )
    };
    assert_eq!(s, -1);
    assert_eq!(sys::with(|w| w.sleeps.clone()), [1, 2, 4, 8, 16]);
    sys::set(world(Vec::new()));
    // SAFETY: as above.
    let s = unsafe {
        rexec_af(
            &raw mut ahost,
            i32::from(512u16.to_be()),
            c"carol".as_ptr().cast(),
            c"pw".as_ptr().cast(),
            c"x".as_ptr().cast(),
            core::ptr::null_mut(),
            AF_INET as SaFamilyT,
        )
    };
    assert_eq!(s, -1);
    assert_eq!(sys::with(|w| w.sleeps.clone()), [1, 2, 4, 8, 16]);
}

/// An address refused when another follows: said, in glibc's words, and the
/// next one tried -- here `dual`'s IPv4 address first, as this library sorts
/// it, refused by a server that listens on IPv6 alone.
#[test]
fn the_next_address_is_tried_after_a_refusal() {
    databases();
    sys::set(World {
        server: Some(Server {
            family: AF_INET6,
            port: 514,
            back: 1000,
            strings: true,
            reply: std::vec![0],
            data: b"out\n".to_vec(),
            errdata: Vec::new(),
        }),
        ..world(Vec::new())
    });
    let mut ahost = c"dual".as_ptr().cast_mut().cast::<u8>();
    // SAFETY: C strings, live locals.
    let s = unsafe {
        rcmd_af(
            &raw mut ahost,
            514u16.to_be(),
            c"alice".as_ptr().cast(),
            c"bob".as_ptr().cast(),
            c"x".as_ptr().cast(),
            core::ptr::null_mut(),
            AF_UNSPEC as SaFamilyT,
        )
    };
    assert!(s >= 0, "{s}");
    assert_eq!(drain(s), b"out\n");
    sys::close(s);
    let stderr = sys::with(|w| w.stderr.clone());
    assert_eq!(
        String::from_utf8_lossy(&stderr),
        "connect to address 127.0.0.1: Connection refused\nTrying ::1...\n"
    );
    assert_eq!(sys::with(|w| w.recv.clone()), b"\0alice\0bob\0x\0");
}

/// `rexec` with no password, given or in `~/.netrc`, fails with `EINVAL`,
/// where glibc's takes the length of the NULL.
#[test]
fn rexec_without_a_password_fails_rather_than_crashing() {
    databases();
    sys::set(World {
        server: Some(Server {
            family: AF_INET,
            port: 512,
            back: 0,
            strings: true,
            ..Server::default()
        }),
        ..world(Vec::new())
    });
    set_home(Some(b"/tmp/rc/h/carol"));
    let mut ahost = c"localhost".as_ptr().cast_mut().cast::<u8>();
    errno::set_errno(0);
    // SAFETY: C strings and a NULL password, live locals.
    let s = unsafe {
        rexec_af(
            &raw mut ahost,
            i32::from(512u16.to_be()),
            c"carol".as_ptr().cast(),
            core::ptr::null(),
            c"x".as_ptr().cast(),
            core::ptr::null_mut(),
            AF_INET as SaFamilyT,
        )
    };
    assert_eq!((s, errno::get_errno()), (-1, errno::EINVAL));
}

/// `~/.rhosts` owned by another user than the local one, or root, is
/// refused -- the one check the oracle, whose namespace maps uid 0 alone,
/// cannot make.
#[test]
fn a_rhosts_file_another_user_owns_is_refused() {
    databases();
    let mut f = spec("f:600:1:616c70686120626f620a").expect("spec");
    f.uid = 1234;
    sys::set(world(std::vec![(b"/tmp/rc/h/alice/.rhosts".to_vec(), f)]));
    state::set_errstr(core::ptr::null());
    // SAFETY: C strings.
    let r = unsafe {
        ruserok_af(
            c"alpha".as_ptr().cast(),
            1,
            c"bob".as_ptr().cast(),
            c"alice".as_ptr().cast(),
            AF_INET as SaFamilyT,
        )
    };
    assert_eq!(r, -1);
    assert_eq!(hexs(state::errstr()), hex(b"bad owner"));
}

/// A `~/.netrc` word past 99 bytes keeps its first 99, where glibc's
/// writes past its buffer.
#[test]
fn a_long_netrc_word_is_cut_short_not_overflowed() {
    let long = "p".repeat(150);
    let text = format!("machine localhost login carol password {long}\n");
    sys::set(world(std::vec![(
        b"/tmp/rc/h/carol/.netrc".to_vec(),
        FileSpec {
            text: text.into_bytes(),
            mode: 0o600,
            kind: b'f',
            links: 1,
            uid: 0
        },
    )]));
    set_home(Some(b"/tmp/rc/h/carol"));
    let mut found = Found::default();
    assert_eq!(
        ruserpass(
            b"localhost",
            core::ptr::null(),
            core::ptr::null(),
            &mut found
        ),
        0
    );
    let pass = found.pass.as_ref().map(found_text).expect("a password");
    assert_eq!(pass, "p".repeat(99).as_bytes());
}
