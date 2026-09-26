//! logger -- enter messages into the system log.
//!
//! A port of util-linux 2.39.3's `misc-utils/logger.c`, function by function
//! and with upstream's names, built as the distributions build it (with
//! libsystemd): `--journald` exists, and the automatic socket-error mode also
//! turns on when the system was booted with systemd. Measured against
//! `logger from util-linux 2.39.3` by `scripts/logger-diff.sh`.
//!
//! # The modules
//!
//! | module | upstream |
//! |---|---|
//! | [`priority`] | `pencode`, `decode`, glibc's `facilitynames`/`prioritynames` |
//! | [`frame`] | the three headers and `rfc3164_current_time` |
//! | [`sd`] | RFC 5424 structured data |
//! | [`input`] | `logger_command_line`, `logger_stdin` |
//! | [`output`] | the frame `write_output` assembles |
//! | [`deliver`] | `unix_socket`, `inet_socket`, the send |
//! | [`sys`] | the libc calls std does not wrap |
//!
//! # What is not upstream's
//!
//! * **Where a local message goes on SlateOS.** `/dev/log` cannot exist there
//!   yet -- the platform has no path-bound Unix-domain sockets -- so where
//!   every attempt on it fails with `EAFNOSUPPORT` the message becomes a
//!   journal record instead of being dropped; see [`deliver`]. `--journald`
//!   writes the same journal, since there is no journald.
//! * **A name or argument in a diagnostic** is printed where and as upstream
//!   prints it -- pasted ([`shown`]), or inside upstream's own `'...'`
//!   (`quoting::escaped_in_quotes`) -- except that what is not printable is
//!   octal-escaped: a name holding a newline must not be able to forge a
//!   second diagnostic line (design-decisions §370). For printable text the
//!   two are byte-identical.
//! * **`-S 0` with input from stdin** is one empty message per line; upstream
//!   loops forever ([`input`]).

mod deliver;
mod frame;
mod input;
mod output;
mod priority;
mod sd;
mod sys;

use getoptlong::{Opt, Program, Takes};
use localtime::Zone;
use quoting::{escape_unprintable, escaped_in_quotes, escaped_in_quotes_os, os_bytes};
use std::ffi::OsString;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Read};
use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};
use ulclosestream::Stdout;

use deliver::{ALL_TYPES, Conn, OpenError, PATH_DEVLOG, Parts, TYPE_TCP, TYPE_UDP};
use frame::{Header, NILVALUE, TimeVal};

/// Every refusal here is `errx`/`err` with `EXIT_FAILURE`, and getopt's with
/// the referral to `--help` (`errtryhelp(EXIT_FAILURE)`).
const LOGGER: Program = Program::new("logger", 1);

/// upstream's `getopt_long` option string.
const SHORTS: &str = "ef:ip:S:st:u:dTn:P:Vh";

/// upstream's `longopts[]`, IN ITS ORDER: the order is observable, in the
/// candidates `getopt_long` lists for an ambiguous abbreviation (`--pri`:
/// `'--priority' '--prio-prefix'`). `socket-errors` is declared
/// `required_argument` there although the help shows its argument as
/// optional; the table is what getopt obeys.
const LONGS: &[(&str, Takes)] = &[
    ("id", Takes::Optional),
    ("stderr", Takes::Nothing),
    ("file", Takes::Required),
    ("no-act", Takes::Nothing),
    ("priority", Takes::Required),
    ("tag", Takes::Required),
    ("socket", Takes::Required),
    ("socket-errors", Takes::Required),
    ("udp", Takes::Nothing),
    ("tcp", Takes::Nothing),
    ("server", Takes::Required),
    ("port", Takes::Required),
    ("version", Takes::Nothing),
    ("help", Takes::Nothing),
    ("octet-count", Takes::Nothing),
    ("prio-prefix", Takes::Nothing),
    ("rfc3164", Takes::Nothing),
    ("rfc5424", Takes::Optional),
    ("size", Takes::Required),
    ("msgid", Takes::Required),
    ("skip-empty", Takes::Nothing),
    ("sd-id", Takes::Required),
    ("sd-param", Takes::Required),
    ("journald", Takes::Optional),
];

/// `print_version`.
const VERSION: &str = "logger from util-linux 2.39.3\n";

/// `usage()`, byte for byte as util-linux 2.39.3 prints it (captured, not
/// retyped).
const HELP: &str = include_str!("help.txt");

