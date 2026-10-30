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
/* lx_emul.c: the record `current` names between tasks, Linux's idle task's. */
extern struct task_struct init_task;
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
/* lx_emul.c: the first argument "<key><value>"'s value, or NULL. */
const char *lx_arg(const char *key);
/* lx_emul.c: whether the initcall at `fn`, answering `ret`, ends the process. */
bool lx_initcall_fatal(const void *fn, int ret);
/* lx_mm.c: Linux's direct map laid over the heap; 0, or -ENOMEM. And the
 * pages of `size` bytes kmalloc handed out at `p` marked slab memory. */
int lx_mm_init(void);
void lx_mm_mark_slab(const void *p, size_t size);
/* lx_mm.c: the pages of `size` bytes at `p`, handed out whole, said to be no slab's. */
void lx_mm_mark_pages(const void *p, size_t size);
/* lx_pci.c: whether a driver took every PCI function the process was given,
 * once probing is done; each it did not is said. And of the functions given,
 * at most LX_PCI_FUNCTIONS_MAX: `dev`'s place among them (-1 for none), and
 * the handle of the one at `i`. And whether `dev` is the device whose
 * arenas back the DMA window (the first function given). */
#define LX_PCI_FUNCTIONS_MAX 8
struct pci_dev;
struct device;
bool lx_pci_check_bound(void);
int lx_pci_index(const struct pci_dev *dev);
long lx_pci_handle(int i);
bool lx_pci_dma_device(const struct device *dev);
/* lx_irq.c: the interrupts that came since the last turn, handled; which
 * the loop is to wait for when idle, told the kit (just before it waits);
 * and how many times the handler of interrupt `irq` has run, for the tests. */
void lx_irq_poll(void);
void lx_irq_listen(void);
/* lx_irq.c: whether an interrupt owed while its handler was disabled waits
 * to be run now that it is enabled -- the next turn must come at once. */
bool lx_irq_due(void);
/* lx_hrtimer.c: the high-resolution timers due, run; and when the earliest
 * queued one is due by the monotonic clock reading `now_ns` (KTIME_MAX for
 * none). */
void lx_hrtimer_run(void);
u64 lx_hrtimer_next(u64 now_ns);
/* lx_softirq.c: run at the end of every turn once set, the clock reading
 * `now_ns` -- by lx_nic.c, to see to a card served to the network stack. */
extern void (*lx_emul_turn_hook)(u64 now_ns);
unsigned long lx_irq_count(unsigned int irq);
/* lx_irq.c: how many interrupts on `irq` were dropped, no handler requested. */
unsigned long lx_irq_dropped(unsigned int irq);
/* lx_irq.c: whether the kernel has interrupts of `dev`'s not yet taken,
 * looked at as lx_intx_poll() looks (slx_pci_irq_ready), for the tests. */
bool lx_irq_ready(struct pci_dev *dev);

/*
 * lx_netlink.c: the layer as a generic netlink user. A family registered,
 * by its name (NULL for none); a request to its command `cmd`, for the
 * caller to add attributes to (NULL for no memory); the request run as
 * genetlink runs a doit, its replies queued on `replies` -- the handler's
 * answer, the request still the caller's; a multicast group, by its name,
 * listened to (0, or -ENOENT for no such group); and the next message sent
 * to it, waiting up to `timeout` jiffies (NULL for none by then).
 */
struct genl_family;
struct sk_buff;
struct sk_buff_head;
const struct genl_family *lx_genl_family(const char *name);
struct sk_buff *lx_genl_request(const struct genl_family *family, u8 cmd);
int lx_genl_doit(const struct genl_family *family, struct sk_buff *req,
		 struct sk_buff_head *replies);
int lx_genl_listen(const struct genl_family *family, const char *name);
struct sk_buff *lx_genl_event(const struct genl_family *family, const char *name,
			      unsigned long timeout);

/*
 * Hardirq context, as Linux enters it for an interrupt -- interrupts off,
 * the count raised (<linux/hardirq.h>'s __irq_enter_raw()) -- and left: for
 * the devices' handlers and the turn's irq_work alike, so that both run in
 * the one context.
 */
#define lx_hardirq_enter(flags)		\
	do {				\
		local_irq_save(flags);	\
		__irq_enter_raw();	\
	} while (0)
#define lx_hardirq_exit(flags)		\
	do {				\
		__irq_exit_raw();	\
		local_irq_restore(flags);	\
	} while (0)

/* For the hello driver's tests: warnings said (lx_emul.c), and threads'
 * records not yet freed, daemons (kthreadd, idle workers) not counted
 * (lx_sched.c). */
extern unsigned long lx_warnings;
unsigned long lx_live_kthreads(void);

#endif /* LX_EMUL_H */
