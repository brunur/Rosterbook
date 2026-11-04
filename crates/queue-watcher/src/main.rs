//! Entry point: loads config, captures the two regions on a schedule, OCRs
//! and parses them, appends samples, and updates the status file.
//!
//! The desktop app is responsible for starting and stopping this process —
//! there's no internal "paused" state beyond the config's `enabled` flag,
//! which only guards against the process being left running by mistake.

mod capture;
mod ocr;
mod sampler;
mod store;

use alt_ledger_queue::config;
use alt_ledger_queue::QueueSample;
use capture::{ScreenCapturer, XcapCapturer};
use ocr::{OcrEngine, TesseractOcr};
use sampler::{Outcome, Sampler};
use std::time::{SystemTime, UNIX_EPOCH};

fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs() as i64
}

fn main() -> anyhow::Result<()> {
    let data_dir = config::default_dir();
    let cfg = config::WatcherConfig::load(&data_dir.join(config::CONFIG_FILE))?;

    if !cfg.is_ready() {
        eprintln!("alt-ledger-queue-watcher: not configured (enabled + both capture regions required), exiting");
        return Ok(());
    }

    let capturer = XcapCapturer;
    let ocr = TesseractOcr::default();
    let mut sampler = Sampler::default();
    // One session per process run, for now — real session start/end
    // detection (the connecting window appearing/disappearing) is a later
    // step; every sample here is tagged with this placeholder.
    let session_id = format!("session-{}", now());
    let mut sample_counter: u64 = 0;

    loop {
        let outcome = capture_and_parse(&capturer, &ocr, &cfg);
        let ts = now();

        if let Outcome::Parsed(parsed) = &outcome {
            sample_counter += 1;
            let sample = QueueSample::new(format!("{ts}-{sample_counter}"), session_id.clone(), ts, parsed.clone());
            if let Err(e) = store::append_sample(&data_dir, &sample) {
                eprintln!("alt-ledger-queue-watcher: couldn't write sample: {e}");
            }
        }

        sampler.record(&outcome);
        if let Err(e) = store::write_status(&data_dir, ts, sampler.cadence(), sampler.status()) {
            eprintln!("alt-ledger-queue-watcher: couldn't write status: {e}");
        }

        std::thread::sleep(sampler.interval(cfg.active_interval_secs, cfg.idle_interval_secs));
    }
}

fn capture_and_parse(capturer: &impl ScreenCapturer, ocr: &impl OcrEngine, cfg: &config::WatcherConfig) -> Outcome {
    // `cfg.is_ready()` is checked once before the loop starts, so both
    // regions are guaranteed to be set for the lifetime of this process.
    let position_region = cfg.position_region.expect("checked by is_ready()");
    let time_region = cfg.time_region.expect("checked by is_ready()");

    let position_text = match capturer.capture(position_region).and_then(|img| ocr.read_text(&img)) {
        Ok(text) => text,
        Err(e) => return Outcome::Failed(e.to_string()),
    };
    let time_text = match capturer.capture(time_region).and_then(|img| ocr.read_text(&img)) {
        Ok(text) => text,
        Err(e) => return Outcome::Failed(e.to_string()),
    };

    Outcome::Parsed(alt_ledger_queue::parse(&position_text, &time_text))
}

#[cfg(test)]
mod tests {
    use super::*;
    use capture::fakes::FakeCapturer;
    use config::{CaptureRegion, WatcherConfig};
    use ocr::fakes::FakeOcr;
    use xcap::image::RgbaImage;

    fn region() -> CaptureRegion {
        CaptureRegion { monitor_index: 0, x: 0, y: 0, width: 10, height: 10 }
    }

    fn ready_config() -> WatcherConfig {
        WatcherConfig { enabled: true, position_region: Some(region()), time_region: Some(region()), ..WatcherConfig::default() }
    }

    #[test]
    fn parses_position_from_a_successful_capture() {
        let capturer = FakeCapturer { result: Ok(RgbaImage::new(1, 1)) };
        let ocr = FakeOcr { result: Ok("You are #42 in the queue.".to_string()) };
        match capture_and_parse(&capturer, &ocr, &ready_config()) {
            Outcome::Parsed(parsed) => assert_eq!(parsed.position, Some(42)),
            Outcome::Failed(e) => panic!("expected a parsed outcome, got a failure: {e}"),
        }
    }

    #[test]
    fn capture_error_becomes_a_failed_outcome() {
        let capturer = FakeCapturer { result: Err("no monitor".to_string()) };
        let ocr = FakeOcr { result: Ok("unused".to_string()) };
        match capture_and_parse(&capturer, &ocr, &ready_config()) {
            Outcome::Failed(message) => assert!(message.contains("no monitor")),
            Outcome::Parsed(_) => panic!("expected a failed outcome"),
        }
    }

    #[test]
    fn ocr_error_becomes_a_failed_outcome() {
        let capturer = FakeCapturer { result: Ok(RgbaImage::new(1, 1)) };
        let ocr = FakeOcr { result: Err("tesseract not found".to_string()) };
        match capture_and_parse(&capturer, &ocr, &ready_config()) {
            Outcome::Failed(message) => assert!(message.contains("tesseract not found")),
            Outcome::Parsed(_) => panic!("expected a failed outcome"),
        }
    }
}
