use std::time::Duration;

/// How far from the position the app asked for its seek may land. mpv lands
/// an exact seek on the frame after the target; a seek of the user lands
/// wherever the user pointed, or on a keyframe.
const LANDING: Duration = Duration::from_millis(500);

/// Tells the seeks of the player's user from the ones the app asked for, by
/// where the player lands. The room's position plays no part in it, so a
/// seek that lands late is not taken for the user's.
#[derive(Debug, Default)]
pub struct Seeks {
    /// Where the app last asked the player to be.
    asked: Option<Duration>,
    /// The player began a seek that has not landed.
    began: bool,
}

impl Seeks {
    /// The app asked the player to seek to `to`.
    pub fn ask(&mut self, to: Duration) {
        self.asked = Some(to);
    }

    /// The player began a seek, the app's or its user's.
    pub fn began(&mut self) {
        self.began = true;
    }

    /// The player landed at `at` and has no further seek to do. Returns
    /// the position when it was its user who moved it there.
    pub fn landed(&mut self, at: Duration) -> Option<Duration> {
        let began = std::mem::take(&mut self.began);
        let asked = self.asked.take();
        let ours = asked.is_some_and(|to| to.abs_diff(at) <= LANDING);
        (began && !ours).then_some(at)
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
        seeks.ask(secs(61.3));
        seeks.began();
        assert_eq!(seeks.landed(secs(61.32)), None);
    }

    #[test]
    fn a_seek_nobody_asked_for_is_the_users() {
        let mut seeks = Seeks::default();
        seeks.began();
        assert_eq!(seeks.landed(secs(40.0)), Some(secs(40.0)));
    }

    #[test]
    fn the_last_of_the_apps_seeks_is_the_one_that_lands() {
        let mut seeks = Seeks::default();
        seeks.ask(secs(12.2));
        seeks.began();
        seeks.ask(secs(84.4));
        seeks.began();
        assert_eq!(seeks.landed(secs(84.4)), None);
    }

    #[test]
    fn a_user_seek_that_overtakes_the_apps_is_the_users() {
        let mut seeks = Seeks::default();
        seeks.ask(secs(15.5));
        seeks.began();
        seeks.began();
        assert_eq!(seeks.landed(secs(70.0)), Some(secs(70.0)));
    }

    #[test]
    fn a_seek_the_app_asked_for_does_not_cover_a_later_one() {
        let mut seeks = Seeks::default();
        seeks.ask(secs(15.5));
        seeks.began();
        assert_eq!(seeks.landed(secs(15.54)), None);
        seeks.began();
        assert_eq!(seeks.landed(secs(15.5)), Some(secs(15.5)));
    }

    #[test]
    fn a_restart_without_a_seek_reports_nothing() {
        let mut seeks = Seeks::default();
        assert_eq!(seeks.landed(secs(33.0)), None);
    }
}
