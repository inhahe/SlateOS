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
#include <linux/uaccess.h>
#include <linux/cpumask.h>

#include "slx.h"
#include "lx_emul.h"

/* kernel/sched/sched.h's, which a driver file does not include. */
int try_to_wake_up(struct task_struct *p, unsigned int state, int wake_flags);

/* The first task's handle. */
int lx_main_handle = -1;

/*
 * A thread the layer made (kernel_thread): Linux's record of it, the Rust
 * half's handle, and what it runs.
 */
struct lx_task {
	struct task_struct task;
	/* -1 once it has ended: its slot may be another task's by then. */
	int handle;
	int (*fn)(void *arg);
	void *arg;
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
 * Kernel threads: Linux's own kernel/kthread.c -- kthread_create(),
 * kthread_stop(), parking, kthread workers, kthreadd -- over what Linux's
 * fork and exit would give it, which the layer gives here: kernel_thread()
 * makes a task with a record of its own, and do_exit() ends one.
 *
 * A record is counted as Linux counts a task's: the thread holds one
 * reference while it lives, which do_exit() lets go; kthread_stop() holds
 * another while it waits; the last let go frees it -- at once when it is the
 * layer's own (lx_put_task), after an RCU grace period when Linux's
 * put_task_struct() lets it go.
 */

/*
 * The last reference to a task's record let go: a thread's record is
 * freed, with what kthread.c hung on it. The first task's and the idle
 * record are never let go.
 */
void __put_task_struct(struct task_struct *p)
{
	struct lx_task *t = lx_task_of(p);

	if (WARN_ON(!t))
		return;
	free_kthread_struct(p);
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
 * may be walking the task list; a driver process has neither, and the
 * layer's own drops free it at once, as Linux's put_task_struct_many()
 * does.
 */
static void lx_put_task(struct task_struct *p)
{
	if (refcount_dec_and_test(&p->usage))
		__put_task_struct(p);
}

/* A thread's first and only frame: its function, then its end. */
static void lx_thread_entry(void *arg)
{
	struct lx_task *t = arg;

	do_exit(t->fn(t->arg));
}

/*
 * A new task running fn(arg), as Linux's copy_process() makes a kernel
 * thread: a record of its own, counted as one reference -- its own, which
 * do_exit() lets go -- marked a kernel thread, with kthread.c's part hung
 * on it. Runnable at once. The record, or an ERR_PTR.
 */
static struct task_struct *lx_new_thread(int (*fn)(void *), void *arg, const char *name)
{
	struct lx_task *t = kzalloc(sizeof(*t), GFP_KERNEL);

	if (!t)
		return ERR_PTR(-ENOMEM);
	t->fn = fn;
	t->arg = arg;
	refcount_set(&t->task.usage, 1);
	t->task.flags = PF_KTHREAD;
	strscpy(t->task.comm, name ?: "", sizeof(t->task.comm));
	if (!set_kthread_struct(&t->task)) {
		kfree(t);
		return ERR_PTR(-ENOMEM);
	}
	t->handle = slx_task_new(lx_thread_entry, t, &t->task);
	if (t->handle < 0) {
		free_kthread_struct(&t->task);
		kfree(t);
		return ERR_PTR(-ENOMEM);
	}
	lx_kthreads++;
	return &t->task;
}

/* What kthreadd's create_kthread() calls: the new thread's "pid", positive. */
pid_t kernel_thread(int (*fn)(void *), void *arg, const char *name, unsigned long flags)
{
	struct task_struct *p = lx_new_thread(fn, arg, name);

	if (IS_ERR(p))
		return PTR_ERR(p);
	return lx_task_of(p)->handle + 1;
}

/*
 * Linux's kthreadd, the thread every kthread_create() asks for a new
 * thread, started as Linux's rest_init() starts it: before anything can ask.
 * 0, or -ENOMEM.
 */
int lx_start_kthreadd(void)
{
	struct task_struct *p = lx_new_thread(kthreadd, NULL, "kthreadd");

	if (IS_ERR(p))
		return PTR_ERR(p);
	kthreadd_task = p;
	return 0;
}

/*
 * The running thread ends, as Linux's do_exit() ends one: its result kept
 * for kthread_stop() (kthread_do_exit), its record marked dead -- no wake
 * reaches it now, and its slot may be another task's -- what waits for its
 * end told (vfork_done, which set_kthread_struct() points at the kthread's
 * `exited`, completed as Linux's exit_mm() completes it), and its own
 * reference let go, which may free the record. Its last touch of it.
 */
void __noreturn do_exit(long code)
{
	struct task_struct *tsk = current;
	struct lx_task *t = lx_task_of(tsk);
	struct kthread *kthread = tsk_is_kthread(tsk);
	struct completion *vfork = tsk->vfork_done;

	if (!t)
		slx_stop("do_exit (by the first task, which returns instead)",
			 __builtin_return_address(0));
	if (kthread)
		kthread_do_exit(kthread, code);
	WRITE_ONCE(tsk->__state, TASK_DEAD);
	t->handle = -1;
	if (vfork) {
		tsk->vfork_done = NULL;
		complete(vfork);
	}
	lx_put_task(tsk);
	slx_task_exit();
}

/* A thread's name, as Linux's __set_task_comm() sets it. */
void __set_task_comm(struct task_struct *tsk, const char *buf, bool exec)
{
	strscpy(tsk->comm, buf, sizeof(tsk->comm));
}

/*
 * What kthread.c asks of Linux's scheduler and signals, about CPUs, policy
 * and signals a driver process does not have: its one CPU is every
 * thread's, every thread runs at one priority, and no signal but
 * kthread_stop()'s reaches a thread. A thread not running is inactive, as
 * every thread but the caller is here.
 */
int set_cpus_allowed_ptr(struct task_struct *p, const struct cpumask *new_mask)
{
	return 0;
}

void do_set_cpus_allowed(struct task_struct *p, const struct cpumask *new_mask)
{
}

int sched_setscheduler_nocheck(struct task_struct *p, int policy,
			       const struct sched_param *param)
{
	return 0;
}

void ignore_signals(struct task_struct *t)
{
}

unsigned long wait_task_inactive(struct task_struct *p, unsigned int match_state)
{
	return 1;
}

/*
 * A read of memory that may not be mapped (kthread_probe_data): what the
 * layer's threads probe this way is their own kthread record, always
 * mapped.
 */
long copy_from_kernel_nofault(void *dst, const void *src, size_t size)
{
	memcpy(dst, src, size);
	return 0;
}

/*
 * Tasks whose wait is for work to come rather than for an event -- kthreadd,
 * a workqueue's workers: with every other task finished, the process is
 * done, though these wait for ever.
 */
bool lx_emul_task_is_daemon(void *task)
{
	struct task_struct *p = task ? task : &lx_main_task;

	return p == kthreadd_task || (p->flags & PF_WQ_WORKER);
}
