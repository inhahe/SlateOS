//! Capability request broker -- a program asks the user for authority it
//! does not hold, and one trusted program answers on the user's behalf.
//!
//! design.txt: "We should have a way for programs to lack certain
//! capabilities by default, and be able to ask the user for them when they
//! install or when they run", and "When the app asks a user to grant it some
//! capability, it should pass a parameter to the request that's passed along
//! to the user security dialog that tells the user what that particular
//! capability is being asked for." How it is built: design-decisions 1548.
//!
//! ## A request
//!
//! A type, an object and rights -- `(Process, 1234, DEBUG)` is "debug process
//! 1234", `(SystemClock, 0, WRITE)` "set the clock" -- and a reason shown to
//! the user. The object is the capability's `resource_id`: a pid, a thread, a
//! port, an interrupt line, or 0 for the whole class. Only *authority* can be
//! asked for ([`requestable`]): a handle -- a channel, a pipe, a terminal end
//! -- is had by making or receiving it, never by asking, and the right to
//! answer requests ([`ResourceType::CapBroker`]) is never asked for. A file is
//! not asked for one at a time: a program gets the file the user chose from
//! the file chooser (design-decisions 1415); asking for `File` is asking for
//! every file, and the dialog must say so.
//!
//! ## Flow
//!
//! 1. The asker files it (`SYS_CAP_REQUEST_FOR`; `SYS_CAP_REQUEST` for a whole
//!    class) -- [`request_capability`].
//! 2. The broker queues it and tells the handler: a message on the channel the
//!    handler got when it registered ([`EVENT_NEW`], with the request's
//!    [record](RECORD_HEADER_LEN)).
//! 3. The handler asks the user and answers (`SYS_CAP_REQUEST_DECIDE`,
//!    [`decide`]): allow puts the capability in the asker's table in the same
//!    step, so a request never reads Approved while nothing is held; deny
//!    refuses.
//! 4. The asker learns the answer by waiting (`SYS_CAP_REQUEST_WAIT`,
//!    [`wait`]) or asking (`SYS_CAP_REQUEST_STATUS`).
//!
//! A request also ends unanswered: it times out ([`REQUEST_TIMEOUT_MS`]), its
//! asker cancels it or exits, or the handler goes away. Every request the
//! handler was told of ends with one [`EVENT_ENDED`] carrying its final
//! status -- decided ones too, so the protocol is one `NEW` and one `ENDED`
//! per request -- and a prompt for a question that no longer stands comes
//! down.
//!
//! ## Who answers
//!
//! One handler at a time: a process holding `(CapBroker, 0, WRITE)` that
//! registered ([`register_process_handler`]), or the kernel shell's console
//! (`capreq handler on`, a debugging aid, which a process handler replaces).
//! With neither, a request is refused as it is filed: fail safe. The
//! handler's exit or unregistration refuses what was pending. A handler
//! cannot answer its own requests.
//!
//! ## Time
//!
//! A request not answered within [`REQUEST_TIMEOUT_MS`] ends `TimedOut`.
//! Expiry is checked whenever the broker is used and when an asker's wait
//! reaches the deadline; a record carries the time left, so a dialog takes
//! its prompt down on time even if nothing else touches the broker.
//!
//! ## Locks
//!
//! `REQUESTS` is taken before the process table (a grant happens under it, so
//! no cancel or timeout can land between the grant and its record) and never
//! while holding it. `HANDLER`, `LOST` and `WAITERS` are leaves. Events are
//! built under `REQUESTS` and sent on the channel after it is released.

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use crate::cap::{ResourceType, Rights};
use crate::error::{KernelError, KernelResult};
use crate::ipc::channel::{self, ChannelHandle};
use crate::ipc::waiters::{self, Patience, WaiterSet};
use crate::serial_println;

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Maximum pending requests in the system.
const MAX_PENDING_REQUESTS: usize = 32;

/// Maximum pending requests per process.
const MAX_PENDING_PER_PROCESS: usize = 4;

/// Maximum length of the reason string, in bytes. Refused above it at the
/// system call, not cut: a reason cut mid-sentence can read as more innocuous
/// than it is.
pub const MAX_REASON_LEN: usize = 256;

/// Longest program or object name a record carries, in bytes (cut on a
/// character boundary; a name is the kernel's own, not the asker's words).
const MAX_NAME_LEN: usize = 255;

/// How long a request waits for an answer, in milliseconds.
pub const REQUEST_TIMEOUT_MS: u64 = 30_000;

/// Ended requests kept for the audit trail (`capreq all`), oldest dropped
/// first.
const MAX_ENDED_KEPT: usize = 64;

// ---------------------------------------------------------------------------
// The handler's events: the wire format
// ---------------------------------------------------------------------------

/// Event kind: a new request, its record after the header.
pub const EVENT_NEW: u32 = 1;
/// Event kind: a request ended; the header's status word is how.
pub const EVENT_ENDED: u32 = 2;
/// Event kind: events were lost (the handler's queue was full); the handler
/// should read the whole list again (`SYS_CAP_REQUEST_LIST`).
pub const EVENT_LOST: u32 = 3;

/// Length of an event's header:
///
/// ```text
/// offset size
///  0     4    kind (EVENT_*)
///  4     4    status: EVENT_ENDED's final status (RequestStatus::code), else 0
///  8     8    request id (0 for EVENT_LOST)
/// 16          EVENT_NEW: the request's record
/// ```
pub const EVENT_HEADER_LEN: usize = 16;

/// Length of a record's fixed part. A record -- one in `SYS_CAP_REQUEST_LIST`'s
/// answer, or after an `EVENT_NEW` header -- is little-endian:
///
/// ```text
/// offset size
///  0     8    request id
///  8     8    asker's pid
/// 16     8    resource id: the object (a pid, a port...), 0 for the class
/// 24     8    rights (Rights bits)
/// 32     8    milliseconds left before it times out
/// 40     2    resource type (ResourceType discriminant)
/// 42     2    asker's name length
/// 44     2    object's name length (a process's name for a Process,
///             Thread or ResourceLimit object; 0 for none)
/// 46     2    reason length
/// 48          asker's name, object's name, reason (UTF-8, unterminated),
///             then zeros to a multiple of 8
/// ```
pub const RECORD_HEADER_LEN: usize = 48;

