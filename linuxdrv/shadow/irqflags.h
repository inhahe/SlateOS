/* SPDX-License-Identifier: MIT */
/*
 * Interrupt flags for a driver process, in place of Linux's
 * <asm/irqflags.h>.
 *
 * SHADOWS arch/x86/include/asm/irqflags.h _X86_IRQFLAGS_H_
 *
 * Linux turns interrupts off and on with `cli` and `sti`, which fault in a
 * program. In a driver process nothing interrupts Linux's code: its interrupt
 * handlers are run by the layer, on the process's one thread, only where
 * Linux code calls into the scheduler (design-decisions 1578 rule 5). So
 * "interrupts off" is a flag the layer keeps, `slx_irq_flags`, which holds
 * X86_EFLAGS_IF while handlers may run; saving, restoring and testing it work
 * as Linux's do, and the layer runs no handler while it is clear. One thread
 * reads and writes it, so plain accesses are all it needs.
 *
 * Every function Linux's header defines is defined here, with the same name
 * and meaning. Halting has nothing to wait for in a program and is a stop.
 */
#ifndef _X86_IRQFLAGS_H_
#define _X86_IRQFLAGS_H_

#include <asm/processor-flags.h>
#include <linux/types.h>

/* X86_EFLAGS_IF when the layer may run an interrupt handler, 0 when not. */
extern unsigned long slx_irq_flags;

/* Stops the driver: a halt with nothing to wake it. Defined by the layer. */
void slx_halted(void) __attribute__((noreturn));

static __always_inline unsigned long native_save_fl(void)
{
	return slx_irq_flags;
}

static __always_inline void native_irq_disable(void)
{
	slx_irq_flags = 0;
}

static __always_inline void native_irq_enable(void)
{
	slx_irq_flags = X86_EFLAGS_IF;
}

static __always_inline void native_safe_halt(void)
{
	slx_halted();
}

static __always_inline void native_halt(void)
{
	slx_halted();
}

static __always_inline int native_irqs_disabled_flags(unsigned long flags)
{
	return !(flags & X86_EFLAGS_IF);
}

static __always_inline unsigned long native_local_irq_save(void)
{
	unsigned long flags = native_save_fl();

	native_irq_disable();
	return flags;
}

static __always_inline void native_local_irq_restore(unsigned long flags)
{
	if (!native_irqs_disabled_flags(flags))
		native_irq_enable();
}

static __always_inline void arch_safe_halt(void)
{
	native_safe_halt();
}

static __always_inline void halt(void)
{
	native_halt();
}

static __always_inline unsigned long arch_local_save_flags(void)
{
	return native_save_fl();
}

static __always_inline void arch_local_irq_disable(void)
{
	native_irq_disable();
}

static __always_inline void arch_local_irq_enable(void)
{
	native_irq_enable();
}

static __always_inline unsigned long arch_local_irq_save(void)
{
	return native_local_irq_save();
}

static __always_inline int arch_irqs_disabled_flags(unsigned long flags)
{
	return native_irqs_disabled_flags(flags);
}

static __always_inline int arch_irqs_disabled(void)
{
	return arch_irqs_disabled_flags(arch_local_save_flags());
}

static __always_inline void arch_local_irq_restore(unsigned long flags)
{
	native_local_irq_restore(flags);
}

#endif /* _X86_IRQFLAGS_H_ */
