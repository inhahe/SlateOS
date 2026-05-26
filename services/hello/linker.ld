/*
 * Linker script for the hello userspace binary.
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
