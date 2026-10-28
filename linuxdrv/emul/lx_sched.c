// SPDX-License-Identifier: MIT
/*
 * Linux's scheduler, as a driver process has it (design-decisions 1578
 * rule 5).
 *
 * Linux's tasks are the Rust half's coroutines (slx_task_*): one runs at a
 * time, until it blocks, yields or returns. This file is what Linux's own
 * wait queues, completions, wait-on-bit and mutexes -- kernel/sched/wait.c,
 * completion.c, swait.c, wait_bit.c and kernel/locking/mutex.c, compiled in
 * as they are -- call to sleep and to wake: schedule(), try_to_wake_up()
 * and the few around them. And kernel threads, sleeping and delays, which
 * Linux builds on its timers and its full scheduler, written here over the
 * same two calls.
 *
 * A task sleeps by setting its state (set_current_state) and calling
 * schedule(); it is woken by try_to_wake_up() setting it back to
 * TASK_RUNNING. As in Linux, a wake that comes between the two finds the
 * state already changed, and schedule() then does not block.
 */
#include <linux/sched.h>
#include <linux/sched/task.h>
#include <linux/sched/wake_q.h>
#include <linux/kthread.h>
#include <linux/completion.h>
#include <linux/delay.h>
#include <linux/jiffies.h>
#include <linux/slab.h>
#include <linux/wait.h>
#include <linux/err.h>

#include "slx.h"

/* kernel/sched/sched.h's, which a driver file does not include. */
int try_to_wake_up(struct task_struct *p, unsigned int state, int wake_flags);

/* The first task's record (lx_emul.c), and its handle. */
extern struct task_struct lx_main_task;
int lx_main_handle = -1;

/* A task the layer made: Linux's record of it, and the Rust half's handle. */
struct lx_task {
	struct task_struct task;
	int handle;
	int (*threadfn)(void *data);
	void *data;
	int result;
	bool should_stop;
	bool ran;
	struct completion exited;
};

static struct lx_task *lx_task_of(struct task_struct *p)
{
	return p == &lx_main_task ? NULL : container_of(p, struct lx_task, task);
}

static int lx_handle(struct task_struct *p)
{
	struct lx_task *t = lx_task_of(p);

	return t ? t->handle : lx_main_handle;
}

asmlinkage __visible void schedule(void)
{
	if (READ_ONCE(current->__state) == TASK_RUNNING) {
		slx_task_yield();
		return;
	}
	/* Until try_to_wake_up() makes it TASK_RUNNING and wakes it. */
	slx_task_block(0);
}

void schedule_preempt_disabled(void)
{
	schedule();
}

int try_to_wake_up(struct task_struct *p, unsigned int state, int wake_flags)
{
	if (!(READ_ONCE(p->__state) & state))
		return 0;
	WRITE_ONCE(p->__state, TASK_RUNNING);
	if (p != current)
		slx_task_wake(lx_handle(p));
	return 1;
}

int wake_up_process(struct task_struct *p)
{
	return try_to_wake_up(p, TASK_NORMAL, 0);
}

int wake_up_state(struct task_struct *p, unsigned int state)
{
	return try_to_wake_up(p, state, 0);
}

int default_wake_function(struct wait_queue_entry *curr, unsigned int mode,
			  int wake_flags, void *key)
{
	return try_to_wake_up(curr->private, mode, wake_flags);
}

/*
 * Wake queues, as Linux's kernel/sched/core.c keeps them, without the
 * reference counts: a task's record here lives until kthread_stop().
 */
void wake_q_add(struct wake_q_head *head, struct task_struct *task)
{
	struct wake_q_node *node = &task->wake_q;

	if (node->next)
		return;
	node->next = WAKE_Q_TAIL;
	*head->lastp = node;
	head->lastp = &node->next;
}

void wake_q_add_safe(struct wake_q_head *head, struct task_struct *task)
{
	wake_q_add(head, task);
}

void wake_up_q(struct wake_q_head *head)
{
	struct wake_q_node *node = head->first;

	while (node != WAKE_Q_TAIL) {
		struct task_struct *task = container_of(node, struct task_struct, wake_q);

		node = node->next;
		WRITE_ONCE(task->wake_q.next, NULL);
		wake_up_process(task);
	}
}

/* Nanoseconds in `j` jiffies. */
static u64 lx_jiffies_ns(unsigned long j)
{
	return (u64)j * (NSEC_PER_SEC / HZ);
}

/*
 * Sleep, in the state the caller set, until woken or `timeout` jiffies
 * pass: the jiffies left, as Linux's own answers.
 */
long schedule_timeout(long timeout)
{
	unsigned long expire;
	long left;

	if (timeout == MAX_SCHEDULE_TIMEOUT) {
		schedule();
		return timeout;
	}
	if (timeout < 0) {
		pr_err("schedule_timeout: wrong timeout value %lx\n", timeout);
		__set_current_state(TASK_RUNNING);
		return 0;
	}
	expire = jiffies + timeout;
	if (READ_ONCE(current->__state) == TASK_RUNNING) {
		slx_task_yield();
	} else {
		slx_task_block(slx_now_ns() + lx_jiffies_ns(timeout));
		/* Woken, or its time came: running either way. */
		__set_current_state(TASK_RUNNING);
	}
	left = (long)(expire - jiffies);
	return left < 0 ? 0 : left;
}

