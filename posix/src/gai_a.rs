// Counts here are of a caller's list or the process's queue, and time is
// seconds and nanoseconds of a `struct timespec`, checked; clippy cannot see
// the bounds.
#![allow(clippy::arithmetic_side_effects)]
//! Asynchronous lookups (`<netdb.h>`, GNU): `getaddrinfo_a`, `gai_suspend`,
//! `gai_error` and `gai_cancel` -- [`getaddrinfo`] on threads of the
//! library's own, as getaddrinfo_a(3) describes and glibc 2.39 answers.
//!
//! - **`getaddrinfo_a`** queues each request of the list that is not NULL --
//!   its `gai_error` `EAI_INPROGRESS` from then on -- and, for `GAI_WAIT`,
//!   returns once every one has an answer; for `GAI_NOWAIT` at once, the
//!   `struct sigevent` saying how the caller is told the batch is done
//!   (`SIGEV_THREAD`, `SIGEV_SIGNAL`, or nothing).  0, `EAI_SYSTEM` with
//!   `errno` `EINVAL` for a mode that is neither, `EAI_MEMORY` or
//!   `EAI_AGAIN` when a request could not be queued.
//! - **Threads.**  At most 20 run lookups at once, as glibc's default; a
//!   request waits in the queue for one, and each takes the next as it
//!   finishes, in the order they were queued.
//! - **`gai_error`** is the request's answer -- what `getaddrinfo` returned,
//!   its `ar_result` set -- or `EAI_INPROGRESS`; read from the control
//!   block, as glibc reads it, so a block never queued answers what it
//!   holds.
//! - **`gai_suspend`** waits until a listed request that is still being
//!   looked up has its answer: 0; `EAI_AGAIN` when the timeout (relative,
//!   on `CLOCK_MONOTONIC`) runs out first; and `EAI_ALLDONE`, as glibc's,
//!   when none listed is still being looked up -- all of them finished, or
//!   the list only NULLs.
//! - **`gai_cancel`**: `EAI_CANCELED` for a request still in the queue,
//!   which is taken out; `EAI_NOTCANCELED` for one being looked up;
//!   `EAI_ALLDONE` for one with its answer, or never queued.
//!
//! **Where glibc is at odds with itself, this follows the rest of it**
//! (design-decisions §1155): a cancelled request's `gai_error` is
//! `EAI_CANCELED`, as getaddrinfo_a(3) says -- glibc's says
//! `EAI_INPROGRESS` for ever, while its `gai_cancel` then calls the same
//! request finished -- and cancelling counts as an answer for the batch it
//! came in: a `GAI_WAIT` caller returns and a `GAI_NOWAIT` batch is
//! notified, where glibc's would wait, and stay silent, for ever.
//!
//! A signal handler that runs on the thread cuts `gai_suspend` short with
//! `EAI_INTR`: without a timeout, one installed without `SA_RESTART`; with
//! one, any -- as glibc's answers ([`crate::interrupt`]).
//!
//! glibc 2.39's answers -- a waited batch of numbers, hosts-file names and
//! failures, notification by thread, a suspended wait, the cancellation of a
//! request queued behind 199 others -- are `gai_a_oracle.txt`
//! (`posix/tools/oracle/gai_a_harness.py`), which the tests replay.
//!
//! [`getaddrinfo`]: crate::gai::getaddrinfo

use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicI32, Ordering};

use crate::errno;
use crate::interrupt::Restart;
use crate::lowlevellock::{
    Waited, futex_wait, futex_wait_interruptible, futex_wake_all, lll_lock, lll_unlock,
};
use crate::sigevent::SigeventView;
use crate::socket::{Addrinfo, EAI_AGAIN, EAI_MEMORY, EAI_SYSTEM};

/// `getaddrinfo_a`'s mode: return once every request has its answer.
pub const GAI_WAIT: i32 = 0;
/// Return at once.
pub const GAI_NOWAIT: i32 = 1;

/// The request is still being looked up.
pub const EAI_INPROGRESS: i32 = -100;
/// The request was cancelled.
pub const EAI_CANCELED: i32 = -101;
/// The request could not be cancelled: it is being looked up.
pub const EAI_NOTCANCELED: i32 = -102;
/// No request asked about is still being looked up.
pub const EAI_ALLDONE: i32 = -103;
/// A signal interrupted the wait (never answered here: see the module).
pub const EAI_INTR: i32 = -104;

/// How many threads look up at once: glibc's default.
const MAX_THREADS: usize = 20;

/// `struct gaicb`, glibc's layout: the request, and its answer.
#[repr(C)]
pub struct Gaicb {
    /// The name to look up.
    pub ar_name: *const u8,
    /// The service.
    pub ar_service: *const u8,
    /// The hints, or NULL.
    pub ar_request: *const Addrinfo,
    /// The answer, once there is one.
    pub ar_result: *mut Addrinfo,
    /// The request's state: `EAI_INPROGRESS`, or `getaddrinfo`'s answer
    /// (glibc's `__return`).
    pub ret: i32,
    /// glibc's reserved words.
    pub reserved: [i32; 5],
}