/// `LOG_USER | LOG_NOTICE`.
const DEFAULT_PRI: i32 = (1 << 3) | 5;

/// A name or argument as upstream's diagnostics print it -- as is -- except
/// that a character that is not printable, and a byte that is not part of
/// one, is octal-escaped: `a\nb` prints `a\012b`, so a name cannot write a
/// line of its own into the diagnostics. Everything printable, spaces and
/// quotes included, is left exactly as upstream leaves it.
fn shown(text: &[u8]) -> String {
    escape_unprintable(text)
}

/// [`shown`], for an argument or a path.
fn shown_os(text: &std::ffi::OsStr) -> String {
    escape_unprintable(&os_bytes(text))
}

/// The program's end: the status to exit with, everything already printed.
#[derive(Debug, PartialEq, Eq)]
struct Exit(u8);

/// Print `logger: MSG` to stderr -- `warnx`, and the printing half of `errx`.
///
/// A diagnostic that cannot be written has nowhere else to go, but it is
/// not forgotten: `close_stdout` makes the exit status 1 for it, as upstream's
/// does -- a `send message failed` into a closed stderr is status 1, not 0.
fn diag(msg: &str) {
    ulclosestream::warnx(b"logger", msg);
}

/// `errx(EXIT_FAILURE, ...)`.
fn die(msg: &str) -> Exit {
    diag(msg);
    Exit(1)
}

/// `struct logger_ctl`.
struct Ctl {
    conn: Conn,
    pri: i32,
    /// `pid_t`: 0 when no id is wanted; `--id` values past `INT_MAX` wrap,
    /// as the C assignment does.
    pid: i32,
    hdr: Vec<u8>,
    tag: Vec<u8>,
    msgid: Option<Vec<u8>>,
    unix_socket: Option<OsString>,
    server: Option<OsString>,
    port: Option<OsString>,
    socket_type: u8,
    max_message_size: usize,
    user_sds: Vec<sd::Element>,
    reserved_sds: Vec<sd::Element>,
    header: Option<Header>,
    unix_socket_errors: bool,
    noact: bool,
    prio_prefix: bool,
    stderr_printout: bool,
    rfc5424_time: bool,
    rfc5424_tq: bool,
    rfc5424_host: bool,
    skip_empty_lines: bool,
    octet_count: bool,
    zone: Zone,
}

/// Where the lines come from when there are no message operands.
enum Input {
    Stdin,
    File(File),
}

/// `AF_UNIX_ERRORS_*`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum SocketErrors {
    Off,
    On,
    Auto,
}

stdfdguard::guard_std_fds!();

fn main() -> ExitCode {
    // Before anything touches standard I/O: a descriptor the process was
    // started without is closed again, as upstream would find it.
    stdfdguard::restore();
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    // `logger.c` leaves `CLOSE_EXIT_CODE` at `EXIT_FAILURE`.
    let mut stdout = Stdout::new(1);
    let status = match run(&args, &mut stdout) {
        Ok(()) => 0,
        Err(Exit(code)) => code,
    };
    // `close_stdout`, which upstream registers with `atexit`: every way out
    // passes it.
    ExitCode::from(stdout.close(status, b"logger"))
}

