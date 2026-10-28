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
#include <linux/cpumask.h>
#include <linux/moduleparam.h>
#include <linux/device.h>
#include <linux/cache.h>
#include <linux/workqueue.h>
#include <linux/radix-tree.h>

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

/*
 * What `current` names between tasks, while the loop runs softirqs, timers
 * and RCU callbacks (lx_softirq.c): no task's, as Linux's idle task belongs
 * to no process, so that a wake done there reaches the task it names --
 * a timer waking the first task wakes it, where `current` being the first
 * task's own record would read as the task waking itself. Never freed,
 * never woken.
 */
struct task_struct lx_idle_task = { .usage = REFCOUNT_INIT(1) };

DEFINE_PER_CPU_CACHE_HOT(struct task_struct *, current_task) = &lx_main_task;

/*
 * The process's one CPU, as Linux's kernel/cpu.c describes the CPUs it
 * has: possible, present, online and active, and the one-bit mask
 * get_cpu_mask() hands out for it (cpu_bit_bitmap's row 1 + cpu).
 */
struct cpumask __cpu_possible_mask __ro_after_init = { CPU_BITS_ALL };
struct cpumask __cpu_present_mask __read_mostly = { CPU_BITS_ALL };
struct cpumask __cpu_online_mask __read_mostly = { CPU_BITS_ALL };
struct cpumask __cpu_active_mask __read_mostly = { CPU_BITS_ALL };
const unsigned long cpu_bit_bitmap[BITS_PER_LONG + 1][BITS_TO_LONGS(NR_CPUS)] = {
	[1] = { 1UL },
};

/*
 * What Linux's delay-loop calibration would find on a 4 GHz CPU. A driver
 * process never calibrates -- its delays are the clock's (lx_sched.c) --
 * but workqueues scale their CPU-intensive threshold by it: at Linux's
 * uncalibrated value they would wait a whole second before letting the
 * work behind a CPU hog run, rather than Linux's 10 ms.
 */
unsigned long loops_per_jiffy = 4000000000UL / HZ;

/*
 * Module parameters' kinds, which Linux code names in its parameter
 * tables. Nothing in a driver process sets a parameter -- it has no kernel
 * command line, and the tables are discarded at link (linuxdrv/link.ld) --
 * so a kind needs no operations.
 */
const struct kernel_param_ops param_ops_bool;
const struct kernel_param_ops param_ops_ulong;

/* Not in an interrupt, nothing disabled: what a task's count reads. */
DEFINE_PER_CPU_CACHE_HOT(int, __preempt_count) = PREEMPT_ENABLED;

/*
 * The monotonic clock reads `now_ns`: jiffies count the ticks since the
 * first time it was read, at HZ, from INITIAL_JIFFIES -- five minutes short
 * of wrapping, so that a driver's wrong comparison shows early. The
 * variable is Linux's own (kernel/time/timer.c's jiffies_64; `jiffies` is
 * its other name, as Linux's linker script makes it, `linuxdrv/link.ld`);
 * the layer is what moves it, as Linux's tick does.
 */
static u64 lx_start_ns;

void lx_emul_jiffies_at(u64 now_ns)
{
	if (!lx_start_ns)
		lx_start_ns = now_ns;
	jiffies_64 = INITIAL_JIFFIES + (now_ns - lx_start_ns) / TICK_NSEC;
}

/* When, by the monotonic clock, the current jiffy began. */
u64 lx_emul_jiffy_began_ns(void)
{
	return lx_start_ns + (u64)(jiffies - INITIAL_JIFFIES) * TICK_NSEC;
}

void lx_emul_jiffies_now(void)
{
	lx_emul_jiffies_at(slx_now_ns());
}

/* When the running task was switched to, by the monotonic clock. */
u64 lx_switched_ns;

/* Up to when the running task's run time has been counted. */
static u64 lx_accounted_ns;

void lx_emul_account(u64 now_ns)
{
	current->se.sum_exec_runtime += now_ns - lx_accounted_ns;
	lx_accounted_ns = now_ns;
}

/*
 * The Rust half's hook, at every switch (linuxdrv/kit, sched.rs): the task
 * whose record is `task` is about to run, and is `current`, the clock
 * reading `now_ns` (jiffies brought up to it by the turn's lx_emul_turn()).
 * The first task, Linux's initcalls, has no record of its own but the main
 * one.
 */
