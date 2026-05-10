// rate_limiter.rs — sliding 1-second window counter with strike tracking

use std::time::Instant;

pub const POSITION_RATE_LIMIT: u32 = 120; // PlayerPosition packets per second
pub const EVENT_RATE_LIMIT:    u32 = 20;  // GameEvent / ChatMessages per second
pub const MAX_STRIKES:         u8  = 3;   // kicks after this many over-limit windows

pub struct RateLimiter {
    window_start: Instant,
    count:        u32,
    max_per_sec:  u32,
    pub strikes:  u8,
}

impl RateLimiter {
    pub fn new(max_per_sec: u32) -> Self {
        Self {
            window_start: Instant::now(),
            count:        0,
            max_per_sec,
            strikes:      0,
        }
    }

    // Returns true if the packet is within the limit and should be processed.
    // Resets the counter once per second and increments strikes on excess.
    pub fn allow(&mut self) -> bool {
        if self.window_start.elapsed().as_secs() >= 1 {
            self.window_start = Instant::now();
            self.count        = 0;
        }
        self.count += 1;
        if self.count > self.max_per_sec {
            self.strikes += 1;
            return false;
        }
        true
    }
}
