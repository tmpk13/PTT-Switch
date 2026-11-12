//! CH572D firmware for the ptt-switch: a push-to-talk switch whose only
//! input is a wire sensed capacitively.  The chip measures the wire's
//! charge time with its comparator and timer, decides touched or not, and
//! reports that over BLE to a phone app; between measurements it sleeps
//! on the RTC.  docs/BLE.md is the app's side of it.
//!
//! `hw` and `ble` touch registers and WCH's BLE library and only build
//! for the chip; everything else is plain logic, tested on the host
//! (`mise run fw:test`).

#![cfg_attr(not(test), no_std)]

#[cfg(target_arch = "riscv32")]
pub mod ble;
#[cfg(target_arch = "riscv32")]
pub mod hw;
pub mod protocol;
pub mod regs;
pub mod sleep;
pub mod touch;

/// System clock: the 600 MHz PLL divided by 6, WCH's
/// CLK_SOURCE_HSE_PLL_100MHz, which their BLE examples run at.  Touch
/// charge times are counted in its cycles.
pub const FSYS_HZ: u32 = 100_000_000;
