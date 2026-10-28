//! The driver process's tasks running: their stacks, the switch between
//! them, and the loop that picks the next or sleeps until one may run
//! (design-decisions 1578 rule 5). Which may run is [`crate::task`]'s.
//!
//! A task runs until it blocks, yields or returns; then the loop runs
//! another. Before it switches to a task, it tells the C half which, and
//! the time (`lx_emul_switch_to`, which sets Linux's `current` and advances
//! `jiffies`). With nothing runnable it sleeps until the earliest deadline;
//! with nothing runnable and no deadline, nothing will ever run again, and
//! the process ends.

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
}

static SCHED: Single<Sched> = Single::new(Sched {
    table: Table::new(),
    tasks: [NO_TASK; MAX_TASKS],
    current: None,
    loop_rsp: 0,
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
    let (i, entry, arg) = unsafe {
        let i = (*s).current.unwrap_or(0);
        let t = (*s).tasks.get(i).copied().unwrap_or(NO_TASK);
        (i, t.entry, t.arg)
    };
    if let Some(entry) = entry {
        // SAFETY: the C half's function and its argument, as it gave them.
        unsafe { entry(arg) };
    }
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

/// `slx_task_new`: a new task, runnable, that will run `entry(arg)`; `data`
/// is the C half's record of it. Its handle, or -1 when no slot or stack is
/// left.
#[unsafe(no_mangle)]
pub extern "C" fn slx_task_new(entry: Entry, arg: *mut c_void, data: *mut c_void) -> i32 {
    let Some(mapping) = map_stack() else {
        return -1;
    };
    let s = sched();
    // SAFETY: the one thread, outside any switch.
    let Some(i) = (unsafe { (*s).table.add() }) else {
        unmap_stack(mapping);
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
/// others run. Returns when it runs again. Outside every task, nothing.
#[unsafe(no_mangle)]
pub extern "C" fn slx_task_block(until_ns: u64) {
    let s = sched();
    // SAFETY: the one thread, outside any switch.
    let Some(i) = (unsafe { (*s).current }) else {
        return;
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

/// Run the tasks until nothing ever will again: every task finished, or
/// every one left waits with no deadline and nothing to wake it.
pub fn run() {
    let s = sched();
    loop {
        let now = now_ns();
        // SAFETY: the one thread, outside any switch.
        unsafe { (*s).table.wake_expired(now) };
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
                    unmap_stack(t.mapping);
                    (*s).table.free(i);
                    if let Some(slot) = (*s).tasks.get_mut(i) {
                        *slot = NO_TASK;
                    }
                }
            }
            continue;
        }
        // SAFETY: as above.
        match unsafe { (*s).table.next_deadline() } {
            Some(deadline) => sleep_ns(deadline.saturating_sub(now)),
            None => return,
        }
    }
}
