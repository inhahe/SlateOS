/* SPDX-License-Identifier: MIT */
/*
 * cpu_relax() for a driver process, in place of Linux's
 * <asm/vdso/processor.h>.
 *
 * SHADOWS arch/x86/include/asm/vdso/processor.h __ASM_VDSO_PROCESSOR_H
 *
 * Linux code waits for time to pass by spinning on jiffies with cpu_relax()
 * in the loop -- "while (time_before(jiffies, timeout)) cpu_relax();" --
 * while Linux's timer interrupt moves jiffies on. Nothing interrupts a
 * driver process's task (design-decisions 1578 rule 5), so here each turn
 * of such a loop brings jiffies up to the clock (slx_cpu_relax), and the
 * loop ends when Linux's would. A loop that spins without cpu_relax() still
 * sees jiffies stand still.
 *
 * Every name Linux's header defines is defined here, with its meaning.
 */
#ifndef __ASM_VDSO_PROCESSOR_H
#define __ASM_VDSO_PROCESSOR_H

/* PAUSE, as Linux's: what eases a busy wait. */
static __always_inline void native_pause(void)
{
	asm volatile("pause" ::: "memory");
}

/* jiffies brought up to the monotonic clock. Defined by the layer. */
void slx_cpu_relax(void);

static __always_inline void cpu_relax(void)
{
	native_pause();
	slx_cpu_relax();
}

struct getcpu_cache;

notrace long __vdso_getcpu(unsigned *cpu, unsigned *node, struct getcpu_cache *unused);

#endif /* __ASM_VDSO_PROCESSOR_H */
