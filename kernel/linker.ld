/*
 * Kernel linker script for Limine boot protocol.
 *
 * The kernel is loaded in the higher half at 0xFFFFFFFF80000000.
 * Limine sets up paging with:
 *   - Identity map of the first 4 GiB (or more)
 *   - Higher Half Direct Map (HHDM) at a bootloader-chosen offset
 *   - Kernel mapped at its ELF load address
 *
 * Section layout:
 *   .requests  — Limine protocol request/response structures
 *   .text      — executable code
 *   .rodata    — read-only data
 *   .data      — initialized read-write data
 *   .bss       — zero-initialized read-write data
 */

ENTRY(kmain)

PHDRS
{
    requests PT_LOAD FLAGS((1 << 0) | (1 << 1));           /* RW  */
    text     PT_LOAD FLAGS((1 << 0) | (1 << 2));           /* RX  */
    rodata   PT_LOAD FLAGS((1 << 0));                       /* R   */
    data     PT_LOAD FLAGS((1 << 0) | (1 << 1));           /* RW  */
}

SECTIONS
{
    /* Higher half kernel base address. */
    . = 0xFFFFFFFF80000000;

    /* Limine request markers and request objects.
     * These must be contiguous and in the correct order for the bootloader
     * to discover them. */
    .requests : ALIGN(4K) {
        __requests_start = .;
        KEEP(*(.requests_start_marker))
        KEEP(*(.requests))
        KEEP(*(.requests_end_marker))
        __requests_end = .;
    } :requests

    /* Executable code. */
    .text : ALIGN(4K) {
        __text_start = .;
        *(.text .text.*)
        __text_end = .;
    } :text

    /* Read-only data (constants, string literals, vtables). */
    .rodata : ALIGN(4K) {
        __rodata_start = .;
        *(.rodata .rodata.*)
        __rodata_end = .;
    } :rodata

    /* Initialized read-write data. */
    .data : ALIGN(4K) {
        __data_start = .;
        *(.data .data.*)
        *(.got .got.*)
        __data_end = .;
    } :data

    /* Zero-initialized read-write data. */
    .bss : ALIGN(4K) {
        __bss_start = .;
        *(COMMON)
        *(.bss .bss.*)
        __bss_end = .;
    } :data

    /* Symbols for the kernel to know its own extent. */
    __kernel_end = .;

    /* Discard unneeded sections. */
    /DISCARD/ : {
        *(.eh_frame*)
        *(.note .note.*)
        *(.comment)
    }
}
