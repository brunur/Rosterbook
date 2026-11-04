//! Turns the raw OCR text read from the connecting-window capture into the
//! two fields a queue sample tracks. OCR is noisy — stray punctuation, odd
//! line breaks, misread characters — so this is deliberately permissive
//! about everything except the shape of a plausible number.

/// Parsed OCR text for one capture, before it becomes a `QueueSample`
/// (which also needs an id, a session id and a capture time).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ParsedQueueText {
    pub position: Option<u32>,
    pub displayed_time_text: Option<String>,
}

/// Parses one capture's two OCR regions: the queue position number and
/// Blizzard's own displayed-time text.
pub fn parse(position_raw: &str, time_raw: &str) -> ParsedQueueText {
    ParsedQueueText { position: parse_position(position_raw), displayed_time_text: parse_displayed_time(time_raw) }
}

/// Pulls a queue position out of OCR text such as "You are #1,234 in
/// queue." or "Position: 87". Takes the first run of digits (commas
/// allowed as thousand separators); `None` if the text has no digits at
/// all, or the first digit run doesn't fit a `u32` (OCR garbage).
pub fn parse_position(raw: &str) -> Option<u32> {
    raw.split(|c: char| !(c.is_ascii_digit() || c == ','))
        .map(|group| group.replace(',', ""))
        .find(|group| !group.is_empty())
        .and_then(|group| group.parse().ok())
}

/// The displayed-time text, trimmed and stored verbatim — including joke
/// text like "lifetimes". `None` only when OCR returned nothing usable.
pub fn parse_displayed_time(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_position_from_sentence() {
        assert_eq!(parse_position("You are #1,234 in the queue."), Some(1234));
    }

    #[test]
    fn parses_bare_position() {
        assert_eq!(parse_position("Position: 87"), Some(87));
    }

    #[test]
    fn parses_position_with_leading_zeros_stripped_by_u32() {
        assert_eq!(parse_position("007"), Some(7));
    }

    #[test]
    fn no_digits_is_none() {
        assert_eq!(parse_position("Connecting to World of Warcraft..."), None);
        assert_eq!(parse_position(""), None);
        assert_eq!(parse_position("   "), None);
    }

    #[test]
    fn ocr_garbled_digit_still_found() {
        // A misread leading '1' as 'l' loses that digit, but the rest of
        // the run still parses — a known OCR limitation, not a crash.
        assert_eq!(parse_position("l234 in queue"), Some(234));
    }

    #[test]
    fn position_overflowing_u32_is_none() {
        assert_eq!(parse_position("99999999999999999999 in queue"), None);
    }

    #[test]
    fn displayed_time_keeps_joke_text_verbatim() {
        assert_eq!(parse_displayed_time("  lifetimes  "), Some("lifetimes".to_string()));
        assert_eq!(parse_displayed_time("a few hundred years, give or take"), Some("a few hundred years, give or take".to_string()));
    }

    #[test]
    fn displayed_time_normal_text() {
        assert_eq!(parse_displayed_time("Estimated time: 2 hr 15 min"), Some("Estimated time: 2 hr 15 min".to_string()));
    }

    #[test]
    fn displayed_time_blank_is_none() {
        assert_eq!(parse_displayed_time(""), None);
        assert_eq!(parse_displayed_time("   \n  "), None);
    }

    #[test]
    fn parse_combines_both_fields() {
        let parsed = parse("Position: 4,821", "about 40-60 min");
        assert_eq!(parsed.position, Some(4821));
        assert_eq!(parsed.displayed_time_text.as_deref(), Some("about 40-60 min"));
    }

    #[test]
    fn parse_handles_fully_unreadable_capture() {
        // A bad capture region or a transient OCR failure: no digits, no
        // text. The parser never panics — it just reports nothing found.
        let parsed = parse("####", "");
        assert_eq!(parsed.position, None);
        assert_eq!(parsed.displayed_time_text, None);
    }
}
