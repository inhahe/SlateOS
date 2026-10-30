// SPDX-License-Identifier: MIT
/*
 * Interrupts in a driver process (design-decisions 1585, 1587): a PCI
 * function's MSI, MSI-X or legacy line (INTx), which the kernel sets up
 * through its device handle, delivered to the handlers a driver requests as
 * Linux's interrupt core would deliver them.
 *
 * Linux's own MSI and interrupt cores are not compiled: they program the
 * device and the CPU's interrupt controller, which the kernel does here, and
 * hang every interrupt on irq domains a driver process has no use for. So
 * this file is the bottom of both, for what a driver calls: Linux's MSI API
 * (pci_alloc_irq_vectors(), pci_enable_msi(), pci_enable_msix_range(),
 * pci_irq_vector()) -- one MSI vector, or MSI-X vectors for a function's
 * first table entries, as many as the kernel gives (LX_IRQ_VECTORS at most);
 * and request_threaded_irq(), free_irq(), disable_irq() and enable_irq(),
 * synchronize_irq(). And a function's legacy line: dev->irq, from before
 * its driver probes (pcibios_alloc_irq), is a number of the layer's own for
 * it, set up as the kernel routes the line when a handler is first asked
 * for on it -- see lx_intx_poll() for how it is taken.
 *
 * Each turn of the loop asks the kernel which vectors of each function with
 * its MSI or MSI-X on have had interrupts since the last turn (lx_irq_poll),
 * and runs each one's handler as Linux runs a hard interrupt's: in hardirq context,
 * interrupts off, once however many came -- a message is an edge, which
 * Linux's own handlers read their device's cause register for. The turn's
 * softirqs run after it, as on Linux's irq_exit(). A threaded handler's
 * second half runs in a kthread of its own, as Linux's "irq/N-name" thread
 * runs it, woken when the first half answers IRQ_WAKE_THREAD, and run once
 * for however many asked. A message is safe without ONESHOT -- an edge
 * cannot storm while the thread runs -- so IRQF_ONESHOT is dropped, as
 * Linux drops it for such an interrupt chip, and a threaded handler needs no
 * first half. Requests and frees of one interrupt are serialised, as by
 * Linux's request_mutex: both sleep.
 *
 * An interrupt that comes while its handler is disabled is owed, and run
 * once it is enabled, as Linux replays a pending one; one that comes with no
 * handler requested is dropped, as Linux's shut-down interrupt drops it.
 * Which functions' interrupts the loop waits for when idle is worked out
 * just before it waits (lx_irq_listen, from lx_emul_next_event): those with
 * a handler enabled.
 */
#include <linux/interrupt.h>
#include <linux/irqreturn.h>
#include <linux/kthread.h>
#include <linux/pci.h>
#include <linux/msi.h>
#include <linux/sched.h>
#include <linux/wait.h>
#include <linux/mutex.h>
#include <linux/hardirq.h>

#include "slx.h"
#include "lx_emul.h"

/*
 * Interrupt numbers: LX_IRQ_BASE + function * LX_IRQ_VECTORS + vector,
 * above every value PCI's 8-bit Interrupt Line register can hold, so that no
 * function's legacy number -- its dev->irq while MSI is off -- is another's
 * MSI. LX_IRQ_VECTORS is the most MSI-X vectors the kernel gives a function
 * (msi::MSIX_MAX_VECTORS).
 */
#define LX_IRQ_BASE 1024
#define LX_IRQ_VECTORS 32
/*
 * A function's legacy line has a number of its own above every function's
 * vectors -- LX_INTX_BASE + function -- so that, as on Linux, its dev->irq
 * changes when its MSI is turned on and back when it is turned off, and a
 * number kept from one is never the other's.
 */
#define LX_INTX_BASE (LX_IRQ_BASE + LX_PCI_FUNCTIONS_MAX * LX_IRQ_VECTORS)

enum lx_irq_mode { LX_IRQ_NONE, LX_IRQ_MSI, LX_IRQ_MSIX, LX_IRQ_INTX };

struct lx_irq_function;

