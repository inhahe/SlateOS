// SPDX-License-Identifier: MIT
/*
 * hello -- the driver layer's first driver process, and no driver: a Linux
 * initcall that allocates, takes a spinlock with interrupts "off", formats a
 * line through Linux's own vsprintf and frees, run in a process of its own
 * as any Linux driver will be (roadmap.md 2.1, design-decisions 1580).
 *
 * The kernel's self-test (`linuxdrv::self_test`) runs it twice: plainly,
 * when it must exit 0 having printed its line; and with the argument
 * "stop", when it calls a function nothing defines -- which the build turns
 * into a generated stop -- and must exit SLX_EXIT_STOP, naming it.
 */
#include <linux/module.h>
#include <linux/slab.h>
#include <linux/printk.h>
#include <linux/spinlock.h>
#include <linux/irqflags.h>
#include <linux/string.h>

#include "../emul/slx.h"

/* Defined nowhere: the build makes it a generated stop. */
void lx_hello_never_defined(void);

static DEFINE_SPINLOCK(hello_lock);

static int __init hello_init(void)
{
	unsigned long flags;
	bool off;
	int *answer = kmalloc(sizeof(*answer), GFP_KERNEL | __GFP_ZERO);

	if (!answer)
		return -ENOMEM;
	if (*answer != 0) {
		kfree(answer);
		return -EINVAL;
	}
	spin_lock_irqsave(&hello_lock, flags);
	off = irqs_disabled();
	*answer = 42;
	spin_unlock_irqrestore(&hello_lock, flags);
	if (!off || irqs_disabled()) {
		kfree(answer);
		return -EFAULT;
	}
	pr_info("hello from Linux's own headers: %d, %s, %#x\n", *answer, "formatted by vsprintf", 0xbeef);
	kfree(answer);
	if (slx_argc() > 1 && strcmp(slx_argv(1), "stop") == 0)
		lx_hello_never_defined();
	return 0;
}
module_init(hello_init);
