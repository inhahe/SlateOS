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

/*
 * Section names: `code-model=large` (.cargo/config.toml) makes LLVM put code in
 * `.ltext.*`, read-only data in `.lrodata.*` and large data in `.ldata.*` /
 * `.lbss.*`, so each pattern below names both spellings. Until 2026-09-25 only
 * the small-model names were listed: every code and read-only-data section was
 * an orphan that lld placed by heuristic, which worked because everything
 * shares one RWE segment, and stopped working the moment a second segment (the
 * SlateOS ABI note's PT_NOTE) existed for an orphan to be wrongly assigned to.
 * `--orphan-handling=error` (.cargo/config.toml) now fails the link instead.
 */

PHDRS
{
    load PT_LOAD FLAGS(7); /* PF_R | PF_W | PF_X */
    note PT_NOTE FLAGS(4); /* PF_R */
}

SECTIONS
{
    /* Code starts at 256 GiB in user virtual address space. */
    . = 0x0000004000000000;

    .text ALIGN(16) : {
        *(.text._start .ltext._start)   /* Entry point first. */
        *(.text .text.* .ltext .ltext.*)
    } :load

    .rodata ALIGN(16) : {
        *(.rodata .rodata.* .lrodata .lrodata.*)
    } :load

    /*
     * The SlateOS ABI note (design-decisions.md §33; the `global_asm!` in
     * src/main.rs): in the loaded image, and in a PT_NOTE of its own, which is
     * where the kernel looks. It must come before /DISCARD/, which would
     * otherwise take it with the rest of `.note*`.
     */
    .note.slateos ALIGN(4) : {
        KEEP(*(.note.slateos))
    } :load :note

    .data ALIGN(16) : {
        *(.data .data.* .ldata .ldata.*)
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
        *(.bss .bss.* .lbss .lbss.*)
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

    /* lld's own tables, named so --orphan-handling=error accepts them. */
    .symtab 0 : { *(.symtab) }
    .strtab 0 : { *(.strtab) }
    .shstrtab 0 : { *(.shstrtab) }
}