/// A batch -- one `getaddrinfo_a` call's requests -- and how its caller is
/// told it is done.
struct Batch {
    /// Its requests that have no answer yet.
    remaining: usize,
    /// `GAI_WAIT`: the caller waits for `remaining` to reach 0, and frees the
    /// batch; `GAI_NOWAIT`: whoever finishes it notifies and frees it.
    waited: bool,
    notice: SigeventView,
}

/// A queued request.
struct Request {
    gaicb: *mut Gaicb,
    batch: *mut Batch,
    running: bool,
    next: *mut Request,
    /// The submitting test thread's databases, which the lookup reads.
    #[cfg(test)]
    dbs: crate::nss_files::TestDbs,
}

/// The queue, in the order the requests came, and how many threads there
/// are to work it.
struct Queue {
    head: *mut Request,
    threads: usize,
}

/// The process's queue, under [`LOCK`]: one for every thread, as the
/// requests and the threads looking them up are the process's.
struct Shared(UnsafeCell<Queue>);

// SAFETY: every access is under `LOCK`.
unsafe impl Sync for Shared {}

static QUEUE: Shared = Shared(UnsafeCell::new(Queue {
    head: core::ptr::null_mut(),
    threads: 0,
}));
static LOCK: AtomicI32 = AtomicI32::new(0);
/// Moves on every answer and cancellation; what the waits sleep on.
static FINISHED: AtomicI32 = AtomicI32::new(0);

/// The queue, the lock held.
struct Guard;

impl Guard {
    fn lock() -> Self {
        lll_lock(&LOCK);
        Self
    }

    // Exclusive by the lock, which `self` holds: borrowing the guard is what
    // ties the queue's use to the lock's being held.
    #[allow(clippy::mut_from_ref, clippy::unused_self)]
    fn queue(&self) -> &mut Queue {
        // SAFETY: the lock is held while `self` lives.
        unsafe { &mut *QUEUE.0.get() }
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        lll_unlock(&LOCK);
    }
}

/// A request's state, read as `gai_error` reads it.
///
/// # Safety
///
/// `g` points to a live control block.
unsafe fn state(g: *const Gaicb) -> i32 {
    // SAFETY: the caller's; the field is written as an atomic by the threads.
    unsafe { (*core::ptr::addr_of!((*g).ret).cast::<AtomicI32>()).load(Ordering::Acquire) }
}

/// Set a request's state.
///
/// # Safety
///
/// As [`state`].
unsafe fn set_state(g: *mut Gaicb, value: i32) {
    // SAFETY: as in `state`.
    unsafe { (*core::ptr::addr_of!((*g).ret).cast::<AtomicI32>()).store(value, Ordering::Release) }
}

/// The request for `g` in the queue, and the one before it.
fn find(q: &Queue, g: *const Gaicb) -> Option<(*mut Request, *mut Request)> {
    let (mut prev, mut r) = (core::ptr::null_mut::<Request>(), q.head);
    while !r.is_null() {
        // SAFETY: the queue's requests are live while the lock is held.
        if core::ptr::eq(unsafe { (*r).gaicb }, g) {
            return Some((prev, r));
        }
        prev = r;
        // SAFETY: as above.
        r = unsafe { (*r).next };
    }
    None
}

/// Take request `r`, after `prev`, out of the queue, and count its answer
/// against its batch: the batch to notify and free, if this finished it.
///
/// # Safety
///
/// `r` is in `q`, after `prev`; the lock is held.
unsafe fn unqueue(q: &mut Queue, prev: *mut Request, r: *mut Request) -> Option<*mut Batch> {
    // SAFETY: the caller's contract.
    unsafe {
        if prev.is_null() {
            q.head = (*r).next;
        } else {
            (*prev).next = (*r).next;
        }
        let batch = (*r).batch;
        crate::malloc::free(r.cast());
        (*batch).remaining -= 1;
        ((*batch).remaining == 0 && !(*batch).waited).then_some(batch)
    }
}

/// Tell a finished `GAI_NOWAIT` batch's caller, and free the batch.
///
/// # Safety
///
/// `batch` is finished, not waited, and nothing else refers to it.
unsafe fn finish(batch: *mut Batch) {
    // SAFETY: the caller's contract.
    let notice = unsafe { (*batch).notice };
    // SAFETY: as above.
    unsafe { crate::malloc::free(batch.cast()) };
    tell(&notice);
}

/// Tell the caller as `notice` asks (`crate::sigevent`, as `aio` and
/// `mq_notify` tell theirs); a notice that cannot be given has no one to
/// report to, as in glibc.
#[cfg(not(all(test, not(target_os = "none"))))]
fn tell(notice: &SigeventView) {
    let _ = crate::sigevent::notify(notice);
}

