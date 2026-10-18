// SPDX-License-Identifier: GPL-3.0-only
//! Hybrid logical clock: orders changes made on different devices even when
//! their wall clocks disagree slightly.

use std::cmp::Ordering;
use std::fmt;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::ids::DeviceId;

/// A hybrid logical timestamp. Total order: wall time, then counter, then device.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Hlc {
    /// Milliseconds since the Unix epoch (the highest wall time seen).
    pub wall_ms: u64,
    /// Disambiguates events within the same millisecond.
    pub counter: u32,
    /// Device that produced the timestamp; breaks remaining ties deterministically.
    pub device: DeviceId,
}

impl Ord for Hlc {
    fn cmp(&self, other: &Self) -> Ordering {
        (self.wall_ms, self.counter, self.device).cmp(&(other.wall_ms, other.counter, other.device))
    }
}

impl PartialOrd for Hlc {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl fmt::Display for Hlc {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:013}-{:08x}-{}",
            self.wall_ms, self.counter, self.device
        )
    }
}

/// Generates monotonically increasing timestamps for one device.
#[derive(Debug, Clone)]
pub struct HlcClock {
    device: DeviceId,
    last: Hlc,
}

impl HlcClock {
    /// Creates a clock for `device`.
    #[must_use]
    pub fn new(device: DeviceId) -> Self {
        Self {
            device,
            last: Hlc {
                wall_ms: 0,
                counter: 0,
                device,
            },
        }
    }

    /// Resumes a clock from the last timestamp persisted by this device.
    #[must_use]
    pub fn resume(device: DeviceId, last: Hlc) -> Self {
        Self { device, last }
    }

    /// Timestamp for a local event, using the system clock.
    pub fn tick(&mut self) -> Hlc {
        self.tick_at(system_millis())
    }

    /// Timestamp for a local event at an explicit wall time (testable).
    pub fn tick_at(&mut self, now_ms: u64) -> Hlc {
        if now_ms > self.last.wall_ms {
            self.last = Hlc {
                wall_ms: now_ms,
                counter: 0,
                device: self.device,
            };
        } else {
            self.last.counter = self.last.counter.saturating_add(1);
        }
        self.last
    }

    /// Merges a timestamp received from another device and returns the next local one.
    pub fn receive(&mut self, remote: Hlc) -> Hlc {
        self.receive_at(remote, system_millis())
    }

    /// As [`receive`](Self::receive) with an explicit wall time.
    pub fn receive_at(&mut self, remote: Hlc, now_ms: u64) -> Hlc {
        let wall = now_ms.max(self.last.wall_ms).max(remote.wall_ms);
        let counter = if wall == self.last.wall_ms && wall == remote.wall_ms {
            self.last.counter.max(remote.counter).saturating_add(1)
        } else if wall == self.last.wall_ms {
            self.last.counter.saturating_add(1)
        } else if wall == remote.wall_ms {
            remote.counter.saturating_add(1)
        } else {
            0
        };
        self.last = Hlc {
            wall_ms: wall,
            counter,
            device: self.device,
        };
        self.last
    }

    /// The most recent timestamp issued.
    #[must_use]
    pub fn last(&self) -> Hlc {
        self.last
    }
}

fn system_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ticks_increase_even_when_the_clock_stalls() {
        let mut c = HlcClock::new(DeviceId::new());
        let a = c.tick_at(100);
        let b = c.tick_at(100);
        let d = c.tick_at(90); // clock went backwards
        assert!(a < b && b < d);
    }

    #[test]
    fn ticks_reset_counter_when_time_advances() {
        let mut c = HlcClock::new(DeviceId::new());
        c.tick_at(100);
        c.tick_at(100);
        assert_eq!(c.tick_at(101).counter, 0);
    }

    #[test]
    fn receive_orders_after_remote() {
        let mut a = HlcClock::new(DeviceId::new());
        let mut b = HlcClock::new(DeviceId::new());
        let sent = a.tick_at(5_000);
        let after = b.receive_at(sent, 1_000); // b's clock is behind
        assert!(after > sent);
    }

    #[test]
    fn device_breaks_ties_deterministically() {
        let (d1, d2) = (DeviceId::new(), DeviceId::new());
        let a = HlcClock::new(d1).tick_at(10);
        let b = HlcClock::new(d2).tick_at(10);
        assert_ne!(a, b);
        assert_eq!(a < b, d1 < d2);
    }

    #[test]
    fn display_sorts_like_ord_for_same_device() {
        let mut c = HlcClock::new(DeviceId::new());
        let a = c.tick_at(9);
        let b = c.tick_at(10);
        assert!(a.to_string() < b.to_string());
    }

    #[test]
    fn serde_round_trip() {
        let h = HlcClock::new(DeviceId::new()).tick_at(42);
        let json = serde_json::to_string(&h).unwrap();
        assert_eq!(serde_json::from_str::<Hlc>(&json).unwrap(), h);
    }
}