/// `main()`.
fn run(args: &[OsString], stdout: &mut Stdout) -> Result<(), Exit> {
    let mut ctl = Ctl {
        conn: Conn::None,
        pri: DEFAULT_PRI,
        pid: 0,
        hdr: Vec::new(),
        tag: Vec::new(),
        msgid: None,
        unix_socket: None,
        server: None,
        port: None,
        socket_type: ALL_TYPES,
        max_message_size: 1024,
        user_sds: Vec::new(),
        reserved_sds: Vec::new(),
        header: None,
        unix_socket_errors: false,
        noact: false,
        prio_prefix: false,
        stderr_printout: false,
        rfc5424_time: true,
        rfc5424_tq: true,
        rfc5424_host: true,
        skip_empty_lines: false,
        octet_count: false,
        zone: Zone::from_env(),
    };
    let mut tag: Option<Vec<u8>> = None;
    let mut input = Input::Stdin;
    let mut stdin_reopened = false;
    let mut journald: Option<Input> = None;
    let mut errors_mode = SocketErrors::Auto;
    let mut operands: Vec<&OsString> = Vec::new();

    for item in LOGGER.parse(args, SHORTS, LONGS) {
        let item = item.map_err(|e| die(&e.message()))?;
        let (flag, value) = match &item {
            Opt::Operand(word) => {
                operands.push(word);
                continue;
            }
            Opt::Short(c, v) => (Flag::Short(*c), v.as_ref()),
            Opt::Long(name, v) => (Flag::Long(name), v.as_ref()),
        };
        let arg = || value.cloned().unwrap_or_default();
        match flag.name() {
            "file" => {
                let path = arg();
                match File::open(&path) {
                    Ok(f) => input = Input::File(f),
                    Err(e) => {
                        return Err(die(&format!(
                            "file {}: {}",
                            shown_os(&path),
                            errmsg::strerror(&e)
                        )));
                    }
                }
                stdin_reopened = true;
            }
            "skip-empty" => ctl.skip_empty_lines = true,
            "i" => ctl.pid = getpid(),
            "id" => {
                ctl.pid = match value {
                    // Upstream steps a pointer past a leading `=` and then
                    // parses `optarg` anyway, so `--id==5` is refused naming
                    // `'=5'`. Reproduced: it is what a script sees.
                    Some(v) => match ulstrutils::ul_strtou64(&os_bytes(v), 10) {
                        #[expect(
                            clippy::cast_possible_truncation,
                            reason = "strtoul's unsigned long assigned to a pid_t: the wrap is upstream's"
                        )]
                        Ok(n) => n as i32,
                        Err(e) => {
                            return Err(die(&ulstrutils::num_error_message(
                                "failed to parse id",
                                v,
                                e,
                            )));
                        }
                    },
                    None => getpid(),
                };
            }
            "priority" => {
                ctl.pri = priority::pencode(&os_bytes(&arg())).map_err(|e| match e {
                    priority::PriorityError::Facility(name) => {
                        die(&format!("unknown facility name: {}", shown(&name)))
                    }
                    priority::PriorityError::Priority(name) => {
                        die(&format!("unknown priority name: {}", shown(&name)))
                    }
                })?;
            }
            "stderr" => ctl.stderr_printout = true,
            "tag" => tag = Some(os_bytes(&arg()).into_owned()),
            "socket" => ctl.unix_socket = Some(arg()),
            "size" => {
                let v = arg();
                ctl.max_message_size = ulstrutils::parse_size(&os_bytes(&v))
                    .map(|n| usize::try_from(n).unwrap_or(usize::MAX))
                    .map_err(|e| {
                        die(&ulstrutils::size_error_message(
                            "failed to parse message size",
                            &v,
                            e,
                        ))
                    })?;
            }
            "udp" => ctl.socket_type = TYPE_UDP,
            "tcp" => ctl.socket_type = TYPE_TCP,
            "server" => ctl.server = Some(arg()),
            "port" => ctl.port = Some(arg()),
            "octet-count" => ctl.octet_count = true,
            "prio-prefix" => ctl.prio_prefix = true,
            "rfc3164" => ctl.header = Some(Header::Rfc3164),
            "rfc5424" => {
                ctl.header = Some(Header::Rfc5424);
                if let Some(v) = value {
                    parse_rfc5424_flags(&mut ctl, &os_bytes(v));
                }
            }
            "msgid" => {
                let v = os_bytes(&arg()).into_owned();
                if v.contains(&b' ') {
                    return Err(die("--msgid cannot contain space"));
                }
                ctl.msgid = Some(v);
            }
            "journald" => {
                journald = Some(match value {
                    Some(v) => Input::File(File::open(v).map_err(|e| {
                        die(&format!(
                            "cannot open {}: {}",
                            shown_os(v),
                            errmsg::strerror(&e)
                        ))
                    })?),
                    None => Input::Stdin,
                });
            }
            "socket-errors" => errors_mode = parse_unix_socket_errors_flags(&os_bytes(&arg())),
            "no-act" => ctl.noact = true,
            "sd-id" => {
                let v = arg();
                let id = os_bytes(&v).into_owned();
                if !sd::valid_id(&id) {
                    return Err(die(&format!(
                        "invalid structured data ID: {}",
                        escaped_in_quotes_os(&v)
                    )));
                }
                if ctl.user_sds.iter().any(|e| e.id == id) {
                    return Err(die(&format!(
                        "structured data ID {} is not unique",
                        escaped_in_quotes_os(&v)
                    )));
                }
                ctl.user_sds.push(sd::Element {
                    id,
                    params: Vec::new(),
                });
            }
            "sd-param" => {
                let v = arg();
                let param = os_bytes(&v).into_owned();
                if !sd::valid_param(&param) {
                    return Err(die(&format!(
                        "invalid structured data parameter: {}",
                        escaped_in_quotes_os(&v)
                    )));
                }
                match ctl.user_sds.last_mut() {
                    Some(e) => e.params.push(param),
                    None => {
                        return Err(die(&format!(
                            "--sd-id was not specified for --sd-param {}",
                            shown_os(&v)
                        )));
                    }
                }
            }
            "version" => return print_and_close(stdout, VERSION),
            "help" => return print_and_close(stdout, HELP),
            _ => {}
        }
    }

    if stdin_reopened && !operands.is_empty() {
        // Upstream's words; it then logs the MESSAGE and ignores the file,
        // the opposite of what it says, and so does this.
        diag("--file <file> and <message> are mutually exclusive, message is ignored");
    }

    if let Some(source) = journald {
        return journald_entry(&ctl, source);
    }

    // A user-supplied timeQuality element replaces the built-in one.
    if ctl.user_sds.iter().any(|e| e.id == b"timeQuality") {
        ctl.rfc5424_tq = false;
    }

    ctl.unix_socket_errors = match errors_mode {
        SocketErrors::Off => false,
        SocketErrors::On => true,
        SocketErrors::Auto => ctl.noact || ctl.stderr_printout || sd_booted(),
    };

    logger_open(&mut ctl, tag)?;
    if operands.is_empty() {
        logger_stdin(&mut ctl, input)
    } else {
        generate_syslog_header(&mut ctl)?;
        logger_command_line(&mut ctl, &operands)
    }
    // `logger_close`: dropping the connection closes it. Upstream reports a
    // failed `close()` of the socket, which does not fail for a socket in
    // any case this program reaches.
}

