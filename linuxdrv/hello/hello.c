// SPDX-License-Identifier: MIT
/*
 * hello -- the driver layer's first driver process, and no driver: a Linux
 * initcall that allocates, takes a spinlock with interrupts "off", formats a
 * line through Linux's own vsprintf and frees, then others that run Linux's
 * tasks over the layer's scheduler, in a process of its own as any Linux
 * driver will be (roadmap.md 2.1, design-decisions 1580).
 *
 * The kernel's self-test (`linuxdrv::self_test`) runs it plainly, when it
 * must exit 0 having printed its lines, and with one argument, which makes
 * the first initcall end the process another way:
 *   "stop"      calls a function nothing defines -- which the build turns
 *               into a generated stop -- so must exit SLX_EXIT_STOP;
 *   "deadlock"  waits for what never comes, with no other task to run, so
 *               must exit SLX_EXIT_DEADLOCK;
 *   "overflow"  runs past the end of its task's stack, so must be killed
 *               there by the guard page below it;
 *   "fail"      starts a kthread that never ends and fails, so must exit
 *               SLX_EXIT_INIT_FAILED at once;
 *   "stranded"  succeeds, leaving a kthread waiting for what never comes,
 *               so must exit SLX_EXIT_DEADLOCK, not 0;
 *   "atomic"    sleeps with bottom halves off, so must exit SLX_EXIT_BUG.
 * With an argument, the later initcalls do nothing: the first's ending is
 * the test.
 */
#include <linux/module.h>
#include <linux/slab.h>
#include <linux/printk.h>
#include <linux/spinlock.h>
#include <linux/irqflags.h>
#include <linux/string.h>
#include <linux/kthread.h>
#include <linux/mutex.h>
#include <linux/completion.h>
#include <linux/delay.h>
#include <linux/jiffies.h>
#include <linux/sched.h>
#include <linux/sched/task.h>
#include <linux/sched/wake_q.h>
#include <linux/rcupdate.h>
#include <linux/err.h>
#include <linux/timer.h>
#include <linux/interrupt.h>
#include <linux/bottom_half.h>
#include <linux/preempt.h>
#include <linux/cpuhotplug.h>
#include <asm/processor.h>

#include "../emul/slx.h"
#include "../emul/lx_emul.h"

/* Kernel threads' records not yet freed, less kthreadd's, which never is. */
#define HELLO_KTHREADS() (lx_kthreads - 1)

/* Defined nowhere: the build makes it a generated stop. */
void lx_hello_never_defined(void);

/* Never completed. */
static DECLARE_COMPLETION(hello_forever);

/* Whether the process's argument is `mode`. */
static bool hello_mode(const char *mode)
{
	return slx_argc() > 1 && strcmp(slx_argv(1), mode) == 0;
}

/* A warning from one place, given twice: said once, and the driver goes on. */
static void hello_warn_once(void)
{
	WARN_ON_ONCE(1);
}

/*
 * About `depth` KiB of stack, used and given back: a frame of a KiB each.
 * hello_overflow_kib() of it runs past a task's stack but not past the
 * guard below it (slx_stack_bytes, slx_guard_bytes), so that it returns,
 * having written over memory, only where the guard is missing.
 */
static int hello_overflow_kib(void)
{
	return (slx_stack_bytes + slx_guard_bytes / 4) / 1024;
}

static noinline int hello_deep(int depth)
{
	volatile char frame[1024];

	frame[0] = depth;
	frame[sizeof(frame) - 1] = depth;
	if (depth > 0)
		frame[0] += hello_deep(depth - 1);
	return frame[0] + frame[sizeof(frame) - 1];
}

/* A kthread that polls until it is stopped, which nothing does. */
static int hello_polls(void *unused)
{
	while (!kthread_should_stop())
		msleep(100);
	return 0;
}

/* A kthread that waits for what never comes. */
static int hello_waits_forever(void *unused)
{
	wait_for_completion(&hello_forever);
	return 0;
}

static DEFINE_SPINLOCK(hello_lock);

