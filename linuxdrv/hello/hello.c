// SPDX-License-Identifier: MIT
/*
 * hello -- the driver layer's first driver process, and no driver: a Linux
 * initcall that allocates, takes a spinlock with interrupts "off", formats a
 * line through Linux's own vsprintf and frees, then others that run Linux's
 * tasks over the layer's scheduler, in a process of its own as any Linux
 * driver will be (roadmap.md 2.1, design-decisions 1580).
 *
 * The kernel's self-test (`linuxdrv::self_test`) runs it plainly, when it
 * must exit 0 having printed its lines, and with one argument, which makes
 * the first initcall end the process another way:
 *   "stop"      calls a function nothing defines -- which the build turns
 *               into a generated stop -- so must exit SLX_EXIT_STOP;
 *   "deadlock"  waits for what never comes, with no other task to run, so
 *               must exit SLX_EXIT_DEADLOCK;
 *   "overflow"  runs past the end of its task's stack, so must be killed
 *               there by the guard page below it;
 *   "fail"      starts a kthread that never ends and fails, so must exit
 *               SLX_EXIT_INIT_FAILED at once;
 *   "earlyfail" fails a core initcall of the driver's own -- at a level
 *               Linux's subsystems use too, whose failures the process
 *               carries past -- so must exit SLX_EXIT_INIT_FAILED as well;
 *   "stranded"  succeeds, leaving a kthread waiting for what never comes,
 *               so must exit SLX_EXIT_DEADLOCK, not 0;
 *   "atomic"    sleeps with bottom halves off, so must exit SLX_EXIT_BUG;
 *   "pci"       with `pci=BB:DD.F` naming the boot test's spare 82574,
 *               registers hello's pci_driver for it, which must be probed
 *               through Linux's PCI core, read the card's MAC through BAR0
 *               and handle its interrupts, so must exit 0;
 *   "pcifail"   the same, its probe failing, so that the function is left
 *               without a driver: must exit SLX_EXIT_INIT_FAILED;
 *   "firmware"  with `firmware_class.path=<dir>`, the test files in it, and
 *               hello's own parameters, reads files through the layer and
 *               Linux's own firmware loader (hello_firmware_init), so must
 *               exit 0;
 *   "firmware-denied"  the same without a capability to read files, which
 *               must read none: must exit 0;
 *   "bootfw"    with no firmware_class.path, loads the boot medium's
 *               firmware from /lib/firmware, where Linux's loader looks by
 *               default and the kernel put it: must exit 0.
 * With an argument, the later initcalls do nothing: the first's ending is
 * the test ("earlyfail"'s comes before the first) -- but the firmware runs',
 * a late one's. An argument naming a parameter hello does not have, or
 * holding both a space and a quote, ends the process before any initcall;
 * one giving a parameter a value its kind refuses, at the parameter's own
 * level -- before any initcall for a module's own: SLX_EXIT_ARGUMENT.
 */
#include <linux/module.h>
#include <linux/slab.h>
#include <linux/printk.h>
#include <linux/spinlock.h>
#include <linux/irqflags.h>
#include <linux/string.h>
#include <linux/kthread.h>
#include <linux/mutex.h>
#include <linux/completion.h>
#include <linux/delay.h>
#include <linux/jiffies.h>
#include <linux/sched.h>
#include <linux/sched/task.h>
#include <linux/sched/wake_q.h>
#include <linux/rcupdate.h>
#include <linux/rcuwait.h>
#include <linux/err.h>
#include <linux/timer.h>
#include <linux/interrupt.h>
#include <linux/bottom_half.h>
#include <linux/preempt.h>
#include <linux/cpuhotplug.h>
#include <linux/workqueue.h>
#include <linux/irq_work.h>
#include <linux/device.h>
#include <linux/kobject_ns.h>
#include <linux/srcu.h>
#include <linux/mm.h>
#include <linux/pci.h>
#include <linux/dma-mapping.h>
#include <linux/io.h>
#include <linux/etherdevice.h>
#include <linux/unaligned.h>
#include <linux/sizes.h>
#include <linux/firmware.h>
#include <linux/kernel_read_file.h>
#include <linux/vmalloc.h>
#include <asm/processor.h>

#include "../emul/slx.h"
#include "../emul/lx_emul.h"

/* Defined nowhere: the build makes it a generated stop. */
void lx_hello_never_defined(void);

/* Never completed. */
static DECLARE_COMPLETION(hello_forever);

/* Whether the process's argument is `mode`. */
static bool hello_mode(const char *mode)
{
	return slx_argc() > 1 && strcmp(slx_argv(1), mode) == 0;
}

/* A warning from one place, given twice: said once, and the driver goes on. */
static void hello_warn_once(void)
{
	WARN_ON_ONCE(1);
}

/*
 * About `depth` KiB of stack, used and given back: a frame of a KiB each.
 * hello_overflow_kib() of it runs past a task's stack but not past the
 * guard below it (slx_stack_bytes, slx_guard_bytes), so that it returns,
 * having written over memory, only where the guard is missing.
 */
static int hello_overflow_kib(void)
{
	return (slx_stack_bytes + slx_guard_bytes / 4) / 1024;
}

static noinline int hello_deep(int depth)
{
	volatile char frame[1024];

	frame[0] = depth;
	frame[sizeof(frame) - 1] = depth;
	if (depth > 0)
		frame[0] += hello_deep(depth - 1);
	return frame[0] + frame[sizeof(frame) - 1];
}

/* A kthread that polls until it is stopped, which nothing does. */
static int hello_polls(void *unused)
{
	while (!kthread_should_stop())
		msleep(100);
	return 0;
}

/* A kthread that waits for what never comes. */
static int hello_waits_forever(void *unused)
{
	wait_for_completion(&hello_forever);
	return 0;
}

/*
 * The boot test's spare 82574 (kernel/src/devhandle.rs's probe card), as
 * hello's own pci_driver drives it: enabled, its memory BARs claimed, bus
 * mastering on, BAR0 mapped as e1000e maps it -- ioremap() of the resource
 * Linux's own probe sized -- and the MAC address the boot test gave it read
 * from receive address 0. On the way, the BARs as the layer shadows them:
 * BAR0 sized by writing all ones and reading back, without the device
 * moving; the I/O BAR absent, the handle offering none.
 */
#define HELLO_82574 0x10d3
/* The ID table's mark for QEMU's edu (1234:11e8), the second function. */
#define HELLO_EDU 1
#define HELLO_RAL0 0x5400
#define HELLO_RAH0 0x5404
#define HELLO_RAH_VALID BIT(31)
static const u8 hello_card_mac[ETH_ALEN] = { 0x52, 0x54, 0x00, 0x12, 0x34, 0x59 };
static bool hello_pci_probed;

/* BAR0 written all ones reads its size back, and written back reads as before. */
static bool hello_bar0_sizes(struct pci_dev *pdev)
{
	u32 bar, sized, again, len = pci_resource_len(pdev, 0);

	pci_read_config_dword(pdev, PCI_BASE_ADDRESS_0, &bar);
	pci_write_config_dword(pdev, PCI_BASE_ADDRESS_0, ~0U);
	pci_read_config_dword(pdev, PCI_BASE_ADDRESS_0, &sized);
	pci_write_config_dword(pdev, PCI_BASE_ADDRESS_0, bar);
	pci_read_config_dword(pdev, PCI_BASE_ADDRESS_0, &again);
	return len && (sized & PCI_BASE_ADDRESS_MEM_MASK) == (~(len - 1) & PCI_BASE_ADDRESS_MEM_MASK) &&
	       (sized & ~PCI_BASE_ADDRESS_MEM_MASK) == (bar & ~PCI_BASE_ADDRESS_MEM_MASK) &&
	       again == bar && (bar & PCI_BASE_ADDRESS_MEM_MASK) == (u32)pci_resource_start(pdev, 0);
}

/*
 * The card's interrupts, through Linux's own API (lx_irq.c): its MSI
 * allocated, a handler requested, and the card made to raise one -- its
 * interrupt cause set (ICS) with the cause unmasked (IMS), as the boot
 * test's probe driver raises it. The handler runs in hardirq context; a
 * disabled interrupt is held, not lost, and delivered once enabled; a
 * throttled one (the card's interrupt throttle, ITR, holding it back after
 * the last) comes when the process has gone idle with nothing else due, so
 * that only the loop's wait on the device can wake it; a threaded
 * handler's second half runs in a task, which may sleep. The waits have no
 * timeout: an interrupt lost leaves the process waiting on its device for
 * ever -- a driver with a handler requested is never done by itself -- and
 * the kernel's deadline for the run ends it. The handlers read the cause
 * register, which clears it, as e1000e's do.
 */
#define HELLO_ICR 0x00c0
#define HELLO_ICS 0x00c8
#define HELLO_IMS 0x00d0
#define HELLO_IMC 0x00d8
#define HELLO_ITR 0x00c4
#define HELLO_CAUSE 0x1
static void __iomem *hello_regs;
static DECLARE_COMPLETION(hello_irq_done);
static bool hello_irq_in_hardirq, hello_thread_in_task;

static void hello_raise(void)
{
	reinit_completion(&hello_irq_done);
	writel(HELLO_CAUSE, hello_regs + HELLO_IMS);
	writel(HELLO_CAUSE, hello_regs + HELLO_ICS);
}

static irqreturn_t hello_isr(int irq, void *data)
{
	if (!(readl(hello_regs + HELLO_ICR) & HELLO_CAUSE))
		return IRQ_NONE;
	hello_irq_in_hardirq = in_hardirq() && irqs_disabled();
	complete(&hello_irq_done);
	return IRQ_HANDLED;
}

static irqreturn_t hello_isr_first_half(int irq, void *data)
{
	return readl(hello_regs + HELLO_ICR) & HELLO_CAUSE ? IRQ_WAKE_THREAD : IRQ_NONE;
}

static irqreturn_t hello_isr_thread(int irq, void *data)
{
	hello_thread_in_task = !in_interrupt() && !irqs_disabled() && (current->flags & PF_KTHREAD);
	msleep(1);
	complete(&hello_irq_done);
	return IRQ_HANDLED;
}

/*
 * DMA (lx_dma.c): a 64-bit mask; memory outside the DMA window refused;
 * coherent memory within the coherent mask, zeroed whole -- the block, not
 * just the bytes asked for -- and aligned; a mask the memory already given
 * lies beyond refused; a kmalloc'd frame given a DMA address -- and the
 * card itself, given a descriptor ring in the coherent memory pointing at
 * the frame, reading both from the window through its IOMMU domain and
 * writing the descriptor's status back into it. And a second function
 * (QEMU's edu, when given) refused all DMA: the window is the card's.
 */
#define HELLO_TCTL 0x0400
#define HELLO_TCTL_EN BIT(1)
#define HELLO_TCTL_PSP BIT(3)
#define HELLO_TDBAL 0x3800
#define HELLO_TDBAH 0x3804
#define HELLO_TDLEN 0x3808
#define HELLO_TDH 0x3810
#define HELLO_TDT 0x3818
#define HELLO_TXD_EOP 0x01
#define HELLO_TXD_IFCS 0x02
#define HELLO_TXD_RS 0x08
#define HELLO_TXD_DD 0x01
#define HELLO_TX_DESCS 8

/* The 82574's legacy transmit descriptor. */
struct hello_tx_desc {
	__le64 addr;
	__le16 length;
	u8 cso;
	u8 cmd;
	u8 status;
	u8 css;
	__le16 special;
};

static char hello_static_frame[ETH_ZLEN];

#define HELLO_PCI_CHECK(cond, what)						\
	do {									\
		if (!(cond)) {							\
			dev_err(&pdev->dev, "check failed: %s\n", what);	\
			return -EPROTO;						\
		}								\
	} while (0)

/*
 * MSI-X (lx_irq.c): three vectors, the card's receive, transmit and other
 * causes routed one to each (its IVAR register: each field's vector, and bit
 * 3 for valid); an interrupt before any handler is requested dropped; each
 * cause raised in turn running its own vector's handler alone -- QEMU's
 * second sending of each taken in its own turn, the throttle interval (EITR)
 * at its most meanwhile -- and nothing running in the 200 ms after the
 * last; and the vectors gone once freed.
 */