// ---------------------------------------------------------------------------
// Request data structures
// ---------------------------------------------------------------------------

/// Unique identifier for a capability request.
pub type RequestId = u64;

/// Status of a capability request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestStatus {
    /// Waiting for an answer.
    Pending,
    /// Allowed: the capability is in the asker's table.
    Approved,
    /// Refused -- by the handler, or because there was none.
    Denied,
    /// Not answered in time.
    TimedOut,
    /// Cancelled by the asker, or the asker is gone.
    Cancelled,
}

impl RequestStatus {
    /// The status as the system calls report it.
    #[must_use]
    pub const fn code(self) -> u32 {
        match self {
            Self::Pending => 0,
            Self::Approved => 1,
            Self::Denied => 2,
            Self::TimedOut => 3,
            Self::Cancelled => 4,
        }
    }
}

/// A capability request from a process.
#[derive(Debug, Clone)]
pub struct CapRequest {
    /// Unique request identifier.
    pub id: RequestId,
    /// PID of the requesting process.
    pub pid: u64,
    /// Name of the requesting process (for display).
    pub process_name: String,
    /// Resource type being requested.
    pub resource_type: ResourceType,
    /// The object: its `resource_id`, 0 for the whole class.
    pub resource_id: u64,
    /// Rights being requested.
    pub rights: Rights,
    /// The object's own name where it has one -- the target process's for a
    /// `Process`, `Thread` or `ResourceLimit` object -- else empty.
    pub target_name: String,
    /// Human-readable reason for the request.
    pub reason: String,
    /// Current status.
    pub status: RequestStatus,
    /// When the request was filed (monotonic ms).
    pub created_at_ms: u64,
    /// When it ended (0 while pending).
    pub resolved_at_ms: u64,
    /// Whether the handler was told of it, so its end is told too.
    told: bool,
}

/// Who may answer requests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Handler {
    /// Nobody: requests are refused as they are filed.
    None,
    /// The kernel shell's console (`capreq handler on`).
    Console,
    /// A process, told of requests on its channel; the kernel sends on `end`.
    Process { pid: u64, end: ChannelHandle },
}

/// Who decides a request, for [`decide`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decider {
    /// The registered handler process, by pid.
    Handler(u64),
    /// The kernel shell's console: whoever types there has the kernel's
    /// authority already.
    Console,
}

// ---------------------------------------------------------------------------
// Global state
// ---------------------------------------------------------------------------

/// Next request ID counter.
static NEXT_REQUEST_ID: AtomicU64 = AtomicU64::new(1);

/// Every request, pending and ended (the audit trail).
static REQUESTS: crate::sync::Mutex<Vec<CapRequest>> =
    crate::sync::Mutex::named(Vec::new(), b"capreq");

/// Who answers.
static HANDLER: crate::sync::PreemptSpinMutex<Handler> =
    crate::sync::PreemptSpinMutex::new(Handler::None);

/// Set when an event could not be sent (the handler's queue was full), so
/// the next one is preceded by [`EVENT_LOST`].
static LOST: AtomicBool = AtomicBool::new(false);

/// Askers blocked in [`wait`]; every end wakes them all, and each looks at
/// its own request.
static WAITERS: crate::sync::PreemptSpinMutex<WaiterSet> =
    crate::sync::PreemptSpinMutex::new(WaiterSet::new());

/// Monotonic milliseconds, on the clock the waits' timers use.
fn now_ms() -> u64 {
    crate::hrtimer::now_ns() / 1_000_000
}

/// `s` cut to at most `max` bytes on a character boundary.
fn truncate_utf8(s: &str, max: usize) -> String {
    if s.len() <= max {
        return String::from(s);
    }
    // `floor_char_boundary` is still unstable: walk back from the limit past
    // any UTF-8 continuation bytes. The cut never panics (a byte index inside
    // a character would), which matters because `s` may come from userspace.
    let mut end = max;
    while end > 0 && !s.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    s.get(..end).map_or_else(String::new, String::from)
}

// ---------------------------------------------------------------------------
// What may be asked for
// ---------------------------------------------------------------------------

/// Whether `(resource_type, resource_id)` may be asked for: `Ok` for
/// authority over an object or a class, `InvalidArgument` otherwise.
///
/// - Authority with an object: a process, thread, I/O port, interrupt line,
///   reserved port or a process's limits -- by id, or 0 for every one.
/// - Authority over a class only (`resource_id` 0): files, the network,
///   timers, real-time I/O, registering services, namespaces, raw network
///   access, the clock, input devices, disks. Their ids are reserved or name
///   handles, so a non-zero one names nothing a user could judge.
/// - Never: handles (had by making or receiving them -- a channel, a pipe,
///   another process's terminal end would be a keystroke injector), and the
///   right to answer requests itself.
///
/// Exhaustive on purpose: a new type cannot be added without deciding here
/// whether a user may be asked for it.
pub fn requestable(resource_type: ResourceType, resource_id: u64) -> KernelResult<()> {
    use ResourceType as T;
    match resource_type {
        T::Process
        | T::Thread
        | T::PortIo
        | T::DeviceIrq
        | T::PrivilegedPort
        | T::ResourceLimit => Ok(()),
        T::File
        | T::Socket
        | T::Timer
        | T::IoScheduler
        | T::Service
        | T::Namespace
        | T::NetRaw
        | T::SystemClock
        | T::InputDevice
        | T::BlockDevice => {
            if resource_id == 0 {
                Ok(())
            } else {
                Err(KernelError::InvalidArgument)
            }
        }
        T::Channel
        | T::Pipe
        | T::SharedMemory
        | T::EventFd
        | T::CompletionPort
        | T::StreamSocket
        | T::MemFd
        | T::Epoll
        | T::SignalFd
        | T::Timerfd
        | T::Inotify
        | T::AlsaPcm
        | T::Drm
        | T::NetSocket
        | T::Pty
        | T::Semaphore
        | T::UnixSocket
        | T::NativeSocket
        | T::CapBroker => Err(KernelError::InvalidArgument),
    }
}

