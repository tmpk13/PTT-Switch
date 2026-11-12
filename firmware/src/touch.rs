//! Touch detection from charge times.
//!
//! Each poll measures how long the touch wire takes to charge through
//! the 1 MOhm resistor up to the comparator's reference, in system clock
//! counts (`hw::measure_touch`).  A finger near or on the wire adds
//! capacitance and lengthens that time.  The detector keeps a baseline of
//! the untouched time, follows it slowly as the battery, temperature and
//! surroundings drift, and calls a touch when a reading stands far enough
//! above it for long enough.
//!
//! The baseline is kept in sixteenths of a count, so slow tracking does
//! not stall on integer rounding.

/// Fixed-point fraction bits of the baseline.
const FRAC: u32 = 4;

#[derive(Clone, Copy, Debug)]
pub struct Config {
    /// Readings averaged into the first baseline; the wire is taken to be
    /// untouched while they are taken.
    pub calibration_polls: u8,
    /// Consecutive readings above the threshold that make a touch.
    pub press_polls: u8,
    /// Consecutive readings below half the threshold that end one.
    pub release_polls: u8,
    /// The threshold is the baseline shifted right by this (4: 1/16, a
    /// 6.25 % longer charge time)...
    pub threshold_shift: u8,
    /// ...but never less than this many counts.
    pub min_delta: u32,
    /// How fast the baseline follows a reading above it while untouched:
    /// each reading moves it by 1/2^shift of the difference.
    pub rise_shift: u8,
    /// The same for readings below it, faster, since capacitance that went
    /// away (a hand withdrawn during calibration, a drying enclosure)
    /// should not hold the baseline up.
    pub fall_shift: u8,
    /// A touch held longer than this (ms) is taken for a changed
    /// surrounding (a wet enclosure, a moved wire, a hand resting on it)
    /// rather than a press: it is released and the baseline restarts from
    /// the current reading.  A stuck push-to-talk is worse than a long
    /// one cut short.
    pub max_touch_ms: u32,
}

/// Starting values, to be tuned on the built board with the diagnostics
/// characteristic.  The NOTES.md estimate for 10 pF of wire and pad is a
/// 2.8 us charge (280 counts at 100 MHz), each pF a finger adds 28 counts.
pub const DEFAULT_CONFIG: Config = Config {
    calibration_polls: 4,
    press_polls: 2,
    release_polls: 2,
    threshold_shift: 4,
    min_delta: 30,
    rise_shift: 6,
    fall_shift: 2,
    max_touch_ms: 120_000,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    Pressed,
    Released,
}

pub struct Detector {
    cfg: Config,
    /// Baseline in counts << FRAC; 0 until calibration has a reading.
    baseline: u32,
    calibration_left: u8,
    calibration_sum: u32,
    touched: bool,
    streak: u8,
    touched_at_ms: u32,
    last: u32,
}

impl Detector {
    pub const fn new(cfg: Config) -> Self {
        Self {
            cfg,
            baseline: 0,
            calibration_left: cfg.calibration_polls,
            calibration_sum: 0,
            touched: false,
            streak: 0,
            touched_at_ms: 0,
            last: 0,
        }
    }

    pub fn touched(&self) -> bool {
        self.touched
    }

    /// The untouched charge time, in counts (0 while calibrating).
    pub fn baseline(&self) -> u32 {
        self.baseline >> FRAC
    }

    /// The latest reading, in counts.
    pub fn last(&self) -> u32 {
        self.last
    }

    /// Counts above the baseline that make a touch.
    pub fn threshold(&self) -> u32 {
        (self.baseline() >> self.cfg.threshold_shift).max(self.cfg.min_delta)
    }

    /// Feed one reading (counts) taken at `now_ms`; returns the change of
    /// state, if any.  A failed measurement is not fed at all.
    pub fn update(&mut self, reading: u32, now_ms: u32) -> Option<Event> {
        self.last = reading;
        if self.calibration_left > 0 {
            self.calibration_sum += reading;
            self.calibration_left -= 1;
            if self.calibration_left == 0 {
                let n = self.cfg.calibration_polls.max(1) as u32;
                self.baseline = (self.calibration_sum << FRAC) / n;
            }
            return None;
        }
        let delta = reading.saturating_sub(self.baseline());
        let threshold = self.threshold();
        if !self.touched {
            if delta >= threshold {
                self.streak += 1;
                if self.streak >= self.cfg.press_polls {
                    self.touched = true;
                    self.streak = 0;
                    self.touched_at_ms = now_ms;
                    return Some(Event::Pressed);
                }
            } else {
                self.streak = 0;
                self.track(reading);
            }
            return None;
        }
        if now_ms.wrapping_sub(self.touched_at_ms) >= self.cfg.max_touch_ms {
            self.touched = false;
            self.streak = 0;
            self.baseline = reading << FRAC;
            return Some(Event::Released);
        }
        if delta < threshold / 2 {
            self.streak += 1;
            if self.streak >= self.cfg.release_polls {
                self.touched = false;
                self.streak = 0;
                return Some(Event::Released);
            }
        } else {
            self.streak = 0;
        }
        None
    }