#define HELLO_IVAR 0x00e4
#define HELLO_ICR_LSC BIT(2)
#define HELLO_ICR_RXQ0 BIT(20)
#define HELLO_ICR_TXQ0 BIT(22)
#define HELLO_ICR_OTHER BIT(24)
#define HELLO_IVAR_ROUTES (0x8 | (0x9 << 8) | (0xa << 16))
/*
 * Each vector's throttle interval, EITR(n), in 256 ns units, while the test
 * raises its causes: its most, 16.8 ms. QEMU's model sends each vector a
 * second time when the interval ends, and this puts that second past any
 * short wait, and never in the same turn of the loop as the first. 0, the
 * reset value, after.
 */
#define HELLO_EITR(n) (0x00e8 + 4 * (n))
#define HELLO_EITR_LATE 0xffff

static irqreturn_t hello_isr_msix(int irq, void *data)
{
	return IRQ_HANDLED;
}

static void hello_set_eitr(u32 interval)
{
	unsigned int v;

	for (v = 0; v < 3; v++)
		writel(interval, hello_regs + HELLO_EITR(v));
}

/*
 * Yield -- each yield a turn of the loop, which takes the devices'
 * interrupts -- until `count(irq)` reaches `want`, `ns` at most: whether it
 * did.
 */
static bool hello_wait(unsigned long (*count)(unsigned int), unsigned int irq,
		       unsigned long want, u64 ns)
{
	u64 give_up = slx_now_ns() + ns;

	while (count(irq) < want && slx_now_ns() < give_up)
		yield();
	return count(irq) >= want;
}

static int hello_pci_msix(struct pci_dev *pdev)
{
	static const struct { u32 cause, clear; unsigned int vector; } causes[] = {
		{ HELLO_ICR_RXQ0, HELLO_ICR_RXQ0, 0 },
		{ HELLO_ICR_TXQ0, HELLO_ICR_TXQ0, 1 },
		{ HELLO_ICR_LSC, HELLO_ICR_LSC | HELLO_ICR_OTHER, 2 },
	};
	unsigned long before[3], dropped;
	unsigned int i, v;
	int irqs[3];

	HELLO_PCI_CHECK(pci_alloc_irq_vectors(pdev, 3, 3, PCI_IRQ_MSIX) == 3 && pdev->msix_enabled,
			"no three MSI-X vectors");
	/* As Linux's: enabled twice is a driver's bug, refused (and warned of once). */
	HELLO_PCI_CHECK(pci_enable_msix_range(pdev, NULL, 3, 3) == -EINVAL,
			"MSI-X enabled while it was on already");
	writel(~0U, hello_regs + HELLO_IMC);
	writel(~0U, hello_regs + HELLO_ICR);
	writel(HELLO_IVAR_ROUTES, hello_regs + HELLO_IVAR);
	hello_set_eitr(HELLO_EITR_LATE);
	writel(HELLO_ICR_RXQ0 | HELLO_ICR_TXQ0 | HELLO_ICR_OTHER, hello_regs + HELLO_IMS);
	for (v = 0; v < 3; v++) {
		irqs[v] = pci_irq_vector(pdev, v);
		HELLO_PCI_CHECK(irqs[v] > 0 && (!v || irqs[v] != irqs[v - 1]),
				"MSI-X vectors without interrupts of their own");
		before[v] = lx_irq_count(irqs[v]);
	}
	/*
	 * An interrupt that comes before any handler is requested is dropped,
	 * not the next handler's -- and so is QEMU's second sending of it, which
	 * is waited for: both dropped, a second at most, before the handlers
	 * are requested.
	 */
	dropped = lx_irq_dropped(irqs[0]);
	writel(HELLO_ICR_RXQ0, hello_regs + HELLO_ICS);
	hello_wait(lx_irq_dropped, irqs[0], dropped + 2, NSEC_PER_SEC);
	writel(HELLO_ICR_RXQ0, hello_regs + HELLO_ICR);
	HELLO_PCI_CHECK(lx_irq_dropped(irqs[0]) > dropped,
			"an interrupt before any handler was requested was not dropped");
	for (v = 0; v < 3; v++)
		HELLO_PCI_CHECK(!request_irq(irqs[v], hello_isr_msix, 0, "hello-msix", pdev),
				"no MSI-X handler requested");
	msleep(5);
	for (v = 0; v < 3; v++)
		HELLO_PCI_CHECK(lx_irq_count(irqs[v]) == before[v],
				"an interrupt from before its handler was requested reached it");
	for (i = 0; i < ARRAY_SIZE(causes); i++) {
		unsigned int own = causes[i].vector;

		for (v = 0; v < 3; v++)
			before[v] = lx_irq_count(irqs[v]);
		writel(causes[i].cause, hello_regs + HELLO_ICS);
		/* Its first run, seen as it comes: a yield is a turn of the loop. */
		hello_wait(lx_irq_count, irqs[own], before[own] + 1, 100 * NSEC_PER_MSEC);
		writel(causes[i].clear, hello_regs + HELLO_ICR);
		/*
		 * Then QEMU's second sending, which comes when the vector's
		 * throttle interval ends -- HELLO_EITR_LATE -- and late by an
		 * amount no fixed wait bounds, under emulation: waited for, a
		 * second at most (none comes from hardware), before the next
		 * cause. Whatever else ran meanwhile counts against this cause.
		 */
		if (lx_irq_count(irqs[own]) > before[own])
			hello_wait(lx_irq_count, irqs[own], before[own] + 2, NSEC_PER_SEC);
		for (v = 0; v < 3; v++)
			if ((lx_irq_count(irqs[v]) != before[v]) != (v == causes[i].vector)) {
				dev_err(&pdev->dev, "cause %#x: vectors ran %lu, %lu, %lu times, not vector %u alone\n",
					causes[i].cause, lx_irq_count(irqs[0]) - before[0],
					lx_irq_count(irqs[1]) - before[1], lx_irq_count(irqs[2]) - before[2],
					causes[i].vector);
				HELLO_PCI_CHECK(false, "a cause did not run its own MSI-X vector's handler alone");
			}
	}
	/*
	 * Then, every cause masked, nothing for 200 ms: each vector's second
	 * sending was taken in its own cause's turn, and none of the card's
	 * causes can raise another. One that runs now is a second a wait left
	 * for later -- which a later cause's turn would have taken for its own.
	 */
	writel(~0U, hello_regs + HELLO_IMC);
	for (v = 0; v < 3; v++)
		before[v] = lx_irq_count(irqs[v]);
	msleep(200);
	for (v = 0; v < 3; v++)
		HELLO_PCI_CHECK(lx_irq_count(irqs[v]) == before[v],
				"an MSI-X vector ran after its cause's turn had ended");
	writel(0, hello_regs + HELLO_IVAR);
	hello_set_eitr(0);
	for (v = 0; v < 3; v++)
		free_irq(irqs[v], pdev);
	pci_free_irq_vectors(pdev);
	HELLO_PCI_CHECK(!pdev->msix_enabled && pci_irq_vector(pdev, 1) == -EINVAL,
			"MSI-X stayed on once its vectors were freed");
	/* No entries named, as Linux allows: its first ones, in order. */
	HELLO_PCI_CHECK(pci_enable_msix_range(pdev, NULL, 3, 3) == 3 && pdev->msix_enabled,
			"MSI-X with no entries named was refused");
	pci_disable_msix(pdev);
	return 0;
}

/* A oneshot thread with no first half: it reads the cause, as nothing did. */
static irqreturn_t hello_intx_thread(int irq, void *data)
{
	return hello_isr_first_half(irq, data) == IRQ_WAKE_THREAD ? hello_isr_thread(irq, data) :
								      IRQ_NONE;
}

/*
 * The function's legacy line (lx_irq.c's INTx), before its MSI: dev->irq a
 * number of the layer's own from before the probe; a handler on it run in
 * hardirq context once for each interrupt -- the line re-armed only after
 * the handler has run, so a serviced card does not interrupt again for
 * nothing -- and the line re-armed for the next; disabled, its interrupt
 * held until enabled; a threaded handler with no first half refused
 * without ONESHOT, as Linux refuses it, and with it run in its thread, the
 * line re-armed once the thread is done.
 */
static int hello_pci_intx(struct pci_dev *pdev)
{
	unsigned long before;
	int irq = pdev->irq;

	HELLO_PCI_CHECK(irq >= 1024 && pci_irq_vector(pdev, 0) == irq,
			"the legacy interrupt is not a number of the layer's own");
	HELLO_PCI_CHECK(pci_alloc_irq_vectors(pdev, 1, 1, PCI_IRQ_INTX) == 1 && pdev->irq == irq,
			"no INTx vector");
	HELLO_PCI_CHECK(!request_irq(irq, hello_isr, IRQF_SHARED, "hello-intx", pdev),
			"no handler requested on the legacy line");
	before = lx_irq_count(irq);
	hello_raise();
	HELLO_PCI_CHECK(wait_for_completion_timeout(&hello_irq_done, HZ), "an INTx interrupt never came");
	HELLO_PCI_CHECK(hello_irq_in_hardirq, "an INTx handler ran outside hardirq context");
	msleep(20);
	HELLO_PCI_CHECK(lx_irq_count(irq) == before + 1,
			"an INTx interrupt ran its handler more than once: the line re-armed before it was handled");
	hello_raise();
	HELLO_PCI_CHECK(wait_for_completion_timeout(&hello_irq_done, HZ),
			"a second INTx interrupt never came: the line was not re-armed");

	disable_irq(irq);
	before = lx_irq_count(irq);
	hello_raise();
	msleep(20);
	HELLO_PCI_CHECK(lx_irq_count(irq) == before, "a disabled INTx interrupt was delivered");
	enable_irq(irq);
	HELLO_PCI_CHECK(wait_for_completion_timeout(&hello_irq_done, HZ),
			"an INTx interrupt held while disabled never came once enabled");
	HELLO_PCI_CHECK(free_irq(irq, pdev), "free_irq() did not name the INTx handler");

	HELLO_PCI_CHECK(request_threaded_irq(irq, NULL, hello_intx_thread, 0, "hello-intx", pdev) ==
				-EINVAL,
			"a threaded INTx handler with no first half and no ONESHOT was not refused");
	HELLO_PCI_CHECK(!request_threaded_irq(irq, NULL, hello_intx_thread, IRQF_ONESHOT,
					      "hello-intx", pdev),
			"no oneshot threaded handler requested on the legacy line");
	hello_thread_in_task = false;
	before = lx_irq_count(irq);
	hello_raise();
	HELLO_PCI_CHECK(wait_for_completion_timeout(&hello_irq_done, HZ) && hello_thread_in_task,
			"a oneshot INTx thread did not run in its task");
	msleep(20);
	HELLO_PCI_CHECK(lx_irq_count(irq) == before + 1,
			"a oneshot INTx line was re-armed before its thread had run");
	hello_raise();
	HELLO_PCI_CHECK(wait_for_completion_timeout(&hello_irq_done, HZ),
			"the line was not re-armed once its oneshot thread had run");
	HELLO_PCI_CHECK(free_irq(irq, pdev), "free_irq() did not name the threaded INTx handler");
	return 0;
}

