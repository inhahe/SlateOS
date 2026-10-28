/* SPDX-License-Identifier: MIT */
/*
 * BUG() and WARN() for a driver process, in place of Linux's x86
 * <asm/bug.h>.
 *
 * SHADOWS arch/x86/include/asm/bug.h _ASM_X86_BUG_H
 *
 * Linux's own executes `ud2` for both and leaves the rest to the kernel's
 * invalid-opcode handler, which finds the site in __bug_table, prints it
 * and, for a warning, steps over the instruction. Nothing does that in a
 * program: the `ud2` would kill the driver at its first warning. Here a
 * warning is a call to the layer (slx_warn), which prints it -- once, for
 * WARN_ONCE -- and returns, and BUG() a call that prints and ends the
 * process (slx_bug). The rest -- WARN_ON(), WARN(), BUG_ON() and the flag
 * values -- is Linux's own <asm-generic/bug.h>, built on these two.
 *
 * Every name Linux's header defines that a driver might use is defined
 * here, with its meaning.
 */
#ifndef _ASM_X86_BUG_H
#define _ASM_X86_BUG_H

#include <linux/stringify.h>
#include <linux/instrumentation.h>
#include <linux/objtool.h>
#include <asm/asm.h>

/* The instructions Linux's own uses, which code may still name. */
#define ASM_UD2		__ASM_FORM(ud2)
#define INSN_UD2	0x0b0f
#define LEN_UD2		2
#define ASM_UDB		_ASM_BYTES(0xd6)
#define INSN_UDB	0xd6
#define LEN_UDB		1
#define INSN_ASOP		0x67
#define INSN_LOCK		0xf0
#define OPCODE_ESCAPE		0x0f
#define SECOND_BYTE_OPCODE_UD1	0xb9
#define SECOND_BYTE_OPCODE_UD2	0x0b
#define BUG_NONE		0xffff
#define BUG_UD2			0xfffe
#define BUG_UD1			0xfffd
#define BUG_UD1_UBSAN		0xfffc
#define BUG_UDB			0xffd6
#define BUG_LOCK		0xfff0

#ifndef __ASSEMBLER__
/* The layer's (linuxdrv/emul/lx_emul.c). */
void slx_bug(const char *file, int line) __attribute__((noreturn));
void slx_warn(const char *file, int line, unsigned int flags);
#endif

#define HAVE_ARCH_BUG
#define BUG()						\
do {							\
	slx_bug(__FILE__, __LINE__);			\
} while (0)

#define __WARN_FLAGS(flags)					\
do {								\
	slx_warn(__FILE__, __LINE__, BUGFLAG_WARNING | (flags));	\
} while (0)

#include <asm-generic/bug.h>

#endif /* _ASM_X86_BUG_H */
