//! A program's lifecycle endpoint: how it is asked to close or to stop.
//!
//! SlateOS stops programs by message, not by Unix signal (`design.txt`; the
//! roadmap's "Structured shutdown via IPC message"). A program that can be
//! asked registers once ([`register`], `SYS_LIFECYCLE_REGISTER`) and gets the
//! end of a channel on which the kernel brings it every request made of it.
//! Anyone who may signal the program may ask ([`request`],
//! `SYS_LIFECYCLE_REQUEST`) -- the same rule, `check_signal_target`'s -- and
//! the request is one message on that channel.
//!
//! Two requests, the operator's design for `kill` and the task manager
//! (design-decisions §1072, answering B-Q22):
//!
//! | Request | The program should |
//! |---|---|
//! | [`CLOSE`] | close as if the user closed it: it may ask about unsaved work, and may decline |
//! | [`TERMINATE`] | stop now, cleanly -- flush, release, exit -- and ask nothing |
//!
//! The third way, *force*, is not a request: the kernel ends the program
//! (`SYS_PROCESS_KILL`, `SIGKILL`), whatever it would answer. A program with
//! no endpoint answers neither request ([`request`] says `NotSupported`), and
//! the caller does what it would without one: `kill` sends `SIGTERM`, which a
//! Linux program answers.
//!
//! The endpoint is the program image's: [`on_exec`] ends it, so a new image
//! that never registered is not sent requests it cannot read, and a fork's
//! child has none until it registers. It goes when the process does
//! ([`on_process_exit`]), and when the program closes its end -- the next
//! request then finds no one, and answers as if it had never registered.
//!
//! Nothing comes back on the channel: the asker learns the outcome by the
//! program's exit (or its staying), as `kill --timeout` watches for it before
//! it escalates (lane B's `requests/a-b-the-lifecycle-endpoint.md`).
//!
//! The model is Fuchsia's `fuchsia.process.lifecycle` (`PA_LIFECYCLE`, a
//! channel each component is started with, on which it is told `Stop`), less
//! the start-time handle: here the program registers, since programs start
//! many ways and only the program knows whether it answers.

use alloc::collections::BTreeMap;
use core::sync::atomic::{AtomicU64, Ordering};

use crate::cap::ResourceType;
use crate::error::{KernelError, KernelResult};
use crate::ipc::channel::{self, ChannelHandle};
use crate::proc::pcb::{self, ProcessId};
use crate::serial_println;
use crate::sync::PreemptSpinMutex as Mutex;

/// Request: close as the user would -- the program may ask about unsaved
/// work, and may decline.
pub const CLOSE: u32 = 1;
/// Request: stop now, cleanly, asking nothing.
pub const TERMINATE: u32 = 2;

/// Bytes of a request message: `u32` kind, `u32` flags (0), `u64` request
/// id, `u32` asker's process id, `u32` asker's real uid; little-endian.
pub const MESSAGE_LEN: usize = 24;

/// A registered program's channel: the end the kernel sends on, and the
/// program's, which `fork` does not copy ([`is_program_end`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Endpoint {
    kernel: ChannelHandle,
    program: ChannelHandle,
}

/// Every registered program's endpoint, by process. A leaf lock: nothing is
/// taken while it is held (the channel is sent on after it is let go).
static ENDPOINTS: Mutex<BTreeMap<ProcessId, Endpoint>> = Mutex::new(BTreeMap::new());

/// The next request's id: never 0, never reused.
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// One request, as its message carries it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Request {
    /// [`CLOSE`] or [`TERMINATE`].
    pub kind: u32,
    /// Its id, which the asker is answered with.
    pub id: u64,
    /// Who asked.
    pub asker_pid: u32,
    /// The asker's real uid.
    pub asker_uid: u32,
}

impl Request {
    /// The request as its [`MESSAGE_LEN`]-byte message.
    #[must_use]
    pub fn encode(&self) -> [u8; MESSAGE_LEN] {
        let mut out = [0u8; MESSAGE_LEN];
        let fields: [&[u8]; 5] = [
            &self.kind.to_le_bytes(),
            &0u32.to_le_bytes(),
            &self.id.to_le_bytes(),
            &self.asker_pid.to_le_bytes(),
            &self.asker_uid.to_le_bytes(),
        ];
        let mut at = 0usize;
        for field in fields {
            let end = at.saturating_add(field.len());
            if let Some(dst) = out.get_mut(at..end) {
                dst.copy_from_slice(field);
            }
            at = end;
        }
        out
    }

