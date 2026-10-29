/* SPDX-License-Identifier: MIT */
/*
 * slx -- the interface between the driver layer's two halves
 * (design-decisions 1578 rule 3).
 *
 * The Linux-facing half (C, `linuxdrv/emul/`) implements Linux's functions
 * in terms of these; the system-facing half (Rust, `linuxdrv/kit/`)
 * implements these on SlateOS. Nothing here names a Linux type, so the
 * Rust half never needs a Linux header, and this file includes none.
 */
#ifndef SLX_H
#define SLX_H

/* Write `len` bytes of text to the console, as one line or part of one. */
void slx_log(const char *text, unsigned long len);

/*
 * Memory: `size` bytes, aligned to `size` rounded up to a power of two
 * (kmalloc's own promise), or NULL. The contents are not cleared.
 */
void *slx_alloc(unsigned long size);
/* Give back what slx_alloc gave; NULL is ignored. */
void slx_free(const void *ptr);
/*
 * The bytes slx_alloc(size) hands out -- `size` rounded up as the heap
 * rounds it (a power of two, 16 at least) -- or `size` where the heap could
 * never hold that much; and the bytes the block slx_alloc gave at `ptr`
 * holds, 0 for anything it did not give.
 */
unsigned long slx_alloc_size(unsigned long size);
unsigned long slx_block_size(const void *ptr);

/* Fill `len` bytes at `buf` from the system's CSPRNG, or stop the driver. */
void slx_random(void *buf, unsigned long len);

/* Nanoseconds since some fixed moment, never going back. */
unsigned long long slx_now_ns(void);

/* The process's arguments: how many, and the n-th (NULL past the last). */
int slx_argc(void);
const char *slx_argv(int n);

/* End the driver process with `code`. */
void slx_exit(int code) __attribute__((noreturn));

/*
 * End it because Linux code reached `what`, a function the layer does not
 * implement, called from `caller`: said on the console -- the caller's
 * address for the driver's symbol table -- then exit code SLX_EXIT_STOP.
 */
void slx_stop(const char *what, const void *caller) __attribute__((noreturn));

/*
 * The exit codes a driver process ends with; each means one thing, so that
 * the kernel can tell them apart (kernel/src/linuxdrv.rs, lxkit::exit).
 */
#define SLX_EXIT_OK 0
#define SLX_EXIT_NO_CLOCK 120		/* the system gave no clock */
#define SLX_EXIT_BUG 121		/* Linux code called BUG() */
#define SLX_EXIT_PANIC 122		/* the layer's Rust half panicked */
#define SLX_EXIT_NO_RANDOM 123		/* the system gave no random bytes */
#define SLX_EXIT_DEADLOCK 124		/* every task waits on what will never happen */
#define SLX_EXIT_STOP 125		/* a generated stop was reached */
#define SLX_EXIT_INIT_FAILED 126	/* an initcall failed */
#define SLX_EXIT_NO_MEMORY 127		/* the process's memory could not be set up */

/*
 * Tasks: coroutines on the process's one thread (design-decisions 1578
 * rule 5), each run until it blocks, yields or returns.
 *
 * slx_task_new: a new runnable task that will run entry(arg); `data` is
 * the C half's record of it (its task_struct), handed back at each switch.
 * Its handle, or -1. slx_task_block: the running task waits until woken,
 * or until the monotonic clock reaches `until_ns` when that is not 0.
 * slx_task_wake: task `handle`, if it waits, may run again.
 */
int slx_task_new(void (*entry)(void *), void *arg, void *data);
/*
 * A stack for the C half's own use, mapped with a guard below it as a
 * task's is: its top, or NULL. Softirqs run on one.
 */
void *slx_stack_top(void);
/* The size of a task's stack, and of the no-access guard below each. */
extern const unsigned long slx_stack_bytes, slx_guard_bytes;
int slx_task_current(void);
void slx_task_block(unsigned long long until_ns);
void slx_task_yield(void);
/* The running task ends: finished, and never run again. */
void slx_task_exit(void) __attribute__((noreturn));
void slx_task_wake(int handle);

/* Implemented by the C half, called by the Rust half. */
/*
 * Set up what must be ready before the loop first turns: SLX_EXIT_OK, or
 * the code the process exits with.
 */
int lx_emul_init(void);
/*
 * Run the driver's initcalls, as the first task: what the process exits
 * with, at once when it is not 0.
 */
int lx_emul_main(void);
/*
 * The task whose record is `task` (NULL: the first) is about to run, the
 * monotonic clock reading `now_ns`.
 */
void lx_emul_switch_to(void *task, unsigned long long now_ns);
/* The task whose record is `task` (NULL: the first) waited past its deadline. */
void lx_emul_timed_out(void *task);
/*
 * Whether the task whose record is `task` waits for work rather than for
 * an event (kthreadd, a workqueue's workers): left waiting when every other
 * task has finished, it does not keep the process from being done.
 */
_Bool lx_emul_task_is_daemon(void *task);
/*
 * A turn of the loop, outside every task, the monotonic clock reading
 * `now_ns`: jiffies, RCU callbacks, timers and softirqs seen to.
 */
void lx_emul_turn(unsigned long long now_ns);
/*
 * With no task to run, the clock reading `now_ns`: when the loop must turn
 * again, by the same clock -- `now_ns` while work is pending, else when the
 * next timer is due -- or 0, never.
 */
unsigned long long lx_emul_next_event(unsigned long long now_ns);

#endif /* SLX_H */
