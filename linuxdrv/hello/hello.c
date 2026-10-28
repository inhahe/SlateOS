// SPDX-License-Identifier: MIT
/*
 * hello -- the driver layer's first driver process, and no driver: a Linux
 * initcall that allocates, takes a spinlock with interrupts "off", formats a
 * line through Linux's own vsprintf and frees, run in a process of its own
 * as any Linux driver will be (roadmap.md 2.1, design-decisions 1580).
 *
 * The kernel's self-test (`linuxdrv::self_test`) runs it twice: plainly,
 * when it must exit 0 having printed its line; and with the argument
 * "stop", when it calls a function nothing defines -- which the build turns
 * into a generated stop -- and must exit SLX_EXIT_STOP, naming it.
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
#include <linux/err.h>

#include "../emul/slx.h"

/* Defined nowhere: the build makes it a generated stop. */
void lx_hello_never_defined(void);

/* How many warnings the layer has said (lx_emul.c). */
extern unsigned long lx_warnings;

/* A warning from one place, given twice: said once, and the driver goes on. */
static void hello_warn_once(void)
{
	WARN_ON_ONCE(1);
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
	if (slx_argc() > 1 && strcmp(slx_argv(1), "stop") == 0)
		lx_hello_never_defined();
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