static int __init hello_init(void)
{
	unsigned long flags, warned;
	bool off;
	int *answer, *dirty = kmalloc(sizeof(*answer), GFP_KERNEL);

	/*
	 * kzalloc clears: a block written and given back is the one the next
	 * allocation of its size gets (the free list is last in, first out),
	 * so a zero read there is the clearing's and not the memory's.
	 */
	if (!dirty)
		return -ENOMEM;
	*dirty = 0x5a5a;
	kfree(dirty);
	answer = kmalloc(sizeof(*answer), GFP_KERNEL | __GFP_ZERO);
	if (!answer)
		return -ENOMEM;
	if (answer != dirty || *answer != 0) {
		kfree(answer);
		return -EINVAL;
	}
	warned = lx_warnings;
	hello_warn_once();
	hello_warn_once();
	if (lx_warnings != warned + 1) {
		kfree(answer);
		return -EDOM;
	}
	spin_lock_irqsave(&hello_lock, flags);
	off = irqs_disabled();
	*answer = 42;
	spin_unlock_irqrestore(&hello_lock, flags);
	if (!off || irqs_disabled()) {
		kfree(answer);
		return -EFAULT;
	}
	pr_info("hello from Linux's own headers: %d, %s, %#x\n", *answer, "formatted by vsprintf", 0xbeef);
	kfree(answer);
	if (hello_mode("stop"))
		lx_hello_never_defined();
	if (hello_mode("deadlock"))
		wait_for_completion(&hello_forever);
	if (hello_mode("overflow")) {
		hello_deep(hello_overflow_kib());
		pr_err("hello: %d KiB of stack used, past the guard, and nothing faulted\n",
		       hello_overflow_kib());
		return -EFAULT;
	}
	if (hello_mode("fail")) {
		/* Not -ENODEV without its kthread: the run would prove nothing. */
		if (IS_ERR(kthread_run(hello_polls, NULL, "hello-polls")))
			return -EPROTO;
		return -ENODEV;
	}
	if (hello_mode("stranded") &&
	    IS_ERR(kthread_run(hello_waits_forever, NULL, "hello-stranded")))
		return -EPROTO;
	if (hello_mode("atomic")) {
		local_bh_disable();
		msleep(1);
		local_bh_enable();
		pr_err("hello: slept with bottom halves off, and nothing said so\n");
		return -EPROTO;
	}
	return 0;
}
module_init(hello_init);

/*
 * Tasks: a kernel thread, a mutex held across a sleep, a completion --
 * Linux's own mutex.c and completion.c over the layer's scheduler. The
 * worker starts while the first task holds the mutex, blocks on it, takes
 * it when the first task lets go after sleeping, and completes; the steps
 * must come in that order, the sleep must have taken its time, and
 * kthread_stop() must hand back what the worker returned.
 */
static DECLARE_COMPLETION(hello_worker_done);
static DEFINE_MUTEX(hello_mutex);
static int hello_steps[4];
static int hello_step_count;

static void hello_step(int step)
{
	if (hello_step_count < ARRAY_SIZE(hello_steps))
		hello_steps[hello_step_count] = step;
	hello_step_count++;
}

static int hello_worker(void *unused)
{
	hello_step(2);
	mutex_lock(&hello_mutex);
	hello_step(4);
	mutex_unlock(&hello_mutex);
	complete(&hello_worker_done);
	while (!kthread_should_stop()) {
		set_current_state(TASK_INTERRUPTIBLE);
		if (kthread_should_stop())
			break;
		schedule();
	}
	__set_current_state(TASK_RUNNING);
	return 7;
}

static int __init hello_tasks_init(void)
{
	unsigned long start = jiffies;
	struct task_struct *worker;
	int i, ret;

	if (slx_argc() > 1)
		return 0;
	mutex_lock(&hello_mutex);
	hello_step(1);
	worker = kthread_run(hello_worker, NULL, "hello-worker");
	if (IS_ERR(worker)) {
		mutex_unlock(&hello_mutex);
		return PTR_ERR(worker);
	}
	msleep(20);
	hello_step(3);
	mutex_unlock(&hello_mutex);
	if (!wait_for_completion_timeout(&hello_worker_done, HZ))
		return -ETIMEDOUT;
	ret = kthread_stop(worker);
	if (ret != 7)
		return -EINVAL;
	if (time_before(jiffies, start + msecs_to_jiffies(20)))
		return -ERANGE;
	for (i = 0; i < ARRAY_SIZE(hello_steps); i++)
		if (hello_step_count != ARRAY_SIZE(hello_steps) || hello_steps[i] != i + 1)
			return -EPROTO;
	pr_info("hello tasks: a kthread, a mutex held across a 20 ms sleep, a completion: %lu jiffies\n",
		jiffies - start);
	return 0;
}
late_initcall(hello_tasks_init);

