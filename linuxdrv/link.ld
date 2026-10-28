/*
 * SPDX-License-Identifier: MIT
 *
 * The linker script of a Linux driver process (design-decisions 1580).
 *
 * Linked at 4 MiB, below 2 GiB: Linux's code is compiled for the small code
 * model and reaches its data with 32-bit absolute relocations, which only
 * the low 2 GiB satisfy. One segment, read-write-execute, as every native
 * program has, and the SlateOS ABI note.
 *
 * The initcalls are kept in Linux's order -- its INIT_CALLS, level by level,
 * each level's "sync" variant after it -- between __initcall_start and
 * __initcall_end, where the layer's C half runs them. `jiffies` is
 * `jiffies_64` under another name, as Linux's own linker script makes it.
 * Every other section a driver brings must be named here: the link refuses
 * an orphan (`--orphan-handling=error`, scripts/linux-tree.sh), so a new
 * kind of section is a decision, not a guess.
 */

ENTRY(_start)

PHDRS
{
    load PT_LOAD FLAGS(7); /* PF_R | PF_W | PF_X */
    note PT_NOTE FLAGS(4); /* PF_R */
}

SECTIONS
{
    . = 0x400000;

    .text : {
        KEEP(*(.text.start))
        /* The driver's own code (scripts/linux-tree.sh marks its objects'
         * sections), first, so that the patterns below do not take it. */
        __driver_text_start = .;
        *(.text.lxdriver .init.text.lxdriver)
        __driver_text_end = .;
        *(.text .text.*)
        *(.init.text .init.text.* .exit.text .noinstr.text .ref.text)
        *(.sched.text .cpuidle.text .entry.text .head.text .spinlock.text)
        *(.softirqentry.text)
    } :load

    .rodata : ALIGN(64) {
        *(.rodata .rodata.*)
        *(.init.rodata .init.rodata.*)
    } :load

    /* Before the writable sections: after .bss it would make .bss take
     * room in the file. */
    .note.slateos : ALIGN(4) {
        KEEP(*(.note.slateos))
    } :load :note

    .initcalls : ALIGN(8) {
        __initcall_start = .;
        KEEP(*(.initcallearly.init))
        KEEP(*(.initcall0.init)) KEEP(*(.initcall0s.init))
        KEEP(*(.initcall1.init)) KEEP(*(.initcall1s.init))
        KEEP(*(.initcall2.init)) KEEP(*(.initcall2s.init))
        KEEP(*(.initcall3.init)) KEEP(*(.initcall3s.init))
        KEEP(*(.initcall4.init)) KEEP(*(.initcall4s.init))
        KEEP(*(.initcall5.init)) KEEP(*(.initcall5s.init))
        KEEP(*(.initcallrootfs.init))
        KEEP(*(.initcall6.init)) KEEP(*(.initcall6s.init))
        KEEP(*(.initcall7.init)) KEEP(*(.initcall7s.init))
        __initcall_end = .;
    } :load

    .data : ALIGN(64) {
        *(.data .data.*)
        *(.init.data .init.data.* .ref.data .exit.data)
    } :load

    /* The global offset table the linker makes for a reference through it;
     * filled at link time, a static program having no loader to fill it. */
    .got : ALIGN(8) {
        *(.got .got.plt)
    } :load

    .bss : ALIGN(64) {
        *(.bss .bss.* COMMON)
    } :load

    jiffies = jiffies_64;

    /DISCARD/ : {
        *(.comment) *(.note.GNU-stack) *(.note.gnu.*) *(.llvm_addrsig)
        *(.eh_frame .eh_frame_hdr) *(.discard .discard.*) *(.modinfo)
        *(.debug_*)
    }

    /* Not loaded: the symbol table, for a backtrace read off the file. */
    .symtab 0 : { *(.symtab) }
    .strtab 0 : { *(.strtab) }
    .shstrtab 0 : { *(.shstrtab) }
}