void lx_emul_switch_to(void *task, u64 now_ns)
{
	/*
	 * Through a pointer to this CPU's copy: the per-CPU write macros
	 * dispatch on the value's size, and refuse a pointer.
	 */
	*raw_cpu_ptr(&current_task) = task ? task : &lx_main_task;
	lx_switched_ns = now_ns;
	lx_accounted_ns = now_ns;
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

void *__kmalloc_node_noprof(DECL_BUCKET_PARAMS(size, b), gfp_t flags, int node)
{
	return lx_alloc(size, flags);
}

void *__kmalloc_cache_node_noprof(struct kmem_cache *s, gfp_t flags, int node, size_t size)
{
	return lx_alloc(size, flags);
}

/*
 * Slab caches: what kmem_cache_alloc() hands out, by size. kmalloc's buddy
 * allocator serves the objects -- aligned to their size rounded up to a
 * power of two, so to any alignment the cache asks when the size is raised
 * to it. A cache keeps what of Linux's matters here: the size, and the
 * constructor, run on each object as it is handed out (Linux runs it once
 * per object of a new slab; running it again on an object freed in its
 * constructed state is the same object). mm/slab.h's struct, which no
 * driver sees. What it cannot keep it refuses out loud: a cache whose
 * objects RCU readers may touch after their free (SLAB_TYPESAFE_BY_RCU)
 * needs its memory to stay the cache's, which kfree does not keep; and a
 * cache Linux would panic without (SLAB_PANIC) ends the process.
 */
struct kmem_cache {
	unsigned int size;
	void (*ctor)(void *object);
};

struct kmem_cache *__kmem_cache_create_args(const char *name, unsigned int object_size,
					    struct kmem_cache_args *args, slab_flags_t flags)
{
	struct kmem_cache *s;
	unsigned int align = args ? args->align : 0;

	if (flags & SLAB_TYPESAFE_BY_RCU)
		slx_stop("__kmem_cache_create_args (SLAB_TYPESAFE_BY_RCU)",
			 __builtin_return_address(0));
	s = kzalloc(sizeof(*s), GFP_KERNEL);
	if (!s && (flags & SLAB_PANIC)) {
		pr_err("no memory for the slab cache %s\n", name);
		slx_exit(SLX_EXIT_NO_MEMORY);
	}
	if (!s)
		return NULL;
	if (flags & SLAB_HWCACHE_ALIGN)
		align = max(align, (unsigned int)L1_CACHE_BYTES);
	s->size = max(object_size, align);
	s->ctor = args ? args->ctor : NULL;
	return s;
}

void kmem_cache_destroy(struct kmem_cache *s)
{
	kfree(s);
}

static void *lx_cache_alloc(struct kmem_cache *s, gfp_t flags)
{
	void *p = lx_alloc(s->size, flags);

	if (p && s->ctor)
		s->ctor(p);
	return p;
}

void *kmem_cache_alloc_noprof(struct kmem_cache *s, gfp_t flags)
{
	return lx_cache_alloc(s, flags);
}

void *kmem_cache_alloc_node_noprof(struct kmem_cache *s, gfp_t flags, int node)
{
	return lx_cache_alloc(s, flags);
}

void *kmem_cache_alloc_lru_noprof(struct kmem_cache *s, struct list_lru *lru, gfp_t flags)
{
	return lx_cache_alloc(s, flags);
}

void kmem_cache_free(struct kmem_cache *s, void *p)
{
	kfree(p);
}

/*
 * Per-CPU memory: one CPU, so one copy, and a per-CPU pointer is the
 * copy's address (per_cpu_ptr() adds nothing with this configuration).
 * Zeroed, as Linux's is.
 */
void __percpu *pcpu_alloc_noprof(size_t size, size_t align, bool reserved, gfp_t gfp)
{
	return (void __percpu __force *)lx_alloc(max(size, align), gfp | __GFP_ZERO);
}

void free_percpu(void __percpu *ptr)
{
	kfree((void __force *)ptr);
}

/*
 * sysfs, which a driver process has none of: what Linux code would show
 * there has nowhere to go, and a subsystem registering with it is told so.
 * Workqueues' core initcall (wq_sysfs_init) is, and carries on without.
 */
int subsys_virtual_register(const struct bus_type *subsys, const struct attribute_group **groups)
{
	return -ENODEV;
}

/* Deferred printk: every line goes straight out here, so nothing to defer. */
void __printk_deferred_enter(void)
{
}

void __printk_deferred_exit(void)
{
}

/* kmalloc_track_caller(), which kasprintf uses: one node, no tracking. */
void *__kmalloc_node_track_caller_noprof(DECL_BUCKET_PARAMS(size, b), gfp_t flags, int node,
					 unsigned long caller)
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
 * The process's initcalls, in Linux's order (the linker script keeps the
 * levels in sequence between __initcall_start and __initcall_end), each an
 * offset from its own entry (CONFIG_HAVE_ARCH_PREL32_RELOCATIONS); and where
 * the driver's own code lies (__driver_text_start/_end, which
 * scripts/linux-tree.sh marks), which tells the driver's initcalls from
 * Linux's own subsystems', whatever their level.
 */