/*
 * The scheduler's edges: a wait that times out; kthreads stopped before
 * they ran, and out of interruptible waits, as Linux's kthread_stop() does
 * by signal, and then not put back to sleep; their records freed when they
 * end by themselves, or when the last reference goes -- by kthread_stop_put(),
 * by put_task_struct() after an RCU grace period, by a wake queue; RCU
 * callbacks waiting for a grace period; usleep_range(0, ...), cond_resched() and yield()
 * letting the others run; and jiffies moving on through busy waits, and
 * before a sleep reckons its end, as Linux's timer interrupt keeps them.
 * Each check says which failed.
 */
#define HELLO_CHECK(cond, what)						\
	do {								\
		if (!(cond)) {						\
			pr_err("hello: check failed: %s\n", what);	\
			return -EPROTO;					\
		}							\
	} while (0)

static int hello_never_runs(void *ran)
{
	*(bool *)ran = true;
	return 0;
}

static int hello_waits(void *unused)
{
	return wait_for_completion_interruptible(&hello_forever);
}

static int hello_naps(void *unused)
{
	return msleep_interruptible(10000);
}

static int hello_quick(void *flag)
{
	WRITE_ONCE(*(bool *)flag, true);
	return 5;
}

static int hello_wakes(void *task)
{
	wake_up_process(task);
	return 0;
}

/*
 * Stopped while in an uninterruptible sleep, which goes on: then neither a
 * bare interruptible schedule() nor schedule_timeout_interruptible() may
 * sleep, the stop's signal pending.
 */
static int hello_sleeps_then_waits(void *unused)
{
	msleep(10);
	set_current_state(TASK_INTERRUPTIBLE);
	schedule();
	return schedule_timeout_interruptible(10 * HZ) > 9 * HZ ? 3 : -ETIME;
}

static struct rcu_head hello_rcu;
static bool hello_rcu_ran;

static void hello_rcu_done(struct rcu_head *head)
{
	hello_rcu_ran = true;
}

/* A wake of `task`, and what try_to_wake_up() answered: 1 if it slept. */
struct hello_wake {
	struct task_struct *task;
	int woke;
};

static int hello_wakes_and_says(void *arg)
{
	struct hello_wake *wake = arg;

	wake->woke = wake_up_process(wake->task);
	return 0;
}

static int hello_naps_briefly(void *unused)
{
	set_current_state(TASK_INTERRUPTIBLE);
	schedule_timeout(1);
	return 0;
}

/* A sleep of `timeout` jiffies, and whether it has ended. */
struct hello_sleep {
	long timeout;
	bool woke;
};

static int hello_sleeps_long(void *arg)
{
	struct hello_sleep *sleep = arg;

	schedule_timeout_uninterruptible(sleep->timeout);
	WRITE_ONCE(sleep->woke, true);
	return 0;
}

