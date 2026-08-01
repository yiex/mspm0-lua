MEMORY
{
    MODULE_SLOT (RX) : ORIGIN = NATIVE_MODULE_LINK_ADDR, LENGTH = 0x00001000
}

SECTIONS
{
    .module_header : {
        KEEP (*(.module_header))
    } > MODULE_SLOT

    .text : {
        *(.text)
        *(.text.*)
    } > MODULE_SLOT

    .rodata : {
        *(.rodata)
        *(.rodata.*)
        . = ALIGN(4);
    } > MODULE_SLOT

    .forbidden_data (NOLOAD) : {
        *(.data*)
        *(.bss*)
        *(COMMON)
    } > MODULE_SLOT

    /DISCARD/ : {
        *(.ARM.exidx*)
        *(.ARM.extab*)
        *(.comment)
    }

    ASSERT(SIZEOF(.module_header) == 32, "native module header must be 32 bytes")
    ASSERT(SIZEOF(.forbidden_data) == 0,
        "native modules must not contain mutable global data")
}