/// A short or long option, named the way the dispatch reads it.
enum Flag<'a> {
    Short(u8),
    Long(&'a str),
}

impl Flag<'_> {
    /// The long name a short option shares its case with in upstream's
    /// switch; `-i` has none (`--id` differs: it takes an optional value).
    fn name(&self) -> &str {
        match self {
            Flag::Long(name) => name,
            Flag::Short(c) => match c {
                b'e' => "skip-empty",
                b'f' => "file",
                b'i' => "i",
                b'p' => "priority",
                b'S' => "size",
                b's' => "stderr",
                b't' => "tag",
                b'u' => "socket",
                b'd' => "udp",
                b'T' => "tcp",
                b'n' => "server",
                b'P' => "port",
                b'V' => "version",
                b'h' => "help",
                _ => "",
            },
        }
    }
}

/// `logger_getpid`, as the `pid_t` it is.
fn getpid() -> i32 {
    i32::try_from(std::process::id()).unwrap_or(i32::MAX)
}

/// `parse_rfc5424_flags`: `strtok` on `,`, so empty items vanish.
fn parse_rfc5424_flags(ctl: &mut Ctl, s: &[u8]) {
    for tok in s.split(|&b| b == b',').filter(|t| !t.is_empty()) {
        match tok {
            b"notime" => {
                ctl.rfc5424_time = false;
                ctl.rfc5424_tq = false;
            }
            b"notq" => ctl.rfc5424_tq = false,
            b"nohost" => ctl.rfc5424_host = false,
            _ => diag(&format!("ignoring unknown option argument: {}", shown(tok))),
        }
    }
}

/// `parse_unix_socket_errors_flags`.
fn parse_unix_socket_errors_flags(s: &[u8]) -> SocketErrors {
    match s {
        b"off" => SocketErrors::Off,
        b"on" => SocketErrors::On,
        b"auto" => SocketErrors::Auto,
        _ => {
            diag(&format!(
                "invalid argument: {}: using automatic errors",
                shown(s)
            ));
            SocketErrors::Auto
        }
    }
}

/// libsystemd's `sd_booted()`: `/run/systemd/system/` exists.
fn sd_booted() -> bool {
    std::fs::symlink_metadata("/run/systemd/system/").is_ok()
}

/// `-V` and `-h`: the text into stdout, and exit 0 -- through `main`'s
/// `close_stdout`, which judges whether it could be written.
fn print_and_close(stdout: &mut Stdout, text: &str) -> Result<(), Exit> {
    stdout.write(text.as_bytes());
    Err(Exit(0))
}