static int hello_pci_interrupts(struct pci_dev *pdev)
{
	unsigned long before, dropped;
	int irq, legacy;

	HELLO_PCI_CHECK(pci_alloc_irq_vectors(pdev, 1, 1, PCI_IRQ_MSI) == 1, "no MSI vector");
	irq = pci_irq_vector(pdev, 0);
	HELLO_PCI_CHECK(irq > 0 && irq == pdev->irq && pdev->msi_enabled,
			"the MSI vector's interrupt is not the device's");
	HELLO_PCI_CHECK(!request_irq(irq, hello_isr, 0, "hello", pdev), "no handler requested");
	hello_raise();
	wait_for_completion(&hello_irq_done);
	HELLO_PCI_CHECK(hello_irq_in_hardirq, "a handler ran outside hardirq context");

	/* 0xffff x 256 ns, about 16.8 ms, between the card's interrupts. */
	writel(0xffff, hello_regs + HELLO_ITR);
	hello_raise();
	wait_for_completion(&hello_irq_done);
	hello_raise();
	wait_for_completion(&hello_irq_done);
	writel(0, hello_regs + HELLO_ITR);

	disable_irq(irq);
	before = lx_irq_count(irq);
	hello_raise();
	msleep(20);
	HELLO_PCI_CHECK(lx_irq_count(irq) == before, "a disabled interrupt was delivered");
	enable_irq(irq);
	wait_for_completion(&hello_irq_done);
	HELLO_PCI_CHECK(free_irq(irq, pdev), "free_irq() did not name the handler");

	HELLO_PCI_CHECK(!request_threaded_irq(irq, hello_isr_first_half, hello_isr_thread,
					      IRQF_ONESHOT, "hello", pdev),
			"no threaded handler requested");
	hello_raise();
	wait_for_completion(&hello_irq_done);
	synchronize_irq(irq);
	HELLO_PCI_CHECK(hello_thread_in_task, "a threaded handler's second half ran outside a task");
	free_irq(irq, pdev);
	pci_free_irq_vectors(pdev);
	HELLO_PCI_CHECK(!pdev->msi_enabled && pci_irq_vector(pdev, 0) == pdev->irq && pdev->irq != irq,
			"MSI stayed on once its vectors were freed");
	/*
	 * MSI off, but still the kernel's until another set-up replaces it
	 * (lx_irq_set_mode), and none of the layer's to take: an interrupt the
	 * kernel counts is seen by a look twice -- a look takes nothing, as
	 * lx_intx_poll's must, re-arming a legacy line only once its handler
	 * has run -- then its cause is cleared at the card.
	 */
	hello_raise();
	msleep(20);
	HELLO_PCI_CHECK(lx_irq_ready(pdev) && lx_irq_ready(pdev),
			"a look at the function's interrupts took them");
	readl(hello_regs + HELLO_ICR);
	/* MSI off: the legacy line is the interrupt again, set up anew when asked for. */
	legacy = pdev->irq;
	HELLO_PCI_CHECK(!request_irq(legacy, hello_isr, IRQF_SHARED, "hello", pdev),
			"the legacy line could not be requested once MSI was off");
	hello_raise();
	HELLO_PCI_CHECK(wait_for_completion_timeout(&hello_irq_done, HZ),
			"no interrupt came on the legacy line set up again after MSI");
	/* A number kept from MSI is not the line's. */
	HELLO_PCI_CHECK(request_irq(irq, hello_isr, 0, "hello-stale", pdev) == -EINVAL,
			"a number kept from MSI reached the legacy line");
	/*
	 * MSI turned on over the line's handler: the handler stays the line's --
	 * asked for again while MSI is on, too, leaving MSI on -- never run
	 * while MSI is on, and the MSI vector is free: its interrupt dropped, at
	 * least once, since the card sends it again while its cause is unread
	 * (QEMU's e1000e, once its throttling interval, never under 128 us,
	 * ends). MSI off, the line delivers again: for a new cause, the unread
	 * one cleared first, since the card raises its line only for a cause
	 * that is new.
	 */
	before = lx_irq_count(legacy);
	HELLO_PCI_CHECK(!pci_enable_msi(pdev) && pdev->irq != legacy,
			"MSI could not be turned on over the legacy line's handler");
	HELLO_PCI_CHECK(free_irq(legacy, pdev) &&
				!request_irq(legacy, hello_isr, IRQF_SHARED, "hello", pdev) &&
				pdev->msi_enabled,
			"the legacy line's handler could not be asked for again while MSI was on");
	dropped = lx_irq_dropped(pdev->irq);
	hello_raise();
	msleep(20);
	HELLO_PCI_CHECK(lx_irq_count(legacy) == before,
			"the legacy line's handler ran for an MSI interrupt");
	HELLO_PCI_CHECK(lx_irq_dropped(pdev->irq) > dropped,
			"an MSI interrupt with no handler requested was not dropped");
	pci_disable_msi(pdev);
	HELLO_PCI_CHECK(pdev->irq == legacy, "dev->irq is not the legacy line once MSI was off");
	readl(hello_regs + HELLO_ICR);
	hello_raise();
	HELLO_PCI_CHECK(wait_for_completion_timeout(&hello_irq_done, HZ),
			"the legacy line did not deliver again once MSI was off");
	HELLO_PCI_CHECK(free_irq(legacy, pdev), "free_irq() did not name the legacy line's handler");
	writel(~0U, hello_regs + HELLO_IMC);
	readl(hello_regs + HELLO_ICR);
	return hello_pci_msix(pdev);
}

static int hello_pci_dma(struct pci_dev *pdev)
{
	size_t ring_bytes = HELLO_TX_DESCS * sizeof(struct hello_tx_desc);
	struct hello_tx_desc *ring;
	dma_addr_t ring_dma, frame_dma;
	void *big;
	u64 mask;
	u8 *frame;
	size_t i;

	HELLO_PCI_CHECK(!dma_set_mask_and_coherent(&pdev->dev, DMA_BIT_MASK(64)),
			"a 64-bit DMA mask was refused");
	HELLO_PCI_CHECK(dma_mapping_error(&pdev->dev,
					  dma_map_single(&pdev->dev, hello_static_frame,
							 sizeof(hello_static_frame), DMA_TO_DEVICE)),
			"memory outside the DMA window was mapped for the device");
	/*
	 * kmalloc and the window (linuxdrv/kit): a block the size of a chunk
	 * comes from the window, whole; a larger one from the heap, the window
	 * holding nothing past a chunk; ksize() answers for both, and
	 * kmalloc_size_roundup() as the allocator rounds. And kfree() gives
	 * back: more than the window and the heap together hold, a block at a
	 * time, all comes.
	 */
	big = kmalloc(SZ_4M, GFP_KERNEL);
	HELLO_PCI_CHECK(big && !dma_mapping_error(&pdev->dev, dma_map_single(&pdev->dev, big, SZ_4M,
									    DMA_TO_DEVICE)),
			"a block the size of a chunk did not come from the DMA window, whole");
	HELLO_PCI_CHECK(ksize(big) == SZ_4M, "ksize() of a window block was not its size");
	kfree(big);
	big = kmalloc(SZ_4M + 1, GFP_KERNEL);
	HELLO_PCI_CHECK(big && dma_mapping_error(&pdev->dev, dma_map_single(&pdev->dev, big, SZ_4M + 1,
									   DMA_TO_DEVICE)),
			"a block larger than a chunk came from the DMA window");
	HELLO_PCI_CHECK(ksize(big) == SZ_8M, "ksize() of a heap block was not its size");
	kfree(big);
	HELLO_PCI_CHECK(kmalloc_size_roundup(100) == 128,
			"kmalloc_size_roundup() did not round as the allocator does");
	for (i = 0; i < 600; i++) {
		big = kmalloc(SZ_1M, GFP_KERNEL);
		HELLO_PCI_CHECK(big, "memory kfree() gave back did not come again");
		kfree(big);
	}

	/* Coherent memory answers to the coherent mask, not the streaming one. */
	mask = pdev->dev.coherent_dma_mask;
	pdev->dev.coherent_dma_mask = PAGE_SIZE - 1;
	ring = dma_alloc_coherent(&pdev->dev, ring_bytes, &ring_dma, GFP_KERNEL);
	pdev->dev.coherent_dma_mask = mask;
	HELLO_PCI_CHECK(!ring, "coherent memory was given beyond the coherent mask");
	ring = dma_alloc_coherent(&pdev->dev, ring_bytes, &ring_dma, GFP_KERNEL);
	HELLO_PCI_CHECK(ring && ring_dma && !(ring_dma & (PAGE_SIZE - 1)),
			"no page-aligned coherent memory for a descriptor ring");
	for (i = 0; i < ring_bytes; i++)
		HELLO_PCI_CHECK(!((u8 *)ring)[i], "coherent memory was not zeroed");
	/*
	 * A width the memory already given lies beyond -- the ring's chunk, above
	 * 16 MiB -- is refused, and the mask stays what it was. (A chunk that
	 * low says nothing, and is not asked.)
	 */
	HELLO_PCI_CHECK(ring_dma < SZ_16M ||
			dma_set_coherent_mask(&pdev->dev, DMA_BIT_MASK(24)) == -EIO,
			"a mask narrower than the memory already given was taken");
	HELLO_PCI_CHECK(pdev->dev.coherent_dma_mask == DMA_BIT_MASK(64), "a refused mask was kept");
	/*
	 * Written to the end of its page, given back and taken again -- the
	 * window reuses it first -- still zero, the whole page.
	 */
	memset(ring, 0xa5, PAGE_SIZE);
	dma_free_coherent(&pdev->dev, ring_bytes, ring, ring_dma);
	ring = dma_alloc_coherent(&pdev->dev, ring_bytes, &ring_dma, GFP_KERNEL);
	HELLO_PCI_CHECK(ring, "no coherent memory the second time");
	for (i = 0; i < PAGE_SIZE; i++)
		HELLO_PCI_CHECK(!((u8 *)ring)[i], "coherent memory given back and taken again was not zeroed");
	frame = kmalloc(ETH_ZLEN, GFP_KERNEL);
	HELLO_PCI_CHECK(frame, "no memory");
	eth_broadcast_addr(frame);
	ether_addr_copy(frame + ETH_ALEN, hello_card_mac);
	memset(frame + 2 * ETH_ALEN, 0, ETH_ZLEN - 2 * ETH_ALEN);
	frame_dma = dma_map_single(&pdev->dev, frame, ETH_ZLEN, DMA_TO_DEVICE);
	HELLO_PCI_CHECK(!dma_mapping_error(&pdev->dev, frame_dma),
			"a kmalloc'd frame had no DMA address");

	ring[0].addr = cpu_to_le64(frame_dma);
	ring[0].length = cpu_to_le16(ETH_ZLEN);
	ring[0].cmd = HELLO_TXD_EOP | HELLO_TXD_IFCS | HELLO_TXD_RS;
	writel(lower_32_bits(ring_dma), hello_regs + HELLO_TDBAL);
	writel(upper_32_bits(ring_dma), hello_regs + HELLO_TDBAH);
	writel(ring_bytes, hello_regs + HELLO_TDLEN);
	writel(0, hello_regs + HELLO_TDH);
	writel(0, hello_regs + HELLO_TDT);
	writel(HELLO_TCTL_EN | HELLO_TCTL_PSP, hello_regs + HELLO_TCTL);
	/* The descriptor in memory before the card is told of it. */
	wmb();
	writel(1, hello_regs + HELLO_TDT);
	for (i = 0; i < 200 && !(READ_ONCE(ring[0].status) & HELLO_TXD_DD); i++)
		msleep(1);
	writel(0, hello_regs + HELLO_TCTL);
	HELLO_PCI_CHECK(READ_ONCE(ring[0].status) & HELLO_TXD_DD,
			"the card never wrote back the descriptor it was given");
	dma_unmap_single(&pdev->dev, frame_dma, ETH_ZLEN, DMA_TO_DEVICE);
	kfree(frame);
	dma_free_coherent(&pdev->dev, ring_bytes, ring, ring_dma);
	return 0;
}

/*
 * QEMU's edu, given after the card: a second function, whose own IOMMU
 * domain holds none of the card's DMA window -- so every DMA call for it is
 * refused, kmalloc'd memory from the window included.
 */
static int hello_edu_probe(struct pci_dev *pdev)
{
	dma_addr_t handle;
	void *p;

	HELLO_PCI_CHECK(dma_set_mask(&pdev->dev, DMA_BIT_MASK(32)) == -EIO,
			"a second function's DMA mask was taken");
	p = kmalloc(64, GFP_KERNEL);
	HELLO_PCI_CHECK(p, "no memory");
	handle = dma_map_single(&pdev->dev, p, 64, DMA_TO_DEVICE);
	kfree(p);
	HELLO_PCI_CHECK(dma_mapping_error(&pdev->dev, handle),
			"a second function was given the card's DMA window");
	HELLO_PCI_CHECK(!dma_alloc_coherent(&pdev->dev, PAGE_SIZE, &handle, GFP_KERNEL),
			"a second function was given coherent memory");
	dev_info(&pdev->dev, "a second function: its DMA refused\n");
	return 0;
}