/// Host tests: the library's threads are the kernel's, which a host has
/// none of, so a `SIGEV_THREAD` function runs on a thread of the test's.
#[cfg(all(test, not(target_os = "none")))]
fn tell(notice: &SigeventView) {
    if notice.sigev_notify != crate::time::SIGEV_THREAD {
        let _ = crate::sigevent::notify(notice);
        return;
    }
    if let Some(f) = notice.sigev_notify_function {
        let value = notice.sigev_value;
        std::thread::spawn(move || f(value));
    }
}

/// Wake whoever waits on an answer.
fn announce() {
    FINISHED.fetch_add(1, Ordering::Release);
    futex_wake_all(&FINISHED);
}

/// A lookup thread: take the queue's first request no thread has, look it
/// up, record the answer; until there is none.
extern "C" fn worker(_arg: *mut u8) -> *mut u8 {
    loop {
        // A test holds the threads back to find a request still queued.
        #[cfg(test)]
        while tests::PAUSED.load(Ordering::Acquire) {
            std::thread::yield_now();
        }
        let (g, r) = {
            let guard = Guard::lock();
            let q = guard.queue();
            let mut r = q.head;
            // SAFETY: the queue's requests are live while the lock is held.
            while !r.is_null() && unsafe { (*r).running } {
                // SAFETY: as above.
                r = unsafe { (*r).next };
            }
            if r.is_null() {
                q.threads -= 1;
                return core::ptr::null_mut();
            }
            // SAFETY: as above; a running request is this thread's to finish.
            unsafe {
                (*r).running = true;
                #[cfg(test)]
                crate::nss_files::test_adopt(&(*r).dbs);
                ((*r).gaicb, r)
            }
        };
        let mut answer: *mut Addrinfo = core::ptr::null_mut();
        // SAFETY: the caller's control block, live until its request has an
        // answer (the interface's contract), with its strings and hints.
        let rc = unsafe {
            crate::gai::getaddrinfo((*g).ar_name, (*g).ar_service, (*g).ar_request, &mut answer)
        };
        let done = {
            let guard = Guard::lock();
            let q = guard.queue();
            // SAFETY: the request is still queued -- a running one cannot be
            // cancelled -- and its block live.
            unsafe {
                (*g).ar_result = answer;
                set_state(g, rc);
                let prev = find(q, g).map_or(core::ptr::null_mut(), |(p, _)| p);
                unqueue(q, prev, r)
            }
        };
        announce();
        if let Some(batch) = done {
            // SAFETY: finished by this thread, and no longer reachable.
            unsafe { finish(batch) };
        }
    }
}

/// Start a lookup thread if there are fewer than [`MAX_THREADS`]: whether
/// one runs now.
fn start_thread(q: &mut Queue) -> bool {
    if q.threads >= MAX_THREADS {
        return true;
    }
    if spawn_worker() {
        q.threads += 1;
    }
    q.threads > 0
}

/// Start a detached lookup thread: whether it started.
#[cfg(not(all(test, not(target_os = "none"))))]
fn spawn_worker() -> bool {
    let mut attr: crate::pthread::PthreadAttrT = [0; 56];
    // Neither can fail for an attribute object of this library's own.
    let _ = crate::pthread::pthread_attr_init(&mut attr);
    let _ = crate::pthread::pthread_attr_setdetachstate(
        &mut attr,
        crate::pthread::PTHREAD_CREATE_DETACHED,
    );
    let mut id: crate::pthread::PthreadT = 0;
    let rc = crate::pthread::pthread_create(&mut id, &attr, Some(worker), core::ptr::null_mut());
    let _ = crate::pthread::pthread_attr_destroy(&mut attr);
    rc == 0
}

/// Host tests: a thread of the test's (see [`tell`]).
#[cfg(all(test, not(target_os = "none")))]
fn spawn_worker() -> bool {
    std::thread::Builder::new()
        .spawn(|| {
            worker(core::ptr::null_mut());
        })
        .is_ok()
}