/// The name of the object a request names, where it has one: the process's
/// for a `Process` or `ResourceLimit` id, the owning process's for a `Thread`
/// id. `NoSuchProcess` for an id naming no live process or thread, so nobody
/// is asked about a process that is not there.
fn target_name(resource_type: ResourceType, resource_id: u64) -> KernelResult<String> {
    if resource_id == 0 {
        return Ok(String::new());
    }
    let name = match resource_type {
        ResourceType::Process | ResourceType::ResourceLimit => {
            crate::proc::pcb::name(resource_id).ok_or(KernelError::NoSuchProcess)?
        }
        ResourceType::Thread => crate::proc::thread::owner_process(resource_id)
            .and_then(crate::proc::pcb::name)
            .ok_or(KernelError::NoSuchProcess)?,
        _ => return Ok(String::new()),
    };
    Ok(truncate_utf8(&name, MAX_NAME_LEN))
}

// ---------------------------------------------------------------------------
// Records and events
// ---------------------------------------------------------------------------

/// Append `bytes` (a field already in its little-endian form).
fn put(out: &mut Vec<u8>, bytes: &[u8]) {
    out.extend_from_slice(bytes);
}

/// `len` as a record's 16-bit length (every string is bounded well below).
fn len16(len: usize) -> [u8; 2] {
    u16::try_from(len).unwrap_or(u16::MAX).to_le_bytes()
}

/// Append `req`'s record (the layout at [`RECORD_HEADER_LEN`]), its time left
/// measured from `now`.
fn put_record(out: &mut Vec<u8>, req: &CapRequest, now: u64) {
    let start = out.len();
    let left = req
        .created_at_ms
        .saturating_add(REQUEST_TIMEOUT_MS)
        .saturating_sub(now);
    put(out, &req.id.to_le_bytes());
    put(out, &req.pid.to_le_bytes());
    put(out, &req.resource_id.to_le_bytes());
    put(out, &req.rights.raw().to_le_bytes());
    put(out, &left.to_le_bytes());
    put(out, &req.resource_type.discriminant().to_le_bytes());
    put(out, &len16(req.process_name.len()));
    put(out, &len16(req.target_name.len()));
    put(out, &len16(req.reason.len()));
    put(out, req.process_name.as_bytes());
    put(out, req.target_name.as_bytes());
    put(out, req.reason.as_bytes());
    while !out.len().saturating_sub(start).is_multiple_of(8) {
        out.push(0);
    }
}

/// An event's header (the layout at [`EVENT_HEADER_LEN`]).
fn event_header(kind: u32, status: u32, id: RequestId) -> Vec<u8> {
    let mut out = Vec::new();
    put(&mut out, &kind.to_le_bytes());
    put(&mut out, &status.to_le_bytes());
    put(&mut out, &id.to_le_bytes());
    out
}

/// The [`EVENT_NEW`] for `req`.
fn new_event(req: &CapRequest, now: u64) -> Vec<u8> {
    let mut out = event_header(EVENT_NEW, 0, req.id);
    put_record(&mut out, req, now);
    out
}

/// The [`EVENT_ENDED`] for `req`, ended with its present status.
fn ended_event(req: &CapRequest) -> Vec<u8> {
    event_header(EVENT_ENDED, req.status.code(), req.id)
}

/// Send `events` to the handler process, if there is one; an event that does
/// not fit (the handler's queue is full) is dropped and the next sent one is
/// preceded by [`EVENT_LOST`]. Called with no lock held.
fn post(events: Vec<Vec<u8>>) {
    if events.is_empty() {
        return;
    }
    let Handler::Process { end, .. } = *HANDLER.lock() else {
        return;
    };
    for event in events {
        if LOST.load(Ordering::Acquire) {
            if send(end, &event_header(EVENT_LOST, 0, 0)).is_err() {
                continue;
            }
            LOST.store(false, Ordering::Release);
        }
        if send(end, &event).is_err() {
            LOST.store(true, Ordering::Release);
        }
    }
}

/// Send one message on the kernel's end of the handler's channel. Any
/// failure -- a full queue, or a handler that closed its end or is
/// unregistering -- is the caller's to count as lost.
fn send(end: ChannelHandle, bytes: &[u8]) -> KernelResult<()> {
    channel::send(end, channel::Message::from_bytes(bytes)?)
}

/// Wake every blocked asker; each re-checks its own request.
fn wake_askers() {
    let tasks = WAITERS.lock().take_all();
    waiters::wake_all(tasks);
}

/// End every pending request in `requests` that has outlived
/// [`REQUEST_TIMEOUT_MS`] at `now`, returning their events.
fn expire_locked(requests: &mut [CapRequest], now: u64) -> Vec<Vec<u8>> {
    let mut events = Vec::new();
    for req in requests.iter_mut() {
        if req.status == RequestStatus::Pending
            && now.saturating_sub(req.created_at_ms) >= REQUEST_TIMEOUT_MS
        {
            req.status = RequestStatus::TimedOut;
            req.resolved_at_ms = now;
            serial_println!(
                "[cap-request] #{}: timed out after {} ms",
                req.id,
                REQUEST_TIMEOUT_MS
            );
            if req.told {
                events.push(ended_event(req));
            }
        }
    }
    events
}

/// Drop the oldest ended requests past [`MAX_ENDED_KEPT`].
fn trim_ended_locked(requests: &mut Vec<CapRequest>) {
    let ended = requests
        .iter()
        .filter(|r| r.status != RequestStatus::Pending)
        .count();
    let mut excess = ended.saturating_sub(MAX_ENDED_KEPT);
    if excess > 0 {
        requests.retain(|r| {
            if excess > 0 && r.status != RequestStatus::Pending {
                excess = excess.saturating_sub(1);
                false
            } else {
                true
            }
        });
    }
}

/// End what has timed out, tell the handler and wake the askers.
pub fn expire_timeouts() {
    expire_at(now_ms());
}

/// [`expire_timeouts`] as though it were `now`: the self-test's clock.
fn expire_at(now: u64) {
    let events = expire_locked(&mut REQUESTS.lock(), now);
    if !events.is_empty() {
        post(events);
        wake_askers();
    }
}

// ---------------------------------------------------------------------------
// Asking
// ---------------------------------------------------------------------------