/* One vector of a function: its handler, and what runs it. */
struct lx_irq {
	struct lx_irq_function *fn;
	unsigned int index;
	/* The requested handler, NULL for none. */
	irq_handler_t handler;
	irq_handler_t thread_fn;
	unsigned long flags;
	const char *name;
	void *dev_id;
	/* disable_irq() nesting; run only at 0. */
	unsigned int depth;
	/* An interrupt that came while it was disabled, run once enabled. */
	bool owed;
	/* INTx: handled, its line to be re-armed once its oneshot thread has run. */
	bool rearm;
	/* Held across a request or a free, which both sleep. */
	struct mutex lock;
	/* The threaded half's thread; woken to run it, and running it; and
	 * who waits for it to have finished (synchronize_irq). */
	struct task_struct *thread;
	bool thread_due, thread_running;
	wait_queue_head_t idle;
	/* Handlers run, and interrupts dropped for want of one, for the tests. */
	unsigned long count, dropped;
};

/* A function given to the process, and its vectors. */
struct lx_irq_function {
	struct pci_dev *dev;
	long handle;
	/*
	 * The function's INTx: a number of the layer's own (LX_INTX_BASE +
	 * function), or 0 for a function with no interrupt pin -- dev->irq
	 * while its MSI is off.
	 */
	unsigned int intx;
	enum lx_irq_mode mode;
	/* Its vectors in the mode: 1 for MSI, as many as given for MSI-X. */
	unsigned int vectors;
	struct lx_irq vec[LX_IRQ_VECTORS];
	/*
	 * The legacy line's own record, as Linux's INTx descriptor is apart
	 * from the MSI ones: a handler on it stays its own while messages are
	 * on -- never run then -- and is run again once they are off.
	 */
	struct lx_irq line;
};

static struct lx_irq_function lx_irq_functions[LX_PCI_FUNCTIONS_MAX];

/* Set when an owed interrupt's handler is enabled: the next turn runs it. */
static bool lx_irq_replay;

static bool lx_irq_is_line(const struct lx_irq *d)
{
	return d == &d->fn->line;
}

static unsigned int lx_irq_number(const struct lx_irq *d)
{
	if (lx_irq_is_line(d))
		return d->fn->intx;
	return LX_IRQ_BASE + (d->fn - lx_irq_functions) * LX_IRQ_VECTORS + d->index;
}

/*
 * The record `irq` names: a function's legacy line, whenever it has one; a
 * message vector, while its function's MSI or MSI-X has it.
 */
static struct lx_irq *lx_irq_of(unsigned int irq)
{
	unsigned int at = irq - LX_IRQ_BASE;
	struct lx_irq_function *fn;

	if (irq >= LX_INTX_BASE && irq - LX_INTX_BASE < ARRAY_SIZE(lx_irq_functions)) {
		fn = &lx_irq_functions[irq - LX_INTX_BASE];
		return fn->dev && fn->intx ? &fn->line : NULL;
	}
	if (irq < LX_IRQ_BASE || at / LX_IRQ_VECTORS >= ARRAY_SIZE(lx_irq_functions))
		return NULL;
	fn = &lx_irq_functions[at / LX_IRQ_VECTORS];
	if (!fn->dev || (fn->mode != LX_IRQ_MSI && fn->mode != LX_IRQ_MSIX) ||
	    at % LX_IRQ_VECTORS >= fn->vectors)
		return NULL;
	return &fn->vec[at % LX_IRQ_VECTORS];
}

static void lx_irq_init(struct lx_irq *d, struct lx_irq_function *fn, unsigned int index)
{
	d->fn = fn;
	d->index = index;
	init_waitqueue_head(&d->idle);
	mutex_init(&d->lock);
}

/* The function's record, made when it is first asked after. */
static struct lx_irq_function *lx_irq_for(struct pci_dev *dev)
{
	int i = lx_pci_index(dev);
	struct lx_irq_function *fn;
	unsigned int v;

	if (i < 0 || i >= ARRAY_SIZE(lx_irq_functions))
		return NULL;
	fn = &lx_irq_functions[i];
	if (!fn->dev) {
		fn->dev = dev;
		fn->handle = lx_pci_handle(i);
		for (v = 0; v < LX_IRQ_VECTORS; v++)
			lx_irq_init(&fn->vec[v], fn, v);
		lx_irq_init(&fn->line, fn, 0);
		/*
		 * A line only for a pin the firmware routed: dev->irq is still the
		 * Interrupt Line register here, and 0 or 0xff routes nothing, as
		 * the kernel's set-up refuses it.
		 */
		fn->intx = dev->pin && dev->irq && dev->irq != 0xff ? LX_INTX_BASE + i : 0;
	}
	return fn;
}

