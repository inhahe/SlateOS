//! The guest agent: the in-guest end of the host's channel into a running
//! SlateOS (C-Q11 idea 1, `requests/c-a-two-ways-to-test-a-change-without-a-full-boot.md`).
//!
//! When QEMU gives the guest a virtio console port named [`PORT`]
//! (`virtio::console`; `scripts/guest.py` starts such a guest and speaks to
//! it), this task serves the port. The host can put a file into the guest,
//! read one out, run a program with the capabilities it names and get back
//! its output and exit code, and run a kernel-shell command. So a change
//! outside the kernel -- a service, a utility, a C fixture -- can be tried in
//! a guest that is already up, in seconds rather than a boot. It is for trying
//! a change, never for publishing one: the guest's state carries from one
//! request to the next, and the boot test stays the gate for `main`.
//!
//! ## Why in the kernel
//!
//! It runs programs holding whatever the host asks, as the boot's own rungs do
//! (`proc::spawn`), and it is no more than the kernel shell already is. The
//! host that attached the port controls the whole virtual machine, so the
//! agent grants it nothing it lacked; and no program in the guest can reach
//! the port, which has no device file (design-decisions §1534).
//!
//! ## The protocol
//!
//! A request is one line of ASCII words -- a verb, then numbers -- ending in
//! `\n`, followed by the byte fields its numbers measure. A reply has the same
//! shape. Lengths, never delimiters, frame the fields, so a path, a file or an
//! argument may hold any byte.
//!
//! | request | its fields | reply |
//! |---|---|---|
//! | `ping` | | `pong 1` (the protocol's version) |
//! | `put <mode, octal> <path-len> <data-len>` | the path, the data | `ok 0` |
//! | `get <path-len>` | the path | `ok <len>`, then the file |
//! | `run <seconds> <grants-len> <argc> <arg-len>...` | the grants, then each argument; the first is the program's path | `exit <code> <len>` or `timeout <len>`, then what it wrote to stdout and stderr |
//! | `sh <len>` | a kernel-shell command line | `ok <status> <len>`, then its output |
//!
//! Grants are `-` for none, or words joined by `,` from the C fixtures'
//! vocabulary (`proc::spawn::ctest_generic_grant`: `file`, `secureboot`), so
//! a program tried here holds what its fixture would at boot.
//!
//! Any request may be answered `err <len>` and a message instead. A request
//! line the agent cannot read ends the exchange: it answers `err`, forgets
//! everything buffered, and waits for the host to begin again.

use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use crate::cap::{ResourceType, Rights};
use crate::error::{KernelError, KernelResult};
use crate::fs::path::Path;
use crate::serial_println;
use crate::virtio::console;

/// The port the agent serves.
pub const PORT: &[u8] = b"org.slateos.agent.0";

/// The protocol's version, which `ping` answers.
const VERSION: u32 = 1;

/// Serial-log prefix.
const LOG_TAG: &str = "[guest-agent]";

/// The longest request line.
const MAX_LINE: usize = 1024;
/// The longest field: a whole toolchain binary fits.
const MAX_FIELD: u64 = 64 << 20;
/// The most arguments a `run` may pass.
const MAX_ARGS: u64 = 256;
/// The longest a `run` may last.
const MAX_SECONDS: u64 = 3600;
/// The most output a `run` hands back; past it, the rest is cut.
const MAX_OUTPUT: usize = 16 << 20;
/// How long the task sleeps when the port has nothing for it.
const IDLE_MS: u64 = 5;

/// Whether the task has been started.
static STARTED: AtomicBool = AtomicBool::new(false);
/// Numbers each `run`'s capture file.
static RUNS: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Requests
// ---------------------------------------------------------------------------

/// One request, its fields read.
#[derive(Debug, PartialEq, Eq)]
enum Request {
    Ping,
    Put {
        mode: u16,
        path: Vec<u8>,
        data: Vec<u8>,
    },
    Get {
        path: Vec<u8>,
    },
    Run {
        seconds: u64,
        grants: Vec<u8>,
        argv: Vec<Vec<u8>>,
    },
    Sh {
        command: Vec<u8>,
    },
}

