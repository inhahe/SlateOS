//! The driver process's tasks running: their stacks, the switch between
//! them, and the loop that picks the next or sleeps until one may run
//! (design-decisions 1578 rule 5). Which may run is [`crate::task`]'s.
//!
//! A task runs until it blocks, yields or returns; then the loop runs
//! another. At every turn, between tasks, the C half sees to what Linux's
//! tick and ksoftirqd would (`lx_emul_turn`: `jiffies`, RCU callbacks,
//! timers, softirqs); before switching to a task, the loop tells the C half
//! which (`lx_emul_switch_to`, which sets Linux's `current`). With nothing
//! runnable it sleeps until the earliest of a task's deadline and the C
//! half's next event (`lx_emul_next_event`: a timer, or work pending now);
//! with neither, nothing will ever run again, and the process ends.

use core::ffi::c_void;

use crate::sys::{Single, exit_with, forbid, log, map_lazy, now_ns, sleep_ns, unmap};
use crate::task::{MAX_TASKS, State, Table};

/// A task's stack: 64 KiB, four times a Linux kernel stack, since Linux code
/// was written for 16 KiB and its calls here run through the layer too.
const STACK: usize = 64 * 1024;

/// Below each stack, a guard no access may touch: one of the system's pages,
/// so that a task overflowing its stack faults there and the process ends
/// where it went wrong, instead of writing over whatever lies below.
const GUARD: usize = 16 * 1024;

/// A task's mapping: its guard, then its stack.
const MAPPING: usize = GUARD + STACK;

/// `slx_stack_bytes` and `slx_guard_bytes`: [`STACK`] and [`GUARD`], for C
/// code that needs a stack's shape -- the hello driver's tests of it.
// Lower-case: they are C's names, declared in slx.h.
#[allow(non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static slx_stack_bytes: usize = STACK;
#[allow(non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static slx_guard_bytes: usize = GUARD;

/// Finished tasks' mappings kept for the next tasks, so that a driver that
/// starts and stops kthreads does not map, guard and unmap a stack each
/// time: at most this many.
const SPARE_STACKS: usize = 8;

/// A task's function, which the C half gives.
type Entry = unsafe extern "C" fn(*mut c_void);

#[derive(Clone, Copy)]
struct Task {
    /// Its stack pointer while it is not running.
    rsp: usize,
    /// Its mapping ([`map_stack`]): the guard's address.
    mapping: usize,
    entry: Option<Entry>,
    arg: *mut c_void,
    /// The C half's own record of it (a `task_struct`), given back to it at
    /// every switch.
    data: *mut c_void,
}

const NO_TASK: Task = Task {
    rsp: 0,
    mapping: 0,
    entry: None,
    arg: core::ptr::null_mut(),
    data: core::ptr::null_mut(),
};

struct Sched {
    table: Table,
    tasks: [Task; MAX_TASKS],
    /// The task running, if one is.
    current: Option<usize>,
    /// The loop's own stack pointer while a task runs.
    loop_rsp: usize,
    /// Spare mappings ([`SPARE_STACKS`]): the first `spares` of these.
    spare: [usize; SPARE_STACKS],
    spares: usize,
}

static SCHED: Single<Sched> = Single::new(Sched {
    table: Table::new(),
    tasks: [NO_TASK; MAX_TASKS],
    current: None,
    loop_rsp: 0,
    spare: [0; SPARE_STACKS],
    spares: 0,
});

// Switch stacks: save the callee-saved registers on the current stack, its
// pointer at `*save`, then take `to` and restore what it saved. A new
// task's stack is built to look like one that switched away just before
// calling `trampoline` ([`slx_task_new`]). The x87 and SSE control state is
// not switched: the target and Linux's flags use neither.
core::arch::global_asm!(
    ".globl lxkit_switch",
    "lxkit_switch:",
    "push rbp",
    "push rbx",
    "push r12",
    "push r13",
    "push r14",
    "push r15",
    "mov [rdi], rsp",
    "mov rsp, rsi",
    "pop r15",
    "pop r14",
    "pop r13",
    "pop r12",
    "pop rbx",
    "pop rbp",
    "ret",
);