/*
 * x86's hook into the PCI core before a driver probes, which routes the
 * function's legacy interrupt into dev->irq: here its INTx number, the
 * layer's own (lx_irq_for), so that a legacy driver's request_irq(pdev->irq)
 * reaches its function and no other -- two functions on one line keep
 * numbers of their own -- or 0, Linux's "none", for a function with no pin.
 * The line is set up when a handler is asked for on it.
 */
int pcibios_alloc_irq(struct pci_dev *dev)
{
	struct lx_irq_function *fn = lx_irq_for(dev);

	if (fn && fn->mode == LX_IRQ_NONE)
		dev->irq = fn->intx;
	return 0;
}

void pcibios_free_irq(struct pci_dev *dev)
{
}

/*
 * Whether the legacy line waits for its oneshot thread before it is
 * re-armed, as Linux keeps a oneshot interrupt masked until its thread has
 * run.
 */
static bool lx_intx_held(const struct lx_irq *d)
{
	return (d->flags & IRQF_ONESHOT) && (d->thread_due || d->thread_running);
}

/* Whether the vector's handler runs when its interrupts come: requested and enabled. */
static bool lx_irq_live(const struct lx_irq *d)
{
	return d->handler && !d->depth;
}

/* Whether any of the function's message vectors has a handler requested. */
static bool lx_irq_requested(const struct lx_irq_function *fn)
{
	unsigned int v;

	for (v = 0; (fn->mode == LX_IRQ_MSI || fn->mode == LX_IRQ_MSIX) && v < fn->vectors; v++)
		if (fn->vec[v].handler)
			return true;
	return false;
}

void lx_irq_listen(void)
{
	unsigned int i, v;

	for (i = 0; i < ARRAY_SIZE(lx_irq_functions); i++) {
		struct lx_irq_function *fn = &lx_irq_functions[i];
		bool live = false;

		if (!fn->dev)
			continue;
		for (v = 0; (fn->mode == LX_IRQ_MSI || fn->mode == LX_IRQ_MSIX) && v < fn->vectors; v++)
			live |= lx_irq_live(&fn->vec[v]);
		/*
		 * A legacy line whose re-arm waits -- for its thread, or for its
		 * handler to be enabled -- has an interrupt not yet taken, which
		 * would end every wait at once: what it waits for turns the loop.
		 */
		if (fn->mode == LX_IRQ_INTX)
			live = lx_irq_live(&fn->line) && !lx_intx_held(&fn->line) && !fn->line.rearm;
		slx_pci_irq_listen(fn->handle, live);
	}
}

/* Whether the next turn has an owed interrupt to run, and must come at once. */
bool lx_irq_due(void)
{
	return lx_irq_replay;
}

/* A handler is still requested on the function: a driver's bug, which Linux warns of. */
static void lx_irq_warn_requested(struct lx_irq_function *fn)
{
	WARN(lx_irq_requested(fn), "%s: its vectors turned off with a handler still requested\n",
	     pci_name(fn->dev));
}

/*
 * The function's vectors in a new mode: the kernel asked (slx_pci_irq_setup
 * with `ask`, 0 for MSI; slx_pci_intx_setup for INTx) and its answer, how
 * many it gave, recorded. The
 * kernel keeps a set-up for as long as nothing replaces it, so turning a
 * mode off here only stops its interrupts being taken or waited for.
 */
static int lx_irq_set_mode(struct lx_irq_function *fn, enum lx_irq_mode mode, unsigned int ask)
{
	long given = mode == LX_IRQ_INTX ? slx_pci_intx_setup(fn->handle) :
					   slx_pci_irq_setup(fn->handle, ask);
	unsigned int v;

	/*
	 * The kernel gives at most what was asked, and what is asked is at most
	 * LX_IRQ_VECTORS -- the records this side keeps: more would be vectors
	 * with nowhere to go.
	 */
	if (WARN_ON(given > (long)ask))
		given = -1;
	if (given < 0) {
		/* None to take: the kernel may have let the old set-up go. */
		fn->mode = LX_IRQ_NONE;
		return given;
	}
	fn->mode = mode;
	fn->vectors = mode == LX_IRQ_MSIX ? given : 1;
	for (v = 0; v < LX_IRQ_VECTORS; v++)
		fn->vec[v].owed = false;
	fn->line.owed = false;
	fn->line.rearm = false;
	return 0;
}

