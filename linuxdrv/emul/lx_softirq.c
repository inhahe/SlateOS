// SPDX-License-Identifier: MIT
/*
 * Softirqs and timers in a driver process: Linux's own -- kernel/softirq.c,
 * tasklets with it, and kernel/time/timer.c's timer wheel, compiled in as
 * they are -- with the Rust half's loop as their tick and their ksoftirqd
 * (design-decisions 1578 rule 5, 1581).
 *
 * At every turn of the loop, between tasks, lx_emul_turn() brings jiffies
 * up to the clock, runs the RCU callbacks whose grace period has passed,
 * raises the timer softirq when a timer is due, and runs what softirqs
 * are pending -- where Linux runs them on the way out of its timer
 * interrupt, or in ksoftirqd. It answers when the next timer is due, which
 * the loop sleeps no longer than: Linux's NO_HZ_IDLE accounting
 * (get_next_timer_interrupt), so a process with nothing to do but wait for
 * a timer sleeps until it, and one with nothing at all can be seen to.
 * Softirqs raised in a task run at local_bh_enable(), as on Linux, or at the
 * next turn; they run on a stack of their own, as x86-64 Linux runs them
 * (hardirq_stack_ptr), which the Rust half maps with a guard below it.
 */
#include <linux/interrupt.h>
#include <linux/jiffies.h>
#include <linux/kernel_stat.h>
#include <linux/jump_label.h>
#include <linux/hrtimer.h>
#include <linux/posix-timers.h>
#include <linux/smpboot.h>
#include <linux/cpuhotplug.h>
#include <linux/workqueue.h>
#include <linux/timer.h>
#include <linux/sched.h>
#include <linux/percpu.h>

#include "slx.h"
#include "lx_emul.h"

/* kernel/time/tick-internal.h's, which a driver file does not include. */
u64 get_next_timer_interrupt(unsigned long basej, u64 basem);

/*
 * The data Linux's softirq.c and timer.c read, which the kernel proper
 * defines elsewhere (arch/x86/kernel/irq*.c, kernel/sched/core.c,
 * kernel/irq/manage.c, kernel/jump_label.c, kernel/workqueue.c). Per-CPU
 * variables are plain globals with this configuration (no SMP).
 */
DEFINE_PER_CPU_CACHE_HOT(u16, __softirq_pending);
DEFINE_PER_CPU_CACHE_HOT(struct irq_stack *, hardirq_stack_ptr);
DEFINE_PER_CPU_CACHE_HOT(bool, hardirq_stack_inuse);
DEFINE_PER_CPU(struct kernel_stat, kstat);
/* Interrupt handlers are not forced into threads. */
DEFINE_STATIC_KEY_FALSE(force_irqthreads_key);
/* Static keys are plain variables here: nothing to patch, so ready at once. */
bool static_key_initialized = true;
/* No workqueues yet: timer.c names one only where NO_HZ modes change. */
struct workqueue_struct *system_percpu_wq;

/*
 * What Linux's softirq.c and timer.c call at their start. The loop is the
 * process's ksoftirqd and its CPU never goes away, so there is no thread
 * to register and no hotplug state to set up; no POSIX CPU timers, and no
 * high-resolution timers, so none is ever due.
 */
int smpboot_register_percpu_thread(struct smp_hotplug_thread *plug_thread)
{
	return 0;
}

int __cpuhp_setup_state(enum cpuhp_state state, const char *name, bool invoke,
			int (*startup)(unsigned int cpu),
			int (*teardown)(unsigned int cpu), bool multi_instance)
{
	return 0;
}

void posix_cputimers_init_work(void)
{
}

u64 hrtimer_get_next_event(void)
{
	return KTIME_MAX;
}

/* An RCU quiescent state: a switch is the grace period here (lx_sched.c). */
void rcu_qs(void)
{
}

/* Softirq-run work items: no workqueues yet, so none is queued. */
void workqueue_softirq_action(bool highpri)
{
}

/*
 * Softirqs and timers made ready, before the first initcall, as Linux's
 * start_kernel() makes them: the softirqs' stack, the tasklet lists, the
 * timer wheel. 0, or -ENOMEM when no stack could be mapped.
 */
int lx_softirq_init(void)
{
	char *top = slx_stack_top();

	if (!top)
		return -ENOMEM;
	/* As Linux's irq_init_percpu_irqstack(): the top word holds the old %rsp. */
	*raw_cpu_ptr(&hardirq_stack_ptr) = (struct irq_stack *)(top - 8);
	softirq_init();
	timers_init();
	return 0;
}

/*
 * The Rust half's loop, at every turn, outside every task (linuxdrv/kit,
 * sched.rs), the monotonic clock reading `now_ns`: the time and the work
 * Linux's tick and ksoftirqd would see to (above). `current` is the idle
 * record meanwhile (lx_emul.c), which is no task's and never freed. The
 * monotonic time the next timer is due, or 0 for none.
 */
u64 lx_emul_turn(u64 now_ns)
{
	u64 next;

	*raw_cpu_ptr(&current_task) = &lx_idle_task;
	lx_emul_jiffies_at(now_ns);
	lx_rcu_run();
	/* A timer is due when its jiffy has come: the wheel answers `now`. */
	if (get_next_timer_interrupt(jiffies, now_ns) <= now_ns)
		raise_softirq(TIMER_SOFTIRQ);
	if (local_softirq_pending())
		do_softirq();
	/* Asked again: a timer's function may have armed another. */
	next = get_next_timer_interrupt(jiffies, now_ns);
	return next == KTIME_MAX ? 0 : max(next, 1ULL);
}
