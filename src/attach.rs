pub const CONNECTING_TITLE: &str = "Connecting…";
pub const CONNECT_TIMEOUT_SECONDS: u64 = 5;
pub const MAX_CONNECT_ATTEMPTS: u32 = 3;
pub const TIMED_OUT_MESSAGE: &str = "connection timed out";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TabState {
    Idle,
    Attaching,
    Live,
    Failed(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttachExit {
    Clean,
    Failed(String),
}

pub fn classify_exit(code: Option<i32>) -> AttachExit {
    match code {
        Some(0) => AttachExit::Clean,
        None => AttachExit::Failed("process died".into()),
        Some(code) => AttachExit::Failed(format!("exited ({code})")),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Phase {
    Idle,
    Attaching(String),
    Live,
    Failed(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaceAttach {
    phase: Phase,
    generation: u32,
    connect_attempt: u32,
}

impl Default for PlaceAttach {
    fn default() -> Self {
        Self {
            phase: Phase::Idle,
            generation: 0,
            connect_attempt: 0,
        }
    }
}

impl PlaceAttach {
    pub fn phase(&self) -> &Phase {
        &self.phase
    }

    pub fn generation(&self) -> u32 {
        self.generation
    }

    pub fn connect_attempt(&self) -> u32 {
        self.connect_attempt
    }

    pub fn shows_pane(&self) -> bool {
        matches!(self.phase, Phase::Attaching(_) | Phase::Live)
    }

    pub fn connect_headline(&self) -> Option<&str> {
        match &self.phase {
            Phase::Attaching(title) => Some(title),
            _ => None,
        }
    }

    pub fn tab_state(&self) -> TabState {
        match &self.phase {
            Phase::Idle => TabState::Idle,
            Phase::Attaching(_) => TabState::Attaching,
            Phase::Live => TabState::Live,
            Phase::Failed(message) => TabState::Failed(message.clone()),
        }
    }

    pub fn is_connecting(&self) -> bool {
        self.connect_headline().is_some_and(is_connecting_headline)
    }

    pub fn select(&mut self) {
        if self.phase == Phase::Idle {
            self.begin_connect(false);
        }
    }

    pub fn reconnect(&mut self) {
        self.begin_connect(true);
    }

    pub fn refresh(&mut self) {
        self.generation += 1;
    }

    pub fn note_progress(&mut self, headline: &str) {
        let Phase::Attaching(current) = &self.phase else {
            return;
        };
        if current == headline {
            return;
        }
        if current != CONNECTING_TITLE && headline == CONNECTING_TITLE {
            return;
        }
        self.phase = Phase::Attaching(headline.to_string());
    }

    pub fn note_tty(&mut self) {
        if matches!(self.phase, Phase::Attaching(_)) {
            self.phase = Phase::Live;
        }
    }

    pub fn sleep(&mut self) {
        self.phase = Phase::Idle;
    }

    pub fn fail_connect_timeout(&mut self) {
        if self.is_connecting() {
            self.phase = Phase::Failed(TIMED_OUT_MESSAGE.into());
        }
    }

    pub fn set_connect_attempt(&mut self, value: u32) {
        self.connect_attempt = value;
    }

    pub fn note_connect_timeout(&mut self) {
        if !self.is_connecting() {
            return;
        }
        if self.connect_attempt >= MAX_CONNECT_ATTEMPTS {
            self.fail_connect_timeout();
            return;
        }
        let next = self.connect_attempt + 1;
        self.reconnect();
        self.connect_attempt = next;
    }

    pub fn handle_exit(&mut self, code: Option<i32>, generation: Option<u32>) {
        if let Some(generation) = generation
            && generation != self.generation
        {
            return;
        }
        match self.phase {
            Phase::Attaching(_) | Phase::Live => match classify_exit(code) {
                AttachExit::Clean => self.phase = Phase::Idle,
                AttachExit::Failed(message) => self.phase = Phase::Failed(message),
            },
            Phase::Idle | Phase::Failed(_) => {}
        }
    }

    fn begin_connect(&mut self, bump_generation: bool) {
        if bump_generation {
            self.refresh();
        }
        self.connect_attempt = 1;
        self.phase = Phase::Attaching(CONNECTING_TITLE.into());
    }
}

pub fn is_connecting_headline(title: &str) -> bool {
    title == CONNECTING_TITLE || title.starts_with("Connecting to ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_is_clean_detach() {
        assert_eq!(classify_exit(Some(0)), AttachExit::Clean);
    }

    #[test]
    fn nil_and_nonzero_are_failures() {
        assert_eq!(
            classify_exit(None),
            AttachExit::Failed("process died".into())
        );
        assert_eq!(
            classify_exit(Some(255)),
            AttachExit::Failed("exited (255)".into())
        );
    }

    #[test]
    fn idle_select_goes_attaching() {
        let mut attach = PlaceAttach::default();
        attach.select();
        assert_eq!(attach.phase(), &Phase::Attaching(CONNECTING_TITLE.into()));
        assert_eq!(attach.connect_headline(), Some(CONNECTING_TITLE));
        assert!(attach.shows_pane());
        assert!(attach.is_connecting());
        assert_eq!(attach.generation(), 0);
        assert_eq!(attach.connect_attempt(), 1);
    }

    #[test]
    fn tty_promotes_attaching_to_live() {
        let mut attach = PlaceAttach::default();
        attach.select();
        attach.note_tty();
        assert_eq!(attach.phase(), &Phase::Live);
        assert!(attach.shows_pane());
        attach.note_tty();
        assert_eq!(attach.phase(), &Phase::Live);
        assert_eq!(attach.generation(), 0);
    }

    #[test]
    fn live_select_does_not_bump_generation() {
        let mut attach = PlaceAttach::default();
        attach.select();
        attach.note_tty();
        attach.select();
        assert_eq!(attach.phase(), &Phase::Live);
        assert_eq!(attach.generation(), 0);
    }

    #[test]
    fn reconnect_from_idle_and_failed() {
        let mut attach = PlaceAttach::default();
        attach.reconnect();
        assert_eq!(attach.phase(), &Phase::Attaching(CONNECTING_TITLE.into()));
        assert_eq!(attach.generation(), 1);

        attach.handle_exit(Some(255), None);
        assert_eq!(attach.phase(), &Phase::Failed("exited (255)".into()));
        assert!(!attach.shows_pane());

        attach.reconnect();
        assert_eq!(attach.phase(), &Phase::Attaching(CONNECTING_TITLE.into()));
        assert_eq!(attach.generation(), 2);
        assert!(attach.shows_pane());
    }

    #[test]
    fn progress_updates_attaching_title() {
        let mut attach = PlaceAttach::default();
        attach.select();
        attach.note_progress("Authenticating to whiterun");
        assert_eq!(
            attach.phase(),
            &Phase::Attaching("Authenticating to whiterun".into())
        );
        attach.note_progress("Authenticating to whiterun");
        attach.note_tty();
        attach.note_progress("Waiting for remote");
        assert_eq!(attach.phase(), &Phase::Live);
    }

    #[test]
    fn clean_exit_goes_idle() {
        let mut attach = PlaceAttach::default();
        attach.select();
        attach.handle_exit(Some(0), None);
        assert_eq!(attach.phase(), &Phase::Idle);
        assert!(!attach.shows_pane());
    }

    #[test]
    fn exit_from_attaching_fails() {
        let mut attach = PlaceAttach::default();
        attach.select();
        attach.handle_exit(Some(255), None);
        assert_eq!(attach.phase(), &Phase::Failed("exited (255)".into()));
        assert!(!attach.shows_pane());
    }

    #[test]
    fn failed_exits_do_not_show_pane() {
        let mut died = PlaceAttach::default();
        died.select();
        died.handle_exit(None, None);
        assert_eq!(died.phase(), &Phase::Failed("process died".into()));
        assert!(!died.shows_pane());

        let mut failed = PlaceAttach::default();
        failed.select();
        failed.handle_exit(Some(255), None);
        assert_eq!(failed.phase(), &Phase::Failed("exited (255)".into()));
        assert!(!failed.shows_pane());
    }

    #[test]
    fn sleep_keeps_generation() {
        let mut attach = PlaceAttach::default();
        attach.reconnect();
        assert_eq!(attach.generation(), 1);
        attach.sleep();
        assert_eq!(attach.phase(), &Phase::Idle);
        assert_eq!(attach.generation(), 1);

        attach.select();
        attach.handle_exit(Some(255), None);
        attach.sleep();
        assert_eq!(attach.phase(), &Phase::Idle);
        assert_eq!(attach.generation(), 1);
    }

    #[test]
    fn connecting_headline() {
        assert!(is_connecting_headline(CONNECTING_TITLE));
        assert!(is_connecting_headline("Connecting to whiterun"));
        assert!(!is_connecting_headline("Connected"));
        assert!(!is_connecting_headline("Authenticating to whiterun"));
        assert!(!is_connecting_headline("Offering key"));
    }

    #[test]
    fn connect_timeout_retries_then_gives_up() {
        let mut attach = PlaceAttach::default();
        attach.select();
        assert_eq!(attach.connect_attempt(), 1);

        attach.note_connect_timeout();
        assert_eq!(attach.phase(), &Phase::Attaching(CONNECTING_TITLE.into()));
        assert_eq!(attach.connect_attempt(), 2);
        assert_eq!(attach.generation(), 1);

        attach.note_progress("Connecting to whiterun");
        attach.note_connect_timeout();
        assert_eq!(attach.phase(), &Phase::Attaching(CONNECTING_TITLE.into()));
        assert_eq!(attach.connect_attempt(), 3);
        assert_eq!(attach.generation(), 2);

        attach.note_connect_timeout();
        assert_eq!(attach.phase(), &Phase::Failed(TIMED_OUT_MESSAGE.into()));
        assert_eq!(attach.generation(), 2);
        assert!(!attach.shows_pane());
        assert!(!attach.is_connecting());

        attach.note_connect_timeout();
        assert_eq!(attach.phase(), &Phase::Failed(TIMED_OUT_MESSAGE.into()));
        attach.handle_exit(Some(255), None);
        assert_eq!(attach.phase(), &Phase::Failed(TIMED_OUT_MESSAGE.into()));
    }

    #[test]
    fn progress_does_not_regress_to_connecting_title() {
        let mut attach = PlaceAttach::default();
        attach.select();
        attach.note_progress("Connecting to whiterun");
        attach.note_progress(CONNECTING_TITLE);
        assert_eq!(
            attach.phase(),
            &Phase::Attaching("Connecting to whiterun".into())
        );
    }

    #[test]
    fn connect_timeout_ignored_after_connecting() {
        let mut attach = PlaceAttach::default();
        attach.select();
        attach.note_progress("Connected");
        assert!(!attach.is_connecting());
        attach.note_connect_timeout();
        assert_eq!(attach.phase(), &Phase::Attaching("Connected".into()));
        assert_eq!(attach.generation(), 0);
        assert_eq!(attach.connect_attempt(), 1);

        attach.note_progress("Offering key");
        attach.note_connect_timeout();
        assert_eq!(attach.phase(), &Phase::Attaching("Offering key".into()));
    }

    #[test]
    fn stale_exit_does_not_clobber_retry() {
        let mut attach = PlaceAttach::default();
        attach.select();
        attach.note_connect_timeout();
        assert_eq!(attach.generation(), 1);
        attach.handle_exit(Some(255), Some(0));
        assert_eq!(attach.phase(), &Phase::Attaching(CONNECTING_TITLE.into()));
        attach.handle_exit(Some(255), Some(1));
        assert_eq!(attach.phase(), &Phase::Failed("exited (255)".into()));
    }

    #[test]
    fn refresh_remounts_without_resetting_attempts() {
        let mut attach = PlaceAttach::default();
        attach.select();
        attach.note_connect_timeout();
        assert_eq!(attach.connect_attempt(), 2);
        assert_eq!(attach.generation(), 1);
        attach.refresh();
        assert_eq!(attach.generation(), 2);
        assert_eq!(attach.connect_attempt(), 2);
        assert_eq!(attach.phase(), &Phase::Attaching(CONNECTING_TITLE.into()));
    }

    #[test]
    fn reconnect_resets_connect_attempts() {
        let mut attach = PlaceAttach::default();
        attach.select();
        attach.note_connect_timeout();
        attach.note_connect_timeout();
        attach.note_connect_timeout();
        assert_eq!(attach.phase(), &Phase::Failed(TIMED_OUT_MESSAGE.into()));
        attach.reconnect();
        assert_eq!(attach.connect_attempt(), 1);
        assert!(attach.is_connecting());
    }
}
