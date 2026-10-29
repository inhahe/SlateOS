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
/* lx_emul.c: the record `current` names between tasks: no task's. */
extern struct task_struct lx_idle_task;
/* lx_sched.c: the first task's handle, to wake it by. */
extern int lx_main_handle;

/* lx_emul.c: jiffies brought up to the monotonic clock, read now or `now_ns`. */
void lx_emul_jiffies_now(void);
void lx_emul_jiffies_at(u64 now_ns);
/* lx_emul.c: when, by the monotonic clock, the current jiffy began. */
u64 lx_emul_jiffy_began_ns(void);
/* lx_softirq.c: softirqs, tasklets and timers made ready; 0, or -ENOMEM. */
int lx_softirq_init(void);
/* lx_sched.c: the RCU callbacks queued so far, run: every task is switched out. */
void lx_rcu_run(void);
/* lx_sched.c: whether any RCU callback waits to run. */
bool lx_rcu_pending(void);
/* lx_sched.c: Linux's kthreadd started, as a task; 0, or -ENOMEM. */
int lx_start_kthreadd(void);
/* lx_emul.c: when the running task was switched to, by the monotonic clock. */
extern u64 lx_switched_ns;
/* lx_emul.c: the running task's run time counted up to `now_ns`. */
void lx_emul_account(u64 now_ns);
/* lx_fs.c: the objects Linux's own sysfs set-up makes; 0, or -ENOMEM. */
int lx_fs_init(void);
/* lx_emul.c: whether the initcall at `fn`, answering `ret`, ends the process. */
bool lx_initcall_fatal(const void *fn, int ret);
/* lx_mm.c: Linux's direct map laid over the heap; 0, or -ENOMEM. And the
 * pages of `size` bytes kmalloc handed out at `p` marked slab memory. */
int lx_mm_init(void);
void lx_mm_mark_slab(const void *p, size_t size);
/* lx_pci.c: whether a driver took every PCI function the process was given,
 * once probing is done; each it did not is said. */
bool lx_pci_check_bound(void);

/* For the hello driver's tests: warnings said (lx_emul.c), and threads'
 * records not yet freed, daemons (kthreadd, idle workers) not counted
 * (lx_sched.c). */
extern unsigned long lx_warnings;
unsigned long lx_live_kthreads(void);

#endif /* LX_EMUL_H */
