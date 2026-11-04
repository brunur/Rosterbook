//! The shapes a queue session is built from. Timestamps are unix seconds
//! (local clock), matching the rest of the app (see `alt-ledger-core`'s
//! `GameData::generated_at`) rather than the handoff doc's ISO8601, since
//! nothing else in this codebase uses string timestamps.

use crate::parser::ParsedQueueText;
use serde::{Deserialize, Serialize};

/// One OCR capture of the connecting window.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueueSample {
    pub id: String,
    /// Capture time, unix seconds.
    pub ts: i64,
    pub position: Option<u32>,
    pub displayed_time_text: Option<String>,
    /// True when `position` parsed; the headline number for trend/ETA math.
    pub parse_ok: bool,
    /// The `QueueSession` this sample belongs to.
    pub session_id: String,
}

impl QueueSample {
    pub fn new(id: impl Into<String>, session_id: impl Into<String>, ts: i64, parsed: ParsedQueueText) -> Self {
        Self {
            id: id.into(),
            session_id: session_id.into(),
            ts,
            parse_ok: parsed.position.is_some(),
            position: parsed.position,
            displayed_time_text: parsed.displayed_time_text,
        }
    }
}

/// Why a queue session stopped. `None` on an active session — set together
/// with `ended_at` when it stops.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EndReason {
    LoggedIn,
    Disconnected,
    Cancelled,
    Unknown,
}

/// One queue attempt: from the connecting window first appearing to it
/// going away (logged in, disconnected, or the player gave up).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueueSession {
    pub id: String,
    /// Unix seconds.
    pub started_at: i64,
    /// Unix seconds; `None` while the session is still active.
    pub ended_at: Option<i64>,
    pub start_position: Option<u32>,
    pub end_reason: Option<EndReason>,
    /// Forever has no realms, so this holds the ruleset name when shown.
    pub realm_or_ruleset: Option<String>,
}

impl QueueSession {
    pub fn start(id: impl Into<String>, started_at: i64, start_position: Option<u32>, realm_or_ruleset: Option<String>) -> Self {
        Self { id: id.into(), started_at, ended_at: None, start_position, end_reason: None, realm_or_ruleset }
    }

    pub fn is_active(&self) -> bool {
        self.ended_at.is_none()
    }

    pub fn end(&mut self, ended_at: i64, reason: EndReason) {
        self.ended_at = Some(ended_at);
        self.end_reason = Some(reason);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sample_parse_ok_follows_position() {
        let parsed = ParsedQueueText { position: Some(42), displayed_time_text: None };
        let sample = QueueSample::new("s1", "sess1", 1000, parsed);
        assert!(sample.parse_ok);
        assert_eq!(sample.position, Some(42));

        let unreadable = ParsedQueueText::default();
        let sample = QueueSample::new("s2", "sess1", 1001, unreadable);
        assert!(!sample.parse_ok);
    }

    #[test]
    fn session_starts_active_and_ends_once() {
        let mut session = QueueSession::start("sess1", 1000, Some(500), None);
        assert!(session.is_active());
        assert_eq!(session.end_reason, None);

        session.end(2000, EndReason::LoggedIn);
        assert!(!session.is_active());
        assert_eq!(session.ended_at, Some(2000));
        assert_eq!(session.end_reason, Some(EndReason::LoggedIn));
    }

    #[test]
    fn sample_serializes_camel_case_for_the_frontend() {
        let parsed = ParsedQueueText { position: Some(42), displayed_time_text: Some("about 40 min".into()) };
        let sample = QueueSample::new("s1", "sess1", 1000, parsed);
        let json = serde_json::to_value(&sample).unwrap();
        assert_eq!(json["displayedTimeText"], "about 40 min");
        assert_eq!(json["parseOk"], true);
        assert_eq!(json["sessionId"], "sess1");
    }

    #[test]
    fn end_reason_serializes_snake_case() {
        assert_eq!(serde_json::to_value(EndReason::LoggedIn).unwrap(), "logged_in");
        assert_eq!(serde_json::to_value(EndReason::Disconnected).unwrap(), "disconnected");
    }
}
