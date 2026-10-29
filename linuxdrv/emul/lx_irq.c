// SPDX-License-Identifier: MIT
/*
 * Interrupts in a driver process (design-decisions 1585): a PCI function's
 * MSI, which the kernel programs through its device handle, delivered to the
 * handler a driver requests as Linux's interrupt core would deliver it.
 *
 * Linux's own MSI and interrupt cores are not compiled: they program the
 * device and the CPU's interrupt controller, which the kernel does here, and
 * hang every interrupt on irq domains a driver process has no use for. So
 * this file is the bottom of both, for what a driver calls: Linux's MSI API
 * (pci_alloc_irq_vectors(), pci_enable_msi(), pci_irq_vector()), one vector
 * per function -- the handle offers one -- and MSI-X refused, which drivers
 * take as the cue to fall back to MSI; and request_threaded_irq(),
 * free_irq(), disable_irq() and enable_irq(), synchronize_irq(). A
 * function's legacy line is not routed: its interrupt number is no
 * interrupt here.
 *
 * Each turn of the loop takes the interrupts each function with a handler
 * counted since the last (lx_irq_poll), and runs the handler as Linux runs
 * a hard interrupt's: in hardirq context, interrupts off, once however many
 * came -- an MSI is an edge, which Linux's own handlers read their device's
 * cause register for. The turn's softirqs run after it, as on Linux's
 * irq_exit(). A threaded handler's second half runs in a kthread of its
 * own, as Linux's "irq/N-name" thread runs it, woken when the first half
 * answers IRQ_WAKE_THREAD, and run once for however many asked. MSI is
 * safe without ONESHOT -- an edge cannot storm while the thread runs -- so
 * IRQF_ONESHOT is dropped, as Linux drops it for such an interrupt chip,
 * and a threaded handler needs no first half. Requests and frees of one
 * interrupt are serialised, as by Linux's request_mutex: both sleep.
 *
 * While no handler is requested, or the interrupt is disabled, its
 * interrupts stay counted in the kernel, not taken -- delivered once it is
 * enabled, as Linux replays a pending one -- and the loop does not wait for
 * them: which interrupts it waits for is worked out just before it waits
 * (lx_irq_listen, from lx_emul_next_event).
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
 * A function's interrupt number: above every value PCI's 8-bit Interrupt
 * Line register can hold, so that no function's legacy number -- its
 * dev->irq while its MSI is off -- is another function's MSI.
 */
#define LX_IRQ_BASE 1024

struct lx_irq {
	struct pci_dev *dev;
	long handle;
	/* The function's legacy line, restored when its MSI is turned off. */
	unsigned int intx;
	bool msi;
	/* The requested handler, NULL for none. */
	irq_handler_t handler;
	irq_handler_t thread_fn;
	unsigned long flags;
	const char *name;
	void *dev_id;
	/* disable_irq() nesting; taken only at 0. */
	unsigned int depth;
	/* Held across a request or a free, which both sleep. */
	struct mutex lock;
	/* The threaded half's thread; woken to run it, and running it; and
	 * who waits for it to have finished (synchronize_irq). */
	struct task_struct *thread;
	bool thread_due, thread_running;
	wait_queue_head_t idle;
	/* Handlers run, for the tests. */
	unsigned long count;
};

static struct lx_irq lx_irqs[LX_PCI_FUNCTIONS_MAX];

static struct lx_irq *lx_irq_of(unsigned int irq)
{
	unsigned int i = irq - LX_IRQ_BASE;

	if (irq < LX_IRQ_BASE || i >= ARRAY_SIZE(lx_irqs) || !lx_irqs[i].dev)
		return NULL;
	return &lx_irqs[i];
}

/* The function's record, made when it is first asked after. */
static struct lx_irq *lx_irq_for(struct pci_dev *dev)
{
	int i = lx_pci_index(dev);

	if (i < 0 || i >= ARRAY_SIZE(lx_irqs))
		return NULL;
	if (!lx_irqs[i].dev) {
		lx_irqs[i].dev = dev;
		lx_irqs[i].handle = lx_pci_handle(i);
		lx_irqs[i].intx = dev->irq;
		init_waitqueue_head(&lx_irqs[i].idle);
		mutex_init(&lx_irqs[i].lock);
	}
	return &lx_irqs[i];
}

static unsigned int lx_irq_number(const struct lx_irq *d)
{
	return LX_IRQ_BASE + (d - lx_irqs);
}

/*
 * Whether the loop waits for the interrupt, and lx_irq_poll() takes it: its
 * MSI on, a handler requested, the interrupt enabled.
 */
static bool lx_irq_live(const struct lx_irq *d)
{
	return d->msi && d->handler && !d->depth;
}