/// `logger_open`.
fn logger_open(ctl: &mut Ctl, tag: Option<Vec<u8>>) -> Result<(), Exit> {
    ctl.conn = open_conn(ctl)?;
    if ctl.header.is_none() {
        ctl.header = Some(if ctl.server.is_some() {
            Header::Rfc5424
        } else {
            Header::Local
        });
    }
    ctl.tag = tag
        .or_else(sys::xgetlogin)
        .unwrap_or_else(|| b"<someone>".to_vec());
    Ok(())
}

/// `__logger_open`.
fn open_conn(ctl: &mut Ctl) -> Result<Conn, Exit> {
    let opened = match &ctl.server {
        Some(server) => deliver::inet_socket(server, ctl.port.as_deref(), &mut ctl.socket_type),
        None => {
            let path = ctl
                .unix_socket
                .clone()
                .unwrap_or_else(|| OsString::from(PATH_DEVLOG));
            deliver::unix_socket(&path, &mut ctl.socket_type, ctl.unix_socket_errors)
        }
    };
    opened.map_err(|e| {
        die(&match e {
            OpenError::PathTooLong(p) => format!("openlog {}: pathname too long", shown_os(&p)),
            OpenError::Socket(p, err) => {
                format!("socket {}: {}", shown_os(&p), errmsg::strerror(&err))
            }
            OpenError::Resolve(s, p, text) => {
                format!(
                    "failed to resolve name {} port {}: {text}",
                    shown_os(&s),
                    shown_os(&p)
                )
            }
            OpenError::Connect(s, p) => format!(
                "failed to connect to {} port {}",
                shown_os(&s),
                shown_os(&p)
            ),
        })
    })
}

/// `logger_reopen`.
fn logger_reopen(ctl: &mut Ctl) -> Result<(), Exit> {
    ctl.conn = Conn::None;
    ctl.conn = open_conn(ctl)?;
    Ok(())
}

/// `logger_gettimeofday`.
fn now() -> TimeVal {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(d) => TimeVal {
            sec: i64::try_from(d.as_secs()).unwrap_or(i64::MAX),
            usec: d.subsec_micros(),
        },
        // Before 1970: `timeval` keeps `tv_usec` non-negative, so the second
        // is the one below.
        Err(e) => {
            let d = e.duration();
            let whole = i64::try_from(d.as_secs()).unwrap_or(i64::MAX);
            let micros = d.subsec_micros();
            if micros == 0 {
                TimeVal {
                    sec: whole.saturating_neg(),
                    usec: 0,
                }
            } else {
                TimeVal {
                    sec: whole.saturating_add(1).saturating_neg(),
                    usec: 1_000_000u32.saturating_sub(micros),
                }
            }
        }
    }
}

/// `generate_syslog_header`.
fn generate_syslog_header(ctl: &mut Ctl) -> Result<(), Exit> {
    ctl.hdr = match ctl.header.unwrap_or(Header::Local) {
        Header::Local => frame::local_header(
            ctl.pri,
            &frame::rfc3164_time(&ctl.zone, now().sec),
            &ctl.tag,
            ctl.pid,
        ),
        Header::Rfc3164 => {
            let host = sys::hostname();
            frame::rfc3164_header(
                ctl.pri,
                &frame::rfc3164_time(&ctl.zone, now().sec),
                host.as_deref(),
                &ctl.tag,
                ctl.pid,
            )
        }
        Header::Rfc5424 => rfc5424_header(ctl)?,
    };
    Ok(())
}

