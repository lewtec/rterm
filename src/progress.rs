use crate::attach::CONNECTING_TITLE;
use crate::trace::{self, Event};

pub const MAX_CRUMBS: usize = 5;

pub type HeadlineCallback = Box<dyn FnMut(&str) + Send>;

pub struct ConnectProgress {
    is_visible: bool,
    headline: String,
    crumbs: Vec<String>,
    pub on_headline: Option<HeadlineCallback>,
}

impl Default for ConnectProgress {
    fn default() -> Self {
        Self::new()
    }
}

impl ConnectProgress {
    pub fn new() -> Self {
        Self {
            is_visible: false,
            headline: CONNECTING_TITLE.into(),
            crumbs: Vec::new(),
            on_headline: None,
        }
    }

    pub fn is_visible(&self) -> bool {
        self.is_visible
    }

    pub fn headline(&self) -> &str {
        &self.headline
    }

    pub fn crumbs(&self) -> &[String] {
        &self.crumbs
    }

    pub fn begin(&mut self, clear_crumbs: bool) {
        if clear_crumbs {
            self.crumbs.clear();
        }
        self.headline = CONNECTING_TITLE.into();
        self.is_visible = true;
        let headline = self.headline.clone();
        if let Some(callback) = self.on_headline.as_mut() {
            callback(&headline);
        }
    }

    pub fn ingest(&mut self, raw: &str) -> Option<Event> {
        if !self.is_visible {
            return None;
        }
        if let Some(crumb) = trace::crumb(raw) {
            self.crumbs.push(crumb);
            if self.crumbs.len() > MAX_CRUMBS {
                let extra = self.crumbs.len() - MAX_CRUMBS;
                self.crumbs.drain(0..extra);
            }
        }
        match trace::event(raw) {
            Some(Event::Progress(text)) => {
                if self.headline == text {
                    return None;
                }
                self.headline = text.clone();
                if let Some(callback) = self.on_headline.as_mut() {
                    callback(&text);
                }
                Some(Event::Progress(text))
            }
            Some(Event::RevealTerminal) => {
                self.is_visible = false;
                Some(Event::RevealTerminal)
            }
            None => None,
        }
    }

    pub fn finish(&mut self) {
        self.is_visible = false;
    }

    pub fn reset(&mut self) {
        self.crumbs.clear();
        self.headline = CONNECTING_TITLE.into();
        self.is_visible = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn begin_clears_crumbs_when_asked() {
        let mut progress = ConnectProgress::new();
        progress.begin(true);
        progress.ingest("debug1: Connecting to whiterun [10.0.0.2] port 22.");
        assert_eq!(progress.crumbs().len(), 1);
        progress.begin(false);
        assert_eq!(progress.crumbs().len(), 1);
        assert!(progress.is_visible());
        progress.begin(true);
        assert!(progress.crumbs().is_empty());
        assert_eq!(progress.headline(), CONNECTING_TITLE);
    }

    #[test]
    fn ingest_updates_headline_and_caps_crumbs() {
        let mut progress = ConnectProgress::new();
        let headlines = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let recorded = std::sync::Arc::clone(&headlines);
        progress.on_headline = Some(Box::new(move |text| {
            recorded.lock().unwrap().push(text.to_string());
        }));
        progress.begin(true);
        progress.ingest("debug1: Connecting to whiterun [10.0.0.2] port 22.");
        progress.ingest("debug1: Connection established.");
        progress.ingest("debug1: Authenticating to whiterun:22 as 'ada'");
        assert_eq!(progress.headline(), "Authenticating to whiterun:22");
        assert_eq!(progress.crumbs().len(), 3);
        assert!(
            headlines
                .lock()
                .unwrap()
                .iter()
                .any(|line| line == "Authenticating to whiterun:22")
        );
        for index in 0..MAX_CRUMBS {
            progress.ingest(&format!("debug1: extra crumb {index}"));
        }
        assert_eq!(progress.crumbs().len(), MAX_CRUMBS);
    }

    #[test]
    fn reveal_and_finish_hide_overlay() {
        let mut progress = ConnectProgress::new();
        progress.begin(true);
        progress.ingest("debug1: Next authentication method: password");
        assert!(!progress.is_visible());
        progress.begin(false);
        progress.finish();
        assert!(!progress.is_visible());
    }

    #[test]
    fn ingest_ignored_while_hidden() {
        let mut progress = ConnectProgress::new();
        progress.ingest("debug1: Connecting to whiterun [10.0.0.2] port 22.");
        assert!(progress.crumbs().is_empty());
        assert_eq!(progress.headline(), CONNECTING_TITLE);
    }

    #[test]
    fn reset_clears_and_hides() {
        let mut progress = ConnectProgress::new();
        progress.begin(true);
        progress.ingest("debug1: Connection established.");
        progress.reset();
        assert!(!progress.is_visible());
        assert!(progress.crumbs().is_empty());
        assert_eq!(progress.headline(), CONNECTING_TITLE);
    }
}