    /// A message's request; `None` when it is not one.
    #[must_use]
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != MESSAGE_LEN {
            return None;
        }
        let u32_at = |at: usize| -> Option<u32> {
            Some(u32::from_le_bytes(
                bytes.get(at..at.checked_add(4)?)?.try_into().ok()?,
            ))
        };
        let id: [u8; 8] = bytes.get(8..16)?.try_into().ok()?;
        let kind = u32_at(0)?;
        (kind == CLOSE || kind == TERMINATE).then_some(Self {
            kind,
            id: u64::from_le_bytes(id),
            asker_pid: u32_at(16)?,
            asker_uid: u32_at(20)?,
        })
    }
}

/// Make `pid` answer lifecycle requests: returns the end of a new channel the
/// kernel brings them on, registered as `pid`'s, so it closes with it.
///
/// # Errors
///
/// `AlreadyExists` when `pid` has an endpoint still open; `InvalidArgument`
/// for pid 0 (a kernel task: nothing to ask).
pub fn register(pid: ProcessId) -> KernelResult<ChannelHandle> {
    if pid == 0 {
        return Err(KernelError::InvalidArgument);
    }
    let (kernel_end, program_end) = channel::create();
    let new = Endpoint {
        kernel: kernel_end,
        program: program_end,
    };
    loop {
        // An endpoint whose program end is closed is gone already, and may
        // be replaced. Asked with the registry let go: it is a leaf lock.
        let existing = ENDPOINTS.lock().get(&pid).copied();
        if existing.is_some_and(|e| !channel::peer_closed(e.kernel)) {
            channel::close(kernel_end);
            channel::close(program_end);
            return Err(KernelError::AlreadyExists);
        }
        let replaced = {
            let mut endpoints = ENDPOINTS.lock();
            // Changed since the look -- registered by another, or a stale one
            // forgotten by a request or an exec: look again.
            (endpoints.get(&pid).copied() == existing).then(|| endpoints.insert(pid, new))
        };
        if let Some(replaced) = replaced {
            if let Some(old) = replaced {
                channel::close(old.kernel);
            }
            break;
        }
    }
    pcb::register_ipc_handle(pid, ResourceType::Channel, program_end.raw());
    Ok(program_end)
}

/// Whether the channel end `raw` is a registered program's own end of its
/// endpoint: `fork` leaves it out of the child, so that one endpoint stays
/// one program's (design-decisions 1569).
#[must_use]
pub fn is_program_end(raw: u64) -> bool {
    ENDPOINTS
        .lock()
        .values()
        .any(|e| e.program.raw() == raw)
}

/// Whether `target` answers lifecycle requests: it registered, and has not
/// closed its end.
#[must_use]
pub fn answers(target: ProcessId) -> bool {
    let end = ENDPOINTS.lock().get(&target).copied();
    end.is_some_and(|end| !channel::peer_closed(end.kernel))
}

/// Ask `target` to close or terminate, as `asker` (real uid `asker_uid`).
/// The caller has checked that `asker` may signal `target`.
///
/// Returns the request's id.
///
/// # Errors
///
/// - `InvalidArgument`: `kind` is neither [`CLOSE`] nor [`TERMINATE`].
/// - `NotSupported`: `target` has no endpoint -- it never registered, it
///   exec'd since, or it closed its end. Ask it some other way (`SIGTERM`).
/// - `ChannelFull`: requests already fill its queue, unread.
pub fn request(
    target: ProcessId,
    kind: u32,
    asker: ProcessId,
    asker_uid: u32,
) -> KernelResult<u64> {
    if kind != CLOSE && kind != TERMINATE {
        return Err(KernelError::InvalidArgument);
    }
    let end = ENDPOINTS
        .lock()
        .get(&target)
        .copied()
        .ok_or(KernelError::NotSupported)?;
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let message = Request {
        kind,
        id,
        asker_pid: u32::try_from(asker).unwrap_or(u32::MAX),
        asker_uid,
    }
    .encode();
    match channel::send(end.kernel, channel::Message::from_bytes(&message)?) {
        Ok(()) => Ok(id),
        // The program closed its end: as if it had never registered, and
        // forgotten, so the next asker is answered without a send.
        Err(KernelError::ChannelClosed | KernelError::InvalidHandle) => {
            forget(target, end);
            Err(KernelError::NotSupported)
        }
        Err(e) => Err(e),
    }
}