/// `syslog_rfc5424_header`, with its checks.
fn rfc5424_header(ctl: &mut Ctl) -> Result<Vec<u8>, Exit> {
    let time = if ctl.rfc5424_time {
        frame::rfc5424_time(&ctl.zone, now()).ok_or_else(|| die("localtime() failed"))?
    } else {
        NILVALUE.to_vec()
    };
    let hostname = if ctl.rfc5424_host {
        let name = sys::hostname().unwrap_or_else(|| NILVALUE.to_vec());
        if name.len() > 255 {
            return Err(die(&format!(
                "hostname {} is too long",
                escaped_in_quotes(&name)
            )));
        }
        name
    } else {
        NILVALUE.to_vec()
    };
    if ctl.tag.len() > 48 {
        return Err(die(&format!(
            "tag {} is too long",
            escaped_in_quotes(&ctl.tag)
        )));
    }
    let procid = if ctl.pid == 0 {
        NILVALUE.to_vec()
    } else {
        ctl.pid.to_string().into_bytes()
    };
    let msgid = ctl.msgid.clone().unwrap_or_else(|| NILVALUE.to_vec());

    // The time-quality element is added once, on the first header, and
    // reused after: every later line's header carries the first line's
    // reading of the clock's state, as upstream's does.
    if ctl.rfc5424_tq && !ctl.reserved_sds.iter().any(|e| e.id == b"timeQuality") {
        let mut params = vec![b"tzKnown=\"1\"".to_vec()];
        match sys::synced_maxerror() {
            Some(maxerror) => {
                params.push(b"isSynced=\"1\"".to_vec());
                params.push(format!("syncAccuracy=\"{maxerror}\"").into_bytes());
            }
            None => params.push(b"isSynced=\"0\"".to_vec()),
        }
        ctl.reserved_sds.push(sd::Element {
            id: b"timeQuality".to_vec(),
            params,
        });
    }
    let structured =
        sd::render(&ctl.reserved_sds, &ctl.user_sds).unwrap_or_else(|| NILVALUE.to_vec());

    Ok(frame::rfc5424_header(
        ctl.pri,
        [&time, &hostname, &ctl.tag, &procid, &msgid, &structured],
    ))
}

/// `write_output`.
fn write_output(ctl: &mut Ctl, msg: &[u8]) -> Result<(), Exit> {
    if !ctl.noact && !ctl.conn.is_connected() {
        logger_reopen(ctl)?;
    }
    let connected = !ctl.noact && ctl.conn.is_connected();
    let tcp_newline = connected && ctl.socket_type == TYPE_TCP && !ctl.octet_count;
    let f = output::frame(&ctl.hdr, msg, ctl.octet_count, tcp_newline);
    if connected {
        // `ctl->pid && !ctl->server && ctl->pid != getpid() && geteuid() == 0
        // && kill(ctl->pid, 0) == 0`: root may name another live process as
        // a local message's sender. Decided once; the retry sends the same.
        let claim =
            (ctl.pid != 0 && ctl.server.is_none() && sys::may_claim(ctl.pid)).then_some(ctl.pid);
        let parts = Parts {
            pri: ctl.pri,
            tag: &ctl.tag,
            pid: ctl.pid,
            msg,
        };
        if deliver::send(&mut ctl.conn, &f.wire, parts, claim).is_err() {
            logger_reopen(ctl)?;
            let parts = Parts {
                pri: ctl.pri,
                tag: &ctl.tag,
                pid: ctl.pid,
                msg,
            };
            if let Err(e) = deliver::send(&mut ctl.conn, &f.wire, parts, claim) {
                diag(&format!("send message failed: {}", errmsg::strerror(&e)));
            }
        }
    }
    if ctl.stderr_printout {
        // `ignore_result(writev(STDERR_FILENO, ...))`, as upstream: not
        // stdio, so a failure here is no lost diagnostic.
        let _ = ulclosestream::stderr_raw(&f.stderr);
    }
    Ok(())
}

/// `logger_command_line`: one header for every chunk.
fn logger_command_line(ctl: &mut Ctl, operands: &[&OsString]) -> Result<(), Exit> {
    let words: Vec<Vec<u8>> = operands.iter().map(|w| os_bytes(w).into_owned()).collect();
    let refs: Vec<&[u8]> = words.iter().map(Vec::as_slice).collect();
    let mut result = Ok(());
    input::command_line(&refs, ctl.max_message_size, &mut |msg| {
        if result.is_ok() {
            result = write_output(ctl, msg);
        }
    });
    result
}

/// `logger_stdin`: a fresh header for every message.
fn logger_stdin(ctl: &mut Ctl, source: Input) -> Result<(), Exit> {
    let reader: Box<dyn Read> = match source {
        Input::Stdin => Box::new(io::stdin().lock()),
        Input::File(f) => Box::new(f),
    };
    // `getchar()` returns EOF on a read error as on end of input.
    let bytes = BufReader::new(reader).bytes().map_while(Result::ok);
    let mut pri = ctl.pri;
    let (max, prefix, skip) = (ctl.max_message_size, ctl.prio_prefix, ctl.skip_empty_lines);
    let mut result = Ok(());
    input::stdin_messages(bytes, max, prefix, skip, &mut pri, &mut |p, msg| {
        if result.is_ok() {
            ctl.pri = p;
            result = generate_syslog_header(ctl).and_then(|()| write_output(ctl, msg));
        }
    });
    result
}

