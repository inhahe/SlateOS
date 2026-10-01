/* Constructs the tree's own scripts do not use. */
OUTPUT_FORMAT("elf64-x86-64")
OUTPUT_ARCH(i386:x86-64)
ENTRY(_start)
INCLUDE common.ld

MEMORY
{
    rom (rx)  : ORIGIN = 0x08000000, LENGTH = 512K
    ram (rwx) : ORIGIN = 0x20000000, LENGTH = 128K
}

__stack_size = DEFINED(__stack_size) ? __stack_size : 0x1000;

SECTIONS
{
    .vectors : { KEEP(*(.isr_vector)) } > rom

    .text : {
        *(.text*)
        PROVIDE(__etext = .);
    } > rom AT > rom

    .data : ALIGN(8) {
        __data_start = .;
        *(.data .data.*)
        . = ALIGN(8);
        __data_end = .;
    } > ram AT > rom

    __data_load = LOADADDR(.data);
    __data_size = SIZEOF(.data);

    OVERLAY : NOCROSSREFS
    {
        .ov1 { *(.ov1) }
        .ov2 { *(.ov2) }
    } > ram

    /DISCARD/ : { *(.comment) *(.note*) }
}