/// Look the requests of `list` up -- its first `ent`, the NULLs among them
/// passed over -- on the library's threads: for `GAI_WAIT`, returning once
/// each has its answer; for `GAI_NOWAIT`, at once, `sig` saying how to tell
/// the caller the batch is done (NULL: not at all).  0; `EAI_SYSTEM` with
/// `errno` `EINVAL` for another mode; `EAI_MEMORY` when the batch cannot be
/// had, `EAI_AGAIN` when no thread can be started for a request -- that
/// request not queued, the others as they were.
///
/// # Safety
///
/// `list` holds `ent` pointers, each NULL or to a control block that stays
/// live, with its strings and hints, until the request has its answer;
/// `sig` is NULL or a `struct sigevent`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn getaddrinfo_a(
    mode: i32,
    list: *const *mut Gaicb,
    ent: i32,
    sig: *const u8,
) -> i32 {
    if mode != GAI_WAIT && mode != GAI_NOWAIT {
        errno::set_errno(errno::EINVAL);
        return EAI_SYSTEM;
    }
    let notice = if sig.is_null() {
        SigeventView::NONE
    } else {
        SigeventView::read(sig)
    };
    let batch = crate::malloc::malloc(size_of::<Batch>()).cast::<Batch>();
    if batch.is_null() {
        return EAI_MEMORY;
    }
    // SAFETY: a fresh block of the right size.
    unsafe {
        batch.write(Batch {
            remaining: 0,
            waited: mode == GAI_WAIT,
            notice,
        });
    }
    let mut result = 0;
    let guard = Guard::lock();
    let q = guard.queue();
    let count = usize::try_from(ent).unwrap_or(0);
    for i in 0..count {
        // SAFETY: `list` holds `ent` pointers.
        let g = unsafe { *list.add(i) };
        if g.is_null() {
            continue;
        }
        let r = crate::malloc::malloc(size_of::<Request>()).cast::<Request>();
        if r.is_null() {
            result = EAI_MEMORY;
            continue;
        }
        // SAFETY: a fresh block; `g` the caller's live control block.
        unsafe {
            r.write(Request {
                gaicb: g,
                batch,
                running: false,
                next: core::ptr::null_mut(),
                #[cfg(test)]
                dbs: crate::nss_files::test_snapshot(),
            });
            set_state(g, EAI_INPROGRESS);
            // Last in the queue.
            let mut at = &raw mut q.head;
            while !(*at).is_null() {
                at = &raw mut (**at).next;
            }
            *at = r;
            (*batch).remaining += 1;
        }
        if !start_thread(q) {
            // No thread, and none to be had: the request cannot be answered.
            // SAFETY: `r` was just queued, last.
            unsafe {
                let prev = find(q, g).map_or(core::ptr::null_mut(), |(p, _)| p);
                (*batch).remaining -= 1;
                if prev.is_null() {
                    q.head = core::ptr::null_mut();
                } else {
                    (*prev).next = core::ptr::null_mut();
                }
                crate::malloc::free(r.cast());
                set_state(g, EAI_AGAIN);
            }
            result = EAI_AGAIN;
        }
    }
    // SAFETY: the batch is this call's until its requests are queued.
    let remaining = unsafe { (*batch).remaining };
    if remaining == 0 {
        drop(guard);
        if mode == GAI_NOWAIT {
            // Nothing to wait for: told at once, as glibc tells.
            // SAFETY: never shared.
            unsafe { finish(batch) };
        } else {
            // SAFETY: never shared.
            unsafe { crate::malloc::free(batch.cast()) };
        }
        return result;
    }
    if mode == GAI_NOWAIT {
        // The thread that finishes the last request notifies and frees it.
        return result;
    }
    drop(guard);
    loop {
        let seen = FINISHED.load(Ordering::Acquire);
        let guard = Guard::lock();
        // SAFETY: a waited batch is freed only here.
        if unsafe { (*batch).remaining } == 0 {
            drop(guard);
            break;
        }
        drop(guard);
        futex_wait(&FINISHED, seen);
    }
    // SAFETY: finished, and no request refers to it any more.
    unsafe { crate::malloc::free(batch.cast()) };
    result
}

/// A request's answer: `getaddrinfo`'s, `EAI_INPROGRESS` while it has none,
/// `EAI_CANCELED` once cancelled -- read from the control block, so one
/// never queued answers what it holds.  `EAI_SYSTEM` with `errno` `EFAULT`
/// for NULL, where glibc's would fault.
///
/// # Safety
///
/// `req` is NULL or a control block.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn gai_error(req: *mut Gaicb) -> i32 {
    if req.is_null() {
        errno::set_errno(errno::EFAULT);
        return EAI_SYSTEM;
    }
    // SAFETY: the caller's control block.
    unsafe { state(req) }
}