/*
 * MSI or MSI-X turned off: as Linux turns the legacy line back on, a
 * handler still requested on it is delivered again. (Turned on, the kernel
 * replaced the line's set-up with the messages'; the handler stayed the
 * line's, never run while they were on.)
 */
static void lx_intx_resume(struct lx_irq_function *fn)
{
	long err;

	if (!fn->line.handler || fn->mode != LX_IRQ_NONE)
		return;
	err = lx_irq_set_mode(fn, LX_IRQ_INTX, 0);
	if (err)
		pci_warn(fn->dev, "its legacy line could not be set up again: the kernel answered %ld\n",
			 err);
}

/* The function's MSI set up: its interrupt number then Linux's dev->irq. */
static int lx_msi_enable(struct pci_dev *dev)
{
	struct lx_irq_function *fn = lx_irq_for(dev);

	if (!fn)
		return -ENODEV;
	/* As Linux's: on already is a driver's bug; MSI-X on refuses MSI. */
	if (WARN_ON_ONCE(fn->mode == LX_IRQ_MSI) || fn->mode == LX_IRQ_MSIX)
		return -EINVAL;
	if (!dev->msi_cap || lx_irq_set_mode(fn, LX_IRQ_MSI, 0))
		return -ENOSPC;
	/* The kernel turned the legacy line off as it set the MSI up. */
	dev->msi_enabled = 1;
	dev->irq = lx_irq_number(&fn->vec[0]);
	return 0;
}

int pci_enable_msi(struct pci_dev *dev)
{
	return lx_msi_enable(dev);
}

/* MSI off, as Linux's code sees it: dev->irq is its legacy line again. */
void pci_disable_msi(struct pci_dev *dev)
{
	struct lx_irq_function *fn = lx_irq_for(dev);

	if (!fn || fn->mode != LX_IRQ_MSI)
		return;
	lx_irq_warn_requested(fn);
	fn->mode = LX_IRQ_NONE;
	dev->msi_enabled = 0;
	dev->irq = fn->intx;
	lx_intx_resume(fn);
}

/* How many entries the function's MSI-X table has, 0 for no MSI-X. */
static unsigned int lx_msix_table_size(struct pci_dev *dev)
{
	u16 control;

	if (!dev->msix_cap ||
	    pci_read_config_word(dev, dev->msix_cap + PCI_MSIX_FLAGS, &control))
		return 0;
	return (control & PCI_MSIX_FLAGS_QSIZE) + 1;
}

/*
 * MSI-X for between `minvec` and `maxvec` of the function's first table
 * entries, as many as the table holds and the kernel gives: how many, their
 * interrupt numbers then pci_irq_vector()'s. MSI or MSI-X on already is
 * -EINVAL, as Linux's; fewer than `minvec` is -ENOSPC, the function left
 * with no vectors.
 */
static int lx_msix_enable(struct pci_dev *dev, unsigned int minvec, unsigned int maxvec)
{
	struct lx_irq_function *fn = lx_irq_for(dev);
	unsigned int want = min3(maxvec, lx_msix_table_size(dev), (unsigned int)LX_IRQ_VECTORS);

	if (!fn)
		return -ENODEV;
	if (WARN_ON_ONCE(fn->mode == LX_IRQ_MSIX) || fn->mode == LX_IRQ_MSI)
		return -EINVAL;
	if (!minvec || want < minvec)
		return -ENOSPC;
	if (lx_irq_set_mode(fn, LX_IRQ_MSIX, want))
		return -ENOSPC;
	if (fn->vectors < minvec) {
		fn->mode = LX_IRQ_NONE;
		return -ENOSPC;
	}
	dev->msix_enabled = 1;
	return fn->vectors;
}

/*
 * The entries a driver names are its table's first, in order -- the kernel
 * programs the first ones; a driver naming others is refused out loud. With
 * no entries named (NULL, as Linux allows), they are the first, in order.
 */