/// What the bytes buffered so far hold.
#[derive(Debug, PartialEq, Eq)]
enum Parsed {
    /// Not a whole request yet.
    Incomplete,
    /// A request, and how many bytes it took.
    Request(Request, usize),
    /// Not a request: what to answer.
    Invalid(&'static str),
}

/// Cut `lengths` fields off the front of `body`: `None` until all of them
/// have arrived.
fn take_fields(body: &[u8], lengths: &[u64]) -> Option<(Vec<Vec<u8>>, usize)> {
    let mut fields = Vec::with_capacity(lengths.len());
    let mut at = 0usize;
    for &len in lengths {
        let len = usize::try_from(len).ok()?;
        let end = at.checked_add(len)?;
        fields.push(body.get(at..end)?.to_vec());
        at = end;
    }
    Some((fields, at))
}

/// Read one request off the front of `buf`.
fn parse(buf: &[u8]) -> Parsed {
    let Some(newline) = buf.iter().position(|&b| b == b'\n') else {
        return if buf.len() > MAX_LINE {
            Parsed::Invalid("a request line longer than 1024 bytes")
        } else {
            Parsed::Incomplete
        };
    };
    if newline > MAX_LINE {
        return Parsed::Invalid("a request line longer than 1024 bytes");
    }
    let Some(line) = buf
        .get(..newline)
        .and_then(|l| core::str::from_utf8(l).ok())
    else {
        return Parsed::Invalid("a request line that is not text");
    };
    let mut words = line.split(' ');
    let verb = words.next().unwrap_or_default();
    let words: Vec<&str> = words.collect();
    let body = buf.get(newline.saturating_add(1)..).unwrap_or_default();
    let line_len = newline.saturating_add(1);

    // The verb's numbers: the octal mode of a `put`, decimal everywhere else.
    let decimal = |w: &&str| w.parse::<u64>().ok();
    let numbers: Option<Vec<u64>> = match verb {
        "put" => {
            let mode = words.first().and_then(|w| u64::from_str_radix(w, 8).ok());
            let rest: Option<Vec<u64>> = words
                .get(1..)
                .unwrap_or_default()
                .iter()
                .map(decimal)
                .collect();
            mode.zip(rest).map(|(m, mut r)| {
                r.insert(0, m);
                r
            })
        }
        _ => words.iter().map(decimal).collect(),
    };
    let Some(numbers) = numbers else {
        return Parsed::Invalid("a request line with a word that is not a number");
    };

    // Which numbers measure fields, by verb.
    let lengths: Vec<u64> = match (verb, numbers.as_slice()) {
        ("ping", []) => Vec::new(),
        ("put", [mode, path_len, data_len]) if *mode <= 0o7777 => {
            alloc::vec![*path_len, *data_len]
        }
        ("get" | "sh", [len]) => alloc::vec![*len],
        ("run", [seconds, grants_len, argc, args @ ..])
            if *seconds <= MAX_SECONDS
                && (1..=MAX_ARGS).contains(argc)
                && u64::try_from(args.len()).ok() == Some(*argc) =>
        {
            let mut all = alloc::vec![*grants_len];
            all.extend_from_slice(args);
            all
        }
        ("ping" | "put" | "get" | "run" | "sh", _) => {
            return Parsed::Invalid("a request with the wrong numbers for its verb");
        }
        _ => return Parsed::Invalid("an unknown request"),
    };
    // The whole request is bounded, not each field: what is buffered waiting
    // for it is the agent's memory.
    let total = lengths
        .iter()
        .try_fold(0u64, |sum, &len| sum.checked_add(len));
    if total.is_none_or(|t| t > MAX_FIELD) {
        return Parsed::Invalid("a request longer than 64 MiB");
    }
    let Some((mut fields, used)) = take_fields(body, &lengths) else {
        return Parsed::Incomplete;
    };
    let request = match verb {
        "ping" => Some(Request::Ping),
        "put" => {
            let data = fields.pop();
            let path = fields.pop();
            let mode = numbers.first().and_then(|&m| u16::try_from(m).ok());
            match (mode, path, data) {
                (Some(mode), Some(path), Some(data)) => Some(Request::Put { mode, path, data }),
                _ => None,
            }
        }
        "get" => fields.pop().map(|path| Request::Get { path }),
        "sh" => fields.pop().map(|command| Request::Sh { command }),
        "run" => {
            let argv = fields.split_off(1.min(fields.len()));
            let seconds = numbers.first().copied();
            match (seconds, fields.pop()) {
                (Some(seconds), Some(grants)) => Some(Request::Run {
                    seconds,
                    grants,
                    argv,
                }),
                _ => None,
            }
        }
        _ => None,
    };
    match request {
        Some(r) => Parsed::Request(r, line_len.saturating_add(used)),
        None => Parsed::Invalid("a request whose fields could not be read"),
    }
}

// ---------------------------------------------------------------------------
// Replies
// ---------------------------------------------------------------------------

/// A reply line and its one field.
fn reply(line: &str, field: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(line.len().saturating_add(1).saturating_add(field.len()));
    out.extend_from_slice(line.as_bytes());
    out.push(b'\n');
    out.extend_from_slice(field);
    out
}

/// `err <len>` and the message.
fn error_reply(message: &str) -> Vec<u8> {
    reply(&alloc::format!("err {}", message.len()), message.as_bytes())
}

/// An error from the kernel, as a reply.
fn kernel_error(what: &str, e: KernelError) -> Vec<u8> {
    error_reply(&alloc::format!("{what}: {e:?}"))
}

/// Serve one request.
fn serve(request: Request) -> Vec<u8> {
    match request {
        Request::Ping => reply(&alloc::format!("pong {VERSION}"), &[]),
        Request::Put { mode, path, data } => {
            let path = Path::new(&path);
            if !path.is_absolute() {
                return error_reply("put: the path must be absolute");
            }
            let written = crate::fs::Vfs::write_file(path, &data)
                .and_then(|()| crate::fs::Vfs::set_permissions(path, mode));
            match written {
                Ok(()) => reply("ok 0", &[]),
                Err(e) => kernel_error("put", e),
            }
        }
        Request::Get { path } => match crate::fs::Vfs::read_file(Path::new(&path)) {
            Ok(bytes) => reply(&alloc::format!("ok {}", bytes.len()), &bytes),
            Err(e) => kernel_error("get", e),
        },
        Request::Run {
            seconds,
            grants,
            argv,
        } => run(seconds, &grants, &argv),
        Request::Sh { command } => {
            let Ok(command) = core::str::from_utf8(&command) else {
                return error_reply("sh: the kernel shell reads text");
            };
            let (output, status) = crate::kshell::capture_command_with_status(command);
            reply(&alloc::format!("ok {} {}", status, output.len()), &output)
        }
    }
}

/// The capabilities `grants` names: `-` for none, else words joined by `,`.
fn grants_of(grants: &[u8]) -> Result<Vec<(ResourceType, u64, Rights)>, Vec<u8>> {
    let Ok(text) = core::str::from_utf8(grants) else {
        return Err(error_reply("run: grants are words"));
    };
    if text == "-" {
        return Ok(Vec::new());
    }
    text.split(',')
        .map(|word| {
            crate::proc::spawn::ctest_generic_grant(word)
                .ok_or_else(|| error_reply(&alloc::format!("run: no grant named {word}")))
        })
        .collect()
}

/// Run `argv[0]` with `argv` and the `grants`, for up to `seconds`; its
/// stdout and stderr go to a capture file, handed back when it ends.
fn run(seconds: u64, grants: &[u8], argv: &[Vec<u8>]) -> Vec<u8> {
    use crate::fs::handle::{self, OpenFlags};
    use crate::proc::spawn::{self, SpawnOptions, fd_handle_type};

    let capabilities = match grants_of(grants) {
        Ok(c) => c,
        Err(reply) => return reply,
    };
    let Some(program) = argv.first() else {
        return error_reply("run: no program");
    };
    let elf = match crate::fs::Vfs::read_file(Path::new(program)) {
        Ok(e) => e,
        Err(e) => return kernel_error("run: reading the program", e),
    };
    let number = RUNS.fetch_add(1, Ordering::Relaxed);
    let out_path = alloc::format!("/tmp/.guest-agent-run-{number}.out");
    let out_path = out_path.as_str();
    // A capture file of this run's own: one left by an earlier run of the same
    // number would otherwise answer for this one. Absent is the usual case.
    let _ = crate::fs::Vfs::remove(out_path);
    let out = match handle::open(
        out_path,
        OpenFlags::WRITE
            .union(OpenFlags::CREATE)
            .union(OpenFlags::TRUNCATE),
    ) {
        Ok(h) => h,
        Err(e) => return kernel_error("run: the capture file", e),
    };
    let fd_map = [
        (0_i32, fd_handle_type::CONSOLE, 0_u64),
        (1_i32, fd_handle_type::FILE, out),
        (2_i32, fd_handle_type::FILE, out),
    ];
    let args: Vec<&[u8]> = argv.iter().map(Vec::as_slice).collect();
    let name = program
        .rsplit(|&b| b == b'/')
        .next()
        .and_then(|n| core::str::from_utf8(n).ok())
        .unwrap_or("guest-agent-run");
    let envp: [&[u8]; 2] = [b"PATH=/bin:/usr/bin", b"HOME=/tmp"];
    let options = SpawnOptions {
        name,
        parent: 0,
        priority: crate::sched::task::DEFAULT_PRIORITY,
        capabilities: &capabilities,
        fd_map: &fd_map,
        argv: &args,
        envp: &envp,
        exe_path: Some(program.as_slice()),
        cwd: None,
        uid_gid: None,
    };
    let spawned = spawn::spawn_process(&elf, &options);
    // The child holds its own duplicates; this one would only leak.
    if let Err(e) = handle::close(out) {
        serial_println!("{} closing the capture handle: {:?}", LOG_TAG, e);
    }
    let result = match spawned {
        Ok(r) => r,
        Err(e) => {
            // Best effort: the capture file of a run that never started.
            let _ = crate::fs::Vfs::remove(out_path);
            return kernel_error("run: spawning", e);
        }
    };

    let deadline = crate::hrtimer::now_ns().saturating_add(seconds.saturating_mul(1_000_000_000));
    let finished = loop {
        if crate::proc::pcb::state(result.pid) == Some(crate::proc::pcb::ProcessState::Zombie) {
            break true;
        }
        if crate::hrtimer::now_ns() >= deadline {
            break false;
        }
        crate::sched::sleep_ms(1);
    };
    let code = crate::proc::pcb::exit_code(result.pid);
    spawn::teardown_fixture(result.pid, result.task_id);

    // A capture file that cannot be read answers as no output: the exit code
    // is the run's verdict, and the line below says how much came back.
    let mut output = crate::fs::Vfs::read_file(out_path).unwrap_or_default();
    // Best effort: the capture file, read already.
    let _ = crate::fs::Vfs::remove(out_path);
    output.truncate(MAX_OUTPUT);
    match (finished, code) {
        (true, Some(code)) => reply(&alloc::format!("exit {} {}", code, output.len()), &output),
        (true, None) => error_reply("run: it ended with no exit code"),
        (false, _) => reply(&alloc::format!("timeout {}", output.len()), &output),
    }
}

// ---------------------------------------------------------------------------
// The task
// ---------------------------------------------------------------------------

/// Start the agent, when the virtio console has [`PORT`]. Called once the
/// console is up; a second call does nothing.
pub fn start() {
    if console::port_named(PORT).is_none() {
        return;
    }
    if STARTED.swap(true, Ordering::AcqRel) {
        return;
    }
    let pml4 = crate::mm::page_table::active_pml4_phys();
    match crate::sched::spawn(
        b"guest-agent",
        crate::sched::task::DEFAULT_PRIORITY,
        agent_task,
        0,
        pml4,
    ) {
        Ok(tid) => serial_println!("{} serving {} (task {})", LOG_TAG, PORT.escape_ascii(), tid),
        Err(e) => {
            STARTED.store(false, Ordering::Release);
            serial_println!("{} would not start: {:?}", LOG_TAG, e);
        }
    }
}

/// Read the port, serve whole requests, and answer.
extern "C" fn agent_task(_arg: u64) {
    let mut pending: Vec<u8> = Vec::new();
    let mut chunk = alloc::vec![0u8; 64 * 1024];
    loop {
        let Some(id) = console::port_named(PORT) else {
            // Gone with its device: nothing to serve.
            serial_println!("{} the port went away; stopping", LOG_TAG);
            STARTED.store(false, Ordering::Release);
            return;
        };
        match console::read(id, &mut chunk) {
            Ok(0) => {
                // The host end closed: a new connection begins a new exchange.
                pending.clear();
                crate::sched::sleep_ms(IDLE_MS.saturating_mul(20));
                continue;
            }
            Ok(n) => pending.extend_from_slice(chunk.get(..n).unwrap_or_default()),
            Err(KernelError::WouldBlock) => {
                crate::sched::sleep_ms(IDLE_MS);
                continue;
            }
            Err(e) => {
                serial_println!("{} reading the port: {:?}", LOG_TAG, e);
                crate::sched::sleep_ms(IDLE_MS.saturating_mul(20));
                continue;
            }
        }
        loop {
            match parse(&pending) {
                Parsed::Incomplete => break,
                Parsed::Request(request, used) => {
                    pending.drain(..used.min(pending.len()));
                    let answer = serve(request);
                    send(id, &answer);
                }
                Parsed::Invalid(why) => {
                    pending.clear();
                    send(id, &error_reply(why));
                    break;
                }
            }
        }
    }
}

/// Answer the host; a reply it cannot take is reported and dropped, as there
/// is no one else to tell.
fn send(id: u32, bytes: &[u8]) {
    if let Err(e) = console::write(id, bytes) {
        serial_println!("{} answering: {:?}", LOG_TAG, e);
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// The protocol's reading of requests, which needs no host: whole requests of
/// each verb, one arriving in pieces, two in one buffer, and the refusals.
///
/// # Errors
///
/// `InternalError` naming the first case that answered wrongly.
pub fn self_test() -> KernelResult<()> {
    let fail = |what: &str| -> KernelResult<()> {
        serial_println!("{}   FAIL: {}", LOG_TAG, what);
        Err(KernelError::InternalError)
    };
    let put = b"put 755 4 3\n/a/bxyz";
    let expected = Request::Put {
        mode: 0o755,
        path: b"/a/b".to_vec(),
        data: b"xyz".to_vec(),
    };
    if parse(put) != Parsed::Request(expected, put.len()) {
        return fail("a put");
    }
    // In pieces: incomplete until the last byte.
    for cut in 0..put.len() {
        if parse(put.get(..cut).unwrap_or_default()) != Parsed::Incomplete {
            serial_println!("{}     cut at {}", LOG_TAG, cut);
            return fail("a put arriving in pieces read as more than incomplete");
        }
    }
    let mut two = b"ping\n".to_vec();
    two.extend_from_slice(b"get 2\n/x");
    if parse(&two) != Parsed::Request(Request::Ping, 5) {
        return fail("a ping followed by another request");
    }
    let run = b"run 30 4 2 9 2\nfile/bin/true-v";
    let expected = Request::Run {
        seconds: 30,
        grants: b"file".to_vec(),
        argv: alloc::vec![b"/bin/true".to_vec(), b"-v".to_vec()],
    };
    if parse(run) != Parsed::Request(expected, run.len()) {
        return fail("a run");
    }
    let sh = b"sh 2\nls";
    if parse(sh)
        != Parsed::Request(
            Request::Sh {
                command: b"ls".to_vec(),
            },
            sh.len(),
        )
    {
        return fail("a sh");
    }
    // A path holding any byte, newlines included: lengths frame it.
    let odd = b"get 3\n\n\xFF\n";
    if parse(odd)
        != Parsed::Request(
            Request::Get {
                path: b"\n\xFF\n".to_vec(),
            },
            odd.len(),
        )
    {
        return fail("a path holding newlines");
    }
    let refused: [&[u8]; 7] = [
        b"jump 1\n",
        b"get\n",
        b"get x\n",
        b"put 9 1 1\nab",
        b"run 30 1 2 1\n-a",
        b"run 99999 1 1 1\n-a",
        b"get 99999999999\n",
    ];
    for bad in refused {
        if !matches!(parse(bad), Parsed::Invalid(_)) {
            serial_println!("{}     accepted \"{}\"", LOG_TAG, bad.escape_ascii());
            return fail("a malformed request was read");
        }
    }
    let long = alloc::vec![b'a'; MAX_LINE.saturating_add(1)];
    if !matches!(parse(&long), Parsed::Invalid(_)) {
        return fail("an endless request line was waited on");
    }
    if grants_of(b"-").map(|g| g.len()) != Ok(0)
        || grants_of(b"file").map(|g| g.len()) != Ok(1)
        || grants_of(b"file,nonesuch").is_ok()
    {
        return fail("grant words");
    }
    serial_println!(
        "{}   requests whole, in pieces and back to back; any byte in a field; refusals; grants: OK",
        LOG_TAG
    );
    Ok(())
}