long schedule_timeout_interruptible(long timeout)
{
	__set_current_state(TASK_INTERRUPTIBLE);
	return schedule_timeout(timeout);
}

long schedule_timeout_killable(long timeout)
{
	__set_current_state(TASK_KILLABLE);
	return schedule_timeout(timeout);
}

long schedule_timeout_uninterruptible(long timeout)
{
	__set_current_state(TASK_UNINTERRUPTIBLE);
	return schedule_timeout(timeout);
}

long schedule_timeout_idle(long timeout)
{
	__set_current_state(TASK_IDLE);
	return schedule_timeout(timeout);
}

int io_schedule_prepare(void)
{
	return 0;
}

void io_schedule_finish(int token)
{
}

long io_schedule_timeout(long timeout)
{
	return schedule_timeout(timeout);
}

void io_schedule(void)
{
	schedule();
}

/*
 * Sleep `ns` nanoseconds at least, letting the other tasks run; a wake that
 * comes sooner does not end it, as nothing ends Linux's msleep.
 */
static void lx_sleep_ns(u64 ns)
{
	u64 until = slx_now_ns() + ns;

	while (slx_now_ns() < until) {
		__set_current_state(TASK_UNINTERRUPTIBLE);
		slx_task_block(until);
		__set_current_state(TASK_RUNNING);
	}
}

void msleep(unsigned int msecs)
{
	lx_sleep_ns((u64)msecs * NSEC_PER_MSEC);
}

/* No signals reach a driver process: it sleeps it all, and none is left. */
unsigned long msleep_interruptible(unsigned int msecs)
{
	msleep(msecs);
	return 0;
}

void usleep_range_state(unsigned long min, unsigned long max, unsigned int state)
{
	lx_sleep_ns((u64)min * NSEC_PER_USEC);
}

/*
 * Busy waits, as Linux's are: the process holds its thread for them
 * (design-decisions 1578, "What it costs"). __const_udelay takes Linux's
 * fixed-point count, microseconds * 2^32 / 10^6.
 */
static void lx_spin_ns(u64 ns)
{
	u64 until = slx_now_ns() + ns;

	while (slx_now_ns() < until)
		cpu_relax();
}

void __const_udelay(unsigned long xloops)
{
	lx_spin_ns(((u64)xloops * NSEC_PER_SEC) >> 32);
}

void __udelay(unsigned long usecs)
{
	lx_spin_ns((u64)usecs * NSEC_PER_USEC);
}

void __ndelay(unsigned long nsecs)
{
	lx_spin_ns(nsecs);
}

/*
 * Kernel threads: a task of the Rust half's per thread, which waits, as a
 * new Linux kthread does, to be woken (kthread_run wakes it), runs its
 * function, and is stopped by kthread_stop(), which frees its record.
 */
static void lx_kthread_entry(void *arg)
{
	struct lx_task *t = arg;

	if (READ_ONCE(t->task.__state) != TASK_RUNNING)
		schedule();
	if (!t->should_stop) {
		t->ran = true;
		t->result = t->threadfn(t->data);
	}
	/* The last it touches of its record: kthread_stop() may free it next. */
	complete(&t->exited);
}

struct task_struct *kthread_create_on_node(int (*threadfn)(void *data), void *data,
					   int node, const char namefmt[], ...)
{
	struct lx_task *t = kzalloc(sizeof(*t), GFP_KERNEL);
	va_list args;

	if (!t)
		return ERR_PTR(-ENOMEM);
	t->threadfn = threadfn;
	t->data = data;
	init_completion(&t->exited);
	va_start(args, namefmt);
	vsnprintf(t->task.comm, sizeof(t->task.comm), namefmt, args);
	va_end(args);
	t->task.__state = TASK_UNINTERRUPTIBLE;
	t->handle = slx_task_new(lx_kthread_entry, t, &t->task);
	if (t->handle < 0) {
		kfree(t);
		return ERR_PTR(-ENOMEM);
	}
	return &t->task;
}

bool kthread_should_stop(void)
{
	struct lx_task *t = lx_task_of(current);

	return t && READ_ONCE(t->should_stop);
}

bool kthread_should_stop_or_park(void)
{
	return kthread_should_stop();
}

int kthread_stop(struct task_struct *k)
{
	struct lx_task *t = lx_task_of(k);
	int ret;

	if (!t)
		return -EINVAL;
	WRITE_ONCE(t->should_stop, true);
	wake_up_process(k);
	wait_for_completion(&t->exited);
	ret = t->ran ? t->result : -EINTR;
	kfree(t);
	return ret;
}

int kthread_stop_put(struct task_struct *k)
{
	return kthread_stop(k);
}
