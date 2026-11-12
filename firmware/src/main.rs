//! CH572D firmware entry: startup, the interrupt vector table, then the
//! BLE stack's loop.  See the library docs for what the firmware does.

#![no_std]
#![no_main]

use ptt_fw::{ble, hw};

// Reset entry at address 0, where the core starts.  Like WCH's
// startup_CH572.S: global and stack pointers, RAM code and .data copied
// from flash, .bss cleared, instruction prefetch on (corecfgr 0xBC0 =
// 0x25).  Machine mode throughout; interrupts stay off until the BLE
// stack starts.  Until then (hw::interrupts_init) mtvec points straight
// at the trap handler, which resets the chip.
//
// The vector table (datasheet table 3-1) lives in RAM, at its start, as
// in WCH's link script: the RTC interrupt that ends a sleep is taken
// while flash is still powered down.  Absolute-address entries (mtvec
// mode 3).  The BLE stack normally takes its two interrupts through
// fast-interrupt slots it sets up itself, which skip the table; every
// entry but those and the RTC's resets the chip.
//
// rtc_irq is WCH's RTC_IRQHandler: clear the RTC's flags, set
// hw::RTC_FIRED.  The core's hardware stacking (hw::interrupts_init)
// saves the registers it uses, so it ends in a bare mret.
core::arch::global_asm!(
    r#"
    .section .init, "ax"
    .global _start
_start:
    .option push
    .option norvc
    j _reset
    .option pop

    .section .vectors_ram, "aw"
    .global _vectors_ram
    .align 2
_vectors_ram:
    .word _trap                 /* 0 */
    .word 0                     /* 1 */
    .word _trap                 /* 2: NMI */
    .word _trap                 /* 3: faults and exceptions */
    .word 0                     /* 4 */
    .word _trap                 /* 5: ECALL-M */
    .word 0, 0                  /* 6, 7 */
    .word _trap                 /* 8: ECALL-U */
    .word _trap                 /* 9: breakpoint */
    .word 0, 0                  /* 10, 11 */
    .word _trap                 /* 12: SysTick */
    .word 0                     /* 13 */
    .word _trap                 /* 14: SWI */
    .word 0, 0                  /* 15, 16 */
    .word _trap                 /* 17: GPIO_A */
    .word 0                     /* 18 */
    .word _trap                 /* 19: SPI */
    .word BB_IRQLibFunction     /* 20: BLEB */
    .word LLE_IRQFIOFHandler    /* 21: BLEL */
    .word _trap                 /* 22: USB */
    .word 0                     /* 23 */
    .word _trap                 /* 24: TMR */
    .word 0, 0                  /* 25, 26 */
    .word _trap                 /* 27: UART */
    .word rtc_irq               /* 28: RTC */
    .word _trap                 /* 29: CMP */
    .word _trap                 /* 30: I2C */
    .word _trap                 /* 31: PWMx */
    .word 0                     /* 32 */
    .word _trap                 /* 33: KEYSCAN */
    .word _trap                 /* 34: ENCODER */
    .word _trap                 /* 35: WDOG_BAT */

    .section .highcode.rtc_irq, "ax"
    .align 2
rtc_irq:
    li t0, 0x40001030           /* R8_RTC_FLAG_CTRL */
    li t1, 0x30                 /* RB_RTC_TRIG_CLR | RB_RTC_TMR_CLR */
    sb t1, 0(t0)
    la t0, RTC_FIRED
    li t1, 1
    sb t1, 0(t0)
    mret

    .section .text._reset, "ax"
    .align 2
_reset:
    .option push
    .option norelax
    la gp, __global_pointer$
    .option pop
    la sp, _stack_top

    la a0, _highcode_lma
    la a1, _highcode_start
    la a2, _highcode_end
1:  bgeu a1, a2, 2f
    lw t0, 0(a0)
    sw t0, 0(a1)
    addi a0, a0, 4
    addi a1, a1, 4
    j 1b
2:
    la a0, _data_lma
    la a1, _data_start
    la a2, _data_end
3:  bgeu a1, a2, 4f
    lw t0, 0(a0)
    sw t0, 0(a1)
    addi a0, a0, 4
    addi a1, a1, 4
    j 3b
4:
    la a0, _bss_start
    la a1, _bss_end
5:  bgeu a0, a1, 6f
    sw zero, 0(a0)
    addi a0, a0, 4
    j 5b
6:
    .word 0x0000100f            /* fence.i: RAM now holds code */
    li t0, 0x25
    csrw 0xbc0, t0
    la t0, _trap
    csrw mtvec, t0
    call firmware_main
7:  j 7b

    .section .text._trap, "ax"
    .align 2
_trap:
    j trap_handler
"#
);

#[unsafe(no_mangle)]
extern "C" fn trap_handler() -> ! {
    hw::software_reset()
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    hw::software_reset()
}

const FAULT_BLINK_MS: u32 = 100;

/// Something keeps the firmware from running: blink D2 `code` times,
/// pause, repeat, keeping the watchdog fed.  1: the chip's address is
/// blank, 2: the BLE library is not the expected release, 3: the BLE
/// library refused to start.
fn fault(code: u32) -> ! {
    loop {
        for _ in 0..code {
            hw::led_status(true);
            wait_ms(FAULT_BLINK_MS);
            hw::led_status(false);
            wait_ms(FAULT_BLINK_MS * 2);
        }
        wait_ms(FAULT_BLINK_MS * 10);
    }
}

fn wait_ms(ms: u32) {
    for _ in 0..ms {
        hw::watchdog_feed();
        hw::delay_us(1_000);
    }
}

#[unsafe(no_mangle)]
extern "C" fn firmware_main() -> ! {
    hw::stack_paint();
    hw::init_clocks();
    hw::systick_start();
    hw::pins_init();
    hw::led_status(true);
    hw::watchdog_start();

    // The chip's unique ID: the factory MAC address, which also becomes
    // the BLE public address.  Blank means an unprogrammed info area.
    let mac = hw::read_mac();
    if mac == [0; 6] || mac == [0xFF; 6] {
        fault(1);
    }
    match ble::start(mac) {
        Ok(()) => ble::run(),
        Err(ble::StartError::WrongLibrary) => fault(2),
        Err(ble::StartError::LibInit(_)) => fault(3),
    }
}
