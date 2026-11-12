//! The CH572D itself: clocks, pins, the touch measurement, the RTC and
//! Sleep, the LSI, watchdog, resets and the chip's address.  Everything
//! here touches registers; the logic it serves lives in `touch`, `sleep`
//! and `protocol`.
//!
//! CH572D (U1) pins on the ptt-switch board (ptt-switch.kicad_sch):
//!
//! | Pin  | Net            | Use                                          |
//! |------|----------------|----------------------------------------------|
//! | PA0  | SWDIO, J4.2    | two-wire debug, left enabled: J4 is the only way in |
//! | PA1  | SWCLK, J4.3    | two-wire debug                               |
//! | PA2  | LED2 (D2, R5)  | status LED, active low: lit in the boot window |
//! | PA3  | LED1 (D1, R4)  | touch LED, active low: lit while touched     |
//! | PA7  | R6 (10k) from the touch wire | comparator input CMP_P1        |
//! | PA10 | R7 (1M) to the touch wire    | charges and discharges the wire |
//! | PA4-6, PA8, PA9, PA11 | not connected | inputs with pull-up       |

use crate::FSYS_HZ;
use crate::regs::*;
use core::ptr::{read_volatile, write_volatile};
use core::sync::atomic::{AtomicU8, Ordering::Relaxed};

const LED_STATUS: u32 = 1 << 2;
const LED_TOUCH: u32 = 1 << 3;
const SENSE: u32 = 1 << 7;
const CHARGE: u32 = 1 << 10;
const UNUSED: u32 = 1 << 4 | 1 << 5 | 1 << 6 | 1 << 8 | 1 << 9 | 1 << 11;

/// SysTick ticks (HCLK = Fsys) per microsecond.
pub const TICKS_PER_US: u32 = FSYS_HZ / 1_000_000;

#[inline(always)]
fn rd8(addr: usize) -> u8 {
    unsafe { read_volatile(addr as *const u8) }
}
#[inline(always)]
fn wr8(addr: usize, value: u8) {
    unsafe { write_volatile(addr as *mut u8, value) }
}
#[inline(always)]
fn rd16(addr: usize) -> u16 {
    unsafe { read_volatile(addr as *const u16) }
}
#[inline(always)]
fn wr16(addr: usize, value: u16) {
    unsafe { write_volatile(addr as *mut u16, value) }
}
#[inline(always)]
fn rd32(addr: usize) -> u32 {
    unsafe { read_volatile(addr as *const u32) }
}
#[inline(always)]
fn wr32(addr: usize, value: u32) {
    unsafe { write_volatile(addr as *mut u32, value) }
}

#[inline(always)]
fn nop() {
    unsafe { core::arch::asm!("nop", options(nomem, nostack, preserves_flags)) }
}

/// `fence.i`, which WCH's SAFEOPERATE uses around the safe-access
/// sequence.  Spelled as its encoding: the imc target lacks Zifencei.
#[inline(always)]
fn fence_i() {
    unsafe { core::arch::asm!(".word 0x0000100f", options(nostack, preserves_flags)) }
}

/// Mask machine interrupts; returns whether they were enabled.
#[inline(always)]
pub fn irq_save() -> bool {
    let mstatus: usize;
    unsafe { core::arch::asm!("csrrci {}, mstatus, 8", out(reg) mstatus, options(nostack)) };
    mstatus & 8 != 0
}

#[inline(always)]
pub fn irq_restore(was_enabled: bool) {
    if was_enabled {
        unsafe { core::arch::asm!("csrsi mstatus, 8", options(nostack)) };
    }
}

/// Unmask machine interrupts.  The interrupt controller has only the BLE
/// stack's two and the RTC enabled.
pub fn irq_enable() {
    irq_restore(true);
}

/// Open the safe-access window: registers the datasheet marks RWA or WA
/// accept writes for about 112 Fsys cycles after the 0x57, 0xA8 signature
/// (datasheet 4.2).  Same sequence as WCH's sys_safe_access_enable,
/// including its interrupt masking: an interrupt handler that opened and
/// closed a window of its own would close this one early.  Returns the
/// interrupt state for [`safe_off`].
#[inline(always)]
#[must_use]
fn safe_on() -> bool {
    let irq = irq_save();
    fence_i();
    wr8(R8_SAFE_ACCESS_SIG, SAFE_ACCESS_SIG1);
    wr8(R8_SAFE_ACCESS_SIG, SAFE_ACCESS_SIG2);
    fence_i();
    irq
}