extern initcall_entry_t __initcall_start[], __initcall_end[];
extern char __driver_text_start[], __driver_text_end[];

/*
 * What Linux's start_kernel() sets up before anything runs, set up before
 * the Rust half's loop first turns (its turns run softirqs and timers): an
 * exit code, SLX_EXIT_OK when it is ready.
 */
int lx_emul_init(void)
{
	wait_bit_init();
	if (lx_softirq_init())
		return SLX_EXIT_NO_MEMORY;
	/* The radix tree's node cache, which idr and the workqueues number by. */
	radix_tree_init();
	workqueue_init_early();
	return SLX_EXIT_OK;
}

int lx_emul_main(void)
{
	initcall_entry_t *entry;

	lx_main_handle = slx_task_current();
	if (lx_start_kthreadd())
		return SLX_EXIT_NO_MEMORY;
	/* As Linux's kernel_init_freeable(): workers, once kthreadd can make them. */
	workqueue_init();
	workqueue_init_topology();
	for (entry = __initcall_start; entry < __initcall_end; entry++) {
		initcall_t fn = initcall_from_entry(entry);
		bool drivers = (char *)fn >= __driver_text_start && (char *)fn < __driver_text_end;
		int ret = fn();

		/*
		 * By address: naming it would need the symbol table. Only a
		 * negative return is a failure: Linux's module loader takes a
		 * positive one as success. A Linux subsystem's failing is Linux's
		 * own business, which Linux carries on past, as here; the
		 * driver's ends the process.
		 */
		if (ret > 0)
			pr_warn("initcall at %#lx returned %d; taken as success\n",
				(unsigned long)fn, ret);
		if (ret >= 0)
			continue;
		if (!drivers) {
			pr_warn("subsystem initcall at %#lx failed: %d; carrying on\n",
				(unsigned long)fn, ret);
			continue;
		}
		pr_err("initcall at %#lx failed: %d\n", (unsigned long)fn, ret);
		return SLX_EXIT_INIT_FAILED;
	}
	return SLX_EXIT_OK;
}

/*
 * Population counts, for x86's arch_hweight.h, which calls them from
 * inline assembly that names no clobbered register: they keep every
 * register but the result's (%rax), taking the value in %rdi. Linux's own
 * assembly versions are part of a library its build will not make alone,
 * and lib/hweight.c's, plain C, may clobber what the caller keeps live; so
 * the layer's are assembly: the counts of pairs, then nibbles, then bytes,
 * summed by a multiply. Kernel code has no red zone, so the pushes are safe.
 */
asm(
"	.pushsection .text\n"
"	.globl	__sw_hweight64\n"
"	.type	__sw_hweight64, @function\n"
"__sw_hweight64:\n"
"	pushq	%rcx\n"
"	pushq	%rdx\n"
"	movq	%rdi, %rax\n"
"	movq	%rax, %rdx\n"
"	shrq	$1, %rdx\n"
"	movabsq	$0x5555555555555555, %rcx\n"
"	andq	%rcx, %rdx\n"
"	subq	%rdx, %rax\n"
"	movabsq	$0x3333333333333333, %rcx\n"
"	movq	%rax, %rdx\n"
"	shrq	$2, %rdx\n"
"	andq	%rcx, %rdx\n"
"	andq	%rcx, %rax\n"
"	addq	%rdx, %rax\n"
"	movq	%rax, %rdx\n"
"	shrq	$4, %rdx\n"
"	addq	%rdx, %rax\n"
"	movabsq	$0x0f0f0f0f0f0f0f0f, %rcx\n"
"	andq	%rcx, %rax\n"
"	movabsq	$0x0101010101010101, %rcx\n"
"	imulq	%rcx, %rax\n"
"	shrq	$56, %rax\n"
"	popq	%rdx\n"
"	popq	%rcx\n"
"	ret\n"
"	.size	__sw_hweight64, . - __sw_hweight64\n"
"	.globl	__sw_hweight32\n"
"	.type	__sw_hweight32, @function\n"
"__sw_hweight32:\n"
"	pushq	%rdi\n"
"	movl	%edi, %edi\n"
"	call	__sw_hweight64\n"
"	popq	%rdi\n"
"	ret\n"
"	.size	__sw_hweight32, . - __sw_hweight32\n"
"	.popsection\n"
);

/* kvmalloc()'s memory is kmalloc's here, so kvfree() is kfree(). */
void kvfree(const void *addr)
{
	kfree(addr);
}
