//! CH572 register addresses and bit masks.
//!
//! Names and values match WCH's `CH572SFR.h` from the openwch/ch570 EVT
//! package (commit 1e34c94), so they can be checked against it name by
//! name; the datasheet (CH572DS1) chapter for each group is noted.  The
//! interrupt controller and SysTick come from the EVT's `core_riscv.h`.
//! Only what this firmware uses is listed.

// ---- System control and clocks (datasheet chapters 4 to 6) --------------

pub const R8_CLK_SYS_CFG: usize = 0x4000_1008;
pub const R8_HFCK_PWR_CTRL: usize = 0x4000_100A;
pub const RB_CLK_PLL_PON: u8 = 0x10;
pub const RB_CLK_XT32M_PON: u8 = 0x04;
pub const R8_SLP_WAKE_CTRL: usize = 0x4000_100E;
pub const RB_SLP_RTC_WAKE: u8 = 0x08;
pub const R8_SLP_POWER_CTRL: usize = 0x4000_100F;

/// The power plan the chip executes when it next enters Sleep (datasheet
/// 5.2, R16_POWER_PLAN).
pub const R16_POWER_PLAN: usize = 0x4000_1020;
pub const RB_PWR_PLAN_EN: u16 = 0x8000;
/// Start the 32 MHz crystal early on an RTC wake-up (needs the LSI).
pub const RB_XT_PRE_EN: u16 = 0x4000;
pub const RB_PWR_EXTEND: u16 = 0x08;
pub const RB_PWR_CORE: u16 = 0x04;
pub const RB_PWR_RAM12K: u16 = 0x02;

pub const R8_BAT_DET_CTRL: usize = 0x4000_1024;
pub const R8_LSI_CONFIG: usize = 0x4000_102F;
pub const RB_CLK_LSI_PON: u8 = 0x01;

pub const R8_RTC_FLAG_CTRL: usize = 0x4000_1030;
pub const RB_RTC_TRIG_CLR: u8 = 0x20;
pub const RB_RTC_TMR_CLR: u8 = 0x10;
pub const R8_RTC_MODE_CTRL: usize = 0x4000_1031;
pub const RB_RTC_TRIG_EN: u8 = 0x20;
pub const R32_RTC_TRIG: usize = 0x4000_1034;
pub const R32_RTC_CNT_LSI: usize = 0x4000_1038;
pub const R32_MISC_CTRL: usize = 0x4000_1048;
pub const R8_XT32M_TUNE: usize = 0x4000_104E;
pub const RB_XT32M_C_LOAD: u8 = 0x70;
pub const RB_XT32M_I_BIAS: u8 = 0x03;

/// LSI frequency measurement: Fsys cycles counted over a chosen number of
/// LSI periods (datasheet 6.3, "oscillator capture").
pub const R16_OSC_CAL_CNT: usize = 0x4000_1050;
pub const RB_OSC_CAL_IF: u16 = 0x8000;
pub const RB_OSC_CAL_OV_CLR: u16 = 0x4000;
pub const RB_OSC_CAL_CNT: u16 = 0x3FFF;
pub const R8_OSC_CAL_OV_CNT: usize = 0x4000_1052;
pub const R8_OSC_CAL_CTRL: usize = 0x4000_1053;
pub const RB_CNT_CLR: u8 = 0x80;
pub const RB_OSC_CNT_EN: u8 = 0x20;

pub const R8_SAFE_ACCESS_SIG: usize = 0x4000_1040;
pub const SAFE_ACCESS_SIG1: u8 = 0x57;
pub const SAFE_ACCESS_SIG2: u8 = 0xA8;
pub const SAFE_ACCESS_SIG0: u8 = 0x00;