/// Wait until one of the first `ent` requests of `list` that are still
/// being looked up has its answer: 0; `EAI_AGAIN` when `timeout` (NULL:
/// none) runs out first; `EAI_ALLDONE` when none listed is being looked up
/// -- finished, never queued, or NULL -- as glibc answers.
///
/// # Safety
///
/// `list` holds `ent` pointers, each NULL or to a control block; `timeout`
/// is NULL or a `struct timespec`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn gai_suspend(
    list: *const *const Gaicb,
    ent: i32,
    timeout: *const crate::stat::Timespec,
) -> i32 {
    let count = usize::try_from(ent).unwrap_or(0);
    // SAFETY: `list` holds `ent` pointers.
    let listed = |i: usize| unsafe { *list.add(i) };
    // Those being looked up now: only they can end the wait.
    let pending = |g: *const Gaicb| -> bool {
        // SAFETY: a non-null entry is a control block.
        !g.is_null() && unsafe { state(g) } == EAI_INPROGRESS
    };
    // Which listed requests were being looked up when the wait began.
    let Some(mut waiting) = crate::decfloat::MallocBuf::<u8>::zeroed(count) else {
        errno::set_errno(errno::ENOMEM);
        return EAI_SYSTEM;
    };
    let seen = FINISHED.load(Ordering::Acquire);
    {
        let guard = Guard::lock();
        let q = guard.queue();
        let mut any = false;
        for (i, w) in waiting.as_mut().iter_mut().enumerate().take(count) {
            let g = listed(i);
            let queued = pending(g) && find(q, g).is_some();
            *w = u8::from(queued);
            any |= queued;
        }
        if !any {
            return EAI_ALLDONE;
        }
    }
    let deadline = if timeout.is_null() {
        None
    } else {
        // SAFETY: the caller's `struct timespec`.
        let t = unsafe { *timeout };
        if !(0..1_000_000_000).contains(&t.tv_nsec) {
            // glibc's wait refuses it, and the refusal is `EAI_SYSTEM`.
            errno::set_errno(errno::EINVAL);
            return EAI_SYSTEM;
        }
        let now = crate::lowlevellock::now_on(crate::time::CLOCK_MONOTONIC);
        let ns = now.tv_nsec + t.tv_nsec;
        Some(crate::stat::Timespec {
            tv_sec: now
                .tv_sec
                .saturating_add(t.tv_sec)
                .saturating_add(ns / 1_000_000_000),
            tv_nsec: ns % 1_000_000_000,
        })
    };
    let mut seen = seen;
    loop {
        // One that was being looked up has its answer now.
        let answered = waiting
            .as_ref()
            .iter()
            .take(count)
            .enumerate()
            .any(|(i, &was)| was != 0 && !pending(listed(i)));
        if answered {
            return 0;
        }
        let waited = match deadline {
            None => futex_wait_interruptible(&FINISHED, seen, None, Restart::IfAsked),
            Some(d) => {
                let now = crate::lowlevellock::now_on(crate::time::CLOCK_MONOTONIC);
                let Some(left) = crate::lowlevellock::ns_until(&now, &d) else {
                    return EAI_AGAIN;
                };
                futex_wait_interruptible(&FINISHED, seen, Some(left), Restart::Never)
            }
        };
        if waited == Waited::Interrupted {
            return EAI_INTR;
        }
        seen = FINISHED.load(Ordering::Acquire);
    }
}

