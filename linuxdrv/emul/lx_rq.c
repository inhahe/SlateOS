// SPDX-License-Identifier: MIT
/*
 * The scheduler's run queue, as much of it as Linux code outside the
 * scheduler touches. Since 6.18 migrate_disable() and migrate_enable() are
 * inline, and count a task's pinning in its CPU's run queue -- `nr_pinned`,
 * at an offset the build generates (RQ_nr_pinned) inside the per-CPU
 * `runqueues`. Linux's struct rq is the scheduler's own
 * (kernel/sched/sched.h), which the layer does not compile, and on one CPU
 * with no preemption the count means nothing; so here is storage reaching
 * past that field, without the type. This file includes no header that
 * declares `runqueues` (with CONFIG_SMP=n a per-CPU variable is a plain
 * global).
 */
#include <generated/rq-offsets.h>

char runqueues[RQ_nr_pinned + sizeof(unsigned int)] __attribute__((aligned(64)));
