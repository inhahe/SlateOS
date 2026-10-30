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
#include <linux/srcu.h>
#include <linux/async.h>
#include <linux/rcupdate.h>
#include <linux/swiotlb.h>
#include <linux/string.h>
#include <linux/ctype.h>
#include <linux/cred.h>
#include <linux/reboot.h>
#include <linux/panic.h>
#include <asm/sections.h>

/* The driver core's own setup, as Linux's driver_init() runs it. */
#include "drivers/base/base.h"

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
struct task_struct lx_main_task = {
	.usage = REFCOUNT_INIT(1),
	/* Allowed the one CPU, as every task's record says (lx_sched.c). */
	.cpus_ptr = &lx_main_task.cpus_mask,
	.cpus_mask = CPU_MASK_CPU0,
	.nr_cpus_allowed = 1,
};

/*
 * What `current` names between tasks, while the loop runs softirqs, timers
 * and RCU callbacks (lx_softirq.c): no task's, as Linux's idle task belongs
 * to no process, so that a wake done there reaches the task it names --
 * a timer waking the first task wakes it, where `current` being the first
 * task's own record would read as the task waking itself. Never freed,
 * never woken. Linux's name for it, init_task: the firmware loader reads
 * files as it (kernel credentials, prepare_kernel_cred() below).
 */
struct task_struct init_task = {
	.usage = REFCOUNT_INIT(1),
	.cpus_ptr = &init_task.cpus_mask,
	.cpus_mask = CPU_MASK_CPU0,
	.nr_cpus_allowed = 1,
};

DEFINE_PER_CPU_CACHE_HOT(struct task_struct *, current_task) = &lx_main_task;

/*
 * The process's one CPU, as Linux's kernel/cpu.c describes the CPUs it
 * has: possible, present, online and active, the one-bit mask
 * get_cpu_mask() hands out for it (cpu_bit_bitmap's row 1 + cpu), and
 * every CPU (cpu_all_mask).
 */
struct cpumask __cpu_possible_mask __ro_after_init = { CPU_BITS_ALL };
struct cpumask __cpu_present_mask __read_mostly = { CPU_BITS_ALL };
struct cpumask __cpu_online_mask __read_mostly = { CPU_BITS_ALL };
struct cpumask __cpu_active_mask __read_mostly = { CPU_BITS_ALL };
const unsigned long cpu_bit_bitmap[BITS_PER_LONG + 1][BITS_TO_LONGS(NR_CPUS)] = {
	[1] = { 1UL },
};
const DECLARE_BITMAP(cpu_all_bits, NR_CPUS) = CPU_BITS_ALL;

/*
 * The CPU as Linux's code asks after it (boot_cpu_has() and the rest): no
 * feature that the kernel proper would have set up -- no APIC, so that
 * irq_work runs at the loop's tick rather than from a self-interrupt it
 * cannot send -- and Linux's cache line, 64 bytes.
 */
struct cpuinfo_x86 boot_cpu_data = {
	.x86_cache_alignment = 64,
	.x86_clflush_size = 64,
};

/*
 * What Linux's delay-loop calibration would find on a 4 GHz CPU. A driver
 * process never calibrates -- its delays are the clock's (lx_sched.c) --
 * but workqueues scale their CPU-intensive threshold by it: at Linux's
 * uncalibrated value they would wait a whole second before letting the
 * work behind a CPU hog run, rather than Linux's 10 ms.
 */
unsigned long loops_per_jiffy = 4000000000UL / HZ;

/* No initcall's time is reported (init/main.c's initcall_debug, off). */
bool initcall_debug;

/*
 * kmalloc works from the first instruction (lx_mm_init() runs before
 * anything): Linux's code that asks -- a module parameter's string copied
 * into memory of its own (kernel/params.c) -- is told so.
 */
bool slab_is_available(void)
{
	return true;
}