/// File a request: process `pid`, named `process_name`, asks for `rights` on
/// `(resource_type, resource_id)` for `reason`. Returns its id. A request
/// filed with no handler to answer it is refused at once: it exists, with
/// status `Denied`, so the asker reads why.
///
/// # Errors
///
/// `InvalidArgument` for an object that cannot be asked for
/// ([`requestable`]), no rights, or a bit that is no declared right;
/// `NoSuchProcess` for a process or thread
/// object that is not there; `ResourceExhausted` when the queue is full or
/// the process has `MAX_PENDING_PER_PROCESS` (4) pending already.
pub fn request_capability(
    pid: u64,
    process_name: &str,
    resource_type: ResourceType,
    resource_id: u64,
    rights: Rights,
    reason: &str,
) -> KernelResult<RequestId> {
    requestable(resource_type, resource_id)?;
    // No rights is no request; a bit that is no declared right would be
    // shown as nothing and granted as nothing -- until the day it is
    // declared, when what the user once allowed would quietly mean more.
    if rights.is_empty() || !Rights::DECLARED.contains(rights) {
        return Err(KernelError::InvalidArgument);
    }
    // Everything that allocates or takes another lock, before `REQUESTS`.
    let target = target_name(resource_type, resource_id)?;
    let process_name = truncate_utf8(process_name, MAX_NAME_LEN);
    let reason = truncate_utf8(reason, MAX_REASON_LEN);
    let now = now_ms();

    let req = CapRequest {
        id: 0,
        pid,
        process_name,
        resource_type,
        resource_id,
        rights,
        target_name: target,
        reason,
        status: RequestStatus::Pending,
        created_at_ms: now,
        resolved_at_ms: 0,
        told: false,
    };
    let mut events = Vec::new();
    let (result, expired) = {
        let mut requests = REQUESTS.lock();
        let timed_out = expire_locked(&mut requests, now);
        let expired = !timed_out.is_empty();
        events.extend(timed_out);
        (file_locked(&mut requests, req, now, &mut events), expired)
    };
    post(events);
    if expired {
        // Askers of the requests that just timed out wait for that news.
        wake_askers();
    }
    result
}

/// The body of [`request_capability`], under `REQUESTS`: give `req` an id
/// and queue it -- refused at once with no handler, told to a handler
/// process (its `EVENT_NEW` pushed onto `events`).
fn file_locked(
    requests: &mut Vec<CapRequest>,
    mut req: CapRequest,
    now: u64,
    events: &mut Vec<Vec<u8>>,
) -> KernelResult<RequestId> {
    let pending = requests
        .iter()
        .filter(|r| r.status == RequestStatus::Pending)
        .count();
    let own = requests
        .iter()
        .filter(|r| r.pid == req.pid && r.status == RequestStatus::Pending)
        .count();
    if pending >= MAX_PENDING_REQUESTS || own >= MAX_PENDING_PER_PROCESS {
        serial_println!(
            "[cap-request] pid {} refused: {} pending in all, {} of its own",
            req.pid,
            pending,
            own
        );
        return Err(KernelError::ResourceExhausted);
    }
    req.id = NEXT_REQUEST_ID.fetch_add(1, Ordering::Relaxed);
    serial_println!(
        "[cap-request] #{}: pid={} asks {:?} {} {:?} -- {:?}",
        req.id,
        req.pid,
        req.resource_type,
        req.resource_id,
        req.rights,
        req.reason
    );
    // Read under `REQUESTS` (`HANDLER` is a leaf below it), so a handler
    // registering or leaving cannot cross the filing: a request queued pending
    // is either told here or by `register_process_handler`'s scan, and is
    // refused by `unregister_process_handler`'s if its handler leaves. Copied
    // out at once: `HANDLER` is a spinlock, held across no allocation.
    let handler = *HANDLER.lock();
    match handler {
        Handler::None => {
            req.status = RequestStatus::Denied;
            req.resolved_at_ms = now;
            serial_println!("[cap-request] #{}: refused, no handler", req.id);
        }
        Handler::Console => {}
        Handler::Process { .. } => {
            req.told = true;
            events.push(new_event(&req, now));
        }
    }
    let id = req.id;
    requests.push(req);
    trim_ended_locked(requests);
    Ok(id)
}

/// Cancel request `id` -- only its asker, `pid`, may.
///
/// # Errors
///
/// `NotFound` if `pid` filed no such request; `InvalidArgument` if it has
/// ended.
pub fn cancel(request_id: RequestId, pid: u64) -> KernelResult<()> {
    let now = now_ms();
    let event = {
        let mut requests = REQUESTS.lock();
        let req = requests
            .iter_mut()
            .find(|r| r.id == request_id && r.pid == pid)
            .ok_or(KernelError::NotFound)?;
        if req.status != RequestStatus::Pending {
            return Err(KernelError::InvalidArgument);
        }
        req.status = RequestStatus::Cancelled;
        req.resolved_at_ms = now;
        req.told.then(|| ended_event(req))
    };
    post(event.into_iter().collect());
    wake_askers();
    Ok(())
}

/// The status of request `id` -- for its asker or the handler (`viewer`),
/// `None` for anyone else, as for no such request.
#[must_use]
pub fn status_for(request_id: RequestId, viewer: u64) -> Option<RequestStatus> {
    expire_timeouts();
    let handler = *HANDLER.lock();
    let requests = REQUESTS.lock();
    let req = requests.iter().find(|r| r.id == request_id)?;
    let is_handler = matches!(handler, Handler::Process { pid, .. } if pid == viewer);
    (req.pid == viewer || is_handler).then_some(req.status)
}

/// The status of request `id`, for the kernel's own use (the shell).
#[must_use]
pub fn get_status(request_id: RequestId) -> Option<RequestStatus> {
    REQUESTS
        .lock()
        .iter()
        .find(|r| r.id == request_id)
        .map(|r| r.status)
}

