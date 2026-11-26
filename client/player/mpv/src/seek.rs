use std::time::{Duration, Instant};

/// How far from the position the app asked for its seek may land. mpv lands
/// an exact seek on the frame after the target; a seek of the user lands
/// wherever the user pointed, or on a keyframe.
const LANDING: Duration = Duration::from_millis(500);

/// Tells the seeks of the player's user from the ones the app asked for, by
/// where the player lands. The room's position plays no part in it, so a
/// seek that lands late is not taken for the user's.
#[derive(Debug, Default)]
pub struct Seeks {
    /// Where the app last asked the player to be, and when it asked.
    asked: Option<(Duration, Instant)>,
    /// The player began a seek that has not landed.
    began: bool,
    /// How long the last seek the app asked for took to land.
    landing: Duration,
}

impl Seeks {
    /// The app asked the player at `now` to seek to `to`.
    pub fn ask(&mut self, to: Duration, now: Instant) {
        self.asked = Some((to, now));
    }

    /// The player began a seek, the app's or its user's.
    pub fn began(&mut self) {
        self.began = true;
    }

    /// Whether the player is on its way to another position. Until it
    /// lands, where it is says nothing.
    pub fn pending(&self) -> bool {
        self.asked.is_some() || self.began
    }

    /// How far ahead of the room a player is to be asked to seek that
    /// plays at `speed`: a seek takes its time, and the room plays on
    /// meanwhile. The best guess at that time is what the last seek took.
    pub fn lead(&self, speed: f64) -> Duration {
        self.landing.mul_f64(speed)
    }

    /// The player landed at `at` and has no further seek to do. Returns
    /// the position when it was its user who moved it there.
    pub fn landed(&mut self, at: Duration, now: Instant) -> Option<Duration> {
        let began = std::mem::take(&mut self.began);
        let asked = self.asked.take();
        let ours = asked.filter(|(to, _)| to.abs_diff(at) <= LANDING);
        if let Some((_, since)) = ours {
            self.landing = now.duration_since(since);
        }
        (began && ours.is_none()).then_some(at)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn secs(secs: f64) -> Duration {
        Duration::from_secs_f64(secs)
    }

    #[test]
    fn a_seek_the_app_asked_for_is_not_the_users() {
        let mut seeks = Seeks::default();
        let now = Instant::now();
        seeks.ask(secs(61.3), now);
        seeks.began();
        assert_eq!(seeks.landed(secs(61.32), now), None);
    }

    #[test]
    fn a_seek_nobody_asked_for_is_the_users() {
        let mut seeks = Seeks::default();
        seeks.began();
        assert_eq!(seeks.landed(secs(40.0), Instant::now()), Some(secs(40.0)));
    }

    #[test]
    fn the_last_of_the_apps_seeks_is_the_one_that_lands() {
        let mut seeks = Seeks::default();
        let now = Instant::now();
        seeks.ask(secs(12.2), now);
        seeks.began();
        seeks.ask(secs(84.4), now);
        seeks.began();
        assert_eq!(seeks.landed(secs(84.4), now), None);
    }

    #[test]
    fn a_user_seek_that_overtakes_the_apps_is_the_users() {
        let mut seeks = Seeks::default();
        let now = Instant::now();
        seeks.ask(secs(15.5), now);
        seeks.began();
        seeks.began();
        assert_eq!(seeks.landed(secs(70.0), now), Some(secs(70.0)));
    }

    #[test]
    fn a_seek_the_app_asked_for_does_not_cover_a_later_one() {
        let mut seeks = Seeks::default();
        let now = Instant::now();
        seeks.ask(secs(15.5), now);
        seeks.began();
        assert_eq!(seeks.landed(secs(15.54), now), None);
        seeks.began();
        assert_eq!(seeks.landed(secs(15.5), now), Some(secs(15.5)));
    }

    #[test]
    fn a_restart_without_a_seek_reports_nothing() {
        let mut seeks = Seeks::default();
        assert_eq!(seeks.landed(secs(33.0), Instant::now()), None);
    }

    #[test]
    fn a_seek_is_pending_from_when_it_was_asked_or_began_until_it_lands() {
        let mut seeks = Seeks::default();
        let now = Instant::now();
        assert!(!seeks.pending());

        seeks.ask(secs(20.0), now);
        assert!(seeks.pending(), "the app's, before mpv began it");
        seeks.began();
        seeks.landed(secs(20.0), now);
        assert!(!seeks.pending());

        seeks.began();
        assert!(seeks.pending(), "the user's");
        seeks.landed(secs(50.0), now);
        assert!(!seeks.pending());
    }

    #[test]
    fn the_lead_is_what_the_room_plays_while_a_seek_like_the_last_lands() {
        let mut seeks = Seeks::default();
        let now = Instant::now();
        assert_eq!(seeks.lead(1.0), Duration::ZERO, "no seek landed yet");

        seeks.ask(secs(20.0), now);
        seeks.began();
        seeks.landed(secs(20.0), now + secs(0.3));
        assert_eq!(seeks.lead(1.0), secs(0.3));
        assert_eq!(seeks.lead(2.0), secs(0.6));

        // a seek of the user says nothing about how long the app's take
        seeks.began();
        seeks.landed(secs(50.0), now + secs(9.0));
        assert_eq!(seeks.lead(1.0), secs(0.3));
    }
}
