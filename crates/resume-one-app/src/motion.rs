use std::time::{Duration, Instant};

pub(super) const MODE_DURATION: Duration = Duration::from_millis(360);
pub(super) const THEME_WIDTH: f32 = crate::chrome::THEME_WIDTH;

/// One timeline for both directions. Retargeting samples the currently visible
/// value instead of restarting at an endpoint; distance scales the remaining time.
#[derive(Clone, Debug, Default)]
pub(super) struct Visibility {
    from: f32,
    target: f32,
    started: Option<Instant>,
    duration: Duration,
}

impl Visibility {
    pub fn snap(&mut self, value: f32) {
        self.from = value;
        self.target = value;
        self.started = None;
    }

    #[cfg(test)]
    pub fn target(&self) -> f32 {
        self.target
    }

    pub fn sample(&self, now: Instant) -> (f32, bool) {
        let Some(started) = self.started else {
            return (self.target, true);
        };
        let elapsed = now.saturating_duration_since(started);
        if self.duration.is_zero() || elapsed >= self.duration {
            return (self.target, true);
        }
        let t = elapsed.as_secs_f32() / self.duration.as_secs_f32();
        // A quick response followed by a quiet landing; no overshoot.
        let eased = 1. - (1. - t).powi(3);
        (self.from + (self.target - self.from) * eased, false)
    }

    pub fn retarget(&mut self, target: f32, now: Instant, duration: Duration, immediate: bool) {
        if immediate {
            self.snap(target);
        } else if self.target != target {
            self.from = self.sample(now).0;
            self.target = target;
            self.started = Some(now);
            self.duration = duration.mul_f32((self.target - self.from).abs());
        }
    }
}

/// Read the platform preference, not an app-only approximation. Called on
/// renders so a preference change also takes effect without restarting.
pub(super) fn system_reduces_motion() -> bool {
    #[cfg(target_os = "macos")]
    {
        objc2_app_kit::NSWorkspace::sharedWorkspace().accessibilityDisplayShouldReduceMotion()
    }
    #[cfg(target_os = "windows")]
    {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            SPI_GETCLIENTAREAANIMATION, SystemParametersInfoW,
        };
        let mut enabled: i32 = 1;
        // SAFETY: the SPI query writes one BOOL to a valid, aligned local.
        let success = unsafe {
            SystemParametersInfoW(
                SPI_GETCLIENTAREAANIMATION,
                0,
                (&mut enabled as *mut i32).cast(),
                0,
            )
        };
        success != 0 && enabled == 0
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        false
    }
}

/// Progress is 0 for preview with themes and 1 for full-width JSON.
pub(super) struct ModeTransition {
    from: f32,
    to: f32,
    started: Instant,
    duration: Duration,
    handoff_start: f32,
}

impl ModeTransition {
    /// Keep the outgoing document intact during the physical rail movement.
    /// Both directions overlap the final 38% with the content handoff, so the
    /// new view arrives while the rail is still settling.
    pub fn handoff(&self, now: Instant) -> f32 {
        if self.duration.is_zero() {
            return 1.0;
        }
        let t =
            now.saturating_duration_since(self.started).as_secs_f32() / self.duration.as_secs_f32();
        let t = ((t - self.handoff_start) / (1.0 - self.handoff_start)).clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    }
    pub fn new(from: f32, to: f32, now: Instant) -> Self {
        Self::with_duration(from, to, now, MODE_DURATION)
    }

    /// With no docked theme rail there is no space to push. Keep the right
    /// edge stationary and make just a short view handoff instead.
    pub fn crossfade(from: f32, to: f32, now: Instant) -> Self {
        let mut transition = Self::with_duration(from, to, now, Duration::from_millis(80));
        transition.handoff_start = 0.0;
        transition
    }

    pub fn with_duration(from: f32, to: f32, now: Instant, duration: Duration) -> Self {
        let distance = (to - from).abs();
        Self {
            from,
            to,
            started: now,
            duration: duration.mul_f32(distance),
            handoff_start: 0.62,
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
    fn visibility_reverses_continuously_and_finishes_in_remaining_distance() {
        let now = Instant::now();
        let duration = Duration::from_millis(120);
        let mut visibility = Visibility::default();
        visibility.retarget(1., now, duration, false);
        let reverse_at = now + Duration::from_millis(30);
        let before = visibility.sample(reverse_at).0;
        assert!(before > 0. && before < 1.);
        visibility.retarget(0., reverse_at, duration, false);
        assert_eq!(before, visibility.sample(reverse_at).0);
        let back_at = reverse_at + Duration::from_millis(20);
        let faded = visibility.sample(back_at).0;
        assert!(faded < before);
        visibility.retarget(1., back_at, duration, false);
        assert_eq!(faded, visibility.sample(back_at).0);
        assert_eq!(visibility.sample(back_at + duration), (1., true));
        visibility.retarget(0., back_at + duration, duration, true);
        assert_eq!(visibility.sample(back_at + duration), (0., true));
    }

    #[test]
    fn repeated_visibility_retargets_never_jump_or_overshoot() {
        let mut now = Instant::now();
        let mut visibility = Visibility::default();
        for index in 0..40 {
            now += Duration::from_millis(11);
            let before = visibility.sample(now).0;
            visibility.retarget(
                if index % 2 == 0 { 1. } else { 0. },
                now,
                Duration::from_millis(100),
                false,
            );
            assert_eq!(before, visibility.sample(now).0);
            assert!((0. ..=1.).contains(&before));
        }
        assert_eq!(
            visibility.sample(now + Duration::from_millis(100)),
            (0., true)
        );
    }

    #[test]
    fn reaches_endpoints_and_can_reverse_from_current_progress() {
        let now = Instant::now();
        let forward = ModeTransition::new(0.0, 1.0, now);
        assert_eq!(forward.sample(now), (0.0, false));
        let (mid, done) = forward.sample(now + MODE_DURATION / 2);
        assert!(!done);
        assert!((mid - 0.5).abs() < 0.001);
        assert_eq!(forward.sample(now + MODE_DURATION), (1.0, true));
        assert_eq!(forward.handoff(now + MODE_DURATION.mul_f32(0.60)), 0.0);
        assert!(forward.handoff(now + MODE_DURATION.mul_f32(0.81)) > 0.49);
        assert_eq!(forward.handoff(now + MODE_DURATION), 1.0);
        let reverse = ModeTransition::new(mid, 0.0, now);
        assert_eq!(reverse.sample(now).0, mid);
        assert_eq!(reverse.sample(now + MODE_DURATION), (0.0, true));
    }

    #[test]
    fn spotlight_uses_short_duration_and_reverses_without_opacity_jump() {
        let now = Instant::now();
        let duration = Duration::from_millis(140);
        let transition = ModeTransition::with_duration(0.0, 1.0, now, duration);
        let middle = transition.sample(now + duration / 2).0;
        assert!((middle - 0.5).abs() < 0.001);
        let reverse = ModeTransition::with_duration(middle, 0.0, now, duration);
        assert_eq!(reverse.sample(now).0, middle);
        assert_eq!(reverse.sample(now + duration / 2), (0.0, true));
    }

    #[test]
    fn undocked_views_crossfade_throughout_the_short_handoff() {
        let now = Instant::now();
        let fade = ModeTransition::crossfade(0., 1., now);
        assert_eq!(fade.handoff(now), 0.);
        assert!((fade.handoff(now + Duration::from_millis(40)) - 0.5).abs() < 0.001);
        assert_eq!(fade.sample(now + Duration::from_millis(80)), (1., true));
    }
}
