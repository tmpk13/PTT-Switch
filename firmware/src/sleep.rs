//! When to sleep and when to wake, on the RTC's clock.
//!
//! The BLE stack keeps time with the RTC, which counts LSI periods
//! (about 32 kHz) and wraps to 0 at [`RTC_MAX_COUNT`], one day of 32768 Hz
//! counts.  When the stack has nothing to do it hands the sleep callback
//! the RTC count of its next event; the chip must be awake and running on
//! its crystal by then, so the RTC alarm is set [`wake_lead`] earlier.
//! This is WCH's CH57x_LowPower (EVT `BLE/HAL/SLEEP.c`) as arithmetic,
//! kept apart from the registers so the wrap-around can be tested.

/// Where the RTC count wraps (WCH's RTC_MAX_COUNT).
pub const RTC_MAX_COUNT: u32 = 0xA8C0_0000;

/// RTC counts from `from` forward to `to`, across the wrap if need be.
pub fn rtc_distance(from: u32, to: u32) -> u32 {
    if to >= from {
        to - from
    } else {
        to + (RTC_MAX_COUNT - from)
    }
}

/// The RTC count `ticks` before `at`, across the wrap if need be.
pub fn rtc_before(at: u32, ticks: u32) -> u32 {
    if at >= ticks {
        at - ticks
    } else {
        at + (RTC_MAX_COUNT - ticks)
    }
}

/// Microseconds to RTC counts at `lsi_hz`, rounded to nearest.
pub fn us_to_rtc(us: u32, lsi_hz: u32) -> u32 {
    ((us as u64 * lsi_hz as u64 + 500_000) / 1_000_000) as u32
}

/// RTC counts to microseconds at `lsi_hz`, rounded to nearest.
pub fn rtc_to_us(ticks: u32, lsi_hz: u32) -> u32 {
    ((ticks as u64 * 1_000_000 + lsi_hz as u64 / 2) / lsi_hz.max(1) as u64) as u32
}

/// How early to set the alarm before the stack's next event: the crystal
/// and the main LDO restart in this time.  WCH's GET_WakeUpLSIMaxTime: 45
/// RTC counts plus 200 us, and at least 1.6 ms (the datasheet gives the
/// crystal up to 3 ms to be stable, table 19-5, but WCH starts the
/// crystal early with RB_XT_PRE_EN and runs it at 150 % bias meanwhile).
pub fn wake_lead(lsi_hz: u32) -> u32 {
    let us = (rtc_to_us(45, lsi_hz) + 200).max(1_600);
    us_to_rtc(us, lsi_hz)
}

/// Shortest sleep worth taking, in RTC counts: 1 ms (WCH's
/// SLEEP_RTC_MIN_TIME).  Below it the wake-up costs more than it saves.
pub fn min_sleep(lsi_hz: u32) -> u32 {
    us_to_rtc(1_000, lsi_hz)
}

/// Longest sleep accepted, in RTC counts (WCH's SLEEP_RTC_MAX_TIME).  A
/// longer one means the alarm time has already passed and the distance
/// wrapped around the day.
pub const MAX_SLEEP: u32 = RTC_MAX_COUNT - 30_000_000;

#[derive(Debug, PartialEq, Eq)]
pub enum Plan {
    /// Set the alarm at this RTC count, then sleep.
    Sleep { alarm: u32 },
    /// The next event is too close (or already past): stay awake.
    Awake,
}

/// Decide the sleep for an event due at RTC count `due`, with the RTC at
/// `now`.
pub fn plan(now: u32, due: u32, lead: u32, min: u32) -> Plan {
    let alarm = rtc_before(due, lead);
    let span = rtc_distance(now, alarm);
    if span < min || span > MAX_SLEEP {
        Plan::Awake
    } else {
        Plan::Sleep { alarm }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LSI: u32 = 32_000;

    #[test]
    fn distance_and_before_cross_the_wrap() {
        assert_eq!(rtc_distance(100, 250), 150);
        assert_eq!(rtc_distance(RTC_MAX_COUNT - 10, 5), 15);
        assert_eq!(rtc_before(250, 100), 150);
        assert_eq!(rtc_before(5, 15), RTC_MAX_COUNT - 10);
        for (at, d) in [(0, 0), (7, 7), (3, 1000), (RTC_MAX_COUNT - 1, 2)] {
            assert_eq!(rtc_distance(rtc_before(at, d), at), d);
        }
    }

    #[test]
    fn lead_is_wchs_1_6_ms_at_32_khz() {
        // 45 counts = 1406 us, + 200 = 1606 us, above the 1.6 ms floor.
        assert_eq!(wake_lead(LSI), 51);
        assert_eq!(min_sleep(LSI), 32);
        // A fast LSI gives the floor: 45 counts at 42 kHz is 1071 us.
        assert_eq!(rtc_to_us(wake_lead(42_000), 42_000), 1_595);
    }

    #[test]
    fn sleeps_until_lead_before_the_event() {
        // 200 ms ahead at 32 kHz.
        let now = 1_000;
        let due = now + 6_400;
        assert_eq!(plan(now, due, 51, 32), Plan::Sleep { alarm: due - 51 });
    }

    #[test]
    fn sleeps_across_the_wrap() {
        let now = RTC_MAX_COUNT - 100;
        let due = 6_300;
        assert_eq!(plan(now, due, 51, 32), Plan::Sleep { alarm: due - 51 });
        // The alarm itself wraps backwards past 0.
        assert_eq!(
            plan(RTC_MAX_COUNT - 500, 20, 51, 32),
            Plan::Sleep {
                alarm: RTC_MAX_COUNT - 31
            }
        );
    }

    #[test]
    fn stays_awake_when_the_event_is_close_or_past() {
        // Alarm 20 counts away: under the 1 ms minimum.
        assert_eq!(plan(1_000, 1_071, 51, 32), Plan::Awake);
        // Due inside the lead: the alarm is already behind.
        assert_eq!(plan(1_000, 1_030, 51, 32), Plan::Awake);
        // Due in the past.
        assert_eq!(plan(1_000, 900, 51, 32), Plan::Awake);
    }
}
