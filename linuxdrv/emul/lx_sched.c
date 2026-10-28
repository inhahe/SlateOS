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
 *
 * Time: jiffies follow the monotonic clock wherever Linux's would have
 * moved -- at every turn of the loop (lx_emul_turn), before a timed sleep
 * reckons its end, after a busy wait, and at every cpu_relax() (the shadow
 * <asm/vdso/processor.h>).
 */
#include <linux/sched.h>
#include <linux/sched/signal.h>
#include <linux/sched/task.h>
#include <linux/sched/wake_q.h>
#include <linux/kthread.h>
#include <linux/completion.h>
#include <linux/delay.h>
#include <linux/jiffies.h>
#include <linux/math64.h>
#include <linux/overflow.h>
#include <linux/slab.h>
#include <linux/wait.h>
#include <linux/err.h>

#include "slx.h"
#include "lx_emul.h"

/* kernel/sched/sched.h's, which a driver file does not include. */
int try_to_wake_up(struct task_struct *p, unsigned int state, int wake_flags);

/* The first task's handle. */
int lx_main_handle = -1;

/* A task the layer made: Linux's record of it, and the Rust half's handle. */
struct lx_task {
	struct task_struct task;
	/* -1 once it has ended: its slot may be another task's by then. */
	int handle;
	int (*threadfn)(void *data);
	void *data;
	/* What its function returned; -EINTR while it has not run. */
	int result;
	bool should_stop;
	struct completion exited;
};

unsigned long lx_kthreads;

/* The layer's record around `p`: none for the first task's or the idle one. */
static struct lx_task *lx_task_of(struct task_struct *p)
{
	if (p == &lx_main_task || p == &lx_idle_task)
		return NULL;
	return container_of(p, struct lx_task, task);
}

/* The Rust half's handle of the task `p` names; -1 for the idle record. */
static int lx_handle(struct task_struct *p)
{
	struct lx_task *t = lx_task_of(p);

	if (t)
		return t->handle;
	return p == &lx_main_task ? lx_main_handle : -1;
}

/*
 * Whether the running task, about to sleep, stays running instead: it was
 * woken already (TASK_RUNNING), or, as Linux's __schedule() decides, it
 * would sleep interruptibly with a signal pending -- kthread_stop() sends
 * one -- and so is made running again.
 */
static bool lx_stays_running(void)
{
	unsigned int state = READ_ONCE(current->__state);

	if (state == TASK_RUNNING)
		return true;
	if (!signal_pending_state(state, current))
		return false;
	__set_current_state(TASK_RUNNING);
	return true;
}

/*
 * A task about to give up the CPU must be in neither a softirq nor a
 * section with bottom halves off: the count those raise is the process's
 * one, so every other task would run inside it, and the loop could run no
 * softirq again. Linux reports "scheduling while atomic" and carries on;
 * here the process ends as a BUG, naming the caller, as a wait outside
 * every task does.
 */
static void lx_may_sleep(void *caller)
{
	if (likely(!preempt_count()))
		return;
	pr_err("BUG: scheduling while atomic (preempt count %#x), called from %px\n",
	       preempt_count(), caller);
	slx_exit(SLX_EXIT_BUG);
}