void lx_irq_listen(void)
{
	unsigned int i;

	for (i = 0; i < ARRAY_SIZE(lx_irqs); i++)
		if (lx_irqs[i].dev)
			slx_pci_irq_listen(lx_irqs[i].handle, lx_irq_live(&lx_irqs[i]));
}

/* The function's MSI set up: its interrupt number then Linux's dev->irq. */
static int lx_msi_enable(struct pci_dev *dev)
{
	struct lx_irq *d = lx_irq_for(dev);

	if (!d)
		return -ENODEV;
	if (d->msi)
		return 0;
	if (!dev->msi_cap || slx_pci_irq_setup(d->handle) < 0)
		return -ENOSPC;
	/* The kernel turned the legacy line off as it set the MSI up. */
	d->msi = true;
	dev->msi_enabled = 1;
	dev->irq = lx_irq_number(d);
	return 0;
}

int pci_enable_msi(struct pci_dev *dev)
{
	return lx_msi_enable(dev);
}

/*
 * MSI off, as Linux's code sees it: the kernel keeps the device's MSI set
 * up for the handle's life, but none is taken or waited for, and dev->irq
 * is its legacy line again. A handler still requested is a driver's bug,
 * which Linux warns of.
 */
void pci_disable_msi(struct pci_dev *dev)
{
	struct lx_irq *d = lx_irq_for(dev);

	if (!d || !d->msi)
		return;
	WARN_ON(d->handler);
	d->msi = false;
	dev->msi_enabled = 0;
	dev->irq = d->intx;
}

/* MSI-X is not offered: drivers fall back to MSI. */
int pci_enable_msix_range(struct pci_dev *dev, struct msix_entry *entries, int minvec, int maxvec)
{
	return -ENOSPC;
}

void pci_disable_msix(struct pci_dev *dev)
{
}

/*
 * One vector, MSI, when asked for MSI: the handle offers no more. Neither
 * MSI-X nor the legacy line is offered.
 */
int pci_alloc_irq_vectors_affinity(struct pci_dev *dev, unsigned int min_vecs,
				   unsigned int max_vecs, unsigned int flags,
				   struct irq_affinity *affd)
{
	if (!(flags & PCI_IRQ_MSI) || min_vecs > 1 || !max_vecs)
		return -ENOSPC;
	return lx_msi_enable(dev) ?: 1;
}

int pci_alloc_irq_vectors(struct pci_dev *dev, unsigned int min_vecs, unsigned int max_vecs,
			  unsigned int flags)
{
	return pci_alloc_irq_vectors_affinity(dev, min_vecs, max_vecs, flags, NULL);
}

void pci_free_irq_vectors(struct pci_dev *dev)
{
	pci_disable_msi(dev);
}

/*
 * As Linux's: vector 0 is the MSI's interrupt while MSI is on, the legacy
 * line's number (dev->irq) while it is off -- a number requesting which
 * fails here, the line not being routed.
 */
int pci_irq_vector(struct pci_dev *dev, unsigned int nr)
{
	struct lx_irq *d = lx_irq_for(dev);

	if (nr)
		return -EINVAL;
	return d && d->msi ? lx_irq_number(d) : dev->irq;
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
	int ret = 0;

	if (!d || !d->msi || (!handler && !thread_fn))
		return -EINVAL;
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
	d->thread = t;
	d->thread_fn = thread_fn;
	/* MSI is safe without ONESHOT: dropped, as Linux drops it for such a chip. */
	d->flags = flags & ~IRQF_ONESHOT;
	d->name = name;
	d->dev_id = dev;
	d->depth = flags & IRQF_NO_AUTOEN ? 1 : 0;
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
	d->depth--;
}

/*
 * A turn of the loop: each live interrupt's counted since the last turn
 * taken, and its handler run once for them, in hardirq context with
 * interrupts off; a threaded one's thread woken when it asks.
 */
void lx_irq_poll(void)
{
	unsigned int i;

	for (i = 0; i < ARRAY_SIZE(lx_irqs); i++) {
		struct lx_irq *d = &lx_irqs[i];
		unsigned long flags;
		irqreturn_t ret;

		if (!d->dev || !lx_irq_live(d) || slx_pci_irq_take(d->handle) <= 0)
			continue;
		lx_hardirq_enter(flags);
		ret = d->handler(lx_irq_number(d), d->dev_id);
		lx_hardirq_exit(flags);
		d->count++;
		if (!(ret & IRQ_WAKE_THREAD))
			continue;
		if (!d->thread) {
			/* As Linux's warn_no_thread(): a driver's bug, said once. */
			pr_warn_once("IRQ %u device %s returned IRQ_WAKE_THREAD but no thread function available.\n",
				     lx_irq_number(d), d->name);
			continue;
		}
		d->thread_due = true;
		wake_up_process(d->thread);
	}
}

unsigned long lx_irq_count(unsigned int irq)
{
	struct lx_irq *d = lx_irq_of(irq);

	return d ? d->count : 0;
}