/*
 * The kernel's own credentials, which the firmware loader reads its files
 * under (drivers/base/firmware_loader/main.c), so that a driver serving
 * some user reads them as the kernel, not as that user. A driver process
 * has one authority, its own, which the system checks at each file call
 * (lx_fs.c); Linux's record is kept only for its count, root's ids being
 * zero as kzalloc leaves them. Nothing reads a credential under RCU here,
 * so the last reference frees it at once.
 */
struct cred *prepare_kernel_cred(struct task_struct *daemon)
{
	struct cred *new = kzalloc(sizeof(*new), GFP_KERNEL);

	if (new)
		atomic_long_set(&new->usage, 1);
	return new;
}

void __put_cred(struct cred *cred)
{
	kfree(cred);
}

/*
 * What Linux tells before a restart or a power-off: a driver process is
 * never shut down so -- it ends, and the system takes back its device --
 * so the chain is kept, as Linux's, and never called. The firmware loader
 * puts a waiter for user-space-loaded firmware on it, which a driver
 * process has none of.
 */
static BLOCKING_NOTIFIER_HEAD(reboot_notifier_list);

int register_reboot_notifier(struct notifier_block *nb)
{
	return blocking_notifier_chain_register(&reboot_notifier_list, nb);
}

int unregister_reboot_notifier(struct notifier_block *nb)
{
	return blocking_notifier_chain_unregister(&reboot_notifier_list, nb);
}

/*
 * Linux code marking the kernel tainted -- a parameter marked dangerous
 * set (kernel/params.c), a warning -- said, and kept as Linux keeps it, in
 * a mask of TAINT_* bits nothing here reads.
 */
static unsigned long lx_tainted_mask;

void add_taint(unsigned flag, enum lockdep_ok lockdep_ok)
{
	if (!test_and_set_bit(flag, &lx_tainted_mask))
		pr_warn("tainted: flag %u\n", flag);
}

/*
 * Where start-up has got to, as init/main.c moves it: booting until the
 * first task can sleep and kthreadd can make threads, scheduling while the
 * initcalls run, running once they are done (lx_emul_main).
 */
enum system_states system_state __read_mostly;

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

/* Linux's monotonic clock, as its timekeeping reads it: the system's. */
ktime_t ktime_get(void)
{
	return slx_now_ns();
}

/*
 * Linux's other clocks, as offsets from the monotonic one: real time is
 * the system's wall clock; boot time is monotonic, a driver process never
 * having seen a suspend; TAI is real time, its offset from UTC being 0
 * until NTP sets one, as in Linux.
 */
ktime_t ktime_get_with_offset(enum tk_offsets offs)
{
	return offs == TK_OFFS_BOOT ? (ktime_t)slx_now_ns() : (ktime_t)slx_real_ns();
}

void ktime_get_real_ts64(struct timespec64 *ts)
{
	*ts = ns_to_timespec64(slx_real_ns());
}

time64_t ktime_get_real_seconds(void)
{
	return div_u64(slx_real_ns(), NSEC_PER_SEC);
}

/* The monotonic clock's whole seconds (mac80211 times a station's connection by them). */
time64_t ktime_get_seconds(void)
{
	return div_u64(slx_now_ns(), NSEC_PER_SEC);
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
	if (!p)
		return NULL;
	lx_mm_mark_slab(p, size);
	if (flags & __GFP_ZERO)
		memset(p, 0, size);
	return p;
}

/*
 * What kmalloc() really hands out for `size`, and what a block it gave
 * holds: Linux code sizes buffers by these so as to use the whole block
 * (devres's records, socket buffers). As Linux's, 0 for nothing, and for a
 * pointer that is no allocation's.
 */
size_t kmalloc_size_roundup(size_t size)
{
	return size ? slx_alloc_size(size) : 0;
}

size_t ksize(const void *objp)
{
	return ZERO_OR_NULL_PTR(objp) ? 0 : slx_block_size(objp);
}

/* A copy of `len` bytes at `src`, in a block of its own. */
void *kmemdup_noprof(const void *src, size_t len, gfp_t gfp)
{
	void *p = lx_alloc(len, gfp & ~__GFP_ZERO);

	if (!ZERO_OR_NULL_PTR(p))
		memcpy(p, src, len);
	return p;
}