/// `journald_entry`: `KEY=VALUE` lines up to the first empty one, repeated
/// `MESSAGE=` lines joined by newlines into the first, sent as one entry.
///
/// There is no journald on SlateOS; the entry goes to the journal
/// `journalctl` reads, its `MESSAGE`, `PRIORITY` and `SYSLOG_IDENTIFIER`
/// becoming the record's own fields and every other field kept beside them.
fn journald_entry(ctl: &Ctl, source: Input) -> Result<(), Exit> {
    let reader: Box<dyn BufRead> = match source {
        Input::Stdin => Box::new(io::stdin().lock()),
        Input::File(f) => Box::new(BufReader::new(f)),
    };
    let mut lines: Vec<Vec<u8>> = Vec::new();
    let mut msgline: Option<usize> = None;
    for line in reader.split(b'\n') {
        let Ok(mut line) = line else { break };
        // `rtrim_whitespace`: C's isspace, from the end.
        while line.last().is_some_and(|&b| ulstrutils::c_isspace(b)) {
            line.pop();
        }
        if line.is_empty() {
            break;
        }
        if let Some(rest) = line.strip_prefix(b"MESSAGE=") {
            match msgline {
                None => msgline = Some(lines.len()),
                Some(i) => {
                    if let Some(first) = lines.get_mut(i) {
                        first.push(b'\n');
                        first.extend_from_slice(rest);
                    }
                    continue;
                }
            }
        }
        lines.push(line);
    }

    let written = if ctl.noact {
        Ok(())
    } else {
        journald_record(&lines).and_then(|r| deliver::append_record(&r))
    };
    if ctl.stderr_printout {
        for line in &lines {
            // `fprintf(stderr, ...)`, unchecked upstream -- but stdio, so a
            // failure is a lost diagnostic at `close_stdout`.
            ulclosestream::stderr_write(line);
            ulclosestream::stderr_write(b"\n");
        }
    }
    written.map_err(|_| die("journald entry could not be written"))
}

/// The record `sd_journal_sendv` would have made of `fields`.
fn journald_record(fields: &[Vec<u8>]) -> io::Result<journalrec::Record> {
    let mut record = journalrec::Record {
        ts: u64::try_from(now().sec).unwrap_or(0),
        level: "info".to_string(),
        service: String::new(),
        msg: String::new(),
        pid: Some(std::process::id()),
        extra: Vec::new(),
    };
    for field in fields {
        let text =
            std::str::from_utf8(field).map_err(|_| io::Error::from(io::ErrorKind::InvalidData))?;
        // journald refuses a field with no `=`.
        let (key, value) = text
            .split_once('=')
            .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidInput))?;
        match key {
            "MESSAGE" => record.msg = value.to_string(),
            "SYSLOG_IDENTIFIER" => record.service = value.to_string(),
            "PRIORITY" => {
                record.level = value
                    .parse::<usize>()
                    .ok()
                    .and_then(|p| journalrec::PRIORITY_NAMES.get(p))
                    .map_or_else(|| value.to_string(), |n| (*n).to_string());
            }
            _ => record.extra.push((key.to_string(), value.to_string())),
        }
    }
    Ok(record)
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]
mod tests {
    use super::*;

    fn args(words: &[&str]) -> Vec<OsString> {
        words.iter().map(OsString::from).collect()
    }

    /// `run` over `words`, with a stdout nothing is printed to.
    fn ran(words: &[&str]) -> Result<(), Exit> {
        run(&args(words), &mut Stdout::new(1))
    }

    /// Upstream pastes these; so does the port, until a byte could forge a
    /// line or is not text.
    #[test]
    fn a_name_in_a_diagnostic_is_upstreams_unless_it_is_unprintable() {
        assert_eq!(shown(b"x=\"y\""), "x=\"y\"");
        assert_eq!(shown(b"it's a name"), "it's a name");
        assert_eq!(shown(b""), "");
        assert_eq!(shown("café".as_bytes()), "café");
        assert_eq!(shown(b"a\nb"), r"a\012b");
        assert_eq!(shown(b"\x1b[31m"), r"\033[31m");
        assert_eq!(shown(b"\xff"), r"\377");
    }

    #[test]
    fn a_short_option_and_its_long_form_share_their_case() {
        for (c, name) in [
            (b'e', "skip-empty"),
            (b'p', "priority"),
            (b'S', "size"),
            (b'u', "socket"),
            (b'T', "tcp"),
        ] {
            assert_eq!(Flag::Short(c).name(), name);
        }
        // `-i` is not `--id`: it takes no value.
        assert_eq!(Flag::Short(b'i').name(), "i");
    }