static int hello_pci_probe(struct pci_dev *pdev, const struct pci_device_id *id)
{
	void __iomem *regs;
	u8 mac[ETH_ALEN];
	u16 command;
	u32 rah;
	int err;

	if (hello_mode("pcifail"))
		return -EIO;
	if (id->driver_data == HELLO_EDU)
		return hello_edu_probe(pdev);
	if (pci_resource_len(pdev, 0) < SZ_128K || !(pci_resource_flags(pdev, 0) & IORESOURCE_MEM) ||
	    pci_resource_len(pdev, 2) || !pdev->msi_cap) {
		dev_err(&pdev->dev, "BAR0 %pR, BAR2 %pR, MSI capability at %#x: not the card as shown\n",
			&pdev->resource[0], &pdev->resource[2], pdev->msi_cap);
		return -EPROTO;
	}
	if (!hello_bar0_sizes(pdev)) {
		dev_err(&pdev->dev, "BAR0 did not size as hardware does\n");
		return -EPROTO;
	}
	err = pci_enable_device_mem(pdev);
	if (err)
		return err;
	err = pci_request_selected_regions(pdev, pci_select_bars(pdev, IORESOURCE_MEM), "hello");
	if (err)
		goto disable;
	pci_set_master(pdev);
	pci_read_config_word(pdev, PCI_COMMAND, &command);
	if ((command & (PCI_COMMAND_MEMORY | PCI_COMMAND_MASTER)) !=
	    (PCI_COMMAND_MEMORY | PCI_COMMAND_MASTER)) {
		dev_err(&pdev->dev, "command register %#x: decoding or bus mastering off\n", command);
		err = -EPROTO;
		goto release;
	}
	regs = ioremap(pci_resource_start(pdev, 0), pci_resource_len(pdev, 0));
	if (!regs) {
		err = -ENOMEM;
		goto release;
	}
	put_unaligned_le32(readl(regs + HELLO_RAL0), mac);
	rah = readl(regs + HELLO_RAH0);
	put_unaligned_le16(rah & 0xffff, mac + 4);
	if (!(rah & HELLO_RAH_VALID) || !ether_addr_equal(mac, hello_card_mac)) {
		dev_err(&pdev->dev, "receive address 0 reads %pM, not the boot test's\n", mac);
		err = -EPROTO;
		goto unmap;
	}
	hello_regs = regs;
	err = hello_pci_intx(pdev);
	if (!err)
		err = hello_pci_interrupts(pdev);
	if (!err)
		err = hello_pci_dma(pdev);
	if (err)
		goto unmap;
	dev_info(&pdev->dev, "an 82574 (%pR), MAC %pM read through BAR0, its interrupts handled, a descriptor and a frame read by it from the DMA window\n",
		 &pdev->resource[0], mac);
	hello_pci_probed = true;
	return 0;
unmap:
	iounmap(regs);
release:
	pci_release_selected_regions(pdev, pci_select_bars(pdev, IORESOURCE_MEM));
disable:
	pci_disable_device(pdev);
	return err;
}

static const struct pci_device_id hello_pci_ids[] = {
	{ PCI_DEVICE(PCI_VENDOR_ID_INTEL, HELLO_82574) },
	{ PCI_DEVICE(0x1234, 0x11e8), .driver_data = HELLO_EDU },
	{ }
};

static struct pci_driver hello_pci_driver = {
	.name = "hello",
	.id_table = hello_pci_ids,
	.probe = hello_pci_probe,
};

static DEFINE_SPINLOCK(hello_lock);

static int __init hello_init(void)
{
	unsigned long flags, warned;
	bool off;
	int *answer, *dirty = kmalloc(sizeof(*answer), GFP_KERNEL);

	/*
	 * kzalloc clears: a block written and given back is the one the next
	 * allocation of its size gets (the free list is last in, first out),
	 * so a zero read there is the clearing's and not the memory's.
	 */
	if (!dirty)
		return -ENOMEM;
	*dirty = 0x5a5a;
	kfree(dirty);
	answer = kmalloc(sizeof(*answer), GFP_KERNEL | __GFP_ZERO);
	if (!answer)
		return -ENOMEM;
	if (answer != dirty || *answer != 0) {
		kfree(answer);
		return -EINVAL;
	}
	warned = lx_warnings;
	hello_warn_once();
	hello_warn_once();
	if (lx_warnings != warned + 1) {
		kfree(answer);
		return -EDOM;
	}
	spin_lock_irqsave(&hello_lock, flags);
	off = irqs_disabled();
	*answer = 42;
	spin_unlock_irqrestore(&hello_lock, flags);
	if (!off || irqs_disabled()) {
		kfree(answer);
		return -EFAULT;
	}
	/* The wall clock is the system's: some time after 2020 began, not the epoch. */
	if (ktime_get_real_seconds() < 1577836800) {
		pr_err("hello: the wall clock reads %lld s after 1970\n", ktime_get_real_seconds());
		kfree(answer);
		return -ERANGE;
	}
	pr_info("hello from Linux's own headers: %d, %s, %#x\n", *answer, "formatted by vsprintf", 0xbeef);
	kfree(answer);
	if (hello_mode("stop"))
		lx_hello_never_defined();
	if (hello_mode("deadlock"))
		wait_for_completion(&hello_forever);
	if (hello_mode("overflow")) {
		hello_deep(hello_overflow_kib());
		pr_err("hello: %d KiB of stack used, past the guard, and nothing faulted\n",
		       hello_overflow_kib());
		return -EFAULT;
	}
	if (hello_mode("fail")) {
		/* Not -ENODEV without its kthread: the run would prove nothing. */
		if (IS_ERR(kthread_run(hello_polls, NULL, "hello-polls")))
			return -EPROTO;
		return -ENODEV;
	}
	if (hello_mode("stranded") &&
	    IS_ERR(kthread_run(hello_waits_forever, NULL, "hello-stranded")))
		return -EPROTO;
	if (hello_mode("atomic")) {
		local_bh_disable();
		msleep(1);
		local_bh_enable();
		pr_err("hello: slept with bottom halves off, and nothing said so\n");
		return -EPROTO;
	}
	if (hello_mode("pci") || hello_mode("pcifail")) {
		int err = pci_register_driver(&hello_pci_driver);

		if (err)
			return err;
		/* "pcifail": the process's end is the layer's, once probing is done. */
		if (hello_mode("pci") && !hello_pci_probed)
			return -ENODEV;
	}
	return 0;
}
module_init(hello_init);

/*
 * Tasks: a kernel thread, a mutex held across a sleep, a completion --
 * Linux's own mutex.c and completion.c over the layer's scheduler. The
 * worker starts while the first task holds the mutex, blocks on it, takes
 * it when the first task lets go after sleeping, and completes; the steps
 * must come in that order, the sleep must have taken its time, and
 * kthread_stop() must hand back what the worker returned.
 */
static DECLARE_COMPLETION(hello_worker_done);
static DEFINE_MUTEX(hello_mutex);
static int hello_steps[4];
static int hello_step_count;

static void hello_step(int step)
{
	if (hello_step_count < ARRAY_SIZE(hello_steps))
		hello_steps[hello_step_count] = step;
	hello_step_count++;
}

static int hello_worker(void *unused)
{
	hello_step(2);
	mutex_lock(&hello_mutex);
	hello_step(4);
	mutex_unlock(&hello_mutex);
	complete(&hello_worker_done);
	while (!kthread_should_stop()) {
		set_current_state(TASK_INTERRUPTIBLE);
		if (kthread_should_stop())
			break;
		schedule();
	}
	__set_current_state(TASK_RUNNING);
	return 7;
}

static int __init hello_tasks_init(void)
{
	unsigned long start = jiffies;
	struct task_struct *worker;
	int i, ret;

	if (slx_argc() > 1)
		return 0;
	mutex_lock(&hello_mutex);
	hello_step(1);
	worker = kthread_run(hello_worker, NULL, "hello-worker");
	if (IS_ERR(worker)) {
		mutex_unlock(&hello_mutex);
		return PTR_ERR(worker);
	}
	msleep(20);
	hello_step(3);
	mutex_unlock(&hello_mutex);
	if (!wait_for_completion_timeout(&hello_worker_done, HZ))
		return -ETIMEDOUT;
	ret = kthread_stop(worker);
	if (ret != 7)
		return -EINVAL;
	if (time_before(jiffies, start + msecs_to_jiffies(20)))
		return -ERANGE;
	for (i = 0; i < ARRAY_SIZE(hello_steps); i++)
		if (hello_step_count != ARRAY_SIZE(hello_steps) || hello_steps[i] != i + 1)
			return -EPROTO;
	pr_info("hello tasks: a kthread, a mutex held across a 20 ms sleep, a completion: %lu jiffies\n",
		jiffies - start);
	return 0;
}
late_initcall(hello_tasks_init);

/*
 * The scheduler's edges: a wait that times out; kthreads stopped before
 * they ran, and out of interruptible waits, as Linux's kthread_stop() does
 * by signal, and then not put back to sleep; their records freed when they
 * end by themselves, or when the last reference goes -- by kthread_stop_put(),
 * by put_task_struct() after an RCU grace period, by a wake queue; RCU
 * callbacks waiting for a grace period; usleep_range(0, ...), cond_resched() and yield()
 * letting the others run; a task's run time counted; and jiffies moving on through busy waits, and
 * before a sleep reckons its end, as Linux's timer interrupt keeps them.
 * Each check says which failed.
 */
#define HELLO_CHECK(cond, what)						\
	do {								\
		if (!(cond)) {						\
			pr_err("hello: check failed: %s\n", what);	\
			return -EPROTO;					\
		}							\
	} while (0)

static int hello_never_runs(void *ran)
{
	*(bool *)ran = true;
	return 0;
}

static int hello_waits(void *unused)
{
	return wait_for_completion_interruptible(&hello_forever);
}

static int hello_naps(void *unused)
{
	return msleep_interruptible(10000);
}

static int hello_quick(void *flag)
{
	WRITE_ONCE(*(bool *)flag, true);
	return 5;
}

static int hello_wakes(void *task)
{
	wake_up_process(task);
	return 0;
}

/*
 * A task's run time, counted at its switches as Linux's scheduler counts
 * it: 10 ms spun, then a sleep, which switches it out.
 */
static u64 hello_spun_runtime;

static int hello_spins(void *unused)
{
	u64 t0 = slx_now_ns();

	while (slx_now_ns() - t0 < 10 * NSEC_PER_MSEC)
		barrier();
	msleep(1);
	hello_spun_runtime = current->se.sum_exec_runtime;
	return 0;
}

/*
 * Stopped while in an uninterruptible sleep, which goes on: then neither a
 * bare interruptible schedule() nor schedule_timeout_interruptible() may
 * sleep, the stop's signal pending.
 */
static int hello_sleeps_then_waits(void *unused)
{
	msleep(10);
	set_current_state(TASK_INTERRUPTIBLE);
	schedule();
	return schedule_timeout_interruptible(10 * HZ) > 9 * HZ ? 3 : -ETIME;
}

static struct rcu_head hello_rcu, hello_barrier_rcu;
static bool hello_rcu_ran, hello_rcu_in_softirq, hello_barrier_rcu_ran;

static void hello_rcu_done(struct rcu_head *head)
{
	hello_rcu_ran = true;
	hello_rcu_in_softirq = in_serving_softirq();
}

static void hello_barrier_rcu_done(struct rcu_head *head)
{
	hello_barrier_rcu_ran = true;
}


/* rcuwait: one task waiting, and a kthread that wakes it. */
static struct rcuwait hello_rcuwait = __RCUWAIT_INITIALIZER(hello_rcuwait);
static bool hello_rcuwait_cond;

static int hello_rcuwait_wakes(void *unused)
{
	WRITE_ONCE(hello_rcuwait_cond, true);
	rcuwait_wake_up(&hello_rcuwait);
	return 0;
}

/* An object freed with kfree_rcu(), its head past the start. */
struct hello_rcu_object {
	long payload[3];
	struct rcu_head rcu;
};

/*
 * The driver's own initcall, at the level of Linux's core subsystems: its
 * failure ends the process as a later one's does, where a subsystem's
 * there is carried past (lx_initcall_fatal, which hello_driver_core_init
 * asks of Linux's code and the layer's too).
 */
static int __init hello_fails_early(void)
{
	return hello_mode("earlyfail") ? -ENODEV : 0;
}
core_initcall(hello_fails_early);

/*
 * The driver's own initcall answering a positive value, which none should:
 * Linux takes it as success, and so must the process.
 */
static int __init hello_answers_positive(void)
{
	return slx_argc() > 1 ? 0 : 1;
}
late_initcall(hello_answers_positive);

/* A wake of `task`, and what try_to_wake_up() answered: 1 if it slept. */
struct hello_wake {
	struct task_struct *task;
	int woke;
};

static int hello_wakes_and_says(void *arg)
{
	struct hello_wake *wake = arg;

	wake->woke = wake_up_process(wake->task);
	return 0;
}

static int hello_naps_briefly(void *unused)
{
	set_current_state(TASK_INTERRUPTIBLE);
	schedule_timeout(1);
	return 0;
}