/// Drop `pid`'s endpoint if it is still `end` (a newer registration stays).
fn forget(pid: ProcessId, end: Endpoint) {
    let gone = {
        let mut endpoints = ENDPOINTS.lock();
        if endpoints.get(&pid) == Some(&end) {
            endpoints.remove(&pid)
        } else {
            None
        }
    };
    if let Some(end) = gone {
        channel::close(end.kernel);
    }
}

/// `pid` exec'd: the new image has not registered, so the old image's
/// endpoint ends.
pub fn on_exec(pid: ProcessId) {
    let gone = ENDPOINTS.lock().remove(&pid);
    if let Some(end) = gone {
        channel::close(end.kernel);
    }
}

/// `pid` is gone: its endpoint with it.
pub fn on_process_exit(pid: ProcessId) {
    on_exec(pid);
}

/// The registry, in-kernel: a program's end gets a request's message, in
/// order and in the documented layout; a second registration is refused
/// while the first is open; a closed end, an exec and an exit each end the
/// endpoint, after which a request answers `NotSupported`; and a request of
/// no known kind is refused.
///
/// Runs against a process id no process holds, and gives back everything it
/// made.
///
/// # Errors
///
/// `InternalError` naming what failed.
pub fn self_test() -> KernelResult<()> {
    fn fail(what: &str) -> KernelResult<()> {
        serial_println!("[lifecycle]   FAIL: {}", what);
        Err(KernelError::InternalError)
    }
    serial_println!("[lifecycle] Running self-test...");
    // A pid far past any the boot reaches; register_ipc_handle on it records
    // nothing, since no process has it.
    let pid: ProcessId = u64::from(u32::MAX) - 7;

    let end = register(pid)?;
    // fork leaves out the program's end, and only that.
    let program_end_known = is_program_end(end.raw());
    let second = register(pid);
    let first_id = request(pid, CLOSE, 1, 1000)?;
    let second_id = request(pid, TERMINATE, 2, 0)?;
    let bad_kind = request(pid, 3, 1, 0);
    let got_close = channel::try_recv(end)?.and_then(|m| Request::decode(m.data()));
    let got_term = channel::try_recv(end)?.and_then(|m| Request::decode(m.data()));
    let nothing_more = channel::try_recv(end)?.is_none();

    // Closing the program's end ends the endpoint.
    channel::close(end);
    let after_close = request(pid, TERMINATE, 1, 0);
    let after_close_answers = answers(pid);
    // Registering again works once the old end is closed; exec ends it.
    let again = register(pid)?;
    let answers_again = answers(pid);
    on_exec(pid);
    let after_exec = request(pid, CLOSE, 1, 0);
    let forgotten_end = !is_program_end(again.raw());
    channel::close(again);
    // And exit.
    let third = register(pid)?;
    on_process_exit(pid);
    let after_exit = request(pid, CLOSE, 1, 0);
    channel::close(third);

    let checks = [
        (
            "the program's end is known as one, for fork to leave out, until it ends",
            program_end_known && forgotten_end,
        ),
        (
            "a second registration is refused while the first is open",
            second.as_ref().err() == Some(&KernelError::AlreadyExists),
        ),
        ("ids increase", second_id > first_id),
        (
            "the close request arrives first, as sent",
            got_close
                == Some(Request {
                    kind: CLOSE,
                    id: first_id,
                    asker_pid: 1,
                    asker_uid: 1000,
                }),
        ),
        (
            "the terminate request arrives second",
            got_term
                == Some(Request {
                    kind: TERMINATE,
                    id: second_id,
                    asker_pid: 2,
                    asker_uid: 0,
                }),
        ),
        (
            "a request of no known kind is refused, and not sent",
            bad_kind == Err(KernelError::InvalidArgument) && nothing_more,
        ),
        (
            "a closed end answers no more",
            after_close == Err(KernelError::NotSupported) && !after_close_answers,
        ),
        ("a new registration after the close answers", answers_again),
        (
            "exec ends the endpoint",
            after_exec == Err(KernelError::NotSupported),
        ),
        (
            "exit ends the endpoint",
            after_exit == Err(KernelError::NotSupported),
        ),
        (
            "a message of no known kind is not a request",
            Request::decode(&[0u8; MESSAGE_LEN]).is_none(),
        ),
    ];
    if let Some((what, _)) = checks.iter().find(|(_, ok)| !ok) {
        return fail(what);
    }
    serial_println!(
        "[lifecycle]   requests arrive in order as documented; one endpoint at a time; a closed \
         end, exec and exit each end it: OK"
    );
    serial_println!("[lifecycle] Self-test PASSED");
    Ok(())
}