unsafe extern "C" {
    fn lxkit_switch(save: *mut usize, to: usize);
    /// The C half: the task whose record is `data` is about to run, the
    /// monotonic clock reading `now_ns`.
    fn lx_emul_switch_to(data: *mut c_void, now_ns: u64);
    /// The C half: the task whose record is `data` waited past its deadline
    /// and may run again.
    fn lx_emul_timed_out(data: *mut c_void);
    /// The C half: a turn of the loop, the monotonic clock reading `now_ns`.
    fn lx_emul_turn(now_ns: u64);
    /// The C half, with no task to run: when the loop must turn again, by
    /// the monotonic clock, 0 for never.
    fn lx_emul_next_event(now_ns: u64) -> u64;
    /// The C half: whether the task whose record is `data` waits for work
    /// rather than for an event (kthreadd, a workqueue's workers).
    fn lx_emul_task_is_daemon(data: *mut c_void) -> bool;
}

/// The scheduler's state, through a raw pointer: every reference made from
/// it ends before a switch, since the task switched to makes its own.
fn sched() -> *mut Sched {
    SCHED.as_ptr()
}

/// Back to the loop from the current task `i`, which resumes here when the
/// loop next switches to it.
fn to_loop(i: usize) {
    // Only the loop sets `current`, to a slot it picked; a wait that
    // returned without waiting would let Linux code run on as if woken.
    if i >= MAX_TASKS {
        log(b"[lx] BUG: a task outside the table ran\n");
        exit_with(crate::exit::PANIC);
    }
    let s = sched();
    // SAFETY: the loop's stack pointer was saved when it switched to this
    // task; this task's slot is `i`, below MAX_TASKS, and no reference into
    // the state is held across the switch.
    unsafe {
        let slot = core::ptr::addr_of_mut!((*s).tasks).cast::<Task>().add(i);
        let save = core::ptr::addr_of_mut!((*slot).rsp);
        let to = (*s).loop_rsp;
        lxkit_switch(save, to);
    }
}

/// Where a new task starts: its function, then the end of it.
extern "C" fn trampoline() -> ! {
    let s = sched();
    // SAFETY: the loop set `current` to this task before switching to it.
    let (entry, arg) = unsafe {
        let i = (*s).current.unwrap_or(0);
        let t = (*s).tasks.get(i).copied().unwrap_or(NO_TASK);
        (t.entry, t.arg)
    };
    if let Some(entry) = entry {
        // SAFETY: the C half's function and its argument, as it gave them.
        unsafe { entry(arg) };
    }
    slx_task_exit()
}

/// `slx_task_exit`: the running task ends -- finished, never run again.
#[unsafe(no_mangle)]
pub extern "C" fn slx_task_exit() -> ! {
    let s = sched();
    // SAFETY: the one thread; a read.
    let Some(i) = (unsafe { (*s).current }) else {
        log(b"[lx] BUG: an end outside every task\n");
        exit_with(crate::exit::PANIC);
    };
    // SAFETY: the one thread; no other reference into the state is live.
    unsafe { (*s).table.finish(i) };
    to_loop(i);
    // A finished task is never switched to again.
    log(b"[lx] BUG: a finished task was resumed\n");
    exit_with(crate::exit::PANIC)
}

/// A task's stack and the guard below it, mapped -- lazily, so that only
/// what the task touches costs memory: the guard's address, or `None`.
fn map_stack() -> Option<usize> {
    let mapping = map_lazy(MAPPING)?;
    if forbid(mapping, GUARD) {
        Some(mapping)
    } else {
        unmap_stack(mapping);
        None
    }
}

/// Give back a mapping [`map_stack`] made. Failing would leak it, and is said.
fn unmap_stack(mapping: usize) {
    if !unmap(mapping, MAPPING) {
        log(b"[lx] BUG: a task's stack could not be unmapped\n");
    }
}