int pci_enable_msix_range(struct pci_dev *dev, struct msix_entry *entries, int minvec, int maxvec)
{
	struct lx_irq_function *fn = lx_irq_for(dev);
	int i, given;

	for (i = 0; entries && i < maxvec; i++)
		if (entries[i].entry != i) {
			pci_err(dev, "MSI-X entry %u asked for at %d: only the first entries, in order, are given\n",
				entries[i].entry, i);
			return -EINVAL;
		}
	given = lx_msix_enable(dev, minvec, maxvec);
	if (given < 0)
		return given;
	for (i = 0; entries && i < given; i++)
		entries[i].vector = lx_irq_number(&fn->vec[i]);
	return given;
}

void pci_disable_msix(struct pci_dev *dev)
{
	struct lx_irq_function *fn = lx_irq_for(dev);

	if (!fn || fn->mode != LX_IRQ_MSIX)
		return;
	lx_irq_warn_requested(fn);
	fn->mode = LX_IRQ_NONE;
	dev->msix_enabled = 0;
	lx_intx_resume(fn);
}

/*
 * As Linux's: MSI-X first when asked for and the function has it, then MSI
 * -- one vector -- when asked for and enough, then the legacy line, when
 * asked for and the function has one: its number dev->irq, set up when a
 * handler is asked for on it.
 */
int pci_alloc_irq_vectors_affinity(struct pci_dev *dev, unsigned int min_vecs,
				   unsigned int max_vecs, unsigned int flags,
				   struct irq_affinity *affd)
{
	int ret = -ENOSPC;

	if (!min_vecs || max_vecs < min_vecs)
		return -EINVAL;
	if (flags & PCI_IRQ_MSIX) {
		ret = lx_msix_enable(dev, min_vecs, max_vecs);
		if (ret > 0)
			return ret;
	}
	if ((flags & PCI_IRQ_MSI) && min_vecs == 1) {
		ret = lx_msi_enable(dev);
		if (!ret)
			return 1;
	}
	if ((flags & PCI_IRQ_INTX) && min_vecs == 1 && dev->irq)
		return 1;
	/* As Linux's: the last attempt's error. */
	return ret;
}

int pci_alloc_irq_vectors(struct pci_dev *dev, unsigned int min_vecs, unsigned int max_vecs,
			  unsigned int flags)
{
	return pci_alloc_irq_vectors_affinity(dev, min_vecs, max_vecs, flags, NULL);
}

void pci_free_irq_vectors(struct pci_dev *dev)
{
	pci_disable_msix(dev);
	pci_disable_msi(dev);
}

/*
 * As Linux's: vector `nr`'s interrupt while MSI or MSI-X is on, the legacy
 * line's number (dev->irq) for vector 0 while neither is.
 */
int pci_irq_vector(struct pci_dev *dev, unsigned int nr)
{
	struct lx_irq_function *fn = lx_irq_for(dev);

	if (fn && fn->mode != LX_IRQ_NONE)
		return nr < fn->vectors ? lx_irq_number(&fn->vec[nr]) : -EINVAL;
	return nr ? -EINVAL : dev->irq;
}

/* One CPU: every vector's affinity is it, which Linux answers as none. */
const struct cpumask *pci_irq_get_affinity(struct pci_dev *dev, int vec)
{
	return NULL;
}

/* MSI is on for the whole process: the kernel offers it to every function. */
bool pci_msi_enabled(void)
{
	return true;
}

/* The kernel's programming of the MSI is never lost: nothing to restore. */
void pci_restore_msi_state(struct pci_dev *dev)
{
}

/* What a threaded interrupt with no first half has: wake the thread. */
static irqreturn_t lx_irq_wake_thread(int irq, void *dev_id)
{
	return IRQ_WAKE_THREAD;
}

/*
 * An interrupt's thread, as Linux's irq_thread(): its second half, each time
 * it is woken to -- a run asked for before a stop is still made, as Linux's
 * irq_wait_for_interrupt() makes it -- and who waits for it told after each.
 */
static int lx_irq_thread(void *data)
{
	struct lx_irq *d = data;

	for (;;) {
		set_current_state(TASK_INTERRUPTIBLE);
		if (d->thread_due) {
			__set_current_state(TASK_RUNNING);
			d->thread_due = false;
			d->thread_running = true;
			d->thread_fn(lx_irq_number(d), d->dev_id);
			d->thread_running = false;
			wake_up_all(&d->idle);
			continue;
		}
		if (kthread_should_stop())
			break;
		schedule();
	}
	__set_current_state(TASK_RUNNING);
	return 0;
}