/* A sleep of `timeout` jiffies, and whether it has ended. */
struct hello_sleep {
	long timeout;
	bool woke;
};

static int hello_sleeps_long(void *arg)
{
	struct hello_sleep *sleep = arg;

	schedule_timeout_uninterruptible(sleep->timeout);
	WRITE_ONCE(sleep->woke, true);
	return 0;
}

static int __init hello_edges_init(void)
{
	struct task_struct *never, *waiter, *napper, *quick, *held, *waker, *sleepers[2], *k;
	struct hello_wake wake = { NULL, -1 };
	DEFINE_WAKE_Q(wake_q);
	/*
	 * The longest sleep there is, and one whose nanoseconds (at HZ 250,
	 * 4,000,000 a jiffy, 2^8 * 15625) are 2^64 * 15625: both too long to end.
	 */
	struct hello_sleep sleeps[2] = { { MAX_JIFFY_OFFSET }, { 1L << 56 } };
	bool ran = false, quick_ran = false, held_ran = false;
	bool resched_ran = false, yield_ran = false, put_ran = false, queued_ran = false;
	unsigned long j, warned;
	u64 t0, t1, give_up;
	long left, elapsed;
	int i;

	if (slx_argc() > 1)
		return 0;
	/* kthread_stop() lets its record go through RCU: freed by the next turn. */
	msleep(1);
	HELLO_CHECK(lx_live_kthreads() == 0, "a stopped kthread's record was not freed");

	j = jiffies;
	HELLO_CHECK(wait_for_completion_timeout(&hello_forever, 2) == 0,
		    "a wait that timed out said it had not");
	HELLO_CHECK(!time_before(jiffies, j + 2), "a timed-out wait ended early");

	/*
	 * A sleep reckons its end from jiffies brought up to the clock: after
	 * 120 ms that moved nothing on (a spin without cpu_relax(), after the
	 * waker is made, which blocks and so brings jiffies up), 30 jiffies, a
	 * sleep of 60 woken at once has 60 left less the jiffies the sleep
	 * itself took -- not 30 less them. Measured, so that a busy system
	 * slows the sleep without failing the check; the check can tell the
	 * two apart while the sleep takes under 30 jiffies.
	 */
	waker = kthread_run(hello_wakes, current, "hello-wakes");
	HELLO_CHECK(!IS_ERR(waker), "no kthread");
	t0 = slx_now_ns();
	while (slx_now_ns() - t0 < 120 * NSEC_PER_MSEC)
		barrier();
	t1 = slx_now_ns();
	set_current_state(TASK_INTERRUPTIBLE);
	left = schedule_timeout(60);
	elapsed = (slx_now_ns() - t1) / (NSEC_PER_SEC / HZ) + 1;
	HELLO_CHECK(left >= 60 - elapsed,
		    "a sleep woken at once reckoned its end from stale jiffies");

	for (i = 0; i < ARRAY_SIZE(sleeps); i++) {
		sleepers[i] = kthread_run(hello_sleeps_long, &sleeps[i], "hello-sleeps");
		HELLO_CHECK(!IS_ERR(sleepers[i]), "no kthread");
	}
	msleep(5);
	HELLO_CHECK(!READ_ONCE(sleeps[0].woke), "a sleep of MAX_JIFFY_OFFSET ended by itself");
	HELLO_CHECK(!READ_ONCE(sleeps[1].woke), "a sleep of 2^56 jiffies ended by itself");
	for (i = 0; i < ARRAY_SIZE(sleeps); i++)
		HELLO_CHECK(kthread_stop(sleepers[i]) == 0 && sleeps[i].woke,
			    "a long sleep was not woken");

	/*
	 * A task its deadline woke reads awake before it runs, as a Linux
	 * timer's wake leaves it: a wake that reaches it first answers 0. The
	 * waker is made first, so it takes the lower slot and runs first;
	 * the main task spins past the napper's deadline, then lets both go.
	 */
	k = kthread_create(hello_wakes_and_says, &wake, "hello-checks");
	HELLO_CHECK(!IS_ERR(k), "no kthread");
	wake.task = kthread_run(hello_naps_briefly, NULL, "hello-naps-briefly");
	HELLO_CHECK(!IS_ERR(wake.task), "no kthread");
	yield();
	t0 = slx_now_ns();
	while (slx_now_ns() - t0 < 10 * NSEC_PER_MSEC)
		barrier();
	wake_up_process(k);
	msleep(1);
	HELLO_CHECK(wake.woke == 0, "a task its deadline woke still read asleep");

	never = kthread_create(hello_never_runs, &ran, "hello-never");
	HELLO_CHECK(!IS_ERR(never), "no kthread");
	HELLO_CHECK(kthread_stop(never) == -EINTR && !ran,
		    "a kthread stopped before it ran did not end unrun, with -EINTR");

	waiter = kthread_run(hello_waits, NULL, "hello-waits");
	HELLO_CHECK(!IS_ERR(waiter), "no kthread");
	napper = kthread_run(hello_naps, NULL, "hello-naps");
	HELLO_CHECK(!IS_ERR(napper), "no kthread");
	msleep(5);
	HELLO_CHECK(kthread_stop(waiter) == -ERESTARTSYS,
		    "kthread_stop() did not end an interruptible wait");
	HELLO_CHECK(kthread_stop(napper) > 0, "kthread_stop() did not end msleep_interruptible()");
	k = kthread_run(hello_sleeps_then_waits, NULL, "hello-sleeps-waits");
	HELLO_CHECK(!IS_ERR(k), "no kthread");
	msleep(2);
	HELLO_CHECK(kthread_stop(k) == 3,
		    "a stopped kthread slept in schedule_timeout_interruptible()");

	/*
	 * An RCU callback waits for a grace period: a switch. It runs in
	 * softirq context, as Linux's RCU runs it; rcu_barrier() waits for it.
	 */
	call_rcu(&hello_rcu, hello_rcu_done);
	HELLO_CHECK(!hello_rcu_ran, "an RCU callback ran before a grace period");
	msleep(1);
	HELLO_CHECK(hello_rcu_ran, "an RCU callback never ran");
	HELLO_CHECK(hello_rcu_in_softirq, "an RCU callback ran outside softirq context");
	call_rcu(&hello_barrier_rcu, hello_barrier_rcu_done);
	rcu_barrier();
	HELLO_CHECK(hello_barrier_rcu_ran, "rcu_barrier() did not wait for a callback");

	/* rcuwait: the waiter sleeps until the kthread wakes it by name. */
	HELLO_CHECK(!IS_ERR(kthread_run(hello_rcuwait_wakes, NULL, "hello-rcuwait")),
		    "no kthread");
	HELLO_CHECK(rcuwait_wait_event(&hello_rcuwait, READ_ONCE(hello_rcuwait_cond),
				       TASK_UNINTERRUPTIBLE) == 0,
		    "an rcuwait was interrupted");
	HELLO_CHECK(!rcu_access_pointer(hello_rcuwait.task), "an rcuwait still named its waiter");

	/*
	 * kfree_rcu() frees after a grace period, and kvfree_rcu() without a
	 * head at once (its caller may sleep): seen by kmalloc handing back the
	 * block last freed, its free list being last in, first out.
	 */
	{
		struct hello_rcu_object *obj = kmalloc(sizeof(*obj), GFP_KERNEL), *again;

		HELLO_CHECK(obj, "no memory");
		kfree_rcu(obj, rcu);
		again = kmalloc(sizeof(*obj), GFP_KERNEL);
		HELLO_CHECK(again != obj, "kfree_rcu() freed before a grace period");
		kfree(again);
		msleep(1);
		again = kmalloc(sizeof(*obj), GFP_KERNEL);
		HELLO_CHECK(again == obj, "kfree_rcu() never freed its object");
		kvfree_rcu_mightsleep(again);
		obj = kmalloc(sizeof(*obj), GFP_KERNEL);
		HELLO_CHECK(obj == again, "kvfree_rcu() without a head did not free at once");
		kfree(obj);
	}

	/* put_task_struct() frees the last reference after a grace period. */
	k = kthread_run(hello_quick, &put_ran, "hello-put");
	HELLO_CHECK(!IS_ERR(k), "no kthread");
	get_task_struct(k);
	msleep(1);
	/* An ended thread cannot park: Linux warns, and refuses rather than waits. */
	warned = lx_warnings;
	HELLO_CHECK(kthread_park(k) == -ENOSYS && lx_warnings == warned + 1,
		    "kthread_park() of an ended kthread did not refuse");
	HELLO_CHECK(kthread_stop(k) == 5 && put_ran, "a held kthread did not run");
	put_task_struct(k);
	HELLO_CHECK(lx_live_kthreads() == 1, "put_task_struct() freed before a grace period");
	msleep(1);
	HELLO_CHECK(lx_live_kthreads() == 0, "put_task_struct() never freed the record");

	/*
	 * A wake queue takes over a reference its caller held, lets go of a
	 * second given for a task already queued, and of its own once woken.
	 */
	k = kthread_create(hello_quick, &queued_ran, "hello-wakeq");
	HELLO_CHECK(!IS_ERR(k), "no kthread");
	get_task_struct(k);
	get_task_struct(k);
	wake_q_add_safe(&wake_q, k);
	wake_q_add_safe(&wake_q, k);
	wake_up_q(&wake_q);
	msleep(1);
	HELLO_CHECK(queued_ran && lx_live_kthreads() == 0, "a wake queue kept or lost a reference");

	/* A long loop's cond_resched() lets the others run once its slice is spent. */
	HELLO_CHECK(!IS_ERR(kthread_run(hello_quick, &resched_ran, "hello-resched")), "no kthread");
	give_up = slx_now_ns() + NSEC_PER_SEC;
	while (!READ_ONCE(resched_ran) && slx_now_ns() < give_up)
		cond_resched();
	HELLO_CHECK(resched_ran, "a second of cond_resched() let no other task run");
	HELLO_CHECK(!IS_ERR(kthread_run(hello_quick, &yield_ran, "hello-yield")), "no kthread");
	yield();
	HELLO_CHECK(yield_ran, "yield() let no other task run");

	/* Started by the sleep, so that kthread_stop() then waits for its end. */
	k = kthread_run(hello_spins, NULL, "hello-spins");
	HELLO_CHECK(!IS_ERR(k), "no kthread");
	msleep(1);
	HELLO_CHECK(kthread_stop(k) == 0 && hello_spun_runtime >= 10 * NSEC_PER_MSEC,
		    "10 ms a task spun were not counted as its run time");

	quick = kthread_run(hello_quick, &quick_ran, "hello-quick");
	HELLO_CHECK(!IS_ERR(quick), "no kthread");
	held = kthread_run(hello_quick, &held_ran, "hello-held");
	HELLO_CHECK(!IS_ERR(held), "no kthread");
	get_task_struct(held);
	for (i = 0; i < 100 && !(READ_ONCE(quick_ran) && READ_ONCE(held_ran)); i++)
		usleep_range(0, 50);
	HELLO_CHECK(quick_ran && held_ran, "usleep_range(0, 50) let no other task run");
	HELLO_CHECK(lx_live_kthreads() == 1,
		    "a kthread that ended by itself was not freed, or one still held was");
	HELLO_CHECK(kthread_stop_put(held) == 5,
		    "kthread_stop_put() did not hand back an ended kthread's result");
	msleep(1);
	HELLO_CHECK(lx_live_kthreads() == 0, "kthread_stop_put() did not free an ended kthread");

	t0 = slx_now_ns();
	j = jiffies;
	mdelay(10);
	HELLO_CHECK(slx_now_ns() - t0 >= 10 * NSEC_PER_MSEC, "mdelay(10) took less than 10 ms");
	HELLO_CHECK(!time_before(jiffies, j + 2), "jiffies stood still through mdelay(10)");

	j = jiffies;
	give_up = slx_now_ns() + NSEC_PER_SEC;
	while (time_before(jiffies, j + 2) && slx_now_ns() < give_up)
		cpu_relax();
	HELLO_CHECK(!time_before(jiffies, j + 2), "jiffies stood still through cpu_relax()");

	pr_info("hello edges: a wait timed out, and the longest did not; kthreads stopped unrun, and out of interruptible waits; ended ones freed; usleep_range(0), cond_resched() and yield() let others run; busy waits move jiffies\n");
	return 0;
}
late_initcall(hello_edges_init);