    #[test]
    fn rfc5424_flags_are_strtok_items() {
        let mut ctl = test_ctl();
        parse_rfc5424_flags(&mut ctl, b",notq,,nohost,");
        assert!(ctl.rfc5424_time && !ctl.rfc5424_tq && !ctl.rfc5424_host);
        let mut ctl = test_ctl();
        parse_rfc5424_flags(&mut ctl, b"notime");
        assert!(!ctl.rfc5424_time && !ctl.rfc5424_tq && ctl.rfc5424_host);
    }

    #[test]
    fn socket_error_modes() {
        assert_eq!(parse_unix_socket_errors_flags(b"on"), SocketErrors::On);
        assert_eq!(parse_unix_socket_errors_flags(b"off"), SocketErrors::Off);
        assert_eq!(parse_unix_socket_errors_flags(b"maybe"), SocketErrors::Auto);
    }

    #[test]
    fn option_errors_end_the_run_with_status_1() {
        assert_eq!(ran(&["-Q", "x"]), Err(Exit(1)));
        assert_eq!(ran(&["--id=abc", "x"]), Err(Exit(1)));
        assert_eq!(ran(&["--id==5", "x"]), Err(Exit(1)));
        assert_eq!(ran(&["-S", "1.9", "x"]), Err(Exit(1)));
        assert_eq!(ran(&["--msgid", "a b", "x"]), Err(Exit(1)));
        assert_eq!(ran(&["--sd-id", "bad", "x"]), Err(Exit(1)));
        assert_eq!(ran(&["--sd-param", "x=\"y\"", "x"]), Err(Exit(1)));
        assert_eq!(ran(&["-p", "nosuch.x", "x"]), Err(Exit(1)));
        // Ambiguous: `--priority` and `--prio-prefix`.
        assert_eq!(ran(&["--pri", "user.err", "x"]), Err(Exit(1)));
    }

    #[test]
    fn journald_fields_become_a_record() {
        let fields: Vec<Vec<u8>> = [
            "MESSAGE=hello\nworld",
            "PRIORITY=3",
            "SYSLOG_IDENTIFIER=app",
            "CODE_LINE=12",
        ]
        .iter()
        .map(|s| s.as_bytes().to_vec())
        .collect();
        let r = journald_record(&fields).unwrap();
        assert_eq!(
            (r.level.as_str(), r.service.as_str(), r.msg.as_str()),
            ("err", "app", "hello\nworld")
        );
        assert_eq!(r.extra, [("CODE_LINE".to_string(), "12".to_string())]);
        assert!(journald_record(&[b"NOEQUALS".to_vec()]).is_err());
    }

    fn test_ctl() -> Ctl {
        Ctl {
            conn: Conn::None,
            pri: DEFAULT_PRI,
            pid: 0,
            hdr: Vec::new(),
            tag: b"t".to_vec(),
            msgid: None,
            unix_socket: None,
            server: None,
            port: None,
            socket_type: ALL_TYPES,
            max_message_size: 1024,
            user_sds: Vec::new(),
            reserved_sds: Vec::new(),
            header: None,
            unix_socket_errors: false,
            noact: true,
            prio_prefix: false,
            stderr_printout: false,
            rfc5424_time: true,
            rfc5424_tq: true,
            rfc5424_host: true,
            skip_empty_lines: false,
            octet_count: false,
            zone: Zone::utc(),
        }
    }

    /// With a user `timeQuality` element there is no built-in one, and the
    /// header carries the user's.
    #[test]
    fn a_user_time_quality_element_replaces_the_built_in_one() {
        let mut ctl = test_ctl();
        ctl.rfc5424_tq = false;
        ctl.rfc5424_time = false;
        ctl.rfc5424_host = false;
        ctl.user_sds.push(sd::Element {
            id: b"timeQuality".to_vec(),
            params: vec![b"tzKnown=\"0\"".to_vec()],
        });
        let h = rfc5424_header(&mut ctl).unwrap();
        assert_eq!(h, br#"<13>1 - - t - - [timeQuality tzKnown="0"] "#);
    }

    #[test]
    fn a_tag_over_48_bytes_ends_an_rfc5424_run() {
        let mut ctl = test_ctl();
        ctl.tag = vec![b'x'; 49];
        assert_eq!(rfc5424_header(&mut ctl), Err(Exit(1)));
    }
}