#[inline(always)]
fn safe_off(irq: bool) {
    wr8(R8_SAFE_ACCESS_SIG, SAFE_ACCESS_SIG0);
    fence_i();
    irq_restore(irq);
}

/// One write to a safe-access register, in its own window.
#[inline(always)]
fn safe_wr8(addr: usize, value: u8) {
    let irq = safe_on();
    wr8(addr, value);
    safe_off(irq);
}

// ---- Clock, timing, watchdog, resets -------------------------------------

/// R8_CLK_SYS_CFG for Fsys = PLL / 6 = 100 MHz (WCH's
/// CLK_SOURCE_HSE_PLL_100MHz).
const CLK_PLL_DIV6: u8 = 0x40 | 6;

/// Switch Fsys to the PLL at [`FSYS_HZ`], following WCH's SetSysClock for
/// a PLL source.
///
/// Runs from RAM, as WCH runs SetSysClock (`__HIGH_CODE`): it rewrites
/// the flash timing (R8_FLASH_CFG, R8_FLASH_SCK), and code fetched from
/// flash must not run across that change.  Everything it calls is
/// inlined, so nothing executes from flash in between.
#[unsafe(link_section = ".highcode")]
#[inline(never)]
pub fn init_clocks() {
    // The crystal runs from reset; restart it as WCH does if something
    // (the ROM bootloader) stopped it.
    if rd8(R8_HFCK_PWR_CTRL) & RB_CLK_XT32M_PON == 0 {
        let tune = rd8(R8_XT32M_TUNE);
        let irq = safe_on();
        wr8(R8_XT32M_TUNE, tune | 0x03);
        wr8(R8_HFCK_PWR_CTRL, rd8(R8_HFCK_PWR_CTRL) | RB_CLK_XT32M_PON);
        safe_off(irq);
        let cfg = rd8(R8_CLK_SYS_CFG);
        let irq = safe_on();
        wr8(R8_CLK_SYS_CFG, cfg | 0xC0);
        safe_off(irq);
        for _ in 0..9 {
            nop();
        }
        let irq = safe_on();
        wr8(R8_CLK_SYS_CFG, cfg);
        wr8(R8_XT32M_TUNE, tune);
        safe_off(irq);
    }
    let pll_was_off = rd8(R8_HFCK_PWR_CTRL) & RB_CLK_PLL_PON == 0;
    let irq = safe_on();
    wr8(R8_HFCK_PWR_CTRL, rd8(R8_HFCK_PWR_CTRL) | RB_CLK_PLL_PON);
    wr8(R8_FLASH_CFG, 0x01);
    wr8(R8_FLASH_SCK, rd8(R8_FLASH_SCK) | 0x10);
    safe_off(irq);
    if pll_was_off {
        // WCH switches straight over because its PLL is on from reset;
        // give a PLL that was off ~2 ms (at the 6.4 MHz reset clock) to
        // lock.
        for _ in 0..4_000 {
            nop();
        }
    }
    let irq = safe_on();
    wr8(R8_SLP_POWER_CTRL, rd8(R8_SLP_POWER_CTRL) | 0x40);
    wr8(R8_CLK_SYS_CFG, CLK_PLL_DIV6);
    safe_off(irq);
}

/// Start SysTick counting HCLK, free-running upwards.  It stops in Sleep,
/// so it times only spans the chip stays awake for; the RTC keeps the
/// time across sleeps.
pub fn systick_start() {
    wr32(R32_STK_CTLR, STK_STCLK | STK_STE);
}

/// SysTick count: [`TICKS_PER_US`] per microsecond while awake.
#[inline(always)]
pub fn ticks() -> u32 {
    rd32(R32_STK_CNTL)
}

/// Busy-wait `us` microseconds.
#[inline(always)]
pub fn delay_us(us: u32) {
    delay_ticks(us * TICKS_PER_US);
}

