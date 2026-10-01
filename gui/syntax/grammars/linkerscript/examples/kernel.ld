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

        /* Build-time layout padding (see kernel/src/layout_pad.rs).
         *
         * Must come FIRST, before *(.text .text.*), because its entire purpose
         * is to shift the address of every other function — which is how we
         * measure how much of a benchmark's movement is caused by code
         * *placement* rather than code. Under QEMU's TCG a loop that straddles
         * a 4 KiB guest page costs ~1.7x per iteration, and relinking re-rolls
         * that for free, so "the diff made this 1.7x slower" and "the diff
         * moved this function across a page" are indistinguishable without a
         * deliberate perturbation to calibrate against.
         *
         * KEEP() because nothing references it symbolically; empty (0 bytes)
         * unless SLATEOS_TEXT_PAD is set at build time, which is the default
         * and is byte-for-byte the layout we have always built.
         *
         * `self_test_pad_is_first_in_text()` checks on every boot that this
         * really is first — a pad placed mid-.text would still build and boot
         * while silently under-reporting layout sensitivity.
         *
         * The bracketing symbols are how the kernel learns the pad's size. It
         * deliberately does NOT read the Rust constant: doing so let the
         * compiler fold `if PAD_BYTES == 0` away in the unpadded build, which
         * made that build's code ~256 bytes shorter and so differ from the
         * padded ones in code as well as in placement — the exact confound the
         * whole mechanism exists to isolate. */
        __layout_pad_start = .;
        KEEP(*(.text.slateos_layout_pad))
        __layout_pad_end = .;

        *(.text .text.*)
        __text_end = .;
    } :text

    /* Read-only data (constants, string literals, vtables). */
    .rodata : ALIGN(4K) {
        __rodata_start = .;
        *(.rodata .rodata.*)

        /* CPUID-gated code patching (see kernel/src/alternatives.rs).
         *
         * `.altinstructions` is the table of patch records; each names a site
         * in .text and a replacement byte string in `.altinstr_replacement`.
         * Both live in .rodata because they are only ever *read* by the
         * patcher — the bytes are copied into .text, never executed here, so
         * this segment stays non-executable.
         *
         * KEEP() is required: nothing references these sections symbolically,
         * so --gc-sections would otherwise discard the entire table and every
         * alternative would silently keep its default instruction. */
        . = ALIGN(8);
        __altinstructions_start = .;
        KEEP(*(.altinstructions))
        __altinstructions_end = .;
        KEEP(*(.altinstr_replacement))

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