    fn track(&mut self, reading: u32) {
        let target = reading << FRAC;
        if target > self.baseline {
            self.baseline += (target - self.baseline) >> self.cfg.rise_shift;
        } else {
            self.baseline -= (self.baseline - target) >> self.cfg.fall_shift;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: u32 = 280;

    fn calibrated() -> Detector {
        let mut d = Detector::new(DEFAULT_CONFIG);
        for r in [278, 282, 280, 280] {
            assert_eq!(d.update(r, 0), None);
        }
        assert_eq!(d.baseline(), BASE);
        d
    }

    #[test]
    fn calibration_averages_the_first_readings() {
        let d = calibrated();
        assert!(!d.touched());
        // 280 / 16 = 17 is under the floor of 30 counts.
        assert_eq!(d.threshold(), 30);
    }

    #[test]
    fn press_needs_consecutive_readings_over_the_threshold() {
        let mut d = calibrated();
        assert_eq!(d.update(BASE + 40, 100), None);
        assert_eq!(d.update(BASE + 2, 200), None);
        assert_eq!(d.update(BASE + 40, 300), None);
        assert_eq!(d.update(BASE + 40, 400), Some(Event::Pressed));
        assert!(d.touched());
    }

    #[test]
    fn release_has_hysteresis() {
        let mut d = calibrated();
        d.update(BASE + 60, 0);
        assert_eq!(d.update(BASE + 60, 50), Some(Event::Pressed));
        // Below the threshold but above half of it: still touched.
        for t in 0..5 {
            assert_eq!(d.update(BASE + 20, 100 + t * 50), None);
        }
        assert_eq!(d.update(BASE + 5, 400), None);
        assert_eq!(d.update(BASE + 5, 450), Some(Event::Released));
        assert!(!d.touched());
    }

    #[test]
    fn baseline_follows_slow_drift_but_not_a_touch() {
        let mut d = calibrated();
        // Slow upward drift, well under the threshold each step.
        let mut r = BASE;
        for i in 0..2_000 {
            if i % 20 == 0 {
                r += 1;
            }
            assert_eq!(d.update(r, i * 200), None, "reading {r}");
        }
        assert!(d.baseline() >= r - 10, "baseline {} for {r}", d.baseline());
        // A touch does not drag the baseline along.
        let before = d.baseline();
        d.update(r + 100, 500_000);
        d.update(r + 100, 500_200);
        d.update(r + 100, 500_400);
        assert!(d.touched());
        assert_eq!(d.baseline(), before);
    }

    #[test]
    fn baseline_falls_fast() {
        let mut d = calibrated();
        for _ in 0..12 {
            d.update(BASE - 60, 0);
        }
        assert!(d.baseline() <= BASE - 55, "{}", d.baseline());
    }

    #[test]
    fn threshold_scales_with_a_long_wire() {
        let mut d = Detector::new(DEFAULT_CONFIG);
        for _ in 0..4 {
            d.update(3_200, 0);
        }
        assert_eq!(d.threshold(), 200);
        // Just under the threshold: no touch, and the baseline creeps up
        // by 1/64 of the difference each time (about 3 counts).
        assert_eq!(d.update(3_390, 0), None);
        assert_eq!(d.update(3_390, 0), None);
        assert_eq!(d.baseline(), 3_205);
        assert_eq!(d.update(3_430, 0), None);
        assert_eq!(d.update(3_430, 0), Some(Event::Pressed));
    }

    #[test]
    fn a_stuck_touch_is_released_and_rebaselined() {
        let mut d = calibrated();
        d.update(BASE + 80, 1_000);
        assert_eq!(d.update(BASE + 80, 1_050), Some(Event::Pressed));
        assert_eq!(d.update(BASE + 80, 1_050 + 119_999), None);
        assert_eq!(d.update(BASE + 80, 1_050 + 120_000), Some(Event::Released));
        assert_eq!(d.baseline(), BASE + 80);
        // Still covered: no new press from the same reading.
        for t in 0..10 {
            assert_eq!(d.update(BASE + 80, 200_000 + t * 50), None);
        }
    }

    #[test]
    fn stuck_timer_survives_the_millisecond_wrap() {
        let mut d = calibrated();
        d.update(BASE + 80, u32::MAX - 100);
        assert_eq!(d.update(BASE + 80, u32::MAX - 50), Some(Event::Pressed));
        assert_eq!(d.update(BASE + 80, 1_000), None);
    }
}
