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
 * __initcall_end, where the layer's C half runs them; the module parameters'
 * table where it sets them. `jiffies` is
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
        /* Static calls' trampolines, each a jump to its default. Linux
         * patches one to another target only for a CPU feature (crc32's
         * PCLMULQDQ), which the layer reports none of (boot_cpu_data), so
         * these stay as compiled; read-only text could not be patched. */
        *(.static_call.text)
    } :load

    /* Bracketed as Linux's own: kfree_const() leaves alone what lies here. */
    .rodata : ALIGN(64) {
        __start_rodata = .;
        *(.rodata .rodata.*)
        *(.init.rodata .init.rodata.*)
        __end_rodata = .;
    } :load

    /* PCI quirks, bounded as Linux's own linker script bounds them. */
    .pci_fixup : ALIGN(8) {
        __start_pci_fixups_early = .;
        KEEP(*(.pci_fixup_early))
        __end_pci_fixups_early = .;
        __start_pci_fixups_header = .;
        KEEP(*(.pci_fixup_header))
        __end_pci_fixups_header = .;
        __start_pci_fixups_final = .;
        KEEP(*(.pci_fixup_final))
        __end_pci_fixups_final = .;
        __start_pci_fixups_enable = .;
        KEEP(*(.pci_fixup_enable))
        __end_pci_fixups_enable = .;
        __start_pci_fixups_resume = .;
        KEEP(*(.pci_fixup_resume))
        __end_pci_fixups_resume = .;
        __start_pci_fixups_resume_early = .;
        KEEP(*(.pci_fixup_resume_early))
        __end_pci_fixups_resume_early = .;
        __start_pci_fixups_suspend = .;
        KEEP(*(.pci_fixup_suspend))
        __end_pci_fixups_suspend = .;
        __start_pci_fixups_suspend_late = .;
        KEEP(*(.pci_fixup_suspend_late))
        __end_pci_fixups_suspend_late = .;
    } :load

    /* Before the writable sections: after .bss it would make .bss take
     * room in the file. */
    .note.slateos : ALIGN(4) {
        KEEP(*(.note.slateos))
    } :load :note

    /* Each level bounded, as Linux's INIT_CALLS_LEVEL bounds it: the C half
     * sets a level's module parameters just before its initcalls run. The
     * rootfs level is level 5's tail, as in Linux. */
    .initcalls : ALIGN(8) {
        __initcall_start = .;
        KEEP(*(.initcallearly.init))
        __initcall0_start = .;
        KEEP(*(.initcall0.init)) KEEP(*(.initcall0s.init))
        __initcall1_start = .;
        KEEP(*(.initcall1.init)) KEEP(*(.initcall1s.init))
        __initcall2_start = .;
        KEEP(*(.initcall2.init)) KEEP(*(.initcall2s.init))
        __initcall3_start = .;
        KEEP(*(.initcall3.init)) KEEP(*(.initcall3s.init))
        __initcall4_start = .;
        KEEP(*(.initcall4.init)) KEEP(*(.initcall4s.init))
        __initcall5_start = .;
        KEEP(*(.initcall5.init)) KEEP(*(.initcall5s.init))
        KEEP(*(.initcallrootfs.init))
        __initcall6_start = .;
        KEEP(*(.initcall6.init)) KEEP(*(.initcall6s.init))
        __initcall7_start = .;
        KEEP(*(.initcall7.init)) KEEP(*(.initcall7s.init))
        __initcall_end = .;
    } :load

    /* Module parameters' tables, which the C half sets from the process's
     * arguments (kernel/params.c's parse_args), and the versions modules
     * declare, which Linux's own params.c lists; each bounded as Linux's
     * linker script bounds it. */
    __param : ALIGN(8) {
        __start___param = .;
        KEEP(*(__param))
        __stop___param = .;
    } :load

    __modver : ALIGN(8) {
        __start___modver = .;
        KEEP(*(__modver))
        __stop___modver = .;
    } :load

    /* Firmware built into the program (CONFIG_EXTRA_FIRMWARE, empty), which
     * the firmware loader looks through before any file. */
    .builtin_fw : ALIGN(8) {
        __start_builtin_fw = .;
        KEEP(*(.builtin_fw))
        __end_builtin_fw = .;
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