int request_threaded_irq(unsigned int irq, irq_handler_t handler, irq_handler_t thread_fn,
			 unsigned long flags, const char *name, void *dev)
{
	struct lx_irq *d = lx_irq_of(irq);
	struct task_struct *t = NULL;
	bool line;
	int ret = 0;

	if (!d || (!handler && !thread_fn))
		return -EINVAL;
	line = lx_irq_is_line(d);
	/*
	 * As Linux's: a level line with no first half must stay masked until
	 * its thread has run -- without ONESHOT it would interrupt for ever.
	 */
	if (line && !handler && !(flags & IRQF_ONESHOT)) {
		pr_err("Threaded irq requested with handler=NULL and !ONESHOT for %s (irq %u)\n",
		       name, irq);
		return -EINVAL;
	}
	mutex_lock(&d->lock);
	if (d->handler) {
		ret = -EBUSY;
		goto out;
	}
	if (thread_fn) {
		t = kthread_create(lx_irq_thread, d, "irq/%u-%s", irq, name);
		if (IS_ERR(t)) {
			ret = PTR_ERR(t);
			goto out;
		}
	}
	/*
	 * The legacy line set up, as the kernel routes it, only once the
	 * request is sound -- so a refused one leaves the function as it was --
	 * and only while nothing else is: under MSI or MSI-X the handler is the
	 * line's, run once they are off (lx_intx_resume).
	 */
	if (line && d->fn->mode == LX_IRQ_NONE) {
		long err = lx_irq_set_mode(d->fn, LX_IRQ_INTX, 0);

		if (err) {
			pci_err(d->fn->dev, "its legacy interrupt could not be set up: the kernel answered %ld\n",
				err);
			if (t)
				kthread_stop(t);
			ret = -EINVAL;
			goto out;
		}
	}
	d->thread = t;
	d->thread_fn = thread_fn;
	/*
	 * A message is safe without ONESHOT: dropped, as Linux drops it for
	 * such a chip. A level line keeps it.
	 */
	d->flags = line ? flags : flags & ~IRQF_ONESHOT;
	d->name = name;
	d->dev_id = dev;
	d->depth = flags & IRQF_NO_AUTOEN ? 1 : 0;
	d->owed = false;
	d->rearm = false;
	d->thread_due = d->thread_running = false;
	/* Last: lx_irq_poll() may run it from the next turn on. */
	d->handler = handler ?: lx_irq_wake_thread;
	if (t)
		wake_up_process(t);
out:
	mutex_unlock(&d->lock);
	return ret;
}

/*
 * The handler gone, as Linux's __free_irq(): not run again from now, its
 * threaded half's last asked-for run waited for, then its thread stopped.
 */
const void *free_irq(unsigned int irq, void *dev_id)
{
	struct lx_irq *d = lx_irq_of(irq);
	const char *name = NULL;

	if (!d)
		goto none;
	mutex_lock(&d->lock);
	if (!d->handler || d->dev_id != dev_id) {
		mutex_unlock(&d->lock);
		goto none;
	}
	d->handler = NULL;
	d->owed = false;
	if (d->thread) {
		wait_event(d->idle, !d->thread_due && !d->thread_running);
		kthread_stop(d->thread);
		d->thread = NULL;
	}
	name = d->name;
	d->thread_fn = NULL;
	d->name = NULL;
	d->dev_id = NULL;
	mutex_unlock(&d->lock);
	return name;
none:
	WARN(1, "Trying to free already-free IRQ %u\n", irq);
	return NULL;
}

void disable_irq_nosync(unsigned int irq)
{
	struct lx_irq *d = lx_irq_of(irq);

	if (!d)
		return;
	d->depth++;
}

/*
 * A hard interrupt's handler runs only inside a turn of the loop, between
 * tasks, so none is ever running while a task asks: what remains is the
 * threaded half, which is waited for.
 */
void synchronize_irq(unsigned int irq)
{
	struct lx_irq *d = lx_irq_of(irq);

	if (d && d->thread)
		wait_event(d->idle, !d->thread_due && !d->thread_running);
}

void disable_irq(unsigned int irq)
{
	disable_irq_nosync(irq);
	synchronize_irq(irq);
}

void enable_irq(unsigned int irq)
{
	struct lx_irq *d = lx_irq_of(irq);

	if (!d || WARN(!d->depth, "Unbalanced enable for IRQ %u\n", irq))
		return;
	if (!--d->depth && (d->owed || d->rearm))
		lx_irq_replay = true;
}