#[inline(always)]
fn delay_ticks(span: u32) {
    let start = ticks();
    while ticks().wrapping_sub(start) < span {}
}

/// Watchdog timeout reset: R8_WDOG_COUNT steps every 131072 / Fsys
/// (1.3 ms) and resets the chip when it wraps, ~335 ms after a feed.  It
/// counts Fsys, so it stands still in Sleep, and every wake-up feeds it
/// (the main loop and each touch poll), so sleeps of any length are safe.
pub fn watchdog_start() {
    watchdog_feed();
    safe_wr8(R8_RST_WDOG_CTRL, RB_WDOG_RST_EN);
}

#[inline(always)]
pub fn watchdog_feed() {
    wr8(R8_WDOG_COUNT, 0);
}

pub fn software_reset() -> ! {
    let irq = safe_on();
    wr8(R8_RST_WDOG_CTRL, rd8(R8_RST_WDOG_CTRL) | RB_SOFTWARE_RESET);
    safe_off(irq);
    // The reset takes effect at once; this only satisfies the type.
    loop {
        nop();
    }
}

// ---- Pins and LEDs -------------------------------------------------------

/// Pin directions and idle levels for the table in the module doc.  PA0
/// and PA1 stay as the debug interface left them.
pub fn pins_init() {
    // Levels first, so outputs come up already idle: LEDs off (high), the
    // charge pin low, holding the wire discharged.
    wr32(R32_PA_SET, LED_STATUS | LED_TOUCH);
    wr32(R32_PA_CLR, CHARGE);
    // PD_DRV clear: 5 mA drive for outputs, no pull-down for inputs.  The
    // sense pin gets no pull-up either, which would charge the wire.
    let all = LED_STATUS | LED_TOUCH | CHARGE | SENSE | UNUSED;
    wr32(R32_PA_PD_DRV, rd32(R32_PA_PD_DRV) & !all);
    wr32(R32_PA_PU, (rd32(R32_PA_PU) | UNUSED) & !(SENSE | CHARGE));
    let dir = rd32(R32_PA_DIR) & !(SENSE | UNUSED);
    wr32(R32_PA_DIR, dir | LED_STATUS | LED_TOUCH | CHARGE);
}

/// D1: the touch LED.
pub fn led_touch(on: bool) {
    wr32(if on { R32_PA_CLR } else { R32_PA_SET }, LED_TOUCH);
}

/// D2: the status LED.
pub fn led_status(on: bool) {
    wr32(if on { R32_PA_CLR } else { R32_PA_SET }, LED_STATUS);
}

// ---- Touch measurement ---------------------------------------------------

/// Comparator reference 1111b, 800 mV, its highest step: the longest
/// charge time and so the most timer counts per pF.
const CMP_REF_800MV: u8 = 0x0F;
/// The comparator's settling time after it is enabled is not in the
/// datasheet; this is a guess to check on the bench.
const CMP_SETTLE_US: u32 = 10;
/// Capture mode 01b, edge to edge: each edge of the capture input pushes
/// the count since the previous edge, or since counting started, into
/// the FIFO (datasheet 8.3.3).
const CAP_ANY_EDGE: u8 = 0x40;
/// The capture timeout register at its largest (26 bits): the software
/// timeout ends a sample long before.
const TMR_CNT_END_MAX: u32 = (1 << 26) - 1;
/// FIFO data: bits 24:0 are the count, bit 25 the level it measured.
const CAPTURE_COUNT: u32 = (1 << 25) - 1;
/// Discharge between samples: at least this long...
const DISCHARGE_MIN_US: u32 = 20;
/// ...and this many charge times.  Charging to 0.8 V of 3.3 V takes
/// 0.28 RC, so 25 charge times are 7 RC: the wire is back under 0.1 % of
/// the supply and the next sample starts from the same place.
const DISCHARGE_FACTOR: u32 = 25;

/// One touch measurement: several charges of the wire.
pub struct Measurement {
    /// Sum of the charge times, in Fsys counts; None if a charge timed
    /// out (the wire never reached the reference).
    pub total: Option<u32>,
    /// A charge timed out while the comparator output was already high:
    /// the wire crossed the reference but the timer captured nothing.
    pub capture_fault: bool,
}

