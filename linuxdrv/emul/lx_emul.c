// SPDX-License-Identifier: MIT
/*
 * The core of the driver layer's Linux-facing half (design-decisions 1578,
 * 1580): what every Linux driver process needs whatever its driver --
 * the process's one task, interrupt flags, printk, kmalloc, jiffies,
 * randomness, and running the driver's initcalls.
 *
 * Compiled with the flags Linux's own build gives a driver file, so that
 * every declaration it defines a symbol for is Linux's own, checked by the
 * compiler against what it defines.
 */
#include <linux/init.h>
#include <linux/kernel.h>
#include <linux/printk.h>
#include <linux/slab.h>
#include <linux/jiffies.h>
#include <linux/sched.h>
#include <linux/percpu.h>
#include <asm/preempt.h>
#include <linux/tracepoint-defs.h>
#include <linux/random.h>
#include <linux/notifier.h>
#include <linux/wait_bit.h>

#include "slx.h"
#include "lx_emul.h"

/* Interrupt handlers may run (the shadow <asm/irqflags.h>). */
unsigned long slx_irq_flags = X86_EFLAGS_IF;

void slx_halted(void)
{
	slx_stop("halt", __builtin_return_address(0));
}

/*
 * The first task's record: what `current` names while the driver's
 * initcalls run (the others' are lx_sched.c's), counted as one reference
 * that is never let go. The per-CPU variable is a plain global with this
 * configuration (no SMP), as every one is.
 */
struct task_struct lx_main_task = { .usage = REFCOUNT_INIT(1) };
DEFINE_PER_CPU_CACHE_HOT(struct task_struct *, current_task) = &lx_main_task;

/* Not in an interrupt, nothing disabled: what a task's count reads. */
DEFINE_PER_CPU_CACHE_HOT(int, __preempt_count) = PREEMPT_ENABLED;

/*
 * Linux's tick count, starting where Linux's does, five minutes short of
 * wrapping, so that a driver's wrong comparison shows early. `jiffies` is
 * this variable under its other name, as Linux's linker script makes it
 * (`linuxdrv/link.ld`).
 */
u64 __cacheline_aligned_in_smp jiffies_64 = INITIAL_JIFFIES;

/*
 * The monotonic clock reads `now_ns`: jiffies count the ticks since the
 * first time it was read, at HZ, from INITIAL_JIFFIES.
 */
static void lx_emul_tick(u64 now_ns)
{
	static u64 start_ns;

	if (!start_ns)
		start_ns = now_ns;
	jiffies_64 = INITIAL_JIFFIES + (now_ns - start_ns) / (NSEC_PER_SEC / HZ);
}

void lx_emul_jiffies_now(void)
{
	lx_emul_tick(slx_now_ns());
}

/* When the running task was switched to, by the monotonic clock. */
u64 lx_switched_ns;

/*
 * The Rust half's hook, at every switch (linuxdrv/kit, sched.rs): the task
 * whose record is `task` is about to run, and is `current`, the clock
 * reading `now_ns`. The first task, Linux's initcalls, has no record of its
 * own but the main one. Every task is switched out here, so the RCU
 * callbacks queued so far have had their grace period, and run first.
 */
void lx_emul_switch_to(void *task, u64 now_ns)
{
	/*
	 * Through a pointer to this CPU's copy: the per-CPU write macros
	 * dispatch on the value's size, and refuse a pointer.
	 */
	*raw_cpu_ptr(&current_task) = task ? task : &lx_main_task;
	lx_switched_ns = now_ns;
	lx_emul_tick(now_ns);
	lx_rcu_run();
}

/*
 * The Rust half's hook for a task its deadline woke: Linux's record reads
 * running, as the wake of a Linux timer leaves it, so that a wake that
 * comes before the task runs finds it awake (try_to_wake_up() answers 0),
 * and an exclusive wake goes to a waiter still waiting.
 */
void lx_emul_timed_out(void *task)
{
	struct task_struct *p = task ? task : &lx_main_task;

	WRITE_ONCE(p->__state, TASK_RUNNING);
}

/*
 * A busy wait's turn (cpu_relax(), the shadow <asm/vdso/processor.h>). The
 * clock is a system call, so it is read once in 16 turns: a loop waiting on
 * jiffies spins a few turns longer, a loop waiting on anything else is not
 * slowed by a system call each turn.
 */
void slx_cpu_relax(void)
{
	static unsigned int turns;

	if (++turns % 16 == 0)
		lx_emul_jiffies_now();
}

/*
 * kmalloc's per-size caches. Linux's inline kmalloc picks one by size and
 * passes it to __kmalloc_cache_noprof with the size; here they are all
 * NULL, and the size is what is used.
 */
kmem_buckets kmalloc_caches[NR_KMALLOC_TYPES];

static void *lx_alloc(size_t size, gfp_t flags)
{
	void *p;

	/* As Linux's: nothing to hand out, and a pointer no access may use. */
	if (!size)
		return ZERO_SIZE_PTR;
	p = slx_alloc(size);

	if (p && (flags & __GFP_ZERO))
		memset(p, 0, size);
	return p;
}

