/*
 * Linker script for userspace shell binary.
 *
 * Places the binary at 0x0000_0040_0000_0000 (256 GiB), which is well
 * within the user half of the address space (entries 0-255 of PML4).
 *
 * All sections are merged into a single PT_LOAD segment.  This is
 * necessary because the kernel's ELF loader maps at 16 KiB frame
 * granularity -- if separate PT_LOAD segments fall within the same
 * frame, the second mapping attempt would fail.
 *
 * The segment has RWX permissions (flags 7).  This is acceptable for
 * userspace programs until a more sophisticated loader is implemented
 * that handles split permissions properly.
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

    /* Keep the gdb pretty-printer script section as a non-allocated section
     * (no SHF_ALLOC, so it is never placed in a PT_LOAD and the kernel loader
     * ignores it).  In debug builds rustc emits a live relocation to
     * `__rustc_debug_gdb_scripts_section__`; if we let the blanket
     * `*(.debug*)` discard below swallow it, the link fails with
     * "relocation refers to a symbol in a discarded section".  Listing it
     * explicitly here (before /DISCARD/) claims the input section so the
     * discard rule no longer matches it.  Release builds emit nothing here. */
    .debug_gdb_scripts 0 (INFO) : {
        *(.debug_gdb_scripts)
    }

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