asmlinkage __visible void schedule(void)
{
	lx_may_sleep(__builtin_return_address(0));
	if (lx_stays_running()) {
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

/*
 * A long loop's offer to let the others run (cond_resched()): taken once
 * the task has run a jiffy since it was switched to, as Linux's tick asks a
 * task to give way once its slice is spent, and never with interrupts off.
 * The clock is a system call, so it is looked at once in 32 calls: a loop
 * runs at most 31 more turns past its slice. 1 when it gave way.
 */
int __cond_resched(void)
{
	static unsigned int calls;

	/* As Linux's should_resched(0): never inside a softirq or with BHs off. */
	if (irqs_disabled() || preempt_count() || ++calls % 32)
		return 0;
	if (slx_now_ns() - lx_switched_ns < NSEC_PER_SEC / HZ)
		return 0;
	slx_task_yield();
	return 1;
}

/* Let the others run, and run again in turn. */
void yield(void)
{
	lx_may_sleep(__builtin_return_address(0));
	set_current_state(TASK_RUNNING);
	slx_task_yield();
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
 * Wake queues, as Linux's kernel/sched/core.c keeps them: a task queued
 * holds a reference to its record until wake_up_q() has woken it. One
 * thread queues and wakes, so no atomic exchange is needed to claim one.
 */
static bool lx_wake_q_add(struct wake_q_head *head, struct task_struct *task)
{
	struct wake_q_node *node = &task->wake_q;

	if (node->next)
		return false;
	node->next = WAKE_Q_TAIL;
	*head->lastp = node;
	head->lastp = &node->next;
	return true;
}

void wake_q_add(struct wake_q_head *head, struct task_struct *task)
{
	if (lx_wake_q_add(head, task))
		get_task_struct(task);
}

/* The caller's own reference becomes the queue's, or goes if already queued. */
void wake_q_add_safe(struct wake_q_head *head, struct task_struct *task)
{
	if (!lx_wake_q_add(head, task))
		put_task_struct(task);
}

void wake_up_q(struct wake_q_head *head)
{
	struct wake_q_node *node = head->first;

	while (node != WAKE_Q_TAIL) {
		struct task_struct *task = container_of(node, struct task_struct, wake_q);

		node = node->next;
		WRITE_ONCE(task->wake_q.next, NULL);
		wake_up_process(task);
		put_task_struct(task);
	}
}

/*
 * RCU callbacks, after a grace period. Linux's tasks are coroutines on the
 * process's one thread, and an RCU reader may not sleep (this configuration
 * has no preemptible RCU), so once the running task has switched away, no
 * task is inside a read-side critical section: a callback queued before a
 * switch has had its grace period by the next turn of the loop, between
 * tasks, where lx_emul_turn() runs them -- as Linux's tiny RCU runs them
 * after a context switch.
 */
static struct rcu_head *lx_rcu_first;
static struct rcu_head **lx_rcu_last = &lx_rcu_first;

void call_rcu(struct rcu_head *head, rcu_callback_t func)
{
	head->func = func;
	head->next = NULL;
	*lx_rcu_last = head;
	lx_rcu_last = &head->next;
}

bool lx_rcu_pending(void)
{
	return lx_rcu_first;
}

void lx_rcu_run(void)
{
	struct rcu_head *head = lx_rcu_first;

	/* What these queue waits for the next switch. */
	lx_rcu_first = NULL;
	lx_rcu_last = &lx_rcu_first;
	while (head) {
		struct rcu_head *next = head->next;

		head->func(head);
		head = next;
	}
}

/* Nanoseconds in `count` units of `unit` ns; U64_MAX past what a u64 holds. */
static u64 lx_ns(u64 count, u64 unit)
{
	u64 ns;

	return check_mul_overflow(count, unit, &ns) ? U64_MAX : ns;
}

/*
 * The monotonic clock `ns` from now, as slx_task_block() takes a deadline:
 * 0, none, when the clock will never get there.
 */
static u64 lx_deadline(u64 ns)
{
	u64 now = slx_now_ns();

	if (ns >= U64_MAX - now)
		return 0;
	return max(now + ns, 1ULL);
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
	lx_may_sleep(__builtin_return_address(0));
	if (timeout < 0) {
		pr_err("schedule_timeout: wrong timeout value %lx\n", timeout);
		__set_current_state(TASK_RUNNING);
		return 0;
	}
	/*
	 * The deadline is reckoned from a later reading of the clock than
	 * jiffies are, so when it comes jiffies have reached `expire`.
	 */
	lx_emul_jiffies_now();
	expire = jiffies + timeout;
	if (lx_stays_running()) {
		slx_task_yield();
	} else {
		slx_task_block(lx_deadline(lx_ns(timeout, NSEC_PER_SEC / HZ)));
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
 * Sleep `ns` nanoseconds in `state`, letting the other tasks run: at least
 * once even for 0, as Linux's usleep arms a timer and schedules. A wake
 * that comes sooner does not end it, as nothing ends Linux's usleep or
 * msleep; when `signals`, a pending signal does -- kthread_stop() sends
 * one -- as it ends msleep_interruptible(). The nanoseconds left.
 */
static u64 lx_sleep(u64 ns, unsigned int state, bool signals)
{
	u64 until = lx_deadline(ns);
	u64 now;

	lx_may_sleep(__builtin_return_address(0));
	for (;;) {
		if (signals && signal_pending(current))
			break;
		__set_current_state(state);
		slx_task_block(until);
		__set_current_state(TASK_RUNNING);
		if (until && slx_now_ns() >= until)
			return 0;
	}
	now = slx_now_ns();
	if (!until)
		return U64_MAX;
	return until > now ? until - now : 0;
}

/* Linux's sleeps msecs_to_jiffies(msecs), 0 for 0; this one yields for 0. */
void msleep(unsigned int msecs)
{
	lx_sleep((u64)msecs * NSEC_PER_MSEC, TASK_UNINTERRUPTIBLE, false);
}

/* The milliseconds left, rounded up, when a signal ends it. */
unsigned long msleep_interruptible(unsigned int msecs)
{
	u64 left = lx_sleep((u64)msecs * NSEC_PER_MSEC, TASK_INTERRUPTIBLE, true);

	return left / NSEC_PER_MSEC + (left % NSEC_PER_MSEC != 0);
}

/* At least `min` microseconds; Linux's slack up to `max` is not needed. */
void usleep_range_state(unsigned long min, unsigned long max, unsigned int state)
{
	lx_sleep(lx_ns(min, NSEC_PER_USEC), state, false);
}

/*
 * Busy waits, as Linux's are: the process holds its thread for them
 * (design-decisions 1578, "What it costs"), and jiffies move on by the
 * time they end. __const_udelay takes Linux's fixed-point count,
 * microseconds * 2^32 / 10^6.
 */
static void lx_spin_ns(u64 ns)
{
	u64 until = lx_deadline(ns);

	/* Not cpu_relax(), which would read the clock twice a turn. */
	while (!until || slx_now_ns() < until)
		native_pause();
	lx_emul_jiffies_now();
}

void __const_udelay(unsigned long xloops)
{
	lx_spin_ns(mul_u64_u32_shr(xloops, NSEC_PER_SEC, 32));
}

void __udelay(unsigned long usecs)
{
	lx_spin_ns(lx_ns(usecs, NSEC_PER_USEC));
}

void __ndelay(unsigned long nsecs)
{
	lx_spin_ns(nsecs);
}

/*
 * Kernel threads: a task of the Rust half's per thread, which waits, as a
 * new Linux kthread does, to be woken (kthread_run wakes it), runs its
 * function unless told to stop first, and ends.
 *
 * Its record is counted as Linux counts a task's: the thread holds a
 * reference while it lives, and kthread_stop() one while it waits for it;
 * the last let go frees it. So a thread that ends on its own is freed when
 * it ends, and a stopped one when kthread_stop() returns -- and, as in
 * Linux, a caller may stop a thread that can end on its own only while it
 * holds a reference of its own (get_task_struct, then kthread_stop_put).
 */

/*
 * The last reference to a task's record let go: a kthread's is freed. The
 * first task's is never let go.
 */
void __put_task_struct(struct task_struct *p)
{
	struct lx_task *t = lx_task_of(p);

	if (WARN_ON(!t))
		return;
	lx_kthreads--;
	kfree(t);
}

/* What Linux's put_task_struct() has RCU call once the last reference goes. */
void __put_task_struct_rcu_cb(struct rcu_head *rhp)
{
	__put_task_struct(container_of(rhp, struct task_struct, rcu));
}

/*
 * Let go of a reference to a task's record. Linux's put_task_struct()
 * frees the last only after an RCU grace period, for the other CPUs that
 * may be walking the task list; a driver process has neither, and frees it
 * at once, as Linux's put_task_struct_many() does.
 */
static void lx_put_task(struct task_struct *p)
{
	if (refcount_dec_and_test(&p->usage))
		__put_task_struct(p);
}

static void lx_kthread_entry(void *arg)
{
	struct lx_task *t = arg;

	/* Until kthread_run(), or kthread_stop(), wakes it; nothing else does. */
	while (READ_ONCE(t->task.__state) != TASK_RUNNING)
		schedule();
	if (!READ_ONCE(t->should_stop))
		t->result = t->threadfn(t->data);
	/* Dead: no wake reaches it now, and its slot may be another task's. */
	WRITE_ONCE(t->task.__state, TASK_DEAD);
	t->handle = -1;
	complete(&t->exited);
	/* Its last touch of its record, which this may free. */
	lx_put_task(&t->task);
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
	t->result = -EINTR;
	init_completion(&t->exited);
	refcount_set(&t->task.usage, 1);
	va_start(args, namefmt);
	vsnprintf(t->task.comm, sizeof(t->task.comm), namefmt, args);
	va_end(args);
	t->task.__state = TASK_UNINTERRUPTIBLE;
	t->handle = slx_task_new(lx_kthread_entry, t, &t->task);
	if (t->handle < 0) {
		kfree(t);
		return ERR_PTR(-ENOMEM);
	}
	lx_kthreads++;
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

/*
 * As Linux's: told to stop, and -- with TIF_NOTIFY_SIGNAL, which
 * signal_pending() reads -- out of any interruptible wait, the thread is
 * woken, and what its function returned is handed back once it has ended
 * (-EINTR when it never ran).
 */
int kthread_stop(struct task_struct *k)
{
	struct lx_task *t = lx_task_of(k);
	int ret;

	if (WARN_ON(!t))
		return -EINVAL;
	get_task_struct(k);
	WRITE_ONCE(t->should_stop, true);
	set_tsk_thread_flag(k, TIF_NOTIFY_SIGNAL);
	wake_up_process(k);
	wait_for_completion(&t->exited);
	ret = t->result;
	lx_put_task(k);
	return ret;
}

/* kthread_stop(), and the caller's own reference let go. */
int kthread_stop_put(struct task_struct *k)
{
	int ret = kthread_stop(k);

	lx_put_task(k);
	return ret;
}