/// A stack for a new task: a spare one, or a new mapping; its guard's
/// address, or `None`.
fn take_stack() -> Option<usize> {
    let s = sched();
    // SAFETY: the one thread, outside any switch.
    unsafe {
        if let Some(n) = (*s).spares.checked_sub(1)
            && let Some(&mapping) = (*s).spare.get(n)
        {
            (*s).spares = n;
            return Some(mapping);
        }
    }
    map_stack()
}

/// A finished task's stack, kept as a spare, or unmapped when there are
/// enough spares.
fn give_back_stack(mapping: usize) {
    let s = sched();
    // SAFETY: the one thread, outside any switch.
    unsafe {
        let n = (*s).spares;
        if let Some(slot) = (*s).spare.get_mut(n) {
            *slot = mapping;
            // n < SPARE_STACKS: `get_mut` found the slot.
            (*s).spares = n.wrapping_add(1);
            return;
        }
    }
    unmap_stack(mapping);
}

/// `slx_stack_top`: a stack for the C half's own use (softirqs run on one),
/// mapped with a guard below it as a task's is: its top, or null.
#[unsafe(no_mangle)]
pub extern "C" fn slx_stack_top() -> *mut c_void {
    map_stack().map_or(core::ptr::null_mut(), |mapping| {
        mapping.wrapping_add(MAPPING) as *mut c_void
    })
}

/// `slx_task_new`: a new task, runnable, that will run `entry(arg)`; `data`
/// is the C half's record of it. Its handle, or -1 when no slot or stack is
/// left.
#[unsafe(no_mangle)]
pub extern "C" fn slx_task_new(entry: Entry, arg: *mut c_void, data: *mut c_void) -> i32 {
    let Some(mapping) = take_stack() else {
        return -1;
    };
    let s = sched();
    // SAFETY: the one thread, outside any switch.
    let Some(i) = (unsafe { (*s).table.add() }) else {
        give_back_stack(mapping);
        return -1;
    };
    // The top of the stack, page-aligned and so 16-aligned: as if
    // `trampoline` were about to be called -- a return address slot (0, never
    // used), the address `ret` takes in lxkit_switch, and the six
    // callee-saved registers it pops.
    let top = mapping.wrapping_add(MAPPING);
    let start = trampoline as extern "C" fn() -> !;
    let frame = [0usize, 0, 0, 0, 0, 0, start as usize, 0];
    let rsp = top.wrapping_sub(core::mem::size_of_val(&frame));
    // SAFETY: the frame's 64 bytes are the top of the stack just mapped.
    unsafe { core::ptr::copy_nonoverlapping(frame.as_ptr(), rsp as *mut usize, frame.len()) };
    // SAFETY: as above.
    unsafe {
        if let Some(t) = (*s).tasks.get_mut(i) {
            *t = Task {
                rsp,
                mapping,
                entry: Some(entry),
                arg,
                data,
            };
        }
    }
    i32::try_from(i).unwrap_or(-1)
}

/// `slx_task_current`: the running task's handle, -1 outside every task.
#[unsafe(no_mangle)]
pub extern "C" fn slx_task_current() -> i32 {
    // SAFETY: the one thread; a read.
    unsafe { (*sched()).current }
        .and_then(|i| i32::try_from(i).ok())
        .unwrap_or(-1)
}

/// `slx_task_block`: the running task waits -- until woken, or, when
/// `until_ns` is not 0, until the monotonic clock reaches it -- while the
/// others run. Returns when it runs again. Outside every task -- in an RCU
/// callback the loop runs, say -- nothing can wait, and returning would let
/// Linux code go on as if woken: the process ends as a BUG.
#[unsafe(no_mangle)]
pub extern "C" fn slx_task_block(until_ns: u64) {
    let s = sched();
    // SAFETY: the one thread, outside any switch.
    let Some(i) = (unsafe { (*s).current }) else {
        log(b"[lx] BUG: a wait outside every task\n");
        exit_with(crate::exit::PANIC);
    };
    // SAFETY: as above.
    unsafe { (*s).table.block(i, (until_ns != 0).then_some(until_ns)) };
    to_loop(i);
}