/*
 * Timers and softirqs: Linux's own timer wheel and softirq code, run by the
 * layer's loop as Linux's tick and ksoftirqd would. A timer fires no sooner
 * than its jiffy, in softirq context, while the only task waits on it -- so
 * the loop sleeps until it, rather than ending the process as deadlocked;
 * a deleted one never fires; one moved sooner fires sooner; one re-arming
 * itself fires each time; a tasklet scheduled under local_bh_disable() runs
 * at local_bh_enable(), on the softirq stack, and one scheduled plainly at
 * the loop's next turn.
 */
static struct timer_list hello_timer, hello_cancelled, hello_moved, hello_ticker;
static DECLARE_COMPLETION(hello_timer_done);
static DECLARE_COMPLETION(hello_moved_done);
static DECLARE_COMPLETION(hello_ticker_done);
static unsigned long hello_timer_fired_at;
static bool hello_timer_in_softirq, hello_cancelled_fired;
static int hello_ticks;

/* A tasklet that schedules itself again until its 20th run. */
static struct tasklet_struct hello_chain;
static int hello_chain_runs;
static DECLARE_COMPLETION(hello_chain_done);

static void hello_chain_fn(struct tasklet_struct *t)
{
	if (++hello_chain_runs < 20)
		tasklet_schedule(t);
	else
		complete(&hello_chain_done);
}

static struct timer_list hello_deferred;
static bool hello_deferred_fired;

static void hello_deferred_fn(struct timer_list *t)
{
	hello_deferred_fired = true;
}

/* A tasklet calling cond_resched() for 10 ms, and whether another task ran. */
static struct tasklet_struct hello_resched_tasklet;
static bool hello_other_ran, hello_ran_beside_tasklet;

static void hello_resched_tasklet_fn(struct tasklet_struct *t)
{
	u64 t0 = slx_now_ns();

	while (slx_now_ns() - t0 < 10 * NSEC_PER_MSEC)
		cond_resched();
	hello_ran_beside_tasklet = READ_ONCE(hello_other_ran);
}

/*
 * An irq_work queued from a timer, inside the loop's turn: the turn after
 * runs it, in hardirq context, though nothing else is pending -- the loop
 * must not sleep with it queued.
 */
static struct irq_work hello_irq_work;
static struct timer_list hello_irq_work_timer;
static DECLARE_COMPLETION(hello_irq_work_done);
static bool hello_irq_work_in_hardirq;

static void hello_irq_work_fn(struct irq_work *work)
{
	hello_irq_work_in_hardirq = in_hardirq() && irqs_disabled();
	complete(&hello_irq_work_done);
}

static void hello_irq_work_timer_fn(struct timer_list *t)
{
	irq_work_queue(&hello_irq_work);
}

static int hello_cpu_onlined = -1;

static int hello_cpu_online(unsigned int cpu)
{
	hello_cpu_onlined = cpu;
	return 0;
}

static void hello_timer_fn(struct timer_list *t)
{
	hello_timer_fired_at = jiffies;
	hello_timer_in_softirq = in_serving_softirq();
	complete(&hello_timer_done);
}

static void hello_cancelled_fn(struct timer_list *t)
{
	hello_cancelled_fired = true;
}

static void hello_moved_fn(struct timer_list *t)
{
	complete(&hello_moved_done);
}

static void hello_ticker_fn(struct timer_list *t)
{
	if (++hello_ticks < 5)
		mod_timer(t, jiffies + 1);
	else
		complete(&hello_ticker_done);
}

static struct tasklet_struct hello_tasklet, hello_late_tasklet;
static bool hello_tasklet_ran, hello_tasklet_on_its_stack, hello_late_tasklet_ran;

static void hello_tasklet_fn(struct tasklet_struct *t)
{
	char *top = (char *)*raw_cpu_ptr(&hardirq_stack_ptr), here;

	hello_tasklet_ran = true;
	hello_tasklet_on_its_stack = &here < top && &here > top - slx_stack_bytes;
}

static void hello_late_tasklet_fn(struct tasklet_struct *t)
{
	hello_late_tasklet_ran = true;
}

static int __init hello_timers_init(void)
{
	unsigned long j;

	if (slx_argc() > 1)
		return 0;

	timer_setup(&hello_timer, hello_timer_fn, 0);
	j = jiffies;
	mod_timer(&hello_timer, j + 3);
	HELLO_CHECK(timer_pending(&hello_timer), "an armed timer was not pending");
	/* Nothing else can wake this: the loop must sleep until the timer. */
	wait_for_completion(&hello_timer_done);
	HELLO_CHECK(!time_before(hello_timer_fired_at, j + 3), "a timer fired before its jiffy");
	HELLO_CHECK(hello_timer_in_softirq, "a timer's function ran outside softirq context");
	HELLO_CHECK(!timer_pending(&hello_timer), "a timer that fired was still pending");

	timer_setup(&hello_cancelled, hello_cancelled_fn, 0);
	mod_timer(&hello_cancelled, jiffies + 2);
	HELLO_CHECK(timer_delete_sync(&hello_cancelled) == 1, "deleting an armed timer said it was not");
	HELLO_CHECK(timer_delete(&hello_cancelled) == 0, "deleting an idle timer said it was armed");
	msleep(20);
	HELLO_CHECK(!hello_cancelled_fired, "a deleted timer fired");

	timer_setup(&hello_moved, hello_moved_fn, 0);
	mod_timer(&hello_moved, jiffies + 10 * HZ);
	HELLO_CHECK(mod_timer(&hello_moved, jiffies + 2) == 1,
		    "moving an armed timer said it was not armed");
	HELLO_CHECK(wait_for_completion_timeout(&hello_moved_done, HZ),
		    "a timer moved sooner did not fire sooner");

	timer_setup(&hello_ticker, hello_ticker_fn, 0);
	j = jiffies;
	mod_timer(&hello_ticker, j + 1);
	HELLO_CHECK(wait_for_completion_timeout(&hello_ticker_done, HZ) && hello_ticks == 5,
		    "a timer re-arming itself did not fire five times");
	HELLO_CHECK(!time_before(jiffies, j + 5), "five re-armed jiffies took less than five");

	tasklet_setup(&hello_tasklet, hello_tasklet_fn);
	local_bh_disable();
	tasklet_schedule(&hello_tasklet);
	HELLO_CHECK(!hello_tasklet_ran, "a tasklet ran while bottom halves were off");
	local_bh_enable();
	HELLO_CHECK(hello_tasklet_ran, "a tasklet did not run at local_bh_enable()");
	HELLO_CHECK(hello_tasklet_on_its_stack, "a tasklet did not run on the softirq stack");

	tasklet_setup(&hello_late_tasklet, hello_late_tasklet_fn);
	tasklet_schedule(&hello_late_tasklet);
	msleep(1);
	HELLO_CHECK(hello_late_tasklet_ran, "a tasklet scheduled in a task never ran");

	/* More than one turn's softirqs, waited on with nothing else pending. */
	tasklet_setup(&hello_chain, hello_chain_fn);
	tasklet_schedule(&hello_chain);
	wait_for_completion(&hello_chain_done);
	HELLO_CHECK(hello_chain_runs == 20, "a tasklet scheduling itself stopped short");

	/* A deferrable timer runs on the first turn past its jiffy, as on a tick. */
	timer_setup(&hello_deferred, hello_deferred_fn, TIMER_DEFERRABLE);
	mod_timer(&hello_deferred, jiffies + 2);
	msleep(50);
	HELLO_CHECK(hello_deferred_fired, "a deferrable timer did not run past its jiffy");

	/* cond_resched() in softirq context lets no task run. */
	HELLO_CHECK(!IS_ERR(kthread_run(hello_quick, &hello_other_ran, "hello-other")),
		    "no kthread");
	tasklet_setup(&hello_resched_tasklet, hello_resched_tasklet_fn);
	local_bh_disable();
	tasklet_schedule(&hello_resched_tasklet);
	local_bh_enable();
	HELLO_CHECK(!hello_ran_beside_tasklet, "cond_resched() in a tasklet let another task run");
	msleep(1);
	HELLO_CHECK(hello_other_ran, "a kthread never ran");

	/* Nothing but the irq_work a timer queues can wake this. */
	init_irq_work(&hello_irq_work, hello_irq_work_fn);
	timer_setup(&hello_irq_work_timer, hello_irq_work_timer_fn, 0);
	mod_timer(&hello_irq_work_timer, jiffies + 1);
	wait_for_completion(&hello_irq_work_done);
	HELLO_CHECK(hello_irq_work_in_hardirq,
		    "an irq_work ran outside hardirq context, or with interrupts on");

	/* A hotplug state's startup runs for the one CPU, as Linux's for each. */
	HELLO_CHECK(cpuhp_setup_state(CPUHP_AP_ONLINE_DYN, "hello:online", hello_cpu_online,
				      NULL) >= 0 && hello_cpu_onlined == 0,
		    "a hotplug state's startup did not run for CPU 0");

	pr_info("hello timers: a timer slept on, deleted, moved, re-armed, deferred; tasklets at local_bh_enable, at the next turn, twenty in a chain; no switch from softirq; an irq_work queued from a timer; a hotplug startup\n");
	return 0;
}
late_initcall(hello_timers_init);

/*
 * Workqueues: Linux's own, over the layer's kthreads and timers. A work
 * item runs in a worker, and flush_work() waits for it; delayed work waits
 * its jiffies; delayed work cancelled before its time never runs; a
 * driver's own workqueue runs its work and is destroyed; and a work item
 * that sleeps lets another, queued behind it on the same pool, run
 * meanwhile -- the concurrency Linux's scheduler gives a pool by telling it
 * when a worker sleeps -- and so does one that hogs the CPU, once Linux's
 * tick has marked it CPU-intensive. A worker, or a kthread worker, counts
 * as idle (a daemon, not waited for) only while it has no work in hand.
 */
static bool hello_work_in_worker, hello_delayed_ran, hello_cancelled_work_ran;
static bool hello_own_work_ran, hello_sleeper_done, hello_passed_sleeper;
static unsigned long hello_delayed_at;

/* The worker that ran hello_work, and whether it counted as idle meanwhile. */
static struct task_struct *hello_work_worker;
static bool hello_busy_worker_idle;

static void hello_work_fn(struct work_struct *work)
{
	hello_work_in_worker = current->flags & PF_WQ_WORKER;
	hello_work_worker = current;
	hello_busy_worker_idle = lx_emul_task_is_daemon(current);
}

/* A kthread worker's work item, and whether its worker counted as idle meanwhile. */
static bool hello_kwork_ran, hello_busy_kworker_idle;

static void hello_kwork_fn(struct kthread_work *work)
{
	hello_kwork_ran = true;
	hello_busy_kworker_idle = lx_emul_task_is_daemon(current);
}

static DECLARE_COMPLETION(hello_delayed_done);

static void hello_delayed_fn(struct work_struct *work)
{
	hello_delayed_at = jiffies;
	hello_delayed_ran = true;
	complete(&hello_delayed_done);
}

static void hello_cancelled_work_fn(struct work_struct *work)
{
	hello_cancelled_work_ran = true;
}

static void hello_own_work_fn(struct work_struct *work)
{
	hello_own_work_ran = true;
}

static void hello_sleeper_fn(struct work_struct *work)
{
	msleep(20);
	WRITE_ONCE(hello_sleeper_done, true);
}

static void hello_passer_fn(struct work_struct *work)
{
	hello_passed_sleeper = !READ_ONCE(hello_sleeper_done);
}

/*
 * A work item hogging the CPU, giving way only at cond_resched(), and one
 * queued behind it: Linux's tick marks the hog CPU-intensive once it has
 * run 10 ms (wq_worker_tick), and its pool then runs the other beside it.
 * The hog gives up after a second, so that a pool that never does fails
 * the check, not the boot; half a second is what the check allows.
 */
static bool hello_behind_ran;
static u64 hello_hog_waited_ns = U64_MAX;

static void hello_hog_fn(struct work_struct *work)
{
	u64 t0 = slx_now_ns(), now;

	do {
		cond_resched();
		now = slx_now_ns();
		if (READ_ONCE(hello_behind_ran)) {
			hello_hog_waited_ns = now - t0;
			return;
		}
	} while (now - t0 < NSEC_PER_SEC);
}

static void hello_behind_fn(struct work_struct *work)
{
	WRITE_ONCE(hello_behind_ran, true);
}