/// Wait until request `id`, filed by `pid`, ends, as long as `patience`
/// allows; return how it ended. The wait also ends at the request's own
/// deadline, when it times out.
///
/// # Errors
///
/// `NotFound` if `pid` filed no such request; `TimedOut` if `patience` ran
/// out first (the request goes on); `WouldBlock` for [`Patience::Never`] on
/// a pending request; `Interrupted` when a signal arrives.
pub fn wait(request_id: RequestId, pid: u64, patience: Patience) -> KernelResult<RequestStatus> {
    let task = crate::sched::current_task_id();
    // The request's own deadline bounds every wait: a waiter parked past it
    // would otherwise sleep until something else touched the broker.
    let left_ms = {
        let requests = REQUESTS.lock();
        let req = requests
            .iter()
            .find(|r| r.id == request_id && r.pid == pid)
            .ok_or(KernelError::NotFound)?;
        req.created_at_ms
            .saturating_add(REQUEST_TIMEOUT_MS)
            .saturating_sub(now_ms())
    };
    // One millisecond past the deadline, so the wake finds it expired.
    let own_ns = left_ms.saturating_add(1).saturating_mul(1_000_000);
    let (bounded, callers_own) = match patience {
        Patience::Never => (Patience::Never, true),
        Patience::Forever => (Patience::Upto(own_ns), false),
        Patience::Upto(ns) if ns <= own_ns => (Patience::Upto(ns), true),
        Patience::Upto(_) => (Patience::Upto(own_ns), false),
    };
    let mut deadline = waiters::Deadline::new();
    loop {
        WAITERS.lock().remove(task);
        expire_timeouts();
        let status = REQUESTS
            .lock()
            .iter()
            .find(|r| r.id == request_id && r.pid == pid)
            .map(|r| r.status)
            .ok_or(KernelError::NotFound)?;
        if status != RequestStatus::Pending {
            return Ok(status);
        }
        if waiters::deliverable_signal_pending(pid) {
            return Err(KernelError::Interrupted);
        }
        if let Some(e) = deadline.ends_now(bounded, task) {
            if !callers_own {
                // The request's own deadline, a millisecond past which it has
                // timed out: expire it and report that.
                expire_timeouts();
                let ended = REQUESTS
                    .lock()
                    .iter()
                    .find(|r| r.id == request_id)
                    .map(|r| r.status)
                    .filter(|&s| s != RequestStatus::Pending);
                if let Some(status) = ended {
                    return Ok(status);
                }
            }
            return Err(e);
        }
        WAITERS.lock().insert(task);
        // Re-check after registering, so an end between the check above and
        // the registration is not slept through.
        let still = REQUESTS
            .lock()
            .iter()
            .find(|r| r.id == request_id)
            .is_some_and(|r| r.status == RequestStatus::Pending);
        if still {
            waiters::park_interruptible(
                pid,
                task,
                crate::wchan::Wait::new(crate::wchan::WaitChannel::Event, request_id),
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Answering
// ---------------------------------------------------------------------------

/// Decide request `id`: `allow` grants the asker what it asked for, in the
/// same step that marks it Approved; otherwise it is Denied. Returns the
/// status it ended with -- `Cancelled` if the asker was gone by the time it
/// was allowed.
///
/// # Errors
///
/// - `PermissionDenied`: `decider` is not the registered handler (the
///   console may always decide), or the request is the handler's own.
/// - `NotFound`: no such request.
/// - `TimedOut`: it timed out before the answer came.
/// - `InvalidArgument`: it had already ended.
/// - The grant's own error (a full capability table): the request stays
///   pending, for the handler to refuse instead.
pub fn decide(request_id: RequestId, allow: bool, decider: Decider) -> KernelResult<RequestStatus> {
    let handler = *HANDLER.lock();
    let decider_pid = match (decider, handler) {
        (Decider::Console, _) => None,
        (Decider::Handler(p), Handler::Process { pid, .. }) if p == pid => Some(p),
        (Decider::Handler(_), _) => return Err(KernelError::PermissionDenied),
    };
    let now = now_ms();
    let mut events = Vec::new();
    let result = {
        let mut requests = REQUESTS.lock();
        // Whatever this call ends -- the request decided, or others that
        // timed out meanwhile -- is told and woken below, on every path.
        events.extend(expire_locked(&mut requests, now));
        decide_locked(
            &mut requests,
            request_id,
            allow,
            decider_pid,
            now,
            &mut events,
        )
    };
    post(events);
    wake_askers();
    result
}

/// The body of [`decide`], under `REQUESTS`: decide request `id` among
/// `requests` -- `decider_pid` the handler process deciding, `None` for the
/// console -- pushing its `EVENT_ENDED` onto `events`.
fn decide_locked(
    requests: &mut [CapRequest],
    request_id: RequestId,
    allow: bool,
    decider_pid: Option<u64>,
    now: u64,
    events: &mut Vec<Vec<u8>>,
) -> KernelResult<RequestStatus> {
    let req = requests
        .iter_mut()
        .find(|r| r.id == request_id)
        .ok_or(KernelError::NotFound)?;
    match req.status {
        RequestStatus::Pending => {}
        RequestStatus::TimedOut => return Err(KernelError::TimedOut),
        _ => return Err(KernelError::InvalidArgument),
    }
    if decider_pid == Some(req.pid) {
        // A handler answering its own request would hold whatever it asked
        // for by asking.
        return Err(KernelError::PermissionDenied);
    }
    let outcome = if allow {
        // Under `REQUESTS`, so no cancel, exit or timeout can end the request
        // between the grant and its record.
        match crate::proc::pcb::grant_capability(
            req.pid,
            req.resource_type,
            req.resource_id,
            req.rights,
        ) {
            Ok(_) => RequestStatus::Approved,
            Err(KernelError::NoSuchProcess) => RequestStatus::Cancelled,
            Err(e) => return Err(e),
        }
    } else {
        RequestStatus::Denied
    };
    req.status = outcome;
    req.resolved_at_ms = now;
    serial_println!(
        "[cap-request] #{}: {:?} (pid={}, {:?} {} {:?})",
        req.id,
        outcome,
        req.pid,
        req.resource_type,
        req.resource_id,
        req.rights
    );
    if req.told {
        events.push(ended_event(req));
    }
    Ok(outcome)
}

/// Approve request `id` from the kernel shell's console: grant it.
///
/// # Errors
///
/// As [`decide`].
pub fn approve(request_id: RequestId) -> KernelResult<RequestStatus> {
    decide(request_id, true, Decider::Console)
}

/// Deny request `id` from the kernel shell's console.
///
/// # Errors
///
/// As [`decide`].
pub fn deny(request_id: RequestId) -> KernelResult<()> {
    decide(request_id, false, Decider::Console).map(|_| ())
}

/// The records of every pending request, back to back -- what
/// `SYS_CAP_REQUEST_LIST` copies out (the layout at [`RECORD_HEADER_LEN`]).
#[must_use]
pub fn pending_records() -> Vec<u8> {
    expire_timeouts();
    let now = now_ms();
    let mut out = Vec::new();
    for req in REQUESTS
        .lock()
        .iter()
        .filter(|r| r.status == RequestStatus::Pending)
    {
        put_record(&mut out, req, now);
    }
    out
}

/// Whether `pid` is the registered handler process.
#[must_use]
pub fn is_handler(pid: u64) -> bool {
    matches!(*HANDLER.lock(), Handler::Process { pid: p, .. } if p == pid)
}

/// List all pending requests (the kernel shell's `capreq list`).
#[must_use]
pub fn list_pending() -> Vec<CapRequest> {
    expire_timeouts();
    REQUESTS
        .lock()
        .iter()
        .filter(|r| r.status == RequestStatus::Pending)
        .cloned()
        .collect()
}

/// List every request on record, ended ones included (`capreq all`).
#[must_use]
pub fn list_all() -> Vec<CapRequest> {
    REQUESTS.lock().clone()
}

/// Get the number of pending requests.
#[must_use]
pub fn pending_count() -> usize {
    REQUESTS
        .lock()
        .iter()
        .filter(|r| r.status == RequestStatus::Pending)
        .count()
}

// ---------------------------------------------------------------------------
// The handler
// ---------------------------------------------------------------------------

/// Make process `pid` the handler: returns the end of a new channel the
/// kernel will tell it of requests on (registered as `pid`'s, so its exit
/// closes it). The caller has checked `pid`'s `(CapBroker, 0, WRITE)`.
/// Replaces the console; requests pending under it carry on, and `pid` is
/// told of each.
///
/// # Errors
///
/// `AlreadyExists` if a process is the handler already (`pid` itself
/// included: one channel per handler).
pub fn register_process_handler(pid: u64) -> KernelResult<ChannelHandle> {
    let (kernel_end, handler_end) = channel::create();
    {
        let mut handler = HANDLER.lock();
        if matches!(*handler, Handler::Process { .. }) {
            drop(handler);
            channel::close(kernel_end);
            channel::close(handler_end);
            return Err(KernelError::AlreadyExists);
        }
        *handler = Handler::Process {
            pid,
            end: kernel_end,
        };
    }
    LOST.store(false, Ordering::Release);
    crate::proc::pcb::register_ipc_handle(pid, ResourceType::Channel, handler_end.raw());
    serial_println!("[cap-request] process {} is the handler", pid);
    // Requests the console was holding: tell the new handler of each -- once:
    // one filed meanwhile was told as it was filed.
    let now = now_ms();
    let events: Vec<Vec<u8>> = {
        let mut requests = REQUESTS.lock();
        requests
            .iter_mut()
            .filter(|r| r.status == RequestStatus::Pending && !r.told)
            .map(|r| {
                r.told = true;
                new_event(r, now)
            })
            .collect()
    };
    post(events);
    Ok(handler_end)
}

/// Process `pid` stops being the handler: every pending request is refused
/// and the kernel's end of its channel closed.
///
/// # Errors
///
/// `PermissionDenied` if `pid` is not the handler.
pub fn unregister_process_handler(pid: u64) -> KernelResult<()> {
    let end = {
        let mut handler = HANDLER.lock();
        match *handler {
            Handler::Process { pid: p, end } if p == pid => {
                *handler = Handler::None;
                end
            }
            _ => return Err(KernelError::PermissionDenied),
        }
    };
    channel::close(end);
    deny_all_pending("handler gone");
    serial_println!("[cap-request] process {} is no longer the handler", pid);
    Ok(())
}

/// Refuse every pending request (the handler went away) and wake the askers.
fn deny_all_pending(why: &str) {
    let now = now_ms();
    {
        let mut requests = REQUESTS.lock();
        for req in requests.iter_mut() {
            if req.status == RequestStatus::Pending {
                req.status = RequestStatus::Denied;
                req.resolved_at_ms = now;
                serial_println!("[cap-request] #{}: refused, {}", req.id, why);
            }
        }
    }
    wake_askers();
}

/// The kernel shell's console answers (`capreq handler on`), unless a
/// process does.
pub fn register_handler() {
    let mut handler = HANDLER.lock();
    if *handler == Handler::None {
        *handler = Handler::Console;
        serial_println!("[cap-request] the console is the handler");
    }
}

/// The console stops answering (`capreq handler off`): what was pending is
/// refused. A process handler is left alone.
pub fn unregister_handler() {
    let was_console = {
        let mut handler = HANDLER.lock();
        let was = *handler == Handler::Console;
        if was {
            *handler = Handler::None;
        }
        was
    };
    if was_console {
        deny_all_pending("console handler gone");
    }
}

/// Whether anyone answers requests.
#[must_use]
pub fn handler_active() -> bool {
    *HANDLER.lock() != Handler::None
}

/// Process `pid` is exiting, or exec'ing another program: its pending
/// requests are cancelled (the handler told) -- an exec's new program did not
/// ask them, and must not be handed what its predecessor asked for -- and if
/// it was the handler, it is not any more: a new program answers only once it
/// registers itself.
pub fn on_process_exit(pid: u64) {
    let now = now_ms();
    let events: Vec<Vec<u8>> = {
        let mut requests = REQUESTS.lock();
        requests
            .iter_mut()
            .filter(|r| r.pid == pid && r.status == RequestStatus::Pending)
            .filter_map(|r| {
                r.status = RequestStatus::Cancelled;
                r.resolved_at_ms = now;
                r.told.then(|| ended_event(r))
            })
            .collect()
    };
    if !events.is_empty() {
        post(events);
        wake_askers();
    }
    // Its own end of the channel is closed by the exit's handle cleanup; this
    // closes the kernel's.
    if is_handler(pid) {
        // `pid` is the handler, checked just above; a concurrent unregister
        // by the process itself cannot happen while it exits.
        let _ = unregister_process_handler(pid);
    }
}

/// Clear ended requests older than `max_age_ms` (all of them for 0).
pub fn gc(max_age_ms: u64) {
    let now = now_ms();
    REQUESTS.lock().retain(|r| {
        r.status == RequestStatus::Pending || now.saturating_sub(r.resolved_at_ms) < max_age_ms
    });
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Fail the self-test with `what` unless `cond`.
fn check(cond: bool, what: &str) -> KernelResult<()> {
    if cond {
        Ok(())
    } else {
        serial_println!("[cap-request]   FAIL: {}", what);
        Err(KernelError::InternalError)
    }
}

/// The next message on the handler's end, if any.
fn next_event(end: ChannelHandle) -> Option<Vec<u8>> {
    channel::try_recv(end)
        .ok()
        .flatten()
        .map(|m| m.data().to_vec())
}

/// An event's kind, status and request id.
fn event_parts(ev: &[u8]) -> Option<(u32, u32, u64)> {
    let kind = u32::from_le_bytes(ev.get(0..4)?.try_into().ok()?);
    let status = u32::from_le_bytes(ev.get(4..8)?.try_into().ok()?);
    let id = u64::from_le_bytes(ev.get(8..16)?.try_into().ok()?);
    Some((kind, status, id))
}

/// Run self-tests for the capability request broker: with real processes,
/// so a grant is seen in the asker's capability table.
pub fn self_test() -> KernelResult<()> {
    use crate::proc::pcb;
    serial_println!("[cap-request] Running self-test...");
    let asker = pcb::create("capreq-asker", 0);
    let handler_pid = pcb::create("capreq-handler", 0);
    let result = self_test_with(asker, handler_pid);
    // Whatever happened, leave no handler and no processes behind.
    let _ = unregister_process_handler(handler_pid);
    unregister_handler();
    pcb::destroy(asker);
    pcb::destroy(handler_pid);
    gc(0);
    if result.is_ok() {
        serial_println!("[cap-request] Self-test PASSED");
    }
    result
}

/// The body of [`self_test`].
#[allow(clippy::too_many_lines)]
fn self_test_with(asker: u64, handler_pid: u64) -> KernelResult<()> {
    use crate::proc::pcb;
    check(
        *HANDLER.lock() == Handler::None,
        "a handler was registered before the test",
    )?;

    // 1. No handler: refused as it is filed.
    let id = request_capability(
        asker,
        "asker",
        ResourceType::SystemClock,
        0,
        Rights::WRITE,
        "x",
    )?;
    check(
        get_status(id) == Some(RequestStatus::Denied),
        "no handler: not refused",
    )?;

    // 2. What cannot be asked for.
    let refused = [
        (ResourceType::Channel, 3),
        (ResourceType::Pty, 4),
        (ResourceType::CapBroker, 0),
        (ResourceType::File, 7),
    ];
    for (rt, rid) in refused {
        check(
            request_capability(asker, "asker", rt, rid, Rights::READ, "x")
                == Err(KernelError::InvalidArgument),
            "a handle, the broker or a numbered file could be asked for",
        )?;
    }
    check(
        request_capability(
            asker,
            "asker",
            ResourceType::SystemClock,
            0,
            Rights::NONE,
            "x",
        ) == Err(KernelError::InvalidArgument),
        "no rights could be asked for",
    )?;
    check(
        request_capability(
            asker,
            "asker",
            ResourceType::SystemClock,
            0,
            Rights::WRITE.union(Rights::from_raw(1 << 40)),
            "x",
        ) == Err(KernelError::InvalidArgument),
        "a bit that is no declared right could be asked for",
    )?;
    check(
        request_capability(
            asker,
            "asker",
            ResourceType::Process,
            0xFFFF_FFFF_FFFF_FFF0,
            Rights::DEBUG,
            "x",
        ) == Err(KernelError::NoSuchProcess),
        "a process that is not there could be asked about",
    )?;

    // 3. A process handler: told of a request, with its record.
    let end = register_process_handler(handler_pid)?;
    check(
        register_process_handler(asker) == Err(KernelError::AlreadyExists),
        "a second handler could register",
    )?;
    let id = request_capability(
        asker,
        "asker",
        ResourceType::Process,
        handler_pid,
        Rights::DEBUG,
        "debug it",
    )?;
    let ev = next_event(end).unwrap_or_default();
    check(
        event_parts(&ev) == Some((EVENT_NEW, 0, id)),
        "no EVENT_NEW for a request",
    )?;
    let rec = ev.get(EVENT_HEADER_LEN..).unwrap_or_default();
    let field = |at: usize| -> u64 {
        rec.get(at..at.saturating_add(8))
            .and_then(|b| b.try_into().ok())
            .map_or(0, u64::from_le_bytes)
    };
    let half = |at: usize| -> usize {
        rec.get(at..at.saturating_add(2))
            .and_then(|b| b.try_into().ok())
            .map_or(0, |b| usize::from(u16::from_le_bytes(b)))
    };
    check(
        field(0) == id
            && field(8) == asker
            && field(16) == handler_pid
            && field(24) == Rights::DEBUG.raw()
            && field(32) > 0
            && field(32) <= REQUEST_TIMEOUT_MS,
        "the record's id, asker, object, rights or time left",
    )?;
    let names = RECORD_HEADER_LEN;
    let (a, t, r) = (half(42), half(44), half(46));
    let text =
        |from: usize, len: usize| rec.get(from..from.saturating_add(len)).unwrap_or_default();
    check(
        half(40) == usize::from(ResourceType::Process.discriminant())
            && text(names, a) == b"asker"
            && text(names.saturating_add(a), t) == b"capreq-handler"
            && text(names.saturating_add(a).saturating_add(t), r) == b"debug it"
            && rec.len().is_multiple_of(8),
        "the record's type, names or reason",
    )?;
    check(
        pending_records().len() == rec.len(),
        "the list is not the one record",
    )?;

    // 4. The handler cannot answer its own request; another decider not at all.
    let own = request_capability(
        handler_pid,
        "handler",
        ResourceType::SystemClock,
        0,
        Rights::WRITE,
        "x",
    )?;
    let _ = next_event(end);
    check(
        decide(own, true, Decider::Handler(handler_pid)) == Err(KernelError::PermissionDenied),
        "a handler approved its own request",
    )?;
    check(
        decide(id, true, Decider::Handler(asker)) == Err(KernelError::PermissionDenied),
        "a process that is not the handler decided",
    )?;
    cancel(own, handler_pid)?;
    let ev = next_event(end).unwrap_or_default();
    check(
        event_parts(&ev) == Some((EVENT_ENDED, RequestStatus::Cancelled.code(), own)),
        "no EVENT_ENDED for a cancelled request",
    )?;

    // 5. Allow grants, in the same step.
    check(
        !pcb::has_capability_for(asker, ResourceType::Process, handler_pid, Rights::DEBUG),
        "the asker held the capability before the answer",
    )?;
    check(
        decide(id, true, Decider::Handler(handler_pid)) == Ok(RequestStatus::Approved),
        "allow did not approve",
    )?;
    check(
        pcb::has_capability_for(asker, ResourceType::Process, handler_pid, Rights::DEBUG),
        "allow did not grant",
    )?;
    check(
        decide(id, false, Decider::Handler(handler_pid)) == Err(KernelError::InvalidArgument),
        "an ended request was decided again",
    )?;
    let ev = next_event(end).unwrap_or_default();
    check(
        event_parts(&ev) == Some((EVENT_ENDED, RequestStatus::Approved.code(), id)),
        "no EVENT_ENDED for an approved request",
    )?;
    check(
        status_for(id, asker) == Some(RequestStatus::Approved)
            && status_for(id, handler_pid) == Some(RequestStatus::Approved)
            && status_for(id, 0xDEAD_BEEF).is_none(),
        "the status is not its asker's and handler's alone",
    )?;

    // 6. Deny refuses; nothing is granted.
    let id = request_capability(
        asker,
        "asker",
        ResourceType::PrivilegedPort,
        80,
        Rights::WRITE,
        "x",
    )?;
    let _ = next_event(end);
    check(
        decide(id, false, Decider::Handler(handler_pid)) == Ok(RequestStatus::Denied)
            && !pcb::has_capability_for(asker, ResourceType::PrivilegedPort, 80, Rights::WRITE),
        "deny granted, or did not deny",
    )?;
    let _ = next_event(end);

    // 7. A request outlives its time: TimedOut, the handler told, and too
    //    late to allow.
    let id = request_capability(
        asker,
        "asker",
        ResourceType::SystemClock,
        0,
        Rights::WRITE,
        "x",
    )?;
    let _ = next_event(end);
    let created = REQUESTS
        .lock()
        .iter()
        .find(|r| r.id == id)
        .map_or(0, |r| r.created_at_ms);
    expire_at(created.saturating_add(REQUEST_TIMEOUT_MS));
    check(
        get_status(id) == Some(RequestStatus::TimedOut),
        "it did not time out",
    )?;
    let ev = next_event(end).unwrap_or_default();
    check(
        event_parts(&ev) == Some((EVENT_ENDED, RequestStatus::TimedOut.code(), id)),
        "no EVENT_ENDED for a timed-out request",
    )?;
    // `TimedOut`, as `decide` documents it: too late, not malformed.
    check(
        decide(id, true, Decider::Handler(handler_pid)) == Err(KernelError::TimedOut)
            && !pcb::has_capability_for(asker, ResourceType::SystemClock, 0, Rights::WRITE),
        "a timed-out request was granted, or its answer was not TimedOut",
    )?;

    // 8. The asker's exit cancels what it had pending.
    let id = request_capability(
        asker,
        "asker",
        ResourceType::SystemClock,
        0,
        Rights::WRITE,
        "x",
    )?;
    let _ = next_event(end);
    on_process_exit(asker);
    check(
        get_status(id) == Some(RequestStatus::Cancelled),
        "the asker's exit did not cancel",
    )?;
    let _ = next_event(end);

    // 9. A full queue loses events, and says so before the next one.
    let flood = request_capability(
        asker,
        "asker",
        ResourceType::SystemClock,
        0,
        Rights::WRITE,
        "x",
    )?;
    // Fill the handler's queue without reading it: every request sends one
    // EVENT_NEW and every cancel one EVENT_ENDED.
    let mut sent = 0usize;
    while let Ok(more) = request_capability(
        asker,
        "asker",
        ResourceType::SystemClock,
        0,
        Rights::WRITE,
        "x",
    ) {
        cancel(more, asker)?;
        sent = sent.saturating_add(2);
        if LOST.load(Ordering::Acquire) || sent > 200 {
            break;
        }
    }
    check(
        LOST.load(Ordering::Acquire),
        "a full queue did not lose events",
    )?;
    while next_event(end).is_some() {}
    cancel(flood, asker)?;
    let ev = next_event(end).unwrap_or_default();
    check(
        event_parts(&ev) == Some((EVENT_LOST, 0, 0)) && !LOST.load(Ordering::Acquire),
        "the first event after a loss was not EVENT_LOST",
    )?;
    let ev = next_event(end).unwrap_or_default();
    check(
        event_parts(&ev) == Some((EVENT_ENDED, RequestStatus::Cancelled.code(), flood)),
        "the event after EVENT_LOST was not the one that came next",
    )?;

    // 10. The handler's exit refuses what was pending and frees the place.
    let id = request_capability(
        asker,
        "asker",
        ResourceType::SystemClock,
        0,
        Rights::WRITE,
        "x",
    )?;
    on_process_exit(handler_pid);
    check(
        get_status(id) == Some(RequestStatus::Denied) && *HANDLER.lock() == Handler::None,
        "the handler's exit did not refuse and unregister",
    )?;
    // The handler's own end is registered as its handle: `pcb::destroy`
    // closes it, as an exit's cleanup would.

    // 11. The console: requests pend, and its approval grants.
    register_handler();
    let id = request_capability(
        asker,
        "asker",
        ResourceType::ResourceLimit,
        asker,
        Rights::WRITE,
        "x",
    )?;
    check(
        get_status(id) == Some(RequestStatus::Pending),
        "the console's request did not pend",
    )?;
    check(
        approve(id) == Ok(RequestStatus::Approved),
        "the console's approval failed",
    )?;
    check(
        pcb::has_capability_for(asker, ResourceType::ResourceLimit, asker, Rights::WRITE),
        "the console's approval granted nothing",
    )?;
    unregister_handler();

    // 12. Names are cut on a character boundary, never mid-character: the
    //     byte at the limit is inside 'é', which a plain slice would panic on.
    let mut long = String::new();
    for _ in 0..255 {
        long.push('a');
    }
    long.push('\u{00E9}');
    let cut = truncate_utf8(&long, MAX_NAME_LEN);
    check(
        cut.len() == 255 && cut.chars().all(|c| c == 'a'),
        "a name cut mid-character",
    )?;

    serial_println!(
        "[cap-request]   refusals, the handler's events and records, allow grants, deny, \
         timeouts, exits, lost events, the console: OK"
    );
    Ok(())
}
