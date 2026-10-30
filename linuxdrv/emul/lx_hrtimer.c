// SPDX-License-Identifier: MIT
/*
 * High-resolution timers in a driver process: Linux's hrtimer interface,
 * the timers kept as Linux keeps them -- in a base per clock (monotonic,
 * real, boot and TAI time), each a queue ordered by expiry (lib/timerqueue.c,
 * Linux's own) -- and run by the loop at every turn (lx_softirq.c), whose
 * sleep the earliest expiry bounds. The kernel's idle wait takes a timeout
 * in nanoseconds, so a timer is as punctual as the process is scheduled.
 *
 * As in Linux, a hard timer's callback runs in hardirq context and a soft
 * one's in softirq context; a callback may restart its timer
 * (HRTIMER_RESTART, having moved its expiry with hrtimer_forward()), and arm
 * or cancel others. A timer is due once its clock reaches its expiry; the
 * slack hrtimer_start_range_ns() allows is not used.
 */
#include <linux/hrtimer.h>
#include <linux/timerqueue.h>
#include <linux/ktime.h>
#include <linux/timekeeping.h>
#include <linux/interrupt.h>

#include "slx.h"
#include "lx_emul.h"

/*
 * The bases in use: Linux's own array has a soft twin of each, which these
 * do without -- a soft timer is marked so on the timer itself (is_soft).
 */
#define LX_HRTIMER_BASES (HRTIMER_BASE_TAI + 1)

static struct hrtimer_clock_base lx_hrtimer_bases[LX_HRTIMER_BASES] = {
	[HRTIMER_BASE_MONOTONIC] = { .index = HRTIMER_BASE_MONOTONIC, .clockid = CLOCK_MONOTONIC },
	[HRTIMER_BASE_REALTIME] = { .index = HRTIMER_BASE_REALTIME, .clockid = CLOCK_REALTIME },
	[HRTIMER_BASE_BOOTTIME] = { .index = HRTIMER_BASE_BOOTTIME, .clockid = CLOCK_BOOTTIME },
	[HRTIMER_BASE_TAI] = { .index = HRTIMER_BASE_TAI, .clockid = CLOCK_TAI },
};

static int __init lx_hrtimer_init(void)
{
	unsigned int i;

	for (i = 0; i < LX_HRTIMER_BASES; i++)
		timerqueue_init_head(&lx_hrtimer_bases[i].active);
	return 0;
}
pure_initcall(lx_hrtimer_init);

/* The time of a base's clock now. */
static ktime_t lx_hrtimer_now(const struct hrtimer_clock_base *base)
{
	switch (base->clockid) {
	case CLOCK_REALTIME:
		return ktime_get_real();
	case CLOCK_BOOTTIME:
		return ktime_get_boottime();
	case CLOCK_TAI:
		return ktime_get_clocktai();
	default:
		return ktime_get();
	}
}

ktime_t hrtimer_cb_get_time(const struct hrtimer *timer)
{
	return lx_hrtimer_now(timer->base);
}

/* The base a timer on `clock_id` goes in, as Linux's hrtimer_clockid_to_base(). */
static struct hrtimer_clock_base *lx_hrtimer_base(clockid_t clock_id)
{
	switch (clock_id) {
	case CLOCK_REALTIME:
		return &lx_hrtimer_bases[HRTIMER_BASE_REALTIME];
	case CLOCK_BOOTTIME:
		return &lx_hrtimer_bases[HRTIMER_BASE_BOOTTIME];
	case CLOCK_TAI:
		return &lx_hrtimer_bases[HRTIMER_BASE_TAI];
	case CLOCK_MONOTONIC:
		return &lx_hrtimer_bases[HRTIMER_BASE_MONOTONIC];
	default:
		WARN(1, "hrtimer on clock %d: taken as monotonic\n", clock_id);
		return &lx_hrtimer_bases[HRTIMER_BASE_MONOTONIC];
	}
}

void hrtimer_setup(struct hrtimer *timer, enum hrtimer_restart (*function)(struct hrtimer *),
		   clockid_t clock_id, enum hrtimer_mode mode)
{
	memset(timer, 0, sizeof(*timer));
	timerqueue_init(&timer->node);
	timer->base = lx_hrtimer_base(clock_id);
	timer->is_soft = !!(mode & HRTIMER_MODE_SOFT);
	timer->is_hard = !!(mode & HRTIMER_MODE_HARD);
	ACCESS_PRIVATE(timer, function) = function;
}

static void lx_hrtimer_dequeue(struct hrtimer *timer)
{
	if (timer->state & HRTIMER_STATE_ENQUEUED) {
		timerqueue_del(&timer->base->active, &timer->node);
		timer->state = HRTIMER_STATE_INACTIVE;
	}
}

static void lx_hrtimer_enqueue(struct hrtimer *timer)
{
	timerqueue_add(&timer->base->active, &timer->node);
	timer->state = HRTIMER_STATE_ENQUEUED;
}