/// Charge the touch wire `samples` times through R7 and time each charge
/// from PA10 rising until the comparator (PA7 against 800 mV) switches,
/// with the timer capturing the comparator output (RB_CMP_CAP).  The
/// comparator draws ~30 uA (datasheet table 19-3), so it is on only for
/// the measurement.  `timeout_us` bounds each charge.
pub fn measure_touch(samples: u32, timeout_us: u32) -> Measurement {
    wr8(
        R8_CMP_CTRL_0,
        CMP_REF_800MV << 4 | RB_CMP_SW | RB_CMP_CAP | RB_CMP_EN,
    );
    delay_us(CMP_SETTLE_US);
    wr8(R8_TMR_CTRL_MOD, RB_TMR_ALL_CLEAR);
    wr32(R32_TMR_CNT_END, TMR_CNT_END_MAX);
    let mut out = Measurement {
        total: Some(0),
        capture_fault: false,
    };
    for _ in 0..samples {
        match charge_time(timeout_us * TICKS_PER_US) {
            Ok(count) => {
                out.total = out.total.map(|t| t + count);
                delay_ticks((DISCHARGE_MIN_US * TICKS_PER_US).max(count * DISCHARGE_FACTOR));
            }
            Err(fault) => {
                out.total = None;
                out.capture_fault = fault;
                // A timed-out charge may have left the wire anywhere up
                // to the supply: discharge it for several timeouts.
                delay_ticks(timeout_us * TICKS_PER_US * DISCHARGE_FACTOR / 4);
                break;
            }
        }
    }
    wr8(R8_TMR_CTRL_MOD, RB_TMR_ALL_CLEAR);
    wr8(R8_CMP_CTRL_0, 0);
    out
}

/// One charge of the discharged wire; Err(capture fault) on timeout.
fn charge_time(timeout_ticks: u32) -> Result<u32, bool> {
    wr8(R8_TMR_CTRL_MOD, RB_TMR_ALL_CLEAR);
    // Leaving RB_TMR_ALL_CLEAR starts the count; the charge starts with
    // the next write.  The two stores are a fixed few cycles apart, an
    // offset the baseline absorbs.
    wr8(
        R8_TMR_CTRL_MOD,
        CAP_ANY_EDGE | RB_TMR_COUNT_EN | RB_TMR_MODE_IN,
    );
    wr32(R32_PA_SET, CHARGE);
    let start = ticks();
    let result = loop {
        if rd8(R8_TMR_FIFO_COUNT) & 0x0F != 0 {
            break Ok(rd32(R32_TMR_FIFO) & CAPTURE_COUNT);
        }
        if ticks().wrapping_sub(start) > timeout_ticks {
            break Err(rd8(R8_CMP_CTRL_3) & RB_APR_OUT_CMP != 0);
        }
    };
    wr32(R32_PA_CLR, CHARGE);
    wr8(R8_TMR_CTRL_MOD, RB_TMR_ALL_CLEAR);
    result
}

// ---- RTC, LSI and Sleep --------------------------------------------------

/// RB_XT32M_C_LOAD: 110b, 18 pF of internal load on the 32 MHz crystal,
/// what WCH's evaluation board (no external load capacitors, like this
/// board's Y1) uses.  The crystal sets the radio's carrier; check its
/// offset on the built board.
const XT32M_LOAD: u8 = 0b110;

/// Clock and interrupt settings WCH's CH57x_BLEInit and HAL_TimeInit make
/// before the BLE stack starts: crystal load, LSI on (the RTC on it is
/// the stack's timebase), R32_MISC_CTRL bits 29:24 = 0x0E (undocumented;
/// WCH sets them in CH57x_BLEInit) and the link-layer interrupt's
/// pre-emption bit (WCH's PFIC_SetPriority(BLEL_IRQn, 0xF0), which writes
/// 0x80).
pub fn ble_clocks_init() {
    let tune = rd8(R8_XT32M_TUNE);
    safe_wr8(R8_XT32M_TUNE, (tune & !RB_XT32M_C_LOAD) | XT32M_LOAD << 4);
    let irq = safe_on();
    wr32(
        R32_MISC_CTRL,
        (rd32(R32_MISC_CTRL) & !(0x3F << 24)) | 0x0E << 24,
    );
    safe_off(irq);
    safe_wr8(R8_LSI_CONFIG, rd8(R8_LSI_CONFIG) | RB_CLK_LSI_PON);
    wr8(R32_PFIC_IPRIORX + BLEL_IRQN, 0x80);
}