static DECLARE_WORK(hello_work, hello_work_fn);
static DECLARE_DELAYED_WORK(hello_delayed, hello_delayed_fn);
static DECLARE_DELAYED_WORK(hello_cancelled_work, hello_cancelled_work_fn);
static DECLARE_WORK(hello_own_work, hello_own_work_fn);
static DECLARE_WORK(hello_sleeper, hello_sleeper_fn);
static DECLARE_WORK(hello_passer, hello_passer_fn);
static DECLARE_WORK(hello_hog, hello_hog_fn);
static DECLARE_WORK(hello_behind, hello_behind_fn);

static int __init hello_workqueues_init(void)
{
	struct workqueue_struct *wq;
	unsigned long j;

	if (slx_argc() > 1)
		return 0;

	/* The layer's population counts (lx_emul.c), at run time. */
	{
		volatile u64 bits = 0x8000000000000001ULL, ones = ~0ULL, none = 0;

		HELLO_CHECK(hweight64(bits) == 2 && hweight64(ones) == 64 &&
			    hweight64(none) == 0 && hweight32((u32)ones) == 32 &&
			    hweight32((u32)(bits >> 32)) == 1,
			    "a population count was wrong");
	}

	HELLO_CHECK(schedule_work(&hello_work), "a work item was not queued");
	flush_work(&hello_work);
	HELLO_CHECK(hello_work_in_worker, "a work item did not run in a worker");
	/*
	 * A worker is a daemon -- not waited for at the end -- only while it
	 * has no work item in hand, so that a stuck item reads as a deadlock;
	 * by the flush's end it has gone back to waiting for work.
	 */
	HELLO_CHECK(!hello_busy_worker_idle, "a worker running a work item counted as idle");
	HELLO_CHECK(lx_emul_task_is_daemon(hello_work_worker), "an idle worker did not count as idle");

	/* And a kthread worker the same. */
	{
		struct kthread_worker *kw = kthread_run_worker(0, "hello-kworker");
		struct kthread_work kwork;

		HELLO_CHECK(!IS_ERR(kw), "no kthread worker");
		kthread_init_work(&kwork, hello_kwork_fn);
		kthread_queue_work(kw, &kwork);
		kthread_flush_work(&kwork);
		HELLO_CHECK(hello_kwork_ran && !hello_busy_kworker_idle,
			    "a kthread worker running a work item counted as idle");
		HELLO_CHECK(lx_emul_task_is_daemon(kw->task),
			    "an idle kthread worker did not count as idle");
		kthread_destroy_worker(kw);
	}

	j = jiffies;
	HELLO_CHECK(schedule_delayed_work(&hello_delayed, 3), "delayed work was not queued");
	/* Not flush_delayed_work(), which runs it at once: its timer's time. */
	wait_for_completion(&hello_delayed_done);
	HELLO_CHECK(hello_delayed_ran && !time_before(hello_delayed_at, j + 3),
		    "delayed work did not wait its jiffies");

	schedule_delayed_work(&hello_cancelled_work, 2);
	HELLO_CHECK(cancel_delayed_work_sync(&hello_cancelled_work),
		    "cancelling pending delayed work said none was pending");
	msleep(20);
	HELLO_CHECK(!hello_cancelled_work_ran, "cancelled delayed work ran");

	wq = alloc_workqueue("hello", 0, 0);
	HELLO_CHECK(wq, "no workqueue");
	queue_work(wq, &hello_own_work);
	flush_workqueue(wq);
	destroy_workqueue(wq);
	HELLO_CHECK(hello_own_work_ran, "a driver's own workqueue did not run its work");

	schedule_work(&hello_sleeper);
	schedule_work(&hello_passer);
	flush_work(&hello_sleeper);
	flush_work(&hello_passer);
	HELLO_CHECK(hello_sleeper_done && hello_passed_sleeper,
		    "work queued behind a sleeping work item waited for it");

	schedule_work(&hello_hog);
	schedule_work(&hello_behind);
	flush_work(&hello_hog);
	flush_work(&hello_behind);
	HELLO_CHECK(hello_hog_waited_ns < NSEC_PER_SEC / 2,
		    "work queued behind a CPU-hogging work item waited half a second for it");

	pr_info("hello workqueues: work in a worker, delayed, cancelled, a queue of the driver's own, workers idle and busy, work passing a sleeper and a CPU hog (%llu ms)\n",
		hello_hog_waited_ns / NSEC_PER_MSEC);
	return 0;
}
late_initcall(hello_workqueues_init);

/*
 * The driver core: Linux's own drivers/base. A bus of hello's own, a driver
 * on it and a device added to it -- matched and probed, the probe's managed
 * resource (devm) let go when the device goes, its driver's remove() called
 * and its release() last; a line through dev_info(); a device link made
 * between two devices and dropped, whose release waits for an SRCU grace
 * period (Linux's tiny SRCU, over the loop's irq_work and a workqueue)
 * before it lets go of the two. And the initcall rule (lx_initcall_fatal):
 * a failure ends the process only when the code is the driver's.
 */
static int hello_probes, hello_removes, hello_devm_releases;
static DECLARE_COMPLETION(hello_dev_released);
static DECLARE_COMPLETION(hello_consumer_released);

static int hello_bus_match(struct device *dev, const struct device_driver *drv)
{
	return 1;
}

static const struct bus_type hello_bus = {
	.name = "hello",
	.match = hello_bus_match,
};

static void hello_devm_release(void *unused)
{
	hello_devm_releases++;
}

static int hello_drv_probe(struct device *dev)
{
	int ret = devm_add_action_or_reset(dev, hello_devm_release, NULL);

	if (ret)
		return ret;
	hello_probes++;
	dev_info(dev, "probed by hello's driver\n");
	return 0;
}

static int hello_drv_remove(struct device *dev)
{
	hello_removes++;
	return 0;
}

static struct device_driver hello_drv = {
	.name = "hello",
	.bus = &hello_bus,
	.probe = hello_drv_probe,
	.remove = hello_drv_remove,
};

static void hello_dev_release(struct device *dev)
{
	complete(&hello_dev_released);
}

static void hello_consumer_release(struct device *dev)
{
	complete(&hello_consumer_released);
}

static void hello_supplier_release(struct device *dev)
{
}

static struct device hello_dev = {
	.init_name = "hello0",
	.bus = &hello_bus,
	.release = hello_dev_release,
};

static struct device hello_supplier = {
	.init_name = "hello-supplier",
	.release = hello_supplier_release,
};

static struct device hello_consumer = {
	.init_name = "hello-consumer",
	.release = hello_consumer_release,
};

/* A driver that probes asynchronously, on a bus of its own. */
static int hello_async_probes;

static int hello_async_bus_match(struct device *dev, const struct device_driver *drv)
{
	return 1;
}

static const struct bus_type hello_async_bus = {
	.name = "hello-async",
	.match = hello_async_bus_match,
};

static int hello_async_probe(struct device *dev)
{
	hello_async_probes++;
	return 0;
}

static struct device_driver hello_async_drv = {
	.name = "hello-async",
	.bus = &hello_async_bus,
	.probe = hello_async_probe,
	.probe_type = PROBE_PREFER_ASYNCHRONOUS,
};

static void hello_static_release(struct device *dev)
{
}

static struct device hello_async_dev = {
	.init_name = "hello-async0",
	.bus = &hello_async_bus,
	.release = hello_static_release,
};

/*
 * A class whose devices' directories carry a namespace, as the net class's
 * do: its own directory is a namespaced one (kernfs_enable_ns()).
 */
static int hello_ns_tag;

static bool hello_ns_may_mount(void)
{
	return true;
}

static void *hello_ns_grab(void)
{
	return &hello_ns_tag;
}

static const void *hello_ns_initial(void)
{
	return &hello_ns_tag;
}

static void hello_ns_drop(void *ns)
{
}

static const struct kobj_ns_type_operations hello_ns_ops = {
	.type = KOBJ_NS_TYPE_NET,
	.current_may_mount = hello_ns_may_mount,
	.grab_current_ns = hello_ns_grab,
	.initial_ns = hello_ns_initial,
	.drop_ns = hello_ns_drop,
};

static const void *hello_ns_of(const struct device *dev)
{
	return &hello_ns_tag;
}

static const struct class hello_ns_class = {
	.name = "hello-ns",
	.ns_type = &hello_ns_ops,
	.namespace = hello_ns_of,
};

/*
 * Two devices of one class under one parent share the class's glue
 * directory there, which must stay while either is in it: Linux's own
 * reference counting, over the layer's directory nodes.
 */
static const struct class hello_glue_class = { .name = "hello-glue" };
static struct device hello_glue_parent = {
	.init_name = "hello-glue-parent",
	.release = hello_static_release,
};
static struct device hello_glue[2] = {
	{ .init_name = "hello-glue0", .class = &hello_glue_class, .parent = &hello_glue_parent,
	  .release = hello_static_release },
	{ .init_name = "hello-glue1", .class = &hello_glue_class, .parent = &hello_glue_parent,
	  .release = hello_static_release },
};

/* An SRCU reader inside its read-side section for 20 ms. */
DEFINE_STATIC_SRCU(hello_srcu);
static bool hello_srcu_reader_done;

static int hello_srcu_reader(void *unused)
{
	int idx = srcu_read_lock(&hello_srcu);

	msleep(20);
	WRITE_ONCE(hello_srcu_reader_done, true);
	srcu_read_unlock(&hello_srcu, idx);
	return 0;
}

static int __init hello_driver_core_init(void)
{
	struct device_link *link;

	if (slx_argc() > 1)
		return 0;

	HELLO_CHECK(lx_initcall_fatal((const void *)hello_init, -ENODEV), "a driver's failing initcall did not end the process");
	HELLO_CHECK(!lx_initcall_fatal((const void *)kthreadd, -ENODEV), "a failure in Linux's code ended the process");
	HELLO_CHECK(!lx_initcall_fatal((const void *)lx_emul_main, -ENODEV), "a failure in the layer's code ended the process");
	HELLO_CHECK(!lx_initcall_fatal((const void *)hello_init, 1) && !lx_initcall_fatal((const void *)hello_init, 0),
		    "a driver's initcall answering 0 or more ended the process");

	/*
	 * Linux's direct map over the heap (lx_mm.c): a kmalloc'd block's page
	 * says it is slab memory -- what kernel/resource.c frees a released
	 * resource by -- and Linux's own arithmetic goes from the block to its
	 * page and back, both ways.
	 */
	{
		char *block = kmalloc(200, GFP_KERNEL);
		struct page *page;

		HELLO_CHECK(block, "no memory");
		page = virt_to_page(block + 150);
		HELLO_CHECK(PageSlab(page), "a kmalloc'd block's page did not say it was slab");
		HELLO_CHECK(page_address(page) == (void *)((unsigned long)block & PAGE_MASK) &&
			    __pa(block) == (unsigned long)block - page_offset_base &&
			    virt_to_page(__va(__pa(block))) == virt_to_page(block),
			    "the direct map's arithmetic did not go from a block to its page and back");
		kfree(block);
	}

	HELLO_CHECK(bus_register(&hello_bus) == 0, "a bus was not registered");
	HELLO_CHECK(driver_register(&hello_drv) == 0, "a driver was not registered");
	HELLO_CHECK(device_register(&hello_dev) == 0, "a device was not registered");
	HELLO_CHECK(hello_probes == 1 && hello_dev.driver == &hello_drv,
		    "a device on a bus with a matching driver was not probed by it");
	device_unregister(&hello_dev);
	HELLO_CHECK(hello_removes == 1, "a device's driver was not told it went");
	HELLO_CHECK(hello_devm_releases == 1, "a probe's managed resource outlived its device");
	HELLO_CHECK(wait_for_completion_timeout(&hello_dev_released, HZ),
		    "a device's last reference never went");
	driver_unregister(&hello_drv);
	bus_unregister(&hello_bus);

	HELLO_CHECK(device_register(&hello_supplier) == 0 && device_register(&hello_consumer) == 0,
		    "a device was not registered");
	link = device_link_add(&hello_consumer, &hello_supplier, DL_FLAG_STATELESS);
	HELLO_CHECK(link, "a device link was not made");
	device_link_del(link);
	device_unregister(&hello_consumer);
	device_unregister(&hello_supplier);
	HELLO_CHECK(wait_for_completion_timeout(&hello_consumer_released, HZ),
		    "a dropped device link never let go of its consumer");

	/* A grace period waits for a reader already inside its section. */
	HELLO_CHECK(!IS_ERR(kthread_run(hello_srcu_reader, NULL, "hello-srcu")), "no kthread");
	msleep(1);
	synchronize_srcu(&hello_srcu);
	HELLO_CHECK(READ_ONCE(hello_srcu_reader_done),
		    "synchronize_srcu() returned while a reader was in its read-side section");

	/* Asynchronous probing: scheduled on async's own workqueue, waited for. */
	HELLO_CHECK(bus_register(&hello_async_bus) == 0 && device_register(&hello_async_dev) == 0 &&
		    driver_register(&hello_async_drv) == 0,
		    "an asynchronously probing driver was not registered");
	wait_for_device_probe();
	HELLO_CHECK(hello_async_probes == 1 && hello_async_dev.driver == &hello_async_drv,
		    "a device was not probed asynchronously");
	device_unregister(&hello_async_dev);
	driver_unregister(&hello_async_drv);
	bus_unregister(&hello_async_bus);

	{
		int ns = kobj_ns_type_register(&hello_ns_ops), cls = class_register(&hello_ns_class);

		if (ns || cls)
			pr_err("hello: kobj_ns_type_register %d, class_register %d\n", ns, cls);
		HELLO_CHECK(!ns && !cls, "a namespaced class was not registered");
	}
	class_unregister(&hello_ns_class);

	HELLO_CHECK(!class_register(&hello_glue_class) && !device_register(&hello_glue_parent) &&
		    !device_register(&hello_glue[0]) && !device_register(&hello_glue[1]),
		    "a class's devices under a parent were not registered");
	HELLO_CHECK(hello_glue[0].kobj.parent == hello_glue[1].kobj.parent &&
		    hello_glue[0].kobj.parent != &hello_glue_parent.kobj,
		    "two devices of a class under one parent did not share a glue directory");
	device_unregister(&hello_glue[0]);
	HELLO_CHECK(hello_glue[1].kobj.parent->state_in_sysfs,
		    "a glue directory went while a device was still in it");
	device_unregister(&hello_glue[1]);
	device_unregister(&hello_glue_parent);
	class_unregister(&hello_glue_class);

	pr_info("hello driver core: a bus, a driver and a device, probed, managed, removed and released; a device link dropped after an SRCU grace period, which waits for a reader; asynchronous probing; a namespaced class; a glue directory kept for its devices; the direct map\n");
	return 0;
}
late_initcall(hello_driver_core_init);