/*
 * `lhs` + `rhs` -- a relative timer's expiry, a periodic one's next
 * (hrtimer_forward) -- or the furthest time Linux gives (KTIME_SEC_MAX
 * seconds) where the sum is below 0 or below either operand, as Linux's
 * own judges it: an overflow, and a negative operand too, whose timer then
 * never fires there either.
 */
ktime_t ktime_add_safe(const ktime_t lhs, const ktime_t rhs)
{
	ktime_t sum = (ktime_t)((u64)lhs + (u64)rhs);

	if (sum < 0 || sum < lhs || sum < rhs)
		return ktime_set(KTIME_SEC_MAX, 0);
	return sum;
}

void hrtimer_start_range_ns(struct hrtimer *timer, ktime_t tim, u64 range_ns,
			    const enum hrtimer_mode mode)
{
	lx_hrtimer_dequeue(timer);
	timer->is_rel = !!(mode & HRTIMER_MODE_REL);
	if (timer->is_rel)
		tim = ktime_add_safe(tim, lx_hrtimer_now(timer->base));
	hrtimer_set_expires(timer, tim);
	lx_hrtimer_enqueue(timer);
}

/*
 * 1 when the timer was queued and is not now, 0 when it was not queued, -1
 * while its callback runs -- which, on the process's one thread, is only
 * from within that callback or a task it woke, never to end while it waits.
 */
int hrtimer_try_to_cancel(struct hrtimer *timer)
{
	if (hrtimer_callback_running(timer))
		return -1;
	if (!(timer->state & HRTIMER_STATE_ENQUEUED))
		return 0;
	lx_hrtimer_dequeue(timer);
	return 1;
}

int hrtimer_cancel(struct hrtimer *timer)
{
	int ret = hrtimer_try_to_cancel(timer);

	/* Linux would wait for the callback; here that would never end. */
	if (WARN_ON_ONCE(ret < 0))
		return 0;
	return ret;
}

bool hrtimer_active(const struct hrtimer *timer)
{
	return (timer->state & HRTIMER_STATE_ENQUEUED) || timer->base->running == timer;
}

/*
 * Move a periodic timer's expiry past `now` by whole `interval`s, as its
 * callback does before HRTIMER_RESTART: how many intervals it moved, 0
 * when its expiry is past `now` already.
 */
u64 hrtimer_forward(struct hrtimer *timer, ktime_t now, ktime_t interval)
{
	ktime_t late = ktime_sub(now, hrtimer_get_expires(timer));
	u64 steps;

	if (late < 0)
		return 0;
	if (WARN_ON(timer->state & HRTIMER_STATE_ENQUEUED))
		return 0;
	interval = max_t(ktime_t, interval, hrtimer_resolution);
	steps = div64_u64(late, interval) + 1;
	hrtimer_add_expires_ns(timer, steps * interval);
	return steps;
}

/* A due timer's callback, in its context; queued again if it says so. */
static void lx_hrtimer_fire(struct hrtimer_clock_base *base, struct hrtimer *timer)
{
	enum hrtimer_restart (*fn)(struct hrtimer *) = ACCESS_PRIVATE(timer, function);
	enum hrtimer_restart restart;
	unsigned long flags;

	lx_hrtimer_dequeue(timer);
	base->running = timer;
	if (timer->is_soft) {
		local_bh_disable();
		restart = fn(timer);
		local_bh_enable();
	} else {
		lx_hardirq_enter(flags);
		restart = fn(timer);
		lx_hardirq_exit(flags);
	}
	base->running = NULL;
	/* Not if the callback restarted it itself (hrtimer_start). */
	if (restart == HRTIMER_RESTART && !(timer->state & HRTIMER_STATE_ENQUEUED))
		lx_hrtimer_enqueue(timer);
}

/*
 * Every timer due by its clock, in order of expiry: the loop's, at each turn.
 * A base's clock is read only while it has a timer queued -- each reading is
 * a system call, and a turn comes at every task switch.
 */
void lx_hrtimer_run(void)
{
	unsigned int i;

	for (i = 0; i < LX_HRTIMER_BASES; i++) {
		struct hrtimer_clock_base *base = &lx_hrtimer_bases[i];
		struct timerqueue_node *next;
		ktime_t now;

		if (!timerqueue_getnext(&base->active))
			continue;
		now = lx_hrtimer_now(base);
		while ((next = timerqueue_getnext(&base->active)) && next->expires <= now)
			lx_hrtimer_fire(base, container_of(next, struct hrtimer, node));
	}
}

/*
 * When the earliest queued timer is due, by the monotonic clock reading
 * `now_ns`: KTIME_MAX for none.
 */
u64 lx_hrtimer_next(u64 now_ns)
{
	u64 next = KTIME_MAX;
	unsigned int i;

	for (i = 0; i < LX_HRTIMER_BASES; i++) {
		struct hrtimer_clock_base *base = &lx_hrtimer_bases[i];
		struct timerqueue_node *first = timerqueue_getnext(&base->active);
		ktime_t left;

		if (!first)
			continue;
		left = ktime_sub(first->expires, lx_hrtimer_now(base));
		next = min(next, left <= 0 ? now_ns : now_ns + (u64)left);
	}
	return next;
}
