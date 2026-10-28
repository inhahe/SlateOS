/* SPDX-License-Identifier: MIT */
/*
 * What the C half's own files share among themselves, beside Linux's
 * declarations (slx.h is the interface with the Rust half). It names Linux
 * types, so it is included after Linux's headers.
 */
#ifndef LX_EMUL_H
#define LX_EMUL_H

/* lx_emul.c: the first task's record, which `current` names while it runs. */
extern struct task_struct lx_main_task;
/* lx_sched.c: the first task's handle, to wake it by. */
extern int lx_main_handle;

/* lx_emul.c: jiffies brought up to the monotonic clock reading `now_ns`. */
void lx_emul_tick(u64 now_ns);
/* lx_emul.c: when the running task was switched to, by the same clock. */
extern u64 lx_switched_ns;

/* For the hello driver's tests: warnings said (lx_emul.c), and kernel
 * threads' records not yet freed (lx_sched.c). */
extern unsigned long lx_warnings;
extern unsigned long lx_kthreads;

#endif /* LX_EMUL_H */