static int __init hello_edges_init(void)
{
	struct task_struct *never, *waiter, *napper, *quick, *held, *waker, *sleepers[2], *k;
	struct hello_wake wake = { NULL, -1 };
	DEFINE_WAKE_Q(wake_q);
	/*
	 * The longest sleep there is, and one whose nanoseconds (at HZ 250,
	 * 4,000,000 a jiffy, 2^8 * 15625) are 2^64 * 15625: both too long to end.
	 */
	struct hello_sleep sleeps[2] = { { MAX_JIFFY_OFFSET }, { 1L << 56 } };
	bool ran = false, quick_ran = false, held_ran = false;
	bool resched_ran = false, yield_ran = false, put_ran = false, queued_ran = false;
	unsigned long j;
	u64 t0, t1, give_up;
	long left, elapsed;
	int i;

	if (slx_argc() > 1)
		return 0;
	/* kthread_stop() lets its record go through RCU: freed by the next turn. */
	msleep(1);
	HELLO_CHECK(HELLO_KTHREADS() == 0, "a stopped kthread's record was not freed");

	j = jiffies;
	HELLO_CHECK(wait_for_completion_timeout(&hello_forever, 2) == 0,
		    "a wait that timed out said it had not");
	HELLO_CHECK(!time_before(jiffies, j + 2), "a timed-out wait ended early");

	/*
	 * A sleep reckons its end from jiffies brought up to the clock: after
	 * 120 ms that moved nothing on (a spin without cpu_relax()), 30
	 * jiffies, a sleep of 60 woken at once has 60 left less the jiffies
	 * the sleep itself took -- not 30 less them. Measured, so that a busy
	 * system slows the sleep without failing the check; the check can
	 * tell the two apart while the sleep takes under 30 jiffies.
	 */
	t0 = slx_now_ns();
	while (slx_now_ns() - t0 < 120 * NSEC_PER_MSEC)
		barrier();
	waker = kthread_run(hello_wakes, current, "hello-wakes");
	HELLO_CHECK(!IS_ERR(waker), "no kthread");
	t1 = slx_now_ns();
	set_current_state(TASK_INTERRUPTIBLE);
	left = schedule_timeout(60);
	elapsed = (slx_now_ns() - t1) / (NSEC_PER_SEC / HZ) + 1;
	HELLO_CHECK(left >= 60 - elapsed,
		    "a sleep woken at once reckoned its end from stale jiffies");

	for (i = 0; i < ARRAY_SIZE(sleeps); i++) {
		sleepers[i] = kthread_run(hello_sleeps_long, &sleeps[i], "hello-sleeps");
		HELLO_CHECK(!IS_ERR(sleepers[i]), "no kthread");
	}
	msleep(5);
	HELLO_CHECK(!READ_ONCE(sleeps[0].woke), "a sleep of MAX_JIFFY_OFFSET ended by itself");
	HELLO_CHECK(!READ_ONCE(sleeps[1].woke), "a sleep of 2^56 jiffies ended by itself");
	for (i = 0; i < ARRAY_SIZE(sleeps); i++)
		HELLO_CHECK(kthread_stop(sleepers[i]) == 0 && sleeps[i].woke,
			    "a long sleep was not woken");

	/*
	 * A task its deadline woke reads awake before it runs, as a Linux
	 * timer's wake leaves it: a wake that reaches it first answers 0. The
	 * waker is made first, so it takes the lower slot and runs first;
	 * the main task spins past the napper's deadline, then lets both go.
	 */
	k = kthread_create(hello_wakes_and_says, &wake, "hello-checks");
	HELLO_CHECK(!IS_ERR(k), "no kthread");
	wake.task = kthread_run(hello_naps_briefly, NULL, "hello-naps-briefly");
	HELLO_CHECK(!IS_ERR(wake.task), "no kthread");
	yield();
	t0 = slx_now_ns();
	while (slx_now_ns() - t0 < 10 * NSEC_PER_MSEC)
		barrier();
	wake_up_process(k);
	msleep(1);
	HELLO_CHECK(wake.woke == 0, "a task its deadline woke still read asleep");

	never = kthread_create(hello_never_runs, &ran, "hello-never");
	HELLO_CHECK(!IS_ERR(never), "no kthread");
	HELLO_CHECK(kthread_stop(never) == -EINTR && !ran,
		    "a kthread stopped before it ran did not end unrun, with -EINTR");

	waiter = kthread_run(hello_waits, NULL, "hello-waits");
	HELLO_CHECK(!IS_ERR(waiter), "no kthread");
	napper = kthread_run(hello_naps, NULL, "hello-naps");
	HELLO_CHECK(!IS_ERR(napper), "no kthread");
	msleep(5);
	HELLO_CHECK(kthread_stop(waiter) == -ERESTARTSYS,
		    "kthread_stop() did not end an interruptible wait");
	HELLO_CHECK(kthread_stop(napper) > 0, "kthread_stop() did not end msleep_interruptible()");
	k = kthread_run(hello_sleeps_then_waits, NULL, "hello-sleeps-waits");
	HELLO_CHECK(!IS_ERR(k), "no kthread");
	msleep(2);
	HELLO_CHECK(kthread_stop(k) == 3,
		    "a stopped kthread slept in schedule_timeout_interruptible()");

	/* An RCU callback waits for a grace period: a switch. */
	call_rcu(&hello_rcu, hello_rcu_done);
	HELLO_CHECK(!hello_rcu_ran, "an RCU callback ran before a grace period");
	msleep(1);
	HELLO_CHECK(hello_rcu_ran, "an RCU callback never ran");

	/* put_task_struct() frees the last reference after a grace period. */
	k = kthread_run(hello_quick, &put_ran, "hello-put");
	HELLO_CHECK(!IS_ERR(k), "no kthread");
	get_task_struct(k);
	msleep(1);
	HELLO_CHECK(kthread_stop(k) == 5 && put_ran, "a held kthread did not run");
	put_task_struct(k);
	HELLO_CHECK(HELLO_KTHREADS() == 1, "put_task_struct() freed before a grace period");
	msleep(1);
	HELLO_CHECK(HELLO_KTHREADS() == 0, "put_task_struct() never freed the record");

	/*
	 * A wake queue takes over a reference its caller held, lets go of a
	 * second given for a task already queued, and of its own once woken.
	 */
	k = kthread_create(hello_quick, &queued_ran, "hello-wakeq");
	HELLO_CHECK(!IS_ERR(k), "no kthread");
	get_task_struct(k);
	get_task_struct(k);
	wake_q_add_safe(&wake_q, k);
	wake_q_add_safe(&wake_q, k);
	wake_up_q(&wake_q);
	msleep(1);
	HELLO_CHECK(queued_ran && HELLO_KTHREADS() == 0, "a wake queue kept or lost a reference");

	/* A long loop's cond_resched() lets the others run once its slice is spent. */
	HELLO_CHECK(!IS_ERR(kthread_run(hello_quick, &resched_ran, "hello-resched")), "no kthread");
	give_up = slx_now_ns() + NSEC_PER_SEC;
	while (!READ_ONCE(resched_ran) && slx_now_ns() < give_up)
		cond_resched();
	HELLO_CHECK(resched_ran, "a second of cond_resched() let no other task run");
	HELLO_CHECK(!IS_ERR(kthread_run(hello_quick, &yield_ran, "hello-yield")), "no kthread");
	yield();
	HELLO_CHECK(yield_ran, "yield() let no other task run");

	quick = kthread_run(hello_quick, &quick_ran, "hello-quick");
	HELLO_CHECK(!IS_ERR(quick), "no kthread");
	held = kthread_run(hello_quick, &held_ran, "hello-held");
	HELLO_CHECK(!IS_ERR(held), "no kthread");
	get_task_struct(held);
	for (i = 0; i < 100 && !(READ_ONCE(quick_ran) && READ_ONCE(held_ran)); i++)
		usleep_range(0, 50);
	HELLO_CHECK(quick_ran && held_ran, "usleep_range(0, 50) let no other task run");
	HELLO_CHECK(HELLO_KTHREADS() == 1,
		    "a kthread that ended by itself was not freed, or one still held was");
	HELLO_CHECK(kthread_stop_put(held) == 5,
		    "kthread_stop_put() did not hand back an ended kthread's result");
	msleep(1);
	HELLO_CHECK(HELLO_KTHREADS() == 0, "kthread_stop_put() did not free an ended kthread");

	t0 = slx_now_ns();
	j = jiffies;
	mdelay(10);
	HELLO_CHECK(slx_now_ns() - t0 >= 10 * NSEC_PER_MSEC, "mdelay(10) took less than 10 ms");
	HELLO_CHECK(!time_before(jiffies, j + 2), "jiffies stood still through mdelay(10)");

	j = jiffies;
	give_up = slx_now_ns() + NSEC_PER_SEC;
	while (time_before(jiffies, j + 2) && slx_now_ns() < give_up)
		cpu_relax();
	HELLO_CHECK(!time_before(jiffies, j + 2), "jiffies stood still through cpu_relax()");

	pr_info("hello edges: a wait timed out, and the longest did not; kthreads stopped unrun, and out of interruptible waits; ended ones freed; usleep_range(0), cond_resched() and yield() let others run; busy waits move jiffies\n");
	return 0;
}
late_initcall(hello_edges_init);