/* The vector's handler run once, in hardirq context; its thread woken when it asks. */
static void lx_irq_run(struct lx_irq *d)
{
	unsigned long flags;
	irqreturn_t ret;

	d->owed = false;
	lx_hardirq_enter(flags);
	ret = d->handler(lx_irq_number(d), d->dev_id);
	lx_hardirq_exit(flags);
	d->count++;
	if (!(ret & IRQ_WAKE_THREAD))
		return;
	if (!d->thread) {
		/* As Linux's warn_no_thread(): a driver's bug, said once. */
		pr_warn_once("IRQ %u device %s returned IRQ_WAKE_THREAD but no thread function available.\n",
			     lx_irq_number(d), d->name);
		return;
	}
	d->thread_due = true;
	wake_up_process(d->thread);
}

/*
 * A turn for a function on its legacy line, which the kernel quenches at
 * each interrupt until its interrupts are taken -- taking re-arms it
 * (design-decisions 1579). It is taken only once its handler has run, as
 * Linux unmasks a level interrupt only after handling it: taken first, the
 * line would be re-armed with the device still asserting, and interrupt
 * again for nothing. So it is looked at without taking (slx_pci_irq_ready,
 * the kernel's look), the handler run, and then taken. While there is no
 * handler, or it is disabled, or a oneshot thread has yet to run, the line
 * stays quenched, as Linux keeps a level line masked: owed, or re-armed once
 * the thread is done and the handler enabled.
 */
static void lx_intx_poll(struct lx_irq_function *fn)
{
	struct lx_irq *d = &fn->line;

	if (lx_intx_held(d))
		return;
	if (d->rearm) {
		/* Disabled since: masked until enabled, as Linux finalises a oneshot. */
		if (d->depth)
			return;
		d->rearm = false;
		slx_pci_irq_pending(fn->handle);
		return;
	}
	if (!d->owed && !slx_pci_irq_ready(fn->handle))
		return;
	/*
	 * No handler, or a disabled one: the line left quenched, as Linux keeps
	 * a level line with no action, or a disabled one, masked -- owed, run
	 * once a handler is requested or enabled.
	 */
	if (!d->handler || d->depth) {
		d->owed = true;
		return;
	}
	lx_irq_run(d);
	if (lx_intx_held(d))
		d->rearm = true;
	else
		slx_pci_irq_pending(fn->handle);
}

/*
 * A turn of the loop: each function with its vectors set up asked which of
 * them have had interrupts, and each of those run -- or owed, while
 * disabled -- together with the owed ones since enabled; those that came
 * with no handler requested are dropped, as Linux's shut-down interrupt
 * drops them. A function none of whose vectors has a handler is asked too,
 * so that what came before a handler was requested is never run by it. A
 * function on its legacy line is lx_intx_poll()'s.
 */
void lx_irq_poll(void)
{
	unsigned int i, v;

	lx_irq_replay = false;
	for (i = 0; i < ARRAY_SIZE(lx_irq_functions); i++) {
		struct lx_irq_function *fn = &lx_irq_functions[i];
		long came;

		if (!fn->dev || fn->mode == LX_IRQ_NONE)
			continue;
		if (fn->mode == LX_IRQ_INTX) {
			lx_intx_poll(fn);
			continue;
		}
		came = slx_pci_irq_pending(fn->handle);
		if (came < 0)
			came = 0;
		for (v = 0; v < fn->vectors; v++) {
			struct lx_irq *d = &fn->vec[v];

			if (!d->handler) {
				d->dropped += (came >> v) & 1;
				continue;
			}
			if (!((came >> v) & 1 || d->owed))
				continue;
			if (d->depth)
				d->owed = true;
			else
				lx_irq_run(d);
		}
	}
}

unsigned long lx_irq_count(unsigned int irq)
{
	struct lx_irq *d = lx_irq_of(irq);

	return d ? d->count : 0;
}

unsigned long lx_irq_dropped(unsigned int irq)
{
	struct lx_irq *d = lx_irq_of(irq);

	return d ? d->dropped : 0;
}

bool lx_irq_ready(struct pci_dev *dev)
{
	struct lx_irq_function *fn = lx_irq_for(dev);

	return fn && slx_pci_irq_ready(fn->handle);
}
