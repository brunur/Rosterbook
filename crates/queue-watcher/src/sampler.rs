//! The scheduling/backoff/error-threshold state machine, kept free of any
//! actual screen capture or OCR so it's fully unit-testable.

use alt_ledger_queue::ParsedQueueText;
use std::time::Duration;

/// Consecutive OCR/capture failures before the watcher reports an error.
pub const OCR_FAILURE_THRESHOLD: u32 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cadence {
    Active,
    Idle,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WatcherStatus {
    /// No samples yet, or the last one read cleanly — a queue being
    /// visible or not is not an error.
    Ok,
    /// `OCR_FAILURE_THRESHOLD` capture/OCR calls failed in a row.
    Error(String),
}

/// One capture attempt's outcome, before it becomes a status update.
pub enum Outcome {
    Parsed(ParsedQueueText),
    /// The capture or OCR call itself failed (not just nothing readable).
    Failed(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sampler {
    cadence: Cadence,
    status: WatcherStatus,
    consecutive_failures: u32,
}

impl Default for Sampler {
    fn default() -> Self {
        // Start checking often: if a queue is already up when the watcher
        // starts, catch it within one interval, not up to 30 seconds.
        Self { cadence: Cadence::Active, status: WatcherStatus::Ok, consecutive_failures: 0 }
    }
}

impl Sampler {
    pub fn cadence(&self) -> Cadence {
        self.cadence
    }

    pub fn status(&self) -> &WatcherStatus {
        &self.status
    }

    pub fn interval(&self, active_secs: u64, idle_secs: u64) -> Duration {
        Duration::from_secs(match self.cadence {
            Cadence::Active => active_secs,
            Cadence::Idle => idle_secs,
        })
    }

    /// Feeds one capture attempt's outcome in, updating cadence and status.
    pub fn record(&mut self, outcome: &Outcome) {
        match outcome {
            Outcome::Parsed(parsed) => {
                self.consecutive_failures = 0;
                self.status = WatcherStatus::Ok;
                self.cadence = if parsed.position.is_some() { Cadence::Active } else { Cadence::Idle };
            }
            Outcome::Failed(message) => {
                self.consecutive_failures += 1;
                if self.consecutive_failures >= OCR_FAILURE_THRESHOLD {
                    self.status = WatcherStatus::Error(format!("can't read queue, check capture region ({message})"));
                }
                // Cadence doesn't change on a bare failure: keep trying at
                // the current rate rather than slowing down right when
                // something's gone wrong and a human needs to notice.
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed_with_position(position: Option<u32>) -> Outcome {
        Outcome::Parsed(ParsedQueueText { position, displayed_time_text: None })
    }

    #[test]
    fn starts_active_before_any_sample() {
        let sampler = Sampler::default();
        assert_eq!(sampler.cadence(), Cadence::Active);
        assert_eq!(sampler.status(), &WatcherStatus::Ok);
    }

    #[test]
    fn goes_idle_once_a_sample_reads_no_position() {
        let mut sampler = Sampler::default();
        sampler.record(&parsed_with_position(None));
        assert_eq!(sampler.cadence(), Cadence::Idle);
    }

    #[test]
    fn goes_active_again_once_a_position_is_read() {
        let mut sampler = Sampler::default();
        sampler.record(&parsed_with_position(None));
        sampler.record(&parsed_with_position(Some(500)));
        assert_eq!(sampler.cadence(), Cadence::Active);
    }

    #[test]
    fn error_status_only_after_three_failures_in_a_row() {
        let mut sampler = Sampler::default();
        sampler.record(&Outcome::Failed("boom".into()));
        assert_eq!(sampler.status(), &WatcherStatus::Ok);
        sampler.record(&Outcome::Failed("boom".into()));
        assert_eq!(sampler.status(), &WatcherStatus::Ok);
        sampler.record(&Outcome::Failed("boom".into()));
        assert!(matches!(sampler.status(), WatcherStatus::Error(_)));
    }

    #[test]
    fn a_success_resets_the_failure_streak() {
        let mut sampler = Sampler::default();
        sampler.record(&Outcome::Failed("boom".into()));
        sampler.record(&Outcome::Failed("boom".into()));
        sampler.record(&parsed_with_position(Some(1)));
        sampler.record(&Outcome::Failed("boom".into()));
        sampler.record(&Outcome::Failed("boom".into()));
        assert_eq!(sampler.status(), &WatcherStatus::Ok, "only 2 failures since the last success");
    }

    #[test]
    fn interval_matches_cadence() {
        let mut sampler = Sampler::default();
        assert_eq!(sampler.interval(5, 30).as_secs(), 5);
        sampler.record(&parsed_with_position(None));
        assert_eq!(sampler.interval(5, 30).as_secs(), 30);
    }
}