/*
 * Timers and softirqs: Linux's own timer wheel and softirq code, run by the
 * layer's loop as Linux's tick and ksoftirqd would. A timer fires no sooner
 * than its jiffy, in softirq context, while the only task waits on it -- so
 * the loop sleeps until it, rather than ending the process as deadlocked;
 * a deleted one never fires; one moved sooner fires sooner; one re-arming
 * itself fires each time; a tasklet scheduled under local_bh_disable() runs
 * at local_bh_enable(), on the softirq stack, and one scheduled plainly at
 * the loop's next turn.
 */
static struct timer_list hello_timer, hello_cancelled, hello_moved, hello_ticker;
static DECLARE_COMPLETION(hello_timer_done);
static DECLARE_COMPLETION(hello_moved_done);
static DECLARE_COMPLETION(hello_ticker_done);
static unsigned long hello_timer_fired_at;
static bool hello_timer_in_softirq, hello_cancelled_fired;
static int hello_ticks;

/* A tasklet that schedules itself again until its 20th run. */
static struct tasklet_struct hello_chain;
static int hello_chain_runs;
static DECLARE_COMPLETION(hello_chain_done);

static void hello_chain_fn(struct tasklet_struct *t)
{
	if (++hello_chain_runs < 20)
		tasklet_schedule(t);
	else
		complete(&hello_chain_done);
}

