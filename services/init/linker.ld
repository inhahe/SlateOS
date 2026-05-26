/*
 * Linker script for userspace init process.
 *
 * Places the binary at 0x0000_0040_0000_0000 (256 GiB), which is well
 * within the user half of the address space (entries 0-255 of PML4).
 *
 * All sections are merged into a single PT_LOAD segment.  This is
 * necessary because the kernel's ELF loader maps at 16 KiB frame
 * granularity -- if separate PT_LOAD segments fall within the same
 * frame, the second mapping attempt would fail.  For a small init
 * binary, everything easily fits in one frame.
 *
 * The segment has RWX permissions (flags 7).  This is acceptable for
 * the bootstrap init process; future binaries will use a more
 * sophisticated loader that handles split permissions properly.
 */

ENTRY(_start)

PHDRS
{
    load PT_LOAD FLAGS(7); /* PF_R | PF_W | PF_X */
}

SECTIONS
{
    /* Code starts at 256 GiB in user virtual address space. */
    . = 0x0000004000000000;

    .text ALIGN(16) : {
        *(.text._start)   /* Entry point first. */
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

    /* Discard unwanted sections. */
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
