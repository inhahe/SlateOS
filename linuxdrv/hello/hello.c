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
 *               SLX_EXIT_INIT_FAILED at once.
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
#include <linux/err.h>

#include "../emul/slx.h"
#include "../emul/lx_emul.h"

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
 * HELLO_OVERFLOW_KIB of it runs past a task's 64 KiB stack but not past the
 * 16 KiB guard below it (linuxdrv/kit/src/sched.rs), so that it returns,
 * having written over memory, only where the guard is missing.
 */
#define HELLO_OVERFLOW_KIB 68

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
		hello_deep(HELLO_OVERFLOW_KIB);
		pr_err("hello: %d KiB of stack used, past the guard, and nothing faulted\n",
		       HELLO_OVERFLOW_KIB);
		return -EFAULT;
	}
	if (hello_mode("fail")) {
		kthread_run(hello_polls, NULL, "hello-polls");
		return -ENODEV;
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
 * by signal; their records freed when they end by themselves, or when the
 * last reference goes; usleep_range(0, ...), cond_resched() and yield()
 * letting the others run; and jiffies moving on through busy waits, and
 * before a sleep reckons its end, as Linux's timer interrupt keeps them.
 * Each check says which failed.
 */
#define HELLO_CHECK(cond, what)						\
	do {								\
		if (!(cond)) {						\
			pr_err("hello edges: %s\n", what);		\
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
	struct task_struct *never, *waiter, *napper, *quick, *held, *waker, *sleepers[2];
	/*
	 * The longest sleep there is, and one whose nanoseconds (at HZ 250,
	 * 4,000,000 a jiffy, 2^8 * 15625) are 2^64 * 15625: both too long to end.
	 */
	struct hello_sleep sleeps[2] = { { MAX_JIFFY_OFFSET }, { 1L << 56 } };
	bool ran = false, quick_ran = false, held_ran = false;
	bool resched_ran = false, yield_ran = false;
	unsigned long j;
	u64 t0, give_up;
	long left;
	int i;

	HELLO_CHECK(lx_kthreads == 0, "a stopped kthread's record was not freed");

	j = jiffies;
	HELLO_CHECK(wait_for_completion_timeout(&hello_forever, 2) == 0,
		    "a wait that timed out said it had not");
	HELLO_CHECK(!time_before(jiffies, j + 2), "a timed-out wait ended early");

	/*
	 * A sleep reckons its end from jiffies brought up to the clock: after
	 * 120 ms that moved nothing on (a spin without cpu_relax()), 30
	 * jiffies, a sleep of 60 woken at once has nearly 60 left, not 30. The
	 * margin is for the process waiting for the CPU on a busy system.
	 */
	t0 = slx_now_ns();
	while (slx_now_ns() - t0 < 120 * NSEC_PER_MSEC)
		barrier();
	waker = kthread_run(hello_wakes, current, "hello-wakes");
	HELLO_CHECK(!IS_ERR(waker), "no kthread");
	set_current_state(TASK_INTERRUPTIBLE);
	left = schedule_timeout(60);
	HELLO_CHECK(left > 40, "a sleep woken at once reckoned its end from stale jiffies");

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

	never = kthread_create(hello_never_runs, &ran, "hello-never");
	HELLO_CHECK(!IS_ERR(never), "no kthread");
	HELLO_CHECK(kthread_stop(never) == -EINTR && !ran,
		    "a kthread stopped before it ran did not end unrun, with -EINTR");

	waiter = kthread_run(hello_waits, NULL, "hello-waits");
	HELLO_CHECK(!IS_ERR(waiter), "no kthread");
	napper = kthread_run(hello_naps, NULL, "hello-naps");
	HELLO_CHECK(!IS_ERR(napper), "no kthread");
	msleep(5);
	j = jiffies;
	HELLO_CHECK(kthread_stop(waiter) == -ERESTARTSYS,
		    "kthread_stop() did not end an interruptible wait");
	HELLO_CHECK(kthread_stop(napper) > 9000 && time_before(jiffies, j + HZ),
		    "kthread_stop() did not end msleep_interruptible()");

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
	HELLO_CHECK(lx_kthreads == 1,
		    "a kthread that ended by itself was not freed, or one still held was");
	HELLO_CHECK(kthread_stop_put(held) == 5 && lx_kthreads == 0,
		    "kthread_stop_put() did not hand back an ended kthread's result and free it");

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