void *__kmalloc_noprof(size_t size, gfp_t flags)
{
	return lx_alloc(size, flags);
}

void *__kmalloc_cache_noprof(struct kmem_cache *s, gfp_t flags, size_t size)
{
	return lx_alloc(size, flags);
}

void kfree(const void *p)
{
	/* NULL and ZERO_SIZE_PTR are Linux's to free, as nothing. */
	if (ZERO_OR_NULL_PTR(p))
		return;
	slx_free(p);
}

/*
 * Randomness, from the system's CSPRNG, which the kernel seeds before any
 * process starts: Linux's random number generator is always "ready" in a
 * driver process, so what waits for it runs at once, as Linux runs it once
 * its generator is (drivers/char/random.c).
 */
void get_random_bytes(void *buf, size_t len)
{
	slx_random(buf, len);
}

int execute_with_initialized_rng(struct notifier_block *nb)
{
	nb->notifier_call(nb, 0, NULL);
	return 0;
}

/*
 * The names vsprintf prints page, GFP and VMA flags by (%pGp, %pGg, %pGv).
 * Linux's own are mm/debug.c's, built from mm's flag lists, which the layer
 * does not compile; empty, each terminated as vsprintf expects, so a driver
 * printing flags gets them in hex. Declared in mm/internal.h, which is not
 * on a driver's include path.
 */
const struct trace_print_flags pageflag_names[] = { { 0, NULL } };
const struct trace_print_flags gfpflag_names[] = { { 0, NULL } };
const struct trace_print_flags vmaflag_names[] = { { 0, NULL } };

/*
 * The console, a line at a time: printk's level prefix (KERN_*) taken off
 * and "[lx] " put on, and a line longer than the buffer cut where it ends.
 */
static void lx_vprint(const char *prefix, const char *fmt, va_list args)
{
	char line[512];
	const char *text = printk_skip_level(fmt);
	size_t at = strscpy(line, prefix, sizeof(line));
	size_t len;

	if ((ssize_t)at < 0)
		at = 0;
	len = at + vscnprintf(line + at, sizeof(line) - at, text, args);
	/* slx_log takes a length: the terminator's byte can hold the newline. */
	if (len > 0 && line[len - 1] != '\n' && len < sizeof(line))
		line[len++] = '\n';
	slx_log(line, len);
}

int _printk(const char *fmt, ...)
{
	va_list args;

	va_start(args, fmt);
	lx_vprint("[lx] ", fmt, args);
	va_end(args);
	return 0;
}

void __warn_printk(const char *fmt, ...)
{
	va_list args;

	va_start(args, fmt);
	lx_vprint("[lx] WARNING: ", fmt, args);
	va_end(args);
}

/*
 * The shadow <asm/bug.h>'s WARN() and BUG() (linuxdrv/shadow/bug.h), which
 * Linux's own reach through `ud2` and the kernel's invalid-opcode handler.
 *
 * A warning is said and the driver goes on; WARN_ONCE's is said once per
 * place, as Linux marks its __bug_table entry done. The places are kept in
 * a small table: past its size a WARN_ONCE is said every time, which loses
 * nothing but quiet.
 */
#define LX_WARNED_ONCE 64
static struct { const char *file; int line; } lx_warned[LX_WARNED_ONCE];
unsigned long lx_warnings;

void slx_warn(const char *file, int line, unsigned int flags)
{
	int i;

	if (flags & BUGFLAG_ONCE) {
		for (i = 0; i < LX_WARNED_ONCE && lx_warned[i].file; i++)
			if (lx_warned[i].file == file && lx_warned[i].line == line)
				return;
		if (i < LX_WARNED_ONCE) {
			lx_warned[i].file = file;
			lx_warned[i].line = line;
		}
	}
	lx_warnings++;
	pr_warn("WARNING at %s:%d\n", file, line);
}

void slx_bug(const char *file, int line)
{
	pr_err("BUG at %s:%d\n", file, line);
	slx_exit(SLX_EXIT_BUG);
}

/*
 * The driver's initcalls, in Linux's order (the linker script keeps the
 * levels in sequence between these two symbols), each an offset from its
 * own entry (CONFIG_HAVE_ARCH_PREL32_RELOCATIONS).
 */
extern initcall_entry_t __initcall_start[], __initcall_end[];

int lx_emul_main(void)
{
	initcall_entry_t *entry;

	lx_main_handle = slx_task_current();
	/* What Linux's sched_init() sets up before any initcall runs. */
	wait_bit_init();
	for (entry = __initcall_start; entry < __initcall_end; entry++) {
		initcall_t fn = initcall_from_entry(entry);
		int ret = fn();

		if (ret) {
			/* By address: naming it would need the symbol table. */
			pr_err("initcall at %#lx failed: %d\n", (unsigned long)fn, ret);
			return SLX_EXIT_INIT_FAILED;
		}
	}
	return SLX_EXIT_OK;
}