/* ... of `count` elements of `element_size` bytes: NULL when that overflows. */
void *kmemdup_array(const void *src, size_t count, size_t element_size, gfp_t gfp)
{
	size_t len;

	if (check_mul_overflow(count, element_size, &len))
		return NULL;
	return kmemdup_noprof(src, len, gfp);
}

/* A block freed with its whole contents wiped first: keys, passphrases. */
void kfree_sensitive(const void *objp)
{
	size_t len = ksize(objp);

	if (len)
		memzero_explicit((void *)objp, len);
	kfree(objp);
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
 * A large kmalloc() -- a size known at compile time past Linux's largest
 * cache, two pages -- is the page allocator's in Linux, page-aligned and its
 * pages no slab's. The same allocator serves it here as every other size
 * (its blocks are aligned to their size, so a page at least), and its pages
 * say what Linux's would.
 */
static void *lx_alloc_large(size_t size, gfp_t flags)
{
	void *p = slx_alloc(size);

	if (!p)
		return NULL;
	lx_mm_mark_pages(p, size);
	if (flags & __GFP_ZERO)
		memset(p, 0, size);
	return p;
}

void *__kmalloc_large_noprof(size_t size, gfp_t flags)
{
	return lx_alloc_large(size, flags);
}

void *__kmalloc_large_node_noprof(size_t size, gfp_t flags, int node)
{
	return lx_alloc_large(size, flags);
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
	unsigned int object_size;
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
	s->object_size = object_size;
	s->ctor = args ? args->ctor : NULL;
	return s;
}

/* The size of the cache's objects, as created. */
unsigned int kmem_cache_size(struct kmem_cache *s)
{
	return s->object_size;
}

static void *lx_cache_alloc(struct kmem_cache *s, gfp_t flags);

/*
 * Objects by the batch (the network core keeps a stock of socket buffer
 * heads this way): `size` of them into `p`, all or none, as Linux's.
 * How many, or 0.
 */
int kmem_cache_alloc_bulk_noprof(struct kmem_cache *s, gfp_t flags, size_t size, void **p)
{
	size_t i;

	for (i = 0; i < size; i++) {
		p[i] = lx_cache_alloc(s, flags);
		if (!p[i]) {
			while (i--)
				kfree(p[i]);
			return 0;
		}
	}
	return size;
}

void kmem_cache_free_bulk(struct kmem_cache *s, size_t size, void **p)
{
	size_t i;

	for (i = 0; i < size; i++)
		kfree(p[i]);
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
 * Strings duplicated, as Linux's mm/util.c duplicates them -- written here,
 * that file being otherwise the memory manager's (mappings, overcommit,
 * folios), which a driver process has none of. A constant string, in the
 * process's read-only data, is handed back itself, and never freed.
 */
static char *lx_kmemdup_nul(const char *s, size_t len, gfp_t gfp)
{
	char *buf = kmalloc_track_caller(len + 1, gfp);

	if (!buf)
		return NULL;
	memcpy(buf, s, len);
	buf[len] = '\0';
	return buf;
}

char *kmemdup_nul(const char *s, size_t len, gfp_t gfp)
{
	return s ? lx_kmemdup_nul(s, len, gfp) : NULL;
}

char *kstrdup(const char *s, gfp_t gfp)
{
	return s ? lx_kmemdup_nul(s, strlen(s), gfp) : NULL;
}

char *kstrndup(const char *s, size_t max, gfp_t gfp)
{
	return s ? lx_kmemdup_nul(s, strnlen(s, max), gfp) : NULL;
}

const char *kstrdup_const(const char *s, gfp_t gfp)
{
	if (is_kernel_rodata((unsigned long)s))
		return s;
	return kstrdup(s, gfp);
}

void kfree_const(const void *x)
{
	if (!is_kernel_rodata((unsigned long)x))
		kfree(x);
}

/*
 * A new device's bounce buffers (device_initialize()): none. Every buffer a
 * driver hands its device lies in memory the device can reach already, so
 * no copy through a buffer it can reach is ever needed; the device keeps no
 * pool, and the layer's DMA mapping never consults one.
 */
void swiotlb_dev_init(struct device *dev)
{
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

/* A uniform random number below `ceil`: Lemire's multiply-and-shift, its biased low end redrawn. */
u32 __get_random_u32_below(u32 ceil)
{
	u32 r;
	u64 mult;

	get_random_bytes(&r, sizeof(r));
	mult = (u64)ceil * r;
	if (unlikely((u32)mult < ceil)) {
		u32 bound = -ceil % ceil;

		while (unlikely((u32)mult < bound)) {
			get_random_bytes(&r, sizeof(r));
			mult = (u64)ceil * r;
		}
	}
	return mult >> 32;
}

u16 get_random_u16(void)
{
	u16 r;

	get_random_bytes(&r, sizeof(r));
	return r;
}

u32 get_random_u32(void)
{
	u32 r;

	get_random_bytes(&r, sizeof(r));
	return r;
}

/*
 * Device data mixed into Linux's pool (a MAC address, as a device
 * registers): the system's generator is seeded already, and owes nothing
 * to what a driver process could add.
 */
void add_device_randomness(const void *buf, size_t len)
{
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
static int lx_vprint(const char *prefix, const char *fmt, va_list args)
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
	return len;
}

int _printk(const char *fmt, ...)
{
	va_list args;
	int len;

	va_start(args, fmt);
	len = lx_vprint("[lx] ", fmt, args);
	va_end(args);
	return len;
}

/*
 * What dev_printk() and the rest reach (drivers/base/core.c): a line already
 * formatted with the device's driver and name. Its level and the device's
 * details for the log's metadata (`dev_info`) the console does not keep.
 */
int vprintk_emit(int facility, int level, const struct dev_printk_info *dev_info,
		 const char *fmt, va_list args)
{
	return lx_vprint("[lx] ", fmt, args);
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

void lx_end_run(const char *who, int err)
{
	if (!err && lx_warnings) {
		pr_err("%s: Linux warned %lu time(s) in a run that must not\n", who, lx_warnings);
		slx_exit(SLX_EXIT_WARNED);
	}
	if (err)
		pr_err("%s: the run failed: %d\n", who, err);
	slx_exit(err ? SLX_EXIT_INIT_FAILED : SLX_EXIT_OK);
}

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
 * Each level's initcalls, as Linux's init/main.c finds them: a level runs
 * from its start to the next one's, the last to __initcall_end, the early
 * ones (core_initcall's "early" kin) before level 0 (linuxdrv/link.ld).
 */
extern initcall_entry_t __initcall0_start[], __initcall1_start[], __initcall2_start[],
	__initcall3_start[], __initcall4_start[], __initcall5_start[], __initcall6_start[],
	__initcall7_start[];

static initcall_entry_t *const lx_initcall_levels[] = {
	__initcall0_start, __initcall1_start, __initcall2_start, __initcall3_start,
	__initcall4_start, __initcall5_start, __initcall6_start, __initcall7_start,
	__initcall_end,
};

/* The levels' names, as Linux's parse_args() says them in a refusal. */
static const char *const lx_initcall_level_names[] = {
	"pure", "core", "postcore", "arch", "subsys", "fs", "device", "late",
};
static_assert(ARRAY_SIZE(lx_initcall_levels) == ARRAY_SIZE(lx_initcall_level_names) + 1);

/* The first argument "<key><value>": its value, or NULL. */
const char *lx_arg(const char *key)
{
	size_t n = strlen(key);
	int i;

	for (i = 1; i < slx_argc(); i++)
		if (!strncmp(slx_argv(i), key, n))
			return slx_argv(i) + n;
	return NULL;
}

/*
 * The process's arguments that name module parameters, as one line of
 * Linux's command line: built once (lx_emul_init), copied for each pass,
 * which parse_args() cuts up, and freed once the initcalls are done.
 */
static char *lx_cmdline;

/*
 * Whether the argument `arg` names a module parameter: a dot in its name
 * (module.parameter), before any '='. Every parameter a driver process
 * carries has one -- Linux's undotted ones, core_param(), are the kernel
 * proper's -- and the other arguments are the layer's own (pci=, serve=,
 * ...) and a driver's mode words, which parse_args() never sees.
 */
static bool lx_names_param(const char *arg)
{
	const char *dot = strchr(arg, '.'), *eq = strchr(arg, '=');

	return dot && (!eq || dot < eq);
}

static char *lx_put(char *at, const char *s, size_t n)
{
	memcpy(at, s, n);
	return at + n;
}

/*
 * lx_cmdline made: each argument naming a parameter, separated by spaces,
 * one holding a space with its value quoted (name="value") -- each is one
 * argument already, which an unquoted space would cut in two -- as the
 * command line quotes one. One holding a quote as well cannot be carried,
 * the command line having no escape: refused. An exit code.
 */
static int lx_cmdline_build(void)
{
	size_t len = 1;
	char *at;
	int i;

	for (i = 1; i < slx_argc(); i++)
		if (lx_names_param(slx_argv(i)))
			/* Two quotes at most, and a space. */
			len += strlen(slx_argv(i)) + 3;
	lx_cmdline = at = kmalloc(len, GFP_KERNEL);
	if (!lx_cmdline)
		return SLX_EXIT_NO_MEMORY;
	for (i = 1; i < slx_argc(); i++) {
		const char *arg = slx_argv(i), *eq = strchr(arg, '='), *c;
		size_t n = strlen(arg), head;
		bool spaced = false;

		if (!lx_names_param(arg))
			continue;
		for (c = arg; *c; c++)
			spaced |= isspace(*c);
		if (spaced && strchr(arg, '"')) {
			pr_err("Setting parameters: `%s' holds both a space and a quote, which no command line carries\n",
			       arg);
			return SLX_EXIT_ARGUMENT;
		}
		/* What goes before the quotes: the name and its '=', or nothing. */
		head = !spaced ? n : eq ? eq + 1 - arg : 0;
		at = lx_put(at, arg, head);
		if (spaced) {
			*at++ = '"';
			at = lx_put(at, arg + head, n - head);
			*at++ = '"';
		}
		*at++ = ' ';
	}
	*at = '\0';
	return SLX_EXIT_OK;
}

/*
 * Linux's module parameters set from the process's arguments, as Linux
 * sets them from its command line (kernel/params.c's parse_args), those of
 * initcall level `level`: -1, a module's own (module_param), before
 * anything but memory is set up, as start_kernel() sets them; each other
 * level's just before its initcalls run, as do_initcall_level(). An exit
 * code: parse_args() says what it did not take -- a name no parameter has
 * (at -1, where every name is looked up), or a value its kind refuses (at
 * the parameter's own level, the earlier levels' initcalls having run) --
 * and the process ends for it (SLX_EXIT_ARGUMENT).
 */
static int lx_params(int level)
{
	char *line, *err;

	if (!*lx_cmdline)
		return SLX_EXIT_OK;
	line = kstrdup(lx_cmdline, GFP_KERNEL);
	if (!line)
		return SLX_EXIT_NO_MEMORY;
	err = parse_args(level < 0 ? "Setting parameters" : lx_initcall_level_names[level], line,
			 __start___param, __stop___param - __start___param, level, level, NULL, NULL);
	kfree(line);
	return IS_ERR(err) ? SLX_EXIT_ARGUMENT : SLX_EXIT_OK;
}

/*
 * What Linux's start_kernel() sets up before anything runs, set up before
 * the Rust half's loop first turns (its turns run softirqs and timers): an
 * exit code, SLX_EXIT_OK when it is ready.
 */
int lx_emul_init(void)
{
	int ret;

	/* First: every kmalloc() marks its pages in the direct map's array. */
	if (lx_mm_init())
		return SLX_EXIT_NO_MEMORY;
	ret = lx_cmdline_build();
	if (!ret)
		ret = lx_params(-1);
	if (ret)
		return ret;
	wait_bit_init();
	if (lx_softirq_init())
		return SLX_EXIT_NO_MEMORY;
	/* The radix tree's node cache, which idr and the workqueues number by. */
	radix_tree_init();
	workqueue_init_early();
	/* As start_kernel(): sleepable RCU's grace periods may start, by work items. */
	srcu_init();
	return SLX_EXIT_OK;
}

/*
 * Whether an initcall, the function at `fn`, ends the process by answering
 * `ret`: when it failed (a negative answer -- Linux's module loader takes a
 * positive one as success) and it is the driver's own code, which the build
 * marks (scripts/linux-tree.sh, __driver_text_start/_end). A Linux
 * subsystem's failing is Linux's own business, which Linux carries on past.
 */
bool lx_initcall_fatal(const void *fn, int ret)
{
	return ret < 0 && (const char *)fn >= __driver_text_start &&
	       (const char *)fn < __driver_text_end;
}

/*
 * The initcalls from `from` to `to`, in order: SLX_EXIT_OK, or
 * SLX_EXIT_INIT_FAILED at the first of the driver's own to fail.
 */
static int lx_run_initcalls(initcall_entry_t *from, initcall_entry_t *to)
{
	initcall_entry_t *entry;

	for (entry = from; entry < to; entry++) {
		initcall_t fn = initcall_from_entry(entry);
		int ret = fn();

		/* By address: naming it would need the symbol table. */
		if (lx_initcall_fatal((const void *)fn, ret)) {
			pr_err("initcall at %#lx failed: %d\n", (unsigned long)fn, ret);
			return SLX_EXIT_INIT_FAILED;
		}
		if (ret > 0)
			pr_warn("initcall at %#lx returned %d; taken as success\n",
				(unsigned long)fn, ret);
		else if (ret < 0)
			pr_warn("subsystem initcall at %#lx failed: %d; carrying on\n",
				(unsigned long)fn, ret);
	}
	return SLX_EXIT_OK;
}

int lx_emul_main(void)
{
	int level, ret;

	lx_main_handle = slx_task_current();
	/*
	 * As rest_init(): the scheduler runs, so RCU's grace periods are real --
	 * before it, synchronize_srcu() returns at once, nothing else running.
	 */
	rcu_scheduler_starting();
	if (lx_start_kthreadd())
		return SLX_EXIT_NO_MEMORY;
	system_state = SYSTEM_SCHEDULING;
	/* As Linux's kernel_init_freeable(): workers, once kthreadd can make them. */
	workqueue_init();
	workqueue_init_topology();
	/* Asynchronous probing's own workqueue, as Linux 6.9 on makes it here. */
	async_init();
	/*
	 * As Linux's driver_init() before the initcalls: the driver core's
	 * devices, buses and classes; the faux bus -- the home of a device that
	 * stands for no hardware, which cfg80211 makes its regulatory one
	 * (drivers/base/faux.c) -- and the platform bus, which a driver of no
	 * bus registers on (mac80211_hwsim's simulated radios). Its other
	 * pieces are for what a driver process has none of -- firmware nodes,
	 * hypervisors, CPUs, memory blocks and NUMA nodes as devices.
	 */
	if (devices_init() || buses_init() || classes_init() || lx_fs_init() || faux_bus_init() ||
	    platform_bus_init())
		return SLX_EXIT_NO_MEMORY;
	ret = lx_run_initcalls(__initcall_start, __initcall0_start);
	for (level = 0; !ret && level < ARRAY_SIZE(lx_initcall_level_names); level++) {
		ret = lx_params(level);
		if (!ret)
			ret = lx_run_initcalls(lx_initcall_levels[level],
					       lx_initcall_levels[level + 1]);
	}
	kfree(lx_cmdline);
	lx_cmdline = NULL;
	if (ret)
		return ret;
	/* A driver process exists to drive what it was given. */
	if (!lx_pci_check_bound())
		return SLX_EXIT_INIT_FAILED;
	system_state = SYSTEM_RUNNING;
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