/// `slx_task_yield`: the running task lets the others run, and runs again
/// in its turn.
#[unsafe(no_mangle)]
pub extern "C" fn slx_task_yield() {
    // SAFETY: the one thread; a read.
    if let Some(i) = unsafe { (*sched()).current } {
        to_loop(i);
    }
}

/// `slx_task_wake`: task `handle`, if it waits, may run again; a runnable
/// or finished task, or none, is left as it is.
#[unsafe(no_mangle)]
pub extern "C" fn slx_task_wake(handle: i32) {
    if let Ok(i) = usize::try_from(handle) {
        // SAFETY: the one thread, outside any switch.
        unsafe { (*sched()).table.wake(i) };
    }
}

/// Run the tasks until nothing ever will again: whether every task
/// finished -- `false` when some wait with no deadline and nothing to wake
/// them.
pub fn run() -> bool {
    let s = sched();
    loop {
        let now = now_ns();
        // SAFETY: the C half's hook, outside every task; the one thread.
        unsafe { lx_emul_turn(now) };
        // SAFETY: the one thread, outside any switch.
        let woken = unsafe { (*s).table.wake_expired(now) };
        timed_out(woken);
        // SAFETY: as above.
        if let Some(i) = unsafe { (*s).table.pick() } {
            // SAFETY: as above; no reference is held across the switch.
            unsafe {
                (*s).current = Some(i);
                let t = (*s).tasks.get(i).copied().unwrap_or(NO_TASK);
                lx_emul_switch_to(t.data, now);
                let save = core::ptr::addr_of_mut!((*s).loop_rsp);
                lxkit_switch(save, t.rsp);
                (*s).current = None;
                if (*s).table.state(i) == State::Finished {
                    give_back_stack(t.mapping);
                    (*s).table.free(i);
                    if let Some(slot) = (*s).tasks.get_mut(i) {
                        *slot = NO_TASK;
                    }
                }
            }
            continue;
        }
        // SAFETY: as above.
        let task = unsafe { (*s).table.next_deadline() };
        // SAFETY: the C half's hook, outside every task; the one thread.
        let event = unsafe { lx_emul_next_event(now) };
        match task.into_iter().chain((event != 0).then_some(event)).min() {
            // From the clock read again: the turn's softirqs may have run long.
            Some(deadline) => sleep_ns(deadline.saturating_sub(now_ns())),
            None => return !stranded(),
        }
    }
}

/// Whether a task waits for what will never come: blocked with no deadline
/// and nothing to wake it, and not one that only waits for work (a daemon,
/// `lx_emul_task_is_daemon`). Asked when nothing can run again.
fn stranded() -> bool {
    let s = sched();
    // SAFETY: the one thread, outside any switch; the C half's records of
    // tasks that have not finished.
    unsafe {
        (*s).table.blocked().any(|i| {
            let data = (*s).tasks.get(i).map_or(core::ptr::null_mut(), |t| t.data);
            !lx_emul_task_is_daemon(data)
        })
    }
}

/// Tell the C half of each task in `woken` (a bit per slot) that its
/// deadline woke it: Linux's record then reads running, as a Linux timer's
/// wake leaves it, so a wake that comes before it runs finds it awake.
fn timed_out(woken: u64) {
    let s = sched();
    for i in 0..MAX_TASKS {
        let bit = u32::try_from(i)
            .ok()
            .and_then(|i| 1u64.checked_shl(i))
            .unwrap_or(0);
        if woken & bit == 0 {
            continue;
        }
        // SAFETY: the one thread, outside any switch; the C half's record
        // of a task that has not finished, as it gave it.
        unsafe {
            let data = (*s).tasks.get(i).map_or(core::ptr::null_mut(), |t| t.data);
            lx_emul_timed_out(data);
        }
    }
}