/// RB_OSC_CNT_TOTAL = 110b: count Fsys over 1024 LSI periods, WCH's
/// Count_1024.
const LSI_CAL_TOTAL: u8 = 0b110;
const LSI_CAL_PERIODS: u64 = 1024;

/// The LSI frequency in Hz, measured against Fsys over 1024 LSI periods
/// (~32 ms), busy-waiting as WCH's RTC_InitClock does.  The overflow
/// count steps every 16384 Fsys cycles and wraps at 256; the wraps are
/// counted on the way.
pub fn lsi_measure() -> u32 {
    wr16(
        R16_OSC_CAL_CNT,
        rd16(R16_OSC_CAL_CNT) | RB_OSC_CAL_OV_CLR | RB_OSC_CAL_IF,
    );
    let irq = safe_on();
    wr8(R8_OSC_CAL_CTRL, rd8(R8_OSC_CAL_CTRL) | RB_CNT_CLR);
    wr8(R8_OSC_CAL_CTRL, LSI_CAL_TOTAL);
    wr8(R8_OSC_CAL_CTRL, rd8(R8_OSC_CAL_CTRL) | RB_OSC_CNT_EN);
    safe_off(irq);
    let mut last = 0u8;
    let mut wraps = 0u64;
    while rd16(R16_OSC_CAL_CNT) & RB_OSC_CAL_IF == 0 {
        let ov = rd8(R8_OSC_CAL_OV_CNT);
        if ov < last {
            wraps += 1;
        }
        last = ov;
        watchdog_feed();
    }
    let cycles = (rd16(R16_OSC_CAL_CNT) & RB_OSC_CAL_CNT) as u64
        + (rd8(R8_OSC_CAL_OV_CNT) as u64 + wraps * 256) * 16384;
    (FSYS_HZ as u64 * LSI_CAL_PERIODS / cycles.max(1)) as u32
}

/// The RTC count the BLE stack keeps time with: LSI periods, read until
/// two reads agree (WCH's SYS_GetClockValue).  In RAM like WCH's, since
/// the stack calls it from its interrupt handlers.
#[unsafe(link_section = ".highcode")]
#[inline(never)]
pub extern "C" fn rtc_count() -> u32 {
    loop {
        let count = rd32(R32_RTC_CNT_LSI);
        if count == rd32(R32_RTC_CNT_LSI) {
            return count;
        }
    }
}

/// Set by the RTC interrupt (`rtc_irq` in main.rs: it clears the RTC's
/// flags and stores 1 here), cleared by [`rtc_alarm`]: WCH's RTCTigFlag.
/// Set before the sleep starts, the alarm (or the stack's
/// [`rtc_pend`]) has already happened and the sleep is skipped.
#[unsafe(no_mangle)]
pub static RTC_FIRED: AtomicU8 = AtomicU8::new(0);

/// The RTC as the sleep timer, as WCH's HAL_TimeInit and HAL_SleepInit
/// leave it: trigger (alarm) mode, RTC events wake the chip, and its
/// interrupt enabled, which is what ends the WFI.
pub fn rtc_init() {
    safe_wr8(R8_SLP_WAKE_CTRL, rd8(R8_SLP_WAKE_CTRL) | RB_SLP_RTC_WAKE);
    safe_wr8(R8_RTC_MODE_CTRL, rd8(R8_RTC_MODE_CTRL) | RB_RTC_TRIG_EN);
    wr32(R32_PFIC_IENR1, 1 << RTC_IRQN);
}