/*
 * Module parameters of hello's own, which the "firmware" run is given
 * (hello.answer=42 "hello.motto=two words" hello.device_level=6) and set
 * from the process's arguments by Linux's own parse_args() (lx_emul.c):
 * a module's own before any initcall, one of the device level's
 * (device_param_cb) just before the device initcalls, so that a core
 * initcall still sees it unset (hello_params_early).
 */
static int hello_answer;
module_param_named(answer, hello_answer, int, 0444);
static char *hello_motto;
module_param_named(motto, hello_motto, charp, 0444);
static int hello_device_level;
device_param_cb(device_level, &param_ops_int, &hello_device_level, 0444);
static int hello_device_level_at_core = -1;

static int __init hello_params_early(void)
{
	hello_device_level_at_core = hello_device_level;
	return 0;
}
core_initcall(hello_params_early);

/*
 * What the kernel's self-test puts in the directory the "firmware" run is
 * pointed at (firmware_class.path=<dir>; kernel/src/linuxdrv.rs): slx-test.fw,
 * HELLO_FW_BYTES bytes of hello_fw_byte() -- several pages, the last not
 * whole -- and slx-empty.fw, empty;
 * and it names a directory to read as a file (dir=<path>), one whose size
 * is not 0, so that it is refused for not being a regular file rather than
 * for being empty.
 */
#define HELLO_FW_BYTES 70001
#define HELLO_FW_PATH_ARG "firmware_class.path="
#define HELLO_DIR_ARG "dir="

/*
 * What the boot test stages as the module firmware/slateos/hello.fw, which
 * the kernel puts at /lib/firmware/slateos/hello.fw (scripts/boot-test.sh,
 * kernel/src/linuxdrv.rs): the "bootfw" run's, found by Linux's loader with
 * no firmware_class.path.
 */
#define HELLO_BOOT_FW "slateos/hello.fw"
#define HELLO_BOOT_FW_TEXT "SlateOS boot-medium firmware for linuxdrv/hello\n"

static u8 hello_fw_byte(size_t i)
{
	return (u8)(i * 7 + (i >> 8));
}

/* Whether `len` bytes at `data` are the test file's from byte `from`. */
static bool hello_fw_matches(const u8 *data, size_t from, size_t len)
{
	size_t i;

	for (i = 0; i < len; i++)
		if (data[i] != hello_fw_byte(from + i))
			return false;
	return true;
}

/* The layer's read of `name` in `dir` (lx_fs.c), as the loader reads one. */
static ssize_t hello_read(const char *dir, const char *name, loff_t offset, void **buf,
			  size_t buf_size, size_t *file_size)
{
	char *path = kasprintf(GFP_KERNEL, "%s/%s", dir, name);
	ssize_t got;

	if (!path)
		return -ENOMEM;
	got = kernel_read_file_from_path_initns(path, offset, buf, buf_size, file_size,
						READING_FIRMWARE);
	kfree(path);
	return got;
}

static DECLARE_COMPLETION(hello_fw_loaded);
static const struct firmware *hello_fw_async;

static void hello_fw_arrived(const struct firmware *fw, void *done)
{
	hello_fw_async = fw;
	complete(done);
}

/*
 * The "firmware" run: hello's parameters set as given; the layer's reading
 * at its edges, each answer Linux's kernel_read_file()'s; then Linux's own
 * firmware loader finding the test file in firmware_class.path -- whole,
 * shared when asked for twice, into a buffer of the driver's own, its tail
 * from an offset, and asynchronously -- and a missing one -ENOENT. The
 * "firmware-denied" run, without a File capability: nothing read, -EACCES
 * from every place the loader looks.
 */
static int __init hello_firmware_init(void)
{
	const char *dir = lx_arg(HELLO_FW_PATH_ARG), *a_dir = lx_arg(HELLO_DIR_ARG);
	const struct firmware *fw, *again;
	struct device *dev;
	size_t size = 0;
	u8 tail[100];
	void *data;

	if (!hello_mode("firmware") && !hello_mode("firmware-denied") && !hello_mode("bootfw"))
		return 0;
	dev = root_device_register("hello-fw");
	HELLO_CHECK(!IS_ERR(dev), "no device to load firmware for");

	if (hello_mode("bootfw")) {
		HELLO_CHECK(!dir, "the bootfw run was given a firmware_class.path=");
		HELLO_CHECK(request_firmware(&fw, HELLO_BOOT_FW, dev) == 0,
			    "the boot medium's firmware was not found where Linux looks by default");
		HELLO_CHECK(fw->size == sizeof(HELLO_BOOT_FW_TEXT) - 1 &&
				    !memcmp(fw->data, HELLO_BOOT_FW_TEXT, fw->size),
			    "the boot medium's firmware was not its bytes");
		release_firmware(fw);
		root_device_unregister(dev);
		pr_info("hello firmware: the boot medium's found in /lib/firmware\n");
		return 0;
	}
	HELLO_CHECK(dir, "no firmware_class.path= argument");

	if (hello_mode("firmware-denied")) {
		HELLO_CHECK(firmware_request_nowarn(&fw, "slx-test.fw", dev) == -EACCES && !fw,
			    "firmware was read without a capability to read files");
		root_device_unregister(dev);
		pr_info("hello firmware: none read without a capability to read files\n");
		return 0;
	}

	HELLO_CHECK(hello_answer == 42, "hello.answer=42 was not set");
	HELLO_CHECK(hello_motto && !strcmp(hello_motto, "two words"),
		    "\"hello.motto=two words\" was not set as one value");
	HELLO_CHECK(hello_device_level == 6 && hello_device_level_at_core == 0,
		    "a device-level parameter was not set between the core and device initcalls");

	data = NULL;
	HELLO_CHECK(hello_read(dir, "slx-test.fw", 0, &data, INT_MAX, NULL) == HELLO_FW_BYTES &&
		    hello_fw_matches(data, 0, HELLO_FW_BYTES),
		    "a whole file read was not its bytes");
	vfree(data);
	data = tail;
	HELLO_CHECK(hello_read(dir, "slx-test.fw", 1000, &data, sizeof(tail), &size) == sizeof(tail) &&
		    size == HELLO_FW_BYTES && hello_fw_matches(tail, 1000, sizeof(tail)),
		    "part of a file, from an offset into a buffer given, was not its bytes and size");
	data = NULL;
	HELLO_CHECK(hello_read(dir, "slx-test.fw", 0, &data, 4096, NULL) == -EFBIG && !data,
		    "a whole file larger than the buffer allowed was not -EFBIG");
	HELLO_CHECK(hello_read(dir, "slx-test.fw", 5, &data, INT_MAX, &size) == -EINVAL && !data,
		    "a read from an offset with no buffer was not -EINVAL");
	HELLO_CHECK(a_dir, "no dir= argument");
	HELLO_CHECK(kernel_read_file_from_path_initns(a_dir, 0, &data, INT_MAX, NULL,
						      READING_FIRMWARE) == -EINVAL && !data,
		    "a directory read as a file was not -EINVAL");
	HELLO_CHECK(hello_read(dir, "slx-empty.fw", 0, &data, INT_MAX, NULL) == -EINVAL && !data,
		    "an empty file was not -EINVAL");
	HELLO_CHECK(hello_read(dir, "slx-missing.fw", 0, &data, INT_MAX, NULL) == -ENOENT,
		    "a missing file was not -ENOENT");
	HELLO_CHECK(kernel_read_file_from_path_initns("", 0, &data, INT_MAX, NULL,
						      READING_FIRMWARE) == -EINVAL,
		    "an empty path was not -EINVAL");

	HELLO_CHECK(request_firmware(&fw, "slx-test.fw", dev) == 0, "the firmware was not loaded");
	HELLO_CHECK(fw->size == HELLO_FW_BYTES && hello_fw_matches(fw->data, 0, fw->size),
		    "the firmware loaded was not the file's bytes");
	HELLO_CHECK(request_firmware(&again, "slx-test.fw", dev) == 0 && again->data == fw->data,
		    "firmware asked for twice while held was not shared");
	release_firmware(again);
	release_firmware(fw);
	HELLO_CHECK(firmware_request_nowarn(&fw, "slx-missing.fw", dev) == -ENOENT && !fw,
		    "a missing firmware file was not -ENOENT");

	data = vmalloc(HELLO_FW_BYTES + 1000);
	HELLO_CHECK(data, "no memory");
	HELLO_CHECK(request_firmware_into_buf(&fw, "slx-test.fw", dev, data, HELLO_FW_BYTES + 1000) == 0 &&
		    fw->data == data && fw->size == HELLO_FW_BYTES &&
		    hello_fw_matches(data, 0, HELLO_FW_BYTES),
		    "firmware read into the driver's buffer was not the file's bytes");
	release_firmware(fw);
	HELLO_CHECK(request_partial_firmware_into_buf(&fw, "slx-test.fw", dev, data, 1000,
						      HELLO_FW_BYTES - 501) == 0 &&
		    fw->size == 501 && hello_fw_matches(data, HELLO_FW_BYTES - 501, 501),
		    "the firmware's tail, from an offset, was not its last 501 bytes");
	release_firmware(fw);
	vfree(data);

	HELLO_CHECK(request_firmware_nowait(THIS_MODULE, FW_ACTION_NOUEVENT, "slx-test.fw", dev,
					    GFP_KERNEL, &hello_fw_loaded, hello_fw_arrived) == 0,
		    "an asynchronous load was not started");
	HELLO_CHECK(wait_for_completion_timeout(&hello_fw_loaded, 5 * HZ),
		    "an asynchronous load never called back");
	HELLO_CHECK(hello_fw_async && hello_fw_async->size == HELLO_FW_BYTES &&
		    hello_fw_matches(hello_fw_async->data, 0, HELLO_FW_BYTES),
		    "an asynchronous load was not given the file's bytes");
	release_firmware(hello_fw_async);
	root_device_unregister(dev);

	pr_info("hello firmware: parameters set from the arguments, each at its level; files read at the edges of Linux's kernel_read_file(); Linux's loader finding firmware in firmware_class.path -- whole, shared, into a buffer, its tail from an offset, asynchronously -- and a missing one -ENOENT\n");
	return 0;
}
late_initcall(hello_firmware_init);