/// Cancel a request: `EAI_CANCELED` for one still in the queue, taken out --
/// its `gai_error` `EAI_CANCELED` from then on, and its batch counting it
/// as answered; `EAI_NOTCANCELED` for one being looked up; `EAI_ALLDONE`
/// for one with its answer, never queued, or NULL.
///
/// # Safety
///
/// `req` is NULL or a control block.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn gai_cancel(req: *mut Gaicb) -> i32 {
    if req.is_null() {
        return EAI_ALLDONE;
    }
    let done = {
        let guard = Guard::lock();
        let q = guard.queue();
        let Some((prev, r)) = find(q, req) else {
            return EAI_ALLDONE;
        };
        // SAFETY: a queued request is live while the lock is held.
        if unsafe { (*r).running } {
            return EAI_NOTCANCELED;
        }
        // SAFETY: as above; `req` the caller's block.
        unsafe {
            set_state(req, EAI_CANCELED);
            unqueue(q, prev, r)
        }
    };
    announce();
    if let Some(batch) = done {
        // SAFETY: finished by this cancellation, and no longer reachable.
        unsafe { finish(batch) };
    }
    EAI_CANCELED
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::nss_files::{Which, set_test_text};
    use crate::socket::{AF_INET, AF_INET6, AF_UNSPEC, SOCK_STREAM, SockaddrIn, SockaddrIn6};
    use core::sync::atomic::{AtomicBool, AtomicUsize};
    use std::boxed::Box;
    use std::format;
    use std::string::String;
    use std::vec::Vec;

    /// While set, the lookup threads take no request: a test finds one
    /// still queued.
    pub(crate) static PAUSED: AtomicBool = AtomicBool::new(false);

    /// Held by every test here: the queue, its threads and [`PAUSED`] are the
    /// whole test process's.
    static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

    pub(crate) fn serial() -> std::sync::MutexGuard<'static, ()> {
        SERIAL
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// glibc 2.39's answers (`posix/tools/oracle/gai_a_harness.py`).
    const ORACLE: &str = include_str!("gai_a_oracle.txt");

    /// The oracle's hosts file.
    const HOSTS: &[u8] =
        b"127.0.0.1 localhost\n10.1.2.3 alpha alpha.example\n::1 localhost ip6-localhost\n";

    pub(crate) fn eai(e: i32) -> String {
        match e {
            0 => "0".into(),
            crate::socket::EAI_NONAME => "EAI_NONAME".into(),
            EAI_AGAIN => "EAI_AGAIN".into(),
            EAI_SYSTEM => "EAI_SYSTEM".into(),
            crate::socket::EAI_SERVICE => "EAI_SERVICE".into(),
            EAI_INPROGRESS => "EAI_INPROGRESS".into(),
            EAI_CANCELED => "EAI_CANCELED".into(),
            EAI_NOTCANCELED => "EAI_NOTCANCELED".into(),
            EAI_ALLDONE => "EAI_ALLDONE".into(),
            EAI_INTR => "EAI_INTR".into(),
            e => format!("{e}"),
        }
    }

    /// A request, leaked: it and its strings and hints outlive the lookup.
    fn make(
        name: Option<&'static core::ffi::CStr>,
        service: Option<&'static core::ffi::CStr>,
        family: i32,
    ) -> *mut Gaicb {
        let hints = Box::leak(Box::new(Addrinfo {
            ai_flags: 0,
            ai_family: family,
            ai_socktype: SOCK_STREAM,
            ai_protocol: 0,
            ai_addrlen: 0,
            ai_addr: core::ptr::null_mut(),
            ai_canonname: core::ptr::null_mut(),
            ai_next: core::ptr::null_mut(),
        }));
        Box::leak(Box::new(Gaicb {
            ar_name: name.map_or(core::ptr::null(), |n| n.as_ptr().cast()),
            ar_service: service.map_or(core::ptr::null(), |s| s.as_ptr().cast()),
            ar_request: hints,
            ar_result: core::ptr::null_mut(),
            ret: 0,
            reserved: [0; 5],
        }))
    }

    /// The harness's `answer`: the first address and port, or `-`.
    fn answer(g: *const Gaicb) -> String {
        // SAFETY: a control block of this test's; its answer, when there is
        // one, `getaddrinfo`'s list.
        unsafe {
            let ai = (*g).ar_result;
            if ai.is_null() {
                return "-".into();
            }
            let mut buf = [0u8; 64];
            let (src, port): (*const u8, u16) = match (*ai).ai_family {
                AF_INET => {
                    let s = (*ai).ai_addr.cast::<SockaddrIn>();
                    (
                        core::ptr::addr_of!((*s).sin_addr).cast(),
                        u16::from_be((*s).sin_port),
                    )
                }
                _ => {
                    let s = (*ai).ai_addr.cast::<SockaddrIn6>();
                    (
                        core::ptr::addr_of!((*s).sin6_addr).cast(),
                        u16::from_be((*s).sin6_port),
                    )
                }
            };
            crate::inet::inet_ntop((*ai).ai_family, src, buf.as_mut_ptr(), 64);
            let text = core::ffi::CStr::from_ptr(buf.as_ptr().cast())
                .to_str()
                .unwrap();
            format!("{text}:{port}")
        }
    }

    fn name_of(p: *const u8) -> String {
        if p.is_null() {
            "NULL".into()
        } else {
            // SAFETY: a C string of this test's.
            unsafe { core::ffi::CStr::from_ptr(p.cast()) }
                .to_str()
                .unwrap()
                .into()
        }
    }

    static NOTIFIED: AtomicUsize = AtomicUsize::new(0);
    static NOTIFIED_DONE: AtomicBool = AtomicBool::new(false);

    extern "C" fn notify(value: usize) {
        NOTIFIED.store(value, Ordering::Release);
        NOTIFIED_DONE.store(true, Ordering::Release);
    }

    /// A `struct sigevent`, musl's layout, asking for `notify` on a thread
    /// with `value`.
    fn thread_notice(value: usize) -> [u8; 64] {
        let view = SigeventView {
            sigev_value: value,
            sigev_signo: 0,
            sigev_notify: crate::time::SIGEV_THREAD,
            sigev_notify_function: Some(notify),
            sigev_notify_attributes: core::ptr::null(),
            __pad: [0; 32],
        };
        // SAFETY: `SigeventView` is 64 bytes of plain data.
        unsafe { core::mem::transmute::<SigeventView, [u8; 64]>(view) }
    }

    /// The harness's program, call for call.
    #[allow(clippy::too_many_lines)] // the harness's program, in its order
    fn probes() -> Vec<String> {
        let mut out = Vec::new();
        out.push(format!("sizeof(struct gaicb) = {}", size_of::<Gaicb>()));
        out.push(format!("GAI_WAIT = {GAI_WAIT} GAI_NOWAIT = {GAI_NOWAIT}"));
        let zero = make(None, None, AF_UNSPEC);
        // SAFETY (throughout): control blocks, lists and notices of this
        // test's, which outlive their lookups.
        unsafe {
            out.push(format!(
                "gai_error(never submitted) = {}",
                eai(gai_error(zero))
            ));
            out.push(format!(
                "gai_cancel(never submitted) = {}",
                eai(gai_cancel(zero))
            ));
            errno::set_errno(12345);
            let rc = getaddrinfo_a(2, core::ptr::null(), 0, core::ptr::null());
            let e = match errno::get_errno() {
                errno::EINVAL => "EINVAL",
                12345 => "kept",
                _ => "other",
            };
            out.push(format!("getaddrinfo_a(mode 2) = {} errno={e}", eai(rc)));
            errno::set_errno(12345);
            let rc = getaddrinfo_a(GAI_WAIT, core::ptr::null(), 0, core::ptr::null());
            let e = if errno::get_errno() == 12345 {
                "kept"
            } else {
                "other"
            };
            out.push(format!(
                "getaddrinfo_a(GAI_WAIT, 0 entries) = {} errno={e}",
                eai(rc)
            ));
            let nulls: [*mut Gaicb; 3] = [core::ptr::null_mut(); 3];
            let rc = getaddrinfo_a(GAI_WAIT, nulls.as_ptr(), 3, core::ptr::null());
            out.push(format!("getaddrinfo_a(GAI_WAIT, only NULLs) = {}", eai(rc)));
            let rc = gai_suspend(nulls.as_ptr().cast(), 3, core::ptr::null());
            out.push(format!("gai_suspend(only NULLs) = {}", eai(rc)));

            let list: [*mut Gaicb; 7] = [
                make(Some(c"127.0.0.1"), Some(c"80"), AF_INET),
                core::ptr::null_mut(),
                make(Some(c"alpha"), Some(c"22"), AF_INET),
                make(Some(c"localhost"), None, AF_INET6),
                make(Some(c"no.such.host.invalid"), Some(c"80"), AF_UNSPEC),
                make(Some(c"10.9.8.7"), Some(c"nosuchservice"), AF_INET),
                make(None, Some(c"443"), AF_INET),
            ];
            errno::set_errno(12345);
            let rc = getaddrinfo_a(GAI_WAIT, list.as_ptr(), 7, core::ptr::null());
            let e = if errno::get_errno() == 12345 {
                "kept"
            } else {
                "other"
            };
            out.push(format!(
                "getaddrinfo_a(GAI_WAIT, batch) = {} errno={e}",
                eai(rc)
            ));
            for (i, &g) in list.iter().enumerate() {
                if g.is_null() {
                    continue;
                }
                out.push(format!(
                    "batch[{i}] {}:{} gai_error={} answer={}",
                    name_of((*g).ar_name),
                    name_of((*g).ar_service),
                    eai(gai_error(g)),
                    answer(g)
                ));
            }
            let rc = gai_suspend(list.as_ptr().cast(), 7, core::ptr::null());
            out.push(format!("gai_suspend(finished batch) = {}", eai(rc)));
            let zero_time = crate::stat::Timespec {
                tv_sec: 0,
                tv_nsec: 0,
            };
            let rc = gai_suspend(list.as_ptr().cast(), 7, &zero_time);
            out.push(format!("gai_suspend(finished batch, 0 s) = {}", eai(rc)));
            out.push(format!(
                "gai_cancel(finished) = {}",
                eai(gai_cancel(list[0]))
            ));
            out.push(format!(
                "gai_error(finished, after cancel) = {}",
                eai(gai_error(list[0]))
            ));

            NOTIFIED_DONE.store(false, Ordering::Release);
            let two = [
                make(Some(c"alpha.example"), Some(c"80"), AF_INET),
                make(Some(c"localhost"), Some(c"80"), AF_INET),
            ];
            let notice = thread_notice(4242);
            let rc = getaddrinfo_a(GAI_NOWAIT, two.as_ptr(), 2, notice.as_ptr());
            out.push(format!(
                "getaddrinfo_a(GAI_NOWAIT, SIGEV_THREAD) = {}",
                eai(rc)
            ));
            while !NOTIFIED_DONE.load(Ordering::Acquire) {
                std::thread::yield_now();
            }
            out.push(format!(
                "notified with {}; gai_error = {} {}",
                NOTIFIED.load(Ordering::Acquire),
                eai(gai_error(two[0])),
                eai(gai_error(two[1]))
            ));

            let one = [make(Some(c"alpha"), Some(c"80"), AF_INET)];
            let rc = getaddrinfo_a(GAI_NOWAIT, one.as_ptr(), 1, core::ptr::null());
            out.push(format!(
                "getaddrinfo_a(GAI_NOWAIT, no notice) = {}",
                eai(rc)
            ));
            let wait1 = [one[0].cast_const()];
            while gai_suspend(wait1.as_ptr(), 1, core::ptr::null()) == 0
                && gai_error(one[0]) == EAI_INPROGRESS
            {}
            out.push(format!(
                "then gai_error = {} answer={}",
                eai(gai_error(one[0])),
                answer(one[0])
            ));

            // Held back, so the last is still queued when it is cancelled.
            const MANY: usize = 200;
            let many: Vec<*mut Gaicb> = (0..MANY)
                .map(|_| make(Some(c"localhost"), Some(c"80"), AF_INET))
                .collect();
            PAUSED.store(true, Ordering::Release);
            let rc = getaddrinfo_a(GAI_NOWAIT, many.as_ptr(), 200, core::ptr::null());
            out.push(format!("getaddrinfo_a(GAI_NOWAIT, {MANY}) = {}", eai(rc)));
            let last = many[MANY - 1];
            out.push(format!(
                "gai_cancel(last of {MANY}) = {}",
                eai(gai_cancel(last))
            ));
            out.push(format!("gai_error(cancelled) = {}", eai(gai_error(last))));
            out.push(format!(
                "gai_cancel(cancelled again) = {}",
                eai(gai_cancel(last))
            ));
            PAUSED.store(false, Ordering::Release);
            for &g in &many[..MANY - 1] {
                let w = [g.cast_const()];
                while gai_error(g) == EAI_INPROGRESS {
                    gai_suspend(w.as_ptr(), 1, core::ptr::null());
                }
            }
            let ok = many[..MANY - 1]
                .iter()
                .filter(|&&g| gai_error(g) == 0)
                .count();
            out.push(format!("the other {}: {ok} answered", MANY - 1));
            out.push(format!(
                "gai_error(cancelled, later) = {}",
                eai(gai_error(last))
            ));
        }
        out
    }

    /// Every probe of glibc's, but for its cancelled request's `gai_error`,
    /// which is `EAI_CANCELED` here (the module, design-decisions §1155).
    #[test]
    fn every_answer_is_glibcs_but_a_cancelled_requests() {
        let _serial = serial();
        set_test_text(Which::Hosts, Some(HOSTS));
        let ours = probes();
        set_test_text(Which::Hosts, None);
        let want: Vec<String> = ORACLE
            .lines()
            .filter(|l| !l.starts_with('#') && !l.starts_with("input "))
            .map(|l| {
                if l.starts_with("gai_error(cancelled") {
                    l.replace("EAI_INPROGRESS", "EAI_CANCELED")
                } else {
                    l.into()
                }
            })
            .collect();
        assert_eq!(want, ours);
    }

    /// A cancelled request counts as answered for its batch: a `GAI_WAIT`
    /// caller returns -- where glibc's would wait for ever -- and a
    /// `gai_suspend` on it ends.
    #[test]
    fn a_cancelled_request_ends_its_batchs_wait() {
        let _serial = serial();
        let a = make(Some(c"127.0.0.1"), Some(c"1"), AF_INET);
        let b = make(Some(c"127.0.0.2"), Some(c"2"), AF_INET);
        PAUSED.store(true, Ordering::Release);
        /// The two blocks, for the waiting thread.
        struct Blocks([*mut Gaicb; 2]);
        // SAFETY: leaked blocks, used by one thread at a time but for the
        // library's own atomic state.
        unsafe impl Send for Blocks {}
        let blocks = Blocks([a, b]);
        let waiter = std::thread::spawn(move || {
            let blocks = blocks;
            // SAFETY: control blocks of this test's.
            unsafe { getaddrinfo_a(GAI_WAIT, blocks.0.as_ptr(), 2, core::ptr::null()) }
        });
        // Both queued, neither taken: cancel them, and the waiter returns.
        // SAFETY: as above.
        unsafe {
            while state(b) != EAI_INPROGRESS {
                std::thread::yield_now();
            }
            assert_eq!(gai_cancel(a), EAI_CANCELED);
            assert_eq!(gai_cancel(b), EAI_CANCELED);
        }
        PAUSED.store(false, Ordering::Release);
        assert_eq!(waiter.join().unwrap(), 0);
        // SAFETY: as above.
        unsafe {
            assert_eq!((gai_error(a), gai_error(b)), (EAI_CANCELED, EAI_CANCELED));
            let list = [a.cast_const()];
            assert_eq!(
                gai_suspend(list.as_ptr(), 1, core::ptr::null()),
                EAI_ALLDONE
            );
        }
    }

    /// `gai_suspend` times out with `EAI_AGAIN` while a request waits, and
    /// refuses a timeout whose nanoseconds are not a second's.
    #[test]
    fn gai_suspend_times_out() {
        let _serial = serial();
        let g = make(Some(c"127.0.0.1"), Some(c"7"), AF_INET);
        PAUSED.store(true, Ordering::Release);
        // SAFETY: control blocks and times of this test's.
        unsafe {
            assert_eq!(
                getaddrinfo_a(GAI_NOWAIT, [g].as_ptr(), 1, core::ptr::null()),
                0
            );
            let list = [g.cast_const()];
            let short = crate::stat::Timespec {
                tv_sec: 0,
                tv_nsec: 1_000_000,
            };
            assert_eq!(gai_suspend(list.as_ptr(), 1, &short), EAI_AGAIN);
            let bad = crate::stat::Timespec {
                tv_sec: 0,
                tv_nsec: 1_000_000_000,
            };
            errno::set_errno(0);
            assert_eq!(gai_suspend(list.as_ptr(), 1, &bad), EAI_SYSTEM);
            assert_eq!(errno::get_errno(), errno::EINVAL);
            PAUSED.store(false, Ordering::Release);
            while gai_error(g) == EAI_INPROGRESS {
                gai_suspend(list.as_ptr(), 1, core::ptr::null());
            }
            assert_eq!(gai_error(g), 0);
            assert_eq!(answer(g), "127.0.0.1:7");
        }
    }
}