/// Arm the RTC alarm for RTC count `alarm` (WCH's RTC_SetTignTime).
pub fn rtc_alarm(alarm: u32) {
    let irq = safe_on();
    wr32(R32_RTC_TRIG, alarm);
    safe_off(irq);
    RTC_FIRED.store(0, Relaxed);
}

pub fn rtc_fired() -> bool {
    RTC_FIRED.load(Relaxed) != 0
}

/// Pend the RTC interrupt: the BLE stack's way of cutting a sleep short
/// when an interrupt gives it work (WCH's SYS_SetPendingIRQ).  In RAM
/// like WCH's.
#[unsafe(link_section = ".highcode")]
#[inline(never)]
pub extern "C" fn rtc_pend() {
    wr32(R32_PFIC_IPSR1, 1 << RTC_IRQN);
}

/// R16_POWER_PLAN bits 12:9 are reserved with reset value 1000b, which
/// WCH's LowPower_Sleep writes back (as `2 << 11`).
const PWR_PLAN_RESERVED: u16 = 0x1000;
/// R8_CLK_SYS_CFG for PLL / 25 = 24 MHz (WCH's CLK_SOURCE_HSE_PLL_24MHz),
/// the clock LowPower_Sleep sleeps on and wakes into.
const CLK_PLL_DIV25: u8 = 0x40 | 25;
/// RB_XT32M_I_BIAS: 150 % while asleep, so the crystal restarts fast, and
/// 100 % when running (WCH's HSECFG_Current(HSE_RCur_100) after a sleep).
const XT_BIAS_150: u8 = 0b11;
const XT_BIAS_100: u8 = 0b01;
/// How long after waking before code runs from flash again (WCH's
/// DelayUs(40) at the end of LowPower_Sleep).
const WAKE_SETTLE_TICKS: u32 = 40 * TICKS_PER_US;

/// Sleep until the RTC alarm or another enabled interrupt: WCH's
/// LowPower_Sleep(RB_PWR_RAM12K | RB_PWR_EXTEND | RB_XT_PRE_EN), then
/// its HSECFG_Current(HSE_RCur_100).  Kept while asleep (datasheet table
/// 5-3, about 1.2 uA with the LSI): RAM, the core and its peripherals
/// (GPIO levels hold, so the touch wire stays discharged), and the radio
/// configuration.  Flash is powered down.
///
/// Runs from RAM, with every helper inlined: flash is off from the WFI
/// until the settle delay at the end.  Interrupts stay enabled, so the
/// RTC interrupt is taken (from the RAM vector table) on the way out of
/// the WFI.
#[unsafe(link_section = ".highcode")]
#[inline(never)]
pub fn sleep_now() {
    let clk = rd8(R8_CLK_SYS_CFG);
    let tune = rd8(R8_XT32M_TUNE);
    safe_wr8(R8_BAT_DET_CTRL, 0);
    safe_wr8(R8_XT32M_TUNE, (tune & !RB_XT32M_I_BIAS) | XT_BIAS_150);
    wr32(R32_PFIC_SCTLR, rd32(R32_PFIC_SCTLR) | SCTLR_SLEEPDEEP);
    let irq = safe_on();
    wr16(
        R16_POWER_PLAN,
        RB_PWR_PLAN_EN
            | RB_PWR_CORE
            | RB_PWR_RAM12K
            | RB_PWR_EXTEND
            | RB_XT_PRE_EN
            | PWR_PLAN_RESERVED,
    );
    safe_off(irq);
    safe_wr8(R8_CLK_SYS_CFG, CLK_PLL_DIV25);
    wr32(R32_PFIC_SCTLR, rd32(R32_PFIC_SCTLR) & !SCTLR_WFITOWFE);
    unsafe { core::arch::asm!("wfi", "nop", "nop", options(nostack)) };
    safe_wr8(R8_CLK_SYS_CFG, clk);
    let irq = safe_on();
    wr16(R16_POWER_PLAN, rd16(R16_POWER_PLAN) & !RB_PWR_PLAN_EN);
    wr16(R16_POWER_PLAN, rd16(R16_POWER_PLAN) & !RB_XT_PRE_EN);
    safe_off(irq);
    delay_ticks(WAKE_SETTLE_TICKS);
    let tune = rd8(R8_XT32M_TUNE);
    safe_wr8(R8_XT32M_TUNE, (tune & !RB_XT32M_I_BIAS) | XT_BIAS_100);
}