static struct timer_list hello_deferred;
static bool hello_deferred_fired;

static void hello_deferred_fn(struct timer_list *t)
{
	hello_deferred_fired = true;
}

/* A tasklet calling cond_resched() for 10 ms, and whether another task ran. */
static struct tasklet_struct hello_resched_tasklet;
static bool hello_other_ran, hello_ran_beside_tasklet;

static void hello_resched_tasklet_fn(struct tasklet_struct *t)
{
	u64 t0 = slx_now_ns();

	while (slx_now_ns() - t0 < 10 * NSEC_PER_MSEC)
		cond_resched();
	hello_ran_beside_tasklet = READ_ONCE(hello_other_ran);
}

static int hello_cpu_onlined = -1;

static int hello_cpu_online(unsigned int cpu)
{
	hello_cpu_onlined = cpu;
	return 0;
}

static void hello_timer_fn(struct timer_list *t)
{
	hello_timer_fired_at = jiffies;
	hello_timer_in_softirq = in_serving_softirq();
	complete(&hello_timer_done);
}

static void hello_cancelled_fn(struct timer_list *t)
{
	hello_cancelled_fired = true;
}

static void hello_moved_fn(struct timer_list *t)
{
	complete(&hello_moved_done);
}

static void hello_ticker_fn(struct timer_list *t)
{
	if (++hello_ticks < 5)
		mod_timer(t, jiffies + 1);
	else
		complete(&hello_ticker_done);
}

static struct tasklet_struct hello_tasklet, hello_late_tasklet;
static bool hello_tasklet_ran, hello_tasklet_on_its_stack, hello_late_tasklet_ran;

static void hello_tasklet_fn(struct tasklet_struct *t)
{
	char *top = (char *)*raw_cpu_ptr(&hardirq_stack_ptr), here;

	hello_tasklet_ran = true;
	hello_tasklet_on_its_stack = &here < top && &here > top - slx_stack_bytes;
}

static void hello_late_tasklet_fn(struct tasklet_struct *t)
{
	hello_late_tasklet_ran = true;
}

