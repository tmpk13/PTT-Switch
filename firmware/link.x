/* CH572 memory: 240 KB of user flash at 0 (the core starts executing at
   address 0) and 12 KB of SRAM (datasheet tables 2-2 and 2-4).  The 8 KB
   bootloader and 8 KB InfoFlash above 0x3C000 are not ours. */
MEMORY
{
    FLASH (rx)  : ORIGIN = 0x00000000, LENGTH = 240K
    RAM   (xrw) : ORIGIN = 0x20000000, LENGTH = 12K
}

ENTRY(_start)

/* RAM the stack is guaranteed after .data and .bss: TMOS and the BLE
   stack's calls, its interrupts on top, and the firmware's own frames,
   which are shallow. */
_stack_size = 2K;

SECTIONS
{
    .init ORIGIN(FLASH) :
    {
        KEEP(*(.init))
    } > FLASH

    .text : ALIGN(4)
    {
        *(.text .text.*)
        /* WCH's BLE library: its "fast" code (connection events, TMOS
           helpers) runs from flash.  WCH's own link script has no rule
           for the section either, which leaves it beside .text. */
        *(.highcode_fast .highcode_fast.*)
    } > FLASH

    .rodata : ALIGN(4)
    {
        *(.srodata .srodata.*)
        *(.rodata .rodata.*)
        . = ALIGN(4);
    } > FLASH

    /* Code that runs from RAM, copied there by the startup code.  The
       chip powers its flash down in Sleep (the power plan in
       hw::sleep_now leaves RB_PWR_XROM clear), so whatever runs around a
       sleep and right after the wake-up must not come from flash:

       - the interrupt vector table, first in RAM and so 1 KB aligned, as
         WCH's startup keeps it: the RTC interrupt that ends a sleep is
         taken before flash is back;
       - the firmware's sleep entry, RTC handler, clock setup (which
         retimes the flash) and main loop;
       - WCH's library: its interrupt handlers and TMOS_SystemProcess
         (.highcode.*, in RAM in WCH's script too), and .highcode_lp,
         which holds tmos_idle (the caller of the sleep callback) and the
         radio re-initialization after a wake-up.  WCH's script leaves
         .highcode_lp unplaced; RAM is the conservative home and there
         is room for it. */
    .highcode : ALIGN(4)
    {
        _highcode_start = .;
        KEEP(*(.vectors_ram))
        *(.highcode .highcode.*)
        *(.highcode_lp .highcode_lp.*)
        . = ALIGN(4);
        _highcode_end = .;
    } > RAM AT > FLASH
    _highcode_lma = LOADADDR(.highcode);
    ASSERT(_vectors_ram == ORIGIN(RAM), "the vector table must start RAM (1 KB alignment)")

    .data : ALIGN(4)
    {
        _data_start = .;
        __global_pointer$ = . + 0x800;
        *(.sdata .sdata.* .sdata2 .sdata2.*)
        *(.data .data.*)
        . = ALIGN(4);
        _data_end = .;
    } > RAM AT > FLASH
    _data_lma = LOADADDR(.data);

    .bss (NOLOAD) : ALIGN(4)
    {
        _bss_start = .;
        *(.sbss .sbss.* .bss .bss.*)
        . = ALIGN(4);
        _bss_end = .;
    } > RAM

    _stack_top = ORIGIN(RAM) + LENGTH(RAM);
    ASSERT(_stack_top - _bss_end >= _stack_size, "less than _stack_size of RAM left for the stack")

    .got (INFO) : { KEEP(*(.got .got.*)) }
    /DISCARD/ : { *(.eh_frame .eh_frame_hdr) }
}