pub const R8_WDOG_COUNT: usize = 0x4000_1043;
/// Read: R8_RESET_STATUS.  Write (safe access): R8_GLOB_ROM_CFG.
pub const R8_RESET_STATUS: usize = 0x4000_1044;
pub const RB_RESET_FLAG: u8 = 0x07;
pub const R8_GLOB_ROM_CFG: usize = 0x4000_1044;
pub const R8_RST_WDOG_CTRL: usize = 0x4000_1046;
pub const RB_WDOG_RST_EN: u8 = 0x02;
pub const RB_SOFTWARE_RESET: u8 = 0x01;

pub const R8_FLASH_SCK: usize = 0x4000_1805;
pub const R8_FLASH_CFG: usize = 0x4000_1807;

// ---- Comparator (datasheet chapter 14) ----------------------------------

/// Byte 0 of R32_CMP_CTRL: enable, capture routing, inputs, reference.
pub const R8_CMP_CTRL_0: usize = 0x4000_1054;
pub const RB_CMP_NREF_LEVEL: u8 = 0xF0;
/// Both bits set: positive input PA7 (CMP_P1), negative input the
/// internal reference (WCH's cmp_sw_3).
pub const RB_CMP_SW: u8 = 0x0C;
pub const RB_CMP_CAP: u8 = 0x02;
pub const RB_CMP_EN: u8 = 0x01;
/// Byte 3 of R32_CMP_CTRL: the comparator's live output.
pub const R8_CMP_CTRL_3: usize = 0x4000_1057;
pub const RB_APR_OUT_CMP: u8 = 0x02;

// ---- Pin functions and GPIO (datasheet chapter 7) -----------------------

pub const R32_PA_DIR: usize = 0x4000_10A0;
pub const R32_PA_PIN: usize = 0x4000_10A4;
pub const R32_PA_CLR: usize = 0x4000_10AC;
pub const R32_PA_PU: usize = 0x4000_10B0;
pub const R32_PA_PD_DRV: usize = 0x4000_10B4;
pub const R32_PA_SET: usize = 0x4000_10B8;

// ---- Timer (datasheet chapter 8) -----------------------------------------

pub const R8_TMR_CTRL_MOD: usize = 0x4000_2400;
pub const RB_TMR_CAP_EDGE: u8 = 0xC0;
pub const RB_TMR_COUNT_EN: u8 = 0x04;
pub const RB_TMR_ALL_CLEAR: u8 = 0x02;
pub const RB_TMR_MODE_IN: u8 = 0x01;
pub const R8_TMR_FIFO_COUNT: usize = 0x4000_2407;
pub const R32_TMR_CNT_END: usize = 0x4000_240C;
pub const R32_TMR_FIFO: usize = 0x4000_2410;

// ---- Interrupt controller (datasheet 3.4.1; core_riscv.h) ---------------

/// Interrupt enable set registers: a 1 enables that interrupt number (0
/// to 31, then 32 to 63).
pub const R32_PFIC_IENR1: usize = 0xE000_E100;
/// Interrupt pending set registers.
pub const R32_PFIC_IPSR1: usize = 0xE000_E200;
/// One priority byte per interrupt number from here; bit 7 is the
/// pre-emption bit, the rest read as zero.
pub const R32_PFIC_IPRIORX: usize = 0xE000_E400;
/// System control: SLEEPDEEP (bit 2) picks Halt/Sleep over Idle for WFI;
/// WFITOWFE (bit 3) turns WFI into WFE.
pub const R32_PFIC_SCTLR: usize = 0xE000_ED10;
pub const SCTLR_SLEEPDEEP: u32 = 1 << 2;
pub const SCTLR_WFITOWFE: u32 = 1 << 3;
/// Interrupt numbers (datasheet table 3-1).
pub const BLEL_IRQN: usize = 21;
pub const RTC_IRQN: usize = 28;

// ---- SysTick (datasheet section 3.4.4; core_riscv.h) --------------------

pub const R32_STK_CTLR: usize = 0xE000_F000;
pub const R32_STK_CNTL: usize = 0xE000_F008;
/// STK_CTLR: count on HCLK rather than HCLK/8.
pub const STK_STCLK: u32 = 0x04;
/// STK_CTLR: counter enable.
pub const STK_STE: u32 = 0x01;
