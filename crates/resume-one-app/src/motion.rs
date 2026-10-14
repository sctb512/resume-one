use std::time::{Duration, Instant};

pub(super) const MODE_DURATION: Duration = Duration::from_millis(240);
pub(super) const THEME_WIDTH: f32 = 206.0;

/// Progress is 0 for preview with themes and 1 for full-width JSON.
pub(super) struct ModeTransition {
    from: f32,
    to: f32,
    started: Instant,
    duration: Duration,
}

impl ModeTransition {
    pub fn new(from: f32, to: f32, now: Instant) -> Self {
        let distance = (to - from).abs();
        Self {
            from,
            to,
            started: now,
            duration: MODE_DURATION.mul_f32(distance),
        }
    }

    pub fn sample(&self, now: Instant) -> (f32, bool) {
        if self.duration.is_zero() {
            return (self.to, true);
        }
        let linear =
            now.saturating_duration_since(self.started).as_secs_f32() / self.duration.as_secs_f32();
        let t = linear.clamp(0.0, 1.0);
        let eased = t * t * (3.0 - 2.0 * t);
        (self.from + (self.to - self.from) * eased, linear >= 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reaches_endpoints_and_can_reverse_from_current_progress() {
        let now = Instant::now();
        let forward = ModeTransition::new(0.0, 1.0, now);
        assert_eq!(forward.sample(now), (0.0, false));
        let (mid, done) = forward.sample(now + MODE_DURATION / 2);
        assert!(!done);
        assert!((mid - 0.5).abs() < 0.001);
        assert_eq!(forward.sample(now + MODE_DURATION), (1.0, true));
        let reverse = ModeTransition::new(mid, 0.0, now);
        assert_eq!(reverse.sample(now).0, mid);
        assert_eq!(reverse.sample(now + MODE_DURATION), (0.0, true));
    }
}
