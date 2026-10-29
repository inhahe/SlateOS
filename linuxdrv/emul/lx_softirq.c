// SPDX-License-Identifier: MIT
/*
 * Softirqs and timers in a driver process: Linux's own -- kernel/softirq.c,
 * tasklets with it, and kernel/time/timer.c's timer wheel, compiled in as
 * they are -- with the Rust half's loop as their tick and their ksoftirqd
 * (design-decisions 1578 rule 5, 1581).
 *
 * At every turn of the loop, between tasks, lx_emul_turn() does what
 * Linux's tick and ksoftirqd do: brings jiffies up to the clock, raises the
 * timer softirq whenever they have moved -- as a tick does on a busy CPU,
 * so that deferrable timers run on the first turn after their jiffy as
 * they would on the first tick -- runs the RCU callbacks whose grace period
 * has passed, and runs what softirqs are pending. When the loop has no
 * task to run, lx_emul_next_event() says how long it may sleep: until the
 * next timer, by Linux's NO_HZ_IDLE accounting (get_next_timer_interrupt,
 * which, as on Linux, does not wake an idle process for a deferrable
 * timer), not at all while softirqs or RCU callbacks are still pending,
 * and for ever when there is nothing -- which is how a process that can
 * never be woken is told from one waiting for a timer.
 *
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
#include <linux/irq_work.h>
#include <linux/tick.h>

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

/*
 * Per-CPU hotplug threads: the loop is the process's ksoftirqd, so
 * softirq.c's registration of it needs no thread. Any other registration
 * would want threads that would never run: a stop, which says so.
 */
int smpboot_register_percpu_thread(struct smp_hotplug_thread *plug_thread)
{
	if (plug_thread->store == &ksoftirqd)
		return 0;
	slx_stop("smpboot_register_percpu_thread (a thread other than ksoftirqd)",
		 __builtin_return_address(0));
}

/*
 * A CPU hotplug state: the process's one CPU is online and never goes, so
 * the state's startup runs for it now when asked to (`invoke`), as Linux's
 * runs it on every online CPU, and its teardown never. A dynamic state
 * answers a state number, as Linux's allocation does; a multi-instance
 * state's callbacks run per instance, not here.
 */
int __cpuhp_setup_state(enum cpuhp_state state, const char *name, bool invoke,
			int (*startup)(unsigned int cpu),
			int (*teardown)(unsigned int cpu), bool multi_instance)
{
	int ret = 0;

	if (invoke && startup && !multi_instance)
		ret = startup(0);
	if (ret)
		return ret;
	return state == CPUHP_AP_ONLINE_DYN || state == CPUHP_BP_PREPARE_DYN ? state : 0;
}

/* No POSIX CPU timers, and no high-resolution timers, so none is ever due. */
void posix_cputimers_init_work(void)
{
}

u64 hrtimer_get_next_event(void)
{
	return KTIME_MAX;
}

/*
 * The tick never stops: every turn of the loop is one, and looks at the
 * irq_work lists (lx_emul_turn) -- and the loop does not sleep while either
 * holds work (lx_emul_next_event) -- so irq_work queued lazily, for the next
 * tick, is run by it.
 */
bool tick_nohz_tick_stopped(void)
{
	return false;
}

/* An RCU quiescent state: a switch is the grace period here (lx_sched.c). */
void rcu_qs(void)
{
}

/*
 * Softirqs and timers made ready, before the loop's first turn, as Linux's
 * start_kernel() makes them before anything runs: the softirqs' stack, the
 * tasklet lists, the timer wheel. 0, or -ENOMEM when no stack could be
 * mapped.
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
 * sched.rs), the monotonic clock reading `now_ns`: what Linux's tick and
 * ksoftirqd would do (above). The task that ran last is charged its run
 * time, as Linux's scheduler charges it at a switch; `current` is then the
 * idle record (lx_emul.c), which is no task's and never freed.
 */
void lx_emul_turn(u64 now_ns)
{
	static unsigned long ticked = INITIAL_JIFFIES;

	lx_emul_account(now_ns);
	*raw_cpu_ptr(&current_task) = &lx_idle_task;
	lx_emul_jiffies_at(now_ns);
	if (jiffies != ticked) {
		ticked = jiffies;
		raise_softirq(TIMER_SOFTIRQ);
	}
	/*
	 * In softirq context, as Linux's RCU runs its callbacks from its
	 * softirq: one that would sleep is a bug, and ends the process as one
	 * (lx_may_sleep), rather than blocking the loop.
	 */
	preempt_count_add(SOFTIRQ_OFFSET);
	lx_rcu_run();
	preempt_count_sub(SOFTIRQ_OFFSET);
	/*
	 * irq_work, as Linux's tick runs it where nothing can raise it
	 * sooner (update_process_times -> irq_work_tick): in hardirq context,
	 * interrupts off.
	 */
	local_irq_disable();
	preempt_count_add(HARDIRQ_OFFSET);
	irq_work_tick();
	preempt_count_sub(HARDIRQ_OFFSET);
	local_irq_enable();
	if (local_softirq_pending())
		do_softirq();
}

/*
 * The Rust half's loop, with no task to run, the clock reading `now_ns`:
 * when it must turn again -- `now_ns` while softirqs (more than one turn's
 * worth) or RCU callbacks are still pending, else when the next timer is
 * due -- or 0, never, when nothing is pending at all.
 */
u64 lx_emul_next_event(u64 now_ns)
{
	u64 began, next;

	if (local_softirq_pending() || lx_rcu_pending() || irq_work_needs_cpu())
		return now_ns;
	/* The wheel reckons from when the current jiffy began, as Linux's tick. */
	began = lx_emul_jiffy_began_ns();
	next = get_next_timer_interrupt(jiffies, began);
	if (next == KTIME_MAX)
		return 0;
	/* A timer due already runs on the next jiffy's turn, as on the next tick. */
	return max(next, began + TICK_NSEC);
}
