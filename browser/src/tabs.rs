use std::time::{Duration, Instant};

use blitz_traits::net::Url;

pub const KEEP_ALIVE: Duration = Duration::from_secs(30);

pub struct Tab<T> {
    pub id: u64,
    pub title: String,
    pub page: Option<T>,
    history: Vec<Url>,
    position: usize,
    last_selected: Instant,
}

impl<T> Tab<T> {
    pub fn new(id: u64, url: Url, now: Instant) -> Self {
        Self {
            id,
            title: String::new(),
            page: None,
            history: vec![url],
            position: 0,
            last_selected: now,
        }
    }

    pub fn url(&self) -> &Url {
        &self.history[self.position]
    }

    pub fn select(&mut self, now: Instant) {
        self.last_selected = now;
    }

    pub fn deadline(&self, active_id: u64) -> Option<Instant> {
        (self.id != active_id && self.page.is_some()).then(|| self.last_selected + KEEP_ALIVE)
    }

    pub fn unload_if_expired(&mut self, active_id: u64, now: Instant) -> bool {
        if self.deadline(active_id).is_some_and(|due| now >= due) {
            self.page = None;
            return true;
        }
        false
    }

    pub fn navigate(&mut self, url: Url) {
        if self.url() != &url {
            self.history.truncate(self.position + 1);
            self.history.push(url);
            if self.history.len() > 64 {
                self.history.remove(0);
            }
            self.position = self.history.len() - 1;
        }
        self.clear_page();
    }

    pub fn move_history(&mut self, offset: isize) -> bool {
        let Some(index) = self.position.checked_add_signed(offset) else {
            return false;
        };
        if index >= self.history.len() {
            return false;
        }
        self.position = index;
        self.clear_page();
        true
    }

    pub fn set_resolved_url(&mut self, url: Url) {
        self.history[self.position] = url;
    }

    fn clear_page(&mut self) {
        self.page = None;
        self.title.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_sources_expire_at_thirty_seconds_while_backgrounded() {
        let now = Instant::now();
        for url in ["file:///C:/page.html", "https://example.com/"] {
            let mut tab = Tab::new(1, Url::parse(url).unwrap(), now);
            let state = std::rc::Rc::new(());
            let observed = std::rc::Rc::downgrade(&state);
            tab.page = Some(state);
            tab.title = "Page".into();
            assert!(!tab.unload_if_expired(2, now + Duration::from_millis(29_999)));
            assert!(!tab.unload_if_expired(1, now + KEEP_ALIVE));
            assert!(tab.unload_if_expired(2, now + KEEP_ALIVE));
            assert!(observed.upgrade().is_none());
            assert_eq!(tab.url().as_str(), url);
            assert_eq!(tab.title, "Page");
        }
    }

    #[test]
    fn reselection_renews_the_deadline() {
        let now = Instant::now();
        let mut tab = Tab::new(1, Url::parse("https://example.com/").unwrap(), now);
        tab.page = Some(());
        tab.select(now + Duration::from_secs(20));
        assert!(!tab.unload_if_expired(2, now + KEEP_ALIVE));
        assert!(tab.unload_if_expired(2, now + Duration::from_secs(50)));
    }
}