// ---- Interrupts ----------------------------------------------------------

unsafe extern "C" {
    /// The vector table in RAM (main.rs).
    static _vectors_ram: u32;
}

/// Interrupt entry as WCH's startup_CH572.S sets it up: hardware
/// register stacking and nesting on (INTSYSCR, CSR 0x804 = 0x3; the
/// pre-emption width stays 0, so nothing actually nests), one nesting
/// level (INESTCR, CSR 0xBC1 = 1), and mtvec in vectored mode with
/// absolute addresses, on the table in RAM.  The BLE library's interrupt
/// entry points and `rtc_irq` are a bare handler ending in `mret`: they
/// rely on the hardware stacking.
pub fn interrupts_init() {
    let table = &raw const _vectors_ram as usize;
    unsafe {
        core::arch::asm!(
            "csrw 0x804, {intsyscr}",
            "csrw 0xbc1, {inestcr}",
            "csrw mtvec, {mtvec}",
            intsyscr = in(reg) 0x3usize,
            inestcr = in(reg) 0x1usize,
            mtvec = in(reg) table | 0x3,
            options(nostack),
        )
    };
}

// ---- The chip's address ----------------------------------------------------

unsafe extern "C" {
    /// libISP572.a: reads 8 bytes of the info area through the flash
    /// interface into `buf`.  ISP572.h only offers it through the
    /// FLASH_EEPROM_CMD dispatcher, which pulls every flash routine into
    /// RAM; this is its CMD_GET_ROM_INFO branch, and [`read_mac`] ends
    /// with the dispatcher's epilogue.
    fn FLASH_CMD_GET_ROM_INFO(addr: u32, buf: *mut u8);
}

/// ISP572.h: where the info area holds the chip's MAC address.
const ROM_CFG_MAC_ADDR: u32 = 0x3F018;

/// The chip's factory MAC address, least significant byte first (as the
/// BLE stack takes it).  It is also the chip's unique ID: WCH's
/// GET_UNIQUE_ID (EVT CH57x_flash.c) on this chip returns these 6 bytes
/// and a 16-bit sum of them, which is the datasheet's "unique chip ID,
/// ID data plus its checksum, 8 bytes" (section 4.5).  Call with
/// interrupts off, as the dispatcher would make them.
pub fn read_mac() -> [u8; 6] {
    let mut info = [0u8; 8];
    unsafe { FLASH_CMD_GET_ROM_INFO(ROM_CFG_MAC_ADDR, info.as_mut_ptr()) };
    // The dispatcher's epilogue: R8_GLOB_ROM_CFG keeps only
    // RB_ROM_CODE_OFS, so flash access control and write enables go off.
    let irq = safe_on();
    wr8(R8_GLOB_ROM_CFG, rd8(R8_GLOB_ROM_CFG) & 0x10);
    safe_off(irq);
    info[..6].try_into().unwrap()
}

// ---- Stack use -----------------------------------------------------------

const STACK_PAINT: u32 = 0xA5A5_5A5A;

unsafe extern "C" {
    /// End of .bss (link.x): the stack grows down towards it.
    static _bss_end: u32;
}

/// Fill the unused stack below the caller with a pattern, so
/// [`stack_unused`] can later tell how deep the stack has reached.
#[inline(never)]
pub fn stack_paint() {
    let sp: usize;
    unsafe { core::arch::asm!("mv {}, sp", out(reg) sp, options(nomem, nostack)) };
    let mut addr = &raw const _bss_end as usize;
    while addr < sp - 16 {
        unsafe { write_volatile(addr as *mut u32, STACK_PAINT) };
        addr += 4;
    }
}

/// Bytes of stack never used since [`stack_paint`]: the margin left.
pub fn stack_unused() -> usize {
    let start = &raw const _bss_end as usize;
    let mut addr = start;
    while unsafe { read_volatile(addr as *const u32) } == STACK_PAINT {
        addr += 4;
    }
    addr - start
}
