use std::{path::PathBuf, sync::Mutex, time::{Duration, Instant}};
use serde::Serialize;

#[derive(Default)]
pub struct Schedule {
    minutes: u32,
    deadline: Option<Instant>,
    busy: bool,
    waiting: bool,
    completed: u64,
    error: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub(crate) minutes: u32,
    pub(crate) remaining: u64,
    pub(crate) busy: bool,
    pub(crate) waiting: bool,
    pub(crate) completed: u64,
    pub(crate) error: Option<String>,
}

pub struct CacheSchedule {
    pub state: Mutex<Schedule>,
    pub path: PathBuf,
}

impl Schedule {
    pub fn configure(&mut self, minutes: u32, now: Instant) -> Result<(), String> {
        if minutes > 1440 { return Err("请输入 1～1440 分钟，或选择关闭".into()); }
        self.minutes = minutes;
        self.reset(now);
        Ok(())
    }

    fn reset(&mut self, now: Instant) {
        self.deadline = (self.minutes > 0 && !self.waiting && !self.busy)
            .then(|| now + Duration::from_secs(u64::from(self.minutes) * 60));
    }

    pub fn available(&mut self, available: bool, now: Instant) {
        if self.waiting == available {
            self.waiting = !available;
            self.reset(now);
        }
    }

    pub fn begin(&mut self, automatic: bool, now: Instant) -> bool {
        if self.busy || self.waiting || (automatic && self.deadline.is_none_or(|d| d > now)) {
            return false;
        }
        self.busy = true;
        self.deadline = None;
        self.error = None;
        true
    }

    pub fn finish(&mut self, result: Result<(), String>, now: Instant) {
        self.busy = false;
        self.error = result.err();
        self.completed += 1;
        self.reset(now);
    }

    pub fn status(&self) -> Status {
        Status {
            minutes: self.minutes,
            remaining: self.deadline.map(|d| d.saturating_duration_since(Instant::now()).as_secs_f64().ceil() as u64).unwrap_or(0),
            busy: self.busy, waiting: self.waiting, completed: self.completed, error: self.error.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schedule_waits_then_runs_once_and_restarts_after_failure() {
        let now = Instant::now();
        let mut s = Schedule::default();
        s.configure(15, now).unwrap();
        assert!(!s.begin(true, now));
        assert!(s.begin(true, now + Duration::from_secs(900)));
        assert!(!s.begin(false, now));
        let done = now + Duration::from_secs(905);
        s.finish(Err("failed".into()), done);
        assert_eq!(s.deadline, Some(done + Duration::from_secs(900)));
        assert_eq!(s.completed, 1);
        assert_eq!(s.error.as_deref(), Some("failed"));
    }

    #[test]
    fn manual_release_disable_and_waiting_preserve_intent() {
        let now = Instant::now();
        let mut s = Schedule::default();
        s.configure(30, now).unwrap();
        assert!(s.begin(false, now));
        s.configure(0, now).unwrap();
        s.finish(Ok(()), now);
        assert_eq!(s.deadline, None);
        assert!(s.begin(false, now));
        s.finish(Ok(()), now);
        s.configure(45, now).unwrap();
        s.available(false, now);
        assert!(!s.begin(true, now + Duration::from_secs(9000)));
        s.available(true, now);
        assert_eq!(s.deadline, Some(now + Duration::from_secs(2700)));
        assert!(s.configure(1441, now).is_err());
        assert_eq!(s.minutes, 45);
    }

    #[test]
    fn changed_interval_during_release_applies_after_completion_without_catch_up() {
        let now = Instant::now();
        let mut s = Schedule::default();
        s.configure(1, now).unwrap();
        let resumed = now + Duration::from_secs(86400);
        assert!(s.begin(true, resumed));
        s.configure(1440, resumed).unwrap();
        assert_eq!(s.deadline, None);
        assert!(!s.begin(true, resumed));
        s.finish(Ok(()), resumed);
        assert_eq!(s.deadline, Some(resumed + Duration::from_secs(86400)));
        assert!(!s.begin(true, resumed));
    }
}