static int __init hello_timers_init(void)
{
	unsigned long j;

	if (slx_argc() > 1)
		return 0;

	timer_setup(&hello_timer, hello_timer_fn, 0);
	j = jiffies;
	mod_timer(&hello_timer, j + 3);
	HELLO_CHECK(timer_pending(&hello_timer), "an armed timer was not pending");
	/* Nothing else can wake this: the loop must sleep until the timer. */
	wait_for_completion(&hello_timer_done);
	HELLO_CHECK(!time_before(hello_timer_fired_at, j + 3), "a timer fired before its jiffy");
	HELLO_CHECK(hello_timer_in_softirq, "a timer's function ran outside softirq context");
	HELLO_CHECK(!timer_pending(&hello_timer), "a timer that fired was still pending");

	timer_setup(&hello_cancelled, hello_cancelled_fn, 0);
	mod_timer(&hello_cancelled, jiffies + 2);
	HELLO_CHECK(timer_delete_sync(&hello_cancelled) == 1, "deleting an armed timer said it was not");
	HELLO_CHECK(timer_delete(&hello_cancelled) == 0, "deleting an idle timer said it was armed");
	msleep(20);
	HELLO_CHECK(!hello_cancelled_fired, "a deleted timer fired");

	timer_setup(&hello_moved, hello_moved_fn, 0);
	mod_timer(&hello_moved, jiffies + 10 * HZ);
	HELLO_CHECK(mod_timer(&hello_moved, jiffies + 2) == 1,
		    "moving an armed timer said it was not armed");
	HELLO_CHECK(wait_for_completion_timeout(&hello_moved_done, HZ),
		    "a timer moved sooner did not fire sooner");

	timer_setup(&hello_ticker, hello_ticker_fn, 0);
	j = jiffies;
	mod_timer(&hello_ticker, j + 1);
	HELLO_CHECK(wait_for_completion_timeout(&hello_ticker_done, HZ) && hello_ticks == 5,
		    "a timer re-arming itself did not fire five times");
	HELLO_CHECK(!time_before(jiffies, j + 5), "five re-armed jiffies took less than five");

	tasklet_setup(&hello_tasklet, hello_tasklet_fn);
	local_bh_disable();
	tasklet_schedule(&hello_tasklet);
	HELLO_CHECK(!hello_tasklet_ran, "a tasklet ran while bottom halves were off");
	local_bh_enable();
	HELLO_CHECK(hello_tasklet_ran, "a tasklet did not run at local_bh_enable()");
	HELLO_CHECK(hello_tasklet_on_its_stack, "a tasklet did not run on the softirq stack");

	tasklet_setup(&hello_late_tasklet, hello_late_tasklet_fn);
	tasklet_schedule(&hello_late_tasklet);
	msleep(1);
	HELLO_CHECK(hello_late_tasklet_ran, "a tasklet scheduled in a task never ran");

	/* More than one turn's softirqs, waited on with nothing else pending. */
	tasklet_setup(&hello_chain, hello_chain_fn);
	tasklet_schedule(&hello_chain);
	wait_for_completion(&hello_chain_done);
	HELLO_CHECK(hello_chain_runs == 20, "a tasklet scheduling itself stopped short");

	/* A deferrable timer runs on the first turn past its jiffy, as on a tick. */
	timer_setup(&hello_deferred, hello_deferred_fn, TIMER_DEFERRABLE);
	mod_timer(&hello_deferred, jiffies + 2);
	msleep(50);
	HELLO_CHECK(hello_deferred_fired, "a deferrable timer did not run past its jiffy");

	/* cond_resched() in softirq context lets no task run. */
	HELLO_CHECK(!IS_ERR(kthread_run(hello_quick, &hello_other_ran, "hello-other")),
		    "no kthread");
	tasklet_setup(&hello_resched_tasklet, hello_resched_tasklet_fn);
	local_bh_disable();
	tasklet_schedule(&hello_resched_tasklet);
	local_bh_enable();
	HELLO_CHECK(!hello_ran_beside_tasklet, "cond_resched() in a tasklet let another task run");
	msleep(1);
	HELLO_CHECK(hello_other_ran, "a kthread never ran");

	/* A hotplug state's startup runs for the one CPU, as Linux's for each. */
	HELLO_CHECK(cpuhp_setup_state(CPUHP_AP_ONLINE_DYN, "hello:online", hello_cpu_online,
				      NULL) >= 0 && hello_cpu_onlined == 0,
		    "a hotplug state's startup did not run for CPU 0");

	pr_info("hello timers: a timer slept on, deleted, moved, re-armed, deferred; tasklets at local_bh_enable, at the next turn, twenty in a chain; no switch from softirq; a hotplug startup\n");
	return 0;
}
late_initcall(hello_timers_init);
