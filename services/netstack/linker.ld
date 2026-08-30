/*
 * Linker script for the netstack userspace daemon.
 *
 * Same layout as init: single PT_LOAD segment at 0x4000000000.
 * See userspace/init/linker.ld for details.
 */

ENTRY(_start)

PHDRS
{
    load PT_LOAD FLAGS(7); /* PF_R | PF_W | PF_X */
}

SECTIONS
{
    . = 0x0000004000000000;

    .text ALIGN(16) : {
        *(.text._start)
        *(.text .text.*)
    } :load

    .rodata ALIGN(16) : {
        *(.rodata .rodata.*)
    } :load

    .data ALIGN(16) : {
        *(.data .data.*)
    } :load

    /*
     * ELF global constructors/destructors.  KEEP so --gc-sections can't drop
     * them; the PROVIDE_HIDDEN boundary symbols are what posix's crt
     * (run_constructors/run_destructors) walks.  Empty for pure-Rust
     * programs, which emit no entries.
     */
    .preinit_array ALIGN(8) : {
        PROVIDE_HIDDEN(__preinit_array_start = .);
        KEEP(*(.preinit_array))
        PROVIDE_HIDDEN(__preinit_array_end = .);
    } :load

    .init_array ALIGN(8) : {
        PROVIDE_HIDDEN(__init_array_start = .);
        KEEP(*(SORT_BY_INIT_PRIORITY(.init_array.*) .init_array))
        PROVIDE_HIDDEN(__init_array_end = .);
    } :load

    .fini_array ALIGN(8) : {
        PROVIDE_HIDDEN(__fini_array_start = .);
        KEEP(*(SORT_BY_INIT_PRIORITY(.fini_array.*) .fini_array))
        PROVIDE_HIDDEN(__fini_array_end = .);
    } :load

    .got ALIGN(8) : {
        *(.got .got.*)
    } :load

    .bss ALIGN(16) : {
        __bss_start = .;
        *(.bss .bss.*)
        *(COMMON)
        __bss_end = .;
    } :load

    /DISCARD/ : {
        *(.comment)
        *(.note*)
        *(.eh_frame*)
        *(.debug*)
        *(.dynamic)
        *(.dynsym)
        *(.dynstr)
        *(.gnu.hash)
        *(.rela*)
    }
}
